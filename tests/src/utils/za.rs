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
    storage::{
        index::{GroupwareWrite, ObjectIndexBuilder},
        search::SearchIndexStatus,
    },
    za::{
        lease::lease_key_range,
        test_hooks::{self, Pause as HookPause},
    },
};
use email::{
    cache::MessageCacheFetch,
    mailbox::INBOX_ID,
    message::{
        ingest::{ThreadInfo, thread_hash},
        messagedata::{MessageData, PendingMessageData},
        metadata::{ExtraHeaders, MessageMetadata},
        sortkeys::MessageSortKeys,
        thread::ThreadFields,
    },
};
use groupware::{
    cache::GroupwareCache,
    calendar::{
        CalendarEvent, CalendarEventContent, CalendarEventData, CalendarEventNotification,
        CalendarEventNotificationContent, ChangedBy, EVENT_NOTIFICATION_ZA_PENDING,
        alerts::DefaultAlertsResolver,
        schedule::{EventAlarmScheduler, EventAlarmUsers},
        seal::{
            PendingCarriers, TokenPairs, ZaEvent,
            item::{into_item_message, item_message},
            open_pending_item, seal_drained_item, seal_event, seal_pending_item,
            tree::KEY_PROP,
        },
    },
    scheduling::inbound::itip_method,
};
use http::auth::authenticate::za_test::{self, Pause};
use hyper::{Method, StatusCode};
use mail_parser::MessageParser;
use registry::schema::structs::Task;
use serde::Serialize;
use serde_json::{Value, json};
use std::{future::Future, sync::Arc, time::Duration};
use store::{
    Deserialize, IterateParams, Subspace, U32_LEN, ValueKey,
    write::{
        AnyKey, Archive, ArchiveBytes, ArchiveVersion, BatchBuilder, BlobLink, BlobOp,
        IndexPropertyClass, QueueDocumentId, SearchIndex, SearchIndexClass, TaskId, TaskQueueClass,
        ValueClass, blob::BlobLinkValue, key::DeserializeBigEndian, now,
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
use types::field::{CalendarEventField, CalendarNotificationField, EmailField, PrincipalField};
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
    // 1.0's `HttpAuthEntries` has no iterator; `retain` visits every entry.
    let cached = std::sync::atomic::AtomicBool::new(false);
    test.server.inner.cache.http_auth.retain(|entry| {
        if entry.account_id == account_id {
            cached.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        true
    });
    assert!(
        !cached.into_inner(),
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
        .fetch_groupware_resources(account_id, account_id, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path(calendar)
        .unwrap_or_else(|| panic!("calendar {calendar} not found"))
        .document_id();
    let ical = match calcard::Parser::new(ical).entry() {
        calcard::Entry::ICalendar(ical) => ical,
        other => panic!("{other:?}"),
    };
    // At 1.0 an event is a metadata row and a content record; upstream's
    // builder derives the row's size, range and privacy flags from the
    // content it is given (`refresh_from_content`).
    let meta = CalendarEvent {
        names: vec![common::DavName {
            name: name.into(),
            parent_id: calendar_id,
        }],
        schedule_tag,
        ..Default::default()
    };
    let content = CalendarEventContent {
        data: CalendarEventData::new(ical, Tz::Floating, 100),
        ..Default::default()
    };
    // The next email alarm, computed on the plaintext as a PUT does.
    let next_alarms = test
        .server
        .next_event_alarms(
            account_id,
            &EventAlarmUsers::new(account_id, &content)
                .unwrap()
                .with_event_flags(meta.flags),
            &content.data,
            &[calendar_id],
            store::write::now() as i64,
            &mut DefaultAlertsResolver::default(),
        )
        .await
        .unwrap();
    let mut event = ZaEvent::new(meta, content);
    if let Some((keys, pairs)) = seal {
        seal_event(&mut event, keys, account_id, Some(pairs)).unwrap();
    }
    let (meta, content) = event.into_parts();
    let changed_by = test
        .server
        .account_info(account_id)
        .await
        .unwrap()
        .changed_by();
    let mut batch = BatchBuilder::new();
    let slot = batch.reserve_document_id(account_id, Collection::CalendarEvent);
    meta.insert(
        content,
        changed_by,
        account_id,
        slot,
        None,
        next_alarms,
        &mut batch,
    )
    .unwrap();
    let document_id = test.server.commit_batch(batch).await.unwrap().slot(slot);
    // A direct store write neither wakes the task manager nor refreshes the
    // DAV resource cache the way the DAV handlers do.
    test.server.notify_task_queue();
    test.server
        .invalidate_local_caches(&[CacheInvalidation::GroupwareResources(account_id)]);
    document_id
}

/// The stored records of the account's event `document_id` at 1.0: the
/// metadata row (`Field::ARCHIVE`) and the content archive
/// (`CalendarEventField::Content`).
pub async fn event_records(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
) -> (Archive<ArchiveBytes>, Archive<ArchiveBytes>) {
    let meta = test
        .server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEvent,
            document_id,
        ))
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("no event row {account_id}/{document_id}"));
    let content = test
        .server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
            account_id,
            Collection::CalendarEvent,
            document_id,
            CalendarEventField::Content,
        ))
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("no event content {account_id}/{document_id}"));
    (meta, content)
}

/// An event's two stored records, read together, as the 0.16 tests read
/// the one event archive.
#[derive(Clone)]
pub struct StoredEvent {
    pub meta: Archive<ArchiveBytes>,
    pub content: Archive<ArchiveBytes>,
    /// Both records' versions: 0.16's one archive version. Any write
    /// changes the row's; a full write changes both.
    pub version: (ArchiveVersion, ArchiveVersion),
}

impl StoredEvent {
    /// The deserialized pair, as the seal codec takes it.
    pub fn za_event(&self) -> ZaEvent {
        ZaEvent::new(
            self.meta.deserialize::<CalendarEvent>().unwrap(),
            self.content.deserialize::<CalendarEventContent>().unwrap(),
        )
    }

