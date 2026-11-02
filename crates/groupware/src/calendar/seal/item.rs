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
//!   visible `X-ZA-RSVP-TOKEN` (a REPLY), both written by the fan-out (plan
//!   9); or, on a mail-borne item, the sender's address in a sealed
//!   `X-ZA-SENDER` bound to the same recipient and UID, written at mail
//!   delivery (imip spec 4.1 step 6, 4.2). Opened only by the drain (plan
//!   10, invariant 11).
//! - drained: resealed under the owner's event wrapping key (`mk`), with
//!   none of the three carriers. Written by the drain, read by the inbox
//!   views.
//!
//! Carrier order on the root, as its last entries: `X-ZA-SEALED` (if any),
//! then `X-ZA-RSVP-TOKEN`, `X-ZA-EXTRA` or `X-ZA-SENDER` (pending only, at
//! most one), then `X-ZA-KEY`. Any of them anywhere else, or beside another
//! of the three, is a stray carrier.

use super::{
    event::{open_extra, seal_extra},
    policy::POLICY_VERSION,
    token::{TOKEN_LEN, TokenPairs},
    tree::{
        EXTRA_PROP, KEY_PROP, SealError, WRAP_PK, entry_text, has_stray_carriers, is_carrier,
        open_bytes, open_key_envelope, open_pk_envelope, seal_bytes, seal_key_envelope,
        seal_pk_envelope, seal_tree, text_entry, unseal_tree,
    },
};
use crate::calendar::{
    ArchivedCalendarEventNotification, CalendarEventNotification, CalendarEventNotificationContent,
    ChangedBy, EVENT_NOTIFICATION_PRIVATE, EVENT_NOTIFICATION_ZA_PENDING,
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use calcard::icalendar::{ICalendar, ICalendarComponentType};
use store::{
    Deserialize, Serialize,
    write::{Archive, ArchiveBytes, Archiver},
};
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
/// A mail-borne pending item's sender: its address, sealed with the item's
/// DEK (imip spec 4.2, D4). Only mail delivery writes it.
pub const SENDER_PROP: &str = "X-ZA-SENDER";
/// Purpose of a pending item's `X-ZA-SENDER` bundle (imip spec 4.2).
const AAD_ITIP_SENDER: &str = "itip-sender";

/// What a pending item carries besides its tree. No `Debug`: tokens and an
/// address.
pub struct PendingCarriers {
    /// The recipient's own pair (a REQUEST or CANCEL).
    pub pair: Option<TokenPairs>,
    /// The replier's token (a REPLY).
    pub reply_token: Option<Zeroizing<[u8; TOKEN_LEN]>>,
    /// A mail-borne item's sender address (imip spec 4.2).
    pub sender: Option<String>,
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

/// `za/v1|itip-sender|<recipient>|<uid>`: a mail-borne pending item's
/// `X-ZA-SENDER` bundle, bound like `X-ZA-EXTRA` (imip spec 4.2).
fn sender_aad(recipient_id: u32, uid: &str) -> Vec<u8> {
    let mut out = aad(AAD_ITIP_SENDER, recipient_id);
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

/// The record of a sealed inbox item at Stalwart 1.0 (upgrade ruling 13),
/// built without reading plaintext:
/// - the row: `event_id` the linked event when the caller has one, else
///   `None`; `changed_by` the sender; `calendar_ids` empty, so upstream's
///   reaper (`reap_calendar_notifications`, which selects only rows whose
///   non-empty `calendar_ids` all lie in the deleted calendars) never takes
///   a sealed item; `flags` `EVENT_NOTIFICATION_PRIVATE`, upstream's
///   `NotificationAudience::Members`, so only the owner's members see it
///   (owner-only inbox access), plus `EVENT_NOTIFICATION_ZA_PENDING` while
///   it waits for the drain. `size`, `created`, `modified` and `etag` are
///   left to upstream's builder, which derives them from the sealed content
///   archive (`refresh_from_content`, `GroupwareWrite::full`).
/// - the content: `Itip { message: <the sealed tree>, previous: None,
///   current: None }`; no plaintext snapshot is ever stored.
pub fn sealed_item_record(
    sealed: ICalendar,
    changed_by: ChangedBy,
    event_id: Option<u32>,
    pending: bool,
) -> (CalendarEventNotification, CalendarEventNotificationContent) {
    (
        CalendarEventNotification {
            event_id,
            changed_by,
            calendar_ids: Vec::new(),
            flags: EVENT_NOTIFICATION_PRIVATE
                | if pending {
                    EVENT_NOTIFICATION_ZA_PENDING
                } else {
                    0
                },
            ..Default::default()
        },
        CalendarEventNotificationContent::Itip {
            message: sealed,
            previous: None,
            current: None,
        },
    )
}

/// An iTIP message in the shape upstream queues it (upgrade ruling 94):
/// upstream stores a message as its text (`ItipMessage<String>`) and its
/// ingest reparses it, so the components a builder or the per-recipient
/// split leaves unreferenced by the root (`ICalendarComponent::default()`
/// placeholders) never persist. The fork seals the tree itself, so it takes
/// the same text round-trip before sealing an item. The serialization walks
/// from the root, so the iCalendar an item serves is unchanged: the
/// placeholders were never part of it, and sealing still never adds,
/// removes or reorders a component of the served tree. `None` when the text
/// does not reparse as a VCALENDAR with a component.
pub fn za_item_shape(message: &ICalendar) -> Option<ICalendar> {
    ICalendar::parse(message.to_string())
        .ok()
        .filter(|ical| ical.components.len() > 1 && check_root(ical).is_ok())
}

/// The sealed tree of a stored item's content, if it is one the fork wrote
/// (`Itip` with no snapshots); `None` for any other content.
pub fn item_message(content: &CalendarEventNotificationContent) -> Option<&ICalendar> {
    match content {
        CalendarEventNotificationContent::Itip {
            message,
            previous: None,
            current: None,
        } => Some(message),
        _ => None,
    }
}

/// `item_message`, owned.
pub fn into_item_message(content: CalendarEventNotificationContent) -> Option<ICalendar> {
    match content {
        CalendarEventNotificationContent::Itip {
            message,
            previous: None,
            current: None,
        } => Some(message),
        _ => None,
    }
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
/// CANCEL), `reply_token` the replier's token (a REPLY), `sender` a
/// mail-borne item's sender address (imip spec 4.2); at most one of the
/// three. A refused carrier or public key leaves the tree unchanged.
pub fn seal_pending_item(
    ical: &mut ICalendar,
    recipient_id: u32,
    recipient_public: &[u8; PUBLIC_KEY_LEN],
    pair: Option<&TokenPairs>,
    reply_token: Option<&[u8; TOKEN_LEN]>,
    sender: Option<&str>,
) -> Result<(), SealError> {
    check_root(ical)?;
    if pair.is_some() && reply_token.is_some() {
        return Err(SealError::Structure("a pair and a reply token"));
    }
    if sender.is_some() && (pair.is_some() || reply_token.is_some()) {
        return Err(SealError::Structure("a sender and another carrier"));
    }
    if pair.is_some_and(|pair| pair.len() != 1) {
        return Err(SealError::Structure("an item carries one pair"));
    }
    let uid = uid_of(ical);
    let dek = Secret::random();
    let envelope = seal_pk_envelope(&dek, recipient_public, &pending_key_aad(recipient_id, &uid))?;
    seal_tree(ical, &dek, recipient_id, ITIP_SCOPE);
    let root = &mut ical.components[0].entries;
    if let Some(text) =
        pair.and_then(|pair| seal_extra(&dek, &extra_aad(recipient_id, &uid), None, Some(pair)))
    {
        root.push(text_entry(EXTRA_PROP, text));
    }
    if let Some(token) = reply_token {
        root.push(text_entry(RSVP_TOKEN_PROP, URL_SAFE_NO_PAD.encode(token)));
    }
    if let Some(sender) = sender {
        root.push(text_entry(
            SENDER_PROP,
            seal_bytes(&dek, &sender_aad(recipient_id, &uid), sender.as_bytes()),
        ));
    }
    root.push(text_entry(KEY_PROP, STANDARD.encode(envelope)));
    Ok(())
}

/// Opens a pending item in place with the recipient's private key (the
/// drain, spec 6.4 step 1) and returns what it carries: a pair, a reply
/// token or a sender (imip spec 4.2), at most one. On `Err` the tree must
/// be discarded.
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
    // At most one carrier, popped from just ahead of the key envelope; a
    // second one, or one anywhere else, stays for the stray check.
    let root = &mut ical.components[0].entries;
    let extra = root.pop_if(|e| is_carrier(e, EXTRA_PROP));
    let reply = if extra.is_none() {
        root.pop_if(|e| is_carrier(e, RSVP_TOKEN_PROP))
    } else {
        None
    };
    let sender = if extra.is_none() && reply.is_none() {
        root.pop_if(|e| is_carrier(e, SENDER_PROP))
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
    let sender = match sender {
        Some(carrier) => {
            let plain = open_bytes(
                &dek,
                &sender_aad(recipient_id, &uid),
                entry_text(&carrier).ok_or(SealError::Format)?,
            )?;
            Some(
                std::str::from_utf8(&plain)
                    .map_err(|_| SealError::Format)?
                    .to_string(),
            )
        }
        None => None,
    };
    unseal_tree(ical, &dek, recipient_id, ITIP_SCOPE)?;
    Ok(PendingCarriers {
        pair,
        reply_token,
        sender,
    })
}

/// Reseals a plaintext item tree under the owner's keys: the drained form
/// (spec 6.4 step 5), with none of `X-ZA-EXTRA`, `X-ZA-RSVP-TOKEN` and
/// `X-ZA-SENDER`.
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

/// Spec 6.6: the read view of a stored item's content archive
/// (`CalendarEventNotificationContent`), with `unseal_event_archive`'s
/// contract: `version` copied from the stored archive, read-only, never
/// written back. Content that is not a sealed fork item yields a copy of
/// `stored`.
pub fn unseal_item_archive(
    stored: &Archive<ArchiveBytes>,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<Archive<ArchiveBytes>, SealError> {
    let mut item = stored
        .deserialize::<CalendarEventNotificationContent>()
        .map_err(|_| SealError::Format)?;
    let message = match &mut item {
        CalendarEventNotificationContent::Itip {
            message,
            previous: None,
            current: None,
        } if item_is_sealed(message) => message,
        _ => return Ok(stored.clone()),
    };
    open_drained_item(message, keys, account_id)?;
    let bytes = Archiver::new(item)
        .serialize()
        .map_err(|_| SealError::Format)?;
    let mut view = <Archive<ArchiveBytes> as Deserialize>::deserialize(&bytes)
        .map_err(|_| SealError::Format)?;
    view.version = stored.version;
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{
        CalendarEventContent, CalendarEventData,
        seal::{
            event::{ZaEvent, seal_event, unseal_event},
            tree::{FORMAT_V1, PAD, SEALED_PROP},
        },
    };
    use calcard::{
        Entry, Parser,
        common::timezone::Tz,
        icalendar::{ICalendarEntry, ICalendarProperty, ICalendarValue},
    };
    use vault::keys::{NONCE_LEN, TAG_LEN, WRAP_LEN, derive_public_key, generate_keypair};

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
        seal_pending_item(&mut ical, 9, &public, pair, reply, None).unwrap();
        (ical, private)
    }

    fn envelope_of(ical: &ICalendar) -> Vec<u8> {
        STANDARD
            .decode(entry_text(ical.components[0].entries.last().unwrap()).unwrap())
            .unwrap()
    }

    fn archive_of<T>(item: T) -> Archive<ArchiveBytes>
    where
        T: rkyv::Archive
            + for<'a> rkyv::Serialize<
                rkyv::api::high::HighSerializer<
                    Vec<u8>,
                    rkyv::ser::allocator::ArenaHandle<'a>,
                    rkyv::rancor::Error,
                >,
            > + store::write::ArchiveCompression,
    {
        let bytes = Archiver::new(item).serialize().unwrap();
        <Archive<ArchiveBytes> as Deserialize>::deserialize(&bytes).unwrap()
    }

    /// A stored item as the fan-out or the drain writes it: the row and the
    /// content archives.
    fn stored_item(
        ical: ICalendar,
        pending: bool,
    ) -> (Archive<ArchiveBytes>, Archive<ArchiveBytes>) {
        let (row, content) = sealed_item_record(ical, ChangedBy::PrincipalId(3), None, pending);
        (archive_of(row), archive_of(content))
    }

    #[test]
    fn sealed_item_record_reads_no_plaintext_and_is_owner_only() {
        let (public, _) = generate_keypair();
        let mut ical = tree();
        seal_pending_item(&mut ical, 9, &public, None, None, None).unwrap();
        let (row, content) =
            sealed_item_record(ical.clone(), ChangedBy::PrincipalId(3), Some(5), true);
        assert!(row.calendar_ids.is_empty(), "never reaped by upstream");
        assert_eq!(
            row.flags,
            EVENT_NOTIFICATION_PRIVATE | EVENT_NOTIFICATION_ZA_PENDING
        );
        assert_eq!(
            crate::calendar::notification::NotificationAudience::from_flags(row.flags),
            crate::calendar::notification::NotificationAudience::Members
        );
        assert_eq!(row.event_id, Some(5));
        assert_eq!(item_message(&content), Some(&ical));
        assert!(matches!(
            &content,
            CalendarEventNotificationContent::Itip {
                previous: None,
                current: None,
                ..
            }
        ));
        let (drained, _) = sealed_item_record(ical, ChangedBy::PrincipalId(3), None, false);
        assert_eq!(drained.flags, EVENT_NOTIFICATION_PRIVATE);
        // Only the fork's shape is an item message.
        assert!(
            item_message(&CalendarEventNotificationContent::Itip {
                message: tree(),
                previous: None,
                current: Some(tree()),
            })
            .is_none()
        );
        assert!(item_message(&CalendarEventNotificationContent::default()).is_none());
    }

    #[test]
    fn pending_flag_is_clear_of_upstream_notification_flags() {
        use crate::calendar::{
            EVENT_NOTIFICATION_HIDE_ATTENDEES, EVENT_NOTIFICATION_IS_CHANGE,
            EVENT_NOTIFICATION_IS_DESTROY, EVENT_NOTIFICATION_IS_DIRECT,
            EVENT_NOTIFICATION_IS_DRAFT,
        };
        for flag in [
            EVENT_NOTIFICATION_IS_DRAFT,
            EVENT_NOTIFICATION_IS_CHANGE,
            EVENT_NOTIFICATION_IS_DESTROY,
            EVENT_NOTIFICATION_IS_DIRECT,
            EVENT_NOTIFICATION_PRIVATE,
            EVENT_NOTIFICATION_HIDE_ATTENDEES,
        ] {
            assert_eq!(flag & EVENT_NOTIFICATION_ZA_PENDING, 0);
        }
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
        assert!(carriers.reply_token.is_none() && carriers.sender.is_none());
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
        assert!(carriers.pair.is_none() && carriers.sender.is_none());
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
            seal_pending_item(&mut ical, 9, &public, Some(&two), None, None),
            Err(SealError::Structure("an item carries one pair"))
        );
        assert_eq!(
            seal_pending_item(
                &mut ical,
                9,
                &public,
                Some(&pair()),
                Some(&[1; TOKEN_LEN]),
                None
            ),
            Err(SealError::Structure("a pair and a reply token"))
        );
        assert_eq!(
            seal_pending_item(&mut ical, 9, &[0u8; PUBLIC_KEY_LEN], None, None, None),
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
        let two_pairs =
            pending_with_extra(&public, |dek, aad| seal_extra(dek, aad, None, Some(&two)));
        // A format 1 bundle (no pair list) under the item's purpose.
        let no_pairs = pending_with_extra(&public, |dek, aad| {
            seal_extra(dek, aad, Some("name".into()), None)
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
        for carrier in [RSVP_TOKEN_PROP, EXTRA_PROP, SENDER_PROP] {
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
        entry.values = [ICalendarValue::Text(uid.to_string())].into();
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
        let mut event = ZaEvent {
            content: CalendarEventContent {
                data: CalendarEventData::new(tree(), Tz::Floating, 100),
                ..Default::default()
            },
            ..Default::default()
        };
        seal_event(&mut event, &keys, 9, None).unwrap();
        assert_eq!(
            open_drained_item(&mut event.content.data.event.clone(), &keys, 9),
            Err(SealError::Aead)
        );
        let mut drained = tree();
        seal_drained_item(&mut drained, &keys, 9).unwrap();
        let mut as_event = ZaEvent {
            content: CalendarEventContent {
                data: CalendarEventData::new(drained, Tz::Floating, 100),
                ..Default::default()
            },
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
        seal_pending_item(&mut ical, 9, &public, None, None, None).unwrap();
        let (row, stored) = stored_item(ical, true);
        assert!(archived_item_is_pending(
            row.unarchive::<CalendarEventNotification>().unwrap()
        ));
        assert_eq!(
            unseal_item_archive(&stored, &keys, 9).err(),
            Some(SealError::Structure("pending item"))
        );
        // Plaintext (Review Focus 4): passes through as stored.
        let (row, plain) = stored_item(tree(), false);
        assert!(!archived_item_is_pending(
            row.unarchive::<CalendarEventNotification>().unwrap()
        ));
        let view = unseal_item_archive(&plain, &keys, 9).unwrap();
        assert_eq!(view.as_bytes(), plain.as_bytes());
        assert_eq!(view.version, plain.version);
        // Drained: a plaintext view that keeps the stored version.
        let mut ical = tree();
        seal_drained_item(&mut ical, &keys, 9).unwrap();
        let (_, stored) = stored_item(ical, false);
        assert!(!String::from_utf8_lossy(stored.as_bytes()).contains("item-summary-canary"));
        let view = unseal_item_archive(&stored, &keys, 9).unwrap();
        assert_eq!(view.version, stored.version);
        assert_eq!(
            into_item_message(
                view.deserialize::<CalendarEventNotificationContent>()
                    .unwrap()
            ),
            Some(tree())
        );
        // Upstream's own content (a snapshot) is not opened: as stored.
        let snapshot = archive_of(CalendarEventNotificationContent::Itip {
            message: tree(),
            previous: None,
            current: Some(tree()),
        });
        let view = unseal_item_archive(&snapshot, &keys, 9).unwrap();
        assert_eq!(view.as_bytes(), snapshot.as_bytes());
    }

    /// Upgrade ruling 94: the per-recipient split leaves placeholder
    /// components the root does not reference; `za_item_shape` drops them
    /// as upstream's text round-trip does, serves the same iCalendar, and
    /// the item it seals (pending, then drained) opens to its own reparse.
    #[test]
    fn an_item_takes_upstreams_queued_shape() {
        use crate::scheduling::{
            event_create::itip_create,
            recipient::{AttendeeVisibility, RecipientPolicy},
        };
        let text = concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\n",
            "BEGIN:VEVENT\r\nUID:item-shape\r\nDTSTAMP:20240101T000000Z\r\n",
            "DTSTART:20990301T100000Z\r\nDURATION:PT1H\r\nRRULE:FREQ=DAILY;COUNT=5\r\n",
            "SEQUENCE:0\r\nSUMMARY:master\r\nORGANIZER:mailto:org@example.com\r\n",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:carol@example.com\r\n",
            "END:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:item-shape\r\nDTSTAMP:20240101T000000Z\r\n",
            "RECURRENCE-ID:20990302T100000Z\r\nDTSTART:20990302T100000Z\r\n",
            "DURATION:PT1H\r\nSEQUENCE:0\r\nSUMMARY:x\r\nORGANIZER:mailto:org@example.com\r\n",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:att@example.com\r\n",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:carol@example.com\r\n",
            "END:VEVENT\r\nEND:VCALENDAR\r\n"
        );
        let mut event = ICalendar::parse(text).unwrap();
        let policy = RecipientPolicy {
            visibility: AttendeeVisibility::All,
            max_recipients: usize::MAX,
            max_instances: usize::MAX,
        };
        let messages = itip_create(&mut event, &["org@example.com".into()], policy).unwrap();
        let slice = messages
            .into_iter()
            .find(|message| message.to == ["att@example.com".to_string()])
            .expect("att's slice: X alone")
            .message;
        let referenced = |ical: &ICalendar| {
            let mut seen = vec![false; ical.components.len()];
            let mut stack = vec![0u32];
            while let Some(id) = stack.pop() {
                if !std::mem::replace(&mut seen[id as usize], true) {
                    stack.extend(&ical.components[id as usize].component_ids);
                }
            }
            seen.into_iter().all(|seen| seen)
        };
        assert!(!referenced(&slice), "the split leaves a placeholder");

        let shaped = za_item_shape(&slice).unwrap();
        assert!(referenced(&shaped), "no placeholder");
        assert_eq!(shaped.to_string(), slice.to_string(), "the served text");
        assert_eq!(ICalendar::parse(shaped.to_string()).unwrap(), shaped);

        // Sealed pending, opened, resealed drained, opened: the tree the
        // drain imports and keeps is still its own reparse.
        let (public, private) = generate_keypair();
        let keys = keys();
        let mut item = shaped.clone();
        seal_pending_item(&mut item, 9, &public, Some(&pair()), None, None).unwrap();
        open_pending_item(&mut item, 9, &private).unwrap();
        assert_eq!(item, shaped);
        seal_drained_item(&mut item, &keys, 9).unwrap();
        open_drained_item(&mut item, &keys, 9).unwrap();
        assert_eq!(ICalendar::parse(item.to_string()).unwrap(), item);
    }
    /// Upgrade ruling 94's failure arm (ruling 134, `fanout.rs`'s
    /// `za_write_pending_item`): a message whose text does not reparse as a
    /// VCALENDAR holding a component has no shape, so the fan-out writes no
    /// item and records the recipient's failure. Upstream's builders never
    /// emit one; these are the two shapes the check refuses.
    #[test]
    fn a_message_without_a_component_has_no_shape() {
        let root_only = ICalendar::parse(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        assert_eq!(root_only.components.len(), 1);
        assert!(za_item_shape(&root_only).is_none(), "a root alone");
        assert!(
            za_item_shape(&ICalendar::default()).is_none(),
            "a tree with no root"
        );
    }

    /// The recipient key pair of the sender tests' items: fixed, so an item
    /// built by hand and one `seal_pending_item` wrote open with one key.
    fn private() -> Secret {
        Secret::from_bytes([0x5a; 32])
    }

    fn public() -> [u8; PUBLIC_KEY_LEN] {
        derive_public_key(&private())
    }

    const SENDER: &str = "imip-org@remote.test";

    /// A sealed pending item, its recipient and the private key that opens
    /// it.
    struct Sealed {
        tree: ICalendar,
        private: Secret,
        recipient: u32,
    }

    /// A pending item for `recipient` and `uid` carrying `SENDER`, as
    /// `seal_pending_item` writes it, sealed to `public()`.
    fn sealed_with_sender(recipient: u32, uid: &str) -> Sealed {
        let mut tree = with_uid(&tree(), uid);
        seal_pending_item(&mut tree, recipient, &public(), None, None, Some(SENDER)).unwrap();
        Sealed {
            tree,
            private: private(),
            recipient,
        }
    }

    /// A pending item for `recipient` and `uid` under `dek`, sealed to
    /// `public()` by hand, with the root carriers `carriers` names between
    /// its bundles and its key envelope, in that order, each one well-formed
    /// for that recipient and UID (`seal_pending_item` writes at most one).
    fn hand_seal(recipient: u32, uid: &str, dek: &Secret, carriers: &[&str]) -> ICalendar {
        let mut ical = with_uid(&tree(), uid);
        let envelope = seal_pk_envelope(dek, &public(), &pending_key_aad(recipient, uid)).unwrap();
        seal_tree(&mut ical, dek, recipient, ITIP_SCOPE);
        for name in carriers {
            let text = match *name {
                EXTRA_PROP => {
                    seal_extra(dek, &extra_aad(recipient, uid), None, Some(&pair())).unwrap()
                }
                RSVP_TOKEN_PROP => URL_SAFE_NO_PAD.encode([3u8; TOKEN_LEN]),
                SENDER_PROP => seal_bytes(dek, &sender_aad(recipient, uid), SENDER.as_bytes()),
                other => panic!("not a pending carrier: {other}"),
            };
            root(&mut ical).push(text_entry(name, text));
        }
        root(&mut ical).push(text_entry(KEY_PROP, STANDARD.encode(envelope)));
        ical
    }

    /// `hand_seal` for recipient 9 and `item-1` under a fresh DEK.
    fn hand_built(carriers: &[&str]) -> ICalendar {
        hand_seal(9, "item-1", &Secret::random(), carriers)
    }

    /// Item A's `X-ZA-SENDER` entry moved into an item B for `recipient`
    /// and `uid`, ahead of B's key envelope. B is sealed under A's DEK when
    /// the recipient or the UID differs from A's, so only the carrier's
    /// associated data is wrong in B, and under its own DEK when both are
    /// A's, so only the DEK is.
    fn transplant_sender(a: &Sealed, recipient: u32, uid: &str) -> Sealed {
        let a_uid = uid_of(&a.tree);
        let a_dek = open_pk_envelope(
            &envelope_of(&a.tree),
            &a.private,
            &pending_key_aad(a.recipient, &a_uid),
        )
        .unwrap();
        let dek = if (recipient, uid) == (a.recipient, a_uid.as_str()) {
            Secret::random()
        } else {
            a_dek
        };
        let entries = &a.tree.components[0].entries;
        let carrier = entries[entries.len() - 2].clone();
        assert!(is_carrier(&carrier, SENDER_PROP));
        let mut tree = hand_seal(recipient, uid, &dek, &[]);
        let at = tree.components[0].entries.len() - 1;
        root(&mut tree).insert(at, carrier);
        Sealed {
            tree,
            private: private(),
            recipient,
        }
    }

    /// A sender item whose `X-ZA-SENDER` moved into its VEVENT, ahead of
    /// that component's `X-ZA-SEALED`.
    fn sender_in_vevent() -> ICalendar {
        let mut item = sealed_with_sender(9, "item-1").tree;
        let n = item.components[0].entries.len();
        let sender = root(&mut item).remove(n - 2);
        let vevent = &mut item.components[1].entries;
        let at = vevent.len() - 1;
        vevent.insert(at, sender);
        item
    }

    #[test]
    fn a_sender_carrier_round_trips() {
        let (public, private) = generate_keypair();
        let mut ical = tree();
        seal_pending_item(
            &mut ical,
            9,
            &public,
            None,
            None,
            Some("imip-org@remote.test"),
        )
        .unwrap();
        assert!(
            !ical.to_string().contains("imip-org"),
            "the address is sealed"
        );
        // Imip spec 4.2: [.., X-ZA-SEALED, X-ZA-SENDER, X-ZA-KEY], the
        // sender a format 1 bundle padded to 256 bytes.
        let entries = &ical.components[0].entries;
        let n = entries.len();
        assert!(is_carrier(&entries[n - 1], KEY_PROP));
        assert!(is_carrier(&entries[n - 2], SENDER_PROP));
        assert!(is_carrier(&entries[n - 3], SEALED_PROP));
        let bundle = STANDARD
            .decode(entry_text(&entries[n - 2]).unwrap())
            .unwrap();
        assert_eq!(bundle[0], FORMAT_V1);
        assert_eq!(bundle.len(), 1 + NONCE_LEN + TAG_LEN + PAD);
        let carriers = open_pending_item(&mut ical, 9, &private).unwrap();
        assert_eq!(carriers.sender.as_deref(), Some("imip-org@remote.test"));
        assert!(carriers.pair.is_none() && carriers.reply_token.is_none());
        assert_eq!(ical, tree());
    }

    #[test]
    fn the_sender_carrier_binds_recipient_uid_and_dek() {
        // The `X-ZA-SENDER` entry of item A moved into item B: B for another
        // recipient (same UID), B with another UID (same recipient), and B with
        // the same recipient and UID but its own DEK.
        for (recipient, uid) in [(10, "item-1"), (9, "item-2"), (9, "item-1")] {
            let mut moved = transplant_sender(&sealed_with_sender(9, "item-1"), recipient, uid);
            // Control: B without the moved entry opens, so the entry alone
            // fails.
            let mut control = moved.tree.clone();
            let n = control.components[0].entries.len();
            root(&mut control).remove(n - 2);
            assert!(
                open_pending_item(&mut control, recipient, &moved.private).is_ok(),
                "{recipient} {uid}"
            );
            let opened = open_pending_item(&mut moved.tree, recipient, &moved.private);
            assert!(opened.is_err(), "{recipient} {uid}");
            assert_eq!(opened.err(), Some(SealError::Aead), "{recipient} {uid}");
        }
    }

    #[test]
    fn a_sender_beside_another_carrier_or_out_of_place_is_stray() {
        // Controls: each carrier alone, built by hand, opens.
        for carrier in [EXTRA_PROP, RSVP_TOKEN_PROP, SENDER_PROP] {
            assert!(
                open_pending_item(&mut hand_built(&[carrier]), 9, &private()).is_ok(),
                "{carrier}"
            );
        }
        for carriers in [
            &["X-ZA-EXTRA", "X-ZA-SENDER"][..],
            &["X-ZA-SENDER", "X-ZA-EXTRA"],
            &["X-ZA-RSVP-TOKEN", "X-ZA-SENDER"],
            &["X-ZA-SENDER", "X-ZA-RSVP-TOKEN"],
        ] {
            assert_eq!(
                open_pending_item(&mut hand_built(carriers), 9, &private()).err(),
                Some(SealError::Structure("misplaced carrier")),
                "{carriers:?}"
            );
        }
        assert_eq!(
            open_pending_item(&mut sender_in_vevent(), 9, &private()).err(),
            Some(SealError::Structure("misplaced carrier"))
        );
        // [.., X-ZA-SENDER, X-ZA-SEALED, X-ZA-KEY].
        let mut swapped = sealed_with_sender(9, "item-1").tree;
        let n = swapped.components[0].entries.len();
        root(&mut swapped).swap(n - 3, n - 2);
        assert_eq!(
            open_pending_item(&mut swapped, 9, &private()).err(),
            Some(SealError::Structure("misplaced carrier"))
        );
        assert_eq!(
            seal_pending_item(
                &mut tree(),
                9,
                &public(),
                Some(&pair()),
                None,
                Some("a@b.test")
            ),
            Err(SealError::Structure("a sender and another carrier"))
        );
        assert_eq!(
            seal_pending_item(
                &mut tree(),
                9,
                &public(),
                None,
                Some(&[1; TOKEN_LEN]),
                Some("a@b.test")
            ),
            Err(SealError::Structure("a sender and another carrier"))
        );
    }

    #[test]
    fn the_drained_form_carries_no_sender() {
        let mut ical = sealed_with_sender(9, "item-1").tree;
        open_pending_item(&mut ical, 9, &private()).unwrap();
        seal_drained_item(&mut ical, &keys(), 9).unwrap();
        assert!(!ical.to_string().contains(SENDER_PROP));
    }
}
