/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Sealed dead-property containers (upgrade design, "Dead properties and
//! object metadata"; upgrade rulings 16, 23 and 29).
//!
//! At Stalwart 1.0 a calendar's or event's WebDAV dead properties are one
//! `MetadataClass::Shared` container per document in upstream's metadata
//! store, outside the record. For a key account the whole container (names
//! and values) is sealed. The sealed form is itself a valid upstream
//! container, because keyless code parses every container's header: its
//! header is `[FORMAT_VERSION, MetadataKinds::DAV, 1]` and its one entry is
//! a DAV property `{urn:x-zero-access:sealed}container` whose value is text
//! only (no attributes, no children, no language). The text is the base64
//! of the payload:
//!
//! ```text
//! payload  = version (1) || bound (0 or 1) || envelope || sealed
//! envelope = a key envelope in the event shape: `1 || 1 || wrapped DEK`
//!            under the event wrapping key, or `1 || 3 || calendar id ||
//!            generation || wrapped DEK` under the collection key
//! sealed   = seal_padded(DEK, aad, original container bytes)
//! ```
//!
//! Each container has its own data key. The envelope is bound to the
//! container's scope and the ciphertext's associated data is
//! `za/v1|dead-container|<owner>|<scope>`, where the scope is
//! `<collection byte>|<bound>|<event UID>` for an event and
//! `<collection byte>|<bound>|<calendar id, big-endian>` for a calendar
//! whose document id was assigned when it was sealed (`bound` 1). A
//! calendar created by MKCOL is sealed before its id is assigned (a pending
//! slot), so its scope holds no id (`bound` 0) until its next property
//! write: the recorded limit of ruling 16. An event's scope binds its UID,
//! which a COPY or MOVE keeps, never its document id.
//!
//! Conversion, rollback and rotation rewrap the envelope only
//! (`rewrap_container`); COPY and MOVE between calendars reseal
//! (`reseal_container`). Every opener checks that what it opened is a valid
//! upstream container.

use super::{
    domain::{ZaKeyDomain, validate_event_envelope},
    tree::{SealError, WRAP_MK, open_padded, seal_padded},
};
use crate::calendar::sharing::format::WRAP_CK;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::borrow::Cow;
use types::{
    collection::Collection,
    metadata::{
        EncodedMetadata, MetadataBuilder, MetadataKinds, MetadataView, XmlName, XmlNode, XmlValue,
    },
};
use vault::keys::{Secret, WRAP_LEN, aad};
use zeroize::Zeroizing;

/// The reserved namespace of the sealed wrapper's one DAV entry.
pub const CONTAINER_NAMESPACE: &str = "urn:x-zero-access:sealed";
/// The reserved local name of the sealed wrapper's one DAV entry.
pub const CONTAINER_NAME: &str = "container";
/// The payload's version byte.
pub const CONTAINER_PAYLOAD_V1: u8 = 1;

/// Which document a container belongs to, as its seal binds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZaContainerScope<'x> {
    /// A calendar's container. `Some(id)` when the calendar's document id is
    /// assigned at sealing; `None` for a pending one (MKCOL).
    Calendar(Option<u32>),
    /// An event's container, bound by the event's UID.
    Event(&'x str),
}

/// The document a container is opened for. A calendar's id is always known
/// to a reader; the stored `bound` byte says whether the seal used it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZaContainerOf<'x> {
    Calendar(u32),
    Event(&'x str),
}

impl ZaContainerScope<'_> {
    fn bound(&self) -> u8 {
        match self {
            ZaContainerScope::Calendar(Some(_)) => 1,
            ZaContainerScope::Calendar(None) | ZaContainerScope::Event(_) => 0,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16);
        match self {
            ZaContainerScope::Calendar(id) => {
                out.push(u8::from(Collection::Calendar));
                out.push(b'|');
                out.push(self.bound());
                out.push(b'|');
                if let Some(id) = id {
                    out.extend_from_slice(&id.to_be_bytes());
                }
            }
            ZaContainerScope::Event(uid) => {
                out.push(u8::from(Collection::CalendarEvent));
                out.push(b'|');
                out.push(0);
                out.push(b'|');
                out.extend_from_slice(uid.as_bytes());
            }
        }
        out
    }
}