    /// The stored tree (sealed for a key account).
    pub fn tree(&self) -> &calcard::icalendar::ArchivedICalendar {
        &self
            .content
            .unarchive::<CalendarEventContent>()
            .unwrap()
            .data
            .event
    }

    /// `seal::event_pairs_in_domain` on the stored records.
    pub fn pairs_in(
        &self,
        domain: &groupware::calendar::seal::ZaKeyDomain,
    ) -> Result<Option<TokenPairs>, groupware::calendar::seal::SealError> {
        groupware::calendar::seal::event_pairs_in_domain(
            &self.meta.deserialize::<CalendarEvent>().unwrap(),
            &self.content,
            domain,
        )
    }

    /// The event's ETag as upstream serves it at 1.0: the row's `etag`
    /// (content hash xor row hash), quoted.
    pub fn etag(&self) -> String {
        format!(
            "\"{}\"",
            self.meta
                .unarchive::<CalendarEvent>()
                .unwrap()
                .etag
                .to_native()
        )
    }

    /// The time of the next email alarm upstream's scheduler derives from
    /// the stored content (alarm data is visible, so a sealed event's
    /// stored alarms are the plaintext's), as a write schedules it.
    pub async fn next_alarm_time(
        &self,
        test: &TestServer,
        account_id: u32,
        calendar_id: u32,
    ) -> Option<i64> {
        let meta = self.meta.deserialize::<CalendarEvent>().unwrap();
        let content = self.content.deserialize::<CalendarEventContent>().unwrap();
        test.server
            .next_event_alarms(
                account_id,
                &EventAlarmUsers::new(account_id, &content)
                    .unwrap()
                    .with_event_flags(meta.flags),
                &content.data,
                &[calendar_id],
                store::write::now() as i64,
                &mut DefaultAlertsResolver::default(),
            )
            .await
            .unwrap()
            .into_iter()
            .next()
            .map(|alarm| alarm.alarm_time)
    }

    /// Row and content bytes, concatenated: 0.16's one archive's bytes, for
    /// "byte-identical" assertions.
    pub fn as_bytes(&self) -> Vec<u8> {
        let mut bytes = self.meta.as_bytes().to_vec();
        bytes.extend_from_slice(self.content.as_bytes());
        bytes
    }

    /// Row and content bytes together (`event_bytes`).
    pub fn bytes(&self) -> String {
        event_bytes(&self.meta, &self.content)
    }
}

/// `event_records` as a `StoredEvent`.
pub async fn stored_event(test: &TestServer, account_id: u32, document_id: u32) -> StoredEvent {
    let (meta, content) = event_records(test, account_id, document_id).await;
    StoredEvent {
        version: (meta.version, content.version),
        meta,
        content,
    }
}

/// Writes `event` over `stored` with upstream's full split update (row
/// and content, conditional on the stored row), bypassing every handler:
/// 0.16's `CalendarEvent::update` in the tests. Wakes the task manager.
pub async fn write_stored_event(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
    stored: &StoredEvent,
    event: ZaEvent,
) {
    let changed_by = test
        .server
        .account_info(account_id)
        .await
        .unwrap()
        .changed_by();
    let (meta, content) = event.into_parts();
    let mut batch = BatchBuilder::new();
    meta.update_full(
        content,
        changed_by,
        stored.meta.to_unarchived::<CalendarEvent>().unwrap(),
        stored.content.unarchive::<CalendarEventContent>().unwrap(),
        account_id,
        document_id,
        None,
        &mut batch,
    )
    .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    test.server.notify_task_queue();
    test.server
        .invalidate_local_caches(&[CacheInvalidation::GroupwareResources(account_id)]);
}

/// The raw stored bytes of a document's shared metadata container
/// (`MetadataClass::Shared`), decompressed: the container that holds a
/// calendar's or event's dead properties at 1.0. `None` when absent.
pub async fn shared_container(
    test: &TestServer,
    account_id: u32,
    collection: Collection,
    document_id: u32,
) -> Option<Vec<u8>> {
    test.server
        .metadata_container(account_id, collection, document_id)
        .await
        .unwrap()
        .map(|container| container.view().as_bytes().to_vec())
}

/// A plaintext dead-property container holding `properties`, each a
/// (namespace, name, text) DAV property: what a legacy writer or an
/// ordinary account stores at 1.0.
pub fn plain_dav_container(properties: &[(&str, &str, &str)]) -> types::metadata::EncodedMetadata {
    use types::metadata::{MetadataBuilder, XmlName, XmlNode, XmlValue};
    let mut builder = MetadataBuilder::new();
    for (namespace, name, text) in properties {
        builder
            .set_dav(
                XmlName::owned(Some(namespace.to_string()), name.to_string()),
                &XmlValue {
                    lang: None,
                    attributes: Vec::new(),
                    children: vec![XmlNode::Text(text.to_string().into())],
                },
            )
            .unwrap();
    }
    builder.encode().expect("a non-empty container")
}

/// Writes `container` (or removes the stored one with `None`) as the
/// document's shared metadata container through upstream's keyless writer
/// (`ContainerChange`, `MetadataWrite`: quota, assertion, no seal), and
/// rewrites the row's presence bits as the DAV handlers do. The store
/// writer of a legacy, unsealed container, for negative controls and the
/// conversion fixtures.
pub async fn plant_container(
    test: &TestServer,
    account_id: u32,
    collection: Collection,
    document_id: u32,
    container: Option<types::metadata::EncodedMetadata>,
) {
    use common::storage::{
        index::RewritePresence,
        metadata::{ContainerChange, MetadataLog},
    };
    use groupware::calendar::Calendar;
    use types::metadata::MetadataEdit;
    let owner = test.server.account(account_id).await.unwrap();
    let previous = test
        .server
        .metadata_container(account_id, collection, document_id)
        .await
        .unwrap();
    let Some(change) = ContainerChange::new(previous.as_ref(), container, MetadataEdit::Write)
    else {
        return;
    };
    let row = test
        .server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(account_id, collection, document_id))
        .await
        .unwrap()
        .expect("the container's document");
    let mut batch = BatchBuilder::new();
    // Logged as the DAV writer logs it (`DeadTarget`): the presence rewrite
    // patches the row's change id, which the logged change assigns.
    let log = match collection {
        Collection::Calendar => MetadataLog::Container,
        _ => MetadataLog::Item { prefix: None },
    };
    let presence = change
        .into_write(&owner, collection, log)
        .build(store::write::PendingId::Assigned(document_id), &mut batch)
        .unwrap();
    match collection {
        Collection::Calendar => Calendar::rewrite_presence(
            &row.to_unarchived::<Calendar>().unwrap(),
            presence.after,
            account_id,
            document_id,
            &mut batch,
        ),
        Collection::CalendarEvent => CalendarEvent::rewrite_presence(
            &row.to_unarchived::<CalendarEvent>().unwrap(),
            presence.after,
            account_id,
            document_id,
            &mut batch,
        ),
        other => panic!("no presence rewrite for {other:?}"),
    }
    .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    test.server
        .invalidate_local_caches(&[CacheInvalidation::GroupwareResources(account_id)]);
}

