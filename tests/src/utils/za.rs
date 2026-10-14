/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::utils::{account::Account, http::HttpRequest, server::TestServer};
use base64::{Engine, engine::general_purpose::STANDARD};
use calcard::{
    common::{IanaString, timezone::Tz},
    icalendar::{
        ICalendar, ICalendarComponentType, ICalendarEntry, ICalendarMethod, ICalendarParameterName,
        ICalendarParameterValue, ICalendarProperty, ICalendarValue,
    },
};
use common::{
    auth::vault::ZaVerification,
    ipc::CacheInvalidation,
    storage::index::ObjectIndexBuilder,
    za::{
        lease::lease_key_range,
        test_hooks::{self, Pause as HookPause},
    },
};
use email::cache::MessageCacheFetch;
use groupware::{
    cache::GroupwareCache,
    calendar::{
        CalendarEvent, CalendarEventData, CalendarEventNotification, ChangedBy,
        EVENT_NOTIFICATION_ZA_PENDING,
        seal::{
            PendingCarriers, TokenPairs, open_pending_item, seal_drained_item, seal_event,
            seal_pending_item, tree::KEY_PROP,
        },
        storage::ItipAutoExpunge,
    },
    scheduling::inbound::itip_method,
};
use http::auth::authenticate::za_test::{self, Pause};
use hyper::{Method, StatusCode};
use registry::schema::structs::Task;
use serde::Serialize;
use serde_json::{Value, json};
use std::{future::Future, sync::Arc, time::Duration};
use store::{
    Deserialize, IterateParams, ValueKey,
    write::{AlignedBytes, Archive, BatchBuilder, TaskQueueClass, ValueClass},
};
use tokio::task::JoinHandle;
use types::collection::{Collection, SyncCollection};
use vault::session::SessionKeys;

/// The suite's HTTPS listener.
pub const SERVER_URL: &str = "https://127.0.0.1:8899";

pub struct VaultReply {
    pub status: u16,
    pub body: Value,
}

impl VaultReply {
    pub fn expect(self, status: u16) -> Value {
        assert_eq!(
            self.status, status,
            "unexpected status, body: {}",
            self.body
        );
        self.body
    }

    pub fn str(self, status: u16, field: &str) -> String {
        self.expect(status)[field]
            .as_str()
            .unwrap_or_else(|| panic!("missing field {field}"))
            .to_string()
    }
}

/// Unauthenticated POST to `/api/vault/<path>`.
pub async fn za_post(path: &str, body: &impl Serialize) -> VaultReply {
    za_post_with(HttpRequest::new(), path, body).await
}

/// POST to `/api/vault/<path>` with Basic credentials of `account`.
pub async fn za_post_as(account: &Account, path: &str, body: &impl Serialize) -> VaultReply {
    za_post_with(
        HttpRequest::with_credentials(account.http_listener_port, account.name(), account.secret()),
        path,
        body,
    )
    .await
}

async fn za_post_with(request: HttpRequest, path: &str, body: &impl Serialize) -> VaultReply {
    let response = request
        .send_full(
            Method::POST,
            &format!("/api/vault/{path}"),
            Some(serde_json::to_vec(body).unwrap()),
            Some("application/json"),
        )
        .await;
    VaultReply {
        status: response.status.as_u16(),
        body: serde_json::from_str(&response.body).unwrap_or(Value::Null),
    }
}

/// Unauthenticated POST from `remote_ip` (via `X-Forwarded-For`; the za
/// suite enables forwarded addresses), for fail2ban tests that must not ban
/// the loopback address the rest of the suite uses.
pub async fn za_post_from(remote_ip: &str, path: &str, body: &impl Serialize) -> VaultReply {
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .post(format!("{SERVER_URL}/api/vault/{path}"))
        .header("X-Forwarded-For", remote_ip)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(body).unwrap())
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    VaultReply {
        status,
        body: serde_json::from_str(&body).unwrap_or(Value::Null),
    }
}

