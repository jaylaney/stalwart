/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 5: the fan-out. Under `ItipSendStatus::SendSealed` a key
//! account's own request delivers the iTIP messages upstream's builders made
//! as sealed pending items in the scheduling inboxes of internal key
//! recipients, instead of queueing them as mail (invariant 12). It runs
//! after every check of the request and immediately before the sender's
//! commit (5.4). Each item is its own batch in the recipient's account, and
//! nothing is rolled back if the sender's commit then fails. The
//! address-to-account mapping exists only inside one call (5.2,
//! invariant 13).
//!
//! A recipient is internal when its address resolves to another local
//! account that is a key account, whose account cache entry holds a public
//! key, and that holds `Permission::CalendarSchedulingReceive`. A REQUEST
//! or CANCEL is attempted only when upstream marked it as the organizer's
//! (`from_organizer`); a REPLY only to the event's ORGANIZER, and only when
//! the replier's copy holds its token. Every other recipient is not
//! attempted. `resolve_inner` builds, and so caches, the recipient's access
//! token to read its permissions, as upstream's delivery does for its
//! recipients.

use crate::{
    calendar::{
        CalendarEventNotification, ChangedBy, EVENT_NOTIFICATION_ZA_PENDING,
        seal::{TOKEN_LEN, TokenPairs, seal_error, seal_pending_item},
    },
    scheduling::{
        Email, ItipMessage, ical_size,
        inbound::itip_method,
        itip::{
            ITIP_STATUS_DELIVERED, ITIP_STATUS_DELIVERY_FAILED, ITIP_STATUS_INVALID_USER,
            ITIP_STATUS_NOT_ATTEMPTED,
        },
    },
};
use ahash::AHashMap;
use calcard::icalendar::{
    ICalendar, ICalendarMethod, ICalendarParameter, ICalendarParameterName, ICalendarProperty,
};
use common::{
    Server,
    auth::{AccountInfo, BuildAccessToken},
};
use registry::schema::enums::Permission;
use store::{rand, write::BatchBuilder};
use trc::AddContext;
use types::collection::Collection;
use vault::session::SessionKeys;
use zeroize::Zeroizing;

/// One SCHEDULE-STATUS the fan-out decided (spec 5.2, 5.3, 13). No `Debug`:
/// it holds an address.
#[derive(Clone, PartialEq, Eq)]
pub struct ZaScheduleStatus {
    /// The recipient's address as the message names it.
    pub address: String,
    /// On the ORGANIZER property (a reply to the organizer) instead of the
    /// recipient's ATTENDEE properties.
    pub on_organizer: bool,
    /// `ITIP_STATUS_DELIVERED`, `ITIP_STATUS_NOT_ATTEMPTED` or
    /// `ITIP_STATUS_DELIVERY_FAILED`.
    pub status: &'static str,
}

/// What `za_fan_out` returns to the sender's write path.
pub struct ZaFanOutResult {
    /// The pairs the sender's event bundle keeps (`seal_event(.., pairs)`):
    /// an organizer's union restricted to current attendees, or an
    /// attendee's stored pairs unchanged. `None` when there are none.
    pub pairs: Option<TokenPairs>,
    /// For `za_set_schedule_status` on the sender's plaintext tree, before
    /// it is sealed.
    pub statuses: Vec<ZaScheduleStatus>,
}

/// A CANCEL (organizer) or declined REPLY (attendee) that a key account's
/// DELETE built from the unsealed stored event (spec 5.5), with what the
/// fan-out needs from that event.
pub struct ZaDeleteMessage {
    pub message: ItipMessage<ICalendar>,
    /// The stored event's token pairs.
    pub pairs: Option<TokenPairs>,
    /// The unsealed stored tree.
    pub event: ICalendar,
}

