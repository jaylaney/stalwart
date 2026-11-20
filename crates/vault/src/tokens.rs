/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::keys::{
    KEY_LEN, Secret, TOKEN_TAG_LEN, WRAP_LEN, ZaTokenKind, derive_handoff_key,
    derive_token_next_key, derive_token_tag, derive_token_wrap_key, handoff_aad, token_aad,
    token_next_aad, unwrap_key, wrap_key,
};
use encodify::base64::URL_SAFE_NO_PAD;
use std::collections::HashMap;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

pub const TOKEN_RECORD_VERSION: u8 = 1;
pub const MAX_REFRESH_ENTRIES: usize = 2;
pub const MAX_ACCESS_ENTRIES: usize = 4;
pub const MAX_KEYED_GRANTS: usize = 32;
pub const SECRET_SEPARATOR: char = '~';
pub const SECRET_TEXT_LEN: usize = 43;
pub const CODE_LEN: usize = 40;

// The record and entry types hold wraps and do not derive Debug.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, PartialEq, Eq)]
pub struct ZaTokenRecord {
    pub version: u8,
    pub binding: u64,
    pub grants: Vec<ZaTokenGrant>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, PartialEq, Eq)]
pub struct ZaTokenGrant {
    pub grant_id: u64,
    pub refresh: Vec<ZaRefreshEntry>,
    pub access: Vec<ZaAccessEntry>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, PartialEq, Eq)]
pub struct ZaRefreshEntry {
    pub generation: u64,
    pub wrap: Vec<u8>,
    pub next: Option<Vec<u8>>,
    pub expires: u64,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, PartialEq, Eq)]
pub struct ZaAccessEntry {
    pub tag: [u8; TOKEN_TAG_LEN],
    pub wrap: Vec<u8>,
    pub expires: u64,
}

pub enum ZaSuffix {
    None,
    Secret(Secret),
    Malformed,
}

fn decode_secret(text: &str) -> Option<Secret> {
    if text.len() != SECRET_TEXT_LEN {
        return None;
    }
    let (last, prefix) = text.as_bytes().split_last()?;
    // A 32-byte secret leaves two unused bits in the final sextet. Check
    // them and the alphabet before the decoder's error path can use scratch.
    if !b"AEIMQUYcgkosw048".contains(last)
        || !prefix
            .iter()
            .all(|&byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }

    let mut bytes = Zeroizing::new([0; KEY_LEN]);
    match URL_SAFE_NO_PAD.decode_slice(text, bytes.as_mut()) {
        Ok(KEY_LEN) => Some(Secret::from_zeroizing(bytes)),
        _ => None,
    }
}

/// Splits a token bearing the server's header at the first separator.
pub fn split_secret<'x>(token: &'x str, header: &str) -> (&'x str, ZaSuffix) {
    if token.starts_with(header)
        && let Some((upstream, suffix)) = token.split_once(SECRET_SEPARATOR)
    {
        (
            upstream,
            decode_secret(suffix).map_or(ZaSuffix::Malformed, ZaSuffix::Secret),
        )
    } else {
        (token, ZaSuffix::None)
    }
}

pub fn append_secret(token: &mut String, secret: &Secret) {
    token.push(SECRET_SEPARATOR);
    URL_SAFE_NO_PAD.encode_append(secret.as_bytes(), token);
}

/// Splits an authorization code or consent token from its optional secret.
pub fn split_code(code: &str) -> (&str, ZaSuffix) {
    if code.len() == CODE_LEN {
        return (code, ZaSuffix::None);
    }
    if let Some((upstream, suffix)) = code.split_at_checked(CODE_LEN)
        && let Some(secret) = decode_secret(suffix)
    {
        return (upstream, ZaSuffix::Secret(secret));
    }
    (code, ZaSuffix::Malformed)
}

pub fn append_code_secret(code: &mut String, secret: &Secret) {
    URL_SAFE_NO_PAD.encode_append(secret.as_bytes(), code);
}

/// A registry read for reconciliation or eviction.
pub struct ZaGrantState {
    pub account_id: u32,
    pub generation: u64,
    pub last_used_at: i64,
}

pub enum ZaRefreshClass {
    /// The presented entry opens, along with its sealed next secret when present.
    Opened {
        mk: Secret,
        next: Option<Secret>,
    },
    /// The presented entry is gone and a higher generation exists.
    Superseded,
    Lost,
}

