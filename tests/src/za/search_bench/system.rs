/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    ACCOUNT, BODY_WORD, Generated, PASSWORD, accounting::Capture, deliver, generated, message_ids,
};
use crate::utils::{
    account::Account,
    server::{TestServer, TestServerBuilder},
    za::{manifest_bytes, principal_blob_links, read_manifest, write_manifest_bytes, za_unlock},
};
use common::{
    auth::AccessToken,
    za::{
        ZA_MAIL_SEARCH_UNAVAILABLE,
        search::{
            ZaSearchConfig,
            cache::EvictReason,
            trace::{ZA_SEARCH_REBUILD, ZA_SEARCH_UNAVAILABLE},
        },
        test_hooks::za_search_blob_reads,
    },
};
use email::cache::MessageCacheFetch;
use jmap::email::za_search::za_content_current;
use mail_builder::MessageBuilder;
use mail_parser::MessageParser;
use serde_json::{Value, json};
use std::{
    cell::Cell,
    collections::{BTreeMap, HashMap, HashSet},
    error::Error,
    fmt::Write,
    io,
    process::Command,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};
use store::{
    ValueKey,
    write::serialize::RawValue,
    za_search::{
        BlobHeader, BlobKind,
        blob::SEAL_OVERHEAD,
        manifest::{Manifest, ManifestValue},
    },
};
use tokio::time::{MissedTickBehavior, interval, timeout, timeout_at};
use types::{
    collection::{Collection, SyncCollection},
    field::PrincipalField,
    id::Id,
};

const PLAIN: &str = "search-bench-plain@example.com";
const CAP_CONTROL: &str = "search-bench-cap@example.com";
const MIB: u64 = 1 << 20;
const SMALL: usize = 2_000;
const DESIGN: usize = 30_000;
const SEED: u64 = 1;
// Client measurement headroom for owned compaction. Product deadlines are separate.
const WIRE_TIMEOUT: Duration = Duration::from_secs(600);
const MILESTONE_LOAD_TIMEOUT: Duration = Duration::from_secs(60);
const PHASE_TIMEOUT: Duration = Duration::from_secs(14_400);
const SYSTEM_TIMEOUT: Duration = Duration::from_secs(43_200);
const GROWTH: [usize; 7] = [2_000, 5_000, 10_000, 15_000, 20_000, 25_000, DESIGN];

struct Wire<'a> {
    account: &'a Account,
    client: reqwest::Client,
}

impl<'a> Wire<'a> {
    fn new(account: &'a Account) -> Self {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(WIRE_TIMEOUT)
            .build()
            .unwrap();
        Self { account, client }
    }

    async fn call(&self, method: &str, args: Value) -> (Duration, Value) {
        self.try_call(method, args)
            .await
            .unwrap_or_else(|error| panic!("{method}: {error}"))
    }

    async fn try_call(&self, method: &str, mut args: Value) -> Result<(Duration, Value), String> {
        args["accountId"] = json!(self.account.id_string());
        let payload = json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            "methodCalls": [[method, args, "bench"]],
        });
        let request = self
            .client
            .post(self.account.api_url())
            .basic_auth(self.account.name(), Some(self.account.secret()))
            .json(&payload);
        let start = Instant::now();
        let response = request
            .send()
            .await
            .map_err(|error| transport_failure("send", start.elapsed(), &error))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|error| transport_failure("body", start.elapsed(), &error))?;
        let elapsed = start.elapsed();
        assert!(
            status.is_success(),
            "{method}: HTTP {status} after {elapsed:?}"
        );
        let response: Value = serde_json::from_slice(&body).map_err(|error| {
            format!(
                "JSON decode failure after {:?} (wire {elapsed:?}): {:?}, line {}, column {}",
                start.elapsed(),
                error.classify(),
                error.line(),
                error.column()
            )
        })?;
        let calls = response["methodResponses"].as_array().ok_or_else(|| {
            format!("response envelope failure after {elapsed:?}: missing methodResponses array")
        })?;
        assert_eq!(calls.len(), 1, "{method}: unexpected method response count");
        assert!(calls[0][2] == "bench", "{method}: unexpected call ID");
        Ok((elapsed, calls[0].clone()))
    }

    async fn get(&self) -> Duration {
        self.try_get()
            .await
            .unwrap_or_else(|error| panic!("Email/get: {error}"))
    }

    async fn try_get(&self) -> Result<Duration, String> {
        let (elapsed, call) = self.try_call("Email/get", json!({"ids": []})).await?;
        assert!(
            reply("Email/get", &call)["list"] == json!([]),
            "empty Email/get returned a nonempty or invalid list"
        );
        Ok(elapsed)
    }

    async fn search(&self, term: &str, limit: usize) -> (Duration, Value) {
        self.call(
            "Email/query",
            json!({"filter": {"text": term}, "limit": limit}),
        )
        .await
    }
}

fn transport_failure(stage: &str, elapsed: Duration, error: &reqwest::Error) -> String {
    // Error Display and Debug may include URLs or data from protocol parsing.
    // Walk the source chain using only typed, non-payload classifications.
    let mut causes = Vec::new();
    let mut source = error.source();
    while let Some(cause) = source {
        causes.push(if let Some(error) = cause.downcast_ref::<io::Error>() {
            let tls = error
                .get_ref()
                .is_some_and(|inner| inner.is::<rustls::Error>());
            format!(
                "io(kind={:?}, os_code={:?}, tls={tls})",
                error.kind(),
                error.raw_os_error()
            )
        } else if cause.is::<rustls::Error>() {
            "tls".into()
        } else if let Some(error) = cause.downcast_ref::<hyper::Error>() {
            format!(
                "http(parse={}, timeout={}, canceled={}, closed={}, incomplete={})",
                error.is_parse(),
                error.is_timeout(),
                error.is_canceled(),
                error.is_closed(),
                error.is_incomplete_message()
            )
        } else {
            "unclassified".into()
        });
        source = cause.source();
    }
    format!(
        "{stage} failure after {elapsed:?}: timeout={}, connect={}, dns={}, request={}, body={}, decode={}; sources={causes:?}",
        error.is_timeout(),
        error.is_connect(),
        error.is_dns(),
        error.is_request(),
        error.is_body(),
        error.is_decode()
    )
}

fn reply<'a>(method: &str, call: &'a Value) -> &'a Value {
    assert!(call[0] == method, "{method}: unexpected response method");
    &call[1]
}

fn assert_unavailable(call: &Value) {
    assert!(call[0] == "error", "expected an unavailable error response");
    assert!(
        call[1]["type"] == "unsupportedFilter",
        "unexpected unavailable error type"
    );
    let description = format!("text: {ZA_MAIL_SEARCH_UNAVAILABLE}");
    assert!(
        call[1]["description"].as_str() == Some(description.as_str()),
        "unexpected unavailable description"
    );
}

fn ids(value: &Value) -> Vec<Id> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().parse().unwrap())
        .collect()
}

fn uid(account: &Account) -> u32 {
    account.id().document_id()
}

fn format_ms(d: Duration) -> String {
    format!("{:.3} ms", d.as_secs_f64() * 1_000.0)
}

fn intervals(period: Duration) -> tokio::time::Interval {
    let mut ticks = interval(period);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
    ticks
}

async fn server() -> TestServer {
    assert!(
        std::env::var_os("NO_INSERT").is_none(),
        "benchmark needs a fresh store"
    );
    TestServerBuilder::new("search_bench")
        .await
        .with_default_listeners()
        .await
        .build()
        .await
}

