/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Request-local key domains. Only resolver/conversion code constructs these;
//! downstream codecs receive opaque domains, never another principal's session.

use super::{
    policy::POLICY_VERSION,
    tree::{SealError, WRAP_MK, open_key_envelope, seal_key_envelope},
};
use crate::calendar::sharing::format::WRAP_CK;
use std::collections::BTreeMap;
use vault::{
    keys::{Secret, WRAP_LEN, aad, unwrap_key, wrap_key},
    session::SessionKeys,
};

pub struct ZaKeyDomain {
    owner_id: u32,
    principal_id: u32,
    ewk: Option<Secret>,
    shared: Option<SharedDomain>,
}

struct SharedDomain {
    calendar_id: u32,
    domain_id: [u8; 16],
    keys: BTreeMap<u32, Secret>,
    reverting: bool,
}

impl ZaKeyDomain {
    pub(crate) fn owner(keys: &SessionKeys) -> Self {
        Self {
            owner_id: keys.account_id,
            principal_id: keys.account_id,
            ewk: Some(keys.ewk().clone()),
            shared: None,
        }
    }

    pub(crate) fn shared(
        owner_id: u32,
        principal_id: u32,
        ewk: Option<Secret>,
        calendar_id: u32,
        domain_id: [u8; 16],
        keys: BTreeMap<u32, Secret>,
        reverting: bool,
    ) -> Result<Self, SealError> {
        if keys.is_empty()
            || keys.first_key_value().is_some_and(|(&g, _)| g == 0)
            || keys.len() > u16::MAX as usize
            || (ewk.is_some() != (principal_id == owner_id))
            || (reverting && ewk.is_none())
        {
            return Err(SealError::Structure("collection key generations"));
        }
        Ok(Self {
            owner_id,
            principal_id,
            ewk,
            shared: Some(SharedDomain {
                calendar_id,
                domain_id,
                keys,
                reverting,
            }),
        })
    }

    pub fn owner_id(&self) -> u32 {
        self.owner_id
    }
    pub fn principal_id(&self) -> u32 {
        self.principal_id
    }
    pub fn calendar_id(&self) -> Option<u32> {
        self.shared.as_ref().map(|s| s.calendar_id)
    }
    pub fn domain_id(&self) -> Option<&[u8; 16]> {
        self.shared.as_ref().map(|s| &s.domain_id)
    }
    pub fn is_reverting(&self) -> bool {
        self.shared.as_ref().is_some_and(|s| s.reverting)
    }
    pub(super) fn matches_generations(&self, generations: &[u32]) -> bool {
        self.shared
            .as_ref()
            .is_some_and(|s| s.keys.keys().copied().eq(generations.iter().copied()))
    }
    pub(super) fn is_owner(&self) -> bool {
        self.ewk.is_some()
    }
    pub(crate) fn ewk(&self) -> Result<&Secret, SealError> {
        self.ewk.as_ref().ok_or(SealError::NotSealed)
    }

    /// Prefix shared by the literal sharing AAD table. Generation, principal
    /// and UID suffixes are added by the object-specific codecs below.
    pub(super) fn aad(&self, purpose: &str) -> Result<Vec<u8>, SealError> {
        let shared = self.shared.as_ref().ok_or(SealError::Format)?;
        let mut out = aad(purpose, self.owner_id);
        out.push(b'|');
        out.extend_from_slice(&shared.calendar_id.to_be_bytes());
        out.push(b'|');
        out.extend_from_slice(&shared.domain_id);
        Ok(out)
    }

    pub(super) fn bundle_aad(
        &self,
        purpose: &str,
        principal: Option<u32>,
    ) -> Result<Vec<u8>, SealError> {
        let mut out = self.aad(purpose)?;
        if let Some(principal) = principal {
            out.push(b'|');
            out.extend_from_slice(&principal.to_be_bytes());
        }
        Ok(out)
    }

