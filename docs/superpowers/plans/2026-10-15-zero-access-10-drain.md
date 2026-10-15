# Zero-access plan 10: the drain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A key account's own DAV request opens the pending items in its scheduling inbox, applies them to its calendar through upstream's merge, and reseals them under its master key; expired UID creation leases are swept; the scheduling gates release 1 closed are reopened; the end-to-end invitation flows, the leak coverage of the drain, the key-mode CalDAV variant and the manual checklist complete release 2.

**Architecture:** Release 2 (invitations between key accounts) is three plans. Plan 8 (PR #34) built the key material and the sealed inbox items; plan 9 (PR #35) built the fan-out and the UID creation lease on the PUT side. Plan 10, this plan, adds:
- **The drain** (spec 6; new `crates/groupware/src/calendar/drain.rs`, trait `ZaDrain` on `Server`). It runs in `handle_dav_request` (`crates/dav/src/request.rs`) before the request is dispatched, for every DAV request whose access token holds the authenticated account's session keys, and never fails that request (invariant 14). A pass takes the session's lock, reads the inbox change id (the snapshot, spec 6.2), enumerates the items whose pending bit is set in document id order, unwraps the private key once (`Server::za_private_key`), and for each item: opens the `pk` wrap (`open_pending_item`, the only place one is opened, invariant 11), checks the sender, finds the event by UID, imports under the creation lease or merges under the event's archive assertion, adopts or keeps token pairs (spec 3.3), checks a REPLY's token and SEQUENCE, fans out the organizer's REQUEST to delegates the merge added, and rewrites the item resealed (`seal_drained_item`), bit cleared, in the same store write as the event. Upstream's pure iTIP functions (`itip_snapshot`, `itip_process_message`, `itip_merge_changes`, `itip_import_message`, `organizer_request_full`) run unchanged (invariant 15); only the commit is the fork's.
- **The drain state in `SessionKeys`** (spec 3.2, 6.2-6.3): the remembered inbox change id, the retry deferral and the one-pass-per-session lock, in memory only.
- **The lease sweep** (spec 6.3 step 4): `sweep_expired_uid_leases` in `crates/common/src/za/lease.rs`, called from the maintenance task's `PurgeData` step right after `purge_in_memory_store`, clearing each expired lease with an assertion on the value it read.
- **Test-mode hooks** (spec 11): two drain pauses, a one-shot drain commit failure, a per-account (or global) drain skip, and a retry-backoff override, in `crates/common/src/za/test_hooks.rs`.
- **The gates spec 8 reopens**: `calendar-auto-schedule` in the DAV `OPTIONS` header and the schedule inbox and outbox URLs on a key account's principal.
- **Tests**: the drain modules of the za suite, the leak scan of a drained flow, the key-mode `scheduling` variant's drained copy, the mutation runs; and the documentation: manual checklist item 13, CLAUDE.md, the outcome note.

The fork diff stays narrow: one new module (`drain.rs`), additions to the fork's own modules (`lease.rs`, `test_hooks.rs`, `session.rs`), one call insertion in `request.rs`, one in `maintenance.rs`, and the removal of two release 1 gates (`http/src/request.rs`, `principal/propfind.rs`).

**Tech Stack:**
- Rust 2024 workspace (Stalwart 0.16.25 fork)
- `tokio` (`sync` feature, new to `crates/vault`), `xxhash-rust 0.8` (xxh3), `rand 0.10` (as `store::rand`), `ahash`
- calcard 0.3, rkyv 0.8.18
- the `tests` crate harness (`TestServer`, `DummyWebDavClient`, `HttpRequest`)

**Spec:** `docs/superpowers/specs/2026-10-11-zero-access-invitations-design.md`. These sections bind this plan:
- 3.2 (the private key in session keys, the remembered change id), 3.3 (pair adoption and maintenance at the drain, delegates, verification).
- 4 (the flow overview).
- 6: all of it (where the drain runs, triggers and the cursor, concurrency and the lease, the steps, item errors, the inbox as clients see it).
- 7 (free-busy: unchanged), 8 (the gates: `calendar-auto-schedule` and the principal URLs reopen; everything else unchanged).
- 9 (stored shapes, built by plan 8), 10 (error handling), 11 (hooks, the integration tests, leak coverage, the key-mode suite, mutation checks, the manual checklist), 12 (invariants 11-15), 13 (the names settled below).

`docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md` (release 1) also binds: section 9 (session keys on every DAV request), section 10 (traces and unseal failures), section 12 (invariants 1-10).

Project rules: `CLAUDE.md`, `docs/superpowers/plans/README-dev.md`, `docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md`. The plan 8 outcome note (`docs/superpowers/plans/2026-10-13-zero-access-plan8-outcome.md`, "Facts for plans 9 and 10") and the plan 9 outcome note (`docs/superpowers/plans/2026-10-14-zero-access-plan9-outcome.md`, "Facts for plan 10") give the interfaces this plan calls; their facts are restated in the tasks that need them.

## Global Constraints

Commands:
- Every cargo command starts with `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`.
- Product build: `cargo build --release -p stalwart --no-default-features --features rocks`. Never build it with the `enterprise` feature.
- Per-task compile check in the shipping configuration: `cargo check -p stalwart --no-default-features --features rocks`, filtered with:
  ```bash
  ... 2>&1 | grep -E "^(warning|error)" -A3 | grep -E "crates/(groupware|dav|vault)/|crates/http/src/request.rs|crates/services/src/task_manager/maintenance.rs|crates/common/src/(za/|auth/vault.rs)" || echo clean
  ```
  The 24 warnings upstream's code produces in store, common, jmap and services are known and are not this plan's.
- Test-mode compile check: `cargo check -p tests --tests`. A plain `cargo check -p tests` checks only `tests/src/lib.rs`, and `cargo check -p <crate> --features test_mode` fails in upstream `common` code that needs `enterprise` (plan 9 outcome). The product build never enables `test_mode`.
- Unit tests: `cargo test -p vault`, `cargo test -p groupware`, `cargo test -p common za::lease`. `cargo test -p common` without `enterprise` compiles for unit tests; the hook module has no unit tests of its own.
- Integration suite: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture`. One server on port 8899. Never run two suites at once, never a suite in parallel with a mutation build, and never while Jay's manual server (8080/8443/1143) is under test by another agent.
- CalDAV suites: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture`, plain and with `ZA_KEY_ACCOUNTS=1`. Upstream's `cal_itip` sub-test is timing-flaky (DTSTAMP index mismatch); rerun once before treating a failure there as a regression.

Formatting and warnings:
- `cargo fmt -p <crate> -- --check` for every touched crate: `vault`, `common`, `groupware`, `dav`, `services`, `tests`; for the http crate `cargo fmt --manifest-path crates/http/Cargo.toml -- --check`.
- Build output stays warning-free for new code.

Invariants and code rules:
- No edits to generated code (`crates/registry/src/schema/*`, `crates/trc/src/event/enums.rs`). No new setting, permission, event type or environment knob: the retry backoff is a constant with a test-mode override, not `ZA_DRAIN_RETRY_SECS`.
- No stored struct changes layout (invariant 1). The drain rewrites `CalendarEventNotification` records in place and writes `CalendarEvent` records as the PUT path does.
- The `pk` wrap is opened only by `open_pending_item`, called only from the drain in a request that holds the recipient's session keys (invariant 11). Background code never needs a key: the sweep touches lease keys only.
- Nothing derived from an invitation reaches the task queue, mail or any store location in plaintext (invariant 12). The drain's delegation fan-out goes through `za_fan_out`, never `ItipMessages::queue`.
- The organizer's event never stores an attendee's account id (invariant 13).
- The drain never fails the request that triggered it (invariant 14): `za_drain` returns `()`, traces its errors, and the request proceeds. It deletes an item only for the permanent errors of spec 6.5 and for a reply whose token does not match.
- Upstream's builders and merge run unchanged (invariant 15); non-key accounts take unchanged paths (invariant 9): the drain runs only when the access token holds session keys, which only a key account's own authentication produces (`AccessToken::za_keys_for`).
- Traces name account ids, document ids and fixed strings only: never an address, a tree or a token. `ItipError`'s `Display` carries an address for two variants, so the drain traces a fixed name per variant (`itip_reason`).
- Key material and tokens live in `Zeroizing` buffers with a silent `Debug`.
- New source files start with the SPDX header copied from a neighbour:
  ```
  /*
   * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
   *
   * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
   */
  ```
- Each item's event write and item rewrite are one store write. `Server::commit_batch` (`crates/common/src/storage/transaction.rs`) writes each commit point of a batch separately, but `BatchBuilder::commit_point` records a boundary only once the batch is large (`is_large_batch`: over 5,000,000 bytes or 1,000 operations, `crates/store/src/write/batch.rs:377-395`), so an item's batch, far below that, is one write whatever its commit points. The drain still adds the item rewrite (and the lease assertion) **before** the event write, so an oversized batch, if one ever arose, would split after both; no mutation pins this, since a small batch cannot be split.

Tests:
- Every za sub-module removes what it creates. `assert_is_empty` runs once, at the suite end, after `destroy_key_accounts`; no sub-module calls it.
  - Events are deleted with `Schedule-Reply: F` (`NO_REPLY`), which builds no CANCEL and no declined REPLY; inbox items by href (`<document id>.ics`); the drain's imported copies by the href `copy_href` returns.
  - Accounts outlive the modules: key1, key2, key3 (password `STRONG`) until the suite end; `plain@example.com` beyond it.
- The suite skips every account's drain by default: `za_tests` (`tests/src/za/mod.rs`) sets `test_hooks::set_drain_skip(DRAIN_SKIP_ALL)` right after setup, so the plan 8 and 9 modules keep reading pending items at store level. The drain modules clear it for their duration through the `DrainOn` guard (Task 2) and restore it on drop. The key-mode `webdav_tests` sets no hook, so the drain runs there.
- `DummyWebDavClient` times out after 500 ms. A request that waits on a lease or a pause goes through `HttpRequest::send_full` (5 s): `trigger_drain` and the `put` helper of `uid_lease.rs`.
- Never weaken an assertion to make a test pass. Values read from code and never run are marked "(from code reading)". If a run disagrees with one, assert what the run shows only where the step names an oracle or a fallback, and record a ruling in the task report. Otherwise stop and report.
- Mutation checks are temporary edits under `crates/`, undone at once with `git checkout -- <file>`; `git status --short crates/` must print nothing before the next edit.
- The new sub-modules run after `uid_lease::test` and before `tracing::test`; `leak::test_drain` runs right before `leak::test`.

Process:
- Code in this plan was written without a compiler. Small type, import and borrow fixes are expected (a snapshot borrowing a tree that a later call moves is the usual one); changes of behaviour are not.
- Work on `release2-plan10-drain` (cut from `main` at `9a1855e4`). Do not push.
- Commit messages: subject, blank line, body, then the two trailer lines exactly as the harness's attribution reminder gives them at run time (`Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr` in plans 8 and 9).
- The final verification (Task 7) runs the unit suites, `za_tests` plain and with `BLOB_STORE=FileSystem`, `webdav_tests` in both modes, and the release build.

### Names this plan settles (spec 13)

| Name | Value | Where |
|---|---|---|
| Drain entry | trait `ZaDrain` on `Server`, `za_drain(&self, keys: &Arc<SessionKeys>) -> ()`; called in `handle_dav_request` before `dispatch_dav_request` when `access_token.za_keys_for(access_token.account_id())` is `Some` | new `crates/groupware/src/calendar/drain.rs`, `crates/dav/src/request.rs` |
| Drain state | `ZaDrainState { cursor, retry_after, pass }` in `SessionKeys` (`SessionKeys::drain()`): `cursor()` / `complete(snapshot)` (the remembered inbox change id, `None` until a pass completes), `retry_after()` / `defer_until(secs)`, `lock()` (a `tokio::sync::Mutex<()>`: one pass per session on a node) | `crates/vault/src/session.rs` |
| Mid-session trigger | spec 6.2 option 1: `fetch_dav_resources(.., SyncCollection::CalendarEventNotification).highest_change_id` compared with the cursor on every DAV request of a key account; no in-memory counter, no `CacheInvalidation` | `drain.rs` |
| Retry backoff | `DRAIN_RETRY_SECS: u64 = 30`, a constant (no `ZA_DRAIN_RETRY_SECS`); test-mode override `set_drain_retry_override` | `drain.rs`, `test_hooks.rs` |
| Pass outcome | all items done: `complete(snapshot)` (cursor = snapshot, deferral cleared); an item left pending without an error (a busy lease) or a transient error: `defer_until(now + DRAIN_RETRY_SECS)`, cursor unchanged; a transient error aborts the pass | `drain.rs` |
| Lease sweep | `sweep_expired_uid_leases(store) -> trc::Result<usize>` = `collect_expired_uid_leases(store, now)` then `clear_uid_leases(store, expired)`; each clear asserts `AssertValue::Hash(xxh3_64(value read))`; a malformed value (no expiry) counts as expired, as `acquire` treats it; called in the `PurgeData` arm of `store_maintenance` after `purge_in_memory_store` | `lease.rs`, `crates/services/src/task_manager/maintenance.rs` |
| Sweep on Redis + data store | not tested in a container: the sweep reads and writes the data store whatever the lookup backend, the same code path | ruling |
| Hooks | `DRAIN_LOOKUP_PAUSE` (import: after the first UID lookup found nothing, before the lease), `DRAIN_LEASE_PAUSE` (holding the lease, after the second lookup, before the commit), `DRAIN_FAIL_ONCE` (the account's next drain commit fails with an internal error), `DRAIN_SKIP` (accounts whose drain does not run; `DRAIN_SKIP_ALL = u32::MAX`), `DRAIN_RETRY_OVERRIDE` | `test_hooks.rs` |
| Trace details | `zero-access: drain pass failed` (error level, the pass's error), `zero-access: drain creation lease busy` (`Store(AssertValueFailed)`), `zero-access: drain commit lost its assertion` (the pass's error), `zero-access: undecodable inbox item deleted` and `zero-access: inbox item kept as history` (`Calendar(ItipMessageError)`, `Reason` = a fixed variant name), `zero-access: reply token mismatch` (`Security(Unauthorized)`) | `drain.rs` |
| Item flags after the drain | import: `0`; merge: `EVENT_NOTIFICATION_IS_CHANGE` (as `commit_itip_merge` sets it); kept as history: `0`; `event_id` = the event's document id when one was found or created, else `None`; `created` kept, `modified` = now | `drain.rs` |
| Stale reply | per item: a REPLY is stale when any of its instances has a SEQUENCE lower than the event's current SEQUENCE for the same `InstanceId`, or, for an occurrence the event stores no override for, than the master's (absent = 0); rejected as `ItipError::OutOfSequence`, kept as history | `drain.rs` |
| Sender check | the candidates are the ORGANIZER of a REQUEST or CANCEL and every ATTENDEE of a REPLY (upstream exports the replier with its delegates and delegators); those resolving with `Server::za_account_id_from_email` (quiet) to `ChangedBy::PrincipalId` are the sender's addresses; none, or a `CalendarAddress` sender, is rejected as `SenderIsNotOrganizerNorAttendee`, kept as history. For a REPLY the replier is the sender's address whose pair holds the token; none matching is a forgery | `drain.rs` |
| Quota at the drain | only `Limit(Quota)` / `Limit(TenantQuota)` from `has_available_quota` is the `QuotaExceeded` rejection (as the fan-out classifies it); any other error is a store error that keeps the item pending | `drain.rs` |
| Permanent errors | any `open_pending_item` error, an unparsable or missing METHOD, a METHOD other than REQUEST, REPLY, CANCEL: the item is deleted | `drain.rs` |
| Missing reply token | a REPLY item without `X-ZA-RSVP-TOKEN` is treated as a token mismatch (deleted, security trace) | `drain.rs` |
| Which copy merges | the lowest document id among the account's events with the UID, as upstream's `itip_ingest` takes the first hit | `drain.rs` |
| Gates reopened | `OPTIONS` answers upstream's `DAV` header for every request (the key-account branch and `za_is_key_account_request` are removed); the principal's `ScheduleInboxURL` / `ScheduleOutboxURL` arm for key accounts is removed | `crates/http/src/request.rs`, `crates/dav/src/principal/propfind.rs` |
| `cal_scheduling` in key mode | upstream's test still does not run in key mode; the `scheduling` variant asserts the drained copy, the update and the cancel (Task 6) | `tests/src/webdav/za_variants.rs` |
| Test helpers | `za_app_password`, `trigger_drain`, `pending_ids`, `copy_href` | `tests/src/utils/za.rs` |

## Review Focus

Each line names an input the spec implies but does not walk through, with the behaviour a person would expect. Each has a test in the task that owns the code.

1. **A pending item deleted by its owner while a pass holds it** (a client's DELETE by href between the drain's re-read and its commit). Expected: the pass's commit loses the item's assertion, nothing is imported (the event write and the item rewrite are one store write), the lease is released, and the request that triggered the pass still succeeds. Task 4, `test_lease` case D.
2. **The same UID stored twice** (the attendee copied the imported event into a second calendar, as COPY allows). Expected: an organizer update merges into the lowest document id only; the other copy is untouched. Task 5, `test_errors` step 4.
3. **The attendee's own alarm across an organizer update.** Expected: the merge keeps the VALARM the attendee added to the copy and takes the new time and SEQUENCE (spec 4.2). Task 2, `test` step 3.
4. **A REPLY for an event the organizer no longer has** (deleted with `Schedule-Reply: F`, or never stored because the create failed, spec 5.4). Expected: kept as inbox history with `event_id = None`, the calendar unchanged, no deletion. Task 5, `test_errors` step 3.
5. **A DAV request that touches no calendar** (a principal PROPFIND). Expected: it still drains the authenticated account's inbox, since the drain keys on the token's session keys, not on the request's target; the same holds for a request for another account's data. Task 2, `test` step 1 (the trigger is a principal PROPFIND).

## File map

| File | Change | Task |
|---|---|---|
| `crates/vault/Cargo.toml` | `tokio` (`sync`) | 1 |
| `crates/vault/src/session.rs` | `ZaDrainState`, `SessionKeys::drain()`, tests | 1 |
| `crates/common/src/za/lease.rs` | `collect_expired_uid_leases`, `clear_uid_leases`, `sweep_expired_uid_leases`, tests; module doc | 1 |
| `crates/common/src/za/test_hooks.rs` | the drain hooks | 1 |
| `crates/services/src/task_manager/maintenance.rs` | the sweep call | 1 |
| `tests/src/utils/za.rs` | `za_app_password`, `trigger_drain`, `pending_ids`, `copy_href` | 1 |
| `tests/src/za/sweep.rs` | new: the sweep through the maintenance task | 1 |
| `tests/src/za/mod.rs` | `set_drain_skip(DRAIN_SKIP_ALL)` after setup; module wiring | 1, 2, 3, 4, 5 |
| `crates/groupware/src/calendar/drain.rs` | new: the drain, unit tests | 2 |
| `crates/groupware/src/calendar/mod.rs` | `pub mod drain;` | 2 |
| `crates/dav/src/request.rs` | the trigger | 2 |
| `tests/src/za/drain.rs` | new: `DrainOn`, `test` (2); `test_reply`, `test_adoption` (3); `test_lease`, `test_cursor` (4); `test_errors` (5) | 2, 3, 4, 5 |
| `tests/src/za/uid_lease.rs` | `put` made `pub` | 4 |
| `tests/src/za/leak.rs` | `test_drain`, canaries | 5 |
| `crates/http/src/request.rs`, `crates/dav/src/principal/propfind.rs` | gates removed | 6 |
| `tests/src/za/gating.rs` | flipped assertions | 6 |
| `tests/src/webdav/za_variants.rs` | `scheduling`: the drained copy, update, cancel | 6 |
| `docs/zero-access/manual-checklist.md` | item 13 | 6 |
| `CLAUDE.md` | status, hooks, the drain's location | 6 |
| `docs/superpowers/specs/2026-10-11-zero-access-invitations-design.md` | names settled (6.2, 6.3 step 4, 11, 13) | 7 |
| `docs/superpowers/plans/2026-10-15-zero-access-plan10-outcome.md` | new: mutation table, rulings, deferred findings | 7 |

---

### Task 1: Drain state, hooks, the lease sweep, and the test helpers

**Files:**
- Modify: `crates/vault/Cargo.toml`
- Modify: `crates/vault/src/session.rs`
- Modify: `crates/common/src/za/lease.rs`
- Modify: `crates/common/src/za/test_hooks.rs`
- Modify: `crates/services/src/task_manager/maintenance.rs:218-224` (the `PurgeData` arm, after `purge_in_memory_store`)
- Modify: `tests/src/utils/za.rs`
- Create: `tests/src/za/sweep.rs`
- Modify: `tests/src/za/mod.rs`

**Interfaces:**
- Consumes: `ZaUidLease::acquire` / `release`, `lease_key_range`, `lease_value_expiry`, `lease_class` (private, same file), `RawValue`, `AssertValue::Hash`, `xxh3_64` (`lease.rs`); `Pause`, `take_for`, `wait` (`test_hooks.rs`); `za_post`, `VaultReply::str`, `inbox_items`, `HttpRequest::{with_credentials, send_full}` (`tests/src/utils`); `DavResources::{any_resource_path_by_id, format_resource}` (`crates/common/src/storage/dav.rs`); `Server::document_ids_matching` (`crates/common/src/storage/document.rs`).
- Produces, for Task 2 onwards:
  - `SessionKeys::drain(&self) -> &ZaDrainState`; `ZaDrainState::{cursor() -> Option<u64>, complete(&self, snapshot: u64), retry_after() -> u64, defer_until(&self, secs: u64), lock() -> tokio::sync::MutexGuard<'_, ()>}`.
  - `common::za::lease::{collect_expired_uid_leases, clear_uid_leases, sweep_expired_uid_leases}`.
  - `common::za::test_hooks::{DRAIN_LOOKUP_PAUSE, DRAIN_LEASE_PAUSE, DRAIN_FAIL_ONCE, DRAIN_SKIP_ALL, set_drain_lookup_pause, set_drain_lease_pause, set_drain_fail_once, set_drain_skip, clear_drain_skip, drain_skipped, set_drain_retry_override, drain_retry_override, drain_lookup_pause_point, drain_lease_pause_point, take_drain_fail}`.
  - `tests::utils::za::{za_app_password, trigger_drain, pending_ids, copy_href}`.

- [ ] **Step 1: The drain state in `SessionKeys`**

Add to `crates/vault/Cargo.toml` under `[dependencies]`:

```toml
tokio = { version = "1.53", default-features = false, features = ["sync"] }
```

In `crates/vault/src/session.rs`, replace the `use` lines and the struct with:

```rust
use crate::keys::{KeyError, Secret, derive_event_key};
use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
};

/// Keys resident for one request (spec 5, "in-flight copies"). Cloned by
/// `Arc`; the secrets are zeroed when the last reference drops.
pub struct SessionKeys {
    pub account_id: u32,
    pub generation: u64,
    mk: Secret,
    ewk: Secret,
    /// Invitations spec 3.2: the X25519 private key, unwrapped from the
    /// vault record on first use. Holds the outcome, so a wrap that does not
    /// open is traced once per session.
    private_key: OnceLock<Result<Secret, KeyError>>,
    /// Invitations spec 6.2-6.3: what this session remembers about its
    /// drain. In memory only, dropped with the key-cache entry.
    drain: ZaDrainState,
}

/// The drain's per-session state (invitations spec 6.2, "The cursor", and
/// 6.3, "The pass"): the inbox change id the last complete pass observed
/// before enumerating, the time before which a pass that left an item
/// pending is not retried, and the lock that keeps to one pass per session
/// on a node. Never a value produced by the drain's own writes: the cursor
/// is the snapshot the pass started from.
pub struct ZaDrainState {
    /// `NO_CURSOR` until a pass completes.
    cursor: AtomicU64,
    /// Unix seconds; `0` when no deferral is in force.
    retry_after: AtomicU64,
    pass: tokio::sync::Mutex<()>,
}

const NO_CURSOR: u64 = u64::MAX;

impl ZaDrainState {
    fn new() -> Self {
        ZaDrainState {
            cursor: AtomicU64::new(NO_CURSOR),
            retry_after: AtomicU64::new(0),
            pass: tokio::sync::Mutex::new(()),
        }
    }

    /// The inbox change id the last complete pass started from; `None`
    /// until one completed (the session's first request drains).
    pub fn cursor(&self) -> Option<u64> {
        Some(self.cursor.load(Ordering::SeqCst)).filter(|cursor| *cursor != NO_CURSOR)
    }

    /// A pass found every pending item done: remember its snapshot and lift
    /// any deferral.
    pub fn complete(&self, snapshot: u64) {
        self.cursor
            .store(snapshot.min(NO_CURSOR - 1), Ordering::SeqCst);
        self.retry_after.store(0, Ordering::SeqCst);
    }

    pub fn retry_after(&self) -> u64 {
        self.retry_after.load(Ordering::SeqCst)
    }

    /// A pass left an item pending (spec 6.2): no pass before `secs`.
    pub fn defer_until(&self, secs: u64) {
        self.retry_after.store(secs, Ordering::SeqCst);
    }

    /// One pass per session on a node (spec 6.3): a second request of the
    /// session waits for the running pass and then finds its cursor.
    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.pass.lock().await
    }
}
```

In `SessionKeys::new` add `drain: ZaDrainState::new(),` to the struct literal, and add the accessor after `set_private_key`:

```rust
    pub fn drain(&self) -> &ZaDrainState {
        &self.drain
    }
```

The `Debug` impl is unchanged (it prints the account and generation only).

- [ ] **Step 2: Unit tests for the drain state**

Append to the `tests` module of `session.rs`:

```rust
    #[tokio::test]
    async fn drain_state_starts_empty_and_remembers_a_completed_pass() {
        let keys = SessionKeys::new(3, 11, Secret::from_bytes([9; 32]));
        let drain = keys.drain();
        assert_eq!(drain.cursor(), None, "no pass completed yet");
        assert_eq!(drain.retry_after(), 0);
        drain.defer_until(1_700_000_000);
        assert_eq!(drain.retry_after(), 1_700_000_000);
        drain.complete(42);
        assert_eq!(drain.cursor(), Some(42));
        assert_eq!(drain.retry_after(), 0, "a complete pass lifts the deferral");
        drain.complete(0);
        assert_eq!(drain.cursor(), Some(0), "an empty inbox's change id is a cursor too");
        drain.complete(u64::MAX);
        assert_eq!(drain.cursor(), Some(u64::MAX - 1), "the sentinel is never stored");
        // One pass at a time.
        let held = drain.lock().await;
        assert!(drain.pass.try_lock().is_err(), "a second pass waits");
        drop(held);
        assert!(drain.pass.try_lock().is_ok());
        assert_eq!(format!("{keys:?}"), "SessionKeys(account 3, generation 11)");
    }
```

`cargo test -p vault` needs tokio's test macro: add to `crates/vault/Cargo.toml`:

```toml
[dev-dependencies]
tokio = { version = "1.53", features = ["macros", "rt"] }
```

- [ ] **Step 3: Run the vault tests**

Run: `export PATH="/opt/homebrew/opt/rustup/bin:$PATH" && cargo test -p vault`
Expected: all pass, the new test included.

- [ ] **Step 4: The lease sweep**

In `crates/common/src/za/lease.rs`, replace the last two sentences of the module doc ("The generic `purge_in_memory_store` skips ... with an assertion on the value it read.") with:

```rust
//! The generic `purge_in_memory_store` skips the
//! `KV_LOCK_ZA_UID` range, since its unconditional clear would delete a
//! replacement lease; `sweep_expired_uid_leases` (spec 6.3 step 4), run by
//! the maintenance task's purge step, is the only cleanup of lease keys and
//! clears each expired lease with an assertion on the value it read.
```

Add `IterateParams` to the `store` import (`store::{IterateParams, Store, U64_LEN, ValueKey, rand, write::{..}}`) and add after `lease_value_expiry`:

```rust
/// Spec 6.3 step 4, first half: every lease key whose expiry is at or below
/// `now`, with the hash of the value read. A value too short to hold an
/// expiry counts as expired, as `acquire` treats it.
pub async fn collect_expired_uid_leases(
    store: &Store,
    now: u64,
) -> trc::Result<Vec<(Vec<u8>, u64)>> {
    let (from, to) = lease_key_range();
    let mut expired = Vec::new();
    store
        .iterate(IterateParams::new(from, to), |key, value| {
            if lease_value_expiry(value).is_none_or(|expiry| expiry <= now) {
                expired.push((key.to_vec(), xxh3_64(value)));
            }
            Ok(true)
        })
        .await
        .caused_by(trc::location!())?;
    Ok(expired)
}

/// Spec 6.3 step 4, second half: clears each collected key, asserting the
/// value the sweep read, so a lease re-taken in between survives (its
/// clear loses the assertion and is skipped). One write per key: a lost
/// assertion must not undo the others. Returns how many were cleared.
pub async fn clear_uid_leases(store: &Store, expired: Vec<(Vec<u8>, u64)>) -> trc::Result<usize> {
    let mut cleared = 0;
    for (key, hash) in expired {
        let mut batch = BatchBuilder::new();
        batch.assert_value(lease_class(key.clone()), AssertValue::Hash(hash));
        batch.clear(lease_class(key));
        match store.write(batch.build_all()).await {
            Ok(_) => cleared += 1,
            Err(err) if err.is_assertion_failure() => {}
            Err(err) => return Err(err.caused_by(trc::location!())),
        }
    }
    Ok(cleared)
}

/// Spec 6.3 step 4: the sweep the maintenance task runs on the data store,
/// whatever the lookup backend.
pub async fn sweep_expired_uid_leases(store: &Store) -> trc::Result<usize> {
    let expired = collect_expired_uid_leases(store, now()).await?;
    clear_uid_leases(store, expired).await
}
```

Append to the `tests` module of `lease.rs`:

```rust
    #[tokio::test]
    async fn the_sweep_clears_expired_leases_and_keeps_live_ones() {
        let store = EphemeralStore::open();
        write_raw(&store, 11, "expired", expired_value(3)).await;
        write_raw(&store, 11, "malformed", vec![1, 2, 3]).await;
        let live = ZaUidLease::acquire(&store, 11, "live")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(sweep_expired_uid_leases(&store).await.unwrap(), 2);
        assert!(stored(&store, 11, "expired").await.is_none());
        assert!(stored(&store, 11, "malformed").await.is_none());
        assert!(stored(&store, 11, "live").await.is_some(), "a live lease was swept");
        assert!(commit_fenced(&store, &live).await.is_ok());
        assert!(live.release(&store).await);
        assert_eq!(sweep_expired_uid_leases(&store).await.unwrap(), 0);
    }

    /// A lease re-taken between the sweep's scan and its clear survives: the
    /// clear asserts the value the scan read.
    #[tokio::test]
    async fn the_sweep_keeps_a_lease_retaken_after_its_scan() {
        let store = EphemeralStore::open();
        write_raw(&store, 12, "retaken", expired_value(5)).await;
        let expired = collect_expired_uid_leases(&store, now()).await.unwrap();
        assert_eq!(expired.len(), 1);
        let holder = ZaUidLease::acquire(&store, 12, "retaken")
            .await
            .unwrap()
            .expect("the expired lease is taken over");
        assert_eq!(clear_uid_leases(&store, expired).await.unwrap(), 0);
        assert!(
            commit_fenced(&store, &holder).await.is_ok(),
            "the sweep removed the new holder's lease"
        );
        assert!(holder.release(&store).await);
    }
```

- [ ] **Step 5: Run the lease tests**

Run: `export PATH="/opt/homebrew/opt/rustup/bin:$PATH" && cargo test -p common za::lease`
Expected: 8 passed (6 existing, 2 new). `Option::is_none_or` is stable since Rust 1.82; if the toolchain rejects it, write `map_or(true, |expiry| expiry <= now)`.

- [ ] **Step 6: The sweep in the maintenance task**

In `crates/services/src/task_manager/maintenance.rs`, in the `TaskStoreMaintenanceType::PurgeData` arm, right after the `purge_in_memory_store()` call (line 220-224) and before `purge_dead_nodes()`, insert:

```rust
            // Zero-access fork, invitations spec 6.3 step 4: the generic purge
            // above skips UID creation leases (`KV_LOCK_ZA_UID`), so this
            // asserting sweep is their only cleanup.
            common::za::lease::sweep_expired_uid_leases(server.store())
                .await
                .caused_by(trc::location!())?;
```

Run the product compile check (Global Constraints) and expect `clean`.

- [ ] **Step 7: The hooks**

In `crates/common/src/za/test_hooks.rs`, replace the last line of the module doc ("Plan 10 adds the drain pause and the drain skip here.") with:

```rust
//! - `DRAIN_LOOKUP_PAUSE`: in the drain's import, after the first UID
//!   lookup found nothing and before the creation lease, keyed by the
//!   draining account (`groupware::calendar::drain`).
//! - `DRAIN_LEASE_PAUSE`: in the drain's import, holding the lease, after
//!   the second lookup and immediately before the commit, keyed by the
//!   draining account.
//! - `DRAIN_FAIL_ONCE`: the account's next drain commit fails with an
//!   internal error (a transient item error, spec 6.5).
//! - The drain skip: while an account is listed (or `DRAIN_SKIP_ALL` is),
//!   its drain does not run; the za suite lists every account after setup
//!   and the drain modules clear it for their duration.
//! - The drain retry override: stands in for `DRAIN_RETRY_SECS`.
```

Change the atomics import to `atomic::{AtomicI64, AtomicU64, Ordering}` and add after `COMMIT_FAIL_ONCE`:

```rust
pub static DRAIN_LOOKUP_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static DRAIN_LEASE_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

/// The account whose next drain commit fails.
pub static DRAIN_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

/// Accounts whose drain does not run; `DRAIN_SKIP_ALL` stands for every
/// account.
static DRAIN_SKIP: Mutex<Vec<u32>> = Mutex::new(Vec::new());

pub const DRAIN_SKIP_ALL: u32 = u32::MAX;

/// Seconds before a pass that left an item pending is retried; `-1` unset.
static DRAIN_RETRY_OVERRIDE: AtomicI64 = AtomicI64::new(-1);
```

Add after `lease_secs_override`:

```rust
pub fn set_drain_lookup_pause(pause: Option<Arc<Pause>>) {
    set_slot(&DRAIN_LOOKUP_PAUSE, pause);
}

pub fn set_drain_lease_pause(pause: Option<Arc<Pause>>) {
    set_slot(&DRAIN_LEASE_PAUSE, pause);
}

pub fn set_drain_fail_once(account_id: Option<u32>) {
    *DRAIN_FAIL_ONCE.lock().unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn set_drain_skip(account_id: u32) {
    let mut skipped = DRAIN_SKIP.lock().unwrap_or_else(|e| e.into_inner());
    if !skipped.contains(&account_id) {
        skipped.push(account_id);
    }
}

pub fn clear_drain_skip(account_id: u32) {
    DRAIN_SKIP
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|skipped| *skipped != account_id);
}

/// Whether `account_id`'s drain is skipped: listed itself, or everyone is.
pub fn drain_skipped(account_id: u32) -> bool {
    let skipped = DRAIN_SKIP.lock().unwrap_or_else(|e| e.into_inner());
    skipped.contains(&account_id) || skipped.contains(&DRAIN_SKIP_ALL)
}

pub fn set_drain_retry_override(secs: Option<u64>) {
    DRAIN_RETRY_OVERRIDE.store(secs.map_or(-1, |secs| secs as i64), Ordering::SeqCst);
}

pub fn drain_retry_override() -> Option<u64> {
    let secs = DRAIN_RETRY_OVERRIDE.load(Ordering::SeqCst);
    (secs >= 0).then_some(secs as u64)
}
```

And after `task_consumer_pause_point`:

```rust
pub async fn drain_lookup_pause_point(account_id: u32) {
    wait(take_for(&DRAIN_LOOKUP_PAUSE, account_id)).await;
}

pub async fn drain_lease_pause_point(account_id: u32) {
    wait(take_for(&DRAIN_LEASE_PAUSE, account_id)).await;
}

/// Whether the drain commit of `account_id` must fail now; clears the flag.
pub fn take_drain_fail(account_id: u32) -> bool {
    let mut slot = DRAIN_FAIL_ONCE.lock().unwrap_or_else(|e| e.into_inner());
    if *slot == Some(account_id) {
        *slot = None;
        true
    } else {
        false
    }
}
```

- [ ] **Step 8: The test helpers**

Append to `tests/src/utils/za.rs` (add `use types::field::CalendarEventField;` and `use hyper::Method;` to its imports if absent; `SyncCollection`, `Collection`, `HttpRequest`, `json` and `EVENT_NOTIFICATION_ZA_PENDING` are already imported there or in reach):

```rust
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

/// Any DAV request of a key account drains its inbox first (invitations
/// spec 6.2): a PROPFIND of the account's own principal, through the raw
/// client (5 s), so a drain that waits on a lease or a pause is not cut off
/// by `DummyWebDavClient`'s 500 ms timeout. Resolves to the status.
pub fn trigger_drain(name: &str, secret: &str) -> impl Future<Output = u16> + Send + 'static + use<> {
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
    let path = resources
        .any_resource_path_by_id(ids.iter().next().unwrap())
        .expect("the copy is in the resource cache");
    resources.format_resource(path)
}
```

- [ ] **Step 9: The sweep through the maintenance task, and the suite's default skip**

Create `tests/src/za/sweep.rs`:

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 6.3 step 4 and 11: the maintenance task's lease sweep
//! removes an expired UID creation lease and keeps a live one, with the
//! default configuration (the in-memory store is the data store).

use crate::utils::{server::TestServer, za::za_uid_leases};
use ::registry::schema::{
    enums::TaskStoreMaintenanceType,
    structs::{Task, TaskStatus, TaskStoreMaintenance},
};
use common::za::{lease::ZaUidLease, test_hooks};
use std::time::Duration;

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access lease sweep test...");
    let admin = test.account("admin@example.com").clone();
    let key1 = test.account("key1@example.com").id().document_id();
    assert_eq!(za_uid_leases(test).await, 0, "a lease outlived an earlier module");

    // An expired lease (held, never released: a crashed holder) and a live one.
    test_hooks::set_lease_secs_override(Some(1));
    let expired = ZaUidLease::acquire(test.server.store(), key1, "za-sweep-expired")
        .await
        .unwrap()
        .expect("a free lease");
    test_hooks::set_lease_secs_override(None);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let live = ZaUidLease::acquire(test.server.store(), key1, "za-sweep-live")
        .await
        .unwrap()
        .expect("a free lease");
    assert_eq!(za_uid_leases(test).await, 2);

    // The purge, as the scheduler runs it (from code reading: the
    // `PurgeData` arm of `store_maintenance` runs `purge_store`,
    // `purge_in_memory_store` and then the sweep).
    admin
        .registry_create_object(Task::StoreMaintenance(TaskStoreMaintenance {
            maintenance_type: TaskStoreMaintenanceType::PurgeData,
            shard_index: None,
            status: TaskStatus::now(),
        }))
        .await;
    test.wait_for_tasks().await;
    assert_eq!(za_uid_leases(test).await, 1, "the sweep did not remove exactly the expired lease");
    assert!(live.release(test.server.store()).await, "the live lease releases");
    assert!(
        expired.release(test.server.store()).await,
        "an absent key counts as released"
    );
    assert_eq!(za_uid_leases(test).await, 0);
}
```

In `tests/src/za/mod.rs`: add `pub mod sweep;` (alphabetical), add `use common::za::test_hooks;` to the imports, insert after `test.insert_account(admin);`:

```rust
    // Plan 10: the drain runs on every DAV request of a key account. The
    // plan 8 and 9 modules read pending items at store level, so the suite
    // skips every account's drain; the drain modules turn it on (`DrainOn`).
    test_hooks::set_drain_skip(test_hooks::DRAIN_SKIP_ALL);
```

and add `sweep::test(&mut test).await;` after `uid_lease::test(&mut test).await;`.

- [ ] **Step 10: Compile, format, run the suite**

Run: `cargo check -p tests --tests` (test-mode), the product compile check, `cargo fmt -p vault -p common -p services -p tests -- --check`.
Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture`
Expected: passes; the sweep test prints its line and the suite ends with `assert_is_empty`. A hang in `wait_for_tasks` after the maintenance task means the `PurgeData` arm failed: read the server log for the error before changing the test.

- [ ] **Step 11: Commit**

```bash
git add crates/vault crates/common/src/za crates/services/src/task_manager/maintenance.rs tests/src/utils/za.rs tests/src/za/sweep.rs tests/src/za/mod.rs
git commit -m "Drain state, hooks and the UID lease sweep

Invitations spec 3.2, 6.2-6.3 and 11: SessionKeys remembers its drain's
cursor, deferral and lock; the maintenance task's purge step sweeps expired
UID creation leases with an assertion on each value read; the test-mode
hooks gain the drain's pauses, commit failure, skip and retry override.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```

### Task 2: The drain

**Files:**
- Create: `crates/groupware/src/calendar/drain.rs`
- Modify: `crates/groupware/src/calendar/mod.rs:7-14` (`pub mod drain;`)
- Modify: `crates/dav/src/request.rs:597-660` (`handle_dav_request`, before `dispatch_dav_request`)
- Create: `tests/src/za/drain.rs`
- Modify: `tests/src/za/mod.rs`

**Interfaces:**
- Consumes: Task 1's `SessionKeys::drain()`, `ZaDrainState`, hooks and helpers. Plan 8: `open_pending_item(&mut ICalendar, recipient_id, &Secret) -> Result<PendingCarriers, SealError>` (`PendingCarriers { pair: Option<TokenPairs>, reply_token: Option<Zeroizing<[u8; 16]>> }`; a bundle holds exactly one pair, already checked), `seal_drained_item(&mut ICalendar, &SessionKeys, owner_id)`, `archived_item_is_pending(&ArchivedCalendarEventNotification)`, `Server::za_private_key(&keys) -> trc::Result<&Secret>` (a recorded failure was traced once; a store error was not and is not recorded), `seal_event(&mut CalendarEvent, &SessionKeys, account_id, Option<&TokenPairs>)`, `unseal_event_pairs(&mut CalendarEvent, &SessionKeys, account_id) -> Result<Option<TokenPairs>, SealError>`, `TokenPairs::{insert, matches, to_vec (crate), get_or_insert}`. Plan 9: `ZaUidLease::acquire(store, account_id, uid) -> trc::Result<Option<ZaUidLease>>`, `ZaUidLeaseGuard::{new, lease, release, abandon}`, `ZaUidLease::assert_in(&mut BatchBuilder)`; `Server::za_fan_out(&AccountInfo, &SessionKeys, Vec<ItipMessage<ICalendar>>, Option<TokenPairs>, &ICalendar) -> trc::Result<ZaFanOutResult { pairs, statuses }>`, `za_set_schedule_status`; `Server::za_account_id_from_email(&str) -> trc::Result<Option<u32>>` (quiet). Upstream: `itip_snapshot(&ICalendar, &[String], bool) -> Result<ItipSnapshots, ItipError>` (`ItipSnapshots { organizer: Organizer { email: Email { email, is_local }, .. }, uid, components: AHashMap<InstanceId, ItipSnapshot { attendees: AHashSet<Attendee { email, .. }>, sequence: Option<i64>, .. }> }`), `itip_process_message(&ICalendar, ItipSnapshots, &ICalendar, ItipSnapshots, String) -> Result<MergeResult, ItipError>`, `itip_merge_changes`, `itip_import_message`, `itip_method`, `ical_size`, `organizer_request_full(&ICalendar, &ItipSnapshots, Option<&mut Vec<u16>>, bool) -> Result<Vec<ItipMessage<ICalendar>>, ItipError>` (`pub(crate)`), `CalendarEventData::new_for(ical, Tz, max_expansions, &mut Option<CalendarAlarm>, key_account: bool)`, `CalendarEvent::{insert, update}`, `CalendarEventNotification`, `DestroyArchive<Archive<&ArchivedCalendarEventNotification>>::delete(changed_by, account_id, document_id, &mut batch)`, `Server::{fetch_dav_resources, get_or_create_default_calendar}` (`GroupwareCache`), `Server::itip_ids` (`ItipAutoExpunge`), `Server::document_ids_matching`, `Server::has_available_quota`, `Server::account_info`, `ObjectIndexBuilder`.
- Produces: `groupware::calendar::drain::{ZaDrain, DRAIN_RETRY_SECS}`; the test module's `DrainOn` guard and helpers `body`, `pending_item_count` for Tasks 3-5.

- [ ] **Step 1: The drain module**

Create `crates/groupware/src/calendar/drain.rs`:

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 6: the drain. A key account's own DAV request, which
//! holds the account's session keys, opens the pending items of the
//! account's scheduling inbox before the request's own work (6.1), so no
//! background code needs a key. A pass (6.2-6.3) takes the session's lock,
//! reads the inbox change id (the snapshot), enumerates the items whose
//! pending bit is set in document id order, unwraps the private key once
//! (3.2), and for each item: opens the `pk` wrap (the only place one is
//! opened, invariant 11), checks the sender (6.4 step 2), finds the event
//! by UID, imports under the creation lease or merges under the event's
//! archive assertion, adopts or keeps token pairs (3.3), authenticates a
//! REPLY by its token and rejects a stale one, fans out the organizer's
//! REQUEST to delegates the merge added, and rewrites the item resealed
//! under the master key with the pending bit cleared (6.4 step 5). Each
//! item is one store write holding the event write and the item rewrite
//! (its batch stays far below `BatchBuilder::is_large_batch`, the only
//! point at which a commit point splits a batch).
//! Upstream's pure iTIP functions run unchanged (invariant 15); only the
//! commit is the fork's. The drain never fails the request that triggered
//! it (invariant 14): `za_drain` traces and returns.
//!
//! Item errors (6.5): a transient one (a store error, a lost assertion,
//! the lease's fence included) aborts the pass and the item stays pending;
//! a busy creation lease leaves that item pending and the pass goes on; a
//! permanent one (the wrap does not open, a stray carrier, an unparsable
//! tree, an unknown METHOD) deletes the item, since nothing can ever read
//! it; a token mismatch deletes the item with a security trace; an
//! iTIP-level rejection keeps the item as history, resealed, bit cleared.
//! A pass that leaves any item pending does not advance the cursor and
//! defers the next pass by `DRAIN_RETRY_SECS`.

use crate::{
    DestroyArchive,
    cache::GroupwareCache,
    calendar::{
        CalendarEvent, CalendarEventData, CalendarEventNotification, ChangedBy,
        EVENT_NOTIFICATION_IS_CHANGE,
        fanout::{ZaFanOut, za_set_schedule_status},
        seal::{
            SealError, TOKEN_LEN, TokenPairs, archived_item_is_pending, open_pending_item,
            seal_drained_item, seal_error, seal_event, unseal_event_pairs,
        },
        storage::ItipAutoExpunge,
    },
    scheduling::{
        InstanceId, ItipError, ItipSnapshots, ical_size,
        inbound::{
            MergeAction, MergeResult, itip_import_message, itip_merge_changes, itip_method,
            itip_process_message,
        },
        organizer::organizer_request_full,
        snapshot::itip_snapshot,
    },
};
use ahash::AHashSet;
use calcard::{
    common::timezone::Tz,
    icalendar::{ICalendar, ICalendarMethod},
};
use common::{
    DavName, Server,
    auth::AccountInfo,
    storage::index::ObjectIndexBuilder,
    za::lease::{ZaUidLease, ZaUidLeaseGuard},
};
use std::{future::Future, sync::Arc};
use store::{
    ValueKey, rand,
    write::{AlignedBytes, Archive, BatchBuilder, now},
};
use trc::AddContext;
use types::{
    collection::{Collection, SyncCollection},
    field::CalendarEventField,
};
use vault::session::SessionKeys;
use zeroize::Zeroizing;

/// Spec 6.2: a pass that left an item pending is not retried before this
/// many seconds have passed. A constant with a test-mode override, not a
/// setting (spec 13).
pub const DRAIN_RETRY_SECS: u64 = 30;

pub trait ZaDrain: Sync + Send {
    /// Spec 6.1: drains the inbox of the account whose session keys these
    /// are, before the request's own work. Never fails the request
    /// (invariant 14): errors are traced here.
    fn za_drain(&self, keys: &Arc<SessionKeys>) -> impl Future<Output = ()> + Send;
}

impl ZaDrain for Server {
    async fn za_drain(&self, keys: &Arc<SessionKeys>) {
        #[cfg(feature = "test_mode")]
        if common::za::test_hooks::drain_skipped(keys.account_id) {
            return;
        }
        if let Err(err) = drain_pass(self, keys).await {
            trc::error!(
                err.account_id(keys.account_id)
                    .details("zero-access: drain pass failed")
                    .caused_by(trc::location!())
            );
        }
    }
}

fn drain_retry_secs() -> u64 {
    #[cfg(feature = "test_mode")]
    if let Some(secs) = common::za::test_hooks::drain_retry_override() {
        return secs;
    }
    DRAIN_RETRY_SECS
}

/// Spec 6.2-6.3: one pass under the session's lock. Nothing runs while a
/// deferral is in force or while the cursor is at the inbox's change id.
async fn drain_pass(server: &Server, keys: &Arc<SessionKeys>) -> trc::Result<()> {
    let account_id = keys.account_id;
    let state = keys.drain();
    let _pass = state.lock().await;
    let now = now();
    if state.retry_after() > now {
        return Ok(());
    }
    // The snapshot: the inbox change id observed before enumeration, from
    // the same since-query upstream runs for a request's own collection.
    let snapshot = server
        .fetch_dav_resources(
            account_id,
            account_id,
            SyncCollection::CalendarEventNotification,
        )
        .await
        .caused_by(trc::location!())?
        .highest_change_id;
    if state.cursor().is_some_and(|cursor| cursor >= snapshot) {
        return Ok(());
    }
    let mut pending = Vec::new();
    for document_id in server
        .itip_ids(account_id)
        .await
        .caused_by(trc::location!())?
    {
        if let Some(stored) = read_item(server, account_id, document_id).await?
            && archived_item_is_pending(
                stored
                    .unarchive::<CalendarEventNotification>()
                    .caused_by(trc::location!())?,
            )
        {
            pending.push(document_id);
        }
    }
    if pending.is_empty() {
        state.complete(snapshot);
        return Ok(());
    }
    match drain_items(server, keys, pending).await {
        Ok(true) => {
            state.complete(snapshot);
            Ok(())
        }
        Ok(false) => {
            state.defer_until(now + drain_retry_secs());
            Ok(())
        }
        Err(err) => {
            state.defer_until(now + drain_retry_secs());
            Err(err)
        }
    }
}

async fn read_item(
    server: &Server,
    account_id: u32,
    document_id: u32,
) -> trc::Result<Option<Archive<AlignedBytes>>> {
    server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEventNotification,
            document_id,
        ))
        .await
        .caused_by(trc::location!())
}