/// Every stored byte of an event, row and content together, for canary
/// scans: at 1.0 either record could carry a leak (the row holds the
/// display name, the content the tree).
pub fn event_bytes(meta: &Archive<ArchiveBytes>, content: &Archive<ArchiveBytes>) -> String {
    let mut text = String::from_utf8_lossy(meta.as_bytes()).to_string();
    text.push_str(&String::from_utf8_lossy(content.as_bytes()));
    text
}

/// Upgrade part 4c (per-user event data, calendar spec table near line
/// 717): a key account's event content never carries per-user
/// preferences, whichever path wrote it.
pub async fn assert_no_event_preferences(test: &TestServer, account_id: u32, document_id: u32) {
    let (_, content) = event_records(test, account_id, document_id).await;
    let content = content.unarchive::<CalendarEventContent>().unwrap();
    assert!(
        content.preferences.is_empty(),
        "event {account_id}/{document_id} holds {} preference entries",
        content.preferences.len()
    );
}

/// Mail spec 6.6: a plaintext message in INBOX, as an account that held
/// mail before it became a key account would hold one. `email_ingest`
/// seals every message of a key account, so the message is written here as
/// 1.0's ordinary path writes one (`email_ingest_into` and `index_message`,
/// `email/src/message/{ingest,index/metadata}.rs`): the blob held and
/// linked at its length, a thread of its own, `MessageData` with the
/// attachment keyword upstream sets, the sort keys, the plaintext metadata
/// row, the threading key and the message's index-queue entry. Nothing here
/// asserts vault absence: it plants into key accounts too, as `plant_event`
/// writes an unsealed event. Returns the document id.
pub async fn plant_plain_message(test: &TestServer, account_id: u32, raw: &str) -> u32 {
    let server = &test.server;
    let message = MessageParser::new()
        .parse(raw.as_bytes())
        .expect("a planted message parses");
    let (blob_hash, blob_hold) = server
        .hold_blob(
            account_id,
            types::blob_hash::BlobHash::generate(raw.as_bytes()),
            raw.as_bytes(),
            60,
        )
        .await
        .unwrap();
    let built = MessageMetadata::build(&message, &ExtraHeaders::default(), blob_hash.clone());
    let fields = ThreadFields::scan(&message);
    let received_at = now();
    let keywords = 1u32
        << if built.has_attachments {
            types::keyword::HASATTACHMENT
        } else {
            types::keyword::HASNOATTACHMENT
        };

    let mut batch = BatchBuilder::new();
    batch.with_account_id(account_id);
    let document_slot = batch.reserve_document_id(account_id, Collection::Email);
    let uid_slots = batch.reserve_uids(account_id, [INBOX_ID]);
    batch
        .with_collection(Collection::Thread)
        .create_document(document_slot)
        .log_container_insert(SyncCollection::Thread);
    let data = PendingMessageData {
        data: MessageData {
            mailboxes: [common::MessageUid::new_unassigned(INBOX_ID)]
                .into_iter()
                .collect(),
            keywords,
            keywords_extra: Vec::new(),
            thread_id: 0,
            size: raw.len() as u32,
            received_at,
            sent_at: MessageData::sent_at_offset(fields.sent_at, received_at),
            change_id: 0,
        },
        uid_slots,
        thread_slot: Some(document_slot),
        change_id: None,
    };
    let thread_id = data.thread_id();
    batch
        .with_collection(Collection::Email)
        .create_document(document_slot)
        .set(
            BlobOp::Link {
                hash: blob_hash,
                to: BlobLink::Document,
            },
            BlobLinkValue::new(message.raw().len()),
        )
        .custom(ObjectIndexBuilder::<(), _>::new().with_changes(data))
        .unwrap()
        .set(
            EmailField::SortKeys,
            MessageSortKeys::from_message(&message).serialize(),
        )
        .set(EmailField::Metadata, built.encode().unwrap())
        .set(
            ValueClass::IndexProperty(IndexPropertyClass::Hash {
                property: EmailField::Threading.into(),
                hash: thread_hash(fields.subject),
            }),
            ThreadInfo {
                thread_id,
                ref_ids: &fields.message_ids,
            },
        )
        .queue_document_index(SearchIndex::Email, account_id, QueueDocumentId::Current);
    blob_hold.release(&mut batch);
    server
        .commit_batch(batch)
        .await
        .unwrap()
        .slot(document_slot)
}

/// Emails in the account's mailboxes.
pub async fn mail_count(test: &TestServer, account_id: u32) -> usize {
    test.server
        .get_cached_messages(account_id)
        .await
        .unwrap()
        .emails
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
    wait_for_queue(test).await;
    // Ingest can queue index tasks of its own.
    test.wait_for_tasks().await;
}

/// Waits until the SMTP queue is empty, without waiting for tasks: the
/// iMIP modules read outbound mail at the sink once its delivery is done.
/// Panics after ten seconds, naming what is still queued.
pub async fn wait_for_queue(test: &TestServer) {
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
}

