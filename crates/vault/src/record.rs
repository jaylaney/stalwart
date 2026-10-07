/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

pub const VAULT_RECORD_VERSION: u8 = 1;
/// Pending app-password wraps older than this are pruned (spec 4.1).
pub const PENDING_WRAP_MAX_AGE_SECS: i64 = 3600;

#[derive(rkyv::Archive, rkyv::Deserialize, rkyv::Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[rkyv(compare(PartialEq), derive(Debug))]
pub enum VaultState {
    PendingSetup,
    Active,
}

#[derive(rkyv::Archive, rkyv::Deserialize, rkyv::Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[rkyv(compare(PartialEq), derive(Debug))]
pub enum WrapState {
    Pending,
    Published,
}

/// No `Debug`: `wrap` is key material.
#[derive(rkyv::Archive, rkyv::Deserialize, rkyv::Serialize, Clone, PartialEq, Eq)]
pub struct AppWrap {
    pub credential_id: u32,
    /// MK wrapped under HKDF(secret, "za/v1/akek/<credential id>"); 72 bytes.
    pub wrap: Vec<u8>,
    pub state: WrapState,
    /// Unix seconds.
    pub created: i64,
    /// Random id that identifies this exact Pending entry during publication.
    pub publication_id: u64,
}

/// The sole authority for a key account's login (spec 3.1). One per account,
/// stored as `PrincipalField::ZeroAccessVault`. Byte strings are `Vec<u8>` so
/// the rkyv layout stays simple: salt 16, verifier_hash 32, wraps 72, keys 32.
///
/// No derived `Debug`: it holds the TOTP URL (which contains the secret) and
/// the wraps.
#[derive(rkyv::Archive, rkyv::Deserialize, rkyv::Serialize, Clone, PartialEq, Eq)]
pub struct VaultRecord {
    pub version: u8,
    /// Authentication generation. Incremented by every write.
    pub revision: u64,
    pub state: VaultState,
    pub salt: Vec<u8>,
    pub argon2_m_cost_kib: u32,
    pub argon2_t_cost: u32,
    pub argon2_p_cost: u32,
    pub verifier_hash: Vec<u8>,
    /// otpauth:// URL, in the clear like upstream's registry field.
    pub totp_url: Option<String>,
    pub password_wrap: Vec<u8>,
    pub recovery_wrap: Vec<u8>,
    pub app_wraps: Vec<AppWrap>,
    pub public_key: Vec<u8>,
    pub private_key_wrap: Vec<u8>,
    /// SHA-256 of the setup token while PendingSetup; empty otherwise.
    pub setup_token_hash: Vec<u8>,
    /// Unix seconds; 0 when not pending.
    pub setup_token_expires: i64,
}

impl std::fmt::Debug for VaultRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultRecord")
            .field("version", &self.version)
            .field("state", &self.state)
            .field("revision", &self.revision)
            .field("app_wraps", &format_args!("{}", self.app_wraps.len()))
            .field("totp", &self.totp_url.is_some())
            .finish_non_exhaustive()
    }
}

impl VaultRecord {
    pub fn pending(setup_token_hash: Vec<u8>, expires: i64) -> Self {
        VaultRecord {
            version: VAULT_RECORD_VERSION,
            revision: 1,
            state: VaultState::PendingSetup,
            salt: Vec::new(),
            argon2_m_cost_kib: 0,
            argon2_t_cost: 0,
            argon2_p_cost: 0,
            verifier_hash: Vec::new(),
            totp_url: None,
            password_wrap: Vec::new(),
            recovery_wrap: Vec::new(),
            app_wraps: Vec::new(),
            public_key: Vec::new(),
            private_key_wrap: Vec::new(),
            setup_token_hash,
            setup_token_expires: expires,
        }
    }

    pub fn argon2_params(&self) -> crate::keys::Argon2Params {
        crate::keys::Argon2Params {
            m_cost_kib: self.argon2_m_cost_kib,
            t_cost: self.argon2_t_cost,
            p_cost: self.argon2_p_cost,
        }
    }