impl<'x> ZaContainerOf<'x> {
    fn scope(self, bound: u8) -> Result<ZaContainerScope<'x>, SealError> {
        match (self, bound) {
            (ZaContainerOf::Calendar(id), 1) => Ok(ZaContainerScope::Calendar(Some(id))),
            (ZaContainerOf::Calendar(_), 0) => Ok(ZaContainerScope::Calendar(None)),
            (ZaContainerOf::Event(uid), 0) => Ok(ZaContainerScope::Event(uid)),
            _ => Err(SealError::Format),
        }
    }
}

fn sealed_aad(owner_id: u32, scope: &[u8]) -> Vec<u8> {
    let mut out = aad("dead-container", owner_id);
    out.push(b'|');
    out.extend_from_slice(scope);
    out
}

/// The parts of a sealed wrapper's payload.
struct Payload {
    bound: u8,
    envelope: Vec<u8>,
    sealed: Vec<u8>,
}

impl Payload {
    fn parse(bytes: &[u8]) -> Result<Self, SealError> {
        let [version, bound, rest @ ..] = bytes else {
            return Err(SealError::Format);
        };
        if *version != CONTAINER_PAYLOAD_V1 || *bound > 1 {
            return Err(SealError::Format);
        }
        let envelope_len = match rest.get(1) {
            Some(&WRAP_MK) => 2 + WRAP_LEN,
            Some(&WRAP_CK) => 10 + WRAP_LEN,
            _ => return Err(SealError::Format),
        };
        if rest.len() <= envelope_len {
            return Err(SealError::Format);
        }
        let (envelope, sealed) = rest.split_at(envelope_len);
        validate_event_envelope(envelope)?;
        Ok(Payload {
            bound: *bound,
            envelope: envelope.to_vec(),
            sealed: sealed.to_vec(),
        })
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + self.envelope.len() + self.sealed.len());
        out.push(CONTAINER_PAYLOAD_V1);
        out.push(self.bound);
        out.extend_from_slice(&self.envelope);
        out.extend_from_slice(&self.sealed);
        out
    }

    fn wrap(&self) -> Result<EncodedMetadata, SealError> {
        let mut builder = MetadataBuilder::new();
        builder
            .set_dav(
                XmlName::borrowed(Some(CONTAINER_NAMESPACE), CONTAINER_NAME),
                &XmlValue {
                    lang: None,
                    attributes: Vec::new(),
                    children: vec![XmlNode::Text(Cow::Owned(STANDARD.encode(self.encode())))],
                },
            )
            .map_err(|_| SealError::Format)?;
        builder.encode().ok_or(SealError::Format)
    }
}

/// The payload of a sealed wrapper, or `None` when `container` is not one
/// (a plaintext container, or any other shape). Keyless and structural.
fn wrapper_payload(container: &[u8]) -> Option<Vec<u8>> {
    let view = MetadataView::new(container)?;
    if view.kinds() != MetadataKinds::DAV || view.len() != 1 {
        return None;
    }
    let (name, value) = view.dav().next()?;
    if !name.matches(Some(CONTAINER_NAMESPACE), CONTAINER_NAME) {
        return None;
    }
    let value = value.to_value()?;
    match (
        value.lang,
        value.attributes.as_slice(),
        value.children.as_slice(),
    ) {
        (None, [], [XmlNode::Text(text)]) => STANDARD.decode(text.as_bytes()).ok(),
        _ => None,
    }
}

/// Whether `container` is a sealed wrapper (structure only, no keys). A
/// key account's calendar and event containers are always sealed; the
/// leak rule checks this.
pub fn is_sealed_container(container: &[u8]) -> bool {
    wrapper_payload(container).is_some_and(|payload| Payload::parse(&payload).is_ok())
}

/// The key class of a sealed container's envelope: `Some(true)` under a
/// collection key, `Some(false)` under the event wrapping key, `None` when
/// it is not a sealed wrapper. Keyless; for conversion's classification.
pub fn container_is_ck(container: &[u8]) -> Option<bool> {
    let payload = Payload::parse(&wrapper_payload(container)?).ok()?;
    Some(payload.envelope[1] == WRAP_CK)
}