/// Unauthenticated POSTs to `/api/vault/<path>`, one per body, issued at
/// once. Each client opens its TLS connection beforehand and sends from its
/// own task, so client setup does not stagger the requests; each comes from
/// its own forwarded address (`10.0.8.<n>`), so they do not contend on one
/// anonymous rate-limit counter (a conflicting RocksDB commit backs off for
/// up to 300 ms). Arrival order is still not guaranteed.
pub async fn za_post_concurrent(path: &str, bodies: &[Value]) -> Vec<VaultReply> {
    let barrier = Arc::new(tokio::sync::Barrier::new(bodies.len()));
    let mut tasks = Vec::with_capacity(bodies.len());
    for (i, body) in bodies.iter().enumerate() {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();
        // Read to the end so the connection returns to the pool for reuse.
        let warm_up = client
            .get(format!("{SERVER_URL}/healthz/live"))
            .send()
            .await
            .unwrap();
        assert_eq!(warm_up.status().as_u16(), 200);
        warm_up.bytes().await.unwrap();
        let url = format!("{SERVER_URL}/api/vault/{path}");
        let body = serde_json::to_vec(body).unwrap();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            let response = client
                .post(url)
                .header(hyper::header::CONTENT_TYPE, "application/json")
                .header("X-Forwarded-For", format!("10.0.8.{}", i + 1))
                .body(body)
                .send()
                .await
                .unwrap();
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            VaultReply {
                status,
                body: serde_json::from_str(&body).unwrap_or(Value::Null),
            }
        }));
    }
    let mut replies = Vec::with_capacity(tasks.len());
    for task in tasks {
        replies.push(task.await.unwrap());
    }
    replies
}

pub async fn za_setup_token(admin: &Account, account: &str) -> String {
    za_post_as(admin, "setup-token", &json!({ "account": account }))
        .await
        .str(200, "token")
}

/// Completes setup and returns the recovery key.
pub async fn za_setup(username: &str, token: &str, password: &str) -> String {
    za_post(
        "setup",
        &json!({ "username": username, "token": token, "password": password }),
    )
    .await
    .str(200, "recovery_key")
}

/// PROPFIND on the calendar home. Requests expected to fail are sent from
/// `fail_ip`, so a module's deliberate failures do not consume the loopback
/// address's fail2ban budget.
pub async fn caldav_from(fail_ip: &str, name: &str, secret: &str, status: StatusCode) {
    let mut request = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .request(
            Method::from_bytes(b"PROPFIND").unwrap(),
            format!("{SERVER_URL}/dav/cal/{name}/"),
        )
        .basic_auth(name, Some(secret));
    if !status.is_success() {
        request = request.header("X-Forwarded-For", fail_ip);
    }
    let response = request.send().await.unwrap();
    assert_eq!(
        response.status().as_u16(),
        status.as_u16(),
        "PROPFIND as {name}"
    );
}

pub fn assert_nothing_cached(test: &TestServer, account_id: u32) {
    assert!(
        !test
            .server
            .inner
            .cache
            .http_auth
            .inner()
            .iter()
            .any(|(_, v)| v.account_id == account_id),
        "no cached authentication for the account"
    );
    assert!(
        !test.server.inner.cache.za_keys.contains_account(account_id),
        "no resident keys for the account"
    );
}

/// A request held at a test-mode pause point. `set` names the slot:
/// `za_test::set` (after verification, before the cache-insert fence),
/// `za_test::set_endpoint` (vault endpoints, after verification, before the
/// record re-read) or `za_test::set_publish` (`app-password`, between the
/// Pending wrap and the registry credential). Dropping it clears that
/// process-global slot and releases the request, also on a failed assertion.
pub struct Parked<T> {
    pause: Arc<Pause>,
    clear: fn(Option<Arc<Pause>>),
    handle: Option<JoinHandle<T>>,
}

impl<T: Send + 'static> Parked<T> {
    pub async fn start(
        account_id: u32,
        set: fn(Option<Arc<Pause>>),
        request: impl Future<Output = T> + Send + 'static,
    ) -> Self {
        let pause = Arc::new(Pause::new(account_id));
        set(Some(pause.clone()));
        let mut parked = Parked {
            pause: pause.clone(),
            clear: set,
            handle: None,
        };
        parked.handle = Some(tokio::spawn(request));
        tokio::time::timeout(Duration::from_secs(5), pause.arrived.notified())
            .await
            .expect("the request did not reach the pause point");
        parked
    }

    pub async fn finish(mut self) -> T {
        self.pause.release.notify_one();
        self.handle.take().unwrap().await.unwrap()
    }
}