/// Spec 6.4: the private key once, then each item in document id order.
/// `Ok(true)` when every item is done; `Ok(false)` when one stays pending
/// without an error (a busy creation lease); `Err` on a transient error,
/// which aborts the pass (6.5).
async fn drain_items(
    server: &Server,
    keys: &Arc<SessionKeys>,
    pending: Vec<u32>,
) -> trc::Result<bool> {
    let account_id = keys.account_id;
    let private_key = match server.za_private_key(keys).await {
        Ok(key) => key,
        // A recorded failure (the wrap does not open, the record is missing
        // or unusable) was traced once for the session and the drain is
        // skipped (spec 10); a store error is not recorded, so it is traced
        // by the caller and the next pass retries it.
        Err(err) => {
            return if keys.private_key().is_some() {
                Ok(false)
            } else {
                Err(err)
            };
        }
    };
    let account_info = server
        .account_info(account_id)
        .await
        .caused_by(trc::location!())?;
    let mut all_done = true;
    for document_id in pending {
        if !drain_item(server, &account_info, keys, private_key, document_id).await? {
            all_done = false;
        }
    }
    Ok(all_done)
}

/// One pending item: what the rewrite, the deletion and the commit need.
struct ItemCtx<'a> {
    server: &'a Server,
    account_info: &'a AccountInfo,
    keys: &'a SessionKeys,
    /// The stored archive as read; every write asserts it.
    stored: Archive<AlignedBytes>,
    document_id: u32,
    changed_by: ChangedBy,
    created: i64,
    size: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    Request,
    Cancel,
    Reply,
}