/// A scheduling inbox item as tests read it at 1.0: the notification row's
/// fields and its iTIP message (`CalendarEventNotificationContent::Itip`'s
/// `message`; for upstream's own `Direct` content, its current or previous
/// snapshot), so 0.16's `item.event` and `item.flags` reads keep their
/// meaning.
#[derive(Debug, Clone)]
pub struct InboxItem {
    pub event: ICalendar,
    pub event_id: Option<u32>,
    pub changed_by: ChangedBy,
    pub calendar_ids: Vec<u32>,
    pub flags: u16,
    pub size: u32,
    pub created: i64,
    pub modified: i64,
    pub content: CalendarEventNotificationContent,
}

impl InboxItem {
    fn new(meta: CalendarEventNotification, content: CalendarEventNotificationContent) -> Self {
        let event = match &content {
            CalendarEventNotificationContent::Itip { message, .. } => message.clone(),
            CalendarEventNotificationContent::Direct { previous, current } => current
                .as_ref()
                .or(previous.as_ref())
                .cloned()
                .unwrap_or_default(),
        };
        InboxItem {
            event,
            event_id: meta.event_id,
            changed_by: meta.changed_by,
            calendar_ids: meta.calendar_ids,
            flags: meta.flags,
            size: meta.size,
            created: meta.created,
            modified: meta.modified,
            content,
        }
    }

    /// True for an item the fork wrote (`seal::item_message`): an `Itip`
    /// content with no plaintext snapshot.
    pub fn is_fork_item(&self) -> bool {
        item_message(&self.content).is_some()
    }
}

/// The stored records of an inbox item: its row and its content archive.
pub async fn inbox_records(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
) -> Option<(Archive<ArchiveBytes>, Archive<ArchiveBytes>)> {
    let meta = test
        .server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEventNotification,
            document_id,
        ))
        .await
        .unwrap()?;
    let content = test
        .server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
            account_id,
            Collection::CalendarEventNotification,
            document_id,
            CalendarNotificationField::Content,
        ))
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("item {account_id}/{document_id} has no content record"));
    Some((meta, content))
}

/// The account's scheduling inbox items, by ascending document id, as
/// stored (pending items included; clients never see those).
pub async fn inbox_items(test: &TestServer, account_id: u32) -> Vec<(u32, InboxItem)> {
    let mut items = Vec::new();
    for document_id in inbox_ids(test, account_id).await {
        if let Some((meta, content)) = inbox_records(test, account_id, document_id).await {
            items.push((
                document_id,
                InboxItem::new(
                    meta.deserialize::<CalendarEventNotification>().unwrap(),
                    content
                        .deserialize::<CalendarEventNotificationContent>()
                        .unwrap(),
                ),
            ));
        }
    }
    items
}

/// The document ids of the account's scheduling inbox items (0.16's
/// `itip_ids`: at 1.0 every item is indexed by its creation time).
pub async fn inbox_ids(test: &TestServer, account_id: u32) -> Vec<u32> {
    test.server
        .document_ids(
            account_id,
            Collection::CalendarEventNotification,
            CalendarNotificationField::Created,
        )
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
) -> Vec<(u32, InboxItem)> {
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
    let (_, content) = inbox_records(test, recipient, document_id)
        .await
        .expect("inbox item");
    let mut message = into_item_message(
        content
            .deserialize::<CalendarEventNotificationContent>()
            .unwrap(),
    )
    .unwrap_or_else(|| panic!("item {document_id} is not a fork item"));
    let carriers = open_pending_item(&mut message, recipient, &private_key)
        .unwrap_or_else(|err| panic!("item {document_id} does not open: {err:?}"));
    (message, carriers)
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

/// Upgrade ruling 156: one of `common::za::test_hooks`' pause slots, as the
/// test found it, restored when the guard drops (also when the test panics),
/// so a slot the test armed and a failure left set cannot park a later
/// module's request. The value restored is the prior one, not `None`. A pause
/// still armed in the slot at the drop, other than the prior one, is
/// released first, so a request parked on it does not stay parked.
#[must_use = "dropping the guard restores the slot"]
pub struct PauseSlotGuard {
    slot: &'static std::sync::Mutex<Option<Arc<HookPause>>>,
    prior: Option<Arc<HookPause>>,
}

impl PauseSlotGuard {
    pub fn new(slot: &'static std::sync::Mutex<Option<Arc<HookPause>>>) -> Self {
        let prior = slot.lock().unwrap_or_else(|e| e.into_inner()).clone();
        PauseSlotGuard { slot, prior }
    }
}

impl Drop for PauseSlotGuard {
    fn drop(&mut self) {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(armed) = slot.as_ref()
            && !self
                .prior
                .as_ref()
                .is_some_and(|prior| Arc::ptr_eq(prior, armed))
        {
            armed.release.notify_one();
        }
        *slot = self.prior.take();
    }
}

/// Upgrade ruling 156: `CalendarScheduling`'s `max_recipients` set for the
/// guard's lifetime. `restore` puts the prior value back and reloads the
/// settings; a guard dropped without it (a panic) does the same from its
/// drop, on a thread of its own, so a failure cannot leave the cap set for
/// later modules.
#[must_use = "dropping the guard restores max_recipients"]
pub struct MaxRecipientsGuard {
    admin: Option<Account>,
    prior: u64,
}

impl MaxRecipientsGuard {
    pub async fn set(admin: &Account, max_recipients: u64) -> Self {
        use registry::schema::{prelude::Property, structs::CalendarScheduling};
        let prior = admin
            .registry_get::<CalendarScheduling>(types::id::Id::singleton())
            .await
            .max_recipients;
        admin
            .registry_update_setting(
                CalendarScheduling {
                    max_recipients,
                    ..Default::default()
                },
                &[Property::MaxRecipients],
            )
            .await;
        admin.reload_settings().await;
        MaxRecipientsGuard {
            admin: Some(admin.clone()),
            prior,
        }
    }

    pub async fn restore(mut self) {
        if let Some(admin) = self.admin.take() {
            set_max_recipients(&admin, self.prior).await;
        }
    }
}

impl Drop for MaxRecipientsGuard {
    fn drop(&mut self) {
        if let Some(admin) = self.admin.take() {
            let prior = self.prior;
            let _ = std::thread::spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("a runtime for the restore")
                    .block_on(set_max_recipients(&admin, prior));
            })
            .join();
        }
    }
}