/// Seals a plaintext container (`plain`, a valid upstream container) under
/// a fresh data key in `domain`. There is no "already sealed" check: a
/// client's container that happens to look like a wrapper is sealed and
/// restored unchanged, as a client's `X-ZA-KEY` in an event is.
pub fn seal_container(
    plain: &[u8],
    domain: &ZaKeyDomain,
    scope: ZaContainerScope<'_>,
) -> Result<EncodedMetadata, SealError> {
    if MetadataView::new(plain).is_none() {
        return Err(SealError::Structure("container"));
    }
    let scope_bytes = scope.bytes();
    let dek = Secret::random();
    Payload {
        bound: scope.bound(),
        envelope: domain.seal_container_key(&dek, &scope_bytes)?,
        sealed: seal_padded(&dek, &sealed_aad(domain.owner_id(), &scope_bytes), plain),
    }
    .wrap()
}

/// Opens a sealed container in `domain` and returns the original container
/// bytes, checked to be a valid upstream container. A plaintext container
/// (no wrapper) is returned unchanged to an owner domain, as legacy events
/// are, and refused (`NotSealed`) to a grantee's.
pub fn open_container(
    container: &[u8],
    domain: &ZaKeyDomain,
    of: ZaContainerOf<'_>,
) -> Result<Zeroizing<Vec<u8>>, SealError> {
    let Some(payload) = wrapper_payload(container) else {
        return if domain.is_owner() && MetadataView::new(container).is_some() {
            Ok(Zeroizing::new(container.to_vec()))
        } else {
            Err(SealError::NotSealed)
        };
    };
    let payload = Payload::parse(&payload)?;
    let scope = of.scope(payload.bound)?.bytes();
    let dek = domain.open_container_key(&payload.envelope, &scope)?;
    let plain = open_padded(
        &dek,
        &sealed_aad(domain.owner_id(), &scope),
        &payload.sealed,
    )?;
    if MetadataView::new(&plain).is_none() {
        return Err(SealError::Structure("container"));
    }
    Ok(plain)
}

/// Rewraps a sealed container's data key from `from` to `to`, keeping its
/// ciphertext (conversion, rollback, rotation; ruling 29). A plaintext
/// container is sealed in full under `to`.
pub fn rewrap_container(
    container: &[u8],
    from: &ZaKeyDomain,
    to: &ZaKeyDomain,
    of: ZaContainerOf<'_>,
) -> Result<EncodedMetadata, SealError> {
    if from.owner_id() != to.owner_id() {
        return Err(SealError::Structure("cross-account container rewrap"));
    }
    let Some(payload) = wrapper_payload(container) else {
        let plain = open_container(container, from, of)?;
        let scope = match of {
            ZaContainerOf::Calendar(id) => ZaContainerScope::Calendar(Some(id)),
            ZaContainerOf::Event(uid) => ZaContainerScope::Event(uid),
        };
        return seal_container(&plain, to, scope);
    };
    let mut payload = Payload::parse(&payload)?;
    let scope = of.scope(payload.bound)?.bytes();
    let dek = from.open_container_key(&payload.envelope, &scope)?;
    payload.envelope = to.seal_container_key(&dek, &scope)?;
    payload.wrap()
}