/// What a merge applies.
enum Incoming {
    /// The recipient's pair from the item (spec 3.3, "Attendee side").
    Request(Option<TokenPairs>),
    Cancel,
    /// The replier's token (spec 3.3, "Verification").
    Reply(Zeroizing<[u8; TOKEN_LEN]>),
}

/// Spec 6.4 steps 1-5 for one item. `Ok(true)`: done (rewritten or
/// deleted); `Ok(false)`: left pending (the lease was busy).
async fn drain_item(
    server: &Server,
    account_info: &AccountInfo,
    keys: &SessionKeys,
    private_key: &vault::keys::Secret,
    document_id: u32,
) -> trc::Result<bool> {
    let account_id = keys.account_id;
    // Re-read (spec 6.3, "The pass"): an item that is gone, or that another
    // session drained, is skipped.
    let Some(stored) = read_item(server, account_id, document_id).await? else {
        return Ok(true);
    };
    let archived = stored
        .unarchive::<CalendarEventNotification>()
        .caused_by(trc::location!())?;
    if !archived_item_is_pending(archived) {
        return Ok(true);
    }
    let mut item = stored
        .deserialize::<CalendarEventNotification>()
        .caused_by(trc::location!())?;
    let ctx = ItemCtx {
        server,
        account_info,
        keys,
        stored,
        document_id,
        changed_by: item.changed_by.clone(),
        created: item.created,
        size: item.size,
    };
    // Step 1: open. A failure is permanent (spec 6.5): the private key
    // never changes, so nothing will ever read this item.
    let carriers = match open_pending_item(&mut item.event, account_id, private_key) {
        Ok(carriers) => carriers,
        Err(err) => return ctx.delete_undecodable(seal_reason(&err)).await,
    };
    let tree = item.event;
    // An unknown or missing METHOD cannot come from the fan-out: permanent.
    let method = match itip_method(&tree) {
        Ok(ICalendarMethod::Request) => Method::Request,
        Ok(ICalendarMethod::Cancel) => Method::Cancel,
        Ok(ICalendarMethod::Reply) => Method::Reply,
        Ok(_) => return ctx.delete_undecodable("UnsupportedMethod").await,
        Err(_) => return ctx.delete_undecodable("MissingMethod").await,
    };
    let (uid, candidates) = match itip_snapshot(&tree, account_info.addresses(), false) {
        Ok(snapshots) => (snapshots.uid.to_string(), sender_candidates(&snapshots, method)),
        Err(err) => return ctx.reject(tree, None, &err).await,
    };
    // Step 2: the sending address must resolve to the principal in
    // `changed_by`, the check upstream's ingest makes against a mail's
    // sender. A REQUEST or CANCEL names one candidate, its ORGANIZER; a
    // REPLY names every ATTENDEE it carries, since upstream's attendee
    // builder lists the replier together with its delegates and
    // delegators, so the sender's addresses are the candidates that
    // resolve to `changed_by`. The lookup is the quiet one (spec 10).
    let mut sender_addresses = Vec::new();
    if let ChangedBy::PrincipalId(sender_id) = &ctx.changed_by {
        for address in candidates {
            if server
                .za_account_id_from_email(&address)
                .await
                .caused_by(trc::location!())?
                == Some(*sender_id)
            {
                sender_addresses.push(address);
            }
        }
    }
    if sender_addresses.is_empty() {
        return ctx
            .reject(tree, None, &ItipError::SenderIsNotOrganizerNorAttendee)
            .await;
    }
    // Step 3, by METHOD. The UID lookup is upstream's (the fresh index);
    // the lowest document id is the copy that merges, as `itip_ingest`
    // takes the first hit.
    let existing = find_event(server, account_id, &uid).await?;
    match (method, existing) {
        (Method::Request, None) => {
            import(&ctx, tree, carriers.pair, &uid, sender_addresses).await
        }
        (Method::Request, Some(event_id)) => {
            merge(&ctx, event_id, tree, Incoming::Request(carriers.pair), sender_addresses).await
        }
        (Method::Cancel, Some(event_id)) => {
            merge(&ctx, event_id, tree, Incoming::Cancel, sender_addresses).await
        }
        (Method::Reply, Some(event_id)) => match carriers.reply_token {
            Some(token) => {
                merge(&ctx, event_id, tree, Incoming::Reply(token), sender_addresses).await
            }
            // A fan-out REPLY always carries its token; one without cannot
            // be authenticated.
            None => ctx.drop_forgery().await,
        },
        (Method::Cancel | Method::Reply, None) => {
            ctx.reject(tree, None, &ItipError::EventNotFound).await
        }
    }
}

async fn find_event(server: &Server, account_id: u32, uid: &str) -> trc::Result<Option<u32>> {
    Ok(server
        .document_ids_matching(
            account_id,
            Collection::CalendarEvent,
            CalendarEventField::Uid,
            uid.as_bytes(),
        )
        .await
        .caused_by(trc::location!())?
        .iter()
        .next())
}

/// The addresses an item may have been sent from (spec 6.4 step 2): the
/// ORGANIZER of a REQUEST or CANCEL; every ATTENDEE of a REPLY, because
/// upstream's attendee builder exports the replier's entry together with
/// its delegates' and delegators' (`attendee.rs`, `attendee_entry_uids`)
/// and a set's iteration order says nothing about which one replied.
/// Deduplicated case-insensitively, first occurrence kept.
fn sender_candidates(snapshots: &ItipSnapshots<'_>, method: Method) -> Vec<String> {
    match method {
        Method::Reply => {
            let mut seen = AHashSet::new();
            snapshots
                .components
                .values()
                .flat_map(|instance| instance.attendees.iter())
                .map(|attendee| attendee.email.email.clone())
                .filter(|address| seen.insert(address.to_ascii_lowercase()))
                .collect()
        }
        Method::Request | Method::Cancel => vec![snapshots.organizer.email.email.clone()],
    }
}

/// Spec 6.4, REPLY: stale when any instance of the reply carries a
/// SEQUENCE below the event's current SEQUENCE for the same instance
/// (absent counts as 0). An occurrence the event stores no override for
/// (a decline by EXDATE, which upstream's merge adds as a new override) is
/// judged against the master, whose SEQUENCE a reschedule bumps. Upstream's
/// `handle_reply` makes no such check.
fn is_stale_reply(event: &ItipSnapshots<'_>, reply: &ItipSnapshots<'_>) -> bool {
    reply.components.iter().any(|(instance_id, instance)| {
        event
            .components
            .get(instance_id)
            .or_else(|| event.components.get(&InstanceId::Main))
            .is_some_and(|current| {
                instance.sequence.unwrap_or(0) < current.sequence.unwrap_or(0)
            })
    })
}

/// Spec 6.4, REQUEST with no event: upstream's import (`itip_ingest`) with
/// the commit replaced, under the creation lease (6.3). A local sender
/// always adds, so `itip_auto_add` does not apply.
async fn import(
    ctx: &ItemCtx<'_>,
    tree: ICalendar,
    pair: Option<TokenPairs>,
    uid: &str,
    sender_addresses: Vec<String>,
) -> trc::Result<bool> {
    let server = ctx.server;
    let account_id = ctx.keys.account_id;
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::drain_lookup_pause_point(account_id).await;
    // Spec 6.3 step 1. A lease that stays held through the retries leaves
    // the item pending for a later pass.
    let Some(lease) = ZaUidLease::acquire(server.store(), account_id, uid)
        .await
        .caused_by(trc::location!())?
    else {
        trc::event!(
            Store(trc::StoreEvent::AssertValueFailed),
            AccountId = account_id,
            DocumentId = ctx.document_id,
            Details = "zero-access: drain creation lease busy",
        );
        return Ok(false);
    };
    // Dropped on an early return, the guard releases from a spawned task.
    let lease = ZaUidLeaseGuard::new(server.store().clone(), lease);
    // Under the lease the UID may have appeared (another session's drain
    // or PUT): merge instead.
    if let Some(event_id) = find_event(server, account_id, uid).await? {
        lease.release().await;
        return merge(ctx, event_id, tree, Incoming::Request(pair), sender_addresses).await;
    }
    let mut event_tree = tree.clone();
    if let Err(err) = itip_import_message(&mut event_tree) {
        lease.release().await;
        return ctx.reject(tree, None, &err).await;
    }
    // Upstream's two checks: quota for the plaintext size (the item's, as
    // the fan-out computed it) and the default calendar. Only a quota
    // limit is the iTIP-level rejection; a failed counter read is a store
    // error that keeps the item pending (spec 6.5), as the fan-out
    // classifies it.
    if let Err(err) = server
        .has_available_quota(
            server
                .account(account_id)
                .await
                .caused_by(trc::location!())?
                .as_ref(),
            ctx.size as u64,
        )
        .await
    {
        lease.release().await;
        return if is_over_quota(&err) {
            ctx.reject(tree, None, &ItipError::QuotaExceeded).await
        } else {
            Err(err.caused_by(trc::location!()))
        };
    }
    let Some(parent_id) = server
        .get_or_create_default_calendar(account_id, account_id)
        .await
        .caused_by(trc::location!())?
    else {
        lease.release().await;
        return ctx.reject(tree, None, &ItipError::NoDefaultCalendar).await;
    };
    let mut next_email_alarm = None;
    let now = now() as i64;
    let mut event = CalendarEvent {
        names: vec![DavName {
            name: format!("{}_{}.ics", now, rand::random::<u64>()),
            parent_id,
        }],
        data: CalendarEventData::new_for(
            event_tree,
            Tz::Floating,
            server.core.groupware.max_ical_instances,
            &mut next_email_alarm,
            true,
        ),
        size: ctx.size,
        schedule_tag: Some(1),
        ..Default::default()
    };
    // Spec 3.3, "Attendee side": the copy takes the pair the item carried.
    seal_event(&mut event, ctx.keys, account_id, pair.as_ref())
        .map_err(|err| seal_error(err, account_id, u32::MAX))?;
    let document_id = server
        .store()
        .assign_document_ids(account_id, Collection::CalendarEvent, 1)
        .await
        .caused_by(trc::location!())?;
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::drain_lease_pause_point(account_id).await;
    let mut batch = BatchBuilder::new();
    // Spec 6.3 step 3: fenced on the lease, with the item rewrite ahead of
    // the insert (one write; see the module doc).
    lease.lease().assert_in(&mut batch);
    ctx.rewrite_in(&mut batch, tree, Some(document_id), 0)?;
    event
        .insert(
            ctx.account_info.account_tenant_ids(),
            account_id,
            document_id,
            next_email_alarm,
            &mut batch,
        )
        .caused_by(trc::location!())?;
    ctx.commit(batch, Some(lease)).await
}

enum Step {
    Reject(ItipError),
    Merge(Vec<MergeAction>),
    Nothing,
}

