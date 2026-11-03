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

use crate::{Server, auth::EmailCache};
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

/// The details of mail attempts dropped because their commit returned an
/// error: nothing is submitted (section 7). Named for the DELETE, the first
/// caller; a PUT's and a REFRESH answer's attempts are dropped the same way
/// (`ZaImipPending::dropped`).
pub const ZA_IMIP_DROPPED: &str = "zero-access: iMIP mail dropped, the deletion did not commit";
/// The details of mail attempts left unfinished because the request ended
/// before they were submitted or settled: its future was dropped (a client
/// that disconnects) anywhere from the fan-out to the last submission, or an
/// error returned before the commit's outcome settled them. Their outcome is
/// unknown: the change may or may not have committed, and the attempt in
/// flight may already be queued. An error returned before the commit was
/// called traces this too, although nothing was committed then (PR review
/// ruling O-2: the DAV callers' error returns between the fan-out and the
/// commit do not settle the guard). Traced once per event (section 7,
/// `ZaImipPending`).
pub const ZA_IMIP_ABANDONED: &str = "zero-access: iMIP submission abandoned";

// The pin rule of mail (final review ruling A-2, fail closed).

/// The details of a mail attempt withheld because its address holds a
/// contact pin in the sender's pin record (`3.8`; a REFRESH gets no answer).
pub const ZA_IMIP_PINNED: &str = "zero-access: iMIP withheld, the address holds a pin";
/// The details of a mail attempt withheld because the sender's pin state
/// could not be read or decided (`5.1`; a REFRESH gets no answer).
pub const ZA_IMIP_PIN_UNREAD: &str = "zero-access: iMIP withheld, the pin record could not be read";

// The mail class's local routes (PR review ruling P1, fail closed; a D9
// amendment).

/// The details of a mail attempt withheld because its address is in a local
/// domain and the registry resolves it to neither an account nor a mailing
/// list (`ZaMailClass::Withheld`), or to an account it no longer finds, or
/// (the REFRESH answer) to a key account (`5.1`; a REFRESH gets no answer).
pub const ZA_IMIP_UNROUTED: &str =
    "zero-access: iMIP withheld, the local address is not of the mail class";

/// PR review ruling P1 (fail closed; a D9 amendment): the class of a
/// scheduling address as the registry resolves it (`Server::za_mail_class`).
/// The SMTP layer's `rcpt_resolve` reaches further than the registry's own
/// answer (it falls through a disabled entry to the catch-all and consults
/// an external directory first), so a local address is mailed only when the
/// registry resolves it to a non-key account or a mailing list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZaMailClass {
    /// The address's domain is not local: the mail class.
    NotLocal,
    /// A local account, directly or through the domain's catch-all when the
    /// address has no entry. The caller decides by the account: a key
    /// account keeps the internal route, any other is the mail class.
    Account(u32),
    /// A local mailing list, directly or through the catch-all (Jay's
    /// decision 10): the mail class.
    List,
    /// Any other address in a local domain: a disabled account or list
    /// address, no entry and no catch-all, or a catch-all that resolves to
    /// neither an account nor a list. Never mailed (`5.1`).
    Withheld,
}

/// `ZaMailClass`'s decision, from the address's own registry entry (`entry`)
/// and, only when it has none and its domain names a catch-all, the
/// catch-all's entry (`catch_all`, `Some(None)` when the catch-all resolves
/// to nothing), as `account_id_from_email(address, true)` consults the
/// catch-all.
pub fn za_mail_class_of(
    entry: Option<EmailCache>,
    catch_all: Option<Option<EmailCache>>,
) -> ZaMailClass {
    match entry.or(catch_all.flatten()) {
        Some(EmailCache::Account(account_id)) => ZaMailClass::Account(account_id),
        Some(EmailCache::MailingList(_)) => ZaMailClass::List,
        Some(EmailCache::DisabledAccountAddress(_) | EmailCache::DisabledListAddress(_)) | None => {
            ZaMailClass::Withheld
        }
    }
}