impl<T> Drop for Parked<T> {
    fn drop(&mut self) {
        (self.clear)(None);
        self.pause.release.notify_one();
    }
}

/// A CalDAV login parked after its credentials were verified. The harness
/// client times out after 5 s, long enough to stay parked.
pub async fn park_login(account_id: u32, name: &str, secret: &str) -> Parked<u16> {
    let request = HttpRequest::with_credentials(8899, name, secret);
    let path = format!("/dav/cal/{name}/");
    Parked::start(account_id, za_test::set, async move {
        request
            .send_full(Method::from_bytes(b"PROPFIND").unwrap(), &path, None, None)
            .await
            .status
            .as_u16()
    })
    .await
}

/// A vault request parked between its fresh verification and the re-read.
pub async fn park_endpoint(account_id: u32, path: &'static str, body: Value) -> Parked<VaultReply> {
    Parked::start(account_id, za_test::set_endpoint, async move {
        za_post(path, &body).await
    })
    .await
}

/// The account's session keys, unlocked from `password` by the product's own
/// login verification (`za_verify_password`: Argon2, verifier, MK unwrap), as
/// a CalDAV login unlocks them.
pub async fn za_unlock(test: &TestServer, account_id: u32, password: &str) -> Arc<SessionKeys> {
    match test
        .server
        .za_verify_password(account_id, password, None)
        .await
        .unwrap()
    {
        ZaVerification::Valid(keys) => keys,
        _ => panic!("the password does not unlock account {account_id}"),
    }
}

/// Writes `ical` into the store as the event `name` in the account's
/// calendar `calendar` (a slug such as "default"), bypassing the DAV
/// handlers and so the seal. Such an event is what a legacy plaintext event
/// (written before the account held keys) looks like; tests use it to reach
/// code that sealing otherwise hides. Schedules the event's next email alarm
/// the way a PUT does. Returns the document id.
pub async fn plant_event(
    test: &TestServer,
    account_id: u32,
    calendar: &str,
    name: &str,
    ical: &str,
    schedule_tag: Option<u32>,
) -> u32 {
    insert_event(test, account_id, calendar, name, ical, schedule_tag, None).await
}

/// `plant_event`, sealed under the account's keys (unlocked from
/// `password`) with `pairs` in a format 2 `X-ZA-EXTRA` bundle: the stored
/// form of a key organizer's event once the fan-out (plan 9) hands out
/// tokens. Returns the document id.
pub async fn plant_sealed_event(
    test: &TestServer,
    account_id: u32,
    calendar: &str,
    name: &str,
    ical: &str,
    password: &str,
    pairs: &TokenPairs,
) -> u32 {
    let keys = za_unlock(test, account_id, password).await;
    insert_event(
        test,
        account_id,
        calendar,
        name,
        ical,
        None,
        Some((&*keys, pairs)),
    )
    .await
}

/// An attendee's copy of an invitation as the drain (plan 10) will import
/// it: sealed under the account's keys (unlocked from `password`) with
/// `pairs` (the attendee's own pair; empty pairs write no bundle) and
/// upstream's import schedule tag, in the account's default calendar.
/// Returns the document id.
pub async fn plant_attendee_copy(
    test: &TestServer,
    account_id: u32,
    name: &str,
    ical: &str,
    password: &str,
    pairs: &TokenPairs,
) -> u32 {
    let keys = za_unlock(test, account_id, password).await;
    insert_event(
        test,
        account_id,
        "default",
        name,
        ical,
        Some(1),
        Some((&*keys, pairs)),
    )
    .await
}