/// Opens under (`from`, `of`) and seals under (`to`, `scope`), with a fresh
/// data key: COPY and MOVE between calendars or key domains (ruling 29).
pub fn reseal_container(
    container: &[u8],
    from: &ZaKeyDomain,
    of: ZaContainerOf<'_>,
    to: &ZaKeyDomain,
    scope: ZaContainerScope<'_>,
) -> Result<EncodedMetadata, SealError> {
    if from.owner_id() != to.owner_id() {
        return Err(SealError::Structure("cross-account container reseal"));
    }
    let plain = open_container(container, from, of)?;
    seal_container(&plain, to, scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use vault::session::SessionKeys;

    fn keys() -> SessionKeys {
        SessionKeys::new(9, 1, Secret::random())
    }

    fn ck(keys: &SessionKeys, owner: bool) -> ZaKeyDomain {
        ZaKeyDomain::shared(
            9,
            if owner { 9 } else { 8 },
            owner.then(|| keys.ewk().clone()),
            42,
            [7; 16],
            BTreeMap::from([(1, Secret::from_bytes([3; 32]))]),
            false,
        )
        .unwrap()
    }

    fn plain() -> EncodedMetadata {
        let mut builder = MetadataBuilder::new();
        builder
            .set_dav(
                XmlName::borrowed(Some("urn:secret-namespace-canary"), "secret-name-canary"),
                &XmlValue {
                    lang: None,
                    attributes: Vec::new(),
                    children: vec![XmlNode::Text(Cow::Borrowed("secret value canary"))],
                },
            )
            .unwrap();
        builder
            .set_dav(
                XmlName::borrowed(Some("http://apple.com/ns/ical/"), "calendar-color"),
                &XmlValue {
                    lang: None,
                    attributes: Vec::new(),
                    children: vec![XmlNode::Text(Cow::Borrowed("#00ff00"))],
                },
            )
            .unwrap();
        builder.encode().unwrap()
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn sealed_container_is_a_valid_upstream_container_and_round_trips() {
        let keys = keys();
        let owner = ZaKeyDomain::owner(&keys);
        let plain = plain();
        for (scope, of) in [
            (
                ZaContainerScope::Event("ev-1"),
                ZaContainerOf::Event("ev-1"),
            ),
            (
                ZaContainerScope::Calendar(Some(42)),
                ZaContainerOf::Calendar(42),
            ),
            (
                ZaContainerScope::Calendar(None),
                ZaContainerOf::Calendar(42),
            ),
        ] {
            let sealed = seal_container(plain.as_bytes(), &owner, scope).unwrap();
            let view = MetadataView::new(sealed.as_bytes()).expect("checked view");
            assert_eq!(view.kinds(), MetadataKinds::DAV);
            assert_eq!(view.len(), 1);
            assert_eq!(sealed.kinds(), MetadataKinds::DAV);
            assert!(is_sealed_container(sealed.as_bytes()));
            assert_eq!(container_is_ck(sealed.as_bytes()), Some(false));
            for canary in [
                &b"canary"[..],
                b"secret-namespace",
                b"calendar-color",
                b"apple.com",
            ] {
                assert!(!contains(sealed.as_bytes(), canary));
            }
            let opened = open_container(sealed.as_bytes(), &owner, of).unwrap();
            assert_eq!(opened.as_slice(), plain.as_bytes());
        }
    }

    #[test]
    fn scope_binding_is_enforced() {
        let keys = keys();
        let owner = ZaKeyDomain::owner(&keys);
        let plain = plain();
        let event =
            seal_container(plain.as_bytes(), &owner, ZaContainerScope::Event("ev-1")).unwrap();
        assert_eq!(
            open_container(event.as_bytes(), &owner, ZaContainerOf::Event("ev-2")).err(),
            Some(SealError::Aead)
        );
        assert!(open_container(event.as_bytes(), &owner, ZaContainerOf::Calendar(1)).is_err());
        let calendar = seal_container(
            plain.as_bytes(),
            &owner,
            ZaContainerScope::Calendar(Some(42)),
        )
        .unwrap();
        assert_eq!(
            open_container(calendar.as_bytes(), &owner, ZaContainerOf::Calendar(43)).err(),
            Some(SealError::Aead)
        );
        let other = ZaKeyDomain::owner(&SessionKeys::new(9, 1, Secret::random()));
        assert_eq!(
            open_container(calendar.as_bytes(), &other, ZaContainerOf::Calendar(42)).err(),
            Some(SealError::Aead)
        );
    }

    #[test]
    fn a_flipped_bound_byte_or_corrupted_payload_fails() {
        let keys = keys();
        let owner = ZaKeyDomain::owner(&keys);
        let sealed = seal_container(
            plain().as_bytes(),
            &owner,
            ZaContainerScope::Calendar(Some(42)),
        )
        .unwrap();
        let mut payload = Payload::parse(&wrapper_payload(sealed.as_bytes()).unwrap()).unwrap();
        payload.bound = 0;
        let flipped = payload.wrap().unwrap();
        assert_eq!(
            open_container(flipped.as_bytes(), &owner, ZaContainerOf::Calendar(42)).err(),
            Some(SealError::Aead)
        );
        let mut payload = Payload::parse(&wrapper_payload(sealed.as_bytes()).unwrap()).unwrap();
        let last = payload.sealed.len() - 1;
        payload.sealed[last] ^= 1;
        let corrupted = payload.wrap().unwrap();
        assert_eq!(
            open_container(corrupted.as_bytes(), &owner, ZaContainerOf::Calendar(42)).err(),
            Some(SealError::Aead)
        );
    }

    #[test]
    fn plaintext_is_for_owners_only() {
        let keys = keys();
        let plain = plain();
        assert!(!is_sealed_container(plain.as_bytes()));
        let opened = open_container(
            plain.as_bytes(),
            &ZaKeyDomain::owner(&keys),
            ZaContainerOf::Calendar(42),
        )
        .unwrap();
        assert_eq!(opened.as_slice(), plain.as_bytes());
        assert_eq!(
            open_container(
                plain.as_bytes(),
                &ck(&keys, false),
                ZaContainerOf::Calendar(42)
            )
            .err(),
            Some(SealError::NotSealed)
        );
    }

    #[test]
    fn rewrap_keeps_the_ciphertext_and_moves_the_key() {
        let keys = keys();
        let owner = ZaKeyDomain::owner(&keys);
        let shared = ck(&keys, true);
        let grantee = ck(&keys, false);
        let plain = plain();
        let sealed =
            seal_container(plain.as_bytes(), &owner, ZaContainerScope::Event("ev-1")).unwrap();
        let rewrapped = rewrap_container(
            sealed.as_bytes(),
            &owner,
            &shared,
            ZaContainerOf::Event("ev-1"),
        )
        .unwrap();
        assert_eq!(container_is_ck(rewrapped.as_bytes()), Some(true));
        let before = Payload::parse(&wrapper_payload(sealed.as_bytes()).unwrap()).unwrap();
        let after = Payload::parse(&wrapper_payload(rewrapped.as_bytes()).unwrap()).unwrap();
        assert_eq!(before.sealed, after.sealed);
        assert_ne!(before.envelope, after.envelope);
        // The grantee opens the CK form, never the owner's EWK form.
        assert!(open_container(sealed.as_bytes(), &grantee, ZaContainerOf::Event("ev-1")).is_err());
        let opened =
            open_container(rewrapped.as_bytes(), &grantee, ZaContainerOf::Event("ev-1")).unwrap();
        assert_eq!(opened.as_slice(), plain.as_bytes());
        // Rollback.
        let back = rewrap_container(
            rewrapped.as_bytes(),
            &shared,
            &owner,
            ZaContainerOf::Event("ev-1"),
        )
        .unwrap();
        assert_eq!(container_is_ck(back.as_bytes()), Some(false));
        let opened = open_container(back.as_bytes(), &owner, ZaContainerOf::Event("ev-1")).unwrap();
        assert_eq!(opened.as_slice(), plain.as_bytes());
        // A plaintext container is sealed in full.
        let sealed_now = rewrap_container(
            plain.as_bytes(),
            &owner,
            &shared,
            ZaContainerOf::Calendar(42),
        )
        .unwrap();
        assert!(is_sealed_container(sealed_now.as_bytes()));
    }

    #[test]
    fn reseal_moves_between_calendars() {
        let keys = keys();
        let owner = ZaKeyDomain::owner(&keys);
        let shared = ck(&keys, true);
        let plain = plain();
        let sealed =
            seal_container(plain.as_bytes(), &owner, ZaContainerScope::Event("ev-1")).unwrap();
        let moved = reseal_container(
            sealed.as_bytes(),
            &owner,
            ZaContainerOf::Event("ev-1"),
            &shared,
            ZaContainerScope::Event("ev-1"),
        )
        .unwrap();
        let opened =
            open_container(moved.as_bytes(), &shared, ZaContainerOf::Event("ev-1")).unwrap();
        assert_eq!(opened.as_slice(), plain.as_bytes());
    }

    #[test]
    fn container_and_event_envelopes_do_not_cross() {
        // A container's key envelope uses its own purposes: the same DEK
        // wrapped for a container does not open as an event's and back.
        let keys = keys();
        let owner = ZaKeyDomain::owner(&keys);
        let dek = Secret::random();
        let envelope = owner.seal_container_key(&dek, b"ev-1").unwrap();
        assert!(owner.open_event_key(&envelope, "ev-1").is_err());
        let envelope = owner.seal_event_key(&dek, "ev-1").unwrap();
        assert!(owner.open_container_key(&envelope, b"ev-1").is_err());
    }
}