/// PR review ruling P1: traces one mail attempt withheld because its local
/// address is not of the mail class; ids and a fixed string only.
pub fn za_imip_trace_unrouted(account_id: u32, document_id: u32) {
    trc::event!(
        Calendar(trc::CalendarEvent::ItipMessageError),
        AccountId = account_id,
        DocumentId = document_id,
        Details = ZA_IMIP_UNROUTED,
    );
}

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

/// iMIP spec 5.4, section 7 (final review rulings A-4 and 189): the mail
/// attempts of one request, held from the fan-out's return, before the
/// sender's commit is called, until they are settled. After the commit
/// returns `Ok`, `submit` hands each to `za_imip_submit` in order; after it
/// returns an error, `dropped` traces `ZA_IMIP_DROPPED` and submits nothing.
/// Dropped unsettled (hyper drops the service future of a connection that
/// reaches EOF, at any await from the fan-out to the last submission, or an
/// error returns without settling it), it traces `ZA_IMIP_ABANDONED` for
/// each event among the attempts not yet finished, the one in flight
/// included: ids only. No `Debug`: it holds addresses and content.
#[must_use = "an unsettled ZaImipPending traces its attempts as abandoned"]
#[derive(Default)]
pub struct ZaImipPending {
    /// The attempts not yet handed to `submit`'s loop, in order.
    mails: Vec<ZaImipMail>,
    /// The (account id, document id) of every attempt, in order.
    attempts: Vec<(u32, u32)>,
    /// How many of `attempts` are finished (submitted, or settled).
    finished: usize,
}

impl From<Vec<ZaImipMail>> for ZaImipPending {
    fn from(mails: Vec<ZaImipMail>) -> Self {
        let mut pending = ZaImipPending::default();
        for mail in mails {
            pending.push(mail);
        }
        pending
    }
}

impl ZaImipPending {
    /// Adds one attempt after the others.
    pub fn push(&mut self, mail: ZaImipMail) {
        self.attempts.push((mail.account_id, mail.document_id));
        self.mails.push(mail);
    }

    /// Takes `other`'s attempts after this one's (a DELETE's fan-outs, one
    /// per event, before its one commit); `other` is left settled.
    pub fn extend(&mut self, mut other: ZaImipPending) {
        for mail in std::mem::take(&mut other.mails) {
            self.push(mail);
        }
        other.finished = other.attempts.len();
    }

    pub fn is_empty(&self) -> bool {
        self.attempts.is_empty()
    }

    pub fn len(&self) -> usize {
        self.attempts.len()
    }

    /// Spec 5.4, Jay's decision 9: the commit returned `Ok`; submits each
    /// (message, recipient) in order, each traced by `za_imip_submit`.
    pub async fn submit(self, server: &Server) {
        self.submit_each(|mail| za_imip_submit(server, mail)).await;
    }

    /// `submit`'s loop, over any submission function.
    async fn submit_each<F, Fut>(mut self, mut submit: F)
    where
        F: FnMut(ZaImipMail) -> Fut,
        Fut: Future<Output = ZaImipOutcome>,
    {
        for mail in std::mem::take(&mut self.mails) {
            submit(mail).await;
            self.finished += 1;
        }
    }

    /// Spec 5.4, section 7: the commit returned an error, so nothing is
    /// submitted; traces `ZA_IMIP_DROPPED` once per event among the attempts
    /// (`za_imip_trace_dropped`) and settles the guard.
    pub fn dropped(&mut self) {
        za_imip_trace_dropped(&self.mails);
        self.mails.clear();
        self.finished = self.attempts.len();
    }