async fn set_max_recipients(admin: &Account, max_recipients: u64) {
    use registry::schema::{prelude::Property, structs::CalendarScheduling};
    admin
        .registry_update_setting(
            CalendarScheduling {
                max_recipients,
                ..Default::default()
            },
            &[Property::MaxRecipients],
        )
        .await;
    admin.reload_settings().await;
}

/// PR review ruling 196: `Jmap`'s `max_concurrent_requests` (the per-account
/// HTTP concurrency limit) set for the guard's lifetime. Tokens built while
/// it is set carry it, so a test sets it before its caller first
/// authenticates. `restore` puts the prior value back and reloads the
/// settings; a guard dropped without it (a panic) does the same from its
/// drop, on a thread of its own, as `MaxRecipientsGuard` does.
#[must_use = "dropping the guard restores max_concurrent_requests"]
pub struct ConcurrentRequestsGuard {
    admin: Option<Account>,
    prior: Option<u64>,
}

impl ConcurrentRequestsGuard {
    pub async fn set(admin: &Account, max_concurrent_requests: u64) -> Self {
        use registry::schema::structs::Jmap;
        let prior = admin
            .registry_get::<Jmap>(types::id::Id::singleton())
            .await
            .max_concurrent_requests;
        set_max_concurrent_requests(admin, Some(max_concurrent_requests)).await;
        ConcurrentRequestsGuard {
            admin: Some(admin.clone()),
            prior,
        }
    }

    pub async fn restore(mut self) {
        if let Some(admin) = self.admin.take() {
            set_max_concurrent_requests(&admin, self.prior).await;
        }
    }
}

impl Drop for ConcurrentRequestsGuard {
    fn drop(&mut self) {
        if let Some(admin) = self.admin.take() {
            let prior = self.prior;
            let _ = std::thread::spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("a runtime for the restore")
                    .block_on(set_max_concurrent_requests(&admin, prior));
            })
            .join();
        }
    }
}

async fn set_max_concurrent_requests(admin: &Account, max_concurrent_requests: Option<u64>) {
    use registry::schema::{prelude::Property, structs::Jmap};
    admin
        .registry_update_setting(
            Jmap {
                max_concurrent_requests,
                ..Default::default()
            },
            &[Property::MaxConcurrentRequests],
        )
        .await;
    admin.reload_settings().await;
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
                ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task {
                    id: TaskId::Assigned(0),
                })),
                ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task {
                    id: TaskId::Assigned(u64::MAX),
                })),
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
    // `Some(pending)` for the fork's own item shape, `None` for a plain one.
    let mut fork_item = None;
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
                None,
            )
            .unwrap();
            fork_item = Some(true);
            EVENT_NOTIFICATION_ZA_PENDING
        }
        PlantedItem::Drained { password } => {
            let keys = za_unlock(test, account_id, password).await;
            seal_drained_item(&mut tree, &keys, account_id).unwrap();
            fork_item = Some(false);
            0
        }
        PlantedItem::Plain { flags } => flags,
    };
    // Upgrade ruling 13: a fork item is a row and an `Itip` content with no
    // snapshot (`seal::item::sealed_item_record`'s shape); the row's size
    // comes from upstream's builder. A planted plain item keeps that shape
    // with the flags given.
    let now = store::write::now() as i64;
    let (mut meta, content) = match fork_item {
        Some(pending) => groupware::calendar::seal::item::sealed_item_record(
            tree,
            ChangedBy::PrincipalId(sender),
            None,
            pending,
        ),
        None => (
            CalendarEventNotification {
                changed_by: ChangedBy::PrincipalId(sender),
                flags,
                ..Default::default()
            },
            CalendarEventNotificationContent::Itip {
                message: tree,
                previous: None,
                current: None,
            },
        ),
    };
    meta.created = created.unwrap_or(now);
    meta.modified = now;
    let changed_by = test
        .server
        .account_info(account_id)
        .await
        .unwrap()
        .changed_by();
    let mut batch = BatchBuilder::new();
    let slot = batch.reserve_document_id(account_id, Collection::CalendarEventNotification);
    batch
        .with_account_id(account_id)
        .with_collection(Collection::CalendarEventNotification)
        .with_pending_document(slot.into())
        .custom(
            ObjectIndexBuilder::<(), _>::new()
                .with_changes(GroupwareWrite::full(meta, content, ()).unwrap())
                .with_changed_by(changed_by),
        )
        .unwrap()
        .commit_point();
    let document_id = test.server.commit_batch(batch).await.unwrap().slot(slot);
    test.server
        .invalidate_local_caches(&[CacheInvalidation::GroupwareResources(account_id)]);
    document_id
}

/// Rewrites a planted item in place, same document id, as the drain's commit
/// does (spec 6.4 step 5): `edit` changes the stored item, and the write
/// asserts the row it read (`SplitUpdate::full` over the row and content).
pub async fn rewrite_inbox_item(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
    edit: impl FnOnce(&mut InboxItem),
) {
    let (meta_archive, content_archive) = inbox_records(test, account_id, document_id)
        .await
        .expect("inbox item");
    let meta = meta_archive
        .deserialize::<CalendarEventNotification>()
        .unwrap();
    let mut item = InboxItem::new(
        meta.clone(),
        content_archive
            .deserialize::<CalendarEventNotificationContent>()
            .unwrap(),
    );
    edit(&mut item);
    let content = match item.content {
        CalendarEventNotificationContent::Itip {
            previous, current, ..
        } => CalendarEventNotificationContent::Itip {
            message: item.event,
            previous,
            current,
        },
        direct => direct,
    };
    let next = CalendarEventNotification {
        event_id: item.event_id,
        changed_by: item.changed_by,
        calendar_ids: item.calendar_ids,
        flags: item.flags,
        created: item.created,
        modified: store::write::now() as i64,
        ..meta
    };
    let changed_by = test
        .server
        .account_info(account_id)
        .await
        .unwrap()
        .changed_by();
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::CalendarEventNotification)
        .with_document(document_id)
        .custom(
            common::storage::index::SplitUpdate::full(
                meta_archive
                    .to_unarchived::<CalendarEventNotification>()
                    .unwrap(),
                content_archive
                    .unarchive::<CalendarEventNotificationContent>()
                    .unwrap(),
                next,
                content,
                (),
            )
            .unwrap()
            .into_builder(changed_by, None),
        )
        .unwrap()
        .commit_point();
    test.server.commit_batch(batch).await.unwrap();
    test.server
        .invalidate_local_caches(&[CacheInvalidation::GroupwareResources(account_id)]);
}