/// Spec 6.4, an event exists: upstream's merge (`commit_itip_merge`'s
/// rules) on the unsealed copy, committed sealed under the assertion on
/// the event's stored archive, with the fork's checks around it.
async fn merge(
    ctx: &ItemCtx<'_>,
    event_id: u32,
    tree: ICalendar,
    incoming: Incoming,
    sender_addresses: Vec<String>,
) -> trc::Result<bool> {
    let server = ctx.server;
    let account_id = ctx.keys.account_id;
    let addresses = ctx.account_info.addresses();
    let Some(archive) = server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEvent,
            event_id,
        ))
        .await
        .caused_by(trc::location!())?
    else {
        return ctx.reject(tree, None, &ItipError::EventNotFound).await;
    };
    let event_ = archive
        .to_unarchived::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let mut event = event_
        .deserialize::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let mut pairs = unseal_event_pairs(&mut event, ctx.keys, account_id)
        .map_err(|err| seal_error(err, account_id, event_id))?;
    // Spec 3.3, "Verification": a REPLY is authenticated by the replier's
    // token in this event's bundle, matched by address. The replier is the
    // sender's address whose pair holds the token; an alias of the sender
    // with no pair, or a token that matches none, is a forgery.
    let sender_address = match &incoming {
        Incoming::Reply(token) => match sender_addresses.iter().find(|address| {
            pairs
                .as_ref()
                .is_some_and(|pairs| pairs.matches(address, token))
        }) {
            Some(address) => address.clone(),
            None => return ctx.drop_forgery().await,
        },
        Incoming::Request(_) | Incoming::Cancel => sender_addresses[0].clone(),
    };
    let (step, is_organizer_update, before) = {
        let snapshots = match itip_snapshot(&event.data.event, addresses, false) {
            Ok(snapshots) => snapshots,
            Err(err) => return ctx.reject(tree, Some(event_id), &err).await,
        };
        let itip_snapshots = match itip_snapshot(&tree, addresses, false) {
            Ok(snapshots) => snapshots,
            Err(err) => return ctx.reject(tree, Some(event_id), &err).await,
        };
        let is_organizer_update = !itip_snapshots.organizer.email.is_local;
        // The attendees before the merge: those the merge adds are
        // delegates (spec 6.4, "Delegates").
        let before: AHashSet<String> = snapshots
            .components
            .values()
            .flat_map(|instance| instance.attendees.iter())
            .map(|attendee| attendee.email.email.to_ascii_lowercase())
            .collect();
        let step = if matches!(incoming, Incoming::Reply(_))
            && is_stale_reply(&snapshots, &itip_snapshots)
        {
            Step::Reject(ItipError::OutOfSequence)
        } else {
            match itip_process_message(
                &event.data.event,
                snapshots,
                &tree,
                itip_snapshots,
                sender_address.clone(),
            ) {
                Ok(MergeResult::Actions(changes)) => Step::Merge(changes),
                Ok(MergeResult::None) => Step::Nothing,
                // Only REFRESH yields a message; it cannot come from the
                // fan-out (spec 6.4 step 3).
                Ok(MergeResult::Message(_)) => {
                    Step::Reject(ItipError::UnsupportedMethod(ICalendarMethod::Refresh))
                }
                Err(err) => Step::Reject(err),
            }
        };
        (step, is_organizer_update, before)
    };
    let changes = match step {
        Step::Reject(err) => return ctx.reject(tree, Some(event_id), &err).await,
        Step::Nothing => return ctx.rewrite_only(tree, Some(event_id), 0).await,
        Step::Merge(changes) => changes,
    };
    itip_merge_changes(&mut event.data.event, changes);
    event.size = ical_size(&event.data.event) as u32;
    if event.size > server.core.groupware.max_ical_size as u32 {
        return ctx.reject(tree, Some(event_id), &ItipError::EventTooLarge).await;
    }
    let extra_bytes = (event.size as u64).saturating_sub(event_.inner.size.to_native() as u64);
    if extra_bytes > 0
        && let Err(err) = server
            .has_available_quota(
                server
                    .account(account_id)
                    .await
                    .caused_by(trc::location!())?
                    .as_ref(),
                extra_bytes,
            )
            .await
    {
        return if is_over_quota(&err) {
            ctx.reject(tree, Some(event_id), &ItipError::QuotaExceeded).await
        } else {
            Err(err.caused_by(trc::location!()))
        };
    }
    // Spec 3.3: the pairs the copy keeps.
    match &incoming {
        // An accepted REQUEST replaces this attendee's pair with the
        // incoming one, so a removed and re-added attendee follows the
        // organizer to the new token.
        Incoming::Request(Some(pair)) => {
            let pairs = pairs.get_or_insert_with(TokenPairs::new);
            for (address, token) in pair.to_vec() {
                pairs.insert(&address, token);
            }
        }
        Incoming::Request(None) | Incoming::Cancel => {}
        // Delegates the merge added get their token from the organizer,
        // whose session this is, through the same fan-out a PUT uses:
        // upstream's organizer request builder on the merged tree,
        // restricted to the new attendees (spec 6.4, "Delegates").
        Incoming::Reply(_) => {
            let new_attendees = match itip_snapshot(&event.data.event, addresses, false) {
                Ok(after) => {
                    let new_attendees: AHashSet<String> = after
                        .components
                        .values()
                        .flat_map(|instance| instance.attendees.iter())
                        .map(|attendee| attendee.email.email.to_ascii_lowercase())
                        .filter(|address| !before.contains(address))
                        .collect();
                    if new_attendees.is_empty() {
                        Ok(None)
                    } else {
                        organizer_request_full(&event.data.event, &after, None, false)
                            .map(|messages| Some((new_attendees, messages)))
                    }
                }
                Err(err) => Err(err),
            };
            match new_attendees {
                Ok(Some((new_attendees, mut messages))) => {
                    for message in &mut messages {
                        message
                            .to
                            .retain(|to| new_attendees.contains(&to.to_ascii_lowercase()));
                    }
                    messages.retain(|message| !message.to.is_empty());
                    let fan_out = server
                        .za_fan_out(
                            ctx.account_info,
                            ctx.keys,
                            messages,
                            pairs.take(),
                            &event.data.event,
                        )
                        .await
                        .caused_by(trc::location!())?;
                    za_set_schedule_status(&mut event.data.event, &fan_out.statuses);
                    pairs = fan_out.pairs;
                }
                Ok(None) => {}
                // The reply still applies; the delegate's invitation is
                // not built. Traced as upstream traces a builder error.
                Err(err) => {
                    trc::event!(
                        Calendar(trc::CalendarEvent::ItipMessageError),
                        AccountId = account_id,
                        DocumentId = ctx.document_id,
                        Reason = itip_reason(&err),
                        Details = "zero-access: delegate invitation not built",
                    );
                }
            }
        }
    }
    let now = now() as i64;
    let prev_email_alarm = event_.inner.data.next_alarm(now, Tz::Floating);
    let mut next_email_alarm = None;
    event.data = CalendarEventData::new_for(
        event.data.event,
        Tz::Floating,
        server.core.groupware.max_ical_instances,
        &mut next_email_alarm,
        true,
    );
    if is_organizer_update {
        match &mut event.schedule_tag {
            Some(schedule_tag) => *schedule_tag += 1,
            None => event.schedule_tag = Some(1),
        }
    }
    seal_event(&mut event, ctx.keys, account_id, pairs.as_ref())
        .map_err(|err| seal_error(err, account_id, event_id))?;
    let mut batch = BatchBuilder::new();
    // The item rewrite first, ahead of the event update (one write; see
    // the module doc).
    ctx.rewrite_in(&mut batch, tree, Some(event_id), EVENT_NOTIFICATION_IS_CHANGE)?;
    event
        .update(
            ctx.account_info.account_tenant_ids(),
            event_,
            account_id,
            event_id,
            &mut batch,
        )
        .caused_by(trc::location!())?;
    if prev_email_alarm != next_email_alarm {
        if let Some(prev_alarm) = prev_email_alarm {
            prev_alarm.delete_task(&mut batch);
        }
        if let Some(next_alarm) = next_email_alarm {
            next_alarm.write_task(&mut batch);
        }
    }
    ctx.commit(batch, None).await
}

impl ItemCtx<'_> {
    /// Spec 6.4 step 5: the item resealed under the master key, bit
    /// cleared, no carrier, `event_id` and the change flag as upstream's
    /// ingest sets them, `created` kept. No commit point of its own: the
    /// callers put it ahead of the event write, or end the batch
    /// themselves.
    fn rewrite_in(
        &self,
        batch: &mut BatchBuilder,
        mut tree: ICalendar,
        event_id: Option<u32>,
        flags: u16,
    ) -> trc::Result<()> {
        let account_id = self.keys.account_id;
        seal_drained_item(&mut tree, self.keys, account_id)
            .map_err(|err| seal_error(err, account_id, self.document_id))?;
        let item = CalendarEventNotification {
            event: tree,
            event_id,
            changed_by: self.changed_by.clone(),
            flags,
            size: self.size,
            created: self.created,
            modified: now() as i64,
        };
        batch
            .with_account_id(account_id)
            .with_collection(Collection::CalendarEventNotification)
            .with_document(self.document_id)
            .custom(
                ObjectIndexBuilder::new()
                    .with_current(
                        self.stored
                            .to_unarchived::<CalendarEventNotification>()
                            .caused_by(trc::location!())?,
                    )
                    .with_changes(item)
                    .with_changed_by(self.account_info.account_tenant_ids()),
            )
            .caused_by(trc::location!())?;
        Ok(())
    }

    /// The rewrite alone (nothing to merge, or kept as history).
    async fn rewrite_only(
        &self,
        tree: ICalendar,
        event_id: Option<u32>,
        flags: u16,
    ) -> trc::Result<bool> {
        let mut batch = BatchBuilder::new();
        self.rewrite_in(&mut batch, tree, event_id, flags)?;
        batch.commit_point();
        self.commit(batch, None).await
    }

    /// Spec 6.4 step 4: an iTIP-level rejection leaves the calendar
    /// unchanged; the item is kept as history. The reason is a fixed name.
    async fn reject(
        &self,
        tree: ICalendar,
        event_id: Option<u32>,
        err: &ItipError,
    ) -> trc::Result<bool> {
        trc::event!(
            Calendar(trc::CalendarEvent::ItipMessageError),
            AccountId = self.keys.account_id,
            DocumentId = self.document_id,
            Reason = itip_reason(err),
            Details = "zero-access: inbox item kept as history",
        );
        self.rewrite_only(tree, event_id, 0).await
    }

    /// Spec 6.5, permanent: the item is deleted, since nothing can ever
    /// read it. `reason` is a fixed string.
    async fn delete_undecodable(&self, reason: &'static str) -> trc::Result<bool> {
        trc::event!(
            Calendar(trc::CalendarEvent::ItipMessageError),
            AccountId = self.keys.account_id,
            DocumentId = self.document_id,
            Reason = reason,
            Details = "zero-access: undecodable inbox item deleted",
        );
        self.delete().await
    }

    /// Spec 6.5, token mismatch: a traced drop of a forgery; the event is
    /// unchanged.
    async fn drop_forgery(&self) -> trc::Result<bool> {
        trc::event!(
            Security(trc::SecurityEvent::Unauthorized),
            AccountId = self.keys.account_id,
            DocumentId = self.document_id,
            Details = "zero-access: reply token mismatch",
        );
        self.delete().await
    }

    async fn delete(&self) -> trc::Result<bool> {
        let mut batch = BatchBuilder::new();
        DestroyArchive(
            self.stored
                .to_unarchived::<CalendarEventNotification>()
                .caused_by(trc::location!())?,
        )
        .delete(
            self.account_info.account_tenant_ids(),
            self.keys.account_id,
            self.document_id,
            &mut batch,
        )
        .caused_by(trc::location!())?;
        self.commit(batch, None).await
    }

    /// One store write per item. A lost assertion (the lease's fence, the
    /// item's or the event's archive) aborts the pass (spec 6.3); the
    /// lease, if held, is released either way.
    async fn commit(&self, batch: BatchBuilder, lease: Option<ZaUidLeaseGuard>) -> trc::Result<bool> {
        #[cfg(feature = "test_mode")]
        if common::za::test_hooks::take_drain_fail(self.keys.account_id) {
            if let Some(lease) = lease {
                lease.release().await;
            }
            return Err(trc::StoreEvent::UnexpectedError
                .into_err()
                .details("zero-access test hook: drain commit failure")
                .caused_by(trc::location!()));
        }
        let result = self.server.commit_batch(batch).await;
        if let Some(lease) = lease {
            lease.release().await;
        }
        match result {
            Ok(_) => Ok(true),
            Err(err) if err.is_assertion_failure() => Err(err
                .details("zero-access: drain commit lost its assertion")
                .caused_by(trc::location!())),
            Err(err) => Err(err.caused_by(trc::location!())),
        }
    }
}

/// Only a quota limit is "over quota", as `fanout::write_item_inner`
/// classifies it; any other error of the quota check is a store error.
fn is_over_quota(err: &trc::Error) -> bool {
    err.matches(trc::EventType::Limit(trc::LimitEvent::Quota))
        || err.matches(trc::EventType::Limit(trc::LimitEvent::TenantQuota))
}

/// A fixed name per `SealError`; its `Structure` text is a fixed string too.
fn seal_reason(err: &SealError) -> &'static str {
    match err {
        SealError::Structure(what) => what,
        _ => "unseal failure",
    }
}

/// A fixed name per `ItipError` variant: upstream traces `to_string()`,
/// whose `SenderIsNotParticipant` and `UnknownParticipant` texts carry an
/// address (spec 10).
fn itip_reason(err: &ItipError) -> &'static str {
    match err {
        ItipError::NoSchedulingInfo => "NoSchedulingInfo",
        ItipError::OtherSchedulingAgent => "OtherSchedulingAgent",
        ItipError::NotOrganizer => "NotOrganizer",
        ItipError::NotOrganizerNorAttendee => "NotOrganizerNorAttendee",
        ItipError::NothingToSend => "NothingToSend",
        ItipError::MissingUid => "MissingUid",
        ItipError::MultipleUid => "MultipleUid",
        ItipError::MultipleOrganizer => "MultipleOrganizer",
        ItipError::MultipleObjectTypes => "MultipleObjectTypes",
        ItipError::MultipleObjectInstances => "MultipleObjectInstances",
        ItipError::CannotModifyProperty(_) => "CannotModifyProperty",
        ItipError::CannotModifyInstance => "CannotModifyInstance",
        ItipError::CannotModifyAddress => "CannotModifyAddress",
        ItipError::OrganizerMismatch => "OrganizerMismatch",
        ItipError::MissingMethod => "MissingMethod",
        ItipError::InvalidComponentType => "InvalidComponentType",
        ItipError::OutOfSequence => "OutOfSequence",
        ItipError::OrganizerIsLocalAddress => "OrganizerIsLocalAddress",
        ItipError::SenderIsNotOrganizerNorAttendee => "SenderIsNotOrganizerNorAttendee",
        ItipError::SenderIsNotParticipant(_) => "SenderIsNotParticipant",
        ItipError::UnknownParticipant(_) => "UnknownParticipant",
        ItipError::UnsupportedMethod(_) => "UnsupportedMethod",
        ItipError::ICalendarParseError => "ICalendarParseError",
        ItipError::EventNotFound => "EventNotFound",
        ItipError::EventTooLarge => "EventTooLarge",
        ItipError::QuotaExceeded => "QuotaExceeded",
        ItipError::NoDefaultCalendar => "NoDefaultCalendar",
        ItipError::AutoAddDisabled => "AutoAddDisabled",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calcard::{Entry, Parser};

    fn parse(text: &str) -> ICalendar {
        match Parser::new(text).entry() {
            Entry::ICalendar(ical) => ical,
            other => panic!("{other:?}"),
        }
    }

    fn event(sequence: Option<i64>) -> ICalendar {
        let sequence = sequence.map_or(String::new(), |s| format!("SEQUENCE:{s}\r\n"));
        parse(&format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:stale-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\n{sequence}ORGANIZER:mailto:org@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:att@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        ))
    }

    fn reply(sequence: Option<i64>) -> ICalendar {
        let sequence = sequence.map_or(String::new(), |s| format!("SEQUENCE:{s}\r\n"));
        parse(&format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REPLY\r\nBEGIN:VEVENT\r\nUID:stale-1\r\nDTSTAMP:20240101T000000Z\r\n{sequence}ORGANIZER:mailto:org@example.com\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:att@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        ))
    }

    fn organizer() -> Vec<String> {
        vec!["org@example.com".to_string()]
    }

    #[test]
    fn a_reply_below_the_events_sequence_is_stale() {
        let org = organizer();
        let current = event(Some(1));
        let current = itip_snapshot(&current, &org, false).unwrap();
        for (reply_sequence, stale) in [
            (Some(0), true),
            (None, true),
            (Some(1), false),
            (Some(2), false),
        ] {
            let reply = reply(reply_sequence);
            assert_eq!(
                is_stale_reply(&current, &itip_snapshot(&reply, &org, false).unwrap()),
                stale,
                "{reply_sequence:?}"
            );
        }
        // An event without SEQUENCE counts as 0: nothing is stale.
        let unnumbered = event(None);
        let unnumbered = itip_snapshot(&unnumbered, &org, false).unwrap();
        let reply = reply(None);
        assert!(!is_stale_reply(
            &unnumbered,
            &itip_snapshot(&reply, &org, false).unwrap()
        ));
    }

    /// A delegation REPLY as upstream builds it: the replier's entry with
    /// its delegate's (`attendee_entry_uids`).
    fn delegation_reply() -> ICalendar {
        parse(
            "BEGIN:VCALENDAR\\r\\nVERSION:2.0\\r\\nPRODID:-//t//EN\\r\\nMETHOD:REPLY\\r\\nBEGIN:VEVENT\\r\\nUID:stale-1\\r\\nDTSTAMP:20240101T000000Z\\r\\nSEQUENCE:0\\r\\nORGANIZER:mailto:org@example.com\\r\\nATTENDEE;PARTSTAT=NEEDS-ACTION;DELEGATED-FROM=\\"mailto:att@example.com\\":mailto:del@example.com\\r\\nATTENDEE;PARTSTAT=DELEGATED;DELEGATED-TO=\\"mailto:del@example.com\\":mailto:att@example.com\\r\\nEND:VEVENT\\r\\nEND:VCALENDAR\\r\\n",
        )
    }

    #[test]
    fn sender_candidates_name_the_organizer_or_every_attendee() {
        let org = organizer();
        let request = event(Some(0));
        let snapshots = itip_snapshot(&request, &org, false).unwrap();
        assert_eq!(
            sender_candidates(&snapshots, Method::Request),
            ["org@example.com"]
        );
        assert_eq!(
            sender_candidates(&snapshots, Method::Cancel),
            ["org@example.com"]
        );
        let reply = reply(Some(0));
        let snapshots = itip_snapshot(&reply, &org, false).unwrap();
        assert_eq!(
            sender_candidates(&snapshots, Method::Reply),
            ["att@example.com"]
        );
        // Both the delegator and the delegate are candidates: which one
        // replied is decided by `changed_by` and the token, never by order.
        let delegation = delegation_reply();
        let snapshots = itip_snapshot(&delegation, &org, false).unwrap();
        let mut candidates = sender_candidates(&snapshots, Method::Reply);
        candidates.sort();
        assert_eq!(candidates, ["att@example.com", "del@example.com"]);
    }

    fn recurring_event(sequence: i64, with_override: Option<i64>) -> ICalendar {
        let override_ = with_override.map_or(String::new(), |s| format!(
            "BEGIN:VEVENT\\r\\nUID:stale-1\\r\\nRECURRENCE-ID:20990103T090000Z\\r\\nDTSTAMP:20240101T000000Z\\r\\nDTSTART:20990103T100000Z\\r\\nDTEND:20990103T110000Z\\r\\nSEQUENCE:{s}\\r\\nORGANIZER:mailto:org@example.com\\r\\nATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:att@example.com\\r\\nEND:VEVENT\\r\\n"
        ));
        parse(&format!(
            "BEGIN:VCALENDAR\\r\\nVERSION:2.0\\r\\nPRODID:-//t//EN\\r\\nBEGIN:VEVENT\\r\\nUID:stale-1\\r\\nDTSTAMP:20240101T000000Z\\r\\nDTSTART:20990102T090000Z\\r\\nDTEND:20990102T100000Z\\r\\nRRULE:FREQ=DAILY;COUNT=5\\r\\nSEQUENCE:{sequence}\\r\\nORGANIZER:mailto:org@example.com\\r\\nATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:att@example.com\\r\\nEND:VEVENT\\r\\n{override_}END:VCALENDAR\\r\\n"
        ))
    }

    fn occurrence_reply(sequence: i64) -> ICalendar {
        parse(&format!(
            "BEGIN:VCALENDAR\\r\\nVERSION:2.0\\r\\nPRODID:-//t//EN\\r\\nMETHOD:REPLY\\r\\nBEGIN:VEVENT\\r\\nUID:stale-1\\r\\nRECURRENCE-ID:20990103T090000Z\\r\\nDTSTAMP:20240101T000000Z\\r\\nSEQUENCE:{sequence}\\r\\nORGANIZER:mailto:org@example.com\\r\\nATTENDEE;PARTSTAT=DECLINED:mailto:att@example.com\\r\\nEND:VEVENT\\r\\nEND:VCALENDAR\\r\\n"
        ))
    }

    #[test]
    fn an_occurrence_reply_without_an_override_is_judged_against_the_master() {
        let org = organizer();
        // The master was rescheduled to SEQUENCE 1; the decline of an
        // occurrence at SEQUENCE 0 is stale although no override exists.
        let master_only = recurring_event(1, None);
        let master_only = itip_snapshot(&master_only, &org, false).unwrap();
        let old = occurrence_reply(0);
        assert!(is_stale_reply(&master_only, &itip_snapshot(&old, &org, false).unwrap()));
        let fresh = occurrence_reply(1);
        assert!(!is_stale_reply(&master_only, &itip_snapshot(&fresh, &org, false).unwrap()));
        // A stored override keeps its own SEQUENCE: at 0 while the master
        // is at 1, a reply at 0 for that occurrence is not stale.
        let with_override = recurring_event(1, Some(0));
        let with_override = itip_snapshot(&with_override, &org, false).unwrap();
        assert!(!is_stale_reply(&with_override, &itip_snapshot(&old, &org, false).unwrap()));
    }

    #[test]
    fn reasons_carry_no_address() {
        assert_eq!(
            itip_reason(&ItipError::SenderIsNotParticipant("secret@x.test".into())),
            "SenderIsNotParticipant"
        );
        assert_eq!(
            itip_reason(&ItipError::UnknownParticipant("secret@x.test".into())),
            "UnknownParticipant"
        );
        assert_eq!(seal_reason(&SealError::Structure("misplaced carrier")), "misplaced carrier");
        assert_eq!(seal_reason(&SealError::Aead), "unseal failure");
        assert_eq!(DRAIN_RETRY_SECS, 30);
    }
}
```

Add `pub mod drain;` to `crates/groupware/src/calendar/mod.rs` (alphabetical, after `dates`). `DestroyArchive` is imported the way `crates/dav/src/calendar/scheduling.rs` imports it; `ItipSnapshots` lives in `crate::scheduling`. `TokenPairs::to_vec` is `pub(crate)`, reachable here. If `Attendee`'s set iteration needs the `Hash` impl in scope, nothing else is required.

- [ ] **Step 2: Run the groupware unit tests**

Run: `export PATH="/opt/homebrew/opt/rustup/bin:$PATH" && cargo test -p groupware`
Expected: the three new tests pass with the existing ones. Fix type and borrow errors without changing behaviour; a `snapshots` borrow of `tree` that outlives a move of `tree` is resolved by scoping the snapshot in a block as `merge` does.

- [ ] **Step 3: The trigger**

In `crates/dav/src/request.rs`, `handle_dav_request`, after the headers are parsed (`for (key, value) in request.headers() { .. }`) and before `let start_time = Instant::now();`, insert:

```rust
        // Invitations spec 6.1-6.2: a key account's own request drains its
        // pending inbox items first. The token holds keys only for the
        // account that authenticated, so a request for another account's
        // data still drains the caller's own inbox. The drain never fails
        // the request (invariant 14).
        if let Some(keys) = access_token.za_keys_for(access_token.account_id()) {
            self.za_drain(keys).await;
        }