async fn insert_event(
    test: &TestServer,
    account_id: u32,
    calendar: &str,
    name: &str,
    ical: &str,
    schedule_tag: Option<u32>,
    seal: Option<(&SessionKeys, &TokenPairs)>,
) -> u32 {
    let calendar_id = test
        .server
        .fetch_dav_resources(account_id, account_id, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path(calendar)
        .unwrap_or_else(|| panic!("calendar {calendar} not found"))
        .document_id();
    let size = ical.len() as u32;
    let ical = match calcard::Parser::new(ical).entry() {
        calcard::Entry::ICalendar(ical) => ical,
        other => panic!("{other:?}"),
    };
    let mut next_alarm = None;
    let mut event = CalendarEvent {
        names: vec![common::DavName {
            name: name.into(),
            parent_id: calendar_id,
        }],
        data: CalendarEventData::new(ical, Tz::Floating, 100, &mut next_alarm),
        size,
        schedule_tag,
        ..Default::default()
    };
    if let Some((keys, pairs)) = seal {
        seal_event(&mut event, keys, account_id, Some(pairs)).unwrap();
    }
    let account_info = test.server.account_info(account_id).await.unwrap();
    let document_id = test
        .server
        .store()
        .assign_document_ids(account_id, Collection::CalendarEvent, 1)
        .await
        .unwrap();
    let mut batch = BatchBuilder::new();
    event
        .insert(
            account_info.account_tenant_ids(),
            account_id,
            document_id,
            next_alarm,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    // A direct store write neither wakes the task manager nor refreshes the
    // DAV resource cache the way the DAV handlers do.
    test.server.notify_task_queue();
    test.server
        .invalidate_local_caches(&[CacheInvalidation::DavResources(account_id)])
        .await;
    document_id
}

/// Emails in the account's mailboxes.
pub async fn mail_count(test: &TestServer, account_id: u32) -> usize {
    test.server
        .get_cached_messages(account_id)
        .await
        .unwrap()
        .emails
        .items
        .len()
}

/// Recipients of every message still in the SMTP queue.
pub async fn queued_recipients(test: &TestServer) -> Vec<String> {
    test.read_queued_messages()
        .await
        .iter()
        .flat_map(|m| m.message.recipients.iter().map(|r| r.address.to_string()))
        .collect()
}

/// Waits until everything the server has queued is delivered. A task that
/// sends mail (iMIP, alarms) waits for the local SMTP session to accept the
/// message, so once the task queue is empty every such message is in the
/// SMTP queue; local delivery ingests it and then removes it. The task-queue
/// waits have no deadline and include tasks not yet due (an alarm scheduled
/// for later holds them until it fires); only the SMTP-queue poll between
/// them panics, after ten seconds, naming what is still queued.
pub async fn wait_for_delivery(test: &TestServer) {
    test.wait_for_tasks().await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let queued = queued_recipients(test).await;
        if queued.is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "mail still queued for {queued:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Ingest can queue index tasks of its own.
    test.wait_for_tasks().await;
}

/// The account's scheduling inbox items, by ascending document id, as
/// stored (pending items included; clients never see those).
pub async fn inbox_items(
    test: &TestServer,
    account_id: u32,
) -> Vec<(u32, CalendarEventNotification)> {
    let mut items = Vec::new();
    for document_id in test.server.itip_ids(account_id).await.unwrap() {
        if let Some(stored) = test
            .server
            .store()
            .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                account_id,
                Collection::CalendarEventNotification,
                document_id,
            ))
            .await
            .unwrap()
        {
            items.push((
                document_id,
                stored.deserialize::<CalendarEventNotification>().unwrap(),
            ));
        }
    }
    items
}

/// The document ids of the account's scheduling inbox items.
pub async fn inbox_ids(test: &TestServer, account_id: u32) -> Vec<u32> {
    test.server
        .itip_ids(account_id)
        .await
        .unwrap()
        .into_iter()
        .collect()
}

/// The items written since `before` was taken with `inbox_ids`.
pub async fn new_inbox_items(
    test: &TestServer,
    account_id: u32,
    before: &[u32],
) -> Vec<(u32, CalendarEventNotification)> {
    inbox_items(test, account_id)
        .await
        .into_iter()
        .filter(|(id, _)| !before.contains(id))
        .collect()
}

/// The wrap type of a stored tree's `X-ZA-KEY` envelope (`1` mk, `2` pk), or
/// `None` when the root does not end with one.
pub fn envelope_wrap(ical: &ICalendar) -> Option<u8> {
    let last = ical.components.first()?.entries.last()?;
    if !matches!(&last.name, ICalendarProperty::Other(name) if name.eq_ignore_ascii_case(KEY_PROP))
    {
        return None;
    }
    STANDARD
        .decode(last.values.first()?.as_text()?.trim())
        .ok()?
        .get(1)
        .copied()
}