pub trait ZaFanOut: Sync + Send {
    /// Delivers `messages` (upstream's builders' output for `event`, the
    /// sender's plaintext tree) to internal key recipients as pending items.
    /// `sender` is the account whose own request this is and `keys` its
    /// session keys; `stored_pairs` the token pairs of the sender's stored
    /// event (`event_token_pairs`), `None` on a create. A lookup or write
    /// failure for one recipient becomes that recipient's delivery-failed
    /// status (spec 10); the call fails only when `keys` are not the
    /// sender's. Plan 10's drain calls it for the delegation fan-out
    /// (spec 6.4).
    fn za_fan_out(
        &self,
        sender: &AccountInfo,
        keys: &SessionKeys,
        messages: Vec<ItipMessage<ICalendar>>,
        stored_pairs: Option<TokenPairs>,
        event: &ICalendar,
    ) -> impl Future<Output = trc::Result<ZaFanOutResult>> + Send;
}

impl ZaFanOut for Server {
    async fn za_fan_out(
        &self,
        sender: &AccountInfo,
        keys: &SessionKeys,
        messages: Vec<ItipMessage<ICalendar>>,
        stored_pairs: Option<TokenPairs>,
        event: &ICalendar,
    ) -> trc::Result<ZaFanOutResult> {
        let sender_id = sender.account_id();
        if keys.account_id != sender_id {
            return Err(trc::SecurityEvent::Unauthorized
                .into_err()
                .details("zero-access: fan-out without the sender's session keys")
                .account_id(sender_id)
                .caused_by(trc::location!()));
        }
        let organizer = event_organizer(event, sender.addresses());
        let is_organizer = organizer
            .as_ref()
            .is_some_and(|organizer| organizer.is_local);
        // Spec 3.3, "Maintenance": only the organizer hands out tokens; an
        // attendee's copy holds its own pair.
        let union = if is_organizer {
            token_union(stored_pairs.as_ref(), event, sender.addresses())
        } else {
            stored_pairs.clone().unwrap_or_default()
        };

        let mut recipients: AHashMap<String, Recipient> = AHashMap::new();
        let mut statuses = Vec::new();
        for message in &messages {
            let method = itip_method(&message.message).ok();
            for address in &message.to {
                let (on_organizer, status) =
                    match route(method, message, address, organizer.as_ref(), &union) {
                        Route::NotAttempted { on_organizer } => {
                            (on_organizer, ITIP_STATUS_NOT_ATTEMPTED)
                        }
                        Route::Deliver {
                            on_organizer,
                            pair,
                            reply_token,
                        } => {
                            let key = address.to_ascii_lowercase();
                            let recipient = match recipients.get(&key) {
                                Some(recipient) => *recipient,
                                None => {
                                    let recipient =
                                        resolve_recipient(self, sender_id, address).await;
                                    recipients.insert(key, recipient);
                                    recipient
                                }
                            };
                            let status = match recipient {
                                Recipient::Internal {
                                    account_id,
                                    public_key,
                                } => {
                                    write_item(
                                        self,
                                        sender_id,
                                        account_id,
                                        &public_key,
                                        &message.message,
                                        pair.as_ref(),
                                        reply_token.as_deref(),
                                    )
                                    .await
                                }
                                Recipient::NotInternal => ITIP_STATUS_NOT_ATTEMPTED,
                                Recipient::Failed => ITIP_STATUS_DELIVERY_FAILED,
                            };
                            (on_organizer, status)
                        }
                    };
                record_status(&mut statuses, address, on_organizer, status);
            }
        }

        // Spec 3.3: the organizer's bundle keeps the pairs of current
        // attendees only.
        let pairs = if is_organizer {
            Some(current_attendee_pairs(&union, event, sender.addresses()))
        } else {
            stored_pairs
        };
        Ok(ZaFanOutResult {
            pairs: pairs.filter(|pairs| !pairs.is_empty()),
            statuses,
        })
    }
}