    fn wrap_aad(
        &self,
        purpose: &str,
        generation: u32,
        suffix: Option<&[u8]>,
    ) -> Result<Vec<u8>, SealError> {
        let mut out = self.aad(purpose)?;
        out.push(b'|');
        out.extend_from_slice(&generation.to_be_bytes());
        if let Some(suffix) = suffix {
            out.push(b'|');
            out.extend_from_slice(suffix);
        }
        Ok(out)
    }

    pub(super) fn seal_ck(
        &self,
        dek: &Secret,
        purpose: &str,
        suffix: Option<&[u8]>,
    ) -> Result<Vec<u8>, SealError> {
        let shared = self.shared.as_ref().ok_or(SealError::Format)?;
        let (&generation, ck) = shared.keys.last_key_value().ok_or(SealError::Format)?;
        let mut out = Vec::with_capacity(10 + WRAP_LEN);
        out.extend_from_slice(&[POLICY_VERSION, WRAP_CK]);
        out.extend_from_slice(&shared.calendar_id.to_be_bytes());
        out.extend_from_slice(&generation.to_be_bytes());
        out.extend_from_slice(&wrap_key(
            dek,
            ck,
            &self.wrap_aad(purpose, generation, suffix)?,
        ));
        Ok(out)
    }

    pub(super) fn open_ck(
        &self,
        envelope: &[u8],
        purpose: &str,
        suffix: Option<&[u8]>,
    ) -> Result<Secret, SealError> {
        validate_event_envelope(envelope)?;
        if envelope[1] != WRAP_CK {
            return Err(SealError::Format);
        }
        let shared = self.shared.as_ref().ok_or(SealError::Format)?;
        let calendar = u32::from_be_bytes(envelope[2..6].try_into().unwrap());
        let generation = u32::from_be_bytes(envelope[6..10].try_into().unwrap());
        if calendar != shared.calendar_id {
            return Err(SealError::Structure("collection key calendar"));
        }
        let ck = shared
            .keys
            .get(&generation)
            .ok_or(SealError::Structure("collection key generation"))?;
        unwrap_key(
            &envelope[10..],
            ck,
            &self.wrap_aad(purpose, generation, suffix)?,
        )
        .map_err(|_| SealError::Aead)
    }

    pub(super) fn seal_event_key(&self, dek: &Secret, uid: &str) -> Result<Vec<u8>, SealError> {
        if self.shared.is_some() && !self.is_reverting() {
            self.seal_ck(dek, "ck-dek", Some(uid.as_bytes()))
        } else {
            Ok(seal_key_envelope(
                dek,
                self.ewk()?,
                &event_key_aad(self.owner_id, uid),
            ))
        }
    }
    pub(super) fn open_event_key(&self, envelope: &[u8], uid: &str) -> Result<Secret, SealError> {
        match envelope.get(1) {
            Some(&WRAP_CK) => self.open_ck(envelope, "ck-dek", Some(uid.as_bytes())),
            _ => open_key_envelope(envelope, self.ewk()?, &event_key_aad(self.owner_id, uid)),
        }
    }
}

fn event_key_aad(owner: u32, uid: &str) -> Vec<u8> {
    let mut out = aad("event-key", owner);
    out.push(b'|');
    out.extend_from_slice(uid.as_bytes());
    out
}

