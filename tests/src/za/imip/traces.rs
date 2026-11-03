/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 5, traces (iMIP spec section 7, D17, invariant 47): one scenario
//! runs every row of D17's table, with a subscriber on every event type, and
//! the events of the new paths carry no address and no content. Only the SMTP
//! layer's transit (the session and the queue that carry a mailed
//! invitation) names a recipient or a sender, as it names them for every
//! message; the fork's own lookups stay quiet.

use super::{add_contact, delete_contact, deliver, event, is_request_for, itip_mail};
use crate::{
    utils::{
        http::HttpRequest,
        server::TestServer,
        smtp_sink::sink,
        za::{
            HookParked, copy_href, inbox_ids, new_inbox_items, pending_ids, trigger_drain,
            wait_for_queue,
        },
    },
    za::{
        STRONG,
        drain::DrainOn,
        fanout::{NO_REPLY, Party, attendee_statuses, uid_copies},
        uid_lease::put,
        user_permissions,
    },
};
use common::za::{
    imip::{
        ZA_IMIP_DROPPED, ZA_IMIP_FAILED, ZA_IMIP_ITEM_DELETED, ZA_IMIP_ITEM_REFUSED,
        ZA_IMIP_KEY_SENDER, ZA_IMIP_NOT_TAKEN, ZA_IMIP_REFUSED, ZA_IMIP_TOO_LARGE,
    },
    test_hooks,
};
use hyper::{Method, StatusCode};
use std::{
    collections::HashSet,
    future::Future,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use trc::{
    CalendarEvent, Collector, EventType, Key,
    ipc::subscriber::{EventBatch, Interests, SubscriberBuilder},
};

/// The key account of this module (all accounts at example.com).
const TRACE_KEY: &str = "imip-trace-key@example.com";
/// A key account other than the module's: its address as a mail sender is
/// refused (spec 4.1 step 4).
const KEY_SENDER: &str = "key1@example.com";
/// The sender's full domain, the key of a traced lookup of it.
const SENDER_DOMAIN: &str = "imiptrace-domain-canary.remote.test";
/// The cache key of a traced lookup of `KEY_SENDER`'s local part
/// (`rcpt_id_from_parts`); nothing else in the module looks it up.
const KEY_SENDER_LOCAL: &str = "key1";

/// Content canaries: the SUMMARY and the LOCATION of every event the module
/// sends or receives, and the sender's display name.
const SUMMARY_CANARY: &str = "imiptrace-sum-canary";
const LOCATION_CANARY: &str = "imiptrace-loc-canary";
const NAME_CANARY: &str = "imiptrace-name-canary";
/// The sender of the module's incoming mail.
const FROM_CANARY: &str = "imiptrace-from-canary";
/// The sender's domain label: a lookup of the sender's address that traces
/// the domain (upstream's `account_id_from_email`) names it, where the fork's
/// quiet lookup does not.
const DOMAIN_CANARY: &str = "imiptrace-domain-canary";
const FROM: &str = "imiptrace-from-canary@imiptrace-domain-canary.remote.test";
const FROM_HEADER: &str =
    "\"imiptrace-name-canary\" <imiptrace-from-canary@imiptrace-domain-canary.remote.test>";
/// The attendee the module's invitations mail (a sink address), and an
/// attendee at a local domain no account has (refused at RCPT).
const ATT_CANARY: &str = "imiptrace-att-canary";
const ATT: &str = "imiptrace-att-canary@remote.test";
const NOBODY_CANARY: &str = "imiptrace-local-canary";
const NOBODY: &str = "imiptrace-local-canary@example.com";

/// Every canary the fork's own events must not carry.
const CANARIES: [&str; 7] = [
    SUMMARY_CANARY,
    LOCATION_CANARY,
    NAME_CANARY,
    FROM_CANARY,
    DOMAIN_CANARY,
    ATT_CANARY,
    NOBODY_CANARY,
];

/// The subscriber's id.
const SUBSCRIBER_ID: &str = "za-imip-trace";

/// The SMTP session's, the delivery's and the queue's events: transit
/// metadata (spec section 7).
fn is_transit(typ: EventType) -> bool {
    matches!(
        typ,
        EventType::Smtp(_) | EventType::Delivery(_) | EventType::Queue(_)
    )
}

/// Where a captured event was emitted (as `za::tracing`'s windows).
/// `PreCommit`: while a write was parked before its commit, or during a step
/// that submits nothing; no event may name an attendee, the SMTP layer's
/// included. `PostCommit`: after a write's commit, its submissions and their
/// delivery; the only window where the cache lookups of the session's
/// recipient resolution (upstream's `rcpt_resolve`) may name a recipient.
/// `Other`: the delivery and drain steps and the flushes between steps,
/// where no cache event may name anything.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Window {
    Other,
    PreCommit,
    PostCommit,
}