impl ZaTokenRecord {
    pub fn new(binding: u64) -> Self {
        Self {
            version: TOKEN_RECORD_VERSION,
            binding,
            grants: Vec::new(),
        }
    }

    pub fn grant(&self, grant_id: u64) -> Option<&ZaTokenGrant> {
        self.grants.iter().find(|grant| grant.grant_id == grant_id)
    }

    fn grant_or_insert(&mut self, grant_id: u64) -> &mut ZaTokenGrant {
        if let Some(index) = self
            .grants
            .iter()
            .position(|grant| grant.grant_id == grant_id)
        {
            return &mut self.grants[index];
        }
        self.grants.push(ZaTokenGrant {
            grant_id,
            refresh: Vec::new(),
            access: Vec::new(),
        });
        self.grants.last_mut().unwrap()
    }

    /// Removes expired entries, obsolete refresh generations, and grants
    /// with no live entry or no registry state for this account.
    pub fn reconcile(
        &mut self,
        account_id: u32,
        now: u64,
        grants: &HashMap<u64, Option<ZaGrantState>>,
    ) -> bool {
        let before = self.grants.len();
        let mut entries_removed = false;
        self.grants.retain_mut(|grant| {
            let Some(Some(state)) = grants.get(&grant.grant_id) else {
                return false;
            };
            if state.account_id != account_id {
                return false;
            }

            let refresh_count = grant.refresh.len();
            let access_count = grant.access.len();
            grant
                .refresh
                .retain(|entry| entry.generation >= state.generation && entry.expires > now);
            grant.access.retain(|entry| entry.expires > now);
            entries_removed |=
                grant.refresh.len() != refresh_count || grant.access.len() != access_count;
            !(grant.refresh.is_empty() && grant.access.is_empty())
        });
        entries_removed || self.grants.len() != before
    }

    /// Returns the grant to remove before adding a new grant at the cap.
    pub fn eviction(
        &self,
        adding: u64,
        grants: &HashMap<u64, Option<ZaGrantState>>,
    ) -> Option<u64> {
        if self.grants.len() < MAX_KEYED_GRANTS || self.grant(adding).is_some() {
            return None;
        }
        self.grants
            .iter()
            .map(|grant| grant.grant_id)
            .min_by_key(|id| {
                let last_used_at = grants
                    .get(id)
                    .and_then(Option::as_ref)
                    .map(|state| state.last_used_at);
                (last_used_at, *id)
            })
    }

    pub fn remove_grant(&mut self, grant_id: u64) -> bool {
        let before = self.grants.len();
        self.grants.retain(|grant| grant.grant_id != grant_id);
        self.grants.len() != before
    }

    pub fn open_access(
        &self,
        account_id: u32,
        grant_id: u64,
        secret: &Secret,
        now: u64,
    ) -> Option<Secret> {
        let grant = self.grant(grant_id)?;
        let tag = derive_token_tag(secret);
        let entry = grant
            .access
            .iter()
            .find(|entry| entry.expires > now && bool::from(entry.tag.ct_eq(&tag)))?;
        unwrap_key(
            &entry.wrap,
            &derive_token_wrap_key(secret),
            &token_aad(account_id, grant_id, ZaTokenKind::Access),
        )
        .ok()
    }

    pub fn classify_refresh(
        &self,
        account_id: u32,
        grant_id: u64,
        presented: u64,
        secret: &Secret,
        now: u64,
    ) -> ZaRefreshClass {
        let Some(grant) = self.grant(grant_id) else {
            return ZaRefreshClass::Lost;
        };
        let live = || grant.refresh.iter().filter(|entry| entry.expires > now);
        let Some(entry) = live().find(|entry| entry.generation == presented) else {
            return if live().any(|entry| entry.generation > presented) {
                ZaRefreshClass::Superseded
            } else {
                ZaRefreshClass::Lost
            };
        };
        let Ok(mk) = unwrap_key(
            &entry.wrap,
            &derive_token_wrap_key(secret),
            &token_aad(
                account_id,
                grant_id,
                ZaTokenKind::Refresh {
                    generation: presented,
                },
            ),
        ) else {
            return ZaRefreshClass::Lost;
        };
        let next = entry
            .next
            .as_ref()
            .map(|wrap| {
                unwrap_key(
                    wrap,
                    &derive_token_next_key(secret),
                    &token_next_aad(account_id, grant_id, presented),
                )
            })
            .transpose();
        match next {
            Ok(next) => ZaRefreshClass::Opened { mk, next },
            Err(_) => ZaRefreshClass::Lost,
        }
    }