pub(super) fn validate_event_envelope(envelope: &[u8]) -> Result<(), SealError> {
    match envelope {
        [POLICY_VERSION, WRAP_MK, ..] if envelope.len() == 2 + WRAP_LEN => Ok(()),
        [POLICY_VERSION, WRAP_CK, ..]
            if envelope.len() == 10 + WRAP_LEN && envelope[6..10] != [0; 4] =>
        {
            Ok(())
        }
        [version, ..] if *version != POLICY_VERSION => Err(SealError::Policy(*version)),
        _ => Err(SealError::Format),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(owner: u32, calendar: u32, id: [u8; 16]) -> ZaKeyDomain {
        ZaKeyDomain::shared(
            owner,
            owner + 1,
            None,
            calendar,
            id,
            BTreeMap::from([
                (1, Secret::from_bytes([3; 32])),
                (2, Secret::from_bytes([5; 32])),
            ]),
            false,
        )
        .unwrap()
    }

    #[test]
    fn ck_wraps_interoperate_with_literal_aad_and_isolate_purposes() {
        let domain = domain(9, 42, [7; 16]);
        let ck = Secret::from_bytes([5; 32]);
        let dek = Secret::from_bytes([11; 32]);
        let vectors: [(&str, Option<&[u8]>, &[u8]); 3] = [
            ("ck-dek", Some(b"uid-1"), b"za/v1|ck-dek|\0\0\0\x09|\0\0\0\x2a|\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07|\0\0\0\x02|uid-1"),
            ("ck-bundle", Some(b"\0\0\0\x08"), b"za/v1|ck-bundle|\0\0\0\x09|\0\0\0\x2a|\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07|\0\0\0\x02|\0\0\0\x08"),
            ("ck-shared-dek", None, b"za/v1|ck-shared-dek|\0\0\0\x09|\0\0\0\x2a|\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07|\0\0\0\x02"),
        ];
        for (purpose, suffix, aad) in vectors {
            let mut envelope = vec![1, 3, 0, 0, 0, 42, 0, 0, 0, 2];
            envelope.extend(wrap_key(&dek, &ck, aad));
            assert_eq!(
                domain
                    .open_ck(&envelope, purpose, suffix)
                    .unwrap()
                    .as_bytes(),
                dek.as_bytes()
            );
            let written = domain.seal_ck(&dek, purpose, suffix).unwrap();
            assert_eq!(&written[..10], &[1, 3, 0, 0, 0, 42, 0, 0, 0, 2]);
            assert_eq!(
                unwrap_key(&written[10..], &ck, aad).unwrap().as_bytes(),
                dek.as_bytes()
            );
            assert!(domain.open_ck(&envelope, "wrong-purpose", suffix).is_err());
            assert!(
                domain
                    .open_ck(&envelope, purpose, Some(b"wrong-suffix"))
                    .is_err()
            );
        }
    }

    #[test]
    fn reads_select_stored_generation_writes_select_newest_and_reverting_uses_ewk() {
        let old = ZaKeyDomain::shared(
            9,
            10,
            None,
            42,
            [7; 16],
            BTreeMap::from([(1, Secret::from_bytes([3; 32]))]),
            false,
        )
        .unwrap();
        let latest = domain(9, 42, [7; 16]);
        let dek = Secret::random();
        let envelope = old.seal_event_key(&dek, "u").unwrap();
        assert_eq!(
            latest.open_event_key(&envelope, "u").unwrap().as_bytes(),
            dek.as_bytes()
        );
        assert_eq!(
            &latest.seal_event_key(&dek, "u").unwrap()[6..10],
            &2u32.to_be_bytes()
        );
        for wrong in [
            domain(8, 42, [7; 16]),
            domain(9, 43, [7; 16]),
            domain(9, 42, [8; 16]),
        ] {
            assert!(wrong.open_event_key(&envelope, "u").is_err());
        }
        assert!(latest.open_event_key(&envelope, "other").is_err());
        let mut unknown = envelope.clone();
        unknown[9] = 3;
        assert!(latest.open_event_key(&unknown, "u").is_err());
        let keys = SessionKeys::new(9, 1, Secret::random());
        let reverting = ZaKeyDomain::shared(
            9,
            9,
            Some(keys.ewk().clone()),
            42,
            [7; 16],
            BTreeMap::from([(1, Secret::from_bytes([3; 32]))]),
            true,
        )
        .unwrap();
        assert_eq!(
            reverting.open_event_key(&envelope, "u").unwrap().as_bytes(),
            dek.as_bytes()
        );
        let account = reverting.seal_event_key(&dek, "u").unwrap();
        assert_eq!(&account[..2], &[1, 1]);
        assert_eq!(account.len(), 74);
        assert_eq!(
            ZaKeyDomain::owner(&keys)
                .open_event_key(&account, "u")
                .unwrap()
                .as_bytes(),
            dek.as_bytes()
        );
        assert!(latest.open_event_key(&account, "u").is_err());
    }
}
