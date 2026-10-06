/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub(crate) const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const DATA_CHARS: usize = 26; // 16 bytes = 128 bits -> 26 base32 characters
const TOTAL_CHARS: usize = DATA_CHARS + 1; // plus one check character

/// 16 random bytes shown once to the user as 27 base32 characters in groups
/// of four. The last character is a check character.
pub struct RecoveryKey(pub(crate) [u8; 16]);

impl Drop for RecoveryKey {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.0.zeroize();
    }
}

impl std::fmt::Debug for RecoveryKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryKey(..)")
    }
}

impl RecoveryKey {
    pub fn generate() -> Self {
        RecoveryKey(rand::random::<[u8; 16]>())
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    fn check_char(bytes: &[u8; 16]) -> char {
        let digest = Sha256::digest(bytes);
        ALPHABET.as_bytes()[(digest[0] % 32) as usize] as char
    }

    pub fn encode(&self) -> String {
        let mut raw = base32::encode(base32::Alphabet::Rfc4648 { padding: false }, &self.0);
        debug_assert_eq!(raw.len(), DATA_CHARS);
        raw.push(Self::check_char(&self.0));
        let mut out = String::with_capacity(TOTAL_CHARS + 6);
        for (i, c) in raw.chars().enumerate() {
            if i > 0 && i % 4 == 0 {
                out.push('-');
            }
            out.push(c);
        }
        out
    }

    pub fn parse(text: &str) -> Option<Self> {
        let cleaned: Zeroizing<String> = Zeroizing::new(
            text.chars()
                .filter(|c| !c.is_whitespace() && *c != '-')
                .map(|c| c.to_ascii_uppercase())
                .collect(),
        );
        if cleaned.len() != TOTAL_CHARS || !cleaned.chars().all(|c| ALPHABET.contains(c)) {
            return None;
        }
        // The 26th character carries two padding bits that must be zero,
        // otherwise several texts would map to one key.
        let last = cleaned.as_bytes()[DATA_CHARS - 1] as char;
        if ALPHABET.find(last)? as u8 & 0b11 != 0 {
            return None;
        }
        let (data, check) = cleaned.split_at(DATA_CHARS);
        let decoded = Zeroizing::new(base32::decode(
            base32::Alphabet::Rfc4648 { padding: false },
            data,
        )?);
        let bytes: [u8; 16] = decoded.as_slice().try_into().ok()?;
        if check.chars().next()? != Self::check_char(&bytes) {
            return None;
        }
        Some(RecoveryKey(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_has_27_characters_in_groups_of_four() {
        let key = RecoveryKey([0x5Au8; 16]);
        let text = key.encode();
        assert_eq!(text.len(), 27 + 6, "{text}");
        assert_eq!(
            text.split('-').map(str::len).collect::<Vec<_>>(),
            vec![4, 4, 4, 4, 4, 4, 3]
        );
        assert!(text.chars().all(|c| c == '-' || ALPHABET.contains(c)));
    }

    #[test]
    fn parse_round_trips_and_tolerates_spacing_and_case() {
        let key = RecoveryKey::generate();
        let text = key.encode();
        assert_eq!(
            RecoveryKey::parse(&text).unwrap().as_bytes(),
            key.as_bytes()
        );
        let loose = text.replace('-', " ").to_lowercase();
        assert_eq!(
            RecoveryKey::parse(&loose).unwrap().as_bytes(),
            key.as_bytes()
        );
        assert_eq!(
            RecoveryKey::parse(&text.replace('-', ""))
                .unwrap()
                .as_bytes(),
            key.as_bytes()
        );
    }

    #[test]
    fn parse_rejects_bad_check_character_and_bad_length() {
        let key = RecoveryKey::generate();
        let mut text: Vec<char> = key.encode().chars().collect();
        let last = text.len() - 1;
        text[last] = if text[last] == 'A' { 'B' } else { 'A' };
        let text: String = text.into_iter().collect();
        assert!(RecoveryKey::parse(&text).is_none());
        assert!(RecoveryKey::parse("ABCD-EFGH").is_none());
        assert!(RecoveryKey::parse("").is_none());
        assert!(RecoveryKey::parse("ABCD-EFGH-IJKL-MNOP-QRST-UVWX-Y1Z").is_none());
    }

    #[test]
    fn parse_rejects_non_zero_padding_bits() {
        let key = RecoveryKey([0x5Au8; 16]);
        let mut chars: Vec<char> = key.encode().chars().filter(|c| *c != '-').collect();
        // The 26th data character holds 3 data bits and 2 padding bits; flip the
        // lowest padding bit. The check character still matches the same bytes.
        let index = ALPHABET.find(chars[25]).unwrap();
        chars[25] = ALPHABET.as_bytes()[index ^ 1] as char;
        let variant: String = chars.into_iter().collect();
        assert_ne!(variant, key.encode().replace('-', ""));
        assert!(RecoveryKey::parse(&variant).is_none());
        assert!(RecoveryKey::parse(&key.encode()).is_some());
    }

    #[test]
    fn generate_is_random() {
        assert_ne!(
            RecoveryKey::generate().as_bytes(),
            RecoveryKey::generate().as_bytes()
        );
    }
}