```

with `use groupware::calendar::drain::ZaDrain;` among the imports. Run the product compile check (expect `clean`) and `cargo check -p tests --tests`.

- [ ] **Step 4: The first drain tests**

Create `tests/src/za/drain.rs`:

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 6: the drain. A key account's own DAV request opens its
//! pending inbox items, applies them through upstream's merge and reseals
//! them; the flows of spec 4 and the cases of spec 11.

use super::{
    STRONG,
    dav_seal::raw_event,
    fanout::{
        CONTENT_TYPE, KEY1_CAL, NO_REPLY, Party, START_A, START_B, invite, stored_pairs,
        stored_sequence, uid_copies,
    },
};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        copy_href, envelope_wrap, inbox_ids, inbox_items, item_method, new_inbox_items,
        pending_ids, trigger_drain, za_uid_leases, za_unlock,
    },
};
use ::registry::schema::{prelude::Property, structs::CalendarScheduling};
use calcard::icalendar::ICalendarMethod;
use common::za::test_hooks::{self, DRAIN_SKIP_ALL};
use groupware::calendar::{
    CalendarEvent, ChangedBy, EVENT_NOTIFICATION_IS_CHANGE, EVENT_NOTIFICATION_ZA_PENDING,
};
use hyper::StatusCode;

/// The suite skips every account's drain (`za_tests` sets `DRAIN_SKIP_ALL`
/// after setup, so the plan 8 and 9 modules keep reading pending items);
/// a drain module turns it on for its duration.
pub struct DrainOn;

impl DrainOn {
    pub fn new() -> Self {
        test_hooks::clear_drain_skip(DRAIN_SKIP_ALL);
        DrainOn
    }
}

impl Drop for DrainOn {
    fn drop(&mut self) {
        test_hooks::set_drain_skip(DRAIN_SKIP_ALL);
    }
}

/// A GET as the owner's client reads it: 200, unsealed, no carrier.
pub async fn body(client: &DummyWebDavClient, href: &str) -> String {
    let body = client
        .request("GET", href, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(!body.contains("X-ZA-"), "{body}");
    body
}

/// `default/<name>` of a copy's href, for `raw_event` and `stored_pairs`.
pub fn relative(href: &str) -> String {
    format!("default/{}", href.rsplit('/').next().unwrap())
}

/// The stored item `document_id` of `account_id`.
pub async fn stored_item(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
) -> groupware::calendar::CalendarEventNotification {
    inbox_items(test, account_id)
        .await
        .into_iter()
        .find(|(id, _)| *id == document_id)
        .map(|(_, item)| item)
        .unwrap_or_else(|| panic!("item {document_id} is gone"))
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access drain tests...");
    let _drain = DrainOn::new();
    let admin = test.account("admin@example.com").clone();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let keys1 = za_unlock(test, key1.id, STRONG).await;
    let keys2 = za_unlock(test, key2.id, STRONG).await;
    let start1 = inbox_ids(test, key1.id).await;
    let start2 = inbox_ids(test, key2.id).await;
    assert_eq!(za_uid_leases(test).await, 0);

    // 1. The session's first request drains (spec 6.2, trigger 1). key1
    // invites key2; the item waits, pending, until key2's first DAV
    // request, a PROPFIND of key2's principal: any DAV request drains, not
    // only a calendar one (Review Focus 5).
    let uid = "za-drain-1";
    let organizer = format!("{KEY1_CAL}drain-1.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer,
            [CONTENT_TYPE],
            invite(uid, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let items = new_inbox_items(test, key2.id, &start2).await;
    assert_eq!(items.len(), 1);
    let item_id = items[0].0;
    assert_eq!(pending_ids(test, key2.id).await, [item_id]);
    assert_eq!(uid_copies(test, key2.id, uid).await, 0, "a copy before the drain");
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207); // (from code reading)
    assert!(
        pending_ids(test, key2.id).await.is_empty(),
        "the item is still pending"
    );
    assert_eq!(uid_copies(test, key2.id, uid).await, 1, "one imported copy");
    let item = stored_item(test, key2.id, item_id).await;
    assert_eq!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0);
    assert_eq!(
        item.flags & EVENT_NOTIFICATION_IS_CHANGE,
        0,
        "an import is not a change"
    );
    assert_eq!(item.changed_by, ChangedBy::PrincipalId(key1.id));
    assert_eq!(envelope_wrap(&item.event), Some(1), "resealed under the master key");
    assert!(item.event_id.is_some(), "the item names its event");
    assert!(
        !item.event.to_string().contains("fo-title"),
        "the drained item is sealed"
    );
    // The copy as key2's client reads it: the invitation, on the default
    // calendar, with a schedule tag, no METHOD.
    let href = copy_href(test, key2.id, uid).await;
    assert!(href.contains("/default/"), "{href}");
    let copy = body(&key2.client, &href).await;
    assert!(
        copy.contains(&format!("fo-title {uid}"))
            && copy.contains("ORGANIZER:mailto:key1@example.com")
            && copy.contains("mailto:key2@example.com")
            && !copy.contains("METHOD:"),
        "{copy}"
    );
    let rel = relative(&href);
    let (archive, _) = raw_event(test, key2.id, &rel).await;
    assert!(
        archive
            .unarchive::<CalendarEvent>()
            .unwrap()
            .schedule_tag
            .is_some()
    );
    // Spec 3.3, "Attendee side": the copy's bundle holds key2's pair, the
    // token key1 handed out, and nothing else.
    let key2_token = *stored_pairs(test, &keys1, key1.id, "default/drain-1.ics")
        .await
        .expect("key1's pairs")
        .get("key2@example.com")
        .expect("key2's pair");
    let copy_pairs = stored_pairs(test, &keys2, key2.id, &rel)
        .await
        .expect("the copy's pairs");
    assert_eq!(copy_pairs.len(), 1);
    assert_eq!(copy_pairs.get("key2@example.com"), Some(&key2_token));
    // The inbox as clients see it (spec 6.6): listed and readable, METHOD
    // intact.
    let listing = key2
        .client
        .request_with_headers("PROPFIND", &key2.inbox(), [("depth", "1")], "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let item_href = format!("{}{item_id}.ics", key2.inbox());
    assert!(
        listing.hrefs().contains(&item_href.as_str()),
        "{:?}",
        listing.hrefs()
    );
    let shown = body(&key2.client, &item_href).await;
    assert!(
        shown.contains("METHOD:REQUEST") && shown.contains(&format!("fo-title {uid}")),
        "{shown}"
    );
    assert_eq!(za_uid_leases(test).await, 0, "the import left its lease");

    // 2. Mid-session arrival (trigger 2): key2 is connected; the next
    // invitation is drained by key2's next request, a GET of the first copy.
    let uid2 = "za-drain-2";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-2.ics"),
            [CONTENT_TYPE],
            invite(uid2, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pending_ids(test, key2.id).await.len(), 1);
    body(&key2.client, &href).await;
    assert!(pending_ids(test, key2.id).await.is_empty());
    assert_eq!(uid_copies(test, key2.id, uid2).await, 1);

    // 3. Update (spec 4.2; Review Focus 3): key2 adds an alarm to its copy
    // (a PUT that builds no message), then key1 moves the event. The merge
    // keeps key2's alarm and takes the new time and SEQUENCE; the drained
    // item is a change.
    let with_alarm = copy.replace(
        "END:VEVENT",
        "BEGIN:VALARM\r\nTRIGGER:-PT10M\r\nACTION:DISPLAY\r\nDESCRIPTION:drain-alarm\r\nEND:VALARM\r\nEND:VEVENT",
    );
    key2.client
        .request_with_headers("PUT", &href, [CONTENT_TYPE], with_alarm)
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        new_inbox_items(test, key1.id, &start1).await.len(),
        0,
        "an alarm-only PUT of the copy built a reply"
    );
    key1.client
        .request_with_headers(
            "PUT",
            &organizer,
            [CONTENT_TYPE],
            invite(uid, START_B, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let update_item = pending_ids(test, key2.id).await;
    assert_eq!(update_item.len(), 1, "the update reached key2");
    let merged = body(&key2.client, &href).await;
    assert!(
        merged.contains(&format!("DTSTART:{START_B}")) && merged.contains("drain-alarm"),
        "{merged}"
    );
    assert_eq!(stored_sequence(&key2.client, &href).await, 1);
    assert_eq!(uid_copies(test, key2.id, uid).await, 1);
    let item = stored_item(test, key2.id, update_item[0]).await;
    assert_eq!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0);
    assert_ne!(item.flags & EVENT_NOTIFICATION_IS_CHANGE, 0, "a merge is a change");

    // 4. Cancel (spec 4.3): key1's DELETE sends a CANCEL; key2's copy
    // becomes STATUS:CANCELLED and stays on the calendar.
    key1.client
        .request("DELETE", &organizer, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let cancel_item = pending_ids(test, key2.id).await;
    assert_eq!(cancel_item.len(), 1);
    let cancelled = body(&key2.client, &href).await;
    assert!(cancelled.contains("STATUS:CANCELLED"), "{cancelled}");
    assert_eq!(uid_copies(test, key2.id, uid).await, 1);
    let item = stored_item(test, key2.id, cancel_item[0]).await;
    assert_eq!(item_method(&item.event), Some(ICalendarMethod::Cancel));
    assert_eq!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0);

    // 5. A local sender always adds (spec 6.4): with auto-add off, the
    // invitation still lands on key2's default calendar.
    admin
        .registry_update_setting(
            CalendarScheduling {
                auto_add_invitations: false,
                ..Default::default()
            },
            &[Property::AutoAddInvitations],
        )
        .await;
    admin.reload_settings().await;
    let uid3 = "za-drain-3";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-3.ics"),
            [CONTENT_TYPE],
            invite(uid3, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(uid_copies(test, key2.id, uid3).await, 1, "auto-add off kept the copy out");
    admin
        .registry_update_setting(
            CalendarScheduling::default(),
            &[Property::AutoAddInvitations],
        )
        .await;
    admin.reload_settings().await;

    // Cleanup: the copies with `Schedule-Reply: F` (no declined REPLY), the
    // organizer's events the same way, every item written here.
    for uid in [uid, uid2, uid3] {
        let href = copy_href(test, key2.id, uid).await;
        key2.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for name in ["drain-2", "drain-3"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key2.delete_items_since(test, &start2).await;
    key1.delete_items_since(test, &start1).await;
    assert_eq!(za_uid_leases(test).await, 0);
    test.wait_for_tasks_skip_not_due().await;
}
```

In `tests/src/za/mod.rs`: add `pub mod drain;` and `drain::test(&mut test).await;` right after `uid_lease::test(&mut test).await;` (before `sweep::test`).

- [ ] **Step 5: Run the suite**

Run: `cargo check -p tests --tests`, `cargo fmt -p groupware -p dav -p tests -- --check`, then `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture`.
Expected: passes. Oracles for values marked "(from code reading)": the PROPFIND status of `trigger_drain` (a 207 is what `setup.rs:154` gets for a PROPFIND without a body; if the principal answers another 2xx/3xx status, assert that status and record it). SEQUENCE 1 after the update comes from `organizer_request_full`'s increment on a rescheduled instance; if the merged copy shows 0, the update was not treated as a reschedule: stop and report, do not drop the assertion. The `auto_add_invitations` field and `Property::AutoAddInvitations` are the registry names behind `core.groupware.itip_auto_add` (`crates/common/src/config/groupware.rs:135`).

Then run the key-mode CalDAV suite: `ZA_KEY_ACCOUNTS=1 STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture`. The drain now runs there for john and jane. The `scheduling` variant expects jane's inbox listing to be empty after john's PUT, which the drain changes (Task 6 rewrites that variant); every other variant and upstream suite must still pass, since no upstream fixture invites john or jane. Report the `scheduling` failure as expected; any other failure is a finding.

- [ ] **Step 6: Commit**

```bash
git add crates/groupware/src/calendar/drain.rs crates/groupware/src/calendar/mod.rs crates/dav/src/request.rs tests/src/za/drain.rs tests/src/za/mod.rs
git commit -m "The drain: pending inbox items applied and resealed in the recipient's request

Invitations spec 6: a key account's DAV request drains its scheduling inbox
before its own work, importing under the creation lease or merging through
upstream's iTIP functions, adopting token pairs, checking reply tokens and
sequence, and rewriting each item sealed under the master key in the same
store write as the event.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```

### Task 3: Replies, delegation and pair adoption

**Files:**
- Modify: `tests/src/za/drain.rs` (add `test_reply`, `test_adoption`)
- Modify: `tests/src/za/mod.rs`
- Modify (fix rounds only): `crates/groupware/src/calendar/drain.rs`