/// True for a stored inbox item whose pending bit is set (spec 9).
pub async fn inbox_item_is_pending(test: &TestServer, account_id: u32, document_id: u32) -> bool {
    inbox_records(test, account_id, document_id)
        .await
        .is_some_and(|(meta, _)| {
            groupware::calendar::seal::archived_item_is_pending(
                meta.unarchive::<CalendarEventNotification>().unwrap(),
            )
        })
}

/// Deletes an inbox item at store level, as `itip_auto_expunge` does. Upgrade
/// ruling 176 (B-M4): a client's DELETE of a pending item answers 404, as
/// its GET does, so cleanups (and the races that need an item gone under
/// the drain) remove pending items here.
pub async fn destroy_inbox_item(test: &TestServer, account_id: u32, document_id: u32) {
    let Some((meta, _)) = inbox_records(test, account_id, document_id).await else {
        return;
    };
    let changed_by = test
        .server
        .account_info(account_id)
        .await
        .unwrap()
        .changed_by();
    let mut batch = BatchBuilder::new();
    groupware::DestroyArchive(meta.to_unarchived::<CalendarEventNotification>().unwrap())
        .delete(changed_by, account_id, document_id, &mut batch)
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    test.server
        .invalidate_local_caches(&[CacheInvalidation::GroupwareResources(account_id)]);
}

