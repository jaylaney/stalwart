/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    ACCOUNT, BODY_WORD, DESIGN, SEED, Sampler, WIRE_TIMEOUT, Wire, assert_mailbox,
    assert_pristine_setup, deliver_cohort, drain, generated, inventory, manifest_bytes,
    server_named, uid, za_unlock,
};
use crate::utils::server::TestServer;
use common::za::{
    ZA_MAIL_SEARCH_UNAVAILABLE,
    search::{
        ZaSearchConfig,
        cache::{EvictReason, LoadedIndex},
        seal::za_search_key,
    },
    test_hooks::{SearchCommitCounter, SearchRequestObservation, SearchStage},
};
use email::cache::MessageCacheFetch;
use jmap::{changes::state::JmapCacheState, email::za_search::za_content_current};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    future::Future,
    time::{Duration, Instant},
};
use store::za_search::manifest::{Manifest, ManifestValue, manifest_hash};
use types::{collection::SyncCollection, id::Id};

struct Target {
    account_id: u32,
    email_state: u64,
    manifest: Manifest,
    manifest_hash: [u8; 32],
    expected_id: Id,
    query_state: String,
}

impl Target {
    fn exact_manifest(&self, index: &LoadedIndex) -> bool {
        index.manifest_bytes_hash == self.manifest_hash
    }

    fn exact_thread(&self, index: &LoadedIndex) -> bool {
        index.thread_segments.iter().copied().eq(self
            .manifest
            .thread_blobs
            .iter()
            .map(|blob| blob.segment))
    }

    fn exact_content(&self, index: &LoadedIndex) -> bool {
        let expected = self.manifest.content.as_ref();
        index
            .content_for_test()
            .is_some_and(|(current, head, part)| {
                current
                    && head == expected
                    && part.segments.first() == expected.map(|head| &head.segment)
            })
    }
}

struct QueryAttempt {
    observation: SearchRequestObservation,
    elapsed: Duration,
    result: Result<(Duration, Value), String>,
}

impl QueryAttempt {
    fn matches(&self, target: &Target) -> bool {
        self.result.as_ref().is_ok_and(|(_, call)| {
            call[0] == "Email/query"
                && call[1]["accountId"] == json!(Id::from(target.account_id))
                && call[1]["position"] == 0
                && call[1]["ids"] == json!([target.expected_id])
                && call[1]["queryState"] == target.query_state
        })
    }

    fn report(&self, label: &str, target: &Target) {
        let outcome = match &self.result {
            Ok((wire, call)) if call[0] == "Email/query" => format!(
                "response, wire {wire:?}, ids {:?}, expected response {}",
                call[1]["ids"].as_array().map(Vec::len),
                self.matches(target),
            ),
            Ok((wire, call)) => {
                let expected_refusal = call[0] == "error"
                    && call[1]["type"] == "unsupportedFilter"
                    && call[1]["description"] == format!("text: {ZA_MAIL_SEARCH_UNAVAILABLE}");
                format!("method error, wire {wire:?}, expected refusal {expected_refusal}")
            }
            Err(error) => error.clone(),
        };
        println!(
            "Cold-start {label}: {outcome}, elapsed {:?}, measurement claimed {}, admission {:?}",
            self.elapsed,
            self.observation.claimed(),
            self.observation
                .admission()
                .map(|admission| (admission.reason, admission.remaining)),
        );
        for (stage, count, elapsed) in self.observation.timings() {
            println!("Cold-start {label} stage {stage:?}: {count}, {elapsed:?}");
        }
    }
}

async fn query(wire: &Wire<'_>, account_id: u32) -> QueryAttempt {
    let observation = SearchRequestObservation::arm(account_id);
    let started = Instant::now();
    let result = wire
        .try_call_with_id(
            "Email/query",
            json!({"filter": {"text": BODY_WORD}, "limit": 1}),
            observation.call_id(),
        )
        .await;
    let elapsed = started.elapsed();
    observation.disarm();
    QueryAttempt {
        observation,
        elapsed,
        result,
    }
}

async fn measure_pair<Q, F, S>(
    mut query: Q,
    snapshot: impl FnOnce() -> S,
) -> (F::Output, S, F::Output)
where
    Q: FnMut() -> F,
    F: Future,
{
    let first = query().await;
    let between = snapshot();
    let repeat = query().await;
    (first, between, repeat)
}

