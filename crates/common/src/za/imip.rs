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
/// The details of mail attempts left unfinished after the commit because
/// the request's future was dropped (a client that disconnects mid-loop):
/// traced once per event (section 7).
pub const ZA_IMIP_ABANDONED: &str = "zero-access: iMIP submission abandoned";

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

/// Submits each (message, recipient) in order. Called in the request after
/// the commit, so the request's future can be dropped mid-loop (hyper drops
/// the service future of a connection that reaches EOF): the attempts not
/// yet finished, the one being submitted included, are then traced as
/// abandoned (`Unfinished`, section 7).
pub async fn za_imip_submit_all(server: &Server, mails: Vec<ZaImipMail>) {
    submit_each(mails, |mail| za_imip_submit(server, mail)).await;
}

/// `za_imip_submit_all`'s loop, over any submission function.
async fn submit_each<F, Fut>(mails: Vec<ZaImipMail>, mut submit: F)
where
    F: FnMut(ZaImipMail) -> Fut,
    Fut: Future<Output = ZaImipOutcome>,
{
    let mut unfinished = Unfinished::new(
        mails
            .iter()
            .map(|mail| (mail.account_id, mail.document_id))
            .collect(),
    );
    for mail in mails {
        submit(mail).await;
        unfinished.finish_one();
    }
    unfinished.disarm();
}

/// The (account id, document id) of each mail attempt of one
/// `za_imip_submit_all` call, in order, and how many have finished. Dropped
/// while armed, it traces each event among the unfinished attempts once with
/// `ZA_IMIP_ABANDONED`: ids only, no address or content. No `Debug`.
struct Unfinished {
    attempts: Vec<(u32, u32)>,
    finished: usize,
}

impl Unfinished {
    fn new(attempts: Vec<(u32, u32)>) -> Self {
        Unfinished {
            attempts,
            finished: 0,
        }
    }

    fn finish_one(&mut self) {
        self.finished += 1;
    }

    fn disarm(&mut self) {
        self.finished = self.attempts.len();
    }

    /// The distinct events of the unfinished attempts, in order.
    fn abandoned(&self) -> Vec<(u32, u32)> {
        let mut events = Vec::new();
        for event in self.attempts.iter().skip(self.finished) {
            if !events.contains(event) {
                events.push(*event);
            }
        }
        events
    }
}

impl Drop for Unfinished {
    fn drop(&mut self) {
        for (account_id, document_id) in self.abandoned() {
            trc::event!(
                Calendar(trc::CalendarEvent::ItipMessageError),
                AccountId = account_id,
                DocumentId = document_id,
                Details = ZA_IMIP_ABANDONED,
            );
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use trc::{
        CalendarEvent, Collector, EventType, Key,
        ipc::subscriber::{Interests, SubscriberBuilder},
    };

    /// Section 7: a guard dropped while armed traces each event among the
    /// unfinished attempts (the one in flight included) once; a disarmed
    /// guard traces nothing. The loop arms it: a submission loop dropped
    /// mid-way (as hyper drops the request future of a disconnected client)
    /// traces its unfinished events, and a loop that completes traces none.
    #[tokio::test]
    async fn an_armed_drop_traces_the_unfinished_events_and_a_disarmed_one_nothing() {
        // Account ids no other test of this crate uses.
        const ARMED: u32 = 0x5a5a_0401;
        const DISARMED: u32 = 0x5a5a_0402;
        const CANCELLED: u32 = 0x5a5a_0403;
        const COMPLETED: u32 = 0x5a5a_0404;
        let id = "za-imip-unfinished-test".to_string();
        let typ = EventType::Calendar(CalendarEvent::ItipMessageError);
        let (_tx, mut rx, _) = SubscriberBuilder::new(id.clone())
            .set_interests([typ])
            .with_lossy(false)
            .register();
        let mut interests = Interests::default();
        interests.set(typ);
        Collector::union_interests(interests);
        Collector::reload();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !Collector::get_subscribers().contains(&id) {
            assert!(Instant::now() < deadline, "the subscriber was not applied");
            std::thread::sleep(Duration::from_millis(1));
        }

        // Five attempts over three events; the first finished, the second
        // in flight: events 7 and 9 are unfinished, 5 is not.
        let attempts = |account| {
            [5, 7, 7, 9, 9]
                .into_iter()
                .map(|document_id| (account, document_id))
                .collect::<Vec<_>>()
        };
        let mut armed = Unfinished::new(attempts(ARMED));
        armed.finish_one();
        assert_eq!(armed.abandoned(), [(ARMED, 7), (ARMED, 9)]);
        drop(armed);
        let mut disarmed = Unfinished::new(attempts(DISARMED));
        disarmed.finish_one();
        disarmed.disarm();
        assert!(disarmed.abandoned().is_empty());
        drop(disarmed);

        // The loop: the second submission never answers, and the loop is
        // dropped there; a loop whose submissions all answer completes.
        let mails = |account| {
            [5, 7, 9]
                .into_iter()
                .map(|document_id| ZaImipMail {
                    account_id: account,
                    document_id,
                    contents: TaskCalendarItipContents::default(),
                })
                .collect::<Vec<_>>()
        };
        let mut calls = 0;
        let cancelled = submit_each(mails(CANCELLED), |_| {
            calls += 1;
            let answers = calls == 1;
            async move {
                if !answers {
                    std::future::pending::<()>().await;
                }
                ZaImipOutcome::Queued(0)
            }
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(50), cancelled)
                .await
                .is_err(),
            "the loop completed"
        );
        submit_each(mails(COMPLETED), |_| async { ZaImipOutcome::Queued(0) }).await;

        let mut traced = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while traced.len() < 4 {
            let Ok(Some(batch)) = tokio::time::timeout_at(deadline, rx.recv()).await else {
                break;
            };
            for event in batch {
                let value = |wanted: Key| {
                    event
                        .keys
                        .iter()
                        .find(|(key, _)| *key == wanted)
                        .map(|(_, value)| value.clone())
                };
                if let (Some(trc::Value::UInt(account)), Some(trc::Value::UInt(document))) =
                    (value(Key::AccountId), value(Key::DocumentId))
                    && [ARMED, DISARMED, CANCELLED, COMPLETED].contains(&(account as u32))
                {
                    assert!(
                        matches!(value(Key::Details), Some(trc::Value::String(details)) if details == ZA_IMIP_ABANDONED)
                    );
                    traced.push((account as u32, document as u32));
                }
            }
        }
        // Anything the disarmed guard or the completed loop might have sent
        // arrives with the other events or after them.
        tokio::time::sleep(Duration::from_millis(200)).await;
        while let Ok(batch) = rx.try_recv() {
            for event in batch {
                assert!(
                    !event.keys.iter().any(|(key, value)| *key == Key::AccountId
                        && matches!(value, trc::Value::UInt(account)
                            if [DISARMED as u64, COMPLETED as u64].contains(account))),
                    "a disarmed guard traced"
                );
            }
        }
        Collector::remove_subscriber(id);
        Collector::reload();
        assert_eq!(
            traced,
            [(ARMED, 7), (ARMED, 9), (CANCELLED, 7), (CANCELLED, 9)]
        );
    }
}