/// Opens a pending item as the drain will (invitations spec 6.4 step 1):
/// unlocks the recipient's keys from `password`, obtains its private key
/// through `Server::za_private_key`, and opens the stored item's `pk` wrap.
/// Returns the plaintext tree and what the item carries. In plan 9 only
/// tests open pending items (invariant 11).
pub async fn open_pending_item_as(
    test: &TestServer,
    recipient: u32,
    password: &str,
    document_id: u32,
) -> (ICalendar, PendingCarriers) {
    let keys = za_unlock(test, recipient, password).await;
    let private_key = test.server.za_private_key(&keys).await.unwrap();
    let stored = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            recipient,
            Collection::CalendarEventNotification,
            document_id,
        ))
        .await
        .unwrap()
        .expect("inbox item");
    let mut item = stored.deserialize::<CalendarEventNotification>().unwrap();
    let carriers = open_pending_item(&mut item.event, recipient, private_key)
        .unwrap_or_else(|err| panic!("item {document_id} does not open: {err:?}"));
    (item.event, carriers)
}

/// The METHOD on an iTIP tree's root (visible on a sealed item too).
pub fn item_method(ical: &ICalendar) -> Option<ICalendarMethod> {
    itip_method(ical).ok().cloned()
}

/// The first SEQUENCE found across the VEVENTs, in component order.
pub fn item_sequence(ical: &ICalendar) -> Option<i64> {
    ical.components
        .iter()
        .filter(|comp| comp.component_type == ICalendarComponentType::VEvent)
        .flat_map(|comp| comp.entries.iter())
        .find(|entry| entry.name == ICalendarProperty::Sequence)
        .and_then(|entry| entry.values.first())
        .and_then(|value| match value {
            ICalendarValue::Integer(sequence) => Some(*sequence),
            _ => None,
        })
}

fn names_address(entry: &ICalendarEntry, address: &str) -> bool {
    entry
        .values
        .first()
        .and_then(|value| value.as_text())
        .is_some_and(|value| {
            value
                .trim_start_matches("mailto:")
                .eq_ignore_ascii_case(address)
        })
}

/// Every ATTENDEE address, lower case, sorted, deduplicated.
pub fn item_attendees(ical: &ICalendar) -> Vec<String> {
    let mut attendees: Vec<String> = ical
        .components
        .iter()
        .flat_map(|comp| comp.entries.iter())
        .filter(|entry| entry.name == ICalendarProperty::Attendee)
        .filter_map(|entry| entry.values.first().and_then(|value| value.as_text()))
        .map(|value| value.trim_start_matches("mailto:").to_ascii_lowercase())
        .collect();
    attendees.sort();
    attendees.dedup();
    attendees
}

/// The PARTSTAT of `address`'s ATTENDEE property, upper case.
pub fn item_partstat(ical: &ICalendar, address: &str) -> Option<String> {
    ical.components
        .iter()
        .flat_map(|comp| comp.entries.iter())
        .filter(|entry| entry.name == ICalendarProperty::Attendee && names_address(entry, address))
        .flat_map(|entry| entry.params.iter())
        .find_map(|param| match (&param.name, &param.value) {
            (ICalendarParameterName::Partstat, ICalendarParameterValue::Partstat(status)) => {
                Some(status.as_str().to_string())
            }
            _ => None,
        })
}

/// The SCHEDULE-STATUS values on every `property` (ORGANIZER or ATTENDEE)
/// naming `address`, in an iCalendar body as GET returns it. Parsed, so
/// folded lines do not matter.
pub fn schedule_statuses(body: &str, property: ICalendarProperty, address: &str) -> Vec<String> {
    parse_ical(body)
        .components
        .iter()
        .flat_map(|comp| comp.entries.iter())
        .filter(|entry| entry.name == property && names_address(entry, address))
        .flat_map(|entry| entry.params.iter())
        .filter(|param| param.name == ICalendarParameterName::ScheduleStatus)
        .filter_map(|param| param.value.as_text().map(str::to_string))
        .collect()
}