async fn key(test: &TestServer, admin: &Account, name: &'static str) -> (Account, Vec<u8>) {
    let account = admin
        .create_key_user_account(name, PASSWORD, "Search benchmark", &[], vec![])
        .await;
    let id = uid(&account);
    let keys = za_unlock(test, id, account.secret()).await;
    let key = common::za::search::seal::za_search_key(keys.swk());
    let (bytes, value) = test
        .server
        .za_search_read_manifest(id, &key)
        .await
        .unwrap()
        .unwrap();
    let ManifestValue::Index(manifest) = value else {
        panic!("setup must create an index manifest")
    };
    let state = test
        .server
        .store()
        .get_last_change_id(id, SyncCollection::Email.into())
        .await
        .unwrap()
        .unwrap_or_default();
    let checkpoint = store::za_search::Checkpoint {
        change_id: state,
        position: 0,
        complete: true,
    };
    let expected = Manifest {
        indexed_to: checkpoint,
        content_to: store::za_search::manifest::ContentTo {
            checkpoint,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        manifest, expected,
        "fresh setup must create only the authenticated empty checkpoint"
    );
    assert!(principal_blob_links(test, id).await.is_empty());
    assert!(test.server.inner.cache.za_search.get(id).is_none());
    (account, bytes)
}

async fn assert_pristine_setup(test: &TestServer, id: u32, setup: Vec<u8>) {
    assert_eq!(
        manifest_bytes(test, id).await,
        Some(setup),
        "unkeyed delivery changed the setup manifest"
    );
    assert!(principal_blob_links(test, id).await.is_empty());
    assert!(test.server.inner.cache.za_search.get(id).is_none());
}

async fn deliver_cohort(
    test: &TestServer,
    account: &Account,
    input: impl Iterator<Item = Generated>,
    expected: usize,
) -> Vec<u32> {
    let parser = MessageParser::new();
    let token = AccessToken::from_id_maybe_invalid(uid(account));
    let mut delivered = Vec::with_capacity(expected);
    let mut attachments = 0;
    let mut bytes = 0;
    let start = Instant::now();
    for message in input {
        attachments += usize::from(message.attachment_bytes > 0);
        bytes += message.raw.len();
        delivered.push(deliver(test, account, &token, &parser, &message.raw).await);
    }
    assert_eq!(delivered.len(), expected);
    assert_eq!(
        delivered.iter().copied().collect::<HashSet<_>>().len(),
        expected
    );
    println!(
        "Delivered {expected}, {attachments} attachments, {bytes} MIME bytes, seed {SEED}, elapsed {:?}",
        start.elapsed()
    );
    delivered
}

async fn assert_mailbox(test: &TestServer, id: u32, delivered: &[u32]) {
    let stored = message_ids(test, id)
        .await
        .into_iter()
        .collect::<HashSet<_>>();
    assert_eq!(stored.len(), delivered.len());
    assert!(delivered.iter().all(|id| stored.contains(id)));
}

#[derive(Debug)]
struct Progress {
    email_state: u64,
    scanned_to: u64,
    manifest: Manifest,
    current: bool,
    indexed: usize,
    thread_estimate: usize,
    thread_charge: u64,
    content: Option<(usize, usize, u64)>,
}

impl Progress {
    fn map_complete(&self) -> bool {
        self.scanned_to >= self.email_state
            && self.manifest.indexed_to.complete
            && self.manifest.cursor.is_none()
            && self.manifest.reconciliation.is_none()
    }

    fn sealed(&self) -> u64 {
        self.manifest.content.as_ref().map_or(0, |head| head.size)
    }
}

async fn progress(test: &TestServer, id: u32, passive: bool) -> Option<Progress> {
    // Match production's state probe without initializing the ordinary cache
    // during a cold/concurrent observation.
    let email_state = if !passive {
        test.server
            .get_cached_messages(id)
            .await
            .unwrap()
            .last_change_id
    } else if let Some(messages) = test.server.fresh_messages(id) {
        messages.last_change_id
    } else {
        test.server
            .store()
            .get_last_change_id(id, SyncCollection::Email.into())
            .await
            .unwrap()
            .unwrap_or_default()
    };
    let entry = test.server.inner.cache.za_search.get(id)?;
    // read_loaded() touches the entry's idle time. Sampling must not.
    let guard = if passive {
        entry.index.try_read().ok()?
    } else {
        entry.index.read().await
    };
    let index = guard.as_ref()?;
    Some(Progress {
        email_state,
        scanned_to: index.scanned_to,
        manifest: index.manifest.clone(),
        current: za_content_current(index, email_state),
        indexed: index
            .thread
            .records()
            .filter(|record| record.deleted.is_none())
            .count(),
        thread_estimate: index.thread.loaded_bytes(),
        thread_charge: index.thread_bytes,
        content: index.content.as_ref().map(|content| {
            (
                content.part.len(),
                content.part.loaded_bytes(),
                content.bytes,
            )
        }),
    })
}

async fn disk_manifest(test: &TestServer, id: u32) -> Manifest {
    match read_manifest(test, id).await {
        Some(ManifestValue::Index(manifest)) => manifest,
        _ => panic!("expected an indexed manifest for {id}"),
    }
}

async fn prefix(test: &TestServer, hash: &types::blob_hash::BlobHash, length: usize) -> Vec<u8> {
    test.server
        .get_blob(hash.as_slice(), 0..length)
        .await
        .unwrap()
        .unwrap()
}

async fn inventory(test: &TestServer, account: &Account, manifest: &Manifest) -> Option<usize> {
    let id = uid(account);
    let keys = za_unlock(test, id, account.secret()).await;
    let key = common::za::search::seal::za_search_key(keys.swk());
    let authenticated = test
        .server
        .za_search_read_manifest(id, &key)
        .await
        .unwrap()
        .unwrap()
        .1;
    let ManifestValue::Index(authenticated) = authenticated else {
        panic!("destroying during inventory");
    };
    assert_eq!(&authenticated, manifest);
    let mut links: HashMap<_, _> = principal_blob_links(test, id).await.into_iter().collect();
    let mut hidden_class = None;
    for blob in &manifest.thread_blobs {
        assert_eq!(links.remove(&blob.hash), Some(blob.size as usize));
        let bytes = prefix(test, &blob.hash, BlobHeader::LEN + 4).await;
        let (header, rest) = BlobHeader::decode(&bytes).unwrap();
        assert_eq!(header.kind, BlobKind::Thread);
        assert_eq!(header.segment, blob.segment);
        let length = u32::from_le_bytes(rest.try_into().unwrap()) as usize;
        if length != 0 && hidden_class.is_none() {
            hidden_class = Some(length.checked_sub(SEAL_OVERHEAD).unwrap());
        }
    }
    if let Some(head) = &manifest.content {
        let mut next = Some((head.hash.clone(), head.segment));
        let mut bytes = 0;
        for _ in 0..head.length {
            let (hash, segment) = next.take().expect("short content chain");
            bytes += links
                .remove(&hash)
                .expect("unlinked or cyclic content segment") as u64;
            let prefix = prefix(test, &hash, BlobHeader::LEN).await;
            let (header, _) = BlobHeader::decode(&prefix).unwrap();
            assert_eq!(header.kind, BlobKind::Content);
            assert_eq!(header.segment, segment);
            next = header.predecessor;
        }
        assert!(next.is_none(), "long content chain");
        assert_eq!(bytes, head.size);
    }
    assert!(
        links.is_empty(),
        "unreachable principal blobs: {}",
        links.len()
    );
    hidden_class
}

async fn coverage(test: &TestServer, id: u32, delivered: &[u32]) {
    let entry = test.server.inner.cache.za_search.get(id).unwrap();
    let guard = entry.index.read().await;
    let index = guard.as_ref().unwrap();
    let content = index
        .content
        .as_ref()
        .expect("content must be resident after serving");
    let head = index.manifest.content.as_ref().expect("content head");
    assert_eq!(content.segments.first(), Some(&head.segment));
    assert_eq!(content.part.len(), delivered.len(), "content cardinality");
    for &id in delivered {
        assert!(content.part.contains(id), "missing delivered document {id}");
        assert!(
            index.thread.record(id).is_some(),
            "missing thread document {id}"
        );
    }
}

#[derive(Clone, Copy, Debug)]
struct MemoryPoint {
    elapsed: Duration,
    content: u64,
    thread: u64,
    rss: Option<u64>,
}

#[derive(Default)]
struct Samples {
    first: Option<MemoryPoint>,
    latest: Option<MemoryPoint>,
    content_peak: u64,
    thread_peak: u64,
    rss_peak: u64,
    rss_reads: usize,
    ledger_reads: usize,
    rss_misses: usize,
}

impl Samples {
    fn record(&mut self, point: MemoryPoint, sampled_rss: bool) {
        self.first.get_or_insert(point);
        self.latest = Some(point);
        self.content_peak = self.content_peak.max(point.content);
        self.thread_peak = self.thread_peak.max(point.thread);
        self.ledger_reads += 1;
        if sampled_rss {
            if let Some(rss) = point.rss {
                self.rss_peak = self.rss_peak.max(rss);
                self.rss_reads += 1;
            } else {
                self.rss_misses += 1;
            }
        }
    }
}

struct Sampler {
    stop: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
    samples: Arc<Mutex<Samples>>,
}

impl Sampler {
    fn start(test: &TestServer) -> Self {
        let server = test.server.clone();
        let (stop, rx) = mpsc::channel();
        let samples = Arc::new(Mutex::new(Samples::default()));
        let shared = Arc::clone(&samples);
        let worker = thread::spawn(move || {
            let cache = &server.inner.cache.za_search;
            let start = Instant::now();
            let mut last_rss = None;
            let mut next_rss = start;
            let pid = std::process::id().to_string();
            loop {
                let sampled_rss = Instant::now() >= next_rss;
                if sampled_rss {
                    last_rss = Command::new("/bin/ps")
                        .args(["-o", "rss=", "-p", &pid])
                        .output()
                        .ok()
                        .filter(|out| out.status.success())
                        .and_then(|out| String::from_utf8(out.stdout).ok())
                        .and_then(|out| out.trim().parse::<u64>().ok())
                        .map(|kib| kib * 1024);
                    next_rss = Instant::now() + Duration::from_secs(1);
                }
                shared.lock().unwrap().record(
                    MemoryPoint {
                        elapsed: start.elapsed(),
                        content: cache.content_used(),
                        thread: cache.thread_used(),
                        rss: last_rss,
                    },
                    sampled_rss,
                );
                if !matches!(
                    rx.recv_timeout(Duration::from_millis(50)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    break;
                }
            }
        });
        Self {
            stop: Some(stop),
            worker: Some(worker),
            samples,
        }
    }

    fn point(&self) -> Option<MemoryPoint> {
        self.samples.lock().unwrap().latest
    }

    fn finish(mut self, name: &str) {
        self.join();
        let samples = self.samples.lock().unwrap();
        let print = |p: Option<MemoryPoint>| p.map(|p| (p.elapsed, p.content, p.thread, p.rss));
        println!(
            "Memory {name}: first sampled reading (may be in flight) {:?}, last {:?}, sampled peak content/thread/RSS {}/{}/{}, {} ledger and {} RSS samples, {} RSS misses (targets 50 ms and 1 s, bytes)",
            print(samples.first),
            print(samples.latest),
            samples.content_peak,
            samples.thread_peak,
            samples.rss_peak,
            samples.ledger_reads,
            samples.rss_reads,
            samples.rss_misses
        );
        assert!(samples.rss_reads > 0, "{name}: no same-PID RSS samples");
    }

    fn join(&mut self) {
        if let Some(worker) = self.worker.take() {
            drop(self.stop.take());
            worker.join().unwrap();
        }
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.join();
    }
}

fn growth_line(label: &str, p: &Progress, sampler: &Sampler) {
    println!(
        "{label}: indexed {}, Email state {}, scanned {}, checkpoint {:?}, coverage {:?}, cursors {:?}/{:?}, reconciliation {:?}, epoch/revision {}/{}, content head bytes/length {}/{}, thread estimate/charge {}/{}, resident content (count, estimate, charge) {:?}, process sample {:?}",
        p.indexed,
        p.email_state,
        p.scanned_to,
        p.manifest.indexed_to,
        p.manifest.content_to.checkpoint,
        p.manifest.cursor,
        p.manifest.content_to.cursor,
        p.manifest.reconciliation,
        p.manifest.epoch,
        p.manifest.revision,
        p.sealed(),
        p.manifest.content.as_ref().map_or(0, |head| head.length),
        p.thread_estimate,
        p.thread_charge,
        p.content,
        sampler
            .point()
            .map(|p| (p.elapsed, p.content, p.thread, p.rss))
    );
}

#[derive(Default)]
struct Serial {
    requests: usize,
    pass_time: Duration,
    thread: Vec<(usize, Duration)>,
    full: Vec<(usize, Duration)>,
    tiered: Vec<(usize, Duration, Vec<u8>)>,
    overruns: usize,
    wall: Duration,
}

async fn drain(
    test: &TestServer,
    wire: &Wire<'_>,
    sampler: &Sampler,
    milestones: &[usize],
    loaded_growth: bool,
    label: &str,
) -> (Progress, Serial) {
    let id = uid(wire.account);
    let cap = test.server.za_search_config().account_max_mb * MIB;
    let period = Duration::from_millis(100);
    let mut ticks = intervals(period);
    let mut capture = Capture::start(id);
    let mut serial = Serial::default();
    let mut pending = milestones.iter().copied().peekable();
    let mut previous = None;
    let mut stalls = 0;
    let mut previous_size = 0;
    let mut prior_count = 0;
    let mut cap_requests_over_budget = 0;
    let mut wire_spans = Vec::new();
    let config = test.server.za_search_config();
    let start = Instant::now();
    let terminal = loop {
        assert!(
            start.elapsed() < PHASE_TIMEOUT,
            "serial watchdog, progress {previous:?}"
        );
        ticks.tick().await;
        let began = start.elapsed();
        let ordinal = serial.requests + 1;
        let elapsed = wire.try_get().await.unwrap_or_else(|error| {
            panic!(
                "{label} serial Email/get request {ordinal}, phase start offset {began:?}, last settled (scanned, checkpoint, indexed, coverage, rebuild cursor) {previous:?}: {error}"
            )
        });
        serial.requests += 1;
        serial.overruns += usize::from(elapsed > period);
        let committed = capture.drain();
        assert!(
            committed.len() <= 1,
            "one opportunistic get published multiple passes"
        );
        let p = progress(test, id, false)
            .await
            .expect("missing loaded map after keyed get");
        let mut tiers = Vec::new();
        if let Some(commit) = committed.first() {
            serial.pass_time += elapsed;
            if commit.thread_compaction {
                serial.thread.push((p.indexed, elapsed));
            }
            if commit.full_merge {
                serial.full.push((p.indexed, elapsed));
            } else if let Some(content) = &commit.content {
                tiers = content
                    .encoded
                    .iter()
                    .filter_map(|&(tier, _)| (tier != 0).then_some(tier))
                    .collect();
                if !tiers.is_empty() {
                    serial.tiered.push((p.indexed, elapsed, tiers.clone()));
                }
            }
            if p.indexed.saturating_sub(prior_count) >= config.pass_cap
                && elapsed > Duration::from_millis(config.pass_ms)
            {
                cap_requests_over_budget += 1;
            }
        }
        // Print only after the wire timer and the existing settled observations.
        // A later failed request must not erase the preceding pass evidence.
        println!(
            "Serial request {label} #{ordinal}: start {began:?}, wire {elapsed:?}, commits {}, thread compaction {}, full merge {}, promoted tiers {tiers:?}, rewritten bytes {}, indexed {} (+{}), Email state {}, scanned {}, epoch/revision {}/{}, checkpoint {:?}, coverage {:?}, cursor {:?}, sealed bytes {}, chain length {}",
            committed.len(),
            committed
                .first()
                .is_some_and(|commit| commit.thread_compaction),
            committed.first().is_some_and(|commit| commit.full_merge),
            committed
                .first()
                .and_then(|commit| commit.content.as_ref())
                .map_or(0, |content| content.rewritten()),
            p.indexed,
            p.indexed.saturating_sub(prior_count),
            p.email_state,
            p.scanned_to,
            p.manifest.epoch,
            p.manifest.revision,
            p.manifest.indexed_to,
            p.manifest.content_to,
            p.manifest.cursor,
            p.sealed(),
            p.manifest.content.as_ref().map_or(0, |head| head.length),
        );
        wire_spans.push((began, elapsed, committed.len(), p.indexed));
        prior_count = p.indexed;
        if previous_size <= cap && p.sealed() > cap {
            println!(
                "Natural cap crossing: previous sealed {previous_size}, now {}",
                p.sealed()
            );
            growth_line("cap transition", &p, sampler);
        }
        previous_size = p.sealed();
        if pending.peek().is_some_and(|&n| n <= p.indexed) {
            if loaded_growth && !p.manifest.content_to.suspended && p.sealed() <= cap {
                let keys = za_unlock(test, id, wire.account.secret()).await;
                let entry = test.server.inner.cache.za_search.get(id).unwrap();
                let loaded = timeout(
                    MILESTONE_LOAD_TIMEOUT,
                    test.server.za_search_load_content(&entry, &keys),
                )
                .await;
                match loaded {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => println!(
                        "Milestone partial loader refused: {}",
                        error.classification().as_str()
                    ),
                    Err(_) => panic!("milestone partial loader exceeded watchdog, {p:?}"),
                }
            }
            let observed = progress(test, id, true).await.unwrap();
            while pending.peek().is_some_and(|&n| n <= p.indexed) {
                growth_line(
                    &format!("growth milestone {}", pending.next().unwrap()),
                    &observed,
                    sampler,
                );
            }
            inventory(test, wire.account, &p.manifest).await;
        }
        if p.current || (p.map_complete() && (p.manifest.content_to.suspended || p.sealed() > cap))
        {
            break p;
        }
        let mark = (
            p.scanned_to,
            p.manifest.indexed_to.clone(),
            p.indexed,
            p.manifest.content_to.clone(),
            p.manifest.cursor.clone(),
        );
        stalls = if previous.as_ref() == Some(&mark) {
            stalls + 1
        } else {
            0
        };
        assert!(stalls < 10, "serial progress stalled: {p:?}");
        previous = Some(mark);
    };
    serial.wall = start.elapsed();
    let (accounting, traces) = capture.finish().await;
    let commits = accounting.commits.len();
    let inserted = traces
        .iter()
        .filter(|event| {
            event.account_id == Some(u64::from(id))
                && event.details == common::za::search::trace::ZA_SEARCH_PASS
        })
        .filter_map(|event| {
            event.keys.iter().find_map(|(k, v)| match (k, v) {
                (trc::Key::Total, trc::Value::UInt(n)) => Some(*n),
                _ => None,
            })
        })
        .sum::<u64>();
    println!(
        "Serial: {} requests, {commits} commits, {inserted} inserted, {} requests over cadence, {} without commits, {} abandoned content attempts, elapsed {:?}",
        serial.requests,
        serial.overruns,
        serial.requests - commits,
        accounting.aborted.len(),
        serial.wall
    );
    println!(
        "Serial get spans (start offset, duration, commits, indexed): {wire_spans:?}; {cap_requests_over_budget} cap-sized publishing requests exceeded configured pass milliseconds end-to-end"
    );
    assert_eq!(inserted, terminal.indexed as u64);
    (terminal, serial)
}

async fn report_drain(test: &TestServer, account: &Account, count: usize, label: &str) -> String {
    let sampler = Sampler::start(test);
    let (progress, metrics) = drain(test, &Wire::new(account), &sampler, &[], false, label).await;
    growth_line(label, &progress, &sampler);
    sampler.finish(label);
    assert!(
        progress.current,
        "small cohort not content-current: {progress:?}"
    );
    assert_eq!(progress.indexed, count);
    assert!(!metrics.pass_time.is_zero());
    format!(
        "{:.2} messages/s ({} timed committed requests' total; {} requests, {:?} wall)",
        count as f64 / metrics.pass_time.as_secs_f64(),
        format_ms(metrics.pass_time),
        metrics.requests,
        metrics.wall
    )
}

fn latency(label: &str, values: &mut [Duration]) -> String {
    assert!(!values.is_empty());
    values.sort_unstable();
    let middle = values.len() / 2;
    let median = if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2
    } else {
        values[middle]
    };
    let p99 = values[(values.len() * 99).div_ceil(100) - 1];
    format!(
        "{label} n={} median {}, p99 {}",
        values.len(),
        format_ms(median),
        format_ms(p99)
    )
}

async fn one_email(wire: &Wire<'_>, id: Id) -> Duration {
    let (elapsed, call) = wire
        .call("Email/get", json!({"ids": [id], "properties": ["id"]}))
        .await;
    assert!(
        reply("Email/get", &call)["list"] == json!([{"id": id}]),
        "validation Email/get did not return the requested ID alone"
    );
    elapsed
}

async fn validate_cost(test: &TestServer, keyed: &Wire<'_>, plain: &Wire<'_>) -> String {
    let one_id = |call: &Value| {
        reply("Email/get", call)["list"][0]["id"]
            .as_str()
            .unwrap()
            .parse::<Id>()
            .unwrap()
    };
    let (_, found) = keyed.search(BODY_WORD, 1).await;
    let keyed_id = ids(&reply("Email/query", &found)["ids"])[0];
    let (_, found) = plain.call("Email/get", json!({"properties": ["id"]})).await;
    let plain_id = one_id(&found);
    one_email(keyed, keyed_id).await;
    one_email(plain, plain_id).await;

    let id = uid(keyed.account);
    let capture = Capture::start(id);
    let mut key = Vec::with_capacity(500);
    let mut control = Vec::with_capacity(500);
    for i in 0..500 {
        if i % 2 == 0 {
            key.push(one_email(keyed, keyed_id).await);
            control.push(one_email(plain, plain_id).await);
        } else {
            control.push(one_email(plain, plain_id).await);
            key.push(one_email(keyed, keyed_id).await);
        }
    }
    let mut raw = Vec::with_capacity(500);
    for _ in 0..500 {
        let start = Instant::now();
        let result = test
            .server
            .store()
            .get_value::<RawValue>(ValueKey::property(
                id,
                Collection::Principal,
                0,
                PrincipalField::ZeroAccessSearch,
            ))
            .await;
        raw.push(start.elapsed());
        assert!(result.unwrap().is_some());
    }
    let (accounting, _) = capture.finish().await;
    assert!(accounting.commits.is_empty(), "validation sampled a pass");
    format!(
        "{}, {}, {}",
        latency("keyed", &mut key),
        latency("plain", &mut control),
        latency("raw field 152", &mut raw)
    )
}

async fn deliver_note(test: &TestServer, account: &Account, ordinal: usize, text: &str) -> u32 {
    let raw = MessageBuilder::new()
        .message_id(format!("search-note-{ordinal}@example.com"))
        .from("bench@remote.test")
        .to(account.name())
        .subject(format!("Benchmark note {ordinal}"))
        .text_body(text.to_string())
        .write_to_vec()
        .unwrap();
    deliver(
        test,
        account,
        &AccessToken::from_id_maybe_invalid(uid(account)),
        &MessageParser::new(),
        &raw,
    )
    .await
}

async fn mailbox_row(test: &TestServer, wire: &Wire<'_>, count: usize) -> String {
    let (_, call) = wire
        .call("Mailbox/get", json!({"properties": ["id"]}))
        .await;
    let existing = reply("Mailbox/get", &call)["list"]
        .as_array()
        .unwrap()
        .len();
    assert!(existing < count);
    let create = (existing..count)
        .map(|i| (i.to_string(), json!({"name": format!("bench-{i}")})))
        .collect::<serde_json::Map<_, _>>();
    let (_, call) = wire.call("Mailbox/set", json!({"create": create})).await;
    assert_eq!(
        reply("Mailbox/set", &call)["created"]
            .as_object()
            .unwrap()
            .len(),
        count - existing
    );
    let (_, call) = wire
        .call("Mailbox/get", json!({"properties": ["id"]}))
        .await;
    let before = reply("Mailbox/get", &call);
    let expected = before["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|mailbox| mailbox["id"].as_str().unwrap().parse::<Id>().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(expected.len(), count);

    deliver_note(
        test,
        wire.account,
        count,
        "One new message for a mailbox state witness.",
    )
    .await;
    let id = uid(wire.account);
    let mut capture = Capture::start(id);
    let publish = wire.get().await;
    assert_eq!(capture.drain().len(), 1);
    assert!(capture.accounting.commits[0].content.is_some());
    let (changes, call) = wire
        .call("Mailbox/changes", json!({"sinceState": before["state"]}))
        .await;
    let changed = reply("Mailbox/changes", &call);
    let updated = ids(&changed["updated"]).into_iter().collect::<HashSet<_>>();
    assert_eq!(updated, expected);
    assert!(
        changed["hasMoreChanges"] == false,
        "incomplete Mailbox/changes response"
    );
    capture.finish().await;
    format!(
        "{} updated, publishing get {}, Mailbox/changes {}",
        updated.len(),
        format_ms(publish),
        format_ms(changes)
    )
}

async fn first_search(test: &TestServer, wire: &Wire<'_>) -> String {
    // Settle the preceding mailbox-publication experiment before the new day's
    // delivery. The first content request after those deliveries is timed.
    let (settled, call) = wire.search(BODY_WORD, 1).await;
    assert!(
        !ids(&reply("Email/query", &call)["ids"]).is_empty(),
        "{settled:?}"
    );
    let last = deliver_cohort(
        test,
        wire.account,
        generated(SEED).skip(SMALL).take(200),
        200,
    )
    .await[199];
    let capture = Capture::start(uid(wire.account));
    let args = json!({"filter": {"text": BODY_WORD}, "limit": 200, "sort": [
        {"property": "receivedAt", "isAscending": false}
    ]});
    let (elapsed, call) = wire.call("Email/query", args.clone()).await;
    let (accounting, events) = capture.finish().await;
    if call[0] == "error" {
        assert_unavailable(&call);
        return format!(
            "first request refused in {} after {} committed passes, reasons {:?}",
            format_ms(elapsed),
            accounting.commits.len(),
            unavailable_reasons(uid(wire.account), &events)
        );
    }
    let mut found = ids(&reply("Email/query", &call)["ids"]);
    let mut args = args;
    // The body canary appears in every generated message and receive-time
    // ties are legal. Confirm the last document without changing the first timing.
    while !found.iter().any(|id| id.document_id() == last) && found.len() < SMALL + 200 {
        args["position"] = json!(found.len());
        let (_, next) = wire.call("Email/query", args.clone()).await;
        let next = ids(&reply("Email/query", &next)["ids"]);
        assert!(!next.is_empty(), "last delivery missing from text query");
        found.extend(next);
    }
    assert!(found.iter().any(|id| id.document_id() == last));
    format!(
        "{} after {} committed passes, last delivery matched",
        format_ms(elapsed),
        accounting.commits.len()
    )
}

fn unavailable_reasons(id: u32, events: &[crate::za::mail::TracedEvent]) -> BTreeMap<&str, usize> {
    let mut reasons = BTreeMap::new();
    for event in events.iter().filter(|event| {
        event.account_id == Some(u64::from(id)) && event.details == ZA_SEARCH_UNAVAILABLE
    }) {
        *reasons.entry(event.reason.as_str()).or_default() += 1;
    }
    reasons
}

fn assert_match(call: &Value, delivered: &[u32]) {
    let hits = ids(&reply("Email/query", call)["ids"]);
    assert!(
        hits.iter().any(|id| delivered.contains(&id.document_id())),
        "Email/query did not match a delivered document"
    );
}

async fn cold_serve(test: &TestServer, wire: &Wire<'_>, delivered: &[u32], label: &str) -> String {
    let id = uid(wire.account);
    let cache = &test.server.inner.cache.za_search;
    let sampler = Sampler::start(test);
    println!(
        "{label} before eviction: ledger content/thread {}/{}",
        cache.content_used(),
        cache.thread_used()
    );
    cache.remove_account(id, EvictReason::Invalidation);
    assert!(cache.get(id).is_none());
    assert_eq!(
        cache.content_used(),
        0,
        "another resident part or a retained reader"
    );
    assert_eq!(
        cache.thread_used(),
        0,
        "another resident part or a retained reader"
    );
    let capture = Capture::start(id);
    let before = za_search_blob_reads(id);
    let (elapsed, call) = wire.search(BODY_WORD, 1).await;
    let reads = za_search_blob_reads(id) - before;
    let (accounting, events) = capture.finish().await;
    assert!(
        accounting.commits.is_empty(),
        "cold serving advanced the supposedly complete durable index"
    );
    if call[0] == "error" {
        assert_unavailable(&call);
        sampler.finish(label);
        return format!(
            "refused {} after {reads} blob reads, reasons {:?}",
            format_ms(elapsed),
            unavailable_reasons(id, &events)
        );
    }
    assert_match(&call, delivered);
    assert!(reads > 0, "cold request must read sealed blobs");
    coverage(test, id, delivered).await;
    let p = progress(test, id, true).await.unwrap();
    assert!(p.current, "{p:?}");
    growth_line(label, &p, &sampler);
    let (warm, call) = wire.search(BODY_WORD, 1).await;
    assert_match(&call, delivered);
    sampler.finish(label);
    format!(
        "cold {}, warm {}, {reads} cold blob reads, {} covered",
        format_ms(elapsed),
        format_ms(warm),
        delivered.len()
    )
}

async fn rebuild(
    test: &TestServer,
    account: &Account,
    delivered: &[u32],
    old: &Manifest,
) -> String {
    let id = uid(account);
    let config = test.server.za_search_config();
    let gets = Wire::new(account);
    let queries = Wire::new(account);
    let sampler = Sampler::start(test);
    let capture = Capture::start(id);
    write_manifest_bytes(test, id, None).await;
    assert!(manifest_bytes(test, id).await.is_none());
    let start = Instant::now();
    let deadline = tokio::time::Instant::now() + PHASE_TIMEOUT;
    let cursor_done = Cell::new(false);
    let get_loop = async {
        let mut ticks = intervals(Duration::from_secs(2));
        let mut samples = Vec::new();
        let mut saw_cursor = false;
        let mut last_progress = Instant::now();
        let mut previous = None;
        let mut busy_samples = 0;
        loop {
            assert!(
                last_progress.elapsed() < Duration::from_secs(180),
                "H7 stalled at durable progress {previous:?}"
            );
            ticks.tick().await;
            let began = start.elapsed();
            let elapsed = gets.get().await;
            samples.push((began, elapsed));
            let manifest = match read_manifest(test, id).await {
                Some(ManifestValue::Index(manifest)) if manifest.epoch != old.epoch => manifest,
                _ => continue,
            };
            saw_cursor |= manifest.cursor.is_some();
            if previous.as_ref() != Some(&manifest) {
                last_progress = Instant::now();
            }
            if let Some(p) = progress(test, id, true).await {
                if samples.len().is_multiple_of(25) {
                    growth_line("H7 passive in-progress sample", &p, &sampler);
                }
            } else {
                busy_samples += 1;
            }
            if manifest.cursor.is_none()
                && manifest.content_to.cursor.is_none()
                && manifest.reconciliation.is_none()
            {
                cursor_done.set(true);
                return (start.elapsed(), samples, saw_cursor, busy_samples);
            }
            previous = Some(manifest);
        }
    };
    let query_loop = async {
        let mut ticks = intervals(Duration::from_secs(5));
        let mut samples = Vec::new();
        let mut post_cursor_refusals = 0;
        loop {
            ticks.tick().await;
            let began = start.elapsed();
            let (elapsed, call) = queries.search(BODY_WORD, 1).await;
            let served = call[0] == "Email/query";
            samples.push((began, elapsed, served));
            if served {
                assert_match(&call, delivered);
                return (Some(start.elapsed()), samples);
            }
            assert_unavailable(&call);
            if cursor_done.get() {
                post_cursor_refusals += 1;
                if post_cursor_refusals == 3 {
                    return (None, samples);
                }
            }
        }
    };
    let ((cursor_time, get_samples, saw_cursor, busy_samples), (served_time, query_samples)) =
        timeout_at(deadline, async { tokio::join!(get_loop, query_loop) })
            .await
            .expect("H7 rebuild watchdog expired");
    let (accounting, traces) = capture.finish().await;
    assert!(
        saw_cursor
            || traces
                .iter()
                .any(|event| event.account_id == Some(u64::from(id))
                    && event.details == ZA_SEARCH_REBUILD),
        "no observed rebuild cursor or boundary trace"
    );
    let manifest = disk_manifest(test, id).await;
    assert_ne!(manifest.epoch, old.epoch);
    let progress = progress(test, id, true).await.unwrap();
    if served_time.is_some() {
        assert!(progress.current, "{progress:?}");
        coverage(test, id, delivered).await;
    }
    assert_eq!(&progress.manifest, &manifest);
    assert_eq!(progress.indexed, delivered.len());
    let hidden_class = inventory(test, account, &manifest).await;
    growth_line("H7 settled after both loops", &progress, &sampler);
    sampler.finish("H7 rebuild and serving attempts");

    let mut tiered = BTreeMap::<u8, (usize, u64)>::new();
    let mut original = 0;
    let mut tiered_bytes = 0;
    let mut tiered_original = 0;
    let mut uploaded = 0;
    let mut linked = 0;
    let mut full = 0;
    let mut full_count = 0;
    let mut content_writes = 0;
    let mut max_chain = 0;
    let mut thread = 0;
    for commit in &accounting.commits {
        max_chain = max_chain.max(commit.chain_length);
        thread += usize::from(commit.thread_compaction);
        if let Some(content) = &commit.content {
            content_writes += 1;
            original += content.original;
            uploaded += content.uploaded.unwrap().1;
            linked += content.linked.unwrap().1;
            if commit.full_merge {
                full += content.rewritten();
                full_count += 1;
            } else {
                tiered_original += content.original;
                for &(tier, bytes) in content.encoded.iter().filter(|(tier, _)| *tier != 0) {
                    let entry = tiered.entry(tier).or_default();
                    entry.0 += 1;
                    entry.1 += bytes;
                    tiered_bytes += bytes;
                }
            }
        }
    }
    let mut exponent = 0;
    let mut capacity = 1usize;
    while capacity < content_writes {
        exponent += 1;
        capacity = capacity.checked_mul(config.segments).unwrap();
    }
    let bound = (config.segments - 1) * exponent + 1;
    let abort_upload = accounting
        .aborted
        .iter()
        .filter_map(|c| c.uploaded.map(|(_, size)| size))
        .sum::<u64>();
    let refusals = query_samples.len() - usize::from(served_time.is_some());
    let get_overruns = get_samples
        .iter()
        .filter(|(_, elapsed)| *elapsed > Duration::from_secs(2))
        .count();
    let query_overruns = query_samples
        .iter()
        .filter(|(_, elapsed, _)| *elapsed > Duration::from_secs(5))
        .count();
    println!(
        "H7 wire spans (start offset, duration): get {get_samples:?}, query (with success) {query_samples:?}"
    );
    println!(
        "H7 refusal reasons {:?}; {busy_samples} busy or absent progress samples; attempted but unlinked content uploads {abort_upload} bytes",
        unavailable_reasons(id, &traces)
    );
    println!(
        "H7 cursor {:?}, serve {:?}, {} gets ({} over cadence), {} queries ({} over cadence), {refusals} refusals; {} commits, {content_writes} content writes, {thread} thread compactions; tiered {tiered:?}, {full_count} full merges; byte totals original {original}, pre-full original {tiered_original}, tiered {tiered_bytes}, full {full}, uploaded {uploaded}, linked {linked}; max chain {max_chain}, derived bound {bound}; thread estimate {} over oldest retained hidden class {:?}",
        cursor_time,
        served_time,
        get_samples.len(),
        get_overruns,
        query_samples.len(),
        query_overruns,
        accounting.commits.len(),
        progress.thread_estimate,
        hidden_class
    );
    assert!(original > 0);
    assert!(
        tiered_bytes <= 3 * tiered_original,
        "R32 rewrite bound: {tiered_bytes} > 3 × {tiered_original}"
    );
    assert!(tiered_bytes <= (exponent as u64) * tiered_original);
    assert!(max_chain as usize <= 46 && max_chain as usize <= bound);
    assert_eq!(uploaded, linked);
    let serving = served_time.map_or_else(
        || "unavailable after three post-cursor refusals".to_string(),
        |elapsed| format!("{elapsed:?} to serve"),
    );
    format!(
        "{:?} to cursor exhaustion, {serving}; {} commits, {refusals} refusals, {thread} thread compactions, max chain {max_chain}; tiered {:.3}× original, full {full} B",
        cursor_time,
        accounting.commits.len(),
        tiered_bytes as f64 / tiered_original as f64
    )
}

async fn d11_changes(test: &TestServer, wire: &Wire<'_>) -> String {
    let (_, email) = wire.call("Email/get", json!({"ids": []})).await;
    let (_, thread) = wire.call("Thread/get", json!({"ids": []})).await;
    let email = reply("Email/get", &email)["state"].clone();
    let thread = reply("Thread/get", &thread)["state"].clone();
    let document = deliver_note(
        test,
        wire.account,
        DESIGN,
        "One new item beyond the pinned state.",
    )
    .await;
    wire.get().await;
    let (email_elapsed, call) = wire
        .call("Email/changes", json!({"sinceState": email}))
        .await;
    let email = reply("Email/changes", &call);
    assert!(
        ids(&email["created"])
            .iter()
            .any(|id| id.document_id() == document)
    );
    assert!(
        email["hasMoreChanges"] == false,
        "incomplete Email/changes response"
    );
    let (thread_elapsed, call) = wire
        .call("Thread/changes", json!({"sinceState": thread}))
        .await;
    let thread = reply("Thread/changes", &call);
    assert!(!ids(&thread["created"]).is_empty());
    assert!(
        thread["hasMoreChanges"] == false,
        "incomplete Thread/changes response"
    );
    format!(
        "Email/changes {}, Thread/changes {}; {} and {} creations, end-to-end request time",
        format_ms(email_elapsed),
        format_ms(thread_elapsed),
        ids(&email["created"]).len(),
        ids(&thread["created"]).len()
    )
}

async fn mapped_documents(wire: &Wire<'_>, documents: &[u32]) -> Vec<Id> {
    let id = uid(wire.account);
    // The real Email/get maps the stored document IDs at the wire boundary.
    // This helper is used only on the small controlled cap fixture.
    let (_, call) = wire
        .call("Email/get", json!({"properties": ["id", "threadId"]}))
        .await;
    let body = reply("Email/get", &call);
    let mut mapped: Vec<_> = body["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().parse::<Id>().unwrap())
        .collect();
    mapped.sort_unstable_by_key(Id::document_id);
    assert_eq!(
        mapped.iter().map(Id::document_id).collect::<HashSet<_>>(),
        documents.iter().copied().collect::<HashSet<_>>(),
        "map service on {id}"
    );
    mapped
}

fn cap_body(ordinal: usize) -> String {
    let mut body = String::with_capacity(300_000);
    body.push_str("capcontrolneedle\r\n");
    for n in 0..16_000 {
        let mut number = ordinal * 16_000 + n;
        let mut word = [b'a'; 6];
        for byte in &mut word {
            *byte += (number % 26) as u8;
            number /= 26;
        }
        write!(body, " term{}recover", std::str::from_utf8(&word).unwrap()).unwrap();
        if n % 16 == 15 {
            body.push_str("\r\n");
        }
    }
    body
}

async fn cap_control(test: &TestServer, admin: &Account) -> String {
    let (account, _) = key(test, admin, CAP_CONTROL).await;
    let wire = Wire::new(&account);
    let id = uid(&account);
    let cap = crate::utils::za::search_config_guard!(test, account_max_mb: 1, segments: 255);
    let mut delivered = Vec::new();
    let mut under = 0;
    // Load a small first part so the crossing initially has a warm index.
    delivered.push(deliver_note(test, &account, 0, "capcontrolneedle").await);
    let (_, call) = wire.search("capcontrolneedle", 1).await;
    assert_match(&call, &delivered);
    let crossed = loop {
        assert!(delivered.len() <= 15, "controlled cap never crossed");
        let n = delivered.len();
        delivered.push(deliver_note(test, &account, n, &cap_body(n)).await);
        wire.get().await;
        let manifest = disk_manifest(test, id).await;
        let size = manifest.content.as_ref().unwrap().size;
        if size > MIB {
            break manifest;
        }
        under = size;
    };
    let warm_present = progress(test, id, true).await.unwrap().content.is_some();
    let refusals = Capture::start(id);
    let (warm_elapsed, call) = wire.search("capcontrolneedle", 1).await;
    assert_unavailable(&call);
    let cache = &test.server.inner.cache.za_search;
    cache.remove_account(id, EvictReason::Invalidation);
    assert!(cache.get(id).is_none());
    let (cold_elapsed, call) = wire.search("capcontrolneedle", 1).await;
    assert_unavailable(&call);
    let (_, events) = refusals.finish().await;
    assert_eq!(unavailable_reasons(id, &events).get("over_cap"), Some(&2));

    let n = delivered.len();
    delivered.push(deliver_note(test, &account, n, "capcontrolneedle").await);
    wire.get().await;
    let suspended = disk_manifest(test, id).await;
    assert!(suspended.content_to.suspended);
    assert_eq!(suspended.content, crossed.content);
    assert_eq!(suspended.epoch, crossed.epoch);
    let mapped = mapped_documents(&wire, &delivered).await;
    let stable = disk_manifest(test, id).await;
    assert!(stable.content_to.suspended);
    assert_eq!(stable.content, suspended.content);
    inventory(test, &account, &stable).await;
    let sealed = stable.content.as_ref().unwrap().size;

    drop(cap);
    let _same_threshold = crate::utils::za::search_config_guard!(test, segments: 255);
    assert_eq!(test.server.za_search_config().account_max_mb, 512);
    let (_, call) = wire.search("capcontrolneedle", 1).await;
    assert_match(&call, &delivered);
    coverage(test, id, &delivered).await;
    let recovered = disk_manifest(test, id).await;
    assert_eq!(recovered.epoch, stable.epoch);
    assert!(!recovered.content_to.suspended);
    assert!(recovered.content_to.cursor.is_none());
    assert_eq!(mapped_documents(&wire, &delivered).await, mapped);
    inventory(test, &account, &recovered).await;
    admin.destroy_account(account).await;
    format!(
        "1 MiB semantic control crossed from {under} to {sealed} B; crossing resident content {warm_present}, warm/cold refusals {}/{}; map and epoch survived cap raise",
        format_ms(warm_elapsed),
        format_ms(cold_elapsed)
    )
}

async fn natural_cap(test: &TestServer, wire: &Wire<'_>, original: &Progress) -> String {
    let id = uid(wire.account);
    let before = disk_manifest(test, id).await;
    assert!(
        before.content.as_ref().unwrap().size > test.server.za_search_config().account_max_mb * MIB
    );
    let capture = Capture::start(id);
    let (_, call) = wire.search(BODY_WORD, 1).await;
    assert_unavailable(&call);
    let cache = &test.server.inner.cache.za_search;
    cache.remove_account(id, EvictReason::Invalidation);
    assert!(cache.get(id).is_none());
    let (_, call) = wire.search(BODY_WORD, 1).await;
    assert_unavailable(&call);
    let new_doc = deliver_note(test, wire.account, DESIGN, BODY_WORD).await;
    wire.get().await;
    let mapped = {
        let entry = cache.get(id).unwrap();
        let guard = entry.index.read().await;
        let thread = &guard.as_ref().unwrap().thread;
        assert!(thread.record(new_doc).is_some());
        Id::from_parts(thread.current_thread(new_doc), new_doc)
    };
    one_email(wire, mapped).await;
    let after = disk_manifest(test, id).await;
    assert!(after.content_to.suspended);
    assert_eq!(after.content, before.content);
    assert_eq!(after.epoch, before.epoch);
    inventory(test, wire.account, &after).await;
    let (_, events) = capture.finish().await;
    assert_eq!(unavailable_reasons(id, &events).get("over_cap"), Some(&2));
    format!(
        "natural cap crossing at {} sealed bytes; indexed map {}, stable suspension, warm-map/cold exact refusal, content resident before eviction {}, map served; full design-size content coverage unavailable",
        original.sealed(),
        original.indexed,
        original.content.is_some()
    )
}

pub(super) async fn run() {
    timeout(SYSTEM_TIMEOUT, run_system())
        .await
        .expect("system benchmark watchdog expired");
}

async fn run_system() {
    let test = server().await;
    let admin = test.create_admin_account("admin@example.com").await;
    let config = test.server.za_search_config();
    assert_eq!(
        config,
        ZaSearchConfig::default(),
        "system benchmark requires defaults"
    );
    assert_eq!(test.server.inner.cache.za_search.config(), &config);
    println!(
        "Benchmark PID {}, CPU architecture {}, boot/effective search config {config:?}",
        std::process::id(),
        std::env::consts::ARCH
    );
    println!(
        "Benchmark watchdogs: client wire {WIRE_TIMEOUT:?}, milestone loader {MILESTONE_LOAD_TIMEOUT:?}, phase {PHASE_TIMEOUT:?}, whole test {SYSTEM_TIMEOUT:?}"
    );
    let mut rows = Vec::new();
    let mut row = |(name, value): (&'static str, String)| {
        println!("Completed measurement | {name} | {value} |");
        rows.push((name, value));
    };

    let (account, setup) = key(&test, &admin, ACCOUNT).await;
    let first = uid(&account);
    let delivered = deliver_cohort(
        &test,
        &account,
        generated(SEED)
            .filter(|g| g.attachment_bytes == 0)
            .take(SMALL),
        SMALL,
    )
    .await;
    assert_pristine_setup(&test, first, setup).await;
    assert_mailbox(&test, first, &delivered).await;
    row((
        "Attachment-free pass rate",
        report_drain(&test, &account, SMALL, "attachment-free").await,
    ));
    admin.destroy_account(account).await;

    let (account, setup) = key(&test, &admin, ACCOUNT).await;
    assert_ne!(uid(&account), first);
    let delivered = deliver_cohort(&test, &account, generated(SEED).take(SMALL), SMALL).await;
    assert_pristine_setup(&test, uid(&account), setup).await;
    assert_mailbox(&test, uid(&account), &delivered).await;
    row((
        "Mixed pass rate",
        report_drain(&test, &account, SMALL, "mixed").await,
    ));
    let plain = admin
        .create_user_account(PLAIN, PASSWORD, "Search benchmark control", &[], vec![])
        .await;
    deliver_note(&test, &plain, 0, BODY_WORD).await;
    let wire = Wire::new(&account);
    row((
        "Validation read cost",
        validate_cost(&test, &wire, &Wire::new(&plain)).await,
    ));
    row(("10 mailbox properties", mailbox_row(&test, &wire, 10).await));
    row((
        "100 mailbox properties",
        mailbox_row(&test, &wire, 100).await,
    ));
    row((
        "First search after 200 deliveries",
        first_search(&test, &wire).await,
    ));
    let second = uid(&account);
    admin.destroy_account(plain).await;
    admin.destroy_account(account).await;

    let (account, setup) = key(&test, &admin, ACCOUNT).await;
    assert_ne!(uid(&account), second);
    let wire = Wire::new(&account);
    let sampler = Sampler::start(&test);
    let delivered = deliver_cohort(&test, &account, generated(SEED).take(DESIGN), DESIGN).await;
    sampler.finish("design delivery (pre-index)");
    let id = uid(&account);
    assert_pristine_setup(&test, id, setup).await;
    assert_mailbox(&test, id, &delivered).await;
    let sampler = Sampler::start(&test);
    let (indexed, serial) = drain(
        &test,
        &wire,
        &sampler,
        &GROWTH,
        false,
        "design normal indexing",
    )
    .await;
    sampler.finish("design-size default serial indexing");
    assert_eq!(indexed.indexed, DESIGN);
    let manifest = disk_manifest(&test, id).await;
    assert_eq!(indexed.manifest, manifest);
    let hidden = inventory(&test, &account, &manifest).await;
    println!(
        "Design thread estimate {}, total sealed thread bytes {}, oldest retained hidden class {hidden:?}",
        indexed.thread_estimate,
        manifest
            .thread_blobs
            .iter()
            .map(|e| u64::from(e.size))
            .sum::<u64>()
    );
    let last = |samples: &[(usize, Duration)]| {
        samples.last().map_or_else(
            || "none observed".to_string(),
            |&(count, elapsed)| {
                format!(
                    "{} at {count} documents ({} samples)",
                    format_ms(elapsed),
                    samples.len()
                )
            },
        )
    };
    row(("Thread compaction get", last(&serial.thread)));
    row(("Full content compaction get", last(&serial.full)));

    if indexed.current && indexed.sealed() <= config.account_max_mb * MIB {
        row((
            "Default sealed cap",
            format!(
                "{} B, {} B headroom; 512 MiB crossing not reached",
                indexed.sealed(),
                config.account_max_mb * MIB - indexed.sealed()
            ),
        ));
        let cold = cold_serve(&test, &wire, &delivered, "default cold both").await;
        row(("30,000-item cold wire serving", cold));
        row((
            "H7/R32 rebuild, tiered content compactions",
            rebuild(&test, &account, &delivered, &manifest).await,
        ));
        row((
            "D11 retained-state changes",
            d11_changes(&test, &wire).await,
        ));
    } else {
        row((
            "Default sealed cap",
            natural_cap(&test, &wire, &indexed).await,
        ));
        row((
            "H7/R32 rebuild",
            "pending after default-cap suspension".into(),
        ));
    }
    admin.destroy_account(account).await;

    let (account, setup) = key(&test, &admin, ACCOUNT).await;
    let wire = Wire::new(&account);
    let delivered = deliver_cohort(&test, &account, generated(SEED).take(DESIGN), DESIGN).await;
    let id = uid(&account);
    assert_pristine_setup(&test, id, setup).await;
    assert_mailbox(&test, id, &delivered).await;
    // Keep normal-log growth and concurrent H7 separate. This cohort
    // deliberately enters missing-manifest recovery before any keyed GET,
    // and loads the actual partial content only at settled milestones.
    write_manifest_bytes(&test, id, None).await;
    assert!(manifest_bytes(&test, id).await.is_none());
    let sampler = Sampler::start(&test);
    let (growth, serial) = drain(
        &test,
        &wire,
        &sampler,
        &GROWTH,
        true,
        "serial rebuild with milestone warming",
    )
    .await;
    assert_eq!(growth.indexed, DESIGN);
    sampler.finish("serial absent-manifest rebuild with milestone warming");
    let tiered = serial.tiered.last().map_or_else(
        || "none observed in the serial absent-manifest rebuild".to_string(),
        |(count, elapsed, tiers)| format!(
            "{} at {count} documents, promoted tiers {tiers:?} ({} serial absent-manifest rebuild samples with milestone warming)",
            format_ms(*elapsed), serial.tiered.len()
        ),
    );
    row(("Tiered content compaction get", tiered));
    admin.destroy_account(account).await;
    row(("Controlled cap lifecycle", cap_control(&test, &admin).await));

    println!("| Figure | Observed |\n|---|---|");
    for (name, value) in rows {
        println!("| {name} | {value} |");
    }
    test.shutdown();
    test.temp_dir.delete();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn observer_control() {
    use common::za::test_hooks::set_search_hold_expire;

    struct ExpiredHold;
    impl Drop for ExpiredHold {
        fn drop(&mut self) {
            set_search_hold_expire(None);
        }
    }

    let test = server().await;
    let admin = test.create_admin_account("admin@example.com").await;
    let (account, setup) = key(&test, &admin, ACCOUNT).await;
    let id = uid(&account);
    let wire = Wire::new(&account);
    let _config =
        crate::utils::za::search_config_guard!(&test, segments: 2, pass_cap: 1, checkpoint: 1);
    let delivered = deliver_cohort(&test, &account, generated(SEED).take(5), 5).await;
    assert_pristine_setup(&test, id, setup).await;
    assert_mailbox(&test, id, &delivered).await;
    let sampler = Sampler::start(&test);
    let (complete, metrics) = drain(
        &test,
        &wire,
        &sampler,
        &[2, 5],
        false,
        "small harness control",
    )
    .await;
    sampler.finish("small harness control");
    assert!(complete.current);
    assert!(!metrics.thread.is_empty() && !metrics.full.is_empty());
    assert!(
        metrics.tiered.is_empty(),
        "normal setup has no rebuild cursor"
    );
    assert_eq!(complete.indexed, 5);
    let cold = cold_serve(&test, &wire, &delivered, "small cold harness control").await;
    assert!(!cold.starts_with("refused"), "{cold}");

    let cache = &test.server.inner.cache.za_search;
    {
        let _budget = cache.override_content_budget_for_test(1);
        cache.remove_account(id, EvictReason::Invalidation);
        assert!(cache.get(id).is_none());
        let capture = Capture::start(id);
        let (_, call) = wire.search(BODY_WORD, 1).await;
        assert_unavailable(&call);
        let (accounting, events) = capture.finish().await;
        assert!(accounting.commits.is_empty());
        assert_eq!(unavailable_reasons(id, &events).get("budget"), Some(&1));
    }
    let (_, call) = wire.search(BODY_WORD, 1).await;
    assert_match(&call, &delivered);
    coverage(&test, id, &delivered).await;

    write_manifest_bytes(&test, id, None).await;
    assert!(manifest_bytes(&test, id).await.is_none());
    let mut capture = Capture::start(id);
    let mut saw_cursor = false;
    let mut saw_tiered = false;
    let mut saw_full = false;
    let mut saw_thread = false;
    let mut saw_noncontent = false;
    let mut previous: Option<Manifest> = None;
    for step in 0..16 {
        let expiry = (step == 1).then(|| {
            // Deliberately let the real hold expire before the asserted write.
            set_search_hold_expire(Some(id));
            ExpiredHold
        });
        wire.get().await;
        drop(expiry);
        let next = disk_manifest(&test, id).await;
        assert_ne!(next.epoch, complete.manifest.epoch);
        saw_cursor |= next.cursor.is_some();
        for commit in capture.drain() {
            assert_eq!(
                commit.chain_length,
                next.content.as_ref().map_or(0, |h| h.length)
            );
            if let Some(base) = &previous {
                let replacement = next.thread_blobs.len() == 1
                    && !base.thread_blobs.is_empty()
                    && base.thread_blobs[0] != next.thread_blobs[0];
                assert_eq!(commit.thread_compaction, replacement);
                saw_thread |= replacement;
            }
            if let Some(content) = &commit.content {
                let head = next.content.as_ref().unwrap();
                let bytes = prefix(&test, &head.hash, BlobHeader::LEN).await;
                let (header, _) = BlobHeader::decode(&bytes).unwrap();
                let links = principal_blob_links(&test, id).await;
                let (_, size) = links.iter().find(|(hash, _)| hash == &head.hash).unwrap();
                assert_eq!(content.linked, Some((header.tier, *size as u64)));
                assert_eq!(
                    commit.full_merge,
                    content.rewritten() > 0 && next.cursor.is_none()
                );
                saw_full |= commit.full_merge;
                saw_tiered |= content.rewritten() > 0 && !commit.full_merge;
            } else {
                saw_noncontent = true;
            }
        }
        previous = Some(next);
        if progress(&test, id, false).await.unwrap().current {
            break;
        }
    }
    let (accounting, _) = capture.finish().await;
    assert!(
        saw_cursor && saw_tiered && saw_full && saw_thread && saw_noncontent,
        "observer coverage cursor={saw_cursor} tiered={saw_tiered} full={saw_full} thread={saw_thread} noncontent={saw_noncontent}"
    );
    assert!(
        accounting
            .aborted
            .iter()
            .any(|attempt| attempt.uploaded.is_some())
    );
    let manifest = disk_manifest(&test, id).await;
    inventory(&test, &account, &manifest).await;
    let (_, call) = wire.search(BODY_WORD, 1).await;
    assert_match(&call, &delivered);
    coverage(&test, id, &delivered).await;
    admin.destroy_account(account).await;
    test.shutdown();
    test.temp_dir.delete();
}

#[test]
fn lazy_corpus_matches_collected_prefix() {
    // Exercise attachment reshuffling and reference resets across both block
    // boundaries. Filtering the stream must retain each original MIME item.
    let corpus = super::corpus(SEED, 1_002);
    // Fingerprint of the accepted ea67be7f3 generator, obtained independently.
    // Frame each record as three u64 LE lengths/counts followed by exact MIME.
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for message in &corpus {
        digest.update((message.raw.len() as u64).to_le_bytes());
        digest.update((message.attachment_bytes as u64).to_le_bytes());
        digest.update((message.references as u64).to_le_bytes());
        digest.update(&message.raw);
    }
    let accepted = [
        0xdd, 0xc9, 0x09, 0x53, 0x9e, 0x36, 0x66, 0x8b, 0xac, 0x22, 0xd5, 0xa7, 0x05, 0x39, 0x81,
        0x55, 0x7b, 0x18, 0xce, 0x88, 0x9a, 0x5c, 0x8d, 0x25, 0x4f, 0xd1, 0xe2, 0xf7, 0x82, 0xbc,
        0x6b, 0x7b,
    ];
    assert_eq!(&digest.finalize()[..], &accepted[..]);

    let mut lazy = generated(SEED).filter(|message| message.attachment_bytes == 0);
    assert_eq!(
        corpus[..1_000]
            .iter()
            .filter(|m| m.attachment_bytes != 0)
            .count(),
        300
    );
    assert!(corpus.iter().any(|m| m.references > super::K));
    for collected in corpus
        .iter()
        .filter(|message| message.attachment_bytes == 0)
    {
        let streamed = lazy.next().unwrap();
        assert_eq!(streamed.raw, collected.raw);
        assert_eq!(streamed.references, collected.references);
    }
}
