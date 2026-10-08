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
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::Argon2 => f.write_str("key derivation failed"),
            KeyError::Length => f.write_str("invalid ciphertext length"),
            KeyError::Aead => f.write_str("authentication failed"),
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
}
