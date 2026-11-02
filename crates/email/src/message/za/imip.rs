/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access fork, release 5 (iMIP spec 4.1): a key account's inbound iMIP
//! part at delivery. In place of `itip_ingest`, the part becomes at most one
//! mail-borne pending item in the account's scheduling inbox: sealed to the
//! served public key the mail was wrapped to (`IngestWrap::Public`), with the
//! sender in the sealed `X-ZA-SENDER` carrier and `ChangedBy::CalendarAddress("")`
//! on its row (spec 4.2, D4). Delivery decides from the message and keyless
//! indexes only and reads no session key (D1, mail spec invariant 29); the
//! drain repeats every check it can. A refusal or a failed write is traced
//! with ids, counts and fixed strings only (section 7, D17) and never fails
//! the delivery: the mail is stored sealed whatever happens to the item.

use super::ingest::IngestWrap;
use crate::message::delivery::SenderAuthentication;
use calcard::icalendar::ICalendarMethod;
use common::{
    Server,
    auth::AccountCache,
    za::imip::{
        ZA_IMIP_ITEM_REFUSED, ZA_IMIP_KEY_SENDER, ZA_IMIP_LOOKUP_FAILED, ZA_IMIP_NOT_TAKEN,
        ZA_IMIP_SESSION_WRAP, ZA_IMIP_TOO_LARGE, ZA_IMIP_WRITE_FAILED,
    },
};
use groupware::{
    calendar::{
        CalendarEventNotification, CalendarEventNotificationContent, ChangedBy,
        drain::itip_reason,
        fanout::za_write_pending_item,
        itip::{itip_auto_add_allowed, itip_parse_inbound, itip_sender_snapshot},
        seal::item::{archived_item_is_pending, item_message},
    },
    scheduling::{ItipError, inbound::itip_method},
};
use mail_parser::Message;
use std::sync::Arc;
use store::roaring::RoaringBitmap;
use trc::AddContext;
use types::{
    collection::Collection,
    field::{CalendarEventField, CalendarNotificationField, Field},
};
use utils::sanitize_email;

/// The iTIP sender of an inbound message, by upstream's rule
/// (`email_ingest_into`'s iMIP branch): the envelope MAIL FROM for an
/// authenticated submission, else the first From address, sanitized.
pub fn za_imip_sender(
    sender_authentication: SenderAuthentication,
    envelope_from: &str,
    message: &Message<'_>,
) -> Option<String> {
    if sender_authentication == SenderAuthentication::Submission {
        sanitize_email(envelope_from)
    } else {
        message
            .from()
            .and_then(|s| s.first())
            .and_then(|s| s.address())
            .and_then(sanitize_email)
    }
}

/// D3: whether `uid` is known to `account_id`: an event with it (the UID
/// index), or a pending inbox item whose sealed tree names it (its UID is
/// visible). No index holds pending items by UID, so for a UID the event
/// index lacks this reads every notification row once and the content of
/// the pending ones.
pub async fn za_imip_uid_known(server: &Server, account_id: u32, uid: &str) -> trc::Result<bool> {
    if !server
        .document_ids_matching(
            account_id,
            Collection::CalendarEvent,
            CalendarEventField::Uid,
            uid.as_bytes(),
        )
        .await
        .caused_by(trc::location!())?
        .is_empty()
    {
        return Ok(true);
    }
    let mut pending = RoaringBitmap::new();
    server
        .archives(
            account_id,
            Collection::CalendarEventNotification,
            Field::ARCHIVE,
            &(),
            |document_id, row| {
                if archived_item_is_pending(row.unarchive::<CalendarEventNotification>()?) {
                    pending.insert(document_id);
                }
                Ok(true)
            },
        )
        .await
        .caused_by(trc::location!())?;
    if pending.is_empty() {
        return Ok(false);
    }
    let mut known = false;
    server
        .archives(
            account_id,
            Collection::CalendarEventNotification,
            CalendarNotificationField::Content.field(),
            &pending,
            |_, content| {
                let content = content.deserialize::<CalendarEventNotificationContent>()?;
                known = item_message(&content)
                    .is_some_and(|tree| tree.uids().any(|item_uid| item_uid == uid));
                Ok(!known)
            },
        )
        .await
        .caused_by(trc::location!())?;
    Ok(known)
}