#[derive(Serialize)]
#[serde(tag = "map", rename_all = "snake_case")]
enum Residency {
    TableBusy,
    Absent,
    IndexBusy,
    Unloaded,
    Loaded {
        exact_manifest: bool,
        exact_thread: bool,
        exact_content: bool,
        content: &'static str,
        content_count: usize,
        content_bytes: u64,
        thread_records: usize,
        thread_bytes: u64,
        epoch: u64,
        revision: u32,
        scanned_to: u64,
        checkpoint: u64,
        content_checkpoint: u64,
        current: bool,
    },
}

#[derive(Serialize)]
struct Snapshot {
    elapsed: Duration,
    residency: Residency,
    content_charge: u64,
    thread_charge: u64,
    commits: u64,
}

fn residency(test: &TestServer, target: &Target) -> Residency {
    let cache = &test.server.inner.cache.za_search;
    let Some(entry) = cache.try_get_for_test(target.account_id) else {
        return Residency::TableBusy;
    };
    let Some(entry) = entry else {
        return Residency::Absent;
    };
    let Ok(guard) = entry.index.try_read() else {
        return Residency::IndexBusy;
    };
    let Some(index) = guard.as_ref() else {
        return Residency::Unloaded;
    };
    let (content, content_count, content_bytes) =
        index
            .content_for_test()
            .map_or(("absent", 0, 0), |(current, _, part)| {
                (
                    if current { "current" } else { "deferred" },
                    part.part.len(),
                    part.bytes,
                )
            });
    Residency::Loaded {
        exact_manifest: target.exact_manifest(index),
        exact_thread: target.exact_thread(index),
        exact_content: target.exact_content(index),
        content,
        content_count,
        content_bytes,
        thread_records: index.thread.records().size_hint().0,
        thread_bytes: index.thread_bytes,
        epoch: index.manifest.epoch,
        revision: index.manifest.revision,
        scanned_to: index.scanned_to,
        checkpoint: index.manifest.indexed_to.change_id,
        content_checkpoint: index.manifest.content_to.checkpoint.change_id,
        current: za_content_current(index, target.email_state),
    }
}

fn snapshot(test: &TestServer, target: &Target, counter: &SearchCommitCounter) -> Snapshot {
    let start = Instant::now();
    let cache = &test.server.inner.cache.za_search;
    let residency = residency(test, target);
    let content_charge = cache.content_used();
    let thread_charge = cache.thread_used();
    let commits = counter.count();
    Snapshot {
        elapsed: start.elapsed(),
        residency,
        content_charge,
        thread_charge,
        commits,
    }
}

#[derive(Serialize)]
struct Census {
    content_count: usize,
    missing_content: usize,
    live_thread_count: usize,
    missing_thread: usize,
    exact_manifest: bool,
    exact_content: bool,
    exact_thread: bool,
    current: bool,
}

fn census(test: &TestServer, target: &Target, delivered: &[u32]) -> Option<Census> {
    let entry = test
        .server
        .inner
        .cache
        .za_search
        .try_get_for_test(target.account_id)??;
    let guard = entry.index.try_read().ok()?;
    let index = guard.as_ref()?;
    let content = index.content.as_ref().map(|part| &part.part);
    Some(Census {
        content_count: content.map_or(0, |part| part.len()),
        missing_content: delivered
            .iter()
            .filter(|&&id| content.is_none_or(|part| !part.contains(id)))
            .count(),
        live_thread_count: index
            .thread
            .records()
            .filter(|record| record.deleted.is_none())
            .count(),
        missing_thread: delivered
            .iter()
            .filter(|&&id| !index.thread.is_present(id))
            .count(),
        exact_manifest: target.exact_manifest(index),
        exact_content: target.exact_content(index),
        exact_thread: target.exact_thread(index),
        current: za_content_current(index, target.email_state),
    })
}

async fn email_state(test: &TestServer, id: u32) -> u64 {
    test.server
        .store()
        .get_last_change_id(id, SyncCollection::Email.into())
        .await
        .expect("read Email state")
        .expect("delivered mail must have an Email state")
}