/// One captured event: its type, its window, and every key and value.
struct Captured {
    typ: EventType,
    window: Window,
    /// The scenario step that emitted it, for the failure messages.
    step: &'static str,
    span: Option<u64>,
    account_id: Option<u64>,
    keys: Vec<(Key, trc::Value)>,
    /// Every string anywhere in the values, joined.
    text: String,
}

impl Captured {
    fn value(&self, key: Key) -> Option<&trc::Value> {
        self.keys
            .iter()
            .find_map(|(candidate, value)| (*candidate == key).then_some(value))
    }

    fn has(&self, key: Key) -> bool {
        self.value(key).is_some()
    }

    fn string(&self, key: Key) -> Option<&str> {
        match self.value(key) {
            Some(trc::Value::String(text)) => Some(text.as_str()),
            _ => None,
        }
    }

    fn uint(&self, key: Key) -> Option<u64> {
        match self.value(key) {
            Some(trc::Value::UInt(value)) => Some(*value),
            _ => None,
        }
    }

    fn reason(&self) -> Option<&str> {
        self.string(Key::Reason)
    }

    fn details(&self) -> Option<&str> {
        self.string(Key::Details)
    }
}

impl std::fmt::Debug for Captured {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {:?}: {}", self.step, self.typ, self.text)
    }
}

/// Every string anywhere in an event's values, flattened.
fn strings(value: &trc::Value, out: &mut Vec<String>) {
    match value {
        trc::Value::String(text) => out.push(text.to_string()),
        trc::Value::Bytes(bytes) => out.push(String::from_utf8_lossy(bytes).into_owned()),
        trc::Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        trc::Value::Event(err) => err.keys().iter().for_each(|(_, item)| strings(item, out)),
        _ => {}
    }
}

/// A subscriber on every event type, drained into `events` at the points
/// the module chooses (as `za::tracing` does).
struct Capture {
    rx: mpsc::Receiver<EventBatch>,
    _tx: mpsc::Sender<EventBatch>,
    events: Vec<Captured>,
}