    /// Adds a fresh access entry and returns its secret. At the cap it first
    /// drops the earliest-expiring of the entries stored before this call
    /// (the first on a tie), so the entry it adds is never the one dropped,
    /// even when it expires before every stored one.
    pub fn put_access(
        &mut self,
        account_id: u32,
        grant_id: u64,
        mk: &Secret,
        expires: u64,
    ) -> Secret {
        let secret = Secret::random();
        let grant = self.grant_or_insert(grant_id);
        if grant.access.len() >= MAX_ACCESS_ENTRIES {
            // min_by_key returns the first entry on a tie.
            let (oldest, _) = grant
                .access
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.expires)
                .unwrap();
            grant.access.remove(oldest);
        }
        grant.access.push(ZaAccessEntry {
            tag: derive_token_tag(&secret),
            wrap: wrap_key(
                mk,
                &derive_token_wrap_key(&secret),
                &token_aad(account_id, grant_id, ZaTokenKind::Access),
            ),
            expires,
        });
        secret
    }

    /// Adds the requested refresh generation if it is missing. Seals its
    /// secret into the presented entry if that entry has no sealed next secret.
    pub fn put_refresh(
        &mut self,
        account_id: u32,
        grant_id: u64,
        mk: &Secret,
        presented: Option<(u64, &Secret)>,
        next: u64,
        next_secret: &Secret,
        expires: u64,
    ) {
        let grant = self.grant_or_insert(grant_id);
        if let Err(index) = grant
            .refresh
            .binary_search_by_key(&next, |entry| entry.generation)
        {
            grant.refresh.insert(
                index,
                ZaRefreshEntry {
                    generation: next,
                    wrap: wrap_key(
                        mk,
                        &derive_token_wrap_key(next_secret),
                        &token_aad(
                            account_id,
                            grant_id,
                            ZaTokenKind::Refresh { generation: next },
                        ),
                    ),
                    next: None,
                    expires,
                },
            );
        }
        if let Some((generation, secret)) = presented
            && let Some(entry) = grant
                .refresh
                .iter_mut()
                .find(|entry| entry.generation == generation)
            && entry.next.is_none()
        {
            entry.next = Some(wrap_key(
                next_secret,
                &derive_token_next_key(secret),
                &token_next_aad(account_id, grant_id, generation),
            ));
        }
        if grant.refresh.len() > MAX_REFRESH_ENTRIES {
            grant
                .refresh
                .drain(..grant.refresh.len() - MAX_REFRESH_ENTRIES);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.grants.is_empty()
    }
}

pub const HANDOFF_VERSION: u8 = 1;
const HANDOFF_LEN: usize = 1 + 4 + 8 + WRAP_LEN;

/// Hand-off value: version, big-endian account id and binding, then wrapped MK.
pub fn seal_handoff(mk: &Secret, account_id: u32, binding: u64, key_id: &str) -> (Secret, Vec<u8>) {
    let secret = Secret::random();
    let wrap = wrap_key(
        mk,
        &derive_handoff_key(&secret),
        &handoff_aad(account_id, binding, key_id),
    );
    let mut value = Vec::with_capacity(HANDOFF_LEN);
    value.push(HANDOFF_VERSION);
    value.extend_from_slice(&account_id.to_be_bytes());
    value.extend_from_slice(&binding.to_be_bytes());
    value.extend_from_slice(&wrap);
    (secret, value)
}