/// A request held at one of the fork's test-mode pause points in
/// `common::za::test_hooks` (`set_fan_out_pause`, `set_put_pause`). As
/// `Parked`, dropping it clears the slot and releases the request, also on a
/// failed assertion.
pub struct HookParked<T> {
    pause: Arc<HookPause>,
    clear: fn(Option<Arc<HookPause>>),
    handle: Option<JoinHandle<T>>,
}

impl<T: Send + 'static> HookParked<T> {
    pub async fn start(
        account_id: u32,
        set: fn(Option<Arc<HookPause>>),
        request: impl Future<Output = T> + Send + 'static,
    ) -> Self {
        let pause = Arc::new(HookPause::new(account_id));
        set(Some(pause.clone()));
        let mut parked = HookParked {
            pause: pause.clone(),
            clear: set,
            handle: None,
        };
        parked.handle = Some(tokio::spawn(request));
        tokio::time::timeout(Duration::from_secs(5), pause.arrived.notified())
            .await
            .expect("the request did not reach the pause point");
        parked
    }

    pub async fn finish(mut self) -> T {
        self.pause.release.notify_one();
        self.handle.take().unwrap().await.unwrap()
    }
}

impl<T> Drop for HookParked<T> {
    fn drop(&mut self) {
        (self.clear)(None);
        self.pause.release.notify_one();
    }
}

/// The task manager's consumer held at `TASK_CONSUMER_PAUSE`; dropping it
/// releases the consumer.
pub struct ConsumerHeld {
    pause: Arc<HookPause>,
}

impl Drop for ConsumerHeld {
    fn drop(&mut self) {
        test_hooks::set_task_consumer_pause(None);
        self.pause.release.notify_one();
    }
}

/// Holds the task manager's consumer at its pause point, so a task-queue
/// read cannot miss a task the worker would otherwise run and clear
/// (invitations spec 11). Waits for due tasks first (at most 30 s), so
/// nothing earlier is mid-flight, then wakes the consumer and waits until it
/// is parked.
pub async fn hold_task_consumer(test: &TestServer) -> ConsumerHeld {
    // Bounded: a permanently failed task left by an earlier module would
    // otherwise hang the suite here without a name.
    tokio::time::timeout(Duration::from_secs(30), test.wait_for_tasks_skip_not_due())
        .await
        .expect("tasks did not drain before the consumer hold");
    let pause = Arc::new(HookPause::new(0));
    test_hooks::set_task_consumer_pause(Some(pause.clone()));
    test.server.notify_task_queue();
    tokio::time::timeout(Duration::from_secs(5), pause.arrived.notified())
        .await
        .expect("the task consumer did not reach its pause point");
    ConsumerHeld { pause }
}

/// `CalendarItipMessage` tasks of `account_id` in the task queue.
pub async fn itip_tasks_for(test: &TestServer, account_id: u32) -> usize {
    let mut count = 0;
    test.server
        .store()
        .iterate(
            IterateParams::new(
                ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task { id: 0 })),
                ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task { id: u64::MAX })),
            )
            .ascending(),
            |_, value| {
                if let Task::CalendarItipMessage(task) = Task::deserialize(value)?
                    && task.account_id.document_id() == account_id
                {
                    count += 1;
                }
                Ok(true)
            },
        )
        .await
        .unwrap();
    count
}

/// UID creation lease keys in the data store (invitations spec 6.3).
pub async fn za_uid_leases(test: &TestServer) -> usize {
    let (from, to) = lease_key_range();
    let mut count = 0;
    test.server
        .store()
        .iterate(IterateParams::new(from, to).no_values(), |_, _| {
            count += 1;
            Ok(true)
        })
        .await
        .unwrap();
    count
}