    pub fn set_argon2_params(&mut self, params: crate::keys::Argon2Params) {
        self.argon2_m_cost_kib = params.m_cost_kib;
        self.argon2_t_cost = params.t_cost;
        self.argon2_p_cost = params.p_cost;
    }

    pub fn app_wrap(&self, credential_id: u32) -> Option<&AppWrap> {
        self.app_wraps
            .iter()
            .find(|w| w.credential_id == credential_id)
    }

    pub fn app_wrap_mut(&mut self, credential_id: u32) -> Option<&mut AppWrap> {
        self.app_wraps
            .iter_mut()
            .find(|w| w.credential_id == credential_id)
    }

    /// Orphan pruning (spec 4.1): removes Published wraps whose registry
    /// credential is gone and Pending wraps older than one hour. Returns the
    /// number of wraps removed. Never touches a fresh Pending wrap.
    pub fn prune_orphans(&mut self, registry_credential_ids: &[u32], now: i64) -> usize {
        let before = self.app_wraps.len();
        self.app_wraps.retain(|w| match w.state {
            WrapState::Published => registry_credential_ids.contains(&w.credential_id),
            WrapState::Pending => now.saturating_sub(w.created) < PENDING_WRAP_MAX_AGE_SECS,
        });
        before - self.app_wraps.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_record() -> VaultRecord {
        VaultRecord {
            version: VAULT_RECORD_VERSION,
            revision: 7,
            state: VaultState::Active,
            salt: vec![1; 16],
            argon2_m_cost_kib: 65536,
            argon2_t_cost: 3,
            argon2_p_cost: 1,
            verifier_hash: vec![2; 32],
            totp_url: Some("otpauth://totp/x?secret=ABC".into()),
            password_wrap: vec![3; 72],
            recovery_wrap: vec![4; 72],
            app_wraps: vec![AppWrap {
                credential_id: 9,
                wrap: vec![5; 72],
                state: WrapState::Pending,
                created: 1_700_000_000,
                publication_id: 42,
            }],
            public_key: vec![6; 32],
            private_key_wrap: vec![7; 72],
            setup_token_hash: vec![],
            setup_token_expires: 0,
        }
    }

    #[test]
    fn record_round_trips_through_rkyv() {
        let record = full_record();
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&record).unwrap();
        let back = rkyv::from_bytes::<VaultRecord, rkyv::rancor::Error>(&bytes).unwrap();
        assert_eq!(back, record);
    }

    #[test]
    fn debug_prints_no_secret_material() {
        let record = full_record();
        let text = format!("{record:?}");
        assert!(!text.contains("otpauth"), "{text}");
        assert!(!text.contains("ABC"), "{text}");
        assert!(!text.contains("secret"), "{text}");
        // Wrap bytes would show up as runs of 5, 3, 4 or 7.
        for needle in ["5, 5", "3, 3", "4, 4", "7, 7"] {
            assert!(!text.contains(needle), "{needle} in {text}");
        }
        assert!(text.contains("app_wraps: 1"), "{text}");
        assert!(text.contains("totp: true"), "{text}");
    }

    #[test]
    fn prune_orphans_removes_only_dead_wraps() {
        let now = 10_000;
        let mut record = VaultRecord::pending(vec![0; 32], now + 100);
        let wrap = |credential_id, state, created| AppWrap {
            credential_id,
            wrap: vec![],
            state,
            created,
            publication_id: credential_id as u64,
        };
        record.app_wraps = vec![
            wrap(1, WrapState::Published, 0),
            wrap(2, WrapState::Published, 0),
            wrap(3, WrapState::Pending, now - 3601),
            wrap(4, WrapState::Pending, now - 10),
        ];
        // Registry still knows credentials 1 and 4 only.
        let removed = record.prune_orphans(&[1, 4], now);
        assert_eq!(removed, 2);
        let ids: Vec<u32> = record.app_wraps.iter().map(|w| w.credential_id).collect();
        assert_eq!(
            ids,
            vec![1, 4],
            "published-without-registry (2) and stale pending (3) are pruned; fresh pending (4) survives"
        );
    }
}
