/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Sealing of scheduling inbox items (invitations spec 3.1, 5.3, 6.4 step 5,
//! 6.6, 9).
//!
//! An item's tree is sealed by `seal_tree` under the scope `itip` with a
//! fresh DEK, in both of its states:
//! - pending: the DEK is wrapped to the recipient's public key (`pk`), with
//!   associated data naming the recipient and the UID. The root may carry
//!   the recipient's token pair in a sealed `X-ZA-EXTRA` bound to the same
//!   recipient and UID (a REQUEST or CANCEL), or the replier's token as the
//!   visible `X-ZA-RSVP-TOKEN` (a REPLY). Written by the fan-out (plan 9),
//!   opened only by the drain (plan 10, invariant 11).
//! - drained: resealed under the owner's event wrapping key (`mk`), with
//!   neither carrier. Written by the drain, read by the inbox views.
//!
//! Carrier order on the root, as its last entries: `X-ZA-SEALED` (if any),
//! then `X-ZA-RSVP-TOKEN` or `X-ZA-EXTRA` (pending only, at most one), then
//! `X-ZA-KEY`. Any of them anywhere else is a stray carrier.

use super::{
    event::{open_extra, seal_extra},
    policy::POLICY_VERSION,
    token::{TOKEN_LEN, TokenPairs},
    tree::{
        EXTRA_PROP, KEY_PROP, SealError, WRAP_PK, entry_text, has_stray_carriers, is_carrier,
        open_key_envelope, open_pk_envelope, seal_key_envelope, seal_pk_envelope, seal_tree,
        text_entry, unseal_tree,
    },
};
use crate::calendar::{
    ArchivedCalendarEventNotification, CalendarEventNotification, EVENT_NOTIFICATION_ZA_PENDING,
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use calcard::icalendar::{ICalendar, ICalendarComponentType};
use store::{
    Deserialize, Serialize,
    write::{AlignedBytes, Archive, Archiver},
};
use types::dead_property::DeadProperty;
use vault::{
    keys::{AAD_PK_DEK, PUBLIC_KEY_LEN, Secret, aad},
    session::SessionKeys,
};
use zeroize::Zeroizing;

/// Associated-data scope of an item's bundles, pending and drained (spec 9).
pub const ITIP_SCOPE: &str = "itip";
/// The replier's token on a pending reply item: visible, base64url without
/// padding (spec 3.3, 9).
pub const RSVP_TOKEN_PROP: &str = "X-ZA-RSVP-TOKEN";
/// Purpose of a drained item's key envelope (spec 9).
pub const AAD_ITIP_KEY: &str = "itip-key";
/// Purpose of a pending item's `X-ZA-EXTRA` bundle.
const AAD_ITIP_EXTRA: &str = "itip-extra";

/// What a pending item carries besides its tree. No `Debug`: tokens.
pub struct PendingCarriers {
    /// The recipient's own pair (a REQUEST or CANCEL).
    pub pair: Option<TokenPairs>,
    /// The replier's token (a REPLY).
    pub reply_token: Option<Zeroizing<[u8; TOKEN_LEN]>>,
}

fn uid_of(ical: &ICalendar) -> String {
    ical.uids().next().unwrap_or_default().to_string()
}

/// `za/v1|itip-key|<owner>|<uid>`: the event-key shape (spec 9).
fn drained_key_aad(account_id: u32, uid: &str) -> Vec<u8> {
    let mut out = aad(AAD_ITIP_KEY, account_id);
    out.push(b'|');
    out.extend_from_slice(uid.as_bytes());
    out
}

/// `za/v1|pk-dek|<recipient>|<uid>`: a pending item's key envelope, in the
/// event-key shape (spec 9).
fn pending_key_aad(account_id: u32, uid: &str) -> Vec<u8> {
    let mut out = aad(AAD_PK_DEK, account_id);
    out.push(b'|');
    out.extend_from_slice(uid.as_bytes());
    out
}

/// `za/v1|itip-extra|<recipient>|<uid>`: a pending item's `X-ZA-EXTRA`
/// bundle, bound like an event's (spec 9).
fn extra_aad(account_id: u32, uid: &str) -> Vec<u8> {
    let mut out = aad(AAD_ITIP_EXTRA, account_id);
    out.push(b'|');
    out.extend_from_slice(uid.as_bytes());
    out
}

fn check_root(ical: &ICalendar) -> Result<(), SealError> {
    match ical.components.first() {
        Some(root) if root.component_type == ICalendarComponentType::VCalendar => Ok(()),
        Some(_) => Err(SealError::Structure("root is not VCALENDAR")),
        None => Err(SealError::Structure("empty tree")),
    }
}

/// True if the root's last entry is a key envelope (either state).
pub fn item_is_sealed(ical: &ICalendar) -> bool {
    ical.components
        .first()
        .and_then(|root| root.entries.last())
        .is_some_and(|e| is_carrier(e, KEY_PROP))
}

/// The pending bit (spec 9) of a stored item.
pub fn archived_item_is_pending(item: &ArchivedCalendarEventNotification) -> bool {
    item.flags.to_native() & EVENT_NOTIFICATION_ZA_PENDING != 0
}

/// Pops the root's key envelope and decodes it.
fn pop_envelope(ical: &mut ICalendar) -> Result<Vec<u8>, SealError> {
    if !item_is_sealed(ical) {
        return Err(SealError::NotSealed);
    }
    let key_entry = ical.components[0]
        .entries
        .pop()
        .ok_or(SealError::Structure("key envelope"))?;
    STANDARD
        .decode(entry_text(&key_entry).ok_or(SealError::Format)?)
        .map_err(|_| SealError::Decode)
}

/// Seals a plaintext item tree for `recipient_id` in its pending form (spec
/// 5.3 steps 1-3). `pair` is the recipient's own pair (a REQUEST or
/// CANCEL), `reply_token` the replier's token (a REPLY); never both. A
/// refused public key leaves the tree unchanged.
pub fn seal_pending_item(
    ical: &mut ICalendar,
    recipient_id: u32,
    recipient_public: &[u8; PUBLIC_KEY_LEN],
    pair: Option<&TokenPairs>,
    reply_token: Option<&[u8; TOKEN_LEN]>,
) -> Result<(), SealError> {
    check_root(ical)?;
    if pair.is_some() && reply_token.is_some() {
        return Err(SealError::Structure("a pair and a reply token"));
    }
    if pair.is_some_and(|pair| pair.len() != 1) {
        return Err(SealError::Structure("an item carries one pair"));
    }
    let uid = uid_of(ical);
    let dek = Secret::random();
    let envelope = seal_pk_envelope(&dek, recipient_public, &pending_key_aad(recipient_id, &uid))?;
    seal_tree(ical, &dek, recipient_id, ITIP_SCOPE);
    let root = &mut ical.components[0].entries;
    if let Some(text) = pair.and_then(|pair| {
        seal_extra(
            &dek,
            &extra_aad(recipient_id, &uid),
            None,
            DeadProperty::default(),
            Some(pair),
        )
    }) {
        root.push(text_entry(EXTRA_PROP, text));
    }
    if let Some(token) = reply_token {
        root.push(text_entry(RSVP_TOKEN_PROP, URL_SAFE_NO_PAD.encode(token)));
    }
    root.push(text_entry(KEY_PROP, STANDARD.encode(envelope)));
    Ok(())
}

/// Opens a pending item in place with the recipient's private key (the
/// drain, spec 6.4 step 1) and returns what it carries. On `Err` the tree
/// must be discarded.
pub fn open_pending_item(
    ical: &mut ICalendar,
    recipient_id: u32,
    private_key: &Secret,
) -> Result<PendingCarriers, SealError> {
    check_root(ical)?;
    // UID is a visible property of every component; the root pops below do
    // not change it.
    let uid = uid_of(ical);
    let envelope = pop_envelope(ical)?;
    let dek = open_pk_envelope(&envelope, private_key, &pending_key_aad(recipient_id, &uid))?;
    let root = &mut ical.components[0].entries;
    let extra = root.pop_if(|e| is_carrier(e, EXTRA_PROP));
    let reply = if extra.is_none() {
        root.pop_if(|e| is_carrier(e, RSVP_TOKEN_PROP))
    } else {
        None
    };
    if has_stray_carriers(ical) {
        return Err(SealError::Structure("misplaced carrier"));
    }
    let pair = match extra {
        Some(carrier) => {
            let pairs = open_extra(
                &dek,
                &extra_aad(recipient_id, &uid),
                entry_text(&carrier).ok_or(SealError::Format)?,
            )?
            .pairs;
            // As on the seal side: a bundle holds exactly one pair.
            if pairs.as_ref().map_or(0, |pairs| pairs.len()) != 1 {
                return Err(SealError::Structure("an item carries one pair"));
            }
            pairs
        }
        None => None,
    };
    let reply_token = match reply {
        Some(carrier) => {
            let raw = Zeroizing::new(
                URL_SAFE_NO_PAD
                    .decode(entry_text(&carrier).ok_or(SealError::Format)?)
                    .map_err(|_| SealError::Decode)?,
            );
            Some(Zeroizing::new(
                <[u8; TOKEN_LEN]>::try_from(raw.as_slice()).map_err(|_| SealError::Format)?,
            ))
        }
        None => None,
    };
    unseal_tree(ical, &dek, recipient_id, ITIP_SCOPE)?;
    Ok(PendingCarriers { pair, reply_token })
}

/// Reseals a plaintext item tree under the owner's keys: the drained form
/// (spec 6.4 step 5), with neither `X-ZA-EXTRA` nor `X-ZA-RSVP-TOKEN`.
pub fn seal_drained_item(
    ical: &mut ICalendar,
    keys: &SessionKeys,
    owner_id: u32,
) -> Result<(), SealError> {
    check_root(ical)?;
    if keys.account_id != owner_id {
        return Err(SealError::Structure("keys of another account"));
    }
    let uid = uid_of(ical);
    let dek = Secret::random();
    seal_tree(ical, &dek, owner_id, ITIP_SCOPE);
    let envelope = seal_key_envelope(&dek, keys.ewk(), &drained_key_aad(owner_id, &uid));
    ical.components[0]
        .entries
        .push(text_entry(KEY_PROP, STANDARD.encode(envelope)));
    Ok(())
}

/// Opens a drained item in place (the inbox views). A pending (`pk`) item
/// is `Structure("pending item")`: only the drain opens it (spec 10). A tree
/// without a key envelope is plaintext and stays unchanged. On `Err` the
/// tree must be discarded.
pub fn open_drained_item(
    ical: &mut ICalendar,
    keys: &SessionKeys,
    owner_id: u32,
) -> Result<(), SealError> {
    if !item_is_sealed(ical) {
        return Ok(());
    }
    let envelope = pop_envelope(ical)?;
    if matches!(envelope.as_slice(), [POLICY_VERSION, WRAP_PK, ..]) {
        return Err(SealError::Structure("pending item"));
    }
    let uid = uid_of(ical);
    let dek = open_key_envelope(&envelope, keys.ewk(), &drained_key_aad(owner_id, &uid))?;
    if has_stray_carriers(ical) {
        return Err(SealError::Structure("misplaced carrier"));
    }
    unseal_tree(ical, &dek, owner_id, ITIP_SCOPE)
}

/// Spec 6.6: the read view of a stored item, with `unseal_event_archive`'s
/// contract: `version` copied from the stored archive, read-only, never
/// written back. A plaintext item yields a copy of `stored`.
pub fn unseal_item_archive(
    stored: &Archive<AlignedBytes>,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<Archive<AlignedBytes>, SealError> {
    let mut item = stored
        .deserialize::<CalendarEventNotification>()
        .map_err(|_| SealError::Format)?;
    if !item_is_sealed(&item.event) {
        return Ok(stored.clone());
    }
    open_drained_item(&mut item.event, keys, account_id)?;
    let bytes = Archiver::new(item)
        .serialize()
        .map_err(|_| SealError::Format)?;
    let mut view = <Archive<AlignedBytes> as Deserialize>::deserialize(&bytes)
        .map_err(|_| SealError::Format)?;
    view.version = stored.version;
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{
        CalendarEvent, CalendarEventData,
        seal::{
            event::{seal_event, unseal_event},
            tree::SEALED_PROP,
        },
    };
    use calcard::{
        Entry, Parser,
        common::timezone::Tz,
        icalendar::{ICalendarEntry, ICalendarProperty, ICalendarValue},
    };
    use vault::keys::{WRAP_LEN, generate_keypair};

    const ITEM: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REQUEST\r\nX-WR-CALNAME:item-root-canary\r\nBEGIN:VEVENT\r\nUID:item-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSEQUENCE:0\r\nSUMMARY:item-summary-canary\r\nORGANIZER;CN=Org:mailto:org@example.com\r\nATTENDEE;CN=Att;PARTSTAT=NEEDS-ACTION:mailto:att@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    fn tree() -> ICalendar {
        match Parser::new(ITEM).entry() {
            Entry::ICalendar(ical) => ical,
            other => panic!("{other:?}"),
        }
    }

    fn root(ical: &mut ICalendar) -> &mut Vec<ICalendarEntry> {
        &mut ical.components[0].entries
    }

    fn pair() -> TokenPairs {
        let mut pairs = TokenPairs::new();
        pairs.insert("att@example.com", [7; TOKEN_LEN]);
        pairs
    }

    fn keys() -> SessionKeys {
        SessionKeys::new(9, 1, Secret::random())
    }

    /// A pending item for account 9 and the private key that opens it.
    fn pending(pair: Option<&TokenPairs>, reply: Option<&[u8; TOKEN_LEN]>) -> (ICalendar, Secret) {
        let (public, private) = generate_keypair();
        let mut ical = tree();
        seal_pending_item(&mut ical, 9, &public, pair, reply).unwrap();
        (ical, private)
    }

    fn envelope_of(ical: &ICalendar) -> Vec<u8> {
        STANDARD
            .decode(entry_text(ical.components[0].entries.last().unwrap()).unwrap())
            .unwrap()
    }

    fn archive_of(item: CalendarEventNotification) -> Archive<AlignedBytes> {
        let bytes = Archiver::new(item).serialize().unwrap();
        <Archive<AlignedBytes> as Deserialize>::deserialize(&bytes).unwrap()
    }

    #[test]
    fn pending_item_with_a_pair_round_trips() {
        let (sealed, private) = pending(Some(&pair()), None);
        let dump = sealed.to_string();
        for canary in [
            "item-root-canary",
            "item-summary-canary",
            "att@example.com",
            "org@example.com",
        ] {
            assert!(!dump.contains(canary), "{canary} in {dump}");
        }
        let entries = &sealed.components[0].entries;
        let n = entries.len();
        assert!(is_carrier(&entries[n - 1], KEY_PROP));
        assert!(is_carrier(&entries[n - 2], EXTRA_PROP));
        assert!(is_carrier(&entries[n - 3], SEALED_PROP));
        let envelope = envelope_of(&sealed);
        assert_eq!(envelope.len(), 2 + PUBLIC_KEY_LEN + WRAP_LEN);
        assert_eq!(&envelope[..2], &[POLICY_VERSION, WRAP_PK]);
        let mut opened = sealed.clone();
        let carriers = open_pending_item(&mut opened, 9, &private).unwrap();
        assert_eq!(opened, tree());
        let pair = carriers.pair.expect("pair");
        assert_eq!(pair.len(), 1);
        assert!(pair.matches("att@example.com", &[7; TOKEN_LEN]));
        assert!(carriers.reply_token.is_none());
    }

    #[test]
    fn pending_reply_item_carries_its_token_visibly() {
        let token = [3u8; TOKEN_LEN];
        let (sealed, private) = pending(None, Some(&token));
        let entries = &sealed.components[0].entries;
        let n = entries.len();
        assert!(is_carrier(&entries[n - 2], RSVP_TOKEN_PROP));
        assert_eq!(
            entry_text(&entries[n - 2]),
            Some(URL_SAFE_NO_PAD.encode(token).as_str())
        );
        let mut opened = sealed.clone();
        let carriers = open_pending_item(&mut opened, 9, &private).unwrap();
        assert_eq!(opened, tree());
        assert_eq!(carriers.reply_token.as_deref(), Some(&token));
        assert!(carriers.pair.is_none());
        // Neither carrier: the root ends with its bundle, then the key.
        let (bare, private) = pending(None, None);
        let n = bare.components[0].entries.len();
        assert!(is_carrier(&bare.components[0].entries[n - 2], SEALED_PROP));
        let carriers = open_pending_item(&mut bare.clone(), 9, &private).unwrap();
        assert!(carriers.pair.is_none() && carriers.reply_token.is_none());
    }

    #[test]
    fn pending_item_opens_only_with_the_recipients_private_key() {
        let (sealed, private) = pending(Some(&pair()), None);
        let (_, other) = generate_keypair();
        assert_eq!(
            open_pending_item(&mut sealed.clone(), 9, &other).err(),
            Some(SealError::Aead)
        );
        assert_eq!(
            open_pending_item(&mut sealed.clone(), 10, &private).err(),
            Some(SealError::Aead)
        );
        // The master-key view refuses it: only the drain opens it (invariant 11).
        assert_eq!(
            open_drained_item(&mut sealed.clone(), &keys(), 9),
            Err(SealError::Structure("pending item"))
        );
    }

    #[test]
    fn pending_item_carriers_out_of_order_are_stray() {
        let (sealed, private) = pending(Some(&pair()), None);
        let n = sealed.components[0].entries.len();
        // [.., SEALED, EXTRA, KEY] -> [.., EXTRA, SEALED, KEY].
        let mut swapped = sealed.clone();
        root(&mut swapped).swap(n - 3, n - 2);
        // EXTRA moved into the VEVENT, ahead of that component's SEALED.
        let mut moved = sealed.clone();
        let extra = root(&mut moved).remove(n - 2);
        let vevent = &mut moved.components[1].entries;
        let at = vevent.len() - 1;
        vevent.insert(at, extra);
        // A reply token next to the pair: [.., SEALED, RSVP, EXTRA, KEY].
        let mut both = sealed.clone();
        root(&mut both).insert(
            n - 2,
            text_entry(RSVP_TOKEN_PROP, URL_SAFE_NO_PAD.encode([1u8; TOKEN_LEN])),
        );
        // A reply token after the pair: [.., SEALED, EXTRA, RSVP, KEY].
        let mut after = sealed.clone();
        root(&mut after).insert(
            n - 1,
            text_entry(RSVP_TOKEN_PROP, URL_SAFE_NO_PAD.encode([1u8; TOKEN_LEN])),
        );
        // A reply item's token moved into its VEVENT.
        let (reply, reply_private) = pending(None, Some(&[3; TOKEN_LEN]));
        let m = reply.components[0].entries.len();
        let mut reply_moved = reply.clone();
        let token = root(&mut reply_moved).remove(m - 2);
        reply_moved.components[1].entries.insert(0, token);
        for (name, mut item, key) in [
            ("swapped", swapped, &private),
            ("moved", moved, &private),
            ("both", both, &private),
            ("after", after, &private),
            ("reply moved", reply_moved, &reply_private),
        ] {
            assert_eq!(
                open_pending_item(&mut item, 9, key).err(),
                Some(SealError::Structure("misplaced carrier")),
                "{name}"
            );
        }
        // The key envelope not last: not a sealed item.
        let mut key_not_last = sealed.clone();
        let key = root(&mut key_not_last).pop().unwrap();
        root(&mut key_not_last).insert(n - 2, key);
        assert_eq!(
            open_pending_item(&mut key_not_last, 9, &private).err(),
            Some(SealError::NotSealed)
        );
    }

    #[test]
    fn an_item_carries_one_pair_or_a_token_and_needs_a_usable_key() {
        let (public, _) = generate_keypair();
        let mut two = pair();
        two.insert("other@example.com", [8; TOKEN_LEN]);
        let mut ical = tree();
        assert_eq!(
            seal_pending_item(&mut ical, 9, &public, Some(&two), None),
            Err(SealError::Structure("an item carries one pair"))
        );
        assert_eq!(
            seal_pending_item(&mut ical, 9, &public, Some(&pair()), Some(&[1; TOKEN_LEN])),
            Err(SealError::Structure("a pair and a reply token"))
        );
        assert_eq!(
            seal_pending_item(&mut ical, 9, &[0u8; PUBLIC_KEY_LEN], None, None),
            Err(SealError::Structure("recipient public key"))
        );
        assert_eq!(ical, tree(), "a refused seal leaves the tree untouched");
    }

    /// A pending item for account 9 whose `X-ZA-EXTRA` is `extra`'s bundle,
    /// built by hand: `seal_pending_item` writes only one-pair bundles.
    fn pending_with_extra(
        public: &[u8; PUBLIC_KEY_LEN],
        extra: impl FnOnce(&Secret, &[u8]) -> Option<String>,
    ) -> ICalendar {
        let mut ical = tree();
        let uid = uid_of(&ical);
        let dek = Secret::random();
        let envelope = seal_pk_envelope(&dek, public, &pending_key_aad(9, &uid)).unwrap();
        seal_tree(&mut ical, &dek, 9, ITIP_SCOPE);
        let text = extra(&dek, &extra_aad(9, &uid)).expect("a bundle");
        root(&mut ical).push(text_entry(EXTRA_PROP, text));
        root(&mut ical).push(text_entry(KEY_PROP, STANDARD.encode(envelope)));
        ical
    }

    #[test]
    fn pending_item_bundle_must_hold_exactly_one_pair() {
        let (public, private) = generate_keypair();
        let mut two = pair();
        two.insert("other@example.com", [8; TOKEN_LEN]);
        let two_pairs = pending_with_extra(&public, |dek, aad| {
            seal_extra(dek, aad, None, DeadProperty::default(), Some(&two))
        });
        // A format 1 bundle (no pair list) under the item's purpose.
        let no_pairs = pending_with_extra(&public, |dek, aad| {
            seal_extra(dek, aad, Some("name".into()), DeadProperty::default(), None)
        });
        for (name, mut item) in [("two pairs", two_pairs), ("no pair list", no_pairs)] {
            assert_eq!(
                open_pending_item(&mut item, 9, &private).err(),
                Some(SealError::Structure("an item carries one pair")),
                "{name}"
            );
        }
    }

    #[test]
    fn drained_item_round_trips_under_the_owners_keys_only() {
        let keys = keys();
        let mut sealed = tree();
        seal_drained_item(&mut sealed, &keys, 9).unwrap();
        let entries = &sealed.components[0].entries;
        let n = entries.len();
        assert!(is_carrier(&entries[n - 1], KEY_PROP) && is_carrier(&entries[n - 2], SEALED_PROP));
        assert!(
            !entries
                .iter()
                .any(|e| is_carrier(e, EXTRA_PROP) || is_carrier(e, RSVP_TOKEN_PROP))
        );
        assert_eq!(envelope_of(&sealed)[1], 1, "wrap type mk");
        let mut opened = sealed.clone();
        open_drained_item(&mut opened, &keys, 9).unwrap();
        assert_eq!(opened, tree());
        assert_eq!(
            open_drained_item(
                &mut sealed.clone(),
                &SessionKeys::new(9, 1, Secret::random()),
                9
            ),
            Err(SealError::Aead)
        );
        assert_eq!(
            open_drained_item(&mut sealed.clone(), &keys, 10),
            Err(SealError::Aead)
        );
        assert_eq!(
            seal_drained_item(&mut tree(), &SessionKeys::new(10, 1, Secret::random()), 9),
            Err(SealError::Structure("keys of another account"))
        );
    }

    #[test]
    fn drained_item_with_a_pending_carrier_is_refused() {
        let keys = keys();
        let mut sealed = tree();
        seal_drained_item(&mut sealed, &keys, 9).unwrap();
        let n = sealed.components[0].entries.len();
        for carrier in [RSVP_TOKEN_PROP, EXTRA_PROP] {
            let mut item = sealed.clone();
            root(&mut item).insert(n - 1, text_entry(carrier, "AA".into()));
            assert_eq!(
                open_drained_item(&mut item, &keys, 9),
                Err(SealError::Structure("misplaced carrier")),
                "{carrier}"
            );
        }
    }

    /// Rewrites the VEVENT's UID, as a store writer moving an item's sealed
    /// parts onto another UID's record would.
    fn with_uid(ical: &ICalendar, uid: &str) -> ICalendar {
        let mut moved = ical.clone();
        let entry = moved.components[1]
            .entries
            .iter_mut()
            .find(|e| e.name == ICalendarProperty::Uid)
            .expect("UID entry");
        entry.values = vec![ICalendarValue::Text(uid.to_string())];
        moved
    }

    #[test]
    fn an_item_moved_onto_another_uid_does_not_open() {
        // Pending: the envelope binds the recipient and the UID. The extra
        // bundle binds them too, but that binding is not separately
        // observable through `open_pending_item`, because the envelope fails
        // first; it is belt and braces.
        let (sealed, private) = pending(Some(&pair()), None);
        assert_eq!(
            open_pending_item(&mut with_uid(&sealed, "item-2"), 9, &private).err(),
            Some(SealError::Aead)
        );
        // Drained: the envelope binds the owner and the UID.
        let keys = keys();
        let mut drained = tree();
        seal_drained_item(&mut drained, &keys, 9).unwrap();
        assert_eq!(
            open_drained_item(&mut with_uid(&drained, "item-2"), &keys, 9),
            Err(SealError::Aead)
        );
        // Control: the unmoved items open with the same keys.
        assert!(open_pending_item(&mut sealed.clone(), 9, &private).is_ok());
        assert!(open_drained_item(&mut drained.clone(), &keys, 9).is_ok());
    }

    #[test]
    fn itip_scope_and_event_scope_do_not_mix() {
        // Bundles: the same DEK under the other scope does not open.
        let dek = Secret::random();
        let mut as_item = tree();
        seal_tree(&mut as_item, &dek, 9, ITIP_SCOPE);
        assert_eq!(
            unseal_tree(&mut as_item, &dek, 9, "item-1"),
            Err(SealError::Aead)
        );
        let mut as_event = tree();
        seal_tree(&mut as_event, &dek, 9, "item-1");
        assert_eq!(
            unseal_tree(&mut as_event, &dek, 9, ITIP_SCOPE),
            Err(SealError::Aead)
        );
        // Envelopes: a sealed event is not a drained item, nor the reverse.
        let keys = keys();
        let mut event = CalendarEvent {
            data: CalendarEventData::new(tree(), Tz::Floating, 100, &mut None),
            ..Default::default()
        };
        seal_event(&mut event, &keys, 9, None).unwrap();
        assert_eq!(
            open_drained_item(&mut event.data.event.clone(), &keys, 9),
            Err(SealError::Aead)
        );
        let mut drained = tree();
        seal_drained_item(&mut drained, &keys, 9).unwrap();
        let mut as_event = CalendarEvent {
            data: CalendarEventData::new(drained, Tz::Floating, 100, &mut None),
            ..Default::default()
        };
        assert_eq!(unseal_event(&mut as_event, &keys, 9), Err(SealError::Aead));
    }

    #[test]
    fn item_views_pending_drained_and_plaintext() {
        let keys = keys();
        // Pending (Review Focus 3): an unseal failure, never opened here.
        let (public, _) = generate_keypair();
        let mut ical = tree();
        seal_pending_item(&mut ical, 9, &public, None, None).unwrap();
        let stored = archive_of(CalendarEventNotification {
            event: ical,
            flags: EVENT_NOTIFICATION_ZA_PENDING,
            ..Default::default()
        });
        assert!(archived_item_is_pending(
            stored.unarchive::<CalendarEventNotification>().unwrap()
        ));
        assert_eq!(
            unseal_item_archive(&stored, &keys, 9).err(),
            Some(SealError::Structure("pending item"))
        );
        // Plaintext (Review Focus 4): passes through as stored.
        let plain = archive_of(CalendarEventNotification {
            event: tree(),
            ..Default::default()
        });
        assert!(!archived_item_is_pending(
            plain.unarchive::<CalendarEventNotification>().unwrap()
        ));
        let view = unseal_item_archive(&plain, &keys, 9).unwrap();
        assert_eq!(view.as_bytes(), plain.as_bytes());
        assert_eq!(view.version, plain.version);
        // Drained: a plaintext view that keeps the stored version.
        let mut ical = tree();
        seal_drained_item(&mut ical, &keys, 9).unwrap();
        let stored = archive_of(CalendarEventNotification {
            event: ical,
            ..Default::default()
        });
        assert!(!String::from_utf8_lossy(stored.as_bytes()).contains("item-summary-canary"));
        let view = unseal_item_archive(&stored, &keys, 9).unwrap();
        assert_eq!(view.version, stored.version);
        assert_eq!(
            view.deserialize::<CalendarEventNotification>()
                .unwrap()
                .event,
            tree()
        );
    }
}