**Interfaces:**
- Consumes: Task 2's drain and test helpers (`DrainOn`, `body`, `relative`, `stored_item`); `plant_inbox_item` / `PlantedItem::Pending { pair, reply_token }` (`tests/src/utils/za.rs`); `reply::copy(uid, organizer, &[attendee lines])` and `reply::KEY2_CAL`; `fanout::{attendee_statuses, stored_pairs}`; `test_hooks::{set_commit_fail_once, set_drain_skip, clear_drain_skip}`.
- Produces: nothing new for later tasks; `accept(client, href) -> String` (the copy body with this attendee's PARTSTAT set) stays in the test module for Tasks 4 and 5.

- [ ] **Step 1: The reply and delegation tests**

Append to `tests/src/za/drain.rs` (add `plant_inbox_item`, `PlantedItem`, `attendee_statuses`, `item_partstat` to the imports; `use super::reply::copy;`):

```rust
/// PUTs `href` back with `address`'s PARTSTAT set to `partstat`, keeping
/// the rest of the body as the client read it. Returns the new body.
pub async fn set_partstat(
    client: &DummyWebDavClient,
    href: &str,
    address: &str,
    partstat: &str,
) -> String {
    let current = body(client, href).await;
    // Unfold first (RFC 5545 3.1): a long ATTENDEE line may be folded.
    let unfolded = current.replace("\r\n ", "");
    let mut out = String::new();
    for line in unfolded.split("\r\n") {
        if line.starts_with("ATTENDEE") && line.ends_with(&format!("mailto:{address}")) {
            let line = match line.find("PARTSTAT=") {
                Some(at) => {
                    let end = line[at..].find([';', ':']).map_or(line.len(), |e| at + e);
                    format!("{}PARTSTAT={partstat}{}", &line[..at], &line[end..])
                }
                None => line.replacen("ATTENDEE;", &format!("ATTENDEE;PARTSTAT={partstat};"), 1),
            };
            out.push_str(&line);
        } else {
            out.push_str(line);
        }
        out.push_str("\r\n");
    }
    client
        .request_with_headers("PUT", href, [CONTENT_TYPE], out.clone())
        .await
        .with_status(StatusCode::NO_CONTENT);
    out
}

/// The PARTSTAT of `address` in the event at `href`, as its owner reads it.
pub async fn partstat_of(client: &DummyWebDavClient, href: &str, address: &str) -> Option<String> {
    let text = body(client, href).await;
    match calcard::Parser::new(&text).entry() {
        calcard::Entry::ICalendar(ical) => item_partstat(&ical, address),
        other => panic!("{other:?}"),
    }
}

pub async fn test_reply(test: &mut TestServer) {
    println!("Running zero-access drain reply tests...");
    let _drain = DrainOn::new();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let key3 = Party::new(test, "key3@example.com");
    let keys1 = za_unlock(test, key1.id, STRONG).await;
    let start1 = inbox_ids(test, key1.id).await;
    let start2 = inbox_ids(test, key2.id).await;
    let start3 = inbox_ids(test, key3.id).await;

    // 1. A reply with a matching token applies (spec 4.4). Both sides first
    // rewrite without a message (spec 3.3, "Every rewrite carries the
    // pairs"): key1 adds an alarm to the organizer event, key2 adds one to
    // its copy; then key2 accepts, and key1's drain applies the reply.
    let uid = "za-drain-r1";
    let organizer = format!("{KEY1_CAL}drain-r1.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer,
            [CONTENT_TYPE],
            invite(uid, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let copy = copy_href(test, key2.id, uid).await;
    let alarm = "BEGIN:VALARM\r\nTRIGGER:-PT5M\r\nACTION:DISPLAY\r\nDESCRIPTION:r1-alarm\r\nEND:VALARM\r\nEND:VEVENT";
    let organizer_body = body(&key1.client, &organizer).await.replace("END:VEVENT", alarm);
    key1.client
        .request_with_headers("PUT", &organizer, [CONTENT_TYPE], organizer_body)
        .await
        .with_status(StatusCode::NO_CONTENT);
    let copy_body = body(&key2.client, &copy).await.replace("END:VEVENT", alarm);
    key2.client
        .request_with_headers("PUT", &copy, [CONTENT_TYPE], copy_body)
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        new_inbox_items(test, key2.id, &start2).await.len(),
        1,
        "an alarm-only organizer PUT sent key2 another item"
    );
    set_partstat(&key2.client, &copy, "key2@example.com", "ACCEPTED").await;
    let replies = pending_ids(test, key1.id).await;
    assert_eq!(replies.len(), 1, "key1's inbox holds one pending reply");
    assert_eq!(
        partstat_of(&key1.client, &organizer, "key2@example.com").await.as_deref(),
        Some("ACCEPTED"),
        "the reply did not apply"
    );
    assert!(pending_ids(test, key1.id).await.is_empty());
    let item = stored_item(test, key1.id, replies[0]).await;
    assert_ne!(item.flags & EVENT_NOTIFICATION_IS_CHANGE, 0);
    assert_eq!(item_method(&item.event), Some(ICalendarMethod::Reply));
    assert!(
        !item.event.to_string().contains("X-ZA-RSVP-TOKEN"),
        "the drained reply still carries its token"
    );
    let organizer_after = body(&key1.client, &organizer).await;
    assert!(organizer_after.contains("r1-alarm"), "{organizer_after}");
    // key1's pairs are intact after the merge.
    assert_eq!(
        stored_pairs(test, &keys1, key1.id, "default/drain-r1.ics")
            .await
            .map(|pairs| pairs.len()),
        Some(1)
    );

    // 2. A forged token is dropped (spec 3.3, "Verification"; 6.5): the
    // item is deleted and key1's copy is unchanged.
    let forged = plant_inbox_item(
        test,
        key1.id,
        &reply_tree(uid, "key1@example.com", "key2@example.com", "DECLINED", 0),
        PlantedItem::Pending {
            pair: None,
            reply_token: Some([0x42; 16]),
        },
        key2.id,
        None,
    )
    .await;
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    assert!(
        !inbox_ids(test, key1.id).await.contains(&forged),
        "a forged reply was kept"
    );
    assert_eq!(
        partstat_of(&key1.client, &organizer, "key2@example.com").await.as_deref(),
        Some("ACCEPTED"),
        "a forged reply changed the organizer's copy"
    );
    assert_eq!(uid_copies(test, key1.id, uid).await, 1);

    // 3. A stale reply is history, not applied (spec 6.4, REPLY). key2
    // declines at SEQUENCE 0; with key1's drain skipped, key1 reschedules
    // (SEQUENCE 1); the next key1 request drains the stale decline and
    // keeps it as history; key2's fresh reply at SEQUENCE 1 then applies.
    let uid2 = "za-drain-r2";
    let organizer2 = format!("{KEY1_CAL}drain-r2.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer2,
            [CONTENT_TYPE],
            invite(uid2, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let copy2 = copy_href(test, key2.id, uid2).await;
    set_partstat(&key2.client, &copy2, "key2@example.com", "DECLINED").await;
    let stale = pending_ids(test, key1.id).await;
    assert_eq!(stale.len(), 1);
    test_hooks::set_drain_skip(key1.id);
    key1.client
        .request_with_headers(
            "PUT",
            &organizer2,
            [CONTENT_TYPE],
            invite(uid2, START_B, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(stored_sequence(&key1.client, &organizer2).await, 1);
    assert_eq!(pending_ids(test, key1.id).await, stale, "the skip let the drain run");
    test_hooks::clear_drain_skip(key1.id);
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    assert!(pending_ids(test, key1.id).await.is_empty());
    let item = stored_item(test, key1.id, stale[0]).await;
    assert_eq!(item.flags & EVENT_NOTIFICATION_IS_CHANGE, 0, "a stale reply is not a change");
    assert!(item.event_id.is_some(), "history names the event it was for");
    assert_ne!(
        partstat_of(&key1.client, &organizer2, "key2@example.com").await.as_deref(),
        Some("DECLINED"),
        "the stale decline applied"
    );
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(stored_sequence(&key2.client, &copy2).await, 1);
    set_partstat(&key2.client, &copy2, "key2@example.com", "ACCEPTED").await;
    assert_eq!(
        partstat_of(&key1.client, &organizer2, "key2@example.com").await.as_deref(),
        Some("ACCEPTED"),
        "the fresh reply did not apply"
    );

    // 4. Delegation between three key accounts (spec 3.3, "Delegates";
    // 6.4). key2's REPLY carries key3's entry too, and key3's later REPLY
    // carries key2's: the drain picks the replier by `changed_by` and the
    // token, so neither direction depends on attendee order. key2
    // delegates to key3: the organizer's drain merges the REPLY
    // and invites key3 itself; key3's inbox holds exactly one REQUEST, from
    // key1; key3's reply applies to key1's copy; key2's inbox receives
    // nothing from key3 and key2's copy is unchanged by key3's reply.
    let uid3 = "za-drain-r3";
    let organizer3 = format!("{KEY1_CAL}drain-r3.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer3,
            [CONTENT_TYPE],
            invite(uid3, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let copy3 = copy_href(test, key2.id, uid3).await;
    let delegated = body(&key2.client, &copy3)
        .await
        .replace("\r\n ", "")
        .replace(
            "PARTSTAT=NEEDS-ACTION",
            "PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:key3@example.com\"",
        )
        .replace(
            "END:VEVENT",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;DELEGATED-FROM=\"mailto:key2@example.com\":mailto:key3@example.com\r\nEND:VEVENT",
        );
    key2.client
        .request_with_headers("PUT", &copy3, [CONTENT_TYPE], delegated)
        .await
        .with_status(StatusCode::NO_CONTENT);
    let key2_items_before = inbox_ids(test, key2.id).await;
    assert_eq!(
        new_inbox_items(test, key3.id, &start3).await.len(),
        0,
        "key2's own delegate invitation reached key3 (plan 9 forbids it)"
    );
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    let key3_items = new_inbox_items(test, key3.id, &start3).await;
    assert_eq!(key3_items.len(), 1, "key3 holds exactly one invitation");
    assert_eq!(key3_items[0].1.changed_by, ChangedBy::PrincipalId(key1.id));
    assert_eq!(item_method(&key3_items[0].1.event), Some(ICalendarMethod::Request));
    assert_eq!(
        attendee_statuses(&key1.client, &organizer3, "key3@example.com").await,
        ["1.2"],
        "key1's copy marks key3 delivered"
    );
    let pairs = stored_pairs(test, &keys1, key1.id, "default/drain-r3.ics")
        .await
        .expect("pairs");
    assert_eq!(pairs.len(), 2, "key1 holds a pair for key2 and one for key3");
    assert!(pairs.get("key3@example.com").is_some());
    assert_eq!(trigger_drain(key3.name, STRONG).await, 207);
    let copy3_key3 = copy_href(test, key3.id, uid3).await;
    set_partstat(&key3.client, &copy3_key3, "key3@example.com", "ACCEPTED").await;
    assert_eq!(
        partstat_of(&key1.client, &organizer3, "key3@example.com").await.as_deref(),
        Some("ACCEPTED"),
        "the delegate's reply did not apply to the organizer"
    );
    assert_eq!(
        inbox_ids(test, key2.id).await,
        key2_items_before,
        "key2 received an item from key3"
    );
    assert_eq!(
        partstat_of(&key2.client, &copy3, "key3@example.com").await.as_deref(),
        Some("NEEDS-ACTION"),
        "key2's copy learnt key3's status from somewhere other than the organizer"
    );

    // 5. A stale occurrence reply with no stored override (spec 6.4,
    // REPLY). key2 declines one occurrence of a recurring invitation by
    // adding an EXDATE to its copy, which upstream sends as a REPLY with
    // RECURRENCE-ID at the master's SEQUENCE 0 (`attendee.rs`, the EXDATE
    // arm; from code reading). With key1's drain skipped, key1 reschedules
    // the master (SEQUENCE 1). key1's drain judges the occurrence against
    // the master: the decline is history and no override is added.
    let uid4 = "za-drain-r4";
    let organizer4 = format!("{KEY1_CAL}drain-r4.ics");
    let recurring = |start: &str| {
        invite(uid4, start, &["key2@example.com"])
            .replace("DURATION:PT1H\r\n", "DURATION:PT1H\r\nRRULE:FREQ=DAILY;COUNT=5\r\n")
    };
    key1.client
        .request_with_headers("PUT", &organizer4, [CONTENT_TYPE], recurring(START_A))
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let copy4 = copy_href(test, key2.id, uid4).await;
    let declined_occurrence = body(&key2.client, &copy4)
        .await
        .replace("END:VEVENT", "EXDATE:20990302T100000Z\r\nEND:VEVENT");
    key2.client
        .request_with_headers("PUT", &copy4, [CONTENT_TYPE], declined_occurrence)
        .await
        .with_status(StatusCode::NO_CONTENT);
    let occurrence_reply = pending_ids(test, key1.id).await;
    assert_eq!(occurrence_reply.len(), 1, "the EXDATE built no occurrence reply");
    test_hooks::set_drain_skip(key1.id);
    key1.client
        .request_with_headers("PUT", &organizer4, [CONTENT_TYPE], recurring(START_B))
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(stored_sequence(&key1.client, &organizer4).await, 1);
    test_hooks::clear_drain_skip(key1.id);
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    assert!(pending_ids(test, key1.id).await.is_empty());
    let item = stored_item(test, key1.id, occurrence_reply[0]).await;
    assert_eq!(item.flags & EVENT_NOTIFICATION_IS_CHANGE, 0, "the stale occurrence reply applied");
    let organizer4_body = body(&key1.client, &organizer4).await;
    assert_eq!(
        organizer4_body.matches("BEGIN:VEVENT").count(),
        1,
        "the stale decline added an override: {organizer4_body}"
    );
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);

    // Cleanup.
    for (party, uid) in [
        (&key2, uid),
        (&key2, uid2),
        (&key2, uid3),
        (&key3, uid3),
        (&key2, uid4),
    ] {
        let href = copy_href(test, party.id, uid).await;
        party
            .client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for name in ["drain-r1", "drain-r2", "drain-r3", "drain-r4"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.delete_items_since(test, &start1).await;
    key2.delete_items_since(test, &start2).await;
    key3.delete_items_since(test, &start3).await;
    assert_eq!(za_uid_leases(test).await, 0);
    test.wait_for_tasks_skip_not_due().await;
}

/// A REPLY as upstream's attendee builder shapes it: one VEVENT with the
/// ORGANIZER, the replying ATTENDEE and the UID.
pub fn reply_tree(uid: &str, organizer: &str, attendee: &str, partstat: &str, sequence: i64) -> String {
    format!(
        concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nMETHOD:REPLY\r\n",
            "BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nSEQUENCE:{sequence}\r\n",
            "ORGANIZER:mailto:{organizer}\r\n",
            "ATTENDEE;PARTSTAT={partstat}:mailto:{attendee}\r\n",
            "REQUEST-STATUS:2.0;Success\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        ),
        uid = uid,
        sequence = sequence,
        organizer = organizer,
        attendee = attendee,
        partstat = partstat
    )
}
```

Oracles: step 5's EXDATE reply is built only when the attendee is server-scheduled with RSVP, which the invitation's `RSVP=TRUE` gives; if the PUT builds no reply (no pending item in key1's inbox), stop and report rather than plant one, since the point is upstream's own occurrence reply. In step 3, the organizer's copy after a reschedule may show key2 as `NEEDS-ACTION` (upstream resets it) or keep `ACCEPTED` from nothing (the invite was never accepted here), so the assertion is only that it is not `DECLINED`. In step 4, the `DELEGATED-FROM`/`DELEGATED-TO` syntax is the one `tests/src/za/reply.rs` case 4(b) uses. If `set_partstat`'s line match fails because the client body folds the ATTENDEE line differently, fix the helper, not the assertions.

- [ ] **Step 2: The adoption tests**

Append to `tests/src/za/drain.rs`:

```rust
pub async fn test_adoption(test: &mut TestServer) {
    println!("Running zero-access drain pair adoption tests...");
    let _drain = DrainOn::new();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let keys1 = za_unlock(test, key1.id, STRONG).await;
    let keys2 = za_unlock(test, key2.id, STRONG).await;
    let start1 = inbox_ids(test, key1.id).await;
    let start2 = inbox_ids(test, key2.id).await;
    let key2_pair = |pairs: Option<groupware::calendar::seal::TokenPairs>| {
        *pairs
            .expect("pairs")
            .get("key2@example.com")
            .expect("key2's pair")
    };

    // 1. Remove, re-add, accept (spec 3.3, 6.4): the CANCEL leaves key2's
    // copy with its old pair; the re-invitation's merge adopts the new
    // token; key2's reply then applies under it.
    let uid = "za-drain-a1";
    let organizer = format!("{KEY1_CAL}drain-a1.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer,
            [CONTENT_TYPE],
            invite(uid, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let token_1 = key2_pair(stored_pairs(test, &keys1, key1.id, "default/drain-a1.ics").await);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let copy = copy_href(test, key2.id, uid).await;
    let rel = relative(&copy);
    assert_eq!(key2_pair(stored_pairs(test, &keys2, key2.id, &rel).await), token_1);
    // Removed: a CANCEL with the old pair; key2's copy keeps its pair.
    key1.client
        .request_with_headers("PUT", &organizer, [CONTENT_TYPE], invite(uid, START_A, &[]))
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert!(
        stored_pairs(test, &keys1, key1.id, "default/drain-a1.ics")
            .await
            .is_none(),
        "the organizer kept a pair for a removed attendee"
    );
    let cancelled = body(&key2.client, &copy).await;
    assert!(cancelled.contains("STATUS:CANCELLED"), "{cancelled}");
    assert_eq!(key2_pair(stored_pairs(test, &keys2, key2.id, &rel).await), token_1);
    // Re-added: a fresh token; the merge into the cancelled copy adopts it.
    key1.client
        .request_with_headers(
            "PUT",
            &organizer,
            [CONTENT_TYPE],
            invite(uid, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let token_2 = key2_pair(stored_pairs(test, &keys1, key1.id, "default/drain-a1.ics").await);
    assert_ne!(token_1, token_2, "a re-added attendee gets a new token");
    body(&key2.client, &copy).await;
    assert_eq!(uid_copies(test, key2.id, uid).await, 1);
    assert_eq!(
        key2_pair(stored_pairs(test, &keys2, key2.id, &rel).await),
        token_2,
        "the copy did not adopt the new token"
    );
    set_partstat(&key2.client, &copy, "key2@example.com", "ACCEPTED").await;
    assert_eq!(
        partstat_of(&key1.client, &organizer, "key2@example.com").await.as_deref(),
        Some("ACCEPTED"),
        "the reply under the new token did not apply"
    );

    // 2. Failed-create retry (spec 5.4): key1's first commit fails after
    // the fan-out wrote key2's item; the resend delivers a second item with
    // an equal SEQUENCE and a fresh token. key2 drains both in order: the
    // first imports, the second merges and adopts the new token; the reply
    // applies.
    let uid2 = "za-drain-a2";
    let organizer2 = format!("{KEY1_CAL}drain-a2.ics");
    test_hooks::set_commit_fail_once(Some(key1.id));
    key1.client
        .request_with_headers(
            "PUT",
            &organizer2,
            [CONTENT_TYPE],
            invite(uid2, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(pending_ids(test, key2.id).await.len(), 1, "the item written before the failed commit");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer2,
            [CONTENT_TYPE],
            invite(uid2, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pending_ids(test, key2.id).await.len(), 2);
    let stored_token = key2_pair(stored_pairs(test, &keys1, key1.id, "default/drain-a2.ics").await);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(pending_ids(test, key2.id).await.is_empty(), "both items drained");
    assert_eq!(uid_copies(test, key2.id, uid2).await, 1, "one copy for two items");
    let copy2 = copy_href(test, key2.id, uid2).await;
    assert_eq!(
        key2_pair(stored_pairs(test, &keys2, key2.id, &relative(&copy2)).await),
        stored_token,
        "the copy holds the orphan token, not the stored one"
    );
    set_partstat(&key2.client, &copy2, "key2@example.com", "ACCEPTED").await;
    assert_eq!(
        partstat_of(&key1.client, &organizer2, "key2@example.com").await.as_deref(),
        Some("ACCEPTED")
    );

    // Cleanup.
    for href in [copy, copy2] {
        key2.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for name in ["drain-a1", "drain-a2"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.delete_items_since(test, &start1).await;
    key2.delete_items_since(test, &start2).await;
    assert_eq!(za_uid_leases(test).await, 0);
    test.wait_for_tasks_skip_not_due().await;
}
```

Wire `drain::test_reply` and `drain::test_adoption` after `drain::test` in `tests/src/za/mod.rs`.

Oracles: step 1's `stored_pairs(..).is_none()` after the attendee list is emptied follows plan 9's Review Focus 2 (an update that removes every ATTENDEE leaves no pair). The failed-create PUT answers 500, as `fanout::test_failures` step 8 asserts. If the merged copy after the re-add still shows `STATUS:CANCELLED`, that is upstream's merge behaviour and not asserted either way.

- [ ] **Step 3: Run the suite, then commit**

Run: `cargo check -p tests --tests`, `cargo fmt -p tests -- --check`, then the za suite.
Expected: passes. A failure in the drain's code is fixed in `drain.rs` without widening scope.

```bash
git add tests/src/za/drain.rs tests/src/za/mod.rs crates/groupware/src/calendar/drain.rs
git commit -m "Drain tests: replies, stale replies, delegation, pair adoption

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```

### Task 4: The creation lease in the drain, and the cursor

**Files:**
- Modify: `tests/src/za/drain.rs` (add `test_lease`, `test_cursor`)
- Modify: `tests/src/za/uid_lease.rs` (`put` becomes `pub`)
- Modify: `tests/src/za/mod.rs`
- Modify (fix rounds only): `crates/groupware/src/calendar/drain.rs`

**Interfaces:**
- Consumes: `HookParked::start(account_id, set, request)` / `finish()` (`tests/src/utils/za.rs`; the request future must be `Send + 'static`), `test_hooks::{set_drain_lookup_pause, set_drain_lease_pause, set_drain_fail_once, set_drain_retry_override, set_lease_secs_override, set_fan_out_pause, set_drain_skip, clear_drain_skip}`, `za_app_password`, `trigger_drain`, `uid_lease::put` (make it `pub` in `tests/src/za/uid_lease.rs`; it builds a 5-second PUT future), `reply::copy`.
- Produces: nothing new.

Two sessions of key2: the password session (`key2.client`, `trigger_drain(key2.name, STRONG)`) and the app-password session (`trigger_drain(key2.name, &app)`, `put(key2.name, &app, ..)`). They are two key-cache entries, so two `SessionKeys` and two drain locks (spec 6.3). The app password is created once at the start of `test_lease` and revoked at its end as `tests/src/za/app_password.rs` revokes one (`app-password/revoke` with the `credential_id` that `common::auth::credential::AppPassword::parse` reads from the password).

- [ ] **Step 1: The lease tests**

Append to `tests/src/za/drain.rs` (imports: `HookParked`, `za_app_password`, `za_post`, `super::uid_lease::put`, `common::auth::credential::AppPassword`, `std::time::Duration`, `serde_json::json`):

```rust
pub async fn test_lease(test: &mut TestServer) {
    println!("Running zero-access drain creation lease tests...");
    let _drain = DrainOn::new();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let key3 = Party::new(test, "key3@example.com");
    let keys2 = za_unlock(test, key2.id, STRONG).await;
    let start1 = inbox_ids(test, key1.id).await;
    let start2 = inbox_ids(test, key2.id).await;
    let start3 = inbox_ids(test, key3.id).await;
    let app = za_app_password(key2.name, STRONG, "drain-second-session").await;
    test_hooks::set_drain_retry_override(Some(0));

    // A. Drain versus drain (spec 6.3, 11). Two fan-outs for one UID
    // publish the lower document id late. The late one is key1's PUT, held
    // at the fan-out pause after its item's id was assigned; key1 holds
    // its own creation lease for the UID while held, so the second
    // fan-out comes from another organizer, key3, whose PUT of the same
    // UID contends on nothing and delivers the higher id. Session 1 drains
    // the higher id and is held, holding key2's creation lease, right
    // before its commit; the first PUT then commits the lower id. Session
    // 2 drains: both items find no event, the lease is busy, both stay
    // pending. Session 1 commits one copy (key3's invitation); its next
    // request finds the lower id, whose ORGANIZER differs, and keeps it as
    // history. Exactly one copy, both items drained.
    let uid = "za-drain-l1";
    let parked_put = HookParked::start(
        key2.id,
        test_hooks::set_fan_out_pause,
        put(
            key1.name,
            STRONG,
            format!("{KEY1_CAL}drain-l1.ics"),
            invite(uid, START_A, &["key2@example.com"]),
        ),
    )
    .await;
    key3.client
        .request_with_headers(
            "PUT",
            "/dav/cal/key3%40example.com/default/drain-l1.ics",
            [CONTENT_TYPE],
            super::reply::copy(
                uid,
                "key3@example.com",
                &["ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com"],
            ),
        )
        .await
        .with_status(StatusCode::CREATED);
    let high = pending_ids(test, key2.id).await;
    assert_eq!(high.len(), 1, "key3's item is the only committed one");
    let session_1 = HookParked::start(
        key2.id,
        test_hooks::set_drain_lease_pause,
        trigger_drain(key2.name, STRONG),
    )
    .await;
    assert_eq!(parked_put.finish().await, 201, "the held PUT");
    let both = pending_ids(test, key2.id).await;
    assert_eq!(both.len(), 2);
    assert!(both[0] < both[1] && both[1] == high[0], "the lower id arrived late: {both:?}");
    assert_eq!(
        trigger_drain(key2.name, &app).await,
        207,
        "session 2's request"
    );
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        2,
        "session 2 imported past the lease"
    );
    assert_eq!(uid_copies(test, key2.id, uid).await, 0);
    assert_eq!(session_1.finish().await, 207);
    assert_eq!(uid_copies(test, key2.id, uid).await, 1, "session 1 committed one copy");
    assert_eq!(pending_ids(test, key2.id).await, [both[0]], "the late item waits for a pass");
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(pending_ids(test, key2.id).await.is_empty());
    assert_eq!(uid_copies(test, key2.id, uid).await, 1, "a second copy was imported");
    assert_eq!(za_uid_leases(test).await, 0);
    // The copy is key3's invitation, with key3's token for key2; key1's
    // late item was kept as history (OrganizerMismatch), its event_id set.
    let keys3 = za_unlock(test, key3.id, STRONG).await;
    let key3_token = *stored_pairs(test, &keys3, key3.id, "default/drain-l1.ics")
        .await
        .expect("pairs")
        .get("key2@example.com")
        .unwrap();
    let copy = copy_href(test, key2.id, uid).await;
    assert_eq!(
        stored_pairs(test, &keys2, key2.id, &relative(&copy))
            .await
            .expect("pairs")
            .get("key2@example.com"),
        Some(&key3_token)
    );
    let late = stored_item(test, key2.id, both[0]).await;
    assert_eq!(late.flags & EVENT_NOTIFICATION_IS_CHANGE, 0);
    assert!(late.event_id.is_some());
    let copy_body = body(&key2.client, &copy).await;
    assert!(copy_body.contains("ORGANIZER:mailto:key3@example.com"), "{copy_body}");

    // B. Drain versus PUT (spec 6.3, 11): session 1 is held after its first
    // lookup found nothing, before the lease; session 2 (drain skipped)
    // PUTs the same UID into key2's default calendar and wins; session 1
    // takes the lease, finds the copy and merges. One copy.
    let uid2 = "za-drain-l2";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-l2.ics"),
            [CONTENT_TYPE],
            invite(uid2, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let session_1 = HookParked::start(
        key2.id,
        test_hooks::set_drain_lookup_pause,
        trigger_drain(key2.name, STRONG),
    )
    .await;
    test_hooks::set_drain_skip(key2.id);
    let own = format!("/dav/cal/key2%40example.com/default/drain-l2-own.ics");
    assert_eq!(
        put(
            key2.name,
            &app,
            own.clone(),
            super::reply::copy(
                uid2,
                "key1@example.com",
                &["ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com"]
            ),
        )
        .await,
        201,
        "session 2's PUT of the UID"
    );
    test_hooks::clear_drain_skip(key2.id);
    assert_eq!(session_1.finish().await, 207);
    assert_eq!(uid_copies(test, key2.id, uid2).await, 1, "the drain imported a second copy");
    assert!(pending_ids(test, key2.id).await.is_empty(), "the drain did not merge");
    let merged = body(&key2.client, &own).await;
    assert!(
        merged.contains(&format!("fo-title {uid2}")),
        "the merge did not reach session 2's copy: {merged}"
    );
    assert_eq!(za_uid_leases(test).await, 0);

    // C. Lease expiry (spec 6.3 step 3, 11): session 1 holds the lease past
    // its 1-second duration; session 2's PUT takes the expired lease over
    // and stores the UID; session 1's fenced commit is lost, the item stays
    // pending, and the next pass merges. One copy.
    let uid3 = "za-drain-l3";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-l3.ics"),
            [CONTENT_TYPE],
            invite(uid3, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    test_hooks::set_lease_secs_override(Some(1));
    let session_1 = HookParked::start(
        key2.id,
        test_hooks::set_drain_lease_pause,
        trigger_drain(key2.name, STRONG),
    )
    .await;
    test_hooks::set_lease_secs_override(None);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    test_hooks::set_drain_skip(key2.id);
    let own3 = format!("/dav/cal/key2%40example.com/default/drain-l3-own.ics");
    assert_eq!(
        put(
            key2.name,
            &app,
            own3.clone(),
            super::reply::copy(
                uid3,
                "key1@example.com",
                &["ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com"]
            ),
        )
        .await,
        201,
        "the expired lease was not taken over"
    );
    test_hooks::clear_drain_skip(key2.id);
    assert_eq!(session_1.finish().await, 207, "a lost fence failed the request");
    assert_eq!(uid_copies(test, key2.id, uid3).await, 1, "the expired holder committed");
    assert_eq!(pending_ids(test, key2.id).await.len(), 1, "the lost commit drained the item");
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(pending_ids(test, key2.id).await.is_empty());
    assert_eq!(uid_copies(test, key2.id, uid3).await, 1);
    assert_eq!(za_uid_leases(test).await, 0);

    // D. The item deleted under a pass (Review Focus 1): session 1 holds
    // the lease before its commit; session 2 deletes the item by href; the
    // commit loses the item's assertion, nothing is imported, the lease is
    // released, the request succeeds.
    let uid4 = "za-drain-l4";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-l4.ics"),
            [CONTENT_TYPE],
            invite(uid4, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let item = pending_ids(test, key2.id).await;
    assert_eq!(item.len(), 1);
    let session_1 = HookParked::start(
        key2.id,
        test_hooks::set_drain_lease_pause,
        trigger_drain(key2.name, STRONG),
    )
    .await;
    test_hooks::set_drain_skip(key2.id);
    let item_href = format!("{}{}.ics", key2.inbox(), item[0]);
    assert_eq!(
        crate::utils::http::HttpRequest::with_credentials(8899, key2.name, &app)
            .send_full(hyper::Method::DELETE, &item_href, None, None)
            .await
            .status
            .as_u16(),
        204
    );
    test_hooks::clear_drain_skip(key2.id);
    assert_eq!(session_1.finish().await, 207);
    assert_eq!(uid_copies(test, key2.id, uid4).await, 0, "a copy without its item");
    assert!(!inbox_ids(test, key2.id).await.contains(&item[0]));
    assert_eq!(za_uid_leases(test).await, 0, "the lost commit kept its lease");

    // Cleanup.
    test_hooks::set_drain_retry_override(None);
    for href in [copy, own, own3] {
        key2.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key3.client
        .request_with_headers(
            "DELETE",
            "/dav/cal/key3%40example.com/default/drain-l1.ics",
            [NO_REPLY],
            "",
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    for href in [
        format!("{KEY1_CAL}drain-l1.ics"),
        format!("{KEY1_CAL}drain-l2.ics"),
        format!("{KEY1_CAL}drain-l3.ics"),
        format!("{KEY1_CAL}drain-l4.ics"),
    ] {
        key1.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.delete_items_since(test, &start1).await;
    key2.delete_items_since(test, &start2).await;
    key3.delete_items_since(test, &start3).await;
    let credential_id = AppPassword::parse(&app).unwrap().credential_id;
    za_post(
        "app-password/revoke",
        &json!({ "username": key2.name, "password": STRONG, "credential_id": credential_id }),
    )
    .await
    .expect(200);
    assert_eq!(za_uid_leases(test).await, 0);
    test.wait_for_tasks_skip_not_due().await;
}
```

Notes for the implementer:
- `put` in `uid_lease.rs` takes `(name: &str, secret: &str, path: String, body: String)`; make it `pub` and import it. `AppPassword` is `common::auth::credential::AppPassword`, as `tests/src/za/app_password.rs` imports it.
- In case A, the second fan-out must come from another account: key1's held PUT holds key1's (account, UID) creation lease through its fan-out, so a second key1 PUT of the UID, into any calendar, would wait on that lease and answer 503. key3 is a key account of the suite with its own lease space. Session 2's request takes about 2 s (two items, 20 retries of 50 ms each on the busy lease); `trigger_drain` allows 5 s.
- In case B, session 2's PUT is a key-account create of the UID: it takes the PUT lease (free, since session 1 is paused before the drain's lease), re-checks archives (none), inserts and releases. Its drain is skipped so it does not import the pending item itself. The PUT answers 201 although key2 is not the organizer: `itip_create` yields `NotOrganizer`, which has no failed precondition and is only traced (`crates/dav/src/calendar/update.rs`). The merged SUMMARY assertion is from code reading: upstream's `Request` arm applies the organizer's entries when the message's SEQUENCE is `>=` the copy's (`inbound.rs:101`); if the copy keeps `fo-reply-title`, stop and report.
- The DELETE in case D goes through the raw client so the password session's lock (held by the parked pass) is not involved; the app-password session's own drain is skipped.
- Every assertion naming the lease count is an oracle for `ZaUidLeaseGuard`'s release paths; a leftover lease is a drain bug, not a test bug.

- [ ] **Step 2: The cursor tests**

Append to `tests/src/za/drain.rs`:

```rust
pub async fn test_cursor(test: &mut TestServer) {
    println!("Running zero-access drain cursor tests...");
    let _drain = DrainOn::new();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let start1 = inbox_ids(test, key1.id).await;
    let start2 = inbox_ids(test, key2.id).await;

    // 1. An item that arrives while a pass is rewriting an earlier item is
    // processed on the next request (spec 6.2, "The cursor"): the pass is
    // held before its commit of item X; item Y arrives; the pass completes
    // X only; the next request drains Y.
    let (uid_x, uid_y) = ("za-drain-c1", "za-drain-c2");
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-c1.ics"),
            [CONTENT_TYPE],
            invite(uid_x, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let pass = HookParked::start(
        key2.id,
        test_hooks::set_drain_lease_pause,
        trigger_drain(key2.name, STRONG),
    )
    .await;
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-c2.ics"),
            [CONTENT_TYPE],
            invite(uid_y, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pass.finish().await, 207);
    assert_eq!(uid_copies(test, key2.id, uid_x).await, 1);
    assert_eq!(uid_copies(test, key2.id, uid_y).await, 0, "the pass took an item past its snapshot");
    assert_eq!(pending_ids(test, key2.id).await.len(), 1);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(uid_copies(test, key2.id, uid_y).await, 1, "the late item was not drained next");
    assert!(pending_ids(test, key2.id).await.is_empty());

    // 2. A transient failure is retried without a new arrival, after the
    // backoff (spec 6.2, 6.5): the drain's commit fails once; the item
    // stays pending; the next request within the 30-second backoff does
    // not drain; with the backoff overridden to zero, the next request
    // drains.
    let uid_z = "za-drain-c3";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-c3.ics"),
            [CONTENT_TYPE],
            invite(uid_z, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    // The deferral is an absolute deadline taken at the failing pass, so
    // the short backoff is set before the failure; the override cannot
    // shorten a deadline already stored.
    test_hooks::set_drain_retry_override(Some(2));
    test_hooks::set_drain_fail_once(Some(key2.id));
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207, "a drain failure failed the request");
    assert_eq!(pending_ids(test, key2.id).await.len(), 1, "a failed commit drained the item");
    assert_eq!(uid_copies(test, key2.id, uid_z).await, 0);
    assert_eq!(za_uid_leases(test).await, 0, "the failed commit kept its lease");
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        1,
        "the drain retried inside the backoff"
    );
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(pending_ids(test, key2.id).await.is_empty(), "the retry did not drain");
    assert_eq!(uid_copies(test, key2.id, uid_z).await, 1);
    test_hooks::set_drain_retry_override(None);

    // Cleanup.
    for uid in [uid_x, uid_y, uid_z] {
        let href = copy_href(test, key2.id, uid).await;
        key2.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for name in ["drain-c1", "drain-c2", "drain-c3"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.delete_items_since(test, &start1).await;
    key2.delete_items_since(test, &start2).await;
    assert_eq!(za_uid_leases(test).await, 0);
    test.wait_for_tasks_skip_not_due().await;
}
```

Wire `drain::test_lease` and `drain::test_cursor` after `drain::test_adoption`.

Step 2's second request, inside the backoff, relies on the pass having set `retry_after` to now + 2 s (the override in force when it failed); the third request comes 2.5 s later. `now()` is the wall clock (`crates/store/src/write/mod.rs`), so a Tokio time pause would not help. The failing commit must release the lease (`ItemCtx::commit`'s hook branch does), which the lease count checks.