pub(in crate::za::search_bench) async fn run(count: usize, test_name: &str) {
    println!(
        "Cold-start {} setup: seed {SEED}, {count} messages, pid {}, arch {}, wire ceiling {WIRE_TIMEOUT:?}",
        if count == DESIGN {
            "design size"
        } else {
            "five-message plumbing control"
        },
        std::process::id(),
        std::env::consts::ARCH,
    );
    assert!(
        !std::env::temp_dir().join(test_name).exists(),
        "use a fresh TMPDIR for cold-start evidence"
    );
    let mut test = server_named(test_name).await;
    // Keep the disposable fixture on failure for diagnosis.
    test.temp_dir.delete = false;
    println!("Cold-start setup fixture: {}", test.tmp_dir());
    let config = test.server.za_search_config();
    let cache = &test.server.inner.cache.za_search;
    assert_eq!(
        config,
        ZaSearchConfig::default(),
        "cold-start requires the search defaults"
    );
    assert_eq!(cache.config(), &config);
    println!("Cold-start setup search config: {config:?}");

    let admin = test.create_admin_account("admin@example.com").await;
    let (account, setup) = super::key(&test, &admin, ACCOUNT).await;
    let id = uid(&account);
    let wire = Wire::new(&account);
    let delivered = deliver_cohort(&test, &account, generated(SEED).take(count), count).await;
    assert_pristine_setup(&test, id, setup).await;
    assert_mailbox(&test, id, &delivered).await;
    let sampler = Sampler::start(&test);
    let (ready, _) = drain(&test, &wire, &sampler, &[], false, "cold-start setup").await;
    sampler.finish("cold-start setup");
    assert!(
        ready.current,
        "setup did not reach complete Content coverage"
    );
    assert_eq!(ready.indexed, count, "setup indexed count");
    let state = email_state(&test, id).await;
    assert_eq!(
        ready.email_state, state,
        "setup must have settled Email state"
    );

    let keys = za_unlock(&test, id, account.secret()).await;
    let (durable, ManifestValue::Index(manifest)) = test
        .server
        .za_search_read_manifest(id, &za_search_key(keys.swk()))
        .await
        .expect("setup manifest must authenticate")
        .expect("setup durable manifest")
    else {
        panic!("setup manifest is destroying");
    };
    drop(keys);
    assert!(
        manifest == ready.manifest,
        "setup durable and resident manifests differ"
    );
    assert!(
        manifest.content.is_some(),
        "setup must have a durable Content head"
    );
    inventory(&test, &account, &manifest).await;
    let expected_head = manifest.content.as_ref().unwrap();
    println!(
        "Cold-start setup durable: Email state {state}, epoch/revision {}/{}, Map segments {}, Content chain {} segments / {} bytes",
        manifest.epoch,
        manifest.revision,
        manifest.thread_blobs.len(),
        expected_head.length,
        expected_head.size,
    );
    let (expected_id, query_state) = {
        let messages = test
            .server
            .fresh_messages(id)
            .expect("setup needs the settled mail cache");
        // No sort is specified. JMAP walks the message-cache order, and
        // every generated body starts with the canary.
        let first = messages
            .emails
            .iter()
            .next()
            .expect("nonempty setup")
            .document_id();
        assert!(delivered.contains(&first));
        let entry = cache.get(id).expect("setup needs a resident Map");
        let guard = entry.index.read().await;
        let index = guard.as_ref().expect("setup needs a loaded Map");
        assert!(index.thread.is_present(first));
        (
            Id::from_parts(index.thread.current_thread(first), first),
            messages.get_state(false).to_string(),
        )
    };
    let target = Target {
        account_id: id,
        email_state: state,
        manifest,
        manifest_hash: manifest_hash(&durable),
        expected_id,
        query_state,
    };

    cache.remove_account(id, EvictReason::Invalidation);
    assert!(cache.get(id).is_none(), "setup eviction left an entry");
    assert_eq!(
        (cache.content_used(), cache.thread_used()),
        (0, 0),
        "setup eviction left charges"
    );

    let commits = SearchCommitCounter::start(id);
    let (cold, between, repeat) =
        measure_pair(|| query(&wire, id), || snapshot(&test, &target, &commits)).await;
    let after = snapshot(&test, &target, &commits);
    drop(commits);

    // Do not format output, issue another wire request, or scan IDs until
    // both attempts have finished. These totals are inclusive, not additive.
    println!("Cold-start pair stages include nested work. Do not add stage durations.");
    cold.report("cold", &target);
    println!("Cold-start between: {}", json!(between));
    repeat.report("repeat", &target);
    println!("Cold-start after: {}", json!(after));
    let census = census(&test, &target, &delivered);
    println!("Cold-start census: {}", json!(census));
    let unchanged_manifest = manifest_bytes(&test, id).await.as_deref() == Some(durable.as_slice());
    let unchanged_email = email_state(&test, id).await == state;
    println!(
        "Cold-start durable after: unchanged manifest {unchanged_manifest}, unchanged Email state {unchanged_email}"
    );

    assert!(
        cold.matches(&target),
        "cold query did not return the expected response"
    );
    assert!(
        repeat.matches(&target),
        "repeat query did not return the expected response"
    );
    for attempt in [&cold, &repeat] {
        assert!(
            attempt.observation.claimed(),
            "missing per-request dispatcher observation"
        );
        assert_eq!(attempt.observation.count(SearchStage::DispatcherPrePass), 1);
        assert!(
            attempt
                .observation
                .admission()
                .is_some_and(|a| a.reason == "admitted")
        );
    }
    assert!(
        cold.observation.count(SearchStage::VerifiedBlobFetch) > 0,
        "cold request must read sealed blobs"
    );
    assert_eq!(
        repeat.observation.count(SearchStage::VerifiedBlobFetch),
        0,
        "repeat reread the sealed index"
    );
    assert_eq!(
        between.commits, 0,
        "first request published indexing commits"
    );
    assert_eq!(after.commits, 0, "the pair published indexing commits");
    assert!(
        unchanged_manifest && unchanged_email,
        "durable state moved during the pair"
    );
    let census = census.expect("a settled Map is required after the pair");
    assert!(census.exact_manifest && census.exact_content && census.exact_thread && census.current);
    assert_eq!(
        (census.content_count, census.live_thread_count),
        (count, count)
    );
    assert_eq!((census.missing_content, census.missing_thread), (0, 0));

    admin.destroy_account(account).await;
    test.shutdown();
    test.temp_dir.delete();
}