/// Writes each status as the SCHEDULE-STATUS of the matching ORGANIZER or
/// ATTENDEE properties of the sender's plaintext tree (RFC 6638 3.2.9),
/// before it is sealed: sealed with the property, shown to the sender's own
/// client. Replaces an earlier fan-out's status; upstream's `3.7` is left
/// to `itip_set_unreachable_status`.
pub fn za_set_schedule_status(ical: &mut ICalendar, statuses: &[ZaScheduleStatus]) {
    if statuses.is_empty() {
        return;
    }
    for comp in ical.components.iter_mut() {
        if !comp.component_type.is_scheduling_object() {
            continue;
        }
        for entry in comp.entries.iter_mut() {
            let on_organizer = match entry.name {
                ICalendarProperty::Organizer => true,
                ICalendarProperty::Attendee => false,
                _ => continue,
            };
            let Some(address) = entry
                .values
                .first()
                .and_then(|value| value.as_text())
                .and_then(|value| Email::new(value, &[]))
            else {
                continue;
            };
            let Some(status) = statuses.iter().find(|status| {
                status.on_organizer == on_organizer
                    && status.address.eq_ignore_ascii_case(&address.email)
            }) else {
                continue;
            };
            entry.params.retain(|param| {
                param.name != ICalendarParameterName::ScheduleStatus
                    || param.value.as_text() == Some(ITIP_STATUS_INVALID_USER)
            });
            entry.params.push(ICalendarParameter::schedule_status(
                status.status.to_string(),
            ));
        }
    }
}

#[derive(Clone, Copy)]
enum Recipient {
    Internal {
        account_id: u32,
        public_key: [u8; 32],
    },
    NotInternal,
    Failed,
}

enum Route {
    NotAttempted {
        on_organizer: bool,
    },
    Deliver {
        on_organizer: bool,
        pair: Option<TokenPairs>,
        reply_token: Option<Zeroizing<[u8; TOKEN_LEN]>>,
    },
}

/// Spec 5.2: whether `recipient` of `message` is attempted, and with which
/// carrier (spec 5.3 step 1).
fn route(
    method: Option<&ICalendarMethod>,
    message: &ItipMessage<ICalendar>,
    recipient: &str,
    organizer: Option<&Email>,
    union: &TokenPairs,
) -> Route {
    let to_organizer =
        organizer.is_some_and(|organizer| organizer.email.eq_ignore_ascii_case(recipient));
    match method {
        // An attendee-originated REQUEST (a delegate invitation) is not
        // attempted: delegates receive their token from the organizer's
        // drain (spec 6.4).
        Some(ICalendarMethod::Request | ICalendarMethod::Cancel) if message.from_organizer => {
            Route::Deliver {
                on_organizer: false,
                pair: union.only(recipient),
                reply_token: None,
            }
        }
        // A REPLY goes to the ORGANIZER only: a delegator's bundle holds no
        // pair to check it against (spec 5.5). Without the replier's own
        // token the organizer's drain would drop it as a forgery (3.3).
        Some(ICalendarMethod::Reply) if to_organizer => match union.get(&message.from) {
            Some(token) => Route::Deliver {
                on_organizer: true,
                pair: None,
                reply_token: Some(Zeroizing::new(*token)),
            },
            None => Route::NotAttempted { on_organizer: true },
        },
        _ => Route::NotAttempted {
            on_organizer: to_organizer,
        },
    }
}

/// The event's ORGANIZER: the first one in a scheduling component,
/// `is_local` when it is one of the sender's addresses.
fn event_organizer(event: &ICalendar, sender_addresses: &[String]) -> Option<Email> {
    event
        .components
        .iter()
        .filter(|comp| comp.component_type.is_scheduling_object())
        .find_map(|comp| {
            comp.entries
                .iter()
                .find(|entry| entry.name == ICalendarProperty::Organizer)
                .and_then(|entry| entry.values.first())
                .and_then(|value| value.as_text())
                .and_then(|value| Email::new(value, sender_addresses))
        })
}

/// Scheduled attendees: the ATTENDEE addresses of the scheduling components
/// other than the sender's own, sanitized and deduplicated.
fn scheduled_attendees(event: &ICalendar, sender_addresses: &[String]) -> Vec<String> {
    let mut attendees: Vec<String> = Vec::new();
    for comp in event
        .components
        .iter()
        .filter(|comp| comp.component_type.is_scheduling_object())
    {
        for entry in comp
            .entries
            .iter()
            .filter(|entry| entry.name == ICalendarProperty::Attendee)
        {
            if let Some(email) = entry
                .values
                .first()
                .and_then(|value| value.as_text())
                .and_then(|value| Email::new(value, sender_addresses))
                && !email.is_local
                && !attendees.contains(&email.email)
            {
                attendees.push(email.email);
            }
        }
    }
    attendees
}

