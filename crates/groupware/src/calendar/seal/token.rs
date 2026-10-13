/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Attendee token pairs (invitations spec 3.3). One random 16-byte token per
//! attendee per event authenticates that attendee's replies. Pairs travel
//! only inside sealed `X-ZA-EXTRA` bundles: all of the organizer's pairs on
//! the organizer's event, the attendee's own pair on the attendee's copy and
//! on a pending REQUEST or CANCEL item. Never logged.

use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

/// Length of one attendee token.
pub const TOKEN_LEN: usize = 16;

/// (attendee address, token) pairs, zeroed on drop, never printed. Addresses
/// compare ASCII case-insensitively; callers store them as they match
/// attendees.
#[derive(Clone, Default)]
pub struct TokenPairs(Zeroizing<Vec<(String, [u8; TOKEN_LEN])>>);

impl TokenPairs {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn from_vec(pairs: Vec<(String, [u8; TOKEN_LEN])>) -> Self {
        TokenPairs(Zeroizing::new(pairs))
    }

    /// A plain copy for serialization; the caller zeroes it after use.
    pub(crate) fn to_vec(&self) -> Vec<(String, [u8; TOKEN_LEN])> {
        self.0.to_vec()
    }

    fn position(&self, address: &str) -> Option<usize> {
        self.0
            .iter()
            .position(|(a, _)| a.eq_ignore_ascii_case(address))
    }

    /// Adds the pair of `address`, replacing a stored one (an accepted
    /// REQUEST replaces the attendee's pair, spec 6.4).
    pub fn insert(&mut self, address: &str, token: [u8; TOKEN_LEN]) {
        match self.position(address) {
            Some(index) => self.0[index].1 = token,
            None => self.0.push((address.to_string(), token)),
        }
    }

    pub fn get(&self, address: &str) -> Option<&[u8; TOKEN_LEN]> {
        self.position(address).map(|index| &self.0[index].1)
    }

    /// Spec 3.3, "Verification": whether `token` is the token of `address`,
    /// compared in constant time. False when `address` has no pair.
    pub fn matches(&self, address: &str, token: &[u8; TOKEN_LEN]) -> bool {
        self.get(address)
            .is_some_and(|stored| tokens_equal(stored, token))
    }

    /// The pair of `address` alone: what an item sent to that attendee
    /// carries (spec 3.3, "Attendee side").
    pub fn only(&self, address: &str) -> Option<TokenPairs> {
        self.position(address)
            .map(|index| TokenPairs::from_vec(vec![self.0[index].clone()]))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for TokenPairs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TokenPairs({})", self.0.len())
    }
}

/// Constant-time equality of two tokens.
pub fn tokens_equal(a: &[u8; TOKEN_LEN], b: &[u8; TOKEN_LEN]) -> bool {
    a.ct_eq(b).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs() -> TokenPairs {
        let mut pairs = TokenPairs::new();
        pairs.insert("Bob@Example.com", [1; TOKEN_LEN]);
        pairs.insert("carol@example.com", [2; TOKEN_LEN]);
        pairs
    }

    #[test]
    fn pairs_are_found_by_address_and_replaced() {
        assert!(TokenPairs::new().is_empty());
        let mut pairs = pairs();
        assert_eq!(pairs.len(), 2);
        assert_eq!(
            pairs.get("bob@example.com"),
            Some(&[1; TOKEN_LEN]),
            "addresses compare ASCII case-insensitively"
        );
        pairs.insert("bob@example.com", [3; TOKEN_LEN]);
        assert_eq!(pairs.len(), 2, "a second insert replaces");
        assert_eq!(pairs.get("BOB@example.com"), Some(&[3; TOKEN_LEN]));
        assert!(pairs.get("dave@example.com").is_none());
    }

    #[test]
    fn matches_compares_the_token_of_that_address() {
        let pairs = pairs();
        assert!(pairs.matches("bob@example.com", &[1; TOKEN_LEN]));
        assert!(!pairs.matches("bob@example.com", &[2; TOKEN_LEN]));
        assert!(!pairs.matches("carol@example.com", &[1; TOKEN_LEN]));
        assert!(!pairs.matches("dave@example.com", &[1; TOKEN_LEN]));
        for i in 0..TOKEN_LEN {
            let mut token = [1; TOKEN_LEN];
            token[i] ^= 0x80;
            assert!(!pairs.matches("bob@example.com", &token), "byte {i}");
        }
        assert!(tokens_equal(&[7; TOKEN_LEN], &[7; TOKEN_LEN]));
        assert!(!tokens_equal(&[7; TOKEN_LEN], &[8; TOKEN_LEN]));
    }

    #[test]
    fn only_keeps_one_attendees_pair() {
        let only = pairs().only("CAROL@example.com").unwrap();
        assert_eq!(only.len(), 1);
        assert_eq!(only.get("carol@example.com"), Some(&[2; TOKEN_LEN]));
        assert!(only.get("bob@example.com").is_none());
        assert!(pairs().only("dave@example.com").is_none());
    }

    #[test]
    fn debug_prints_no_address_or_token() {
        let mut pairs = TokenPairs::new();
        pairs.insert("secret-address@example.com", [0xAB; TOKEN_LEN]);
        let text = format!("{pairs:?}");
        assert_eq!(text, "TokenPairs(1)");
    }
}
