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

fn trace_failed(account_id: u32, document_id: u32, details: &'static str) {
    trc::event!(
        Calendar(trc::CalendarEvent::ItipMessageError),
        AccountId = account_id,
        DocumentId = document_id,
        Details = details,
    );
}