/// Spec 3.3, "Maintenance": the stored pairs plus a fresh token for every
/// scheduled attendee without one.
fn token_union(
    stored: Option<&TokenPairs>,
    event: &ICalendar,
    sender_addresses: &[String],
) -> TokenPairs {
    let mut union = stored.cloned().unwrap_or_default();
    for address in scheduled_attendees(event, sender_addresses) {
        if union.get(&address).is_none() {
            let token = Zeroizing::new(rand::random::<[u8; TOKEN_LEN]>());
            union.insert(&address, *token);
        }
    }
    union
}

/// The union restricted to the event's current attendees.
fn current_attendee_pairs(
    union: &TokenPairs,
    event: &ICalendar,
    sender_addresses: &[String],
) -> TokenPairs {
    let mut pairs = TokenPairs::new();
    for address in scheduled_attendees(event, sender_addresses) {
        if let Some(token) = union.get(&address) {
            pairs.insert(&address, *token);
        }
    }
    pairs
}

fn status_rank(status: &str) -> u8 {
    match status {
        ITIP_STATUS_DELIVERY_FAILED => 2,
        ITIP_STATUS_NOT_ATTEMPTED => 1,
        _ => 0,
    }
}

/// One status per (address, property); the worst outcome wins when a
/// recipient gets several messages.
fn record_status(
    statuses: &mut Vec<ZaScheduleStatus>,
    address: &str,
    on_organizer: bool,
    status: &'static str,
) {
    match statuses.iter_mut().find(|existing| {
        existing.on_organizer == on_organizer && existing.address.eq_ignore_ascii_case(address)
    }) {
        Some(existing) => {
            if status_rank(status) > status_rank(existing.status) {
                existing.status = status;
            }
        }
        None => statuses.push(ZaScheduleStatus {
            address: address.to_string(),
            on_organizer,
            status,
        }),
    }
}

/// Spec 5.2: whether `address` is an internal key account. A lookup error
/// is that recipient's delivery failure, traced without the address.
async fn resolve_recipient(server: &Server, sender_id: u32, address: &str) -> Recipient {
    match resolve_inner(server, sender_id, address).await {
        Ok(recipient) => recipient,
        Err(err) => {
            trc::error!(
                err.account_id(sender_id)
                    .details("zero-access: fan-out recipient lookup failed")
                    .caused_by(trc::location!())
            );
            Recipient::Failed
        }
    }
}

async fn resolve_inner(server: &Server, sender_id: u32, address: &str) -> trc::Result<Recipient> {
    let Some(account_id) = server.za_account_id_from_email(address).await? else {
        return Ok(Recipient::NotInternal);
    };
    // Never the sender's own inbox (an alias or sub-address of its own).
    if account_id == sender_id {
        return Ok(Recipient::NotInternal);
    }
    // `za_is_key_account`'s lookup, which also yields the public key (spec
    // 3.2): a pending setup has none.
    let Some(public_key) = server
        .try_account(account_id)
        .await?
        .filter(|account| account.is_key_account())
        .and_then(|account| account.za_public_key)
    else {
        return Ok(Recipient::NotInternal);
    };
    if !server
        .access_token(account_id)
        .await?
        .build()
        .has_permission(Permission::CalendarSchedulingReceive)
    {
        return Ok(Recipient::NotInternal);
    }
    Ok(Recipient::Internal {
        account_id,
        public_key,
    })
}

/// Spec 5.3: one pending item for one recipient. A failure is `5.1`, traced
/// with the recipient's account id and no content.
async fn write_item(
    server: &Server,
    sender_id: u32,
    recipient_id: u32,
    public_key: &[u8; 32],
    message: &ICalendar,
    pair: Option<&TokenPairs>,
    reply_token: Option<&[u8; TOKEN_LEN]>,
) -> &'static str {
    match write_item_inner(
        server,
        sender_id,
        recipient_id,
        public_key,
        message,
        pair,
        reply_token,
    )
    .await
    {
        Ok(true) => ITIP_STATUS_DELIVERED,
        Ok(false) => ITIP_STATUS_DELIVERY_FAILED,
        Err(err) => {
            trc::error!(
                err.account_id(recipient_id)
                    .details("zero-access: fan-out item not written")
                    .caused_by(trc::location!())
            );
            ITIP_STATUS_DELIVERY_FAILED
        }
    }
}

