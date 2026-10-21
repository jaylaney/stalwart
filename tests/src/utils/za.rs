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
    auth::vault::{RawValue, VaultRead, ZaMark, ZaVerification},
    ipc::CacheInvalidation,
    storage::index::ObjectIndexBuilder,
    za::{
        lease::lease_key_range,
        test_hooks::{self, Pause as HookPause},
    },
};
use email::{
    cache::MessageCacheFetch,
    mailbox::{INBOX_ID, UidMailbox},
    message::{
        ingest::EmailIngest,
        metadata::{MessageData, MessageMetadata},
    },
};
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
use mail_parser::MessageParser;
use registry::schema::{
    enums::IndexDocumentType,
    structs::{Task, TaskIndexDocument, TaskStatus},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{future::Future, sync::Arc, time::Duration};
use store::{
    Deserialize, IterateParams, Serialize as _, ValueKey,
    write::{
        AlignedBytes, Archive, Archiver, BatchBuilder, BlobLink, BlobOp, TaskQueueClass,
        ValueClass, now,
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    task::JoinHandle,
};
use tokio_rustls::{
    TlsConnector,
    client::TlsStream,
    rustls::{
        self, DigitallySignedStruct, SignatureScheme,
        client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        pki_types::{CertificateDer, ServerName, UnixTime},
    },
};
use types::collection::{Collection, SyncCollection};
use types::field::{CalendarEventField, EmailField, PrincipalField};
use vault::{
    keys::{
        AAD_PRIVATE_KEY, AAD_PRIVATE_KEY_PINNED, Secret, aad, generate_keypair, unwrap_key,
        unwrap_private_key, wrap_key,
    },
    pins::{ZaPinRecord, open_pin_record, seal_pin_record},
    record::VaultRecord,
    session::SessionKeys,
};

/// The suite's HTTPS listener.
pub const SERVER_URL: &str = "https://127.0.0.1:8899";

/// `ZA_KEY_ACCOUNTS=1`: every test user of the upstream suites that honour
/// it (`webdav_tests`, `imap_tests`, `jmap_tests`) is a zero-access key
/// account, provisioned through the account API (setup token, then setup).
pub fn key_accounts_mode() -> bool {
    std::env::var("ZA_KEY_ACCOUNTS").is_ok_and(|v| v == "1")
}

/// Accepts the test server's self-signed certificate.
#[derive(Debug)]
pub(crate) struct AcceptAnyCert(pub(crate) Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

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

/// Mail spec 6.6: a plaintext message in INBOX, as an account that held
/// mail before it became a key account would hold one. `email_ingest`
/// seals every message of a key account, so the message is written here as
/// its ordinary path writes one (`index_message`: the blob linked,
/// `MessageData`, the metadata archive) with its thread container and index
/// task, as `plant_event` writes an unsealed event. Returns the document id.
pub async fn plant_plain_message(test: &TestServer, account_id: u32, raw: &str) -> u32 {
    let server = &test.server;
    let (blob_hash, blob_hold) = server
        .put_temporary_blob(account_id, raw.as_bytes(), 60)
        .await
        .unwrap();
    let mut ids = server
        .assign_email_ids(account_id, [INBOX_ID], true)
        .await
        .unwrap();
    let document_id = ids.next().unwrap();
    let uid = ids.next().unwrap();
    let metadata = MessageMetadata::build(
        MessageParser::new().parse(raw.as_bytes()).unwrap(),
        Vec::new(),
        Vec::new(),
        blob_hash.clone(),
        now(),
    );
    let data = MessageData {
        mailboxes: vec![UidMailbox::new(INBOX_ID, uid)].into_boxed_slice(),
        keywords: Box::default(),
        thread_id: document_id,
        size: raw.len() as u32,
    };
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Thread)
        .with_document(document_id)
        .log_container_insert(SyncCollection::Thread)
        .with_collection(Collection::Email)
        .with_document(document_id)
        .set(
            BlobOp::Link {
                hash: blob_hash,
                to: BlobLink::Document,
            },
            Vec::new(),
        )
        .custom(ObjectIndexBuilder::<(), _>::new().with_changes(data))
        .unwrap()
        .set(
            EmailField::Metadata,
            Archiver::new(metadata).serialize().unwrap(),
        )
        .schedule_task(Task::IndexDocument(TaskIndexDocument {
            account_id: account_id.into(),
            document_id: document_id.into(),
            document_type: IndexDocumentType::Email,
            status: TaskStatus::now(),
        }))
        .clear(blob_hold);
    server.commit_batch(batch).await.unwrap();
    server.notify_task_queue();
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
/// unlocks the recipient's keys from `password`, opens the private-key wrap
/// of its vault record read-only (`unwrap_private_key`: no outcome recorded
/// and no write, so a test read never repairs or heals, key pinning spec
/// 3.3), and opens the stored item's `pk` wrap. Returns the plaintext tree
/// and what the item carries. In plan 9 only tests open pending items
/// (invariant 11).
pub async fn open_pending_item_as(
    test: &TestServer,
    recipient: u32,
    password: &str,
    document_id: u32,
) -> (ICalendar, PendingCarriers) {
    let keys = za_unlock(test, recipient, password).await;
    let wrap = saved_wrap(test, recipient).await;
    let (private_key, _) = unwrap_private_key(&wrap, keys.mk(), recipient)
        .unwrap_or_else(|_| panic!("the private-key wrap of account {recipient} does not open"));
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
    let carriers = open_pending_item(&mut item.event, recipient, &private_key)
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

/// An app password of `name`: a second session of the account (invitations
/// spec 6.3, two key-cache entries), through `/api/vault/app-password`.
pub async fn za_app_password(name: &str, password: &str, description: &str) -> String {
    za_post(
        "app-password",
        &json!({ "username": name, "password": password, "description": description }),
    )
    .await
    .str(200, "app_password")
}

/// An OAuth access token of `name`, through the server's own flow: an
/// authorization code from `/api/auth`, exchanged at `/auth/token` (the
/// suite requires no client registration). The token holds no keys.
pub async fn bearer_token(name: &str, password: &str) -> String {
    const CLIENT_ID: &str = "za-bearer-test";
    const REDIRECT_URI: &str = "https://client.example.com/callback";
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let reply: Value = client
        .post(format!("{SERVER_URL}/api/auth"))
        .json(&json!({
            "type": "authCode",
            "accountName": name,
            "accountSecret": password,
            "clientId": CLIENT_ID,
            "redirectUri": REDIRECT_URI,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let code = reply["client_code"]
        .as_str()
        .unwrap_or_else(|| panic!("no authorization code: {reply}"));
    let reply: Value = client
        .post(format!("{SERVER_URL}/auth/token"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", CLIENT_ID),
            ("redirect_uri", REDIRECT_URI),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    reply["access_token"]
        .as_str()
        .unwrap_or_else(|| panic!("no access token: {reply}"))
        .to_string()
}

/// A fresh TOTP enrolment URL (a random 20-byte secret).
pub fn totp_url() -> String {
    totp_rs::Builder::new()
        .with_secret(store::rand::random::<[u8; 20]>())
        .with_account_name("key8")
        .with_issuer(Some("Stalwart"))
        .build()
        .unwrap()
        .to_url()
        .unwrap()
}

/// The current code of the TOTP enrolment `url`.
pub fn code(url: &str) -> String {
    totp_rs::Totp::from_url(url)
        .unwrap()
        .generate_current()
        .to_string()
}

/// A raw line-oriented client of a TLS listener of the suite's server
/// (IMAP, POP3, ManageSieve, SMTP submission).
pub struct Conn(BufReader<TlsStream<TcpStream>>);

impl Conn {
    pub async fn open(port: u16) -> Self {
        let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        Conn::upgrade(tcp).await
    }

    /// The TLS handshake over `tcp`: a connection to an implicit-TLS port,
    /// or one past a STARTTLS.
    pub async fn upgrade(tcp: TcpStream) -> Self {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCert(provider)))
            .with_no_client_auth();
        let tls = TlsConnector::from(Arc::new(config))
            .connect(ServerName::try_from("127.0.0.1").unwrap(), tcp)
            .await
            .unwrap();
        Conn(BufReader::new(tls))
    }

    /// One write, flushed: a read of its own on the server unless the
    /// server reads slower than the client writes.
    pub async fn send(&mut self, data: &str) {
        self.0.get_mut().write_all(data.as_bytes()).await.unwrap();
        self.0.get_mut().flush().await.unwrap();
    }

    /// A submission session on `port` past its greeting. The default
    /// inbound throttle admits 5 connections a second from one address,
    /// LMTP deliveries included, and drops the rest before the greeting: a
    /// dropped connection is retried after the throttle's window.
    pub async fn open_smtp(port: u16) -> Self {
        for _ in 0..5 {
            let mut conn = Conn::open(port).await;
            let mut line = String::new();
            if let Ok(Ok(read)) =
                tokio::time::timeout(Duration::from_secs(10), conn.0.read_line(&mut line)).await
                && read > 0
            {
                assert!(line.starts_with("220"), "SMTP greeting {line:?}");
                return conn;
            }
            tokio::time::sleep(Duration::from_millis(1100)).await;
        }
        panic!("the submission listener dropped every connection");
    }

    /// Reads lines until one starts with any of `prefixes`, and returns it.
    pub async fn until(&mut self, prefixes: &[&str]) -> String {
        loop {
            let mut line = String::new();
            let read = tokio::time::timeout(Duration::from_secs(10), self.0.read_line(&mut line))
                .await
                .unwrap_or_else(|_| panic!("timed out waiting for {prefixes:?}"))
                .unwrap_or_else(|err| panic!("read failed waiting for {prefixes:?}: {err}"));
            assert!(read > 0, "connection closed waiting for {prefixes:?}");
            if prefixes.iter().any(|prefix| line.starts_with(prefix)) {
                return line;
            }
        }
    }

    /// Sends `data` and expects a line starting with `expected`.
    pub async fn exchange(&mut self, data: &str, expected: &str) {
        self.send(data).await;
        let line = self
            .until(&[expected, "-ERR", "NO", "BAD", "a1 ", "5", "4"])
            .await;
        assert!(line.starts_with(expected), "{data:?} answered {line:?}");
    }
}

/// Any DAV request of a key account drains its inbox first (invitations
/// spec 6.2): a PROPFIND of the account's own principal, through the raw
/// client (5 s), so a drain that waits on a lease or a pause is not cut off
/// by `DummyWebDavClient`'s 500 ms timeout. Resolves to the status.
pub fn trigger_drain(
    name: &str,
    secret: &str,
) -> impl Future<Output = u16> + Send + 'static + use<> {
    let request = HttpRequest::with_credentials(8899, name, secret);
    let path = format!("/dav/pal/{}/", name.replace('@', "%40"));
    async move {
        request
            .send_full(Method::from_bytes(b"PROPFIND").unwrap(), &path, None, None)
            .await
            .status
            .as_u16()
    }
}

/// The document ids of the account's inbox items whose pending bit is set.
pub async fn pending_ids(test: &TestServer, account_id: u32) -> Vec<u32> {
    inbox_items(test, account_id)
        .await
        .into_iter()
        .filter(|(_, item)| item.flags & EVENT_NOTIFICATION_ZA_PENDING != 0)
        .map(|(id, _)| id)
        .collect()
}

/// The href of the account's one calendar event with `uid`: the drain names
/// an imported copy `<time>_<random>.ics`, so tests find it by UID.
pub async fn copy_href(test: &TestServer, account_id: u32, uid: &str) -> String {
    let ids = test
        .server
        .document_ids_matching(
            account_id,
            Collection::CalendarEvent,
            CalendarEventField::Uid,
            uid.as_bytes(),
        )
        .await
        .unwrap();
    assert_eq!(ids.len(), 1, "{uid}: {} copies", ids.len());
    let resources = test
        .server
        .fetch_dav_resources(account_id, account_id, SyncCollection::Calendar)
        .await
        .unwrap();
    // Calendars and events number their documents separately, so a
    // calendar can share the event's id: `format_resource` ends a
    // container's href with '/', an event's never.
    resources
        .format_resource_paths_by_id(ids.iter().next().unwrap())
        .find(|href| !href.ends_with('/'))
        .expect("the copy is in the resource cache")
}

// The store writer of key pinning spec 2: these helpers write the vault and
// pin records directly, as a party with store access but without MK can (and,
// where a password is given, as the owner would), so tests can put an account
// into the state a case names. Product code never writes this way.

/// A fresh X25519 public key, the kind a store writer substitutes.
pub fn writer_key() -> [u8; 32] {
    generate_keypair().0
}

/// The account's vault record as read now; it must exist.
async fn vault_read(test: &TestServer, account_id: u32) -> VaultRead {
    test.server
        .za_vault_record(account_id)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("no vault record for account {account_id}"))
}

/// `edit` applied to the vault record at the next revision, written
/// conditionally on the read (`za_vault_write`, the raw write), then the
/// account's caches invalidated when `invalidate`.
async fn rewrite_vault(
    test: &TestServer,
    account_id: u32,
    invalidate: bool,
    edit: impl FnOnce(&mut VaultRecord),
) {
    let read = vault_read(test, account_id).await;
    let mut record = read.record.clone();
    edit(&mut record);
    record.revision = read.record.revision + 1;
    test.server
        .za_vault_write(account_id, &record, Some(&read))
        .await
        .unwrap();
    if invalidate {
        test.server.za_invalidate_account(account_id).await.unwrap();
    }
}

/// Writes `key` as the account's `public_key` at the next revision, with
/// cache invalidation when `invalidate`, and returns the replaced bytes.
/// Restoring is the same call with the original.
pub async fn substitute_public_key(
    test: &TestServer,
    account_id: u32,
    key: &[u8],
    invalidate: bool,
) -> Vec<u8> {
    let mut replaced = Vec::new();
    rewrite_vault(test, account_id, invalidate, |record| {
        replaced = std::mem::replace(&mut record.public_key, key.to_vec());
    })
    .await;
    replaced
}

/// Opens a private-key wrap with `unwrap_key` under each purpose, pinned
/// first, independently of the product's `unwrap_private_key`, so a mutation
/// there cannot blind the tests.
fn open_wrap(wrap: &[u8], mk: &Secret, account_id: u32) -> Option<(Secret, ZaMark)> {
    [
        (AAD_PRIVATE_KEY_PINNED, ZaMark::Pinned),
        (AAD_PRIVATE_KEY, ZaMark::Legacy),
    ]
    .into_iter()
    .find_map(|(purpose, mark)| {
        unwrap_key(wrap, mk, &aad(purpose, account_id))
            .ok()
            .map(|key| (key, mark))
    })
}

/// The mark of the account's stored private-key wrap (key pinning spec 4.4),
/// under the MK `password` unlocks.
pub async fn wrap_mark(test: &TestServer, account_id: u32, password: &str) -> ZaMark {
    let keys = za_unlock(test, account_id, password).await;
    let wrap = saved_wrap(test, account_id).await;
    open_wrap(&wrap, keys.mk(), account_id).map_or(ZaMark::Neither, |(_, mark)| mark)
}

/// The account's stored private-key wrap, for `restore_wrap`.
pub async fn saved_wrap(test: &TestServer, account_id: u32) -> Vec<u8> {
    vault_read(test, account_id).await.record.private_key_wrap
}

/// Writes `wrap` back as the account's private-key wrap at the next revision
/// and invalidates its caches.
pub async fn restore_wrap(test: &TestServer, account_id: u32, wrap: &[u8]) {
    rewrite_vault(test, account_id, true, |record| {
        record.private_key_wrap = wrap.to_vec();
    })
    .await;
}

/// Rewraps the account's private key under the legacy associated data, as
/// release 2 wrapped it, at the next revision, and invalidates its caches.
pub async fn rewrap_legacy(test: &TestServer, account_id: u32, password: &str) {
    let keys = za_unlock(test, account_id, password).await;
    let wrap = saved_wrap(test, account_id).await;
    let (private_key, _) = open_wrap(&wrap, keys.mk(), account_id)
        .unwrap_or_else(|| panic!("the private-key wrap of account {account_id} does not open"));
    let legacy = wrap_key(&private_key, keys.mk(), &aad(AAD_PRIVATE_KEY, account_id));
    restore_wrap(test, account_id, &legacy).await;
}

fn pins_key(account_id: u32) -> ValueKey<ValueClass> {
    ValueKey::property(
        account_id,
        Collection::Principal,
        0,
        PrincipalField::ZeroAccessPins,
    )
}

/// The account's pin record, opened under the MK `password` unlocks; `None`
/// when absent or unusable.
pub async fn pin_record(test: &TestServer, account_id: u32, password: &str) -> Option<ZaPinRecord> {
    let keys = za_unlock(test, account_id, password).await;
    let raw = raw_pins(test, account_id).await?;
    open_pin_record(&raw, keys.mk(), account_id)
}

/// Seals `record` as given (its revision included) under the MK `password`
/// unlocks and writes it unconditionally.
pub async fn write_pin_record(
    test: &TestServer,
    account_id: u32,
    password: &str,
    record: &ZaPinRecord,
) {
    let keys = za_unlock(test, account_id, password).await;
    let sealed = seal_pin_record(record, keys.mk(), account_id);
    set_raw_pins(test, account_id, Some(&sealed)).await;
}

/// The stored bytes of the account's pin record (field 151).
pub async fn raw_pins(test: &TestServer, account_id: u32) -> Option<Vec<u8>> {
    test.server
        .store()
        .get_value::<RawValue>(pins_key(account_id))
        .await
        .unwrap()
        .map(|raw| raw.0)
}

/// An unconditional set of field 151 to `raw`, or its removal with `None`.
pub async fn set_raw_pins(test: &TestServer, account_id: u32, raw: Option<&[u8]>) {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0);
    match raw {
        Some(raw) => batch.set(PrincipalField::ZeroAccessPins, raw.to_vec()),
        None => batch.clear(PrincipalField::ZeroAccessPins),
    };
    test.server.store().write(batch.build_all()).await.unwrap();
}

/// The account's vault revision, its authentication generation.
pub async fn vault_revision(test: &TestServer, account_id: u32) -> u64 {
    vault_read(test, account_id).await.record.revision
}

/// The never-pinned state: a legacy wrap and no pin record.
pub async fn za_unpin(test: &TestServer, account_id: u32, password: &str) {
    rewrap_legacy(test, account_id, password).await;
    set_raw_pins(test, account_id, None).await;
}
