/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 24;
pub const TAG_LEN: usize = 16;
pub const WRAP_LEN: usize = NONCE_LEN + KEY_LEN + TAG_LEN;
pub const SALT_LEN: usize = 16;

const LABEL_VERIFIER: &[u8] = b"za/v1/verifier";
const LABEL_KEK: &[u8] = b"za/v1/kek";
const LABEL_RKEK: &[u8] = b"za/v1/rkek";
const LABEL_AKEK_PREFIX: &[u8] = b"za/v1/akek/";
const LABEL_EVENTS: &[u8] = b"za/v1/events";
const LABEL_PK: &[u8] = b"za/v1/pk";
const AAD_PREFIX: &[u8] = b"za/v1|";

/// A 32-byte secret that is zeroed on drop and never printed.
#[derive(Clone)]
pub struct Secret(Zeroizing<[u8; KEY_LEN]>);

impl Secret {
    pub fn random() -> Self {
        Secret(Zeroizing::new(rand::random::<[u8; KEY_LEN]>()))
    }

    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Secret(Zeroizing::new(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(..)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Argon2Params {
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl Default for Argon2Params {
    fn default() -> Self {
        Argon2Params {
            m_cost_kib: 64 * 1024,
            t_cost: 3,
            p_cost: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    Argon2,
    Length,
    Aead,
    /// A public key whose shared secret is not contributory (low order).
    PublicKey,
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::Argon2 => f.write_str("key derivation failed"),
            KeyError::Length => f.write_str("invalid ciphertext length"),
            KeyError::Aead => f.write_str("authentication failed"),
            KeyError::PublicKey => f.write_str("invalid public key"),
        }
    }
}

impl std::error::Error for KeyError {}

/// Argon2id(password, salt) -> 32-byte root. CPU-heavy: callers run it on the
/// blocking pool (see Task 5).
pub fn derive_root(
    password: &[u8],
    salt: &[u8; SALT_LEN],
    params: Argon2Params,
) -> Result<Secret, KeyError> {
    let params = Params::new(
        params.m_cost_kib,
        params.t_cost,
        params.p_cost,
        Some(KEY_LEN),
    )
    .map_err(|_| KeyError::Argon2)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password, salt, out.as_mut())
        .map_err(|_| KeyError::Argon2)?;
    Ok(Secret(out))
}

fn hkdf_expand(ikm: &[u8], label: &[u8]) -> Secret {
    let hk = Hkdf::<Sha256>::new(None, ikm);
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    hk.expand(label, out.as_mut())
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    Secret(out)
}

/// SHA-256 of HKDF(root, "za/v1/verifier"); the stored form.
pub fn derive_verifier_hash(root: &Secret) -> [u8; 32] {
    let verifier = hkdf_expand(root.as_bytes(), LABEL_VERIFIER);
    Sha256::digest(verifier.as_bytes()).into()
}

pub fn verifier_matches(root: &Secret, stored: &[u8; 32]) -> bool {
    derive_verifier_hash(root).ct_eq(stored).into()
}

pub fn derive_kek(root: &Secret) -> Secret {
    hkdf_expand(root.as_bytes(), LABEL_KEK)
}

pub fn derive_recovery_kek(recovery: &[u8; 16]) -> Secret {
    hkdf_expand(recovery, LABEL_RKEK)
}

pub fn derive_app_kek(secret: &[u8], credential_id: u32) -> Secret {
    let mut label = LABEL_AKEK_PREFIX.to_vec();
    label.extend_from_slice(credential_id.to_string().as_bytes());
    hkdf_expand(secret, &label)
}

pub fn derive_event_key(mk: &Secret) -> Secret {
    hkdf_expand(mk.as_bytes(), LABEL_EVENTS)
}

/// AAD purpose of the primary-password wrap of MK.
pub const AAD_PASSWORD: &str = "password";
/// AAD purpose of the recovery-key wrap of MK.
pub const AAD_RECOVERY: &str = "recovery";
/// AAD purpose of the X25519 private key wrapped under MK.
pub const AAD_PRIVATE_KEY: &str = "private-key";
/// AAD purpose of a DEK wrapped to a recipient's public key (invitations
/// spec 3.1). `aad` adds the `za/v1|` prefix.
pub const AAD_PK_DEK: &str = "pk-dek";
/// Length of an X25519 public key.
pub const PUBLIC_KEY_LEN: usize = 32;
/// AAD purpose of the pin record sealed under MK (key pinning spec 4.1).
pub const AAD_PINS: &str = "pins";
/// AAD purpose of the pinned wrap of the X25519 private key under MK: the
/// account has pinned (key pinning spec 4.4). `AAD_PRIVATE_KEY` is the
/// legacy wrap.
pub const AAD_PRIVATE_KEY_PINNED: &str = "private-key/pins";
/// Length of a public-key fingerprint: a SHA-256 prefix (key pinning spec 3.1).
pub const FINGERPRINT_LEN: usize = 16;

/// Associated data naming the purpose and the account, so a ciphertext
/// cannot be replayed as a different wrap or on a different account.
pub fn aad(purpose: &str, account_id: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(AAD_PREFIX.len() + purpose.len() + 5);
    out.extend_from_slice(AAD_PREFIX);
    out.extend_from_slice(purpose.as_bytes());
    out.push(b'|');
    out.extend_from_slice(&account_id.to_be_bytes());
    out
}

/// Associated data for an app-password wrap. Built here only, so the auth
/// router and the endpoints cannot disagree on the string.
pub fn app_aad(account_id: u32, credential_id: u32) -> Vec<u8> {
    aad(&format!("app/{credential_id}"), account_id)
}

fn cipher(key: &Secret) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new_from_slice(key.as_bytes()).expect("32-byte key")
}

/// nonce || XChaCha20-Poly1305(plaintext, aad)
pub fn seal(dek: &Secret, aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let nonce_bytes = rand::random::<[u8; NONCE_LEN]>();
    let nonce = XNonce::from(nonce_bytes);
    let ciphertext = cipher(dek)
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .expect("XChaCha20-Poly1305 encryption cannot fail for in-memory buffers");
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    out
}

pub fn open(dek: &Secret, aad: &[u8], sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, KeyError> {
    if sealed.len() < NONCE_LEN + TAG_LEN {
        return Err(KeyError::Length);
    }
    let (nonce_bytes, ciphertext) = sealed.split_at(NONCE_LEN);
    let nonce = XNonce::try_from(nonce_bytes).map_err(|_| KeyError::Length)?;
    cipher(dek)
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| KeyError::Aead)
}

pub fn wrap_key(key: &Secret, wrapping: &Secret, aad: &[u8]) -> Vec<u8> {
    seal(wrapping, aad, key.as_bytes())
}

pub fn unwrap_key(wrapped: &[u8], wrapping: &Secret, aad: &[u8]) -> Result<Secret, KeyError> {
    if wrapped.len() != WRAP_LEN {
        return Err(KeyError::Length);
    }
    let plain = open(wrapping, aad, wrapped)?;
    let bytes: [u8; KEY_LEN] = plain.as_slice().try_into().map_err(|_| KeyError::Length)?;
    Ok(Secret::from_bytes(bytes))
}

/// X25519 keypair: (public key bytes, private key). The private key is
/// wrapped under the master key by the caller.
pub fn generate_keypair() -> ([u8; 32], Secret) {
    let private = Secret::random();
    let secret = x25519_dalek::StaticSecret::from(*private.as_bytes());
    let public = x25519_dalek::PublicKey::from(&secret);
    (public.to_bytes(), private)
}

/// The X25519 public key of `private_key`. An account's own fingerprint is
/// computed from this, never from the stored `public_key` (key pinning spec
/// 3.2).
pub fn derive_public_key(private_key: &Secret) -> [u8; PUBLIC_KEY_LEN] {
    let secret = x25519_dalek::StaticSecret::from(*private_key.as_bytes());
    x25519_dalek::PublicKey::from(&secret).to_bytes()
}

/// Key pinning spec 3.1: the first 16 bytes of SHA-256 over the public key.
pub fn fingerprint_bytes(public_key: &[u8; PUBLIC_KEY_LEN]) -> [u8; FINGERPRINT_LEN] {
    let digest: [u8; 32] = Sha256::digest(public_key).into();
    let mut out = [0u8; FINGERPRINT_LEN];
    out.copy_from_slice(&digest[..FINGERPRINT_LEN]);
    out
}

/// `fingerprint_bytes` as lowercase hex in 8 space-separated groups of 4,
/// such as `3f0a 9c41 07be 22d8 e1f0 5a6c 0b93 77e2` (key pinning spec 3.1).
pub fn display_fingerprint(public_key: &[u8; PUBLIC_KEY_LEN]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(FINGERPRINT_LEN * 2 + FINGERPRINT_LEN / 2 - 1);
    for (index, byte) in fingerprint_bytes(public_key).into_iter().enumerate() {
        if index > 0 && index % 2 == 0 {
            out.push(' ');
        }
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

/// Parses a fingerprint in any case and spacing: whitespace is removed and
/// exactly 32 hex digits must remain (key pinning spec 6.4).
pub fn parse_fingerprint(text: &str) -> Option<[u8; FINGERPRINT_LEN]> {
    fn hex_value(c: char) -> Option<u8> {
        c.to_digit(16).and_then(|value| u8::try_from(value).ok())
    }
    let mut digits = text.chars().filter(|c| !c.is_whitespace());
    let mut out = [0u8; FINGERPRINT_LEN];
    for byte in out.iter_mut() {
        let high = hex_value(digits.next()?)?;
        let low = hex_value(digits.next()?)?;
        *byte = (high << 4) | low;
    }
    digits.next().is_none().then_some(out)
}

/// The associated data a private-key wrap opened under (key pinning spec
/// 4.4): whether the account has pinned is carried by it, not by a stored
/// field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapMark {
    /// `AAD_PRIVATE_KEY_PINNED`: the account has pinned.
    Pinned,
    /// `AAD_PRIVATE_KEY`, release 2's wrap: the account never pinned.
    Legacy,
}

impl WrapMark {
    fn purpose(self) -> &'static str {
        match self {
            WrapMark::Pinned => AAD_PRIVATE_KEY_PINNED,
            WrapMark::Legacy => AAD_PRIVATE_KEY,
        }
    }
}

/// Wraps the X25519 private key under MK with the associated data of `mark`
/// (`WRAP_LEN` bytes, the layout of release 2's wrap).
pub fn wrap_private_key(
    private_key: &Secret,
    mk: &Secret,
    account_id: u32,
    mark: WrapMark,
) -> Vec<u8> {
    wrap_key(private_key, mk, &aad(mark.purpose(), account_id))
}

/// Opens a private-key wrap under the pinned associated data first, then the
/// legacy (key pinning spec 4.4). `Err(KeyError::Aead)` when neither opens.
pub fn unwrap_private_key(
    wrap: &[u8],
    mk: &Secret,
    account_id: u32,
) -> Result<(Secret, WrapMark), KeyError> {
    [WrapMark::Pinned, WrapMark::Legacy]
        .into_iter()
        .find_map(|mark| {
            unwrap_key(wrap, mk, &aad(mark.purpose(), account_id))
                .ok()
                .map(|key| (key, mark))
        })
        .ok_or(KeyError::Aead)
}

/// Wrapping key of a `pk` wrap: HKDF over the shared secret with the info
/// `za/v1/pk || E || P` (ephemeral, then recipient public key), built by
/// appending to the label as `derive_app_kek` does.
fn pk_wrapping_key(
    shared: &[u8; 32],
    ephemeral: &[u8; PUBLIC_KEY_LEN],
    recipient: &[u8; PUBLIC_KEY_LEN],
) -> Secret {
    let mut info = Vec::with_capacity(LABEL_PK.len() + 2 * PUBLIC_KEY_LEN);
    info.extend_from_slice(LABEL_PK);
    info.extend_from_slice(ephemeral);
    info.extend_from_slice(recipient);
    hkdf_expand(shared, &info)
}

/// Invitations spec 3.1: wraps `dek` to the X25519 public key `recipient`.
/// Returns the ephemeral public key and `nonce || ciphertext`
/// (`WRAP_LEN` bytes). The ephemeral private key is zeroed on return.
pub fn pk_wrap(
    dek: &Secret,
    recipient: &[u8; PUBLIC_KEY_LEN],
    aad: &[u8],
) -> Result<([u8; PUBLIC_KEY_LEN], Vec<u8>), KeyError> {
    let ephemeral = x25519_dalek::StaticSecret::from(*Secret::random().as_bytes());
    let ephemeral_public = x25519_dalek::PublicKey::from(&ephemeral).to_bytes();
    let shared = ephemeral.diffie_hellman(&x25519_dalek::PublicKey::from(*recipient));
    if !shared.was_contributory() {
        return Err(KeyError::PublicKey);
    }
    let wrapping = pk_wrapping_key(shared.as_bytes(), &ephemeral_public, recipient);
    Ok((ephemeral_public, wrap_key(dek, &wrapping, aad)))
}

/// Opens a `pk_wrap` with the recipient's private key. Only the drain calls
/// this (invariant 11).
pub fn pk_unwrap(
    ephemeral: &[u8; PUBLIC_KEY_LEN],
    wrapped: &[u8],
    private_key: &Secret,
    aad: &[u8],
) -> Result<Secret, KeyError> {
    let secret = x25519_dalek::StaticSecret::from(*private_key.as_bytes());
    let recipient = x25519_dalek::PublicKey::from(&secret).to_bytes();
    let shared = secret.diffie_hellman(&x25519_dalek::PublicKey::from(*ephemeral));
    if !shared.was_contributory() {
        return Err(KeyError::PublicKey);
    }
    let wrapping = pk_wrapping_key(shared.as_bytes(), ephemeral, &recipient);
    unwrap_key(wrapped, &wrapping, aad)
}

/// Keyed BLAKE3 fingerprint of an opaque value (used for cache keys that must
/// not reveal the value). `key` is a per-process random secret.
pub fn fingerprint(key: &[u8; 32], value: &[u8]) -> [u8; 32] {
    *blake3::keyed_hash(key, value).as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SALT: [u8; 16] = [7u8; 16];

    fn fast_params() -> Argon2Params {
        // Small parameters keep the unit tests fast; production uses Default.
        Argon2Params {
            m_cost_kib: 64,
            t_cost: 1,
            p_cost: 1,
        }
    }

    #[test]
    fn root_derivation_is_deterministic_and_salt_dependent() {
        let a = derive_root(b"correct horse", &SALT, fast_params()).unwrap();
        let b = derive_root(b"correct horse", &SALT, fast_params()).unwrap();
        let c = derive_root(b"correct horse", &[8u8; 16], fast_params()).unwrap();
        assert_eq!(a.as_bytes(), b.as_bytes());
        assert_ne!(a.as_bytes(), c.as_bytes());
    }

    #[test]
    fn verifier_and_kek_are_independent() {
        let root = derive_root(b"pw", &SALT, fast_params()).unwrap();
        let verifier = derive_verifier_hash(&root);
        let kek = derive_kek(&root);
        assert_ne!(&verifier, kek.as_bytes());
        assert!(!kek.as_bytes().starts_with(&verifier[..8]));
        assert!(verifier_matches(&root, &verifier));
        let other = derive_root(b"pw2", &SALT, fast_params()).unwrap();
        assert!(!verifier_matches(&other, &verifier));
    }

    #[test]
    fn wrap_round_trips_and_rejects_wrong_key_or_aad() {
        let mk = Secret::random();
        let kek = Secret::random();
        let wrapped = wrap_key(&mk, &kek, &aad("password", 7));
        assert_eq!(wrapped.len(), WRAP_LEN);
        let back = unwrap_key(&wrapped, &kek, &aad("password", 7)).unwrap();
        assert_eq!(back.as_bytes(), mk.as_bytes());
        assert!(unwrap_key(&wrapped, &Secret::random(), &aad("password", 7)).is_err());
        assert!(unwrap_key(&wrapped, &kek, &aad("password", 8)).is_err());
        assert!(unwrap_key(&wrapped, &kek, &aad("recovery", 7)).is_err());
        let mut tampered = wrapped.clone();
        tampered[40] ^= 1;
        assert!(unwrap_key(&tampered, &kek, &aad("password", 7)).is_err());
        assert!(unwrap_key(&wrapped[..WRAP_LEN - 1], &kek, &aad("password", 7)).is_err());
    }

    #[test]
    fn every_wrap_kind_round_trips() {
        let mk = Secret::random();
        let root = derive_root(b"pw", &SALT, fast_params()).unwrap();
        let recovery = [3u8; 16];
        let kinds = [
            ("password", derive_kek(&root)),
            ("recovery", derive_recovery_kek(&recovery)),
            ("app/42", derive_app_kek(b"abcdef", 42)),
        ];
        for (purpose, key) in kinds {
            let w = wrap_key(&mk, &key, &aad(purpose, 1));
            assert_eq!(
                unwrap_key(&w, &key, &aad(purpose, 1)).unwrap().as_bytes(),
                mk.as_bytes()
            );
        }
        assert_ne!(
            derive_app_kek(b"abcdef", 42).as_bytes(),
            derive_app_kek(b"abcdef", 43).as_bytes()
        );
        assert_ne!(
            derive_app_kek(b"abcdef", 42).as_bytes(),
            derive_app_kek(b"abcdeg", 42).as_bytes()
        );
    }

    #[test]
    fn app_aad_names_the_credential_and_account() {
        assert_eq!(app_aad(7, 42), aad("app/42", 7));
        assert_ne!(app_aad(7, 42), app_aad(7, 43));
        assert_ne!(app_aad(7, 42), app_aad(8, 42));
    }

    #[test]
    fn seal_round_trips_and_binds_aad() {
        let dek = Secret::random();
        let sealed = seal(&dek, &aad("event", 5), b"hello world");
        assert_eq!(sealed.len(), NONCE_LEN + 11 + TAG_LEN);
        assert_eq!(
            &*open(&dek, &aad("event", 5), &sealed).unwrap(),
            b"hello world"
        );
        assert!(open(&dek, &aad("event", 6), &sealed).is_err());
        assert!(open(&Secret::random(), &aad("event", 5), &sealed).is_err());
        let mut t = sealed.clone();
        t[NONCE_LEN] ^= 0x80;
        assert!(open(&dek, &aad("event", 5), &t).is_err());
        assert!(open(&dek, &aad("event", 5), &sealed[..NONCE_LEN]).is_err());
    }

    #[test]
    fn nonces_are_fresh() {
        let dek = Secret::random();
        let a = seal(&dek, b"x", b"same");
        let b = seal(&dek, b"x", b"same");
        assert_ne!(a[..NONCE_LEN], b[..NONCE_LEN]);
    }

    #[test]
    fn keypair_public_matches_private() {
        let (public, private) = generate_keypair();
        let derived =
            x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(*private.as_bytes()));
        assert_eq!(public, derived.to_bytes());
    }

    #[test]
    fn secret_debug_is_silent() {
        let s = Secret::from_bytes([0xAB; 32]);
        let text = format!("{s:?}");
        assert!(!text.contains("AB") && !text.contains("171"), "{text}");
    }

    #[test]
    fn event_key_differs_from_master_key() {
        let mk = Secret::random();
        assert_ne!(derive_event_key(&mk).as_bytes(), mk.as_bytes());
        assert_eq!(
            derive_event_key(&mk).as_bytes(),
            derive_event_key(&mk).as_bytes()
        );
    }

    #[test]
    fn default_argon2_params_are_production_values() {
        assert_eq!(
            Argon2Params::default(),
            Argon2Params {
                m_cost_kib: 65536,
                t_cost: 3,
                p_cost: 1
            }
        );
    }

    #[test]
    fn pk_wrap_round_trips_for_the_recipient_only() {
        let (public, private) = generate_keypair();
        let dek = Secret::random();
        let aad7 = aad(AAD_PK_DEK, 7);
        let (ephemeral, wrapped) = pk_wrap(&dek, &public, &aad7).unwrap();
        assert_eq!(wrapped.len(), WRAP_LEN);
        assert_eq!(
            pk_unwrap(&ephemeral, &wrapped, &private, &aad7)
                .unwrap()
                .as_bytes(),
            dek.as_bytes()
        );
        // Another recipient's private key.
        let (_, other) = generate_keypair();
        assert_eq!(
            pk_unwrap(&ephemeral, &wrapped, &other, &aad7).err(),
            Some(KeyError::Aead)
        );
        // Associated data naming another account or another purpose.
        for wrong in [aad(AAD_PK_DEK, 8), aad(AAD_PRIVATE_KEY, 7)] {
            assert_eq!(
                pk_unwrap(&ephemeral, &wrapped, &private, &wrong).err(),
                Some(KeyError::Aead)
            );
        }
        // The wrap moved under another ephemeral key.
        let (other_ephemeral, _) = pk_wrap(&dek, &public, &aad7).unwrap();
        assert_eq!(
            pk_unwrap(&other_ephemeral, &wrapped, &private, &aad7).err(),
            Some(KeyError::Aead)
        );
        // Tampered and truncated ciphertext.
        let mut tampered = wrapped.clone();
        tampered[40] ^= 1;
        assert!(pk_unwrap(&ephemeral, &tampered, &private, &aad7).is_err());
        assert!(pk_unwrap(&ephemeral, &wrapped[..WRAP_LEN - 1], &private, &aad7).is_err());
    }

    #[test]
    fn two_pk_wraps_of_one_dek_differ() {
        let (public, private) = generate_keypair();
        let dek = Secret::random();
        let aad1 = aad(AAD_PK_DEK, 1);
        let (e1, w1) = pk_wrap(&dek, &public, &aad1).unwrap();
        let (e2, w2) = pk_wrap(&dek, &public, &aad1).unwrap();
        assert_ne!(e1, e2, "a fresh ephemeral key per wrap");
        assert_ne!(w1, w2);
        for (e, w) in [(e1, w1), (e2, w2)] {
            assert_eq!(
                pk_unwrap(&e, &w, &private, &aad1).unwrap().as_bytes(),
                dek.as_bytes()
            );
        }
    }

    #[test]
    fn pk_wrap_refuses_low_order_public_keys() {
        let (public, private) = generate_keypair();
        let dek = Secret::random();
        let aad1 = aad(AAD_PK_DEK, 1);
        assert_eq!(
            pk_wrap(&dek, &[0u8; PUBLIC_KEY_LEN], &aad1).err(),
            Some(KeyError::PublicKey)
        );
        let (_, wrapped) = pk_wrap(&dek, &public, &aad1).unwrap();
        assert_eq!(
            pk_unwrap(&[0u8; PUBLIC_KEY_LEN], &wrapped, &private, &aad1).err(),
            Some(KeyError::PublicKey)
        );
    }

    #[test]
    fn pk_info_is_the_label_then_both_public_keys() {
        let shared = [5u8; 32];
        let (e, p) = ([1u8; 32], [2u8; 32]);
        let mut info = LABEL_PK.to_vec();
        info.extend_from_slice(&e);
        info.extend_from_slice(&p);
        let key = pk_wrapping_key(&shared, &e, &p);
        assert_eq!(key.as_bytes(), hkdf_expand(&shared, &info).as_bytes());
        assert_ne!(key.as_bytes(), pk_wrapping_key(&shared, &p, &e).as_bytes());
        assert_ne!(key.as_bytes(), hkdf_expand(&shared, LABEL_PK).as_bytes());
    }

    /// RFC 7748 section 6.1, Alice's public key.
    const RFC7748_ALICE: &str = "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a";

    fn hex32(text: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (byte, pair) in out.iter_mut().zip(text.as_bytes().chunks(2)) {
            *byte = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
        }
        out
    }

    #[test]
    fn display_fingerprint_is_the_sha256_prefix_in_eight_groups() {
        let key: [u8; 32] = hex32(RFC7748_ALICE);
        assert_eq!(
            display_fingerprint(&key),
            "300c 9c96 03b9 2a4b 39ed 3958 bf92 4011"
        );
        assert_eq!(
            display_fingerprint(&[0u8; 32]),
            "6668 7aad f862 bd77 6c8f c18b 8e9f 8e20"
        );
        assert_eq!(
            parse_fingerprint("300C9C9603B92A4B 39ED 3958 BF92 4011"),
            Some(fingerprint_bytes(&key))
        );
        for bad in [
            "",
            "300c 9c96",
            "300c 9c96 03b9 2a4b 39ed 3958 bf92 40110",
            "zz0c 9c96 03b9 2a4b 39ed 3958 bf92 4011",
        ] {
            assert_eq!(parse_fingerprint(bad), None, "{bad}");
        }
    }

    #[test]
    fn pinned_and_legacy_wraps_open_only_under_their_own_purpose() {
        let mk = Secret::random();
        let (public, private) = generate_keypair();
        let pinned = wrap_private_key(&private, &mk, 7, WrapMark::Pinned);
        let legacy = wrap_private_key(&private, &mk, 7, WrapMark::Legacy);
        assert_eq!((pinned.len(), legacy.len()), (WRAP_LEN, WRAP_LEN));
        assert!(unwrap_key(&pinned, &mk, &aad(AAD_PRIVATE_KEY, 7)).is_err());
        assert!(unwrap_key(&legacy, &mk, &aad(AAD_PRIVATE_KEY_PINNED, 7)).is_err());
        let (key, mark) = unwrap_private_key(&pinned, &mk, 7).unwrap();
        assert_eq!((mark, derive_public_key(&key)), (WrapMark::Pinned, public));
        assert_eq!(
            unwrap_private_key(&legacy, &mk, 7).unwrap().1,
            WrapMark::Legacy
        );
        assert_eq!(
            unwrap_private_key(&pinned, &mk, 8).err(),
            Some(KeyError::Aead)
        );
    }
}