/// Spec 4.1: one qualifying `text/calendar` part of a key account's
/// delivery, in order: the size limit, the parse with the Exchange fill, the
/// snapshot and the method's sender rule, the method (REQUEST, ADD, CANCEL,
/// REPLY and REFRESH continue), the key-account sender refusal, the known UID
/// with the auto-add predicate for a REQUEST, then the item
/// (`za_write_pending_item`, with no pair, token or fence). Every step that
/// refuses writes nothing. The item's batch carries no provisioning fence
/// (D16). Every event carries the span id, the account id and `From` as the
/// redaction marker.
#[allow(clippy::too_many_arguments)]
pub async fn za_imip_item(
    server: &Server,
    account: &Arc<AccountCache>,
    wrap: Option<&IngestWrap>,
    itip_text: &str,
    sender_authentication: SenderAuthentication,
    envelope_from: &str,
    message: &Message<'_>,
    recipient: &str,
    session_id: u64,
) {
    let account_id = account.id;
    let refused = |reason: &'static str| {
        trc::event!(
            Calendar(trc::CalendarEvent::ItipMessageError),
            SpanId = session_id,
            AccountId = account_id,
            From = trc::Value::redacted(),
            Reason = reason,
            Details = ZA_IMIP_NOT_TAKEN,
        );
    };
    // Section 7: a store error from a lookup keyed by the sender's address
    // (or the UID) is traced with its type and the account id only.
    let lookup_failed = |err: trc::Error| {
        trc::error!(
            trc::Error::new(err.event_type())
                .span_id(session_id)
                .account_id(account_id)
                .ctx(trc::Key::From, trc::Value::redacted())
                .details(ZA_IMIP_LOOKUP_FAILED)
                .caused_by(trc::location!())
        );
    };

    let account_info = match server.build_account_info(account.clone()).await {
        Ok(account_info) => account_info,
        Err(err) => {
            trc::error!(
                err.span_id(session_id)
                    .account_id(account_id)
                    .ctx(trc::Key::From, trc::Value::redacted())
                    .details(ZA_IMIP_WRITE_FAILED)
                    .caused_by(trc::location!())
            );
            return;
        }
    };
    // An SMTP delivery's token never holds keys, so its wrap is the served
    // public key; a session wrap writes no item.
    let public_key = match wrap {
        Some(IngestWrap::Public(public_key)) => public_key,
        Some(IngestWrap::Session(_)) | None => {
            refused(ZA_IMIP_SESSION_WRAP);
            return;
        }
    };
    // Step 1.
    let limit = server.core.groupware.itip_inbound_max_ical_size;
    if itip_text.len() >= limit {
        trc::event!(
            Calendar(trc::CalendarEvent::ItipMessageError),
            SpanId = session_id,
            AccountId = account_id,
            From = trc::Value::redacted(),
            Details = ZA_IMIP_TOO_LARGE,
            Limit = limit,
            Size = itip_text.len(),
        );
        return;
    }
    let Some(sender) = za_imip_sender(sender_authentication, envelope_from, message) else {
        return;
    };
    // Step 2: upstream's message checks.
    let tree = match itip_parse_inbound(itip_text, recipient) {
        Ok(tree) => tree,
        Err(err) => {
            refused(itip_reason(&err));
            return;
        }
    };
    let uid = match itip_sender_snapshot(&tree, &account_info.addresses, &sender) {
        Ok(snapshots) => snapshots.uid.to_string(),
        Err(err) => {
            refused(itip_reason(&err));
            return;
        }
    };
    // Step 3: the method.
    let method = match itip_method(&tree) {
        Ok(method) => method.clone(),
        Err(err) => {
            refused(itip_reason(&err));
            return;
        }
    };
    if !matches!(
        method,
        ICalendarMethod::Request
            | ICalendarMethod::Add
            | ICalendarMethod::Cancel
            | ICalendarMethod::Reply
            | ICalendarMethod::Refresh
    ) {
        refused(itip_reason(&ItipError::UnsupportedMethod(method)));
        return;
    }
    // Step 4: key accounts reach each other only through the fan-out.
    let principal = match server.za_account_id_from_email(&sender).await {
        Ok(principal) => principal,
        Err(err) => {
            lookup_failed(err);
            return;
        }
    };
    if let Some(principal_id) = principal {
        match server.za_is_key_account(principal_id).await {
            Ok(false) => {}
            Ok(true) => {
                refused(ZA_IMIP_KEY_SENDER);
                return;
            }
            Err(err) => {
                lookup_failed(err);
                return;
            }
        }
    }
    // Step 5: the known UID; a REQUEST for an unknown one under the auto-add
    // predicate.
    match za_imip_uid_known(server, account_id, &uid).await {
        Ok(true) => {}
        Ok(false) if method != ICalendarMethod::Request => {
            refused(itip_reason(&ItipError::EventNotFound));
            return;
        }
        Ok(false) => {
            match itip_auto_add_allowed(server, account_id, principal.is_some(), &sender).await {
                Ok(true) => {}
                Ok(false) => {
                    refused(itip_reason(&ItipError::AutoAddDisabled));
                    return;
                }
                Err(err) => {
                    lookup_failed(err);
                    return;
                }
            }
        }
        Err(err) => {
            lookup_failed(err);
            return;
        }
    }
    // Step 6: the item, its row naming no sender (D4).
    match za_write_pending_item(
        server,
        ChangedBy::CalendarAddress(String::new()),
        account_id,
        public_key,
        &tree,
        None,
        None,
        Some(&sender),
        None,
    )
    .await
    {
        Ok(true) => {
            trc::event!(
                Calendar(trc::CalendarEvent::ItipMessageReceived),
                SpanId = session_id,
                AccountId = account_id,
                From = trc::Value::redacted(),
            );
        }
        Ok(false) => refused(ZA_IMIP_ITEM_REFUSED),
        Err(err) => {
            trc::error!(
                err.span_id(session_id)
                    .account_id(account_id)
                    .ctx(trc::Key::From, trc::Value::redacted())
                    .details(ZA_IMIP_WRITE_FAILED)
                    .caused_by(trc::location!())
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_parser::MessageParser;

    fn parsed(raw: &str) -> Message<'_> {
        MessageParser::new().parse(raw.as_bytes()).unwrap()
    }

    #[test]
    fn za_imip_sender_takes_upstreams_rule() {
        let raw = "From: \"Org\" <IMIP-ORG@Remote.Test>\r\nSubject: x\r\n\r\nbody\r\n";
        let message = parsed(raw);
        // DMARC pass and unauthenticated: the first From address, sanitized.
        for authentication in [
            SenderAuthentication::DmarcPass,
            SenderAuthentication::Unauthenticated,
        ] {
            assert_eq!(
                za_imip_sender(authentication, "bounce@elsewhere.test", &message).as_deref(),
                Some("imip-org@remote.test")
            );
        }
        // Authenticated submission: the envelope MAIL FROM, sanitized.
        assert_eq!(
            za_imip_sender(
                SenderAuthentication::Submission,
                "Plain@Example.com",
                &message
            )
            .as_deref(),
            Some("plain@example.com")
        );
        // No From address: no sender.
        let message = parsed("Subject: x\r\n\r\nbody\r\n");
        assert_eq!(
            za_imip_sender(SenderAuthentication::DmarcPass, "a@b.test", &message),
            None
        );
    }
}