- [ ] **Step 3: Run the suite, then commit**

Run: `cargo check -p tests --tests`, `cargo fmt -p tests -- --check`, then the za suite.
Expected: passes. A hang of a `HookParked::start` ("the request did not reach the pause point") means the drain took a path that skips the pause: a cursor already at the inbox's change id (the previous step's cleanup must leave the cursor behind the new item, which a new PUT guarantees), or the skip still set.

```bash
git add tests/src/za/drain.rs tests/src/za/uid_lease.rs tests/src/za/mod.rs crates/groupware/src/calendar/drain.rs
git commit -m "Drain tests: the creation lease under contention, the cursor and the backoff

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```

### Task 5: Item errors, odd inboxes, and the drain in the leak scan

**Files:**
- Modify: `tests/src/za/drain.rs` (add `test_errors`, `request_tree`)
- Modify: `tests/src/za/leak.rs` (add `test_drain`, `DRAIN_EVENT`, canaries)
- Modify: `tests/src/za/mod.rs`
- Modify (fix rounds only): `crates/groupware/src/calendar/drain.rs`

**Interfaces:**
- Consumes: `plant_inbox_item`, `rewrite_inbox_item(test, account_id, document_id, edit)` (`tests/src/utils/za.rs`); `leak::{scan, token_hits, Scan, EXTRA_CANARIES}`; `fanout::MKCALENDAR`; Task 2-4's helpers.
- Produces: nothing new.

- [ ] **Step 1: The error tests**

Append to `tests/src/za/drain.rs` (imports: `rewrite_inbox_item`, `calcard::icalendar::ICalendarValue`, `super::fanout::MKCALENDAR`; for step 6, `::registry::{schema::{enums::StorageQuota, prelude::ObjectType}, types::EnumImpl}` and `serde_json::json`, as `tests/src/za/fanout.rs` imports them):

```rust
/// A REQUEST as upstream's organizer builder shapes it, for planting.
pub fn request_tree(uid: &str, organizer: &str, attendee: &str) -> String {
    format!(
        concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nMETHOD:REQUEST\r\n",
            "BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:{start}\r\n",
            "DURATION:PT1H\r\nSEQUENCE:0\r\nSUMMARY:planted {uid}\r\n",
            "ORGANIZER:mailto:{organizer}\r\n",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{attendee}\r\n",
            "END:VEVENT\r\nEND:VCALENDAR\r\n"
        ),
        uid = uid,
        start = START_A,
        organizer = organizer,
        attendee = attendee
    )
}

pub async fn test_errors(test: &mut TestServer) {
    println!("Running zero-access drain item error tests...");
    let _drain = DrainOn::new();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let key3 = Party::new(test, "key3@example.com");
    let keys2 = za_unlock(test, key2.id, STRONG).await;
    let start1 = inbox_ids(test, key1.id).await;
    let start2 = inbox_ids(test, key2.id).await;

    // 1. Permanent errors (spec 6.5): a plaintext item with the pending bit
    // (no envelope) and a pending item whose envelope is corrupt are
    // deleted; nothing is imported.
    let plaintext = plant_inbox_item(
        test,
        key2.id,
        &request_tree("za-drain-e1", "key1@example.com", "key2@example.com"),
        PlantedItem::Plain {
            flags: EVENT_NOTIFICATION_ZA_PENDING,
        },
        key1.id,
        None,
    )
    .await;
    let corrupt = plant_inbox_item(
        test,
        key2.id,
        &request_tree("za-drain-e2", "key1@example.com", "key2@example.com"),
        PlantedItem::Pending {
            pair: Some(("key2@example.com", [0x11; 16])),
            reply_token: None,
        },
        key1.id,
        None,
    )
    .await;
    rewrite_inbox_item(test, key2.id, corrupt, |item| {
        // The root's last entry is `X-ZA-KEY`; its text becomes garbage.
        let envelope = item.event.components[0].entries.last_mut().unwrap();
        envelope.values = vec![ICalendarValue::Text("AAAA".to_string())];
    })
    .await;
    assert_eq!(pending_ids(test, key2.id).await.len(), 2);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let ids = inbox_ids(test, key2.id).await;
    assert!(
        !ids.contains(&plaintext) && !ids.contains(&corrupt),
        "an undecodable item was kept: {ids:?}"
    );
    assert_eq!(uid_copies(test, key2.id, "za-drain-e1").await, 0);
    assert_eq!(uid_copies(test, key2.id, "za-drain-e2").await, 0);

    // 2. Sender mismatch (spec 6.4 step 2): an item whose ORGANIZER is key1
    // but whose `changed_by` is key3 is kept as history, not applied.
    let mismatch = plant_inbox_item(
        test,
        key2.id,
        &request_tree("za-drain-e3", "key1@example.com", "key2@example.com"),
        PlantedItem::Pending {
            pair: Some(("key2@example.com", [0x22; 16])),
            reply_token: None,
        },
        key3.id,
        None,
    )
    .await;
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let item = stored_item(test, key2.id, mismatch).await;
    assert_eq!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0, "history is not pending");
    assert_eq!(item.event_id, None);
    assert_eq!(envelope_wrap(&item.event), Some(1), "history is resealed under the master key");
    assert_eq!(uid_copies(test, key2.id, "za-drain-e3").await, 0, "a mismatched sender's invitation was imported");

    // 3. A REPLY for an event the organizer does not have (Review Focus 4):
    // kept as history with no event, nothing deleted.
    let orphan = plant_inbox_item(
        test,
        key2.id,
        &reply_tree("za-drain-e4", "key2@example.com", "key1@example.com", "ACCEPTED", 0),
        PlantedItem::Pending {
            pair: None,
            reply_token: Some([0x33; 16]),
        },
        key1.id,
        None,
    )
    .await;
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let item = stored_item(test, key2.id, orphan).await;
    assert_eq!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0);
    assert_eq!(item.event_id, None);

    // 4. The same UID in two calendars (Review Focus 2): key2 copies its
    // imported event into a second calendar; key1's update merges into the
    // lowest document id only.
    let uid = "za-drain-e5";
    let organizer = format!("{KEY1_CAL}drain-e5.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer,
            [CONTENT_TYPE],
            invite(uid, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let first = copy_href(test, key2.id, uid).await;
    key2.client
        .request("MKCALENDAR", "/dav/cal/key2%40example.com/drain-second/", MKCALENDAR)
        .await
        .with_status(StatusCode::CREATED);
    let second = "/dav/cal/key2%40example.com/drain-second/e5-copy.ics";
    key2.client
        .request_with_headers("COPY", &first, [("destination", second)], "")
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(uid_copies(test, key2.id, uid).await, 2);
    key1.client
        .request_with_headers(
            "PUT",
            &organizer,
            [CONTENT_TYPE],
            invite(uid, START_B, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let merged = body(&key2.client, &first).await;
    let untouched = body(&key2.client, second).await;
    assert!(merged.contains(&format!("DTSTART:{START_B}")), "{merged}");
    assert!(untouched.contains(&format!("DTSTART:{START_A}")), "{untouched}");
    assert!(pending_ids(test, key2.id).await.is_empty());

    // 5. Two addresses that resolve to one account (plan 9 outcome): two
    // items for one UID, different pairs; the drain imports the first and
    // merges the second; the copy ends with both pairs.
    let uid2 = "za-drain-e6";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-e6.ics"),
            [CONTENT_TYPE],
            invite(uid2, START_A, &["key2@example.com", "key2+two@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pending_ids(test, key2.id).await.len(), 2, "one item per address");
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(pending_ids(test, key2.id).await.is_empty());
    assert_eq!(uid_copies(test, key2.id, uid2).await, 1);
    let copy2 = copy_href(test, key2.id, uid2).await;
    let pairs = stored_pairs(test, &keys2, key2.id, &relative(&copy2))
        .await
        .expect("pairs");
    assert_eq!(pairs.len(), 2, "the second item's pair was not adopted alongside the first");
    assert!(pairs.get("key2@example.com").is_some() && pairs.get("key2+two@example.com").is_some());

    // 6. Quota (spec 6.4 step 4): an import past key2's disk quota is an
    // iTIP-level rejection, kept as history, no copy. A failed quota
    // counter read is a store error that keeps the item pending; the test
    // store cannot fail, so that path rests on the classification in
    // `is_over_quota`, the fan-out's.
    let admin = test.account("admin@example.com").clone();
    let key2_account = test.account("key2@example.com").id();
    admin
        .registry_update_object(
            ObjectType::Account,
            key2_account,
            json!({ Property::Quotas: { StorageQuota::MaxDiskQuota.as_str(): 1 } }),
        )
        .await;
    let uid3 = "za-drain-e7";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-e7.ics"),
            [CONTENT_TYPE],
            invite(uid3, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let over_quota = pending_ids(test, key2.id).await;
    assert_eq!(over_quota.len(), 1);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let item = stored_item(test, key2.id, over_quota[0]).await;
    assert_eq!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0, "an over-quota import stayed pending");
    assert_eq!(item.event_id, None);
    assert_eq!(uid_copies(test, key2.id, uid3).await, 0, "an import past the quota");
    admin
        .registry_update_object(ObjectType::Account, key2_account, json!({ Property::Quotas: {} }))
        .await;

    // Cleanup.
    for href in [first, second.to_string(), copy2] {
        key2.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key2.client
        .request("DELETE", "/dav/cal/key2%40example.com/drain-second/", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    for name in ["drain-e5", "drain-e6", "drain-e7"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.delete_items_since(test, &start1).await;
    key2.delete_items_since(test, &start2).await;
    assert_eq!(za_uid_leases(test).await, 0);
    test.wait_for_tasks_skip_not_due().await;
}
```

Wire `drain::test_errors` after `drain::test_cursor`. Oracles: the COPY within one account of a sealed event keeps the stored bytes (plan 2: `copy_move.rs` has no seal step), so the second copy opens under key2's keys; if COPY of an event refuses the destination calendar for another reason, use a second PUT of `first`'s body into `second` instead and record the ruling. Sub-addressing is on by default (`SubAddressing::Enabled`, plan 9 Review Focus 3), so `key2+two@example.com` resolves to key2.

- [ ] **Step 2: The drain in the leak scan**

In `tests/src/za/leak.rs`, add the canaries `"draintitle-canary"`, `"drainloc-canary"`, `"draindesc-canary"`, `"draincn-canary"`, `"drainaddr-canary"` to `EXTRA_CANARIES`, add the fixture:

```rust
/// Plan 10: key1's invitation to key2 with every canary the drain handles,
/// plus an external attendee whose address is a canary.
const DRAIN_EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:leak-drain\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990105T090000Z\r\nDTEND:20990105T100000Z\r\nSEQUENCE:0\r\nSUMMARY:draintitle-canary\r\nLOCATION:drainloc-canary\r\nDESCRIPTION:draindesc-canary\r\nORGANIZER:mailto:key1@example.com\r\nATTENDEE;CN=draincn-canary;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com\r\nATTENDEE:mailto:drainaddr-canary@remote.test\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
```

and the test (imports from `crate::za::drain::{DrainOn, body, relative, set_partstat}` and `crate::utils::za::{copy_href, pending_ids, trigger_drain}`):