/// Deletes `client`'s own inbox item: by href (upstream's DELETE, 204), or
/// at store level when it is pending (`destroy_inbox_item`).
pub async fn delete_inbox_item(
    test: &TestServer,
    client: &crate::utils::webdav::DummyWebDavClient,
    account_id: u32,
    document_id: u32,
) {
    if inbox_item_is_pending(test, account_id, document_id).await {
        destroy_inbox_item(test, account_id, document_id).await;
    } else {
        client
            .request(
                "DELETE",
                &format!(
                    "/dav/itip/{}/inbox/{document_id}.ics",
                    client.name.replace('@', "%40")
                ),
                "",
            )
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
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

/// An OAuth grant of `name` restricted to `scopes`, for a third-party
/// (`Registered`) client, and an access token bound to it, minted
/// in-process with the server's own helpers. 1.0 binds every access token
/// to a grant and resolves only registered, stateless or metadata-document
/// clients, so 0.16's `/api/auth` and `/auth/token` flow no longer applies; the
/// caller revokes the grant (`GrantDeletion`) when it is done. Returns the
/// grant id and the token.
pub async fn za_grant_token(
    server: &common::Server,
    name: &str,
    scopes: common::auth::oauth::scope::OAuthScopes,
) -> (u64, String) {
    let account_id = server
        .account_id_from_email(name, false)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("no account {name}"));
    let grant_id = server
        .create_oauth_grant(common::auth::oauth::grant::NewOAuthGrant {
            account_id,
            client_id: "za-test-client",
            client_name: None,
            client_kind: registry::schema::enums::OAuthClientKind::Registered,
            scopes,
            remote_ip: std::net::IpAddr::from([127, 0, 0, 1]),
        })
        .await
        .unwrap();
    let credential_version = server
        .access_token(account_id)
        .await
        .unwrap()
        .credential_version();
    let token = server
        .encode_grant_access_token(account_id, name, grant_id, credential_version)
        .unwrap();
    (grant_id, token)
}

/// Deletes the grant `za_grant_token` created and applies its invalidation
/// (1.0's `GrantRevocations::evict`).
pub async fn za_revoke_grant(server: &common::Server, grant_id: u64) {
    let stored = server
        .oauth_grant(grant_id)
        .await
        .unwrap()
        .expect("the grant exists");
    let mut invalidator = common::cache::invalidate::CacheInvalidationBuilder::default();
    assert!(
        server
            .delete_oauth_grant(
                stored,
                common::auth::oauth::grant::GrantDeletion::Unconditional,
                &mut invalidator,
            )
            .await
            .unwrap()
    );
    server.invalidate_caches(invalidator).await.unwrap();
}

/// An OAuth access token of `name` with every scope (`za_grant_token`). The
/// token holds no keys.
pub async fn za_bearer_token(server: &common::Server, name: &str) -> String {
    za_grant_token(server, name, common::auth::oauth::scope::OAuthScopes::ALL)
        .await
        .1
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
        .fetch_groupware_resources(account_id, account_id, SyncCollection::Calendar)
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
    test.server.store().write_batch(&mut batch).await.unwrap();
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

/// Case 25's narrow equality oracle: keep SEQUENCE, STATUS, recurrence/date rules
/// and every ATTENDEE parameter. Only server timestamps, fixture UID and delegated
/// ORGANIZER SENT-BY are allowed to differ between the two independently written runs.
pub fn normalize_sharing_copy(mut ical: ICalendar, uid: &str) -> ICalendar {
    for component in &mut ical.components {
        component.entries.retain(|entry| {
            !matches!(
                entry.name,
                ICalendarProperty::Dtstamp
                    | ICalendarProperty::Created
                    | ICalendarProperty::LastModified
            )
        });
        for entry in &mut component.entries {
            if entry.name == ICalendarProperty::Uid {
                entry.values = vec![ICalendarValue::Text(uid.into())].into();
            } else if entry.name == ICalendarProperty::Organizer {
                entry
                    .params
                    .retain(|parameter| parameter.name != ICalendarParameterName::SentBy);
            }
        }
        component.entries.sort_unstable();
    }
    // Component indices are parser-internal. Rebuild them in semantic component order.
    let mut order: Vec<usize> = (1..ical.components.len()).collect();
    order.sort_by(|&a, &b| ical.components[a].cmp(&ical.components[b]));
    order.insert(0, 0);
    let mut mapped = vec![0u32; order.len()];
    for (new, &old) in order.iter().enumerate() {
        mapped[old] = new as u32;
    }
    let original = ical.components.clone();
    ical.components = order
        .into_iter()
        .map(|old| {
            let mut component = original[old].clone();
            for child in &mut component.component_ids {
                *child = mapped[*child as usize];
            }
            component.component_ids.sort_unstable();
            component
        })
        .collect();
    ical
}

/// A complete public DAV request with enough timeout for deliberate commit pauses.
/// This helper carries only disposable fixture credentials.
pub async fn sharing_dav_request(
    username: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> crate::utils::http::HttpResponseFull {
    let mut request = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
        .request(
            Method::from_bytes(method.as_bytes()).unwrap(),
            format!("{SERVER_URL}{path}"),
        )
        .basic_auth(username, Some(crate::za::STRONG))
        .body(body.to_string());
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().await.unwrap();
    crate::utils::http::HttpResponseFull {
        status,
        headers,
        body,
    }
}

/// The account-partitioned indexes of 1.0's index queue (Task 1's ruling
/// 22): their partition is the account, so their entries and status key
/// are the account's.
pub const ACCOUNT_INDEXES: [SearchIndex; 4] = [
    SearchIndex::Email,
    SearchIndex::Calendar,
    SearchIndex::Contacts,
    SearchIndex::File,
];

/// One entry of 1.0's search index queue (`SearchIndexClass::Queue`, in the
/// data store): the index, the document and whether it indexes (`true`,
/// value `1`) or unindexes (value `0`), as `read_queue` reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueEntry {
    pub index: SearchIndex,
    pub document_id: u32,
    pub insert: bool,
}

/// The account's index-queue entries in every account-partitioned index,
/// in queue order (index, document, creation). A key is the index byte, the
/// partition (the account, u32 BE), the document id (u32 BE) and the
/// creation id (u64 BE).
pub async fn queue_entries(test: &TestServer, account_id: u32) -> Vec<QueueEntry> {
    let mut entries = Vec::new();
    for index in ACCOUNT_INDEXES {
        let (from, to) = SearchIndexClass::queue_range(index, account_id);
        test.server
            .store()
            .iterate(
                IterateParams::new(
                    ValueKey::from(ValueClass::SearchIndex(from)),
                    ValueKey::from(ValueClass::SearchIndex(to)),
                )
                .ascending(),
                |key, value| {
                    entries.push(QueueEntry {
                        index,
                        document_id: key.deserialize_be_u32(1 + U32_LEN)?,
                        insert: value.first().is_some_and(|action| *action != 0),
                    });
                    Ok(true)
                },
            )
            .await
            .unwrap();
    }
    entries
}

/// The status key of the account's partition in `index`
/// (`SearchIndexClass::QueueStatus`): set by a failed pass, by
/// `x:IndexQueueStatus/set` and by `hold_index_partition`, cleared by the
/// next pass that completes.
pub async fn partition_status(
    test: &TestServer,
    index: SearchIndex,
    account_id: u32,
) -> Option<SearchIndexStatus> {
    test.server
        .search_index_status(index, account_id)
        .await
        .unwrap()
}

/// The reason `hold_index_partition` writes into the status key.
pub const HELD_REASON: &str = "held by the za suite";

/// Holds the account's partition of `index` in the index queue: its status
/// key is written as a failed pass writes it (and as `x:IndexQueueStatus/set`
/// with a `Failed` status does), with its retry an hour away, so the
/// manager's scan skips the partition (`scan_index_queue` skips a status
/// whose retry is in the future) while its entries stay stored. 1.0's
/// stand-in for 0.16's held task consumer, which does not hold the index
/// queue. Call it only while the partition is idle (no pass in flight);
/// `TestServer::wait_for_tasks` waits for a held partition, so release it
/// first.
pub async fn hold_index_partition(test: &TestServer, index: SearchIndex, account_id: u32) {
    let mut batch = BatchBuilder::new();
    batch.set(
        ValueClass::SearchIndex(SearchIndexClass::QueueStatus {
            index,
            partition: account_id,
        }),
        SearchIndexStatus {
            next_retry: now() + 3600,
            attempts: 0,
            reason: HELD_REASON.to_string(),
        }
        .serialize(),
    );
    test.server.store().write_batch(&mut batch).await.unwrap();
}

/// Releases `hold_index_partition`'s hold (or a failed pass's retry state,
/// as `x:IndexQueueStatus/set` with a `Running` status does): the status key
/// is cleared and the manager is woken, so the partition runs at once.
/// Takes a `Server` so a parked request's future can call it.
pub async fn release_index_partition(server: &common::Server, index: SearchIndex, account_id: u32) {
    let mut batch = BatchBuilder::new();
    batch.clear(ValueClass::SearchIndex(SearchIndexClass::QueueStatus {
        index,
        partition: account_id,
    }));
    server.store().write_batch(&mut batch).await.unwrap();
    server.notify_index_queue();
}

/// Waits (at most `limit`) until the account has no index-queue entry and no
/// partition status in any account-partitioned index; panics naming what is
/// left. Unlike `TestServer::wait_for_tasks`, it does not wait out a
/// partition's retry delay: it fails if the queue is not drained in time.
pub async fn wait_for_index_queue(test: &TestServer, account_id: u32, limit: Duration) {
    let deadline = std::time::Instant::now() + limit;
    loop {
        let entries = queue_entries(test, account_id).await;
        let mut statuses = Vec::new();
        for index in ACCOUNT_INDEXES {
            if let Some(status) = partition_status(test, index, account_id).await {
                statuses.push((index, status));
            }
        }
        if entries.is_empty() && statuses.is_empty() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the index queue of account {account_id} did not drain in {limit:?}: \
             entries {entries:?}, statuses {statuses:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The internal search store (the suites run on it).
pub fn internal_search(test: &TestServer) -> store::Store {
    test.server
        .search_store()
        .internal_fts()
        .expect("the suite runs on the internal search store")
        .clone()
}

/// Writes `documents` into the account's `index` straight through the
/// internal search store, as the index worker's flush writes them (one
/// `index_account_documents` pass). 1.0's `SearchStore::index` is not
/// available on the internal store.
pub async fn index_search_documents(
    test: &TestServer,
    index: SearchIndex,
    account_id: u32,
    documents: Vec<store::search::IndexDocument>,
) {
    internal_search(test)
        .index_account_documents(account_id, index, documents, vec![])
        .await
        .unwrap();
}

/// Unindexes `document_ids` of the account's `index` as the index worker's
/// deletions do: each stored `Document` key and every term it lists.
pub async fn unindex_search_documents(
    test: &TestServer,
    index: SearchIndex,
    account_id: u32,
    document_ids: Vec<u32>,
) {
    internal_search(test)
        .index_account_documents(account_id, index, vec![], document_ids)
        .await
        .unwrap();
}

/// Removes every key of the account in `index` (`Store::unindex_account`,
/// the range delete account destruction runs): R62's remedy for orphaned
/// terms.
pub async fn unindex_search_account(test: &TestServer, index: SearchIndex, account_id: u32) {
    internal_search(test)
        .unindex_account(index, account_id)
        .await
        .unwrap();
}

/// The stored value of a search document that holds no term and no field
/// position (`Document::encode` then `Writer::into_document`,
/// `store/src/search/{term,codec}.rs`, crate-private): the term count 0,
/// the field count 0 (LEB128) and the plain marker 0. A key account's
/// Email document holds its ids only (ruling 64), so its stored value is
/// exactly this.
pub const IDS_ONLY_DOCUMENT: [u8; 3] = [0, 0, 0];

/// The account's keys in one index of the internal search store: its
/// `Document` keys (`Subspace::SearchDocument`: the index byte, the account
/// id and the document id) with their values, and its `Term` keys
/// (`Subspace::SearchTerm`: the index byte, the account id, the term group,
/// the term's `CheekyHash` key bytes, their length, the field and the block
/// id, u16 BE), with each term's key bytes, which are the token itself for
/// one of up to 16 bytes. 1.0 has no separate `Index` keys: integer and
/// keyword fields are terms too.
#[derive(Default)]
pub struct SearchKeys {
    pub documents: Vec<(u32, Vec<u8>)>,
    pub terms: usize,
    pub term_payloads: Vec<Vec<u8>>,
}

impl SearchKeys {
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty() && self.terms == 0
    }

    pub fn document_ids(&self) -> Vec<u32> {
        self.documents.iter().map(|(id, _)| *id).collect()
    }

    pub fn holds_term(&self, token: &str) -> bool {
        self.term_payloads
            .iter()
            .any(|payload| payload == token.as_bytes())
    }

    /// Whether every stored document holds its ids only (`IDS_ONLY_DOCUMENT`).
    pub fn documents_are_ids_only(&self) -> bool {
        self.documents
            .iter()
            .all(|(_, value)| value.as_slice() == IDS_ONLY_DOCUMENT)
    }
}

/// Counts only: the payloads of a large document would flood a failure.
impl std::fmt::Debug for SearchKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "documents {:?} ({} ids only), {} term keys",
            self.document_ids(),
            self.documents
                .iter()
                .filter(|(_, value)| value.as_slice() == IDS_ONLY_DOCUMENT)
                .count(),
            self.terms
        )
    }
}

/// `SearchKeys` of `account_id` in `index`, read raw from the internal
/// search store.
pub async fn search_keys(test: &TestServer, account_id: u32, index: SearchIndex) -> SearchKeys {
    const TERM_BASE: usize = 1 + U32_LEN + 1;
    let store = internal_search(test);
    let mut prefix = vec![index.to_u8()];
    prefix.extend_from_slice(&account_id.to_be_bytes());
    let mut end = prefix.clone();
    end.extend_from_slice(&[u8::MAX; 64]);
    let mut keys = SearchKeys::default();
    store
        .iterate(
            IterateParams::new(
                AnyKey {
                    subspace: Subspace::SearchDocument,
                    key: prefix.clone(),
                },
                AnyKey {
                    subspace: Subspace::SearchDocument,
                    key: end.clone(),
                },
            ),
            |key, value| {
                if key.len() == 1 + U32_LEN * 2 {
                    keys.documents
                        .push((key.deserialize_be_u32(1 + U32_LEN)?, value.to_vec()));
                }
                Ok(true)
            },
        )
        .await
        .unwrap();
    store
        .iterate(
            IterateParams::new(
                AnyKey {
                    subspace: Subspace::SearchTerm,
                    key: prefix,
                },
                AnyKey {
                    subspace: Subspace::SearchTerm,
                    key: end,
                },
            )
            .no_values(),
            |key, _| {
                keys.terms += 1;
                // The term's key bytes: after the base, before its length,
                // field and block id.
                if let Some(payload) = key
                    .len()
                    .checked_sub(4)
                    .and_then(|end| key.get(TERM_BASE..end))
                {
                    keys.term_payloads.push(payload.to_vec());
                }
                Ok(true)
            },
        )
        .await
        .unwrap();
    keys
}

/// Deletes the account's event `document_id` straight from the store, as a
/// DELETE does without its scheduling (`DestroyArchive::delete_all`: the
/// row, the content record, the alarms and the index-queue unindex entry),
/// and wakes the index queue.
pub async fn delete_event_records(test: &TestServer, account_id: u32, document_id: u32) {
    let (meta, content) = event_records(test, account_id, document_id).await;
    let account_info = test.server.account_info(account_id).await.unwrap();
    let mut batch = BatchBuilder::new();
    groupware::DestroyArchive(meta.to_unarchived::<CalendarEvent>().unwrap())
        .delete_all(
            &test.server,
            &account_info,
            account_id,
            document_id,
            Some(content),
            false,
            &mut batch,
        )
        .await
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    test.server.notify_index_queue();
    test.server
        .invalidate_local_caches(&[CacheInvalidation::GroupwareResources(account_id)]);
}