/// How `plant_inbox_item` stores an item (invitations spec 9).
pub enum PlantedItem<'a> {
    /// Sealed to the account's public key, read from its vault record (the
    /// fan-out's form, spec 5.3), pending bit set. `pair` is sealed into the
    /// root's `X-ZA-EXTRA` (a REQUEST or CANCEL); `reply_token` becomes the
    /// visible `X-ZA-RSVP-TOKEN` (a REPLY).
    Pending {
        pair: Option<(&'a str, [u8; 16])>,
        reply_token: Option<[u8; 16]>,
    },
    /// Sealed under the account's keys, unlocked from `password` as a login
    /// does (the drain's form, spec 6.4 step 5), pending bit clear.
    Drained { password: &'a str },
    /// Stored as given with `flags`: a plain account's item, or a key
    /// account's legacy plaintext one.
    Plain { flags: u16 },
}

fn parse_ical(ical: &str) -> ICalendar {
    match calcard::Parser::new(ical).entry() {
        calcard::Entry::ICalendar(ical) => ical,
        other => panic!("{other:?}"),
    }
}

/// Writes `ical` into the account's scheduling inbox as a
/// `CalendarEventNotification` from `sender`, bypassing every handler: the
/// fan-out (plan 9) and the drain (plan 10) that will write such items do not
/// exist yet. `created` backdates the item (inbox expiry; upstream's
/// `insert` would overwrite it). Returns the document id.
pub async fn plant_inbox_item(
    test: &TestServer,
    account_id: u32,
    ical: &str,
    item: PlantedItem<'_>,
    sender: u32,
    created: Option<i64>,
) -> u32 {
    let mut tree = parse_ical(ical);
    let flags = match item {
        PlantedItem::Pending { pair, reply_token } => {
            let record = test
                .server
                .za_vault_record(account_id)
                .await
                .unwrap()
                .expect("vault record")
                .record;
            let public: [u8; 32] = record.public_key.as_slice().try_into().expect("public key");
            let pairs = pair.map(|(address, token)| {
                let mut pairs = TokenPairs::new();
                pairs.insert(address, token);
                pairs
            });
            seal_pending_item(
                &mut tree,
                account_id,
                &public,
                pairs.as_ref(),
                reply_token.as_ref(),
            )
            .unwrap();
            EVENT_NOTIFICATION_ZA_PENDING
        }
        PlantedItem::Drained { password } => {
            let keys = za_unlock(test, account_id, password).await;
            seal_drained_item(&mut tree, &keys, account_id).unwrap();
            0
        }
        PlantedItem::Plain { flags } => flags,
    };
    let now = store::write::now() as i64;
    let item = CalendarEventNotification {
        event: tree,
        event_id: None,
        changed_by: ChangedBy::PrincipalId(sender),
        flags,
        size: ical.len() as u32,
        created: created.unwrap_or(now),
        modified: now,
    };
    let account_info = test.server.account_info(account_id).await.unwrap();
    let document_id = test
        .server
        .store()
        .assign_document_ids(account_id, Collection::CalendarEventNotification, 1)
        .await
        .unwrap();
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::CalendarEventNotification)
        .with_document(document_id)
        .custom(
            ObjectIndexBuilder::<(), _>::new()
                .with_changes(item)
                .with_changed_by(account_info.account_tenant_ids()),
        )
        .unwrap()
        .commit_point();
    test.server.commit_batch(batch).await.unwrap();
    test.server
        .invalidate_local_caches(&[CacheInvalidation::DavResources(account_id)])
        .await;
    document_id
}

/// Rewrites a planted item in place, same document id, as the drain's commit
/// will (spec 6.4 step 5): `edit` changes the stored item, and the write
/// asserts the archive it read.
pub async fn rewrite_inbox_item(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
    edit: impl FnOnce(&mut CalendarEventNotification),
) {
    let stored = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEventNotification,
            document_id,
        ))
        .await
        .unwrap()
        .expect("inbox item");
    let mut item = stored.deserialize::<CalendarEventNotification>().unwrap();
    edit(&mut item);
    item.modified = store::write::now() as i64;
    let account_info = test.server.account_info(account_id).await.unwrap();
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::CalendarEventNotification)
        .with_document(document_id)
        .custom(
            ObjectIndexBuilder::new()
                .with_current(stored.to_unarchived::<CalendarEventNotification>().unwrap())
                .with_changes(item)
                .with_changed_by(account_info.account_tenant_ids()),
        )
        .unwrap()
        .commit_point();
    test.server.commit_batch(batch).await.unwrap();
    test.server
        .invalidate_local_caches(&[CacheInvalidation::DavResources(account_id)])
        .await;
}