impl Capture {
    fn start() -> Self {
        let mut interests = Interests::default();
        for event_type in EventType::variants() {
            interests.set(*event_type);
        }
        let (tx, rx, _) = SubscriberBuilder::new(SUBSCRIBER_ID.into())
            .set_interests(EventType::variants().iter().copied())
            .with_lossy(false)
            .register();
        Collector::union_interests(interests);
        Collector::reload();
        // The collector applies the registration on its own thread: an event
        // it collects before then never reaches the subscriber.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !Collector::get_subscribers()
            .iter()
            .any(|id| id == SUBSCRIBER_ID)
        {
            assert!(
                Instant::now() < deadline,
                "the collector did not apply the subscriber"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        Capture {
            rx,
            _tx: tx,
            events: Vec::new(),
        }
    }

    /// Moves every event the subscriber holds into `events`, tagged `window`
    /// and `step`.
    fn take(&mut self, window: Window, step: &'static str) {
        while let Ok(batch) = self.rx.try_recv() {
            for event in batch {
                let mut values = Vec::new();
                let (mut span, mut account_id) = (None, None);
                for (key, value) in event.keys.iter() {
                    strings(value, &mut values);
                    match (key, value) {
                        (Key::SpanId, trc::Value::UInt(id)) => span = Some(*id),
                        (Key::AccountId, trc::Value::UInt(id)) => account_id = Some(*id),
                        _ => {}
                    }
                }
                self.events.push(Captured {
                    typ: event.inner.typ,
                    window,
                    step,
                    span,
                    account_id,
                    keys: event.keys.clone(),
                    text: values.join("\n"),
                });
            }
        }
    }

    /// Lets the collector deliver what the last step emitted, then takes it.
    async fn settle(&mut self, window: Window, step: &'static str) {
        tokio::time::sleep(Duration::from_millis(500)).await;
        self.take(window, step);
    }

    async fn finish(mut self) -> Vec<Captured> {
        self.settle(Window::PostCommit, "after the last DELETE")
            .await;
        Collector::remove_subscriber(SUBSCRIBER_ID.into());
        Collector::reload();
        self.events
    }
}

/// `event` with the SUMMARY and the LOCATION set to the content canaries.
fn canaried(uid: &str, organizer: &str, attendees: &[&str], sequence: u32, extra: &str) -> String {
    event(
        uid,
        organizer,
        attendees,
        sequence,
        &format!("LOCATION:{LOCATION_CANARY}\r\n{extra}"),
    )
    .replace(
        &format!("SUMMARY:imip {uid}"),
        &format!("SUMMARY:{SUMMARY_CANARY}"),
    )
}

/// The calendar part of an oversized REQUEST: `canaried`'s event padded
/// with property lines to more than `limit` bytes (spec 4.1 step 1 refuses a
/// part of `limit` bytes or more).
fn oversized_ics(uid: &str, to: &str, limit: usize) -> String {
    let base = |pad: &str| canaried(uid, FROM, &[to], 0, pad);
    let mut pad = String::new();
    while base(&pad).len() <= limit {
        pad.push_str("X-IMIP-PAD:");
        pad.push_str(&"a".repeat(60));
        pad.push_str("\r\n");
    }
    base(&pad)
}

/// A DELETE through the raw client (5 s), which may wait at a pause point;
/// resolves to the status.
fn delete(path: String) -> impl Future<Output = u16> + Send + 'static + use<> {
    let request = HttpRequest::with_credentials(8899, TRACE_KEY, STRONG);
    async move {
        request
            .send_full(Method::DELETE, &path, None, None)
            .await
            .status
            .as_u16()
    }
}

/// The key organizer's event `name` in its default calendar.
fn own_path(name: &str) -> String {
    format!(
        "/dav/cal/{}/default/{name}.ics",
        TRACE_KEY.replace('@', "%40")
    )
}

/// Whether `typ` is one of the fork's iMIP events (`ItipMessage*`): never
/// exempt, whatever span it carries (spec section 7: only the SMTP layer's
/// transit names a recipient or a sender).
fn is_itip(typ: EventType) -> bool {
    matches!(
        typ,
        EventType::Calendar(
            CalendarEvent::ItipMessageReceived
                | CalendarEvent::ItipMessageError
                | CalendarEvent::ItipMessageSent
        )
    )
}

/// Whether the event is one of the fork's own (its `Details` is a fixed
/// `zero-access:` string): never exempt, whatever span or type it carries
/// (final review C-8), so a fork trace in a delivery's span that named the
/// sender would fail.
fn is_fork_event(event: &Captured) -> bool {
    event
        .string(Key::Details)
        .is_some_and(|details| details.starts_with("zero-access:"))
}

/// The events that name `canary` and are not the SMTP layer's transit: those
/// of a `PreCommit` window when `strict` (nothing may name an attendee before
/// a write's commit, transit included), and those that are not an SMTP,
/// delivery or queue event, nor an event of the span of one naming the
/// canary, nor, in a `PostCommit` window and when `cache`, a cache lookup
/// (the session's own RCPT lookups, upstream's `rcpt_resolve`, carry no span
/// id). An `ItipMessage*` event and one of the fork's own (`is_fork_event`)
/// are never exempt.
fn outside_transit<'a>(
    events: &'a [Captured],
    canary: &str,
    strict: bool,
    cache: bool,
) -> Vec<&'a Captured> {
    let spans = events
        .iter()
        .filter(|event| is_transit(event.typ) && event.text.contains(canary))
        .filter_map(|event| event.span)
        .collect::<HashSet<_>>();
    events
        .iter()
        .filter(|event| event.text.contains(canary))
        .filter(|event| {
            (strict && event.window == Window::PreCommit)
                || is_itip(event.typ)
                || is_fork_event(event)
                || !(is_transit(event.typ)
                    || event.span.is_some_and(|span| spans.contains(&span))
                    || (cache
                        && event.window == Window::PostCommit
                        && matches!(event.typ, EventType::Cache(_))))
        })
        .collect()
}