pub fn open_handoff(value: &[u8], secret: &Secret, key_id: &str) -> Option<(u32, u64, Secret)> {
    if value.len() != HANDOFF_LEN || value[0] != HANDOFF_VERSION {
        return None;
    }
    let account_id = u32::from_be_bytes(value[1..5].try_into().ok()?);
    let binding = u64::from_be_bytes(value[5..13].try_into().ok()?);
    let mk = unwrap_key(
        &value[13..],
        &derive_handoff_key(secret),
        &handoff_aad(account_id, binding, key_id),
    )
    .ok()?;
    Some((account_id, binding, mk))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_entry_binds_account_grant_kind_generation() {
        let mk = Secret::random();
        let refresh = Secret::random();
        let mut record = ZaTokenRecord::new(3);
        record.put_refresh(7, 23, &mk, None, 0, &refresh, 1_000);
        let access = record.put_access(7, 23, &mk, 1_000);

        assert!(record.open_access(7, 23, &access, 10).unwrap().as_bytes() == mk.as_bytes());
        assert!(matches!(
            record.classify_refresh(7, 23, 0, &refresh, 10),
            ZaRefreshClass::Opened { next: None, .. }
        ));
        assert!(record.open_access(8, 23, &access, 10).is_none());
        assert!(matches!(
            record.classify_refresh(8, 23, 0, &refresh, 10),
            ZaRefreshClass::Lost
        ));

        let mut other_grant = record.clone();
        other_grant.grants[0].grant_id = 24;
        assert!(other_grant.open_access(7, 24, &access, 10).is_none());
        assert!(matches!(
            other_grant.classify_refresh(7, 24, 0, &refresh, 10),
            ZaRefreshClass::Lost
        ));

        let mut other_kind = record.clone();
        let grant = &mut other_kind.grants[0];
        grant.access[0].wrap = grant.refresh[0].wrap.clone();
        grant.access[0].tag = derive_token_tag(&refresh);
        assert!(other_kind.open_access(7, 23, &refresh, 10).is_none());

        record.grants[0].refresh[0].generation = 1;
        assert!(matches!(
            record.classify_refresh(7, 23, 1, &refresh, 10),
            ZaRefreshClass::Lost
        ));
    }

    #[test]
    fn split_secret_cases() {
        let upstream = "sw1.AAAA.BBBB";
        let secret = Secret::from_bytes([0xA5; 32]);
        let mut token = upstream.to_string();
        append_secret(&mut token, &secret);
        assert_eq!(token.len(), 13 + 1 + SECRET_TEXT_LEN);
        assert_eq!(token.as_bytes()[13], b'~');
        let suffix = &token[14..];
        assert!(
            suffix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        );
        assert!(
            matches!(split_secret(upstream, "sw1."), (part, ZaSuffix::None) if part == upstream)
        );
        assert!(matches!(
            split_secret(&token, "sw1."),
            (part, ZaSuffix::Secret(opened)) if part == upstream && opened.as_bytes() == secret.as_bytes()
        ));

        let shortened = &token[..token.len() - 1];
        for malformed in [
            format!("{upstream}~"),
            shortened.to_string(),
            format!("{token}A"),
            format!("{shortened}="),
            format!("{shortened}+"),
            format!("{upstream}~+{}", &suffix[1..]),
            format!("{upstream}~é{}", &suffix[2..]),
            format!("{upstream}~~{suffix}"),
            format!("{token}~{suffix}"),
            format!("{shortened}_"),
        ] {
            assert!(matches!(
                split_secret(&malformed, "sw1."),
                (part, ZaSuffix::Malformed) if part == upstream
            ));
        }

        for opaque in ["opaque~value".to_string(), format!("opaque~{suffix}")] {
            assert!(matches!(
                split_secret(&opaque, "sw1."),
                (part, ZaSuffix::None) if part == opaque
            ));
        }
    }

    #[test]
    fn split_secret_accepts_each_canonical_final_sextet() {
        let upstream = "sw1.AAAA.BBBB";
        let mut bytes = [0xFB; 32];
        // The last four bits of a 32-byte value reach the final sextet.
        for low_nibble in 0..16 {
            bytes[31] = 0xF0 | low_nibble;
            let secret = Secret::from_bytes(bytes);
            let mut token = upstream.to_string();
            append_secret(&mut token, &secret);
            assert!(matches!(
                split_secret(&token, "sw1."),
                (part, ZaSuffix::Secret(opened)) if part == upstream && opened.as_bytes() == secret.as_bytes()
            ));
        }
    }

    #[test]
    fn split_code_cases() {
        let code = "a".repeat(40);
        let secret = Secret::from_bytes([0x5A; 32]);
        let mut with = code.clone();
        append_code_secret(&mut with, &secret);
        assert_eq!(with.len(), CODE_LEN + SECRET_TEXT_LEN);
        assert!(!with.contains('~'));
        assert!(matches!(split_code(&code), (part, ZaSuffix::None) if part == code));
        assert!(matches!(
            split_code(&with),
            (part, ZaSuffix::Secret(opened)) if part == code && opened.as_bytes() == secret.as_bytes()
        ));

        for malformed in [
            code[..39].to_string(),
            with[..82].to_string(),
            format!("{with}A"),
            format!("{code}~{}", &with[40..]),
            format!("{}=", &with[..82]),
            format!("{}é{}", &code[..39], &with[41..]),
            String::new(),
        ] {
            assert!(matches!(
                split_code(&malformed),
                (part, ZaSuffix::Malformed) if part == malformed
            ));
        }
    }

    #[test]
    fn record_round_trips_and_caps() {
        let mk = Secret::random();
        let s0 = Secret::random();
        let s1 = Secret::random();
        let s2 = Secret::random();
        let mut record = ZaTokenRecord::new(5);
        assert!(record.is_empty());
        assert_eq!(record.version, TOKEN_RECORD_VERSION);
        assert_eq!(record.binding, 5);

        record.put_refresh(7, 23, &mk, None, 0, &s0, 100);
        record.put_refresh(7, 23, &mk, Some((0, &s0)), 1, &s1, 200);
        record.put_refresh(7, 23, &mk, Some((1, &s1)), 2, &s2, 300);
        let secrets = [50, 10, 40, 30, 20].map(|expires| record.put_access(7, 23, &mk, expires));
        let grant = record.grant(23).unwrap();
        assert_eq!(
            grant
                .refresh
                .iter()
                .map(|entry| entry.generation)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(
            grant
                .access
                .iter()
                .map(|entry| entry.expires)
                .collect::<Vec<_>>(),
            [50, 40, 30, 20]
        );
        for (index, secret) in secrets.iter().enumerate() {
            let opened = record.open_access(7, 23, secret, 0);
            if index == 1 {
                assert!(opened.is_none());
            } else {
                assert!(opened.unwrap().as_bytes() == mk.as_bytes());
            }
        }

        assert!(grant.refresh.iter().all(|entry| entry.wrap.len() == 72));
        assert!(grant.access.iter().all(|entry| entry.wrap.len() == 72));
        assert_eq!(grant.refresh[0].next.as_ref().unwrap().len(), 72);

        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&record).unwrap();
        let decoded = rkyv::from_bytes::<ZaTokenRecord, rkyv::rancor::Error>(&bytes).unwrap();
        assert!(decoded == record);
        assert!(record.remove_grant(23));
        assert!(!record.remove_grant(23));
        assert!(record.is_empty());

        for id in 1..=32 {
            record.put_refresh(7, id, &mk, None, 0, &s0, 100);
            record.put_refresh(7, id, &mk, Some((0, &s0)), 1, &s1, 200);
            for expires in [10, 20, 30, 40] {
                record.put_access(7, id, &mk, expires);
            }
        }
        assert!(
            rkyv::to_bytes::<rkyv::rancor::Error>(&record)
                .unwrap()
                .len()
                < 50_000
        );
    }

    #[test]
    fn put_access_keeps_the_entry_it_adds() {
        let mk = Secret::random();
        let opens = |record: &ZaTokenRecord, secret: &Secret| {
            record
                .open_access(7, 23, secret, 0)
                .is_some_and(|opened| opened.as_bytes() == mk.as_bytes())
        };
        let expiries = |record: &ZaTokenRecord| {
            record
                .grant(23)
                .unwrap()
                .access
                .iter()
                .map(|entry| entry.expires)
                .collect::<Vec<_>>()
        };

        // The fifth entry expires before all four stored ones (a lowered
        // access-token lifetime, or another node's clock): it stays, and the
        // earliest-expiring stored entry goes.
        let mut record = ZaTokenRecord::new(3);
        let stored = [40, 20, 30, 50].map(|expires| record.put_access(7, 23, &mk, expires));
        let added = record.put_access(7, 23, &mk, 10);
        assert!(
            opens(&record, &added),
            "the access entry just added does not open"
        );
        assert_eq!(expiries(&record), [40, 30, 50, 10]);
        for (index, secret) in stored.iter().enumerate() {
            assert!(
                opens(&record, secret) == (index != 1),
                "an entry other than the earliest-expiring stored one was removed"
            );
        }

        // A fifth entry with a later expiry evicts the earliest stored one.
        let mut record = ZaTokenRecord::new(3);
        let stored = [40, 20, 30, 50].map(|expires| record.put_access(7, 23, &mk, expires));
        let added = record.put_access(7, 23, &mk, 60);
        assert!(
            opens(&record, &added),
            "the access entry just added does not open"
        );
        assert_eq!(expiries(&record), [40, 30, 50, 60]);
        for (index, secret) in stored.iter().enumerate() {
            assert!(
                opens(&record, secret) == (index != 1),
                "an entry other than the earliest-expiring stored one was removed"
            );
        }

        // A tie among the stored entries evicts the first of them.
        let mut record = ZaTokenRecord::new(3);
        let stored = [30, 10, 10, 40].map(|expires| record.put_access(7, 23, &mk, expires));
        let added = record.put_access(7, 23, &mk, 50);
        assert!(
            opens(&record, &added),
            "the access entry just added does not open"
        );
        assert_eq!(expiries(&record), [30, 10, 40, 50]);
        for (index, secret) in stored.iter().enumerate() {
            assert!(
                opens(&record, secret) == (index != 1),
                "an entry other than the first of the tied stored ones was removed"
            );
        }
    }

    #[test]
    fn reconcile_cases() {
        let mk = Secret::random();
        let s0 = Secret::random();
        let s1 = Secret::random();
        let mut record = ZaTokenRecord::new(3);
        for id in 1..=6 {
            let expires = if id == 5 { 500 } else { 1_000 };
            record.put_refresh(7, id, &mk, None, 0, &s0, expires);
            record.put_access(7, id, &mk, expires);
        }
        record.put_refresh(7, 4, &mk, Some((0, &s0)), 1, &s1, 1_000);
        let unchanged = record.grant(1).unwrap().clone();
        let state = |account_id, generation| {
            Some(ZaGrantState {
                account_id,
                generation,
                last_used_at: 0,
            })
        };
        let grants = HashMap::from([
            (1, state(7, 0)),
            (2, None),
            (3, state(8, 0)),
            (4, state(7, 1)),
            (5, state(7, 0)),
        ]);

        assert!(record.reconcile(7, 500, &grants));
        assert_eq!(
            record
                .grants
                .iter()
                .map(|grant| grant.grant_id)
                .collect::<Vec<_>>(),
            [1, 4]
        );
        assert!(*record.grant(1).unwrap() == unchanged);
        assert_eq!(
            record
                .grant(4)
                .unwrap()
                .refresh
                .iter()
                .map(|entry| entry.generation)
                .collect::<Vec<_>>(),
            [1]
        );
        assert!(!record.reconcile(7, 500, &grants));
    }

    #[test]
    fn classify_refresh_cases() {
        let mk = Secret::random();
        let s0 = Secret::random();
        let s1 = Secret::random();
        let mut record = ZaTokenRecord::new(3);
        assert!(matches!(
            record.classify_refresh(7, 23, 0, &s0, 10),
            ZaRefreshClass::Lost
        ));
        record.put_refresh(7, 23, &mk, None, 0, &s0, 100);
        assert!(matches!(
            record.classify_refresh(7, 23, 0, &s0, 10),
            ZaRefreshClass::Opened { mk: opened, next: None } if opened.as_bytes() == mk.as_bytes()
        ));

        record.put_refresh(7, 23, &mk, Some((0, &s0)), 1, &s1, 100);
        assert!(matches!(
            record.classify_refresh(7, 23, 0, &s0, 10),
            ZaRefreshClass::Opened { mk: opened, next: Some(next) }
                if opened.as_bytes() == mk.as_bytes() && next.as_bytes() == s1.as_bytes()
        ));

        let mut advanced = record.clone();
        advanced.grants[0].refresh.remove(0);
        assert!(matches!(
            advanced.classify_refresh(7, 23, 0, &s0, 10),
            ZaRefreshClass::Superseded
        ));
        assert!(matches!(
            advanced.classify_refresh(7, 23, 2, &s1, 10),
            ZaRefreshClass::Lost
        ));
        assert!(matches!(
            record.classify_refresh(7, 23, 0, &Secret::random(), 10),
            ZaRefreshClass::Lost
        ));
        assert!(matches!(
            record.classify_refresh(7, 23, 1, &s1, 100),
            ZaRefreshClass::Lost
        ));
        record.grants[0].refresh[0].next = Some(vec![0; 72]);
        assert!(matches!(
            record.classify_refresh(7, 23, 0, &s0, 10),
            ZaRefreshClass::Lost
        ));
    }

    #[test]
    fn put_refresh_restores_missing_next() {
        let mk = Secret::random();
        let s0 = Secret::random();
        let s1 = Secret::random();
        let mut record = ZaTokenRecord::new(3);
        record.put_refresh(7, 23, &mk, None, 0, &s0, 100);
        record.put_refresh(7, 23, &mk, Some((0, &s0)), 1, &s1, 200);
        let sealed_next = record.grants[0].refresh[0].next.clone().unwrap();
        record.grants[0].refresh.remove(1);

        record.put_refresh(7, 23, &mk, Some((0, &s0)), 1, &s1, 300);
        assert!(record.grants[0].refresh[0].next.as_ref() == Some(&sealed_next));
        assert!(matches!(
            record.classify_refresh(7, 23, 1, &s1, 10),
            ZaRefreshClass::Opened { next: None, .. }
        ));
        let restored = record.clone();
        record.put_refresh(7, 23, &mk, Some((0, &s0)), 1, &s1, 999);
        assert!(record == restored);
    }

    #[test]
    fn eviction_picks_least_recently_used() {
        let mk = Secret::random();
        let mut record = ZaTokenRecord::new(3);
        let mut grants = HashMap::new();
        for id in 1..=31 {
            record.put_access(7, id, &mk, 100);
            grants.insert(
                id,
                Some(ZaGrantState {
                    account_id: 7,
                    generation: 0,
                    last_used_at: 1_000 + id as i64,
                }),
            );
        }
        assert_eq!(record.eviction(99, &grants), None);
        record.put_access(7, 32, &mk, 100);
        grants.insert(
            32,
            Some(ZaGrantState {
                account_id: 7,
                generation: 0,
                last_used_at: 5,
            }),
        );
        assert_eq!(record.eviction(32, &grants), None);
        assert_eq!(record.eviction(99, &grants), Some(32));
        grants.get_mut(&9).unwrap().as_mut().unwrap().last_used_at = 5;
        assert_eq!(record.eviction(99, &grants), Some(9));
        grants.get_mut(&9).unwrap().as_mut().unwrap().last_used_at = i64::MIN;
        grants.remove(&15);
        assert_eq!(record.eviction(99, &grants), Some(15));
        assert_eq!(MAX_KEYED_GRANTS, 32);
    }

    #[test]
    fn handoff_opens_only_as_sealed() {
        let mk = Secret::random();
        let key = "c".repeat(40);
        let (secret, value) = seal_handoff(&mk, 7, 12, &key);
        assert_eq!(value.len(), 85);
        assert_eq!(value[0], HANDOFF_VERSION);
        assert_eq!(value[1..5], 7u32.to_be_bytes());
        assert_eq!(value[5..13], 12u64.to_be_bytes());
        let (account, binding, opened) = open_handoff(&value, &secret, &key).unwrap();
        assert_eq!((account, binding), (7, 12));
        assert!(opened.as_bytes() == mk.as_bytes());

        assert!(open_handoff(&value, &Secret::random(), &key).is_none());
        assert!(open_handoff(&value, &secret, &"d".repeat(40)).is_none());
        assert!(open_handoff(&value[..84], &secret, &key).is_none());
        let mut wrong_account = value.clone();
        wrong_account[4] ^= 1;
        assert!(open_handoff(&wrong_account, &secret, &key).is_none());
        let mut wrong_binding = value.clone();
        wrong_binding[12] ^= 1;
        assert!(open_handoff(&wrong_binding, &secret, &key).is_none());
        let mut wrong_version = value;
        wrong_version[0] = 2;
        assert!(open_handoff(&wrong_version, &secret, &key).is_none());
    }
}