#[tokio::test]
async fn first_query_failure_keeps_both_attempts() {
    use std::cell::Cell;
    for first in [Err("transport"), Ok("method error")] {
        let calls = Cell::new(0);
        let (cold, between, repeat) = measure_pair(
            || {
                let ordinal = calls.get();
                calls.set(ordinal + 1);
                std::future::ready(if ordinal == 0 { first } else { Ok("repeat") })
            },
            || calls.get(),
        )
        .await;
        assert_eq!(
            (cold, between, repeat, calls.get()),
            (first, 1, Ok("repeat"), 2)
        );
    }
}

#[tokio::test]
async fn cold_search_observation_is_scoped_and_cleared() {
    use common::za::test_hooks::{claim_search_observation, search_span, with_search_observation};

    const ACCOUNT: u32 = 29;
    let dropped = std::panic::catch_unwind(|| {
        let _observation = SearchRequestObservation::arm(ACCOUNT);
        panic!("exercise observation cleanup");
    });
    assert!(dropped.is_err());
    let first = SearchRequestObservation::arm(ACCOUNT);
    assert!(claim_search_observation(ACCOUNT + 1, first.call_id()).is_none());
    assert!(claim_search_observation(ACCOUNT, "bench").is_none());
    let claimed = claim_search_observation(ACCOUNT, first.call_id()).unwrap();
    assert!(claim_search_observation(ACCOUNT, first.call_id()).is_none());

    let second = SearchRequestObservation::arm(ACCOUNT);
    let mut first_work = Box::pin(with_search_observation(Some(&claimed), async {
        assert!(search_span(ACCOUNT + 1, SearchStage::Method).is_none());
        let _span = search_span(ACCOUNT, SearchStage::Method).unwrap();
        std::future::pending::<()>().await;
    }));
    assert!(futures::poll!(&mut first_work).is_pending());
    drop(first_work);
    assert_eq!(first.count(SearchStage::Method), 1);
    assert!(search_span(ACCOUNT, SearchStage::Method).is_none());
    assert_eq!(second.count(SearchStage::Method), 0);
    drop(first);
    assert!(claim_search_observation(ACCOUNT, second.call_id()).is_some());
}