/// Upstream's own lookup of the sender's domain: the LMTP session's DATA
/// stage evaluates the DKIM signing-domain default,
/// `is_local_domain(sender_domain) && !is_empty(authenticated_as)`
/// (`eval_signers` over `dkim.sign`, `crates/smtp/src/inbound/data.rs`
/// about `:863-866`; the default in `crates/registry/src/schema/
/// structs_impl.rs` about `:47749`), whose `is_local_domain` goes through
/// `domain()`, which traces the name: a span-less `Cache` event of the
/// session's transit that names the domain whoever the sender is.
fn is_session_domain_lookup(event: &Captured) -> bool {
    matches!(
        event.typ,
        EventType::Cache(trc::CacheEvent::Hit | trc::CacheEvent::Miss)
    ) && event.span.is_none()
        && event.window == Window::Other
        && event.step.starts_with("delivery:")
        && event.string(Key::Key) == Some(SENDER_DOMAIN)
        && matches!(
            event.string(Key::Name),
            Some("domainName" | "domainNameNegative")
        )
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access iMIP trace tests...");
    let admin = test.account("admin@example.com").clone();
    let account = admin
        .create_key_user_account(TRACE_KEY, STRONG, "iMIP trace key", &[], user_permissions())
        .await;
    test.insert_account(account);
    let key = Party::new(test, TRACE_KEY);
    let start = inbox_ids(test, key.id).await;
    // The sender is a ContactCard: a new UID from it is taken (spec 4.1 step
    // 5) without the auto-add setting.
    let card = add_contact(&key.client, TRACE_KEY, FROM).await;
    sink().clear();
    wait_for_queue(test).await;

    let mut capture = Capture::start();

    // Delivery (spec 4.1, with the suite's drain skipped): a REQUEST (item
    // written), a REQUEST from another key account (refused), an oversized
    // part (refused at step 1), a CANCEL for a UID the account does not know
    // (refused at step 5).
    let limit = test.server.core.groupware.itip_inbound_max_ical_size;
    for ((envelope_from, message), step) in [
        (
            format!("dmarc-{FROM}"),
            itip_mail(
                FROM_HEADER,
                TRACE_KEY,
                "REQUEST",
                &canaried("imiptrace-in", FROM, &[TRACE_KEY], 0, ""),
            ),
        ),
        (
            "dmarc-key1@remote.test".to_string(),
            itip_mail(
                KEY_SENDER,
                TRACE_KEY,
                "REQUEST",
                &canaried("imiptrace-key", KEY_SENDER, &[TRACE_KEY], 0, ""),
            ),
        ),
        (
            format!("dmarc-{FROM}"),
            itip_mail(
                FROM_HEADER,
                TRACE_KEY,
                "REQUEST",
                &oversized_ics("imiptrace-big", TRACE_KEY, limit),
            ),
        ),
        (
            format!("dmarc-{FROM}"),
            itip_mail(
                FROM_HEADER,
                TRACE_KEY,
                "CANCEL",
                &canaried(
                    "imiptrace-unknown",
                    FROM,
                    &[TRACE_KEY],
                    1,
                    "STATUS:CANCELLED",
                ),
            ),
        ),
    ]
    .into_iter()
    .zip([
        "delivery: the REQUEST",
        "delivery: a key account's REQUEST",
        "delivery: the oversized part",
        "delivery: the unknown UID's CANCEL",
    ]) {
        deliver(&envelope_from, TRACE_KEY, &message).await;
        wait_for_queue(test).await;
        capture.settle(Window::Other, step).await;
    }
    let delivered = new_inbox_items(test, key.id, &start).await;
    assert_eq!(
        delivered.len(),
        1,
        "only the REQUEST from the outside sender was taken"
    );
    let request_item = delivered[0].0;

    // The drain runs for the rest of the module: the PUT's drain imports the
    // REQUEST.
    let _drain = DrainOn::new();

    // A new invitation to an outside attendee and to a local address no
    // account has, parked before its commit: the events up to there are
    // captured apart (no event may name an attendee), then the commit, the
    // submissions and their delivery. Row: a mail attempt accepted (ATT) and
    // one refused at RCPT (the local address).
    let out = own_path("imiptrace-out");
    let parked = HookParked::start(
        key.id,
        test_hooks::set_commit_pause,
        put(
            TRACE_KEY,
            STRONG,
            out.clone(),
            canaried("imiptrace-out", TRACE_KEY, &[ATT, NOBODY], 0, ""),
        ),
    )
    .await;
    capture
        .settle(
            Window::PreCommit,
            "PUT before its commit (the drain imports the REQUEST)",
        )
        .await;
    assert_eq!(parked.finish().await, 201);
    wait_for_queue(test).await;
    capture
        .settle(Window::PostCommit, "PUT after its commit")
        .await;
    let mailed = sink().take_to(ATT);
    assert_eq!(mailed.len(), 1, "the attendee was mailed once");
    assert!(is_request_for(&mailed[0], "imiptrace-out"));
    assert_eq!(attendee_statuses(&key.client, &out, ATT).await, ["1.1"]);
    assert_eq!(
        pending_ids(test, key.id).await,
        Vec::<u32>::new(),
        "the PUT's drain took the REQUEST"
    );
    assert!(
        inbox_ids(test, key.id).await.contains(&request_item),
        "the drained item stays"
    );

    // A REPLY from a sender who is not a participant: an item is written
    // (the UID is known), and the drain ends it deleted (row: a mail-borne
    // item the drain deletes).
    let before = inbox_ids(test, key.id).await;
    deliver(
        &format!("dmarc-{FROM}"),
        TRACE_KEY,
        &itip_mail(
            FROM_HEADER,
            TRACE_KEY,
            "REPLY",
            &canaried(
                "imiptrace-out",
                TRACE_KEY,
                &[],
                0,
                &format!("ATTENDEE;PARTSTAT=ACCEPTED:mailto:{FROM}\r\n"),
            ),
        ),
    )
    .await;
    wait_for_queue(test).await;
    capture.settle(Window::Other, "delivery: the REPLY").await;
    let reply_items = new_inbox_items(test, key.id, &before).await;
    assert_eq!(reply_items.len(), 1, "the REPLY took an item");
    let reply_item = reply_items[0].0;
    assert_eq!(pending_ids(test, key.id).await, [reply_item]);
    assert_eq!(trigger_drain(TRACE_KEY, STRONG).await, 207);
    capture.settle(Window::Other, "drain: the REPLY").await;
    assert!(
        !inbox_ids(test, key.id).await.contains(&reply_item),
        "the drain did not delete the non-participant's REPLY"
    );

    // A submission that fails (the hook): the status stays `1.1`, the failure
    // is traced, nothing is submitted, so no event names the attendee.
    let failed = own_path("imiptrace-fail");
    test_hooks::set_imip_submit_fail_once(Some(key.id));
    assert_eq!(
        put(
            TRACE_KEY,
            STRONG,
            failed.clone(),
            canaried("imiptrace-fail", TRACE_KEY, &[ATT], 0, "")
        )
        .await,
        201
    );
    test_hooks::set_imip_submit_fail_once(None);
    wait_for_queue(test).await;
    capture
        .settle(Window::PreCommit, "PUT whose submission fails")
        .await;
    assert_eq!(sink().count_to(ATT), 0, "the failed submission was mailed");
    assert_eq!(attendee_statuses(&key.client, &failed, ATT).await, ["1.1"]);

    // A DELETE whose commit fails: its CANCELs are dropped (row: a DELETE's
    // mail dropped after its commit erred), nothing is submitted.
    test_hooks::set_delete_commit_fail_once(Some(key.id));
    assert_eq!(delete(out.clone()).await, 500);
    test_hooks::set_delete_commit_fail_once(None);
    wait_for_queue(test).await;
    capture
        .settle(Window::PreCommit, "DELETE whose commit fails")
        .await;
    assert_eq!(uid_copies(test, key.id, "imiptrace-out").await, 1);
    assert_eq!(sink().count_to(ATT), 0, "the dropped CANCEL was mailed");

    // The DELETE again, parked before its commit like the PUT: its CANCEL to
    // the attendee is accepted, the local address refused at RCPT.
    let parked = HookParked::start(
        key.id,
        test_hooks::set_delete_commit_pause,
        delete(out.clone()),
    )
    .await;
    capture
        .settle(Window::PreCommit, "DELETE before its commit")
        .await;
    assert_eq!(parked.finish().await, 204);
    wait_for_queue(test).await;
    capture
        .settle(Window::PostCommit, "DELETE after its commit")
        .await;
    let cancelled = sink().take_to(ATT);
    assert_eq!(cancelled.len(), 1, "the attendee was mailed the CANCEL");
    assert_eq!(cancelled[0].calendar_method().as_deref(), Some("CANCEL"));

    let captured = capture.finish().await;
    let mailed_total = mailed.len() + cancelled.len();

    // Every iMIP event of the capture, whatever its account: none carries an
    // address or a canary, and `From`, where present, is the redaction
    // marker.
    for event in captured.iter().filter(|event| is_itip(event.typ)) {
        assert!(
            !event.text.contains('@') && !CANARIES.iter().any(|canary| event.text.contains(canary)),
            "an iMIP event carries an address or content: {event:?}"
        );
        if let Some(from) = event.value(Key::From) {
            assert_eq!(
                from,
                &trc::Value::redacted(),
                "From is the redaction marker: {event:?}"
            );
        }
    }
    // The fork's iMIP events of this account.
    let ours = captured
        .iter()
        .filter(|event| is_itip(event.typ) && event.account_id == Some(key.id as u64))
        .collect::<Vec<_>>();
    assert!(
        ours.len() >= 8,
        "the scenario's events were captured: {ours:#?}"
    );
    let of_type = |typ: CalendarEvent| {
        ours.iter()
            .filter(move |event| event.typ == EventType::Calendar(typ))
    };

    // Row: an item written (span id, account id, From redacted). The REQUEST
    // and the REPLY each wrote one.
    let received = of_type(CalendarEvent::ItipMessageReceived).collect::<Vec<_>>();
    assert!(received.len() >= 2, "an event per item written: {ours:#?}");
    assert!(
        received
            .iter()
            .all(|event| event.has(Key::SpanId) && event.has(Key::From)),
        "span id and From: {received:#?}"
    );

    // Row: a delivery refusal. A key account's mail names its fixed reason;
    // an unknown UID's CANCEL names its iTIP reason; both are refusals of
    // the delivery's step, not of the item writer.
    let not_taken = ours
        .iter()
        .filter(|event| event.details() == Some(ZA_IMIP_NOT_TAKEN))
        .collect::<Vec<_>>();
    assert!(
        not_taken
            .iter()
            .any(|event| event.reason() == Some(ZA_IMIP_KEY_SENDER)),
        "the key account's mail: {ours:#?}"
    );
    assert!(
        not_taken
            .iter()
            .any(|event| event.reason() == Some("EventNotFound")),
        "the unknown UID's CANCEL: {ours:#?}"
    );
    // D17 (ledger 234): a refusal row carries `From`, redacted (checked for
    // every iMIP event above).
    assert!(
        not_taken.iter().all(|event| event.has(Key::From)),
        "From on the refusals: {not_taken:#?}"
    );
    // Row: the size limit (account id, fixed details, Limit, Size). The
    // delivery's step 1 refused it, not the item writer (which has its own
    // size check and traces `ZA_IMIP_ITEM_REFUSED`).
    let too_large = ours
        .iter()
        .filter(|event| event.has(Key::Limit) && event.has(Key::Size))
        .collect::<Vec<_>>();
    assert!(!too_large.is_empty(), "the size refusal: {ours:#?}");
    for event in &too_large {
        assert_eq!(event.details(), Some(ZA_IMIP_TOO_LARGE), "{event:?}");
        assert!(event.has(Key::From), "From on the size row: {event:?}");
        assert_eq!(event.uint(Key::Limit), Some(limit as u64), "{event:?}");
        assert!(
            event
                .uint(Key::Size)
                .is_some_and(|size| size > limit as u64),
            "{event:?}"
        );
    }
    assert!(
        !ours.iter().any(|event| {
            event.reason() == Some(ZA_IMIP_ITEM_REFUSED)
                || event.details() == Some(ZA_IMIP_ITEM_REFUSED)
        }),
        "the item writer refused an item: {ours:#?}"
    );

    // Row: a mail-borne item the drain deletes (account id, document id,
    // fixed reason). A session that loses a race to the drain traces a
    // deletion too, so the count is not asserted.
    assert!(
        ours.iter().any(|event| {
            event.details() == Some(ZA_IMIP_ITEM_DELETED)
                && event.uint(Key::DocumentId) == Some(reply_item as u64)
                && event.reason().is_some_and(|reason| !reason.is_empty())
        }),
        "the drain's deletion of the REPLY's item: {ours:#?}"
    );

    // Row: a mail attempt accepted (account id, document id, queue id), one
    // per message the sink received.
    let sent = of_type(CalendarEvent::ItipMessageSent).collect::<Vec<_>>();
    assert!(
        sent.iter()
            .all(|event| event.has(Key::QueueId) && event.has(Key::DocumentId)),
        "sent: account, document, queue id: {sent:#?}"
    );
    assert_eq!(
        sent.len(),
        mailed_total,
        "an ItipMessageSent per message the sink received: {ours:#?}"
    );

    // Row: a mail attempt failed (account id, document id, stage, reply
    // code): the local address refused at RCPT after the PUT's and the
    // DELETE's commits; and a submission that failed before an answer (the
    // hook).
    let refused = ours
        .iter()
        .filter(|event| {
            event.details() == Some(ZA_IMIP_REFUSED)
                && event.reason() == Some("RCPT-TO")
                && event.uint(Key::Code).is_some_and(|code| code / 100 == 5)
                && event.has(Key::DocumentId)
        })
        .count();
    assert!(
        refused >= 2,
        "the local address was refused at RCPT after the PUT and the DELETE: {ours:#?}"
    );
    assert!(
        ours.iter()
            .any(|event| event.details() == Some(ZA_IMIP_FAILED) && event.has(Key::DocumentId)),
        "the failed submission: {ours:#?}"
    );
    // Row: a DELETE's mail dropped after its commit erred.
    assert!(
        ours.iter()
            .any(|event| event.details() == Some(ZA_IMIP_DROPPED) && event.has(Key::DocumentId)),
        "the dropped CANCELs: {ours:#?}"
    );

    // No content anywhere: no event of any type, the SMTP layer's included,
    // carries a SUMMARY, a LOCATION or the sender's display name.
    for canary in [SUMMARY_CANARY, LOCATION_CANARY, NAME_CANARY] {
        let named = captured
            .iter()
            .filter(|event| event.text.contains(canary))
            .collect::<Vec<_>>();
        assert!(named.is_empty(), "content ({canary}) traced: {named:#?}");
    }

    // Only the SMTP layer's transit names the recipients. Before a write's
    // commit (and in the steps that submit nothing) no event names them;
    // after it, the session's, the delivery's and the queue's events, the
    // events of their spans and the cache lookups of the session's recipient
    // resolution (upstream's `rcpt_resolve`, which carries no span id) may.
    // Never an `ItipMessage*` event, one of the fan-out or one of the drain.
    // Positive control: the capture saw the transit name each of them.
    for canary in [ATT_CANARY, NOBODY_CANARY] {
        assert!(
            captured
                .iter()
                .any(|event| is_transit(event.typ) && event.text.contains(canary)),
            "no transit event named {canary}"
        );
        // The remote attendee's domain is not local, so no recipient lookup
        // by local part runs for it: no event of any kind may name it outside
        // the transit. The local address resolves (and misses) in the
        // session's RCPT lookups.
        let named = outside_transit(&captured, canary, true, canary == NOBODY_CANARY);
        assert!(
            named.is_empty(),
            "{canary} traced outside the SMTP layer's transit: {named:#?}"
        );
        // What the cache exemption let through, for the record.
        let exempt = captured
            .iter()
            .filter(|event| {
                event.window == Window::PostCommit
                    && matches!(event.typ, EventType::Cache(_))
                    && event.text.contains(canary)
                    && event.span.is_none()
            })
            .map(|event| format!("{event:?}"))
            .collect::<Vec<_>>();
        println!(
            "{canary}: {} cache events after a commit: {exempt:#?}",
            exempt.len()
        );
        // Positive control, on upstream's behaviour: the session's recipient
        // resolution of the local address does reach the collector as
        // non-transit `Cache` events, so the checks above are not vacuous
        // for a cache event; the remote attendee's domain is not local, so
        // none exists for it.
        assert_eq!(
            !exempt.is_empty(),
            canary == NOBODY_CANARY,
            "upstream's rcpt_resolve cache events for {canary}: {exempt:#?}"
        );
    }
    // The incoming mail's sender: named by the inbound SMTP session (its
    // MAIL FROM) as every sender is, and by nothing else.
    assert!(
        captured
            .iter()
            .any(|event| is_transit(event.typ) && event.text.contains(FROM_CANARY)),
        "no transit event named the sender"
    );
    let named = outside_transit(&captured, FROM_CANARY, false, false);
    assert!(
        named.is_empty(),
        "the sender traced outside the SMTP layer's transit: {named:#?}"
    );

    // Invariant 47, spec section 7 ("the fork's own lookups stay quiet"):
    // delivery's step 4 and the drain's repeat of it look the sender up with
    // `za_account_id_from_email`. The traced lookup of a remote sender
    // (`account_id_from_email`) emits a `Cache` event keyed by the sender's
    // domain, so the domain label is a canary; the traced lookup of a local
    // sender also emits events keyed by its local part. Upstream's session
    // itself looks the sender's domain up once per message (the DATA stage's
    // DKIM signing-domain default, `is_session_domain_lookup`), the one
    // exemption: exactly one such span-less
    // `Cache` event per delivery from the sender's domain, none in any other
    // step (the drains included), so a second lookup of the sender, the
    // fork's, shows as a second event in its delivery step or one in a drain
    // step. Never an `ItipMessage*` event.
    assert!(
        captured
            .iter()
            .any(|event| is_transit(event.typ) && event.text.contains(DOMAIN_CANARY)),
        "no transit event named the sender's domain"
    );
    let mut per_delivery = Vec::<&str>::new();
    let named = outside_transit(&captured, DOMAIN_CANARY, false, false)
        .into_iter()
        .filter(|event| {
            // The session's one lookup of its delivery.
            if is_session_domain_lookup(event) && !per_delivery.contains(&event.step) {
                per_delivery.push(event.step);
                false
            } else {
                true
            }
        })
        .collect::<Vec<_>>();
    assert!(
        named.is_empty(),
        "a lookup of the sender other than the session's traced its domain: {named:#?}"
    );
    // Positive control, on upstream's behaviour: the session's lookup was
    // collected in each delivery from the sender's domain (all but the key
    // account's), so the check above is not vacuous.
    assert_eq!(
        per_delivery,
        [
            "delivery: the REQUEST",
            "delivery: the oversized part",
            "delivery: the unknown UID's CANCEL",
            "delivery: the REPLY",
        ],
        "the session's lookup of the sender's domain, once per delivery"
    );
    let named = captured
        .iter()
        .filter(|event| {
            matches!(event.typ, EventType::Cache(_))
                && event.string(Key::Key) == Some(KEY_SENDER_LOCAL)
        })
        .map(|event| format!("{event:?}"))
        .collect::<Vec<_>>();
    assert!(
        named.is_empty(),
        "a lookup of the key sender traced its local part: {named:#?}"
    );

    // Cleanup: the module's events (without a CANCEL), inbox items (pending
    // or drained), ContactCard. The key account goes with the suite's.
    key.client
        .request_with_headers("DELETE", &failed, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    if uid_copies(test, key.id, "imiptrace-in").await > 0 {
        let href = copy_href(test, key.id, "imiptrace-in").await;
        key.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key.delete_items_since(test, &start).await;
    delete_contact(&key.client, &card).await;
    wait_for_queue(test).await;
    sink().clear();
    test.wait_for_tasks().await;
}