    /// `result` unchanged, after `dropped` when it is an error: for a
    /// commit's result that no retry follows.
    pub fn on_err<T, E>(&mut self, result: Result<T, E>) -> Result<T, E> {
        if result.is_err() {
            self.dropped();
        }
        result
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

impl Drop for ZaImipPending {
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

/// Spec 5.4, section 7: mail attempts whose commit returned an error are not
/// submitted; traces one `ItipMessageError` per distinct event (account id,
/// document id) among them, with ids and the fixed reason only.
pub fn za_imip_trace_dropped(mails: &[ZaImipMail]) {
    let mut traced = Vec::new();
    for mail in mails {
        let event = (mail.account_id, mail.document_id);
        if !traced.contains(&event) {
            traced.push(event);
            trc::event!(
                Calendar(trc::CalendarEvent::ItipMessageError),
                AccountId = mail.account_id,
                DocumentId = mail.document_id,
                Details = ZA_IMIP_DROPPED,
            );
        }
    }
}

/// Final review ruling A-2: traces one mail attempt withheld from an address
/// the sender's pin record pins (`pinned`), or because the pin state could
/// not be read or decided; ids and a fixed string only, never the address.
pub fn za_imip_trace_withheld(account_id: u32, document_id: u32, pinned: bool) {
    trc::event!(
        Calendar(trc::CalendarEvent::ItipMessageError),
        AccountId = account_id,
        DocumentId = document_id,
        Details = if pinned {
            ZA_IMIP_PINNED
        } else {
            ZA_IMIP_PIN_UNREAD
        },
    );
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
        ipc::subscriber::{EventBatch, Interests, SubscriberBuilder},
    };

    /// PR review ruling P1: a local address is the mail class only through
    /// an account or a list, its own entry first and the catch-all only when
    /// it has none; a disabled entry is withheld whatever the catch-all says.
    #[test]
    fn the_mail_class_withholds_every_local_route_but_accounts_and_lists() {
        use EmailCache::*;
        let cases = [
            // The address's own entry.
            (Some(Account(7)), None, ZaMailClass::Account(7)),
            (Some(MailingList(8)), None, ZaMailClass::List),
            (Some(DisabledAccountAddress(7)), None, ZaMailClass::Withheld),
            (Some(DisabledListAddress(8)), None, ZaMailClass::Withheld),
            // No entry and no catch-all.
            (None, None, ZaMailClass::Withheld),
            // No entry: the catch-all decides.
            (None, Some(Some(Account(9))), ZaMailClass::Account(9)),
            (None, Some(Some(MailingList(10))), ZaMailClass::List),
            (None, Some(None), ZaMailClass::Withheld),
            (
                None,
                Some(Some(DisabledAccountAddress(9))),
                ZaMailClass::Withheld,
            ),
            (
                None,
                Some(Some(DisabledListAddress(10))),
                ZaMailClass::Withheld,
            ),
            // An entry is never overridden by the catch-all.
            (
                Some(DisabledAccountAddress(7)),
                Some(Some(Account(9))),
                ZaMailClass::Withheld,
            ),
            (
                Some(DisabledListAddress(8)),
                Some(Some(MailingList(10))),
                ZaMailClass::Withheld,
            ),
            (
                Some(Account(7)),
                Some(Some(Account(9))),
                ZaMailClass::Account(7),
            ),
        ];
        for (entry, catch_all, class) in cases {
            assert_eq!(
                za_mail_class_of(entry, catch_all),
                class,
                "{entry:?} {catch_all:?}"
            );
        }
    }

    /// Section 7 (final review rulings A-4 and 189): the guard is armed from
    /// the fan-out's return, before the commit. Dropped before any
    /// submission it traces each event of its attempts once as abandoned;
    /// settled after the commit's error (`dropped`, `on_err`) it traces each
    /// once as dropped, and nothing more when it drops; `on_err` of `Ok`
    /// leaves it armed; `extend` moves the attempts and leaves the other
    /// guard settled. Its submission loop dropped mid-way (as hyper drops
    /// the request future of a disconnected client) traces the unfinished
    /// events, the one in flight included, and a loop that completes traces
    /// none.
    #[tokio::test]
    async fn the_guard_traces_abandoned_dropped_and_unfinished_attempts() {
        // Account ids no other test of this crate uses.
        const ARMED: u32 = 0x5a5a_0401;
        const DROPPED: u32 = 0x5a5a_0402;
        const ON_ERR: u32 = 0x5a5a_0403;
        const ON_OK: u32 = 0x5a5a_0404;
        const EXTENDED: u32 = 0x5a5a_0405;
        const CANCELLED: u32 = 0x5a5a_0406;
        const COMPLETED: u32 = 0x5a5a_0407;
        const ACCOUNTS: [u32; 7] = [
            ARMED, DROPPED, ON_ERR, ON_OK, EXTENDED, CANCELLED, COMPLETED,
        ];
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

        let mails = |account, documents: &[u32]| {
            documents
                .iter()
                .map(|document_id| ZaImipMail {
                    account_id: account,
                    document_id: *document_id,
                    contents: TaskCalendarItipContents::default(),
                })
                .collect::<Vec<_>>()
        };

        // Armed from the fan-out's mail and dropped before the commit
        // returned (five attempts over three events): each event once.
        let armed = ZaImipPending::from(mails(ARMED, &[5, 7, 7, 9, 9]));
        assert_eq!(armed.len(), 5);
        drop(armed);
        // The commit returned an error: dropped, then nothing on drop.
        let mut dropped = ZaImipPending::from(mails(DROPPED, &[5, 7, 7]));
        dropped.dropped();
        drop(dropped);
        let mut on_err = ZaImipPending::from(mails(ON_ERR, &[5]));
        assert!(on_err.on_err(Err::<(), ()>(())).is_err());
        drop(on_err);
        // `on_err` of `Ok` settles nothing: the guard stays armed.
        let mut on_ok = ZaImipPending::from(mails(ON_OK, &[5]));
        assert!(on_ok.on_err(Ok::<(), ()>(())).is_ok());
        drop(on_ok);
        // A DELETE's guards, one per fan-out, gathered into one.
        let mut extended = ZaImipPending::default();
        assert!(extended.is_empty());
        for document_id in [5, 7] {
            extended.extend(ZaImipPending::from(mails(EXTENDED, &[document_id])));
        }
        drop(extended);

        // The loop: the second submission never answers, and the loop is
        // dropped there (5 finished, 7 in flight, 9 not started); a loop
        // whose submissions all answer completes.
        let mut calls = 0;
        let cancelled = ZaImipPending::from(mails(CANCELLED, &[5, 7, 9])).submit_each(|_| {
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
        ZaImipPending::from(mails(COMPLETED, &[5, 7, 9]))
            .submit_each(|_| async { ZaImipOutcome::Queued(0) })
            .await;

        let expected = [
            (ARMED, 5, ZA_IMIP_ABANDONED),
            (ARMED, 7, ZA_IMIP_ABANDONED),
            (ARMED, 9, ZA_IMIP_ABANDONED),
            (DROPPED, 5, ZA_IMIP_DROPPED),
            (DROPPED, 7, ZA_IMIP_DROPPED),
            (ON_ERR, 5, ZA_IMIP_DROPPED),
            (ON_OK, 5, ZA_IMIP_ABANDONED),
            (EXTENDED, 5, ZA_IMIP_ABANDONED),
            (EXTENDED, 7, ZA_IMIP_ABANDONED),
            (CANCELLED, 7, ZA_IMIP_ABANDONED),
            (CANCELLED, 9, ZA_IMIP_ABANDONED),
        ];
        let mut traced = Vec::new();
        let collect = |batch: EventBatch, traced: &mut Vec<(u32, u32, String)>| {
            for event in batch {
                let value = |wanted: Key| {
                    event
                        .keys
                        .iter()
                        .find(|(key, _)| *key == wanted)
                        .map(|(_, value)| value.clone())
                };
                if let (
                    Some(trc::Value::UInt(account)),
                    Some(trc::Value::UInt(document)),
                    Some(trc::Value::String(details)),
                ) = (
                    value(Key::AccountId),
                    value(Key::DocumentId),
                    value(Key::Details),
                ) && ACCOUNTS.contains(&(account as u32))
                {
                    traced.push((account as u32, document as u32, details.to_string()));
                }
            }
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while traced.len() < expected.len() {
            let Ok(Some(batch)) = tokio::time::timeout_at(deadline, rx.recv()).await else {
                break;
            };
            collect(batch, &mut traced);
        }
        // Anything a settled guard or the completed loop might have sent
        // arrives with the other events or after them.
        tokio::time::sleep(Duration::from_millis(200)).await;
        while let Ok(batch) = rx.try_recv() {
            collect(batch, &mut traced);
        }
        Collector::remove_subscriber(id);
        Collector::reload();
        assert_eq!(
            traced,
            expected
                .iter()
                .map(|(account, document, details)| (*account, *document, details.to_string()))
                .collect::<Vec<_>>()
        );
    }
}
