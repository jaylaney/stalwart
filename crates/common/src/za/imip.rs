/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access fork, release 5 (iMIP spec 5.3, D11): the seam through which
//! the request paths mail a key account's scheduling messages. The `dav` and
//! `groupware` crates cannot reach `services` or `smtp`, so `services`
//! registers the submitter here (`services::za_imip::register`, from
//! `spawn_services`) as a function pointer: code, not state, so one process
//! may host several servers. One (message, recipient) is held in process
//! memory for the call and never written to the task queue or any store
//! location. Every outcome is traced with ids, the SMTP stage, the reply code
//! and fixed strings only (section 7, D17).

use crate::Server;
use registry::schema::structs::TaskCalendarItipContents;
use std::{future::Future, pin::Pin, sync::OnceLock};

/// Section 7: the trace details of a submission the SMTP layer refused.
pub const ZA_IMIP_REFUSED: &str = "zero-access: iMIP submission refused";
/// Section 7: the trace details of a submission that failed before the SMTP
/// layer answered (a composer error, the test hook).
pub const ZA_IMIP_FAILED: &str = "zero-access: iMIP submission failed";
/// Section 7: the trace details of a submission made before the submitter
/// was registered.
pub const ZA_IMIP_UNREGISTERED: &str = "zero-access: iMIP submitter not registered";

// Delivery (spec 4.1, section 7, D17): the fixed strings of the events
// `email::message::za::imip::za_imip_item` traces for a key account's
// inbound iMIP part.

/// The trace details of an iTIP-level refusal at delivery (the reason is
/// the refusal's fixed name): no item is written.
pub const ZA_IMIP_NOT_TAKEN: &str = "zero-access: iMIP not taken";
/// Spec 4.1 step 4: the reason when the sender is a local key account.
pub const ZA_IMIP_KEY_SENDER: &str = "zero-access: iMIP from a key account";
/// The reason when the item writer refused the item (the inbound size, the
/// notification limit or the quota).
pub const ZA_IMIP_ITEM_REFUSED: &str = "zero-access: inbox item refused";
/// The reason when a delivery's wrap is the session's keys, which an SMTP
/// delivery's token never holds: no item is written.
pub const ZA_IMIP_SESSION_WRAP: &str = "zero-access: iMIP delivery holds session keys";
/// The details of an error that stopped the item's write.
pub const ZA_IMIP_WRITE_FAILED: &str = "zero-access: iMIP item not written";
/// The details of a store error from a lookup keyed by the sender's address
/// or the UID, traced with its type and the account id only (section 7).
pub const ZA_IMIP_LOOKUP_FAILED: &str = "zero-access: iMIP sender lookup failed";
/// Upstream's details for an iMIP part over `itip_inbound_max_ical_size`.
pub const ZA_IMIP_TOO_LARGE: &str = "iMIP message too large";

// The drain (spec 4.3, D7, section 7).

/// The details of a mail-borne item the drain deletes, as upstream's ingest
/// would have left no inbox item (the reason is the outcome's fixed name).
pub const ZA_IMIP_ITEM_DELETED: &str = "zero-access: mail-borne item deleted";

// The callers (spec 5.4, Jay's decision 9).

/// The details of a DELETE's mail attempts dropped because its commit
/// returned an error: nothing is submitted (section 7).
pub const ZA_IMIP_DROPPED: &str = "zero-access: iMIP mail dropped, the deletion did not commit";

/// One (message, recipient) of spec 5.3: the event's account (upstream's
/// task account), the event's document id, and one message as
/// `ItipMessages::new` fills it, with `to` the one recipient. No `Debug`:
/// it holds addresses and the event's content.
pub struct ZaImipMail {
    pub account_id: u32,
    pub document_id: u32,
    pub contents: TaskCalendarItipContents,
}

/// A submission's outcome: queued under its queue id, refused by the local
/// SMTP session at a stage with its reply code, or failed before an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZaImipOutcome {
    Queued(u64),
    Refused { stage: &'static str, code: u16 },
    Failed,
}

pub type ZaImipSubmitter =
    fn(Server, ZaImipMail) -> Pin<Box<dyn Future<Output = ZaImipOutcome> + Send>>;

static SUBMITTER: OnceLock<ZaImipSubmitter> = OnceLock::new();

/// Registers the submitter; a second registration is ignored.
pub fn za_register_imip_submitter(submitter: ZaImipSubmitter) {
    let _ = SUBMITTER.set(submitter);
}

/// Spec 5.3: submits one (message, recipient) through the registered
/// submitter and traces the outcome. `Failed`, without calling it, while
/// none is registered or (test builds) when `IMIP_SUBMIT_FAIL_ONCE` is taken
/// for the account.
pub async fn za_imip_submit(server: &Server, mail: ZaImipMail) -> ZaImipOutcome {
    let account_id = mail.account_id;
    let document_id = mail.document_id;

    #[cfg(feature = "test_mode")]
    if super::test_hooks::take_imip_submit_fail(account_id) {
        trace_failed(account_id, document_id, ZA_IMIP_FAILED);
        return ZaImipOutcome::Failed;
    }

    let Some(submitter) = SUBMITTER.get() else {
        trace_failed(account_id, document_id, ZA_IMIP_UNREGISTERED);
        return ZaImipOutcome::Failed;
    };

    let outcome = submitter(server.clone(), mail).await;
    match outcome {
        ZaImipOutcome::Queued(queue_id) => {
            trc::event!(
                Calendar(trc::CalendarEvent::ItipMessageSent),
                AccountId = account_id,
                DocumentId = document_id,
                QueueId = queue_id,
            );
        }
        ZaImipOutcome::Refused { stage, code } => {
            trc::event!(
                Calendar(trc::CalendarEvent::ItipMessageError),
                AccountId = account_id,
                DocumentId = document_id,
                Reason = stage,
                Code = code,
                Details = ZA_IMIP_REFUSED,
            );
        }
        ZaImipOutcome::Failed => trace_failed(account_id, document_id, ZA_IMIP_FAILED),
    }
    outcome
}

/// Submits each (message, recipient) in order.
pub async fn za_imip_submit_all(server: &Server, mails: Vec<ZaImipMail>) {
    for mail in mails {
        za_imip_submit(server, mail).await;
    }
}

/// Spec 5.4, section 7: a DELETE whose commit returned an error mails
/// nothing; traces one `ItipMessageError` per distinct event (document id)
/// among the dropped attempts, with ids and the fixed reason only.
pub fn za_imip_trace_dropped(account_id: u32, mails: &[ZaImipMail]) {
    let mut traced = Vec::new();
    for mail in mails {
        if !traced.contains(&mail.document_id) {
            traced.push(mail.document_id);
            trc::event!(
                Calendar(trc::CalendarEvent::ItipMessageError),
                AccountId = account_id,
                DocumentId = mail.document_id,
                Details = ZA_IMIP_DROPPED,
            );
        }
    }
}

fn trace_failed(account_id: u32, document_id: u32, details: &'static str) {
    trc::event!(
        Calendar(trc::CalendarEvent::ItipMessageError),
        AccountId = account_id,
        DocumentId = document_id,
        Details = details,
    );
}