```rust
/// Spec 11, leak tests: a drained invitation, the attendee's copy and the
/// reply leave no canary and no token in the clear anywhere, in either
/// account's records; the reply's visible token is the one exception, and
/// it is gone once the organizer drains.
pub async fn test_drain(test: &mut TestServer) {
    println!("Running zero-access leak test of the drain...");
    let _drain = DrainOn::new();
    let key1 = DummyWebDavClient::new(
        test.account("key1@example.com").id().document_id(),
        "key1@example.com",
        STRONG,
        "key1@example.com",
    );
    let key1_id = key1.account_id;
    let key2_id = test.account("key2@example.com").id().document_id();
    let key2 = DummyWebDavClient::new(key2_id, "key2@example.com", STRONG, "key2@example.com");
    let key1_items = inbox_ids(test, key1_id).await;
    let key2_items = inbox_ids(test, key2_id).await;
    let held = hold_task_consumer(test).await;
    key1.request_with_headers(
        "PUT",
        "/dav/cal/key1%40example.com/default/leak-drain.ics",
        [CONTENT_TYPE],
        DRAIN_EVENT,
    )
    .await
    .with_status(StatusCode::CREATED);
    let (organizer_event, _) = raw_event(test, key1_id, "default/leak-drain.ics").await;
    let keys1 = za_unlock(test, key1_id, STRONG).await;
    let token = *event_token_pairs(&organizer_event, &keys1, key1_id)
        .unwrap()
        .expect("key1's pairs")
        .get("key2@example.com")
        .expect("key2's pair");
    assert_eq!(pending_ids(test, key2_id).await.len(), 1);

    // key2 drains: the copy and the drained item are sealed; the token is
    // in the clear nowhere; the task queue holds nothing of the invitation.
    assert_eq!(trigger_drain("key2@example.com", STRONG).await, 207);
    assert!(pending_ids(test, key2_id).await.is_empty());
    let copy = copy_href(test, key2_id, "leak-drain").await;
    let shown = body(&key2, &copy).await;
    assert!(
        shown.contains("draintitle-canary") && shown.contains("draincn-canary"),
        "{shown}"
    );
    let drained = scan(test, key2_id).await;
    assert!(
        drained.violations.is_empty(),
        "leaks after the drain:\n{}",
        drained.violations.join("\n")
    );
    assert!(drained.events >= 1 && drained.items >= 1, "{drained:?}");
    assert_eq!(drained.item_keys, drained.items, "{drained:?}");
    let organizer_scan = scan(test, key1_id).await;
    assert!(
        organizer_scan.violations.is_empty(),
        "leaks in the organizer's records:\n{}",
        organizer_scan.violations.join("\n")
    );
    assert_eq!(
        itip_tasks_for(test, key1_id).await + itip_tasks_for(test, key2_id).await,
        0
    );
    assert!(
        token_hits(test, &[token]).await.is_empty(),
        "key2's token in the clear after the drain"
    );

    // key2 accepts: the pending reply in key1's inbox carries the token
    // visibly (spec 3.3), the one place it may appear; key1's drain
    // reseals the item without it.
    set_partstat(&key2, &copy, "key2@example.com", "ACCEPTED").await;
    assert_eq!(pending_ids(test, key1_id).await.len(), 1);
    assert!(
        !token_hits(test, &[token]).await.is_empty(),
        "the positive control: the pending reply's visible token"
    );
    assert_eq!(trigger_drain("key1@example.com", STRONG).await, 207);
    assert!(pending_ids(test, key1_id).await.is_empty());
    assert!(
        token_hits(test, &[token]).await.is_empty(),
        "the token in the clear after the organizer's drain"
    );
    let after_reply = scan(test, key1_id).await;
    assert!(
        after_reply.violations.is_empty(),
        "leaks after the reply:\n{}",
        after_reply.violations.join("\n")
    );
    drop(held);
    test.wait_for_tasks().await;

    // Cleanup, as `test` does: events with `Schedule-Reply: F`, items by
    // href.
    key2.request_with_headers("DELETE", &copy, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    key1.request_with_headers(
        "DELETE",
        "/dav/cal/key1%40example.com/default/leak-drain.ics",
        [NO_REPLY],
        "",
    )
    .await
    .with_status(StatusCode::NO_CONTENT);
    for (client, account_id, before) in [(&key1, key1_id, &key1_items), (&key2, key2_id, &key2_items)] {
        for id in inbox_ids(test, account_id).await {
            if !before.contains(&id) {
                client
                    .request(
                        "DELETE",
                        &format!(
                            "/dav/itip/{}/inbox/{id}.ics",
                            client.name.replace('@', "%40")
                        ),
                        "",
                    )
                    .await
                    .with_status(StatusCode::NO_CONTENT);
            }
        }
    }
    test.wait_for_tasks().await;
}
```

`CONTENT_TYPE` and `NO_REPLY` come from `super::fanout`; `hold_task_consumer`, `itip_tasks_for`, `inbox_ids`, `za_unlock`, `event_token_pairs` and `raw_event` are already imported by `leak.rs`. If `DummyWebDavClient` has no public `name`, keep the inbox path in a local variable per client. Wire `leak::test_drain(&mut test).await;` right before `leak::test(&mut test).await;` in `tests/src/za/mod.rs`.

- [ ] **Step 3: Run the suite, then commit**

Run: `cargo check -p tests --tests`, `cargo fmt -p tests -- --check`, then the za suite, then once more with `BLOB_STORE=FileSystem`.
Expected: passes both times. A `scan` violation names the record and the canary: a canary in plaintext is a drain bug (the rewrite or the import sealed the wrong tree), a structural violation on a drained item ("drained item carries ...") means the rewrite left a carrier.

```bash
git add tests/src/za/drain.rs tests/src/za/leak.rs tests/src/za/mod.rs crates/groupware/src/calendar/drain.rs
git commit -m "Drain tests: item errors, odd inboxes, and the drain in the leak scan

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```

### Task 6: The gates spec 8 reopens, the key-mode variant, the checklist and CLAUDE.md

**Files:**
- Modify: `crates/http/src/request.rs:338-352` and `:1043-1063`
- Modify: `crates/dav/src/principal/propfind.rs:309-315`
- Modify: `tests/src/za/gating.rs:132-171`
- Modify: `tests/src/webdav/za_variants.rs:355-500` (`scheduling`)
- Modify: `docs/zero-access/manual-checklist.md:21`
- Modify: `CLAUDE.md`

**Interfaces:**
- Consumes: `copy_href`, `pending_ids`, `trigger_drain`, `inbox_items` (`tests/src/utils/za.rs`); `uid_copies` is in `tests/src/za/fanout.rs`, reachable as `crate::za::fanout::uid_copies` from the webdav suite (the `za` module is `pub`).
- Produces: nothing new.

- [ ] **Step 1: The OPTIONS header**

In `crates/http/src/request.rs` replace the `DAV` header value (the `if za_is_key_account_request(..) { .. } else { .. }` expression at lines 341-351) with upstream's single string:

```rust
                        .with_header(
                            "DAV",
                            concat!(
                                "1, 2, 3, access-control, extended-mkcol, calendar-access, ",
                                "calendar-auto-schedule, calendar-no-timezone, addressbook"
                            ),
                        )
```

Delete `za_is_key_account_request` (its doc comment included) and any import it alone used. Run `cargo fmt --manifest-path crates/http/Cargo.toml -- --check` and the product compile check.

- [ ] **Step 2: The principal URLs**

In `crates/dav/src/principal/propfind.rs` delete the arm

```rust
                        // Spec 9: key accounts do not advertise scheduling.
                        PrincipalProperty::ScheduleInboxURL
                        | PrincipalProperty::ScheduleOutboxURL
                            if account.is_key_account() =>
                        {
                            fields_not_found.push(DavPropertyValue::empty(property.clone()));
                        }
```

so the two upstream arms answer for every account.

- [ ] **Step 3: The gating assertions**

In `tests/src/za/gating.rs` (lines 132-171):
- Replace the comment "Scheduling URLs are not advertised for key accounts." with "Invitations spec 8: scheduling URLs are advertised for key accounts again (release 1 withheld them)." and change the two `with_status(StatusCode::NOT_FOUND)` checks on key1's `inbox` and `outbox` properties to `.with_status(StatusCode::OK).is_not_empty()`, mirroring the plain principal's loop below them.
- Replace the comment "DAV OPTIONS: calendar-auto-schedule is not advertised to a key account (spec 9); ..." with "DAV OPTIONS (invitations spec 8): `calendar-auto-schedule` is advertised to a key account again; a non-key or anonymous request keeps upstream's header." and change `assert!(!key_dav.contains("calendar-auto-schedule"), "{key_dav}");` to `assert!(key_dav.contains("calendar-auto-schedule"), "{key_dav}");`.

The outbox free-busy check that follows (3.7 for a key target) is unchanged (spec 7).

- [ ] **Step 4: The key-mode `scheduling` variant**

In `tests/src/webdav/za_variants.rs`, `scheduling`, keep everything through the store-level checks of jane's pending item (the `envelope_wrap(..) == Some(2)` and `!..contains("sched-canary")` asserts) and replace the block from the comment "Clients do not see pending items (spec 6.6); ..." through the DELETE's assertions with:

```rust
    // Invitations spec 6: jane's next request drains the item before it is
    // answered. Her inbox lists the item (the collection's href and the
    // item's), its GET shows the invitation, and her default calendar
    // holds the copy, sealed under her master key.
    let inbox = jane
        .request_with_headers(
            "PROPFIND",
            "/dav/itip/jane@example.com/inbox/",
            [("depth", "1")],
            "",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_eq!(inbox.hrefs().len(), 2, "{:?}", inbox.hrefs());
    assert!(pending_ids(test, jane.account_id).await.is_empty());
    let item_id = items[0].0;
    let shown = jane
        .request(
            "GET",
            &format!("/dav/itip/jane@example.com/inbox/{item_id}.ics"),
            "",
        )
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(
        shown.contains("sched-canary") && shown.contains("METHOD:REQUEST") && !shown.contains("X-ZA-"),
        "{shown}"
    );
    assert_eq!(uid_copies(test, jane.account_id, "za-sched-1").await, 1);
    let copy = copy_href(test, jane.account_id, "za-sched-1").await;
    let copy_body = jane
        .request("GET", &copy, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(
        copy_body.contains("sched-canary")
            && copy_body.contains("ORGANIZER:mailto:john@example.com")
            && !copy_body.contains("X-ZA-")
            && !copy_body.contains("METHOD:"),
        "{copy_body}"
    );
    let (copy_archive, _) = crate::za::dav_seal::raw_event(test, jane.account_id, &format!("default/{}", copy.rsplit('/').next().unwrap())).await;
    assert!(!String::from_utf8_lossy(copy_archive.as_bytes()).contains("sched-canary"));
```

Keep the existing GET of john's event, its unfolded assertions, the `1.2` status and the schedule tag check. Then replace the DELETE block with:

```rust
    // john's DELETE cancels: a second item, a CANCEL, still no mail (spec
    // 5.5); jane's next request drains it and her copy is cancelled (spec
    // 4.3). The DELETE's task-queue check lives in the za suite.
    john.request("DELETE", "/dav/cal/john@example.com/default/s.ics", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    wait_for_delivery(test).await;
    assert_eq!(mail_count(test, jane.account_id).await, jane_mail);
    let items = new_inbox_items(test, jane.account_id, &jane_items).await;
    assert_eq!(items.len(), 2, "jane's inbox holds the REQUEST and the CANCEL");
    assert_eq!(item_method(&items[1].1.event), Some(ICalendarMethod::Cancel));
    assert_eq!(envelope_wrap(&items[1].1.event), Some(2));
    let cancelled = jane
        .request("GET", &copy, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(cancelled.contains("STATUS:CANCELLED"), "{cancelled}");
    assert!(pending_ids(test, jane.account_id).await.is_empty());
    // Cleanup: the copy with `Schedule-Reply: F` (no declined REPLY to
    // john), then the items by href.
    jane.request_with_headers("DELETE", &copy, [("schedule-reply", "F")], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
```

and keep the item-deletion loop, `delete_default_containers`, the mailbox destruction and `assert_is_empty` that follow. Add `copy_href`, `pending_ids` to the `crate::utils::za` import and `use crate::za::fanout::uid_copies;`.

- [ ] **Step 5: The manual checklist and CLAUDE.md**

In `docs/zero-access/manual-checklist.md` replace item 13 with:

```markdown
13. Invitations between two key accounts (release 2), with a client connected to each: from the first account, invite the second; confirm the invitation appears in the second client's inbox and on its calendar at its next refresh (no email is sent). Accept it from the second client; confirm the first client shows the attendee as accepted. Move the event from the first client; confirm the second client shows the new time. Delete the event from the first client; confirm the second client shows it cancelled. Repeat the invitation once with an external address alongside the second account; confirm the client shows delivery to the external address as not attempted (`SCHEDULE-STATUS` 5.3, shown as a delivery failure by clients that show it) and that nothing is sent.
```

In `CLAUDE.md`:
- In the "Zero-access fork" status paragraph, after the plan 9 sentence, add: "Plan 10 (the drain, the lease sweep, the reopened scheduling gates, the end-to-end flows; `docs/superpowers/plans/2026-10-15-zero-access-10-drain.md`) is in progress on `release2-plan10-drain`. The drain lives in `crates/groupware/src/calendar/drain.rs` (`ZaDrain::za_drain`, called from `handle_dav_request` in `crates/dav/src/request.rs` for every DAV request whose token holds session keys; `DRAIN_RETRY_SECS = 30` is a constant), its per-session state in `vault::session::ZaDrainState`, and the lease sweep in `common::za::lease::sweep_expired_uid_leases`, called from the maintenance task's `PurgeData` step."
- In the test-harness paragraph add: "The za suite skips every account's drain after setup (`test_hooks::set_drain_skip(DRAIN_SKIP_ALL)`), so modules that read pending items at store level keep working; the drain modules (`tests/src/za/drain.rs`, `leak::test_drain`) turn it on with the `DrainOn` guard. Test hooks: `FAN_OUT_PAUSE`, `PUT_PAUSE`, `COMMIT_FAIL_ONCE`, `TASK_CONSUMER_PAUSE`, `DRAIN_LOOKUP_PAUSE`, `DRAIN_LEASE_PAUSE`, `DRAIN_FAIL_ONCE`, the drain skip, the lease and retry overrides (`crates/common/src/za/test_hooks.rs`)."
- In the gating sentence of the status paragraph ("Key-account gates that look an account up by id ..."), nothing changes; add to the "Where the code lives" list: "`crates/groupware/src/calendar/drain.rs`: the drain (invitations spec 6)."

- [ ] **Step 6: Run everything this task touches**

Run: the product compile check (`clean`), `cargo check -p tests --tests`, fmt for `http`, `dav`, `tests`; the za suite; `webdav_tests` plain and with `ZA_KEY_ACCOUNTS=1`.
Expected: all pass. The plain `webdav_tests` run is unaffected by this task; the key-mode run now passes its `scheduling` variant.

- [ ] **Step 7: Commit**

```bash
git add crates/http/src/request.rs crates/dav/src/principal/propfind.rs tests/src/za/gating.rs tests/src/webdav/za_variants.rs docs/zero-access/manual-checklist.md CLAUDE.md
git commit -m "Reopen the scheduling gates for key accounts; drained copy in the key-mode suite

Invitations spec 8: calendar-auto-schedule and the principal's scheduling
URLs are advertised to key accounts again. The key-mode scheduling variant
asserts the drained copy, the update and the cancel; manual checklist item
13 becomes the two-client invitation flow.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```

### Task 7: Mutation runs, final verification, the spec names, and the outcome note

**Files:**
- Modify: `docs/superpowers/specs/2026-10-11-zero-access-invitations-design.md` (6.2, 6.3 step 4, 11, 13)
- Create: `docs/superpowers/plans/2026-10-15-zero-access-plan10-outcome.md`

- [ ] **Step 1: Mutation runs**

One at a time, each reverted with `git checkout -- <file>` and confirmed with an empty `git status --short crates/` before the next. Run the za suite for each. Record the id, the edit, the first failing assertion (`file:line` and message) and the command in the outcome note's table. Line numbers below are approximate; find the code by its comment.

| Id | Check | Edit | Expected first failure |
|----|-------|------|------------------------|
| M29 | The token check (spec 3.3, "Verification") | in `merge`, make the `Incoming::Reply` guard never drop: replace `&& !pairs.as_ref().is_some_and(..)` with `&& false` | `drain::test_reply` step 2: "a forged reply was kept" |
| M30 | The lease's fence on the import (spec 6.3 step 3) | in `import`, delete `lease.lease().assert_in(&mut batch);` | `drain::test_lease` case C: "the expired holder committed" (two copies) |
| M31 | The lease on the import (spec 6.3 step 1) | in `import`, replace the `acquire` with an unconditional import: `let Some(lease) = ..` becomes a lease taken from `ZaUidLease::acquire` on a UID suffixed with the document id (`&format!("{uid}#{}", ctx.document_id)`), so two items never contend | `drain::test_lease` case A: "a second copy was imported" (if session 2 imports past the lease first: "session 2 imported past the lease") |
| M32 | Pair adoption on merge (spec 3.3, 6.4) | in `merge`, make the `Incoming::Request(Some(pair))` arm a no-op | `drain::test_adoption` step 1: "the copy did not adopt the new token", or the reply under the new token not applying |
| M33 | The stale-reply check (spec 6.4, REPLY) | in `merge`, replace `is_stale_reply(&snapshots, &itip_snapshots)` with `false` | `drain::test_reply` step 3: "the stale decline applied" |
| M34 | The sender check (spec 6.4 step 2) | in `drain_item`, replace `if !sender_matches` with `if false` | `drain::test_errors` step 2: "a mismatched sender's invitation was imported" |
| M35 | The cursor rule (spec 6.2) | in `drain_pass`, make the `Ok(false)` and `Err` arms call `state.complete(snapshot)` | `drain::test_cursor` step 2: "the retry did not drain" (the item never drains again without an arrival) |
| M36 | The sweep's assertion (spec 6.3 step 4) | in `clear_uid_leases`, delete the `batch.assert_value(..)` line | `cargo test -p common za::lease`: `the_sweep_keeps_a_lease_retaken_after_its_scan`, "the sweep removed the new holder's lease" |

If a mutation survives, the covering test is wrong, not the mutation: add the assertion that catches it to the owning test, rerun, and record it.

- [ ] **Step 2: Final verification**

Run, in this order, on the clean tree, recording each result: `cargo test -p vault -p groupware`, `cargo test -p common za::lease`, `cargo test -p common auth::vault`, the za suite plain, the za suite with `BLOB_STORE=FileSystem`, `webdav_tests` plain, `webdav_tests` with `ZA_KEY_ACCOUNTS=1`, `cargo fmt --all --check`, the product compile check, and `cargo build --release -p stalwart --no-default-features --features rocks`. A `cal_itip` failure is rerun once.

- [ ] **Step 3: The spec's settled names**

In `docs/superpowers/specs/2026-10-11-zero-access-invitations-design.md`:
- 6.2, the cursor paragraph: replace "(`ZA_DRAIN_RETRY_SECS` in this spec, a name to confirm in planning)" with "(`DRAIN_RETRY_SECS`, a constant of 30 seconds with a test-mode override; plan 10 settled on no environment knob)".
- 6.3 step 4: after "The fork adds one call to the maintenance task's purge step (`crates/services/src/task_manager/maintenance.rs`)" add ", `sweep_expired_uid_leases`, in the `PurgeData` arm after `purge_in_memory_store`".
- 11, the lease-sweep bullet: replace "a decision for planning (section 13)" with "not added: the sweep reads and writes the data store on every configuration (plan 10)".
- 11, the key-mode suite paragraph: replace "Which of upstream's `cal_scheduling` assertions can now run unchanged in key mode is settled in planning." with "Upstream's `cal_scheduling` test still does not run in key mode; the variant asserts the drained copy, the update and the cancel (plan 10)."
- 13: in the "Names" bullet replace "the drain's retry backoff (`ZA_DRAIN_RETRY_SECS`, 30 seconds)" with "the drain's retry backoff (`DRAIN_RETRY_SECS`, a constant of 30 seconds)"; in the "mid-session trigger" bullet add "Plan 10 took the first option."; in "The lease sweep" bullet add "Plan 10: after `purge_in_memory_store` in the `PurgeData` arm, on every configuration, no Redis container test."

- [ ] **Step 4: The outcome note**

Create `docs/superpowers/plans/2026-10-15-zero-access-plan10-outcome.md` in the shape of the plan 9 note: "Where things stand" (the commits per task, the suite runs of Step 2 with counts), "Names settled (spec 13)" (the table above, with any change), "Rulings" (every ledger ruling, each with its reason and cost), "Mutation runs (plan 10)" (the table of Step 1, with the real first failures), "Facts for release 3" (what iMIP, the sealed notification email and the RSVP page will need from the drain: the `Incoming` arms, where a mail-borne item would enter, `token_hits`'s MIME gap), and "Deferred findings" (every review finding not fixed, one line each, tagged with its review; carry plan 9's F2, the 412-after-delivered-items consequence, with how the adoption rule of this plan, "the pair of the item processed last", bears on it, for Jay's spec 5.4 decision). Then update `CLAUDE.md`'s plan 10 sentence to name the outcome note.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/specs/2026-10-11-zero-access-invitations-design.md docs/superpowers/plans/2026-10-15-zero-access-plan10-outcome.md CLAUDE.md
git commit -m "Plan 10 outcome note, mutation runs and the spec's settled names

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr"
```