/// `Ok(false)` when the inbound size limit or the recipient's quota refuses
/// the item (step 5); `Ok(true)` once it is committed (steps 1-4).
async fn write_item_inner(
    server: &Server,
    sender_id: u32,
    recipient_id: u32,
    public_key: &[u8; 32],
    message: &ICalendar,
    pair: Option<&TokenPairs>,
    reply_token: Option<&[u8; TOKEN_LEN]>,
) -> trc::Result<bool> {
    // Step 5: the two checks upstream's ingest makes.
    let size = ical_size(message);
    if size >= server.core.groupware.itip_inbound_max_ical_size {
        return Ok(false);
    }
    let account = server
        .account(recipient_id)
        .await
        .caused_by(trc::location!())?;
    // Only a quota limit is "over quota"; any other error reaches the
    // `write_item` trace (as upstream's `copy_message` matches it).
    match server
        .has_available_quota(account.as_ref(), size as u64)
        .await
    {
        Ok(_) => {}
        Err(err)
            if err.matches(trc::EventType::Limit(trc::LimitEvent::Quota))
                || err.matches(trc::EventType::Limit(trc::LimitEvent::TenantQuota)) =>
        {
            return Ok(false);
        }
        Err(err) => return Err(err.caused_by(trc::location!())),
    }
    // Steps 1-3 (plan 8): the tree sealed to the recipient's public key.
    let mut tree = message.clone();
    seal_pending_item(&mut tree, recipient_id, public_key, pair, reply_token)
        .map_err(|err| seal_error(err, recipient_id, u32::MAX))?;
    // Step 4: the item, its own batch in the recipient's account.
    let document_id = server
        .store()
        .assign_document_ids(recipient_id, Collection::CalendarEventNotification, 1)
        .await
        .caused_by(trc::location!())?;
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::fan_out_pause_point(recipient_id).await;
    let mut batch = BatchBuilder::new();
    CalendarEventNotification {
        event: tree,
        event_id: None,
        changed_by: ChangedBy::PrincipalId(sender_id),
        flags: EVENT_NOTIFICATION_ZA_PENDING,
        size: size as u32,
        ..Default::default()
    }
    .insert(
        account.account_tenant_ids(),
        recipient_id,
        document_id,
        &mut batch,
    )
    .caused_by(trc::location!())?;
    server
        .commit_batch(batch)
        .await
        .caused_by(trc::location!())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduling::ItipSummary;
    use calcard::{Entry, Parser};

    /// An event as its organizer (org@example.com) stores it: bob listed
    /// twice in two cases, carol with a client-set 3.7, and the organizer
    /// listed as an attendee.
    const EVENT: &str = concat!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\n",
        "UID:fo-unit\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\n",
        "SEQUENCE:0\r\nORGANIZER:mailto:org@example.com\r\n",
        "ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:Bob@Example.com\r\n",
        "ATTENDEE;CN=Bob again:mailto:bob@example.com\r\n",
        "ATTENDEE;SCHEDULE-STATUS=3.7:mailto:carol@example.com\r\n",
        "ATTENDEE:mailto:org@example.com\r\n",
        "END:VEVENT\r\nEND:VCALENDAR\r\n"
    );

    fn parse(text: &str) -> ICalendar {
        match Parser::new(text).entry() {
            Entry::ICalendar(ical) => ical,
            other => panic!("{other:?}"),
        }
    }

    fn organizer_addresses() -> Vec<String> {
        vec!["org@example.com".to_string()]
    }

    fn message(from: &str, from_organizer: bool, method: &str) -> ItipMessage<ICalendar> {
        ItipMessage {
            from: from.to_string(),
            from_organizer,
            to: vec![],
            summary: ItipSummary::Invite(vec![]),
            message: parse(&format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:{method}\r\nBEGIN:VEVENT\r\nUID:fo-unit\r\nDTSTAMP:20240101T000000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
            )),
        }
    }

    fn route_of(
        message: &ItipMessage<ICalendar>,
        recipient: &str,
        organizer: Option<&Email>,
        union: &TokenPairs,
    ) -> Route {
        route(
            itip_method(&message.message).ok(),
            message,
            recipient,
            organizer,
            union,
        )
    }

    /// (value, SCHEDULE-STATUS values) of every `property` in the tree.
    fn statuses_of(ical: &ICalendar, property: ICalendarProperty) -> Vec<(String, Vec<String>)> {
        ical.components
            .iter()
            .flat_map(|comp| comp.entries.iter())
            .filter(|entry| entry.name == property)
            .map(|entry| {
                (
                    entry.values[0].as_text().unwrap().to_string(),
                    entry
                        .params
                        .iter()
                        .filter(|param| param.name == ICalendarParameterName::ScheduleStatus)
                        .map(|param| param.value.as_text().unwrap().to_string())
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn the_union_adds_one_token_per_new_attendee_and_keeps_stored_ones() {
        let event = parse(EVENT);
        let mut stored = TokenPairs::new();
        stored.insert("carol@example.com", [3; TOKEN_LEN]);
        stored.insert("gone@example.com", [9; TOKEN_LEN]);
        let union = token_union(Some(&stored), &event, &organizer_addresses());
        assert_eq!(union.len(), 3, "carol and gone kept, bob added once");
        assert!(union.matches("carol@example.com", &[3; TOKEN_LEN]));
        assert!(union.matches("gone@example.com", &[9; TOKEN_LEN]));
        let bob = *union.get("bob@example.com").expect("bob's fresh token");
        assert!(
            union.get("org@example.com").is_none(),
            "no token for the sender"
        );
        let current = current_attendee_pairs(&union, &event, &organizer_addresses());
        assert_eq!(current.len(), 2, "the removed attendee's pair is not kept");
        assert!(current.get("gone@example.com").is_none());
        assert!(current.matches("bob@example.com", &bob));
        // The next union keeps bob's token; a union with no stored pairs
        // draws a different one.
        let again = token_union(Some(&current), &event, &organizer_addresses());
        assert!(again.matches("bob@example.com", &bob));
        let fresh = token_union(None, &event, &organizer_addresses());
        assert!(!fresh.matches("bob@example.com", &bob));
    }

    #[test]
    fn requests_and_cancels_go_out_only_from_the_organizer() {
        let event = parse(EVENT);
        let organizer = event_organizer(&event, &organizer_addresses());
        assert!(
            organizer
                .as_ref()
                .is_some_and(|organizer| organizer.is_local)
        );
        let mut union = TokenPairs::new();
        union.insert("bob@example.com", [1; TOKEN_LEN]);
        for method in ["REQUEST", "CANCEL"] {
            let ours = message("org@example.com", true, method);
            match route_of(&ours, "bob@example.com", organizer.as_ref(), &union) {
                Route::Deliver {
                    on_organizer: false,
                    pair: Some(pair),
                    reply_token: None,
                } => {
                    assert_eq!(pair.len(), 1);
                    assert!(pair.matches("bob@example.com", &[1; TOKEN_LEN]));
                }
                _ => panic!("{method} from the organizer goes out with bob's pair"),
            }
            let delegate = message("bob@example.com", false, method);
            assert!(
                matches!(
                    route_of(&delegate, "dave@example.com", organizer.as_ref(), &union),
                    Route::NotAttempted {
                        on_organizer: false
                    }
                ),
                "{method} from an attendee is not attempted"
            );
        }
        // A removed attendee with no stored pair: delivered without a pair.
        let cancel = message("org@example.com", true, "CANCEL");
        assert!(matches!(
            route_of(&cancel, "dave@example.com", organizer.as_ref(), &union),
            Route::Deliver { pair: None, .. }
        ));
    }

    #[test]
    fn a_reply_goes_to_the_organizer_only_and_needs_the_repliers_token() {
        // Bob's copy: the organizer is another account.
        let event = parse(EVENT);
        let organizer = event_organizer(&event, &["bob@example.com".to_string()]);
        assert!(
            organizer
                .as_ref()
                .is_some_and(|organizer| !organizer.is_local)
        );
        let mut own = TokenPairs::new();
        own.insert("bob@example.com", [5; TOKEN_LEN]);
        let reply = message("bob@example.com", false, "REPLY");
        match route_of(&reply, "ORG@example.com", organizer.as_ref(), &own) {
            Route::Deliver {
                on_organizer: true,
                pair: None,
                reply_token: Some(token),
            } => assert_eq!(*token, [5; TOKEN_LEN]),
            _ => panic!("the reply to the organizer carries bob's token"),
        }
        assert!(
            matches!(
                route_of(&reply, "carol@example.com", organizer.as_ref(), &own),
                Route::NotAttempted {
                    on_organizer: false
                }
            ),
            "a delegator is not attempted"
        );
        assert!(
            matches!(
                route_of(
                    &reply,
                    "org@example.com",
                    organizer.as_ref(),
                    &TokenPairs::new()
                ),
                Route::NotAttempted { on_organizer: true }
            ),
            "no token: not attempted, the status on the ORGANIZER"
        );
        let refresh = message("bob@example.com", false, "REFRESH");
        assert!(matches!(
            route_of(&refresh, "org@example.com", organizer.as_ref(), &own),
            Route::NotAttempted { on_organizer: true }
        ));
    }

    #[test]
    fn statuses_land_on_every_matching_property_and_replace_earlier_ones() {
        let mut event = parse(EVENT);
        za_set_schedule_status(
            &mut event,
            &[
                ZaScheduleStatus {
                    address: "bob@example.com".into(),
                    on_organizer: false,
                    status: ITIP_STATUS_DELIVERED,
                },
                ZaScheduleStatus {
                    address: "carol@example.com".into(),
                    on_organizer: false,
                    status: ITIP_STATUS_NOT_ATTEMPTED,
                },
                ZaScheduleStatus {
                    address: "org@example.com".into(),
                    on_organizer: true,
                    status: ITIP_STATUS_DELIVERY_FAILED,
                },
            ],
        );
        let s = |values: &[&str]| values.iter().map(|v| v.to_string()).collect::<Vec<_>>();
        assert_eq!(
            statuses_of(&event, ICalendarProperty::Attendee),
            vec![
                ("mailto:Bob@Example.com".to_string(), s(&["1.2"])),
                ("mailto:bob@example.com".to_string(), s(&["1.2"])),
                ("mailto:carol@example.com".to_string(), s(&["3.7", "5.3"])),
                ("mailto:org@example.com".to_string(), s(&[])),
            ]
        );
        assert_eq!(
            statuses_of(&event, ICalendarProperty::Organizer),
            vec![("mailto:org@example.com".to_string(), s(&["5.1"]))]
        );
        za_set_schedule_status(
            &mut event,
            &[ZaScheduleStatus {
                address: "CAROL@example.com".into(),
                on_organizer: false,
                status: ITIP_STATUS_DELIVERED,
            }],
        );
        assert_eq!(
            statuses_of(&event, ICalendarProperty::Attendee)[2].1,
            s(&["3.7", "1.2"]),
            "a fan-out status is replaced, upstream's 3.7 is kept"
        );
    }

    #[test]
    fn the_worst_status_of_a_recipient_wins() {
        let mut statuses = Vec::new();
        record_status(
            &mut statuses,
            "bob@example.com",
            false,
            ITIP_STATUS_DELIVERED,
        );
        record_status(
            &mut statuses,
            "BOB@example.com",
            false,
            ITIP_STATUS_DELIVERY_FAILED,
        );
        record_status(
            &mut statuses,
            "bob@example.com",
            false,
            ITIP_STATUS_DELIVERED,
        );
        record_status(
            &mut statuses,
            "bob@example.com",
            true,
            ITIP_STATUS_NOT_ATTEMPTED,
        );
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].status, ITIP_STATUS_DELIVERY_FAILED);
        assert!(statuses[1].on_organizer);
        assert_eq!(statuses[1].status, ITIP_STATUS_NOT_ATTEMPTED);
    }
}
