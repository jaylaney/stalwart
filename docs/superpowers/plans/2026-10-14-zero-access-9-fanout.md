# Zero-access plan 9: the fan-out Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A key account's own CalDAV request delivers its iTIP messages as sealed pending items in the scheduling inboxes of internal key recipients instead of queueing mail: the `SendSealed` state, the recipient split and its SCHEDULE-STATUS codes, the per-recipient item write, every path that fans out (PUT create and update, event and collection DELETE, the attendee's reply), token maintenance, and the UID creation lease in the PUT create branch.

**Architecture:** Release 2 (invitations between key accounts) is split into three plans. Each one produces working, tested software on its own:
- **Plan 8 (done, PR #34): key material and sealed inbox items.** The `pk` wrap, the private key in session keys, token pairs in format 2 bundles, `seal_pending_item` / `open_pending_item`, the pending flag, and the inbox read paths that hide pending items.
- **Plan 9 (this plan): the fan-out.** It adds:
  - `ItipSendStatus::SendSealed` in place of `KeyAccount` (spec 5.1). Upstream's builders run unchanged on the plaintext tree; the messages go to the fan-out, never to `ItipMessages::queue` (invariant 12).
  - `ZaFanOut` on `Server` (new `crates/groupware/src/calendar/fanout.rs`): token maintenance (3.3), the recipient split and SCHEDULE-STATUS (5.2), the item write with the quota and size checks (5.3 steps 4-5), in the order of 5.4.
  - The DELETE paths (5.5): `delete.rs` drops its key-account term, and `DestroyArchive::delete_all` gains a sealed sibling that unseals before `itip_cancel` and returns the CANCEL (or the attendee's declined REPLY) for the fan-out.
  - The creation lease (6.3 steps 1-6, PUT side; new `crates/common/src/za/lease.rs`) with the archive-based uniqueness re-check in the PUT create branch.
  - Test-mode hooks (spec 11; new `crates/common/src/za/test_hooks.rs`), reachable from `dav`, `groupware` and `services`.

  **Plan 9 does not drain.** Pending items stay pending. Nobody reads them except tests, which open them with the recipient's private key through plan 8's `open_pending_item` after unlocking the account's keys (`za_unlock`, then `Server::za_private_key`).
- **Plan 10: the drain, its hooks (drain pause, drain skip), the lease sweep, the end-to-end flows, the remaining mutation checks, the key-mode variant's drained-copy assertion and the manual checklist.** Plan 10 calls:
  - `ZaUidLease::{acquire, assert_in, release}` for the drain's import, and `lease_key_range` / `lease_value_expiry` for the sweep;
  - `ZaFanOut::za_fan_out` for the delegation fan-out of spec 6.4;
  - the hook module (`common::za::test_hooks`), to which it adds the drain pause and the drain skip;
  - the test helpers `open_pending_item_as`, `inbox_items`, `new_inbox_items`, `hold_task_consumer`, `itip_tasks_for` (`tests/src/utils/za.rs`).

The fork diff stays narrow:
- new modules: `crates/groupware/src/calendar/fanout.rs`, `crates/common/src/za/`;
- insertions at upstream sites: both PUT branches (`update.rs`), the DELETE handler (`delete.rs`), the `delete*` methods of `DestroyArchive` (upstream's signatures stay as wrappers), three helpers next to `assert_is_unique_uid` (`dav/src/calendar/mod.rs`), one hook line in the task consumer loop;
- `ItipSendStatus` loses `KeyAccount` and gains `SendSealed`.

**Tech Stack:**
- Rust 2024 workspace (Stalwart 0.16.25 fork)
- `sha2 0.11`, `xxhash-rust 0.8` (xxh3), `rand 0.10` (re-exported as `store::rand`), `tokio`
- calcard 0.3, rkyv 0.8.18
- the `tests` crate harness (`TestServer`, `DummyWebDavClient`, `HttpRequest`)

**Spec:** `docs/superpowers/specs/2026-10-11-zero-access-invitations-design.md`. These sections bind this plan:
- 3.3: token generation and maintenance at the fan-out.
- 4: the flow overview.
- 5: all of it (sending state, recipient split, per-recipient item, order and partial failure, the paths that fan out).
- 6.3 steps 1-6 as they apply to the PUT create branch (the lease, its value, the fenced commit, release). The sweep is plan 10's.
- 9: the stored shapes plan 8 built.
- 10: error rules.
- 11: the test-mode hooks, the integration tests named below, the leak and mutation checks, the key-mode variant.
- 12: invariants 11-15.
- 13: the names settled below.

`docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md` (release 1) also binds: sections 8 (write paths), 9 (session keys on every DAV request) and 12 (invariants 1-10).

Project rules: `CLAUDE.md`, `docs/superpowers/plans/README-dev.md`, `docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md`. Plan 8's outcome note (`docs/superpowers/plans/2026-10-13-zero-access-plan8-outcome.md`) gives the interfaces this plan calls.

## Global Constraints

Commands:
- Every cargo command starts with `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`.
- Product build: `cargo build --release -p stalwart --no-default-features --features rocks`. Never build it with the `enterprise` feature.
- Per-task compile check in the shipping configuration: `cargo check -p stalwart --no-default-features --features rocks`, filtered with:
  ```bash
  ... 2>&1 | grep -E "^(warning|error)" -A3 | grep -E "crates/(groupware|dav)/|crates/services/src/task_manager/manager.rs|crates/common/src/(za/|auth/vault.rs)" || echo clean
  ```
  The broader filter `crates/common/src/auth/` matches four upstream warnings (`permissions.rs:48`; `authentication.rs:661`, `:698`, `:720`), and `crates/services/` matches upstream's two in `task_manager/index.rs`. Neither belongs to this plan.
- Test-mode compile check: `cargo check -p services --features test_mode`. The product build never enables `test_mode`: `cargo tree -p stalwart --no-default-features --features rocks -e features -i common | grep test_mode` prints nothing.
- Unit tests:
  - `cargo test -p groupware`. No feature is needed; the fan-out's hook site is behind `test_mode` and compiles out.
  - `cargo test -p common za::lease`. `-p common` without `enterprise` compiles for unit tests; `--features test_mode` does not (upstream's telemetry test data), so the hook module has no unit tests of its own. The integration tests exercise every hook.
  - `cargo test -p vault` is unchanged by this plan.
- Integration suite: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture`. It runs one server on port 8899. Never run two suites at once, and never in parallel with a mutation build.
- CalDAV suites: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture`, plain and with `ZA_KEY_ACCOUNTS=1`. Upstream's `cal_itip` sub-test is timing-flaky (DTSTAMP index mismatch, `tests/src/webdav/cal_itip.rs`): rerun once before treating a failure there as a regression.

Formatting and warnings:
- Run `cargo fmt -p <crate> -- --check` for every touched crate: `common`, `groupware`, `dav`, `services`, `tests`. This plan does not touch the http crate.
- The build output stays warning-free for new code. The 24 upstream warnings in store, common, jmap and services are known.

Invariants and code rules:
- No edits to generated code (`crates/registry/src/schema/*`, `crates/trc/src/event/enums.rs`).
- No new setting, permission or event type. The one new environment knob is `ZA_UID_LEASE_SECS`.
- No stored struct changes layout (invariant 1). Items are existing `CalendarEventNotification` records; the lease is an existing in-memory-class key.
- Nothing derived from a key account's invitation reaches the task queue, mail or any store location in plaintext (invariant 12). `ItipMessages::queue` is never called for `SendSealed` messages or for a key account's DELETE.
- The `pk` wrap is opened only by `open_pending_item` (invariant 11). No product path opens one in plan 9; only tests do, through `open_pending_item_as`.
- The organizer's event never stores an attendee's account id (invariant 13). The address-to-account mapping exists only inside one `za_fan_out` call.
- Upstream's builders run unchanged (invariant 15). Non-key accounts take unchanged paths (invariant 9): every new call sits behind a key-account branch (`SendSealed`, `za_keys.is_some()`), and the upstream-signature `delete*` wrappers pass `None`.
- Key material, tokens included, lives in `Zeroizing` buffers with a silent `Debug`. Traces name account ids and document ids only: never an address, a tree or a token.
- New source files start with the SPDX header copied from a neighbour:
  ```
  /*
   * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
   *
   * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
   */
  ```
- Keep the fork diff narrow: new modules (`fanout.rs`, `common/src/za/`) plus one-call insertions at the PUT branches, the DELETE paths, `delete_all`, and the task consumer.

Tests:
- Every za sub-module removes what it creates. `assert_is_empty` runs once, at the suite end (`tests/src/za/mod.rs`, after `destroy_key_accounts`); no sub-module calls it.
  - New modules delete their events with `Schedule-Reply: F` (RFC 6638), which builds no CANCEL, and delete their inbox items by href (`<document id>.ics`, upstream's DELETE, unchanged for pending items).
  - Accounts outlive the modules: key1, key2, key3 and key5 (all with password `STRONG`) until the suite end; `plain@example.com` beyond it, so its mailboxes are destroyed where a module counted them.
- `DummyWebDavClient` times out after 500 ms. A PUT that waits on a lease or a pause goes through `HttpRequest::send_full` (5 s).
- Pending items are hidden from clients (plan 8). Tests read them at store level (`inbox_items`) and open them with `open_pending_item_as`.
- The leak module's alarm wait is `wait_for_delivery` (plan 8's fix wave).
- Never weaken an assertion to make a test pass.
  - Values read from code and never run are marked "(from code reading)".
  - If a run disagrees with one, assert what the run shows only where the step names an oracle or a fallback, and record a ruling in the task report.
  - Otherwise stop and report.
- Mutation checks are temporary edits under `crates/`, undone at once. Every mutated file was committed by an earlier task, so revert with `git checkout -- <file>`, then confirm with `git status --short crates/` that only the task's own changes remain (none, for a test-only task) before the next edit and before the task's commit.
- The new sub-modules run right after `dav_seal::test_legacy`, before `tracing` and `gating`. `gating::test_scheduling` also sends a key organizer's invitation to plain, so a mutation of the recipient split or of the queue path would otherwise fail there first instead of in the test that owns the check.

Process:
- Code in this plan was written without a compiler. Small type and import fixes are expected; changes of behaviour are not.
- Work on `release2-plan9-fanout` (cut from the plan 8 branch at `81cb6fbe`). Do not push.
- Commit messages: subject, blank line, body, then two trailer lines exactly as the harness's attribution reminder gives them at run time: `Co-Authored-By: ...` and `Claude-Session: ...`. Plan 8's commits carried `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01SmmW3Hx6r7uzLjDdJHEtkr`. A wrong trailer is rewritten before the branch is shared.
- The final verification (Task 8) runs the unit suites, `za_tests` plain and with `BLOB_STORE=FileSystem`, `webdav_tests` in both modes, and the release build.

### Names this plan settles (spec 13)

| Name | Value | Where |
|---|---|---|
| Sending state | `ItipSendStatus::SendSealed`: upstream's `Send` preconditions, for a key account; `KeyAccount` is removed. `builds_messages()` (`Send` or `SendSealed`), `is_send_sealed()`; `is_send()` stays `Send` only | `crates/groupware/src/calendar/itip.rs` |
| Fan-out entry | trait `ZaFanOut` on `Server`, one method: `za_fan_out(&self, sender: &AccountInfo, keys: &SessionKeys, messages: Vec<ItipMessage<ICalendar>>, stored_pairs: Option<TokenPairs>, event: &ICalendar) -> trc::Result<ZaFanOutResult>` | new `crates/groupware/src/calendar/fanout.rs` |
| Fan-out result | `ZaFanOutResult { pairs: Option<TokenPairs>, statuses: Vec<ZaScheduleStatus> }`; `ZaScheduleStatus { address, on_organizer, status }` | `fanout.rs` |
| SCHEDULE-STATUS writer | `za_set_schedule_status(&mut ICalendar, &[ZaScheduleStatus])`: replaces an earlier fan-out status, keeps upstream's `3.7` | `fanout.rs` |
| SCHEDULE-STATUS codes | `ITIP_STATUS_DELIVERED = "1.2"` (upstream's `imip.rs` sets no success code for local delivery), `ITIP_STATUS_NOT_ATTEMPTED = "5.3"`, `ITIP_STATUS_DELIVERY_FAILED = "5.1"` | `crates/groupware/src/scheduling/itip.rs`, next to `ITIP_STATUS_INVALID_USER` |
| Token source | `store::rand::random::<[u8; 16]>()` in a `Zeroizing` buffer: rand 0.10's thread-local CSPRNG, the source `vault::keys::Secret::random` uses | `fanout.rs` |
| DELETE messages | `ZaDeleteMessage { message, pairs, event }`; `DestroyArchive::{delete_za, delete_all_za, delete_with_events_za}` with `za_keys: Option<&SessionKeys>`; upstream's `delete`, `delete_all`, `delete_with_events` delegate with `None` | `fanout.rs`, `crates/groupware/src/calendar/storage.rs` |
| Unseal with pairs | `unseal_event_pairs(&mut CalendarEvent, &SessionKeys, account_id) -> Result<Option<TokenPairs>, SealError>` | `crates/groupware/src/calendar/seal/event.rs` |
| Lease prefix | `KV_LOCK_ZA_UID: u8 = 27`, next to `KV_LOCK_DAV` | `crates/common/src/lib.rs` |
| Lease key | `KV_LOCK_ZA_UID \|\| account_id (u32 BE) \|\| SHA-256(uid)`, 37 bytes whatever the UID | `crates/common/src/za/lease.rs` |
| Lease value | `expiry (u64 BE, seconds) \|\| fence (16 random bytes)` | `lease.rs` |
| Lease duration | `DEFAULT_LEASE_SECS = 30`; `ZA_UID_LEASE_SECS`, read once per process (unset, malformed or 0: the default); test-mode override `set_lease_secs_override` | `lease.rs`, `test_hooks.rs` |
| Lease retries | `LEASE_RETRIES = 20`, `LEASE_RETRY_WAIT = 50 ms`, then `None` | `lease.rs` |
| Lease API | `ZaUidLease::{acquire, assert_in, release}`, `ZaUidLeaseGuard` (releases on drop), `lease_key`, `lease_key_range`, `lease_value_expiry` | `lease.rs` |
| PUT helpers | `za_take_uid_lease` (lease, then archive re-check; 503 when busy), `za_assert_is_unique_uid` (archive-based), `za_commit_create` (fenced commit, release, lost-fence path) | `crates/dav/src/calendar/mod.rs` |
| Busy lease | 503, traced as `Store(AssertValueFailed)` with details `zero-access: UID creation lease busy` | `mod.rs` |
| Test-mode hooks | `FAN_OUT_PAUSE`, `PUT_PAUSE`, `COMMIT_FAIL_ONCE`, `TASK_CONSUMER_PAUSE`, and the lease-duration override | `crates/common/src/za/test_hooks.rs` |

## Review Focus

Each line names an input the spec implies but does not walk through, with the behaviour a person would expect. Each has a test in the task that owns the code.

1. **The same attendee listed twice, differing in case** (`KEY2@Example.com` and `key2@example.com`).
   - Expected: one item, one pair, and `1.2` on both ATTENDEE properties.
   - Upstream's builders dedupe recipients by sanitized (lower-case) address (`Email::new`, `utils::sanitize_email`). The fan-out keys pairs and statuses the same way, so a client that repeats an attendee does not get two tokens or two invitations.
   - Task 3.
2. **An update that removes every ATTENDEE.**
   - Expected: only CANCELs go out, each carrying the removed attendee's old pair; the organizer's bundle then holds no pair (`event_token_pairs` is `None`); the PUT answers 204.
   - Task 3.
3. **An attendee address that resolves to the sender's own account**, here the sub-address `key1+fo@example.com` (sub-addressing is on by default, `SubAddressing::Enabled`).
   - Expected: not attempted (`5.3`), and nothing in the sender's own inbox.
   - Upstream's builders skip the account's listed addresses (`is_local`) but not a sub-address, so the fan-out sees it.
   - Task 3.
4. **A recipient that has never made a DAV request** (key5: no resource cache, no default calendar).
   - Expected: the item is written and the status is `1.2`.
   - The scheduling inbox is virtual (`SCHEDULE_INBOX_ID`, `crates/groupware/src/cache/calcard.rs:214`). Upstream's ingest writes its item with no get-or-create step, and so does the fan-out.
   - Task 3.
5. **A reply to an organizer whose vault record has no public key** (setup pending).
   - Expected: not attempted, `5.3` on the ORGANIZER property, the PUT answers 204, nothing written into the pending account.
   - Task 4.

## File map

| File | Change | Task |
|---|---|---|
| `crates/common/src/lib.rs` | `pub mod za;`, `KV_LOCK_ZA_UID` | 1 |
| `crates/common/src/za/mod.rs` | new: module list | 1 |
| `crates/common/src/za/lease.rs` | new: the creation lease, tests | 1 |
| `crates/common/src/za/test_hooks.rs` | new: test-mode hooks | 1 |
| `crates/{dav,groupware,services}/Cargo.toml` | `test_mode = ["common/test_mode"]` | 1 |
| `crates/services/src/task_manager/manager.rs` | consumer-loop hook | 1 |
| `crates/groupware/src/scheduling/itip.rs` | status constants | 2 |
| `crates/groupware/src/calendar/seal/event.rs`, `seal/mod.rs` | `unseal_event_pairs` | 2 |
| `crates/groupware/src/calendar/fanout.rs` | new: `ZaFanOut`, statuses, `ZaDeleteMessage`, tests | 2 |
| `crates/groupware/src/calendar/mod.rs` | `pub mod fanout;` | 2 |
| `crates/groupware/src/calendar/itip.rs` | `SendSealed` | 3 |
| `crates/dav/src/calendar/mod.rs` | lease and re-check helpers | 3 |
| `crates/dav/src/calendar/update.rs` | both PUT branches, `PUT_PAUSE` | 3 |
| `tests/src/utils/za.rs` | inbox, item, task-queue, hook and lease helpers (3); `plant_attendee_copy` (4) | 3, 4 |
| `tests/src/za/fanout.rs` | new: organizer fan-out (3), partial failures (4), task queue and DELETE (5) | 3, 4, 5 |
| `tests/src/za/tracing.rs`, `gating.rs`, `pairs.rs` | assertions that `SendSealed` changes | 3, 5 |
| `tests/src/webdav/za_variants.rs` | `scheduling`: store-level delivery (3), the CANCEL (5) | 3, 5 |
| `tests/src/za/reply.rs` | new: replies and delegation (4); the decline on DELETE (5) | 4, 5 |
| `crates/groupware/src/calendar/storage.rs` | `delete*_za` siblings, wrappers | 5 |
| `crates/dav/src/calendar/delete.rs` | `send_itip`, the fan-out before the commit | 5 |
| `tests/src/za/uid_lease.rs` | new: the creation lease, PUT side | 6 |
| `tests/src/za/leak.rs` | the fan-out in the scan, its token | 7 |
| `tests/src/za/mod.rs` | wiring before `leak::test` | 3, 4, 5, 6 |
| `docs/superpowers/plans/2026-10-14-zero-access-plan9-outcome.md` | new: mutation table, rulings, facts for plan 10 | 8 |

---

### Task 1: The UID creation lease and the test-mode hooks

**Files:**
- Modify: `crates/common/src/lib.rs:62-73` (module list), `:129` (after `KV_SIEVE_ID`)
- Create: `crates/common/src/za/mod.rs`, `crates/common/src/za/lease.rs`, `crates/common/src/za/test_hooks.rs`
- Modify: `crates/dav/Cargo.toml`, `crates/groupware/Cargo.toml`, `crates/services/Cargo.toml` (`[features]`)
- Modify: `crates/services/src/task_manager/manager.rs:302-304` (the consumer loop)
- Test: `crates/common/src/za/lease.rs` (tests module)

**Interfaces:**
- Consumes (existing):
  - `crate::auth::vault::RawValue` (`crates/common/src/auth/vault.rs:29-35`): raw stored bytes, so the value hash is computed exactly as the store's `AssertValue::Hash` computes it (`store/src/write/assert.rs:70`).
  - `store::write::{InMemoryClass, ValueClass, assert::AssertValue, BatchBuilder, now}`, `Store::{get_value, write}`, `store::rand`.
- Produces (Tasks 3, 5, 6 and plan 10):
  - `common::KV_LOCK_ZA_UID: u8 = 27`
  - `common::za::lease::ZaUidLease`:
    - `acquire(store: &Store, account_id: u32, uid: &str) -> trc::Result<Option<ZaUidLease>>` (`None` when the lease stayed held through the retries)
    - `assert_in(&self, batch: &mut BatchBuilder)` (the fenced-commit assertion)
    - `release(self, store: &Store) -> bool` (async; `false` when the key no longer holds this holder's value; traced)
  - `common::za::lease::ZaUidLeaseGuard`: `new(store: Store, lease: ZaUidLease)`, `lease(&self) -> &ZaUidLease`, `release(self) -> bool` (async), `abandon(self)`. Dropped without either, it releases from a spawned task.
  - `common::za::lease::{lease_key, lease_key_range, lease_value_expiry, lease_secs, DEFAULT_LEASE_SECS, LEASE_RETRIES, LEASE_RETRY_WAIT, FENCE_LEN, LEASE_KEY_LEN}`
  - `common::za::test_hooks` (`test_mode` only): `Pause`; `set_fan_out_pause`, `set_put_pause`, `set_task_consumer_pause` (each `fn(Option<Arc<Pause>>)`); `set_commit_fail_once(Option<u32>)`; `set_lease_secs_override(Option<u64>)`, `lease_secs_override() -> Option<u64>`; the sites `fan_out_pause_point(recipient_id)`, `put_pause_point(account_id)`, `task_consumer_pause_point()` (async) and `take_commit_fail(account_id) -> bool`.

Background:
- Spec 6.3 step 1: the lease is written directly through `Server::store()` as `ValueClass::InMemory(InMemoryClass::Key(..))`, as `try_lock` (`crates/store/src/dispatch/lookup.rs:329-385`) writes one when the in-memory store is the data store. Never through the in-memory store dispatch: with Redis as the lookup store, the lease would sit in another system than the event, and the fenced commit needs both in one transaction.
- Step 2: the value is `expiry (u64 BE) || fence`. The leading expiry keeps the key compatible with `purge_in_memory_store` (`lookup.rs:412-491`, which reads the first eight bytes), and the test harness's `store_lookup_expire_all` clears it like any expiring key.
- Step 3: the fenced commit asserts the whole value. `AssertValue::U64` compares only the last eight bytes, so the assertion is `AssertValue::Hash(xxh3_64(value))`, as the vault record's conditional write does (`auth/vault.rs:155`).
- `acquire` reads the key; while an unexpired value is present it waits 50 ms and retries, 20 times. Then it writes `expiry || fence`, asserting the value it read: absent, or the expired value's exact bytes. A malformed value (shorter than eight bytes) counts as expired.
- The key hashes the UID (SHA-256, `sha2` is already a `common` dependency), so every key is 37 bytes whatever the client sent.
- `ZA_UID_LEASE_SECS` is read once per process, like `ZA_KEY_IDLE_SECS` (`crates/vault/src/cache.rs:41-53`: unset or malformed uses the default). A running suite cannot change a value read once, so the test-mode override stands in for `ZA_UID_LEASE_SECS=1` in Task 6's expiry case.
- The hooks copy release 1's `Pause` shape (`crates/http/src/auth/authenticate.rs:250-325`): one-shot slots keyed by account id, `arrived` and `release` notifies, the mutex guard dropped before any await. The task-consumer pause is global and ignores the account id. None of them changes behaviour while unset.
- `dav`, `groupware` and `services` forward their `test_mode` to `common/test_mode`, so each hook site compiles under its own crate's feature. The `tests` crate already enables `test_mode` on all four.

- [ ] **Step 1: The module skeleton and the failing lease tests**

In `crates/common/src/lib.rs`, add `pub mod za;` after `pub mod telemetry;` (`:73`), and after `pub const KV_SIEVE_ID: u8 = 26;` (`:129`):

```rust
/// Invitations spec 6.3: the per-(account, UID) creation lease
/// (`za::lease`).
pub const KV_LOCK_ZA_UID: u8 = 27;
```

Create `crates/common/src/za/mod.rs` with the SPDX header and:

```rust
//! Zero-access helpers shared by the request paths and the background
//! services: the UID creation lease (invitations spec 6.3) and, in test
//! builds, the pause and failure hooks of spec 11.

pub mod lease;

#[cfg(feature = "test_mode")]
pub mod test_hooks;
```

Create `crates/common/src/za/lease.rs` with the SPDX header and only this tests module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use store::backend::ephemeral::EphemeralStore;

    async fn stored(store: &Store, account_id: u32, uid: &str) -> Option<Vec<u8>> {
        store
            .get_value::<RawValue>(ValueKey::from(lease_class(lease_key(account_id, uid))))
            .await
            .unwrap()
            .map(|raw| raw.0)
    }

    async fn write_raw(store: &Store, account_id: u32, uid: &str, value: Vec<u8>) {
        let mut batch = BatchBuilder::new();
        batch.set(lease_class(lease_key(account_id, uid)), value);
        store.write(batch.build_all()).await.unwrap();
    }

    fn expired_value(fence: u8) -> Vec<u8> {
        let mut value = (now() - 1).to_be_bytes().to_vec();
        value.extend_from_slice(&[fence; FENCE_LEN]);
        value
    }

    /// A batch fenced on `lease`, writing an unrelated key after the
    /// assertion, as the PUT create's insert does.
    async fn commit_fenced(store: &Store, lease: &ZaUidLease) -> trc::Result<()> {
        let mut batch = BatchBuilder::new();
        lease.assert_in(&mut batch);
        batch.set(lease_class(b"za-lease-test-payload".to_vec()), vec![0xFF; 8]);
        store.write(batch.build_all()).await.map(|_| ())
    }

    #[test]
    fn lease_key_is_fixed_length_and_binds_account_and_uid() {
        let key = lease_key(7, "uid-a");
        assert_eq!(key.len(), LEASE_KEY_LEN);
        assert_eq!(key[0], KV_LOCK_ZA_UID);
        assert_eq!(&key[1..5], &7u32.to_be_bytes());
        assert_eq!(&key[5..], Sha256::digest(b"uid-a").as_slice());
        assert_ne!(key, lease_key(7, "uid-b"));
        assert_ne!(key, lease_key(8, "uid-a"));
        assert_eq!(lease_key(7, &"u".repeat(2000)).len(), LEASE_KEY_LEN);
        let (from, to) = lease_key_range();
        assert!(from.class.serialize(0, 0, 0, 0) <= lease_class(key.clone()).serialize(0, 0, 0, 0));
        assert!(lease_class(key).serialize(0, 0, 0, 0) < to.class.serialize(0, 0, 0, 0));
    }

    #[test]
    fn lease_duration_knob_parsing() {
        assert_eq!(lease_secs_from(None), DEFAULT_LEASE_SECS);
        assert_eq!(lease_secs_from(Some("5")), 5);
        assert_eq!(lease_secs_from(Some(" 7 ")), 7);
        assert_eq!(lease_secs_from(Some("0")), DEFAULT_LEASE_SECS);
        assert_eq!(lease_secs_from(Some("soon")), DEFAULT_LEASE_SECS);
    }

    #[tokio::test]
    async fn acquire_writes_expiry_then_fence_and_refuses_a_held_lease() {
        let store = EphemeralStore::open();
        let before = now();
        let lease = ZaUidLease::acquire(&store, 1, "held")
            .await
            .unwrap()
            .expect("a free lease is taken");
        let value = stored(&store, 1, "held").await.expect("the key is written");
        assert_eq!(value.len(), U64_LEN + FENCE_LEN);
        let expiry = lease_value_expiry(&value).unwrap();
        assert!(expiry >= before + lease_secs() && expiry <= now() + lease_secs());
        assert!(
            ZaUidLease::acquire(&store, 1, "held").await.unwrap().is_none(),
            "a held lease is refused after the retries"
        );
        assert!(commit_fenced(&store, &lease).await.is_ok(), "the holder commits");
        assert!(lease.release(&store).await, "the holder releases");
        assert!(stored(&store, 1, "held").await.is_none());
    }

    #[tokio::test]
    async fn an_expired_holder_is_fenced_out() {
        let store = EphemeralStore::open();
        // The old holder's lease expired; its value is still stored.
        write_raw(&store, 2, "taken", expired_value(7)).await;
        let old = ZaUidLease {
            key: lease_key(2, "taken"),
            value: expired_value(7),
        };
        let new = ZaUidLease::acquire(&store, 2, "taken")
            .await
            .unwrap()
            .expect("an expired lease is taken over");
        assert_ne!(stored(&store, 2, "taken").await.unwrap(), old.value);
        let err = commit_fenced(&store, &old).await.unwrap_err();
        assert!(err.is_assertion_failure(), "the old holder cannot commit");
        assert!(!old.release(&store).await, "nor remove its successor's lease");
        assert!(commit_fenced(&store, &new).await.is_ok());
        assert!(new.release(&store).await);
        // A released lease fences out its own holder too.
        let gone = ZaUidLease::acquire(&store, 2, "taken").await.unwrap().unwrap();
        let copy = ZaUidLease {
            key: gone.key.clone(),
            value: gone.value.clone(),
        };
        assert!(gone.release(&store).await);
        assert!(commit_fenced(&store, &copy).await.unwrap_err().is_assertion_failure());
    }

    #[tokio::test]
    async fn a_malformed_value_counts_as_expired_and_fences_differ() {
        let store = EphemeralStore::open();
        write_raw(&store, 3, "short", vec![1, 2, 3]).await;
        let a = ZaUidLease::acquire(&store, 3, "short").await.unwrap();
        assert!(a.is_some(), "a value with no expiry is replaced");
        let b = ZaUidLease::acquire(&store, 3, "other").await.unwrap().unwrap();
        assert_ne!(a.unwrap().value[U64_LEN..], b.value[U64_LEN..]);
    }

    #[tokio::test]
    async fn a_dropped_guard_releases_and_an_abandoned_one_does_not() {
        let store = EphemeralStore::open();
        let lease = ZaUidLease::acquire(&store, 4, "guard").await.unwrap().unwrap();
        drop(ZaUidLeaseGuard::new(store.clone(), lease));
        for _ in 0..50 {
            if stored(&store, 4, "guard").await.is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(stored(&store, 4, "guard").await.is_none(), "released on drop");
        let lease = ZaUidLease::acquire(&store, 4, "guard").await.unwrap().unwrap();
        ZaUidLeaseGuard::new(store.clone(), lease).abandon();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(stored(&store, 4, "guard").await.is_some(), "an abandoned lease stays");
    }
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p common za::lease 2>&1 | tail -10`
Expected: compile errors, `cannot find function 'lease_key'` and the other unresolved names.

- [ ] **Step 3: Implement the lease**

Prepend to `lease.rs`, after the SPDX header:

```rust
//! Invitations spec 6.3: the per-(account, UID) creation lease with fenced
//! commits. A key account's session that creates an event for a UID (a PUT
//! create here; the drain's import in plan 10) holds the lease from its
//! absence check through the insert, and the insert batch asserts that the
//! lease still holds this holder's exact value. An expired holder whose
//! lease was taken over therefore cannot commit; the duration only decides
//! how long a crashed holder blocks others.
//!
//! The key lives in the data store as an in-memory-class key, written
//! through `Server::store()` the way `try_lock` writes one when the
//! in-memory store is the data store, and never through the in-memory store
//! dispatch: the fenced commit needs the lease and the event in one
//! transaction.

use crate::{KV_LOCK_ZA_UID, auth::vault::RawValue};
use sha2::{Digest, Sha256};
use std::{sync::LazyLock, time::Duration};
use store::{
    Store, U64_LEN, ValueKey, rand,
    write::{BatchBuilder, InMemoryClass, ValueClass, assert::AssertValue, now},
};
use trc::AddContext;
use xxhash_rust::xxh3::xxh3_64;

/// Lease duration when `ZA_UID_LEASE_SECS` is unset (spec 6.3).
pub const DEFAULT_LEASE_SECS: u64 = 30;
/// Further attempts at a held lease, `LEASE_RETRY_WAIT` apart.
pub const LEASE_RETRIES: u32 = 20;
pub const LEASE_RETRY_WAIT: Duration = Duration::from_millis(50);
/// Length of the random fence token in a lease value.
pub const FENCE_LEN: usize = 16;
/// Prefix, account id (4 bytes), SHA-256 of the UID (32 bytes).
pub const LEASE_KEY_LEN: usize = 1 + 4 + 32;

static LEASE_SECS: LazyLock<u64> =
    LazyLock::new(|| lease_secs_from(std::env::var("ZA_UID_LEASE_SECS").ok().as_deref()));

/// `ZA_UID_LEASE_SECS`: unset, malformed or zero uses the default.
fn lease_secs_from(value: Option<&str>) -> u64 {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_LEASE_SECS)
}

/// How long a new lease lasts: `ZA_UID_LEASE_SECS`, read once per process,
/// or in test builds the override a test set.
pub fn lease_secs() -> u64 {
    #[cfg(feature = "test_mode")]
    if let Some(secs) = crate::za::test_hooks::lease_secs_override() {
        return secs;
    }
    *LEASE_SECS
}

/// `KV_LOCK_ZA_UID || account_id (u32 BE) || SHA-256(uid)`: fixed length
/// whatever the UID.
pub fn lease_key(account_id: u32, uid: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(LEASE_KEY_LEN);
    key.push(KV_LOCK_ZA_UID);
    key.extend_from_slice(&account_id.to_be_bytes());
    key.extend_from_slice(Sha256::digest(uid.as_bytes()).as_slice());
    key
}

/// The key range that holds every lease (plan 10's sweep iterates it).
pub fn lease_key_range() -> (ValueKey<ValueClass>, ValueKey<ValueClass>) {
    (
        ValueKey::from(lease_class(vec![KV_LOCK_ZA_UID])),
        ValueKey::from(lease_class(vec![KV_LOCK_ZA_UID + 1])),
    )
}

/// The leading expiry of a lease value; `None` for a value too short to
/// hold one.
pub fn lease_value_expiry(value: &[u8]) -> Option<u64> {
    value
        .get(..U64_LEN)
        .and_then(|bytes| <[u8; U64_LEN]>::try_from(bytes).ok())
        .map(u64::from_be_bytes)
}

fn lease_class(key: Vec<u8>) -> ValueClass {
    ValueClass::InMemory(InMemoryClass::Key(key))
}

/// A held lease: its key and this holder's exact value
/// (`expiry (u64 BE, seconds) || fence (16 random bytes)`).
pub struct ZaUidLease {
    key: Vec<u8>,
    value: Vec<u8>,
}

impl ZaUidLease {
    /// Spec 6.3 step 1. Reads the key; while it holds an unexpired value,
    /// waits `LEASE_RETRY_WAIT` and tries again, `LEASE_RETRIES` times. Then
    /// writes `expiry || fence`, asserting the value read (absent, or the
    /// expired value's exact bytes); a write that loses its assertion to
    /// another holder counts as an attempt. `None` when every attempt found
    /// the lease held.
    pub async fn acquire(
        store: &Store,
        account_id: u32,
        uid: &str,
    ) -> trc::Result<Option<ZaUidLease>> {
        let key = lease_key(account_id, uid);
        for attempt in 0..=LEASE_RETRIES {
            if attempt > 0 {
                tokio::time::sleep(LEASE_RETRY_WAIT).await;
            }
            let current = store
                .get_value::<RawValue>(ValueKey::from(lease_class(key.clone())))
                .await
                .caused_by(trc::location!())?;
            let now = now();
            if current
                .as_ref()
                .and_then(|raw| lease_value_expiry(&raw.0))
                .is_some_and(|expiry| expiry > now)
            {
                continue;
            }
            let mut value = Vec::with_capacity(U64_LEN + FENCE_LEN);
            value.extend_from_slice(&(now + lease_secs()).to_be_bytes());
            value.extend_from_slice(&rand::random::<[u8; FENCE_LEN]>());
            let mut batch = BatchBuilder::new();
            batch.assert_value(
                lease_class(key.clone()),
                match &current {
                    Some(raw) => AssertValue::Hash(xxh3_64(&raw.0)),
                    None => AssertValue::None,
                },
            );
            batch.set(lease_class(key.clone()), value.clone());
            match store.write(batch.build_all()).await {
                Ok(_) => return Ok(Some(ZaUidLease { key, value })),
                Err(err) if err.is_assertion_failure() => continue,
                Err(err) => return Err(err.caused_by(trc::location!())),
            }
        }
        Ok(None)
    }

    /// Spec 6.3 step 3: adds to `batch` the assertion that the key still
    /// holds this holder's exact value. Call it before the insert, so the
    /// assertion and the insert share the batch's first commit point.
    pub fn assert_in(&self, batch: &mut BatchBuilder) {
        batch.assert_value(
            lease_class(self.key.clone()),
            AssertValue::Hash(xxh3_64(&self.value)),
        );
    }

    /// Spec 6.3 step 4: clears the key if it still holds this holder's
    /// value, so a holder never removes a successor's lease. Returns whether
    /// it cleared it. A failure is traced; the key then expires and plan
    /// 10's sweep removes it.
    pub async fn release(self, store: &Store) -> bool {
        let mut batch = BatchBuilder::new();
        batch.assert_value(
            lease_class(self.key.clone()),
            AssertValue::Hash(xxh3_64(&self.value)),
        );
        batch.clear(lease_class(self.key));
        match store.write(batch.build_all()).await {
            Ok(_) => true,
            Err(err) => {
                trc::error!(
                    err.details("zero-access: UID creation lease not released")
                        .caused_by(trc::location!())
                );
                false
            }
        }
    }
}

/// A lease held across a request's early returns. `release` clears it after
/// the commit; `abandon` gives up a lease whose fence was lost (the key
/// holds another holder's value, so nothing is written); dropped otherwise,
/// it releases from a spawned task.
pub struct ZaUidLeaseGuard {
    store: Store,
    lease: Option<ZaUidLease>,
}

impl ZaUidLeaseGuard {
    pub fn new(store: Store, lease: ZaUidLease) -> Self {
        ZaUidLeaseGuard {
            store,
            lease: Some(lease),
        }
    }

    pub fn lease(&self) -> &ZaUidLease {
        self.lease
            .as_ref()
            .expect("a guard holds its lease until release or abandon")
    }

    pub async fn release(mut self) -> bool {
        match self.lease.take() {
            Some(lease) => lease.release(&self.store).await,
            None => true,
        }
    }

    pub fn abandon(mut self) {
        self.lease = None;
    }
}

impl Drop for ZaUidLeaseGuard {
    fn drop(&mut self) {
        if let Some(lease) = self.lease.take()
            && let Ok(handle) = tokio::runtime::Handle::try_current()
        {
            let store = self.store.clone();
            handle.spawn(async move {
                lease.release(&store).await;
            });
        }
    }
}
```

- [ ] **Step 4: Run to see them pass**

Run: `cargo test -p common za::lease 2>&1 | tail -5`
Expected: PASS, 6 tests. `acquire_writes_expiry_then_fence_and_refuses_a_held_lease` takes about a second (the 20 retries).

Fallback: `key_range` comparison in the first test serializes keys with `ValueClass::serialize(account_id, collection, document_id, 0)` (`crates/store/src/write/key.rs:299-301` writes an in-memory key's bytes only). If that signature differs, compare the raw key vectors (`vec![KV_LOCK_ZA_UID] <= key < vec![KV_LOCK_ZA_UID + 1]`) instead and note it in the task report.

- [ ] **Step 5: The hooks, the features and the consumer site**

Create `crates/common/src/za/test_hooks.rs` with the SPDX header and:

```rust
//! Test-mode hooks of invitations spec 11, compiled only under `test_mode`
//! like release 1's auth-cache pauses (`za_test` in
//! `crates/http/src/auth/authenticate.rs`), whose `Pause` shape this copies.
//! None changes behaviour while unset. A pause is one-shot and keyed by
//! account id: only the first request for that account reaching the point
//! takes it; requests for other accounts pass and leave it set.
//!
//! - `FAN_OUT_PAUSE`: in the fan-out, between an item's document-id
//!   assignment and its commit, keyed by the recipient
//!   (`groupware::calendar::fanout`).
//! - `PUT_PAUSE`: in the CalDAV PUT handler right after the resource
//!   snapshot (`fetch_dav_resources`), keyed by the account
//!   (`dav::calendar::update`).
//! - `COMMIT_FAIL_ONCE`: the commit of a key account's PUT create fails once
//!   with an internal error after its fan-out, keyed by the account
//!   (`dav::calendar::za_commit_create`).
//! - `TASK_CONSUMER_PAUSE`: the task manager's consumer loop takes no task
//!   while it is held; global (`services::task_manager::manager`).
//! - The lease-duration override: stands in for `ZA_UID_LEASE_SECS`, which
//!   is read once per process.
//!
//! Plan 10 adds the drain pause and the drain skip here.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::Notify;

pub struct Pause {
    pub account_id: u32,
    pub arrived: Notify,
    pub release: Notify,
}

impl Pause {
    pub fn new(account_id: u32) -> Self {
        Pause {
            account_id,
            arrived: Notify::new(),
            release: Notify::new(),
        }
    }
}

pub static FAN_OUT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static PUT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

/// Global: its `account_id` is ignored.
pub static TASK_CONSUMER_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

/// The account whose next key-account PUT create fails its commit.
pub static COMMIT_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

/// Seconds a new creation lease lasts; 0 when unset.
static LEASE_SECS_OVERRIDE: AtomicU64 = AtomicU64::new(0);

fn set_slot(slot: &Mutex<Option<Arc<Pause>>>, pause: Option<Arc<Pause>>) {
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = pause;
}

pub fn set_fan_out_pause(pause: Option<Arc<Pause>>) {
    set_slot(&FAN_OUT_PAUSE, pause);
}

pub fn set_put_pause(pause: Option<Arc<Pause>>) {
    set_slot(&PUT_PAUSE, pause);
}

pub fn set_task_consumer_pause(pause: Option<Arc<Pause>>) {
    set_slot(&TASK_CONSUMER_PAUSE, pause);
}

pub fn set_commit_fail_once(account_id: Option<u32>) {
    *COMMIT_FAIL_ONCE.lock().unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn set_lease_secs_override(secs: Option<u64>) {
    LEASE_SECS_OVERRIDE.store(secs.unwrap_or(0), Ordering::SeqCst);
}

pub fn lease_secs_override() -> Option<u64> {
    Some(LEASE_SECS_OVERRIDE.load(Ordering::SeqCst)).filter(|secs| *secs > 0)
}

fn take_for(slot: &Mutex<Option<Arc<Pause>>>, account_id: u32) -> Option<Arc<Pause>> {
    let mut pause = slot.lock().unwrap_or_else(|e| e.into_inner());
    if pause.as_ref().is_some_and(|p| p.account_id == account_id) {
        pause.take()
    } else {
        None
    }
}

/// The guard is gone before this await: `take_for` returns the pause.
async fn wait(pause: Option<Arc<Pause>>) {
    if let Some(pause) = pause {
        pause.arrived.notify_one();
        pause.release.notified().await;
    }
}

pub async fn fan_out_pause_point(recipient_id: u32) {
    wait(take_for(&FAN_OUT_PAUSE, recipient_id)).await;
}

pub async fn put_pause_point(account_id: u32) {
    wait(take_for(&PUT_PAUSE, account_id)).await;
}

pub async fn task_consumer_pause_point() {
    let pause = TASK_CONSUMER_PAUSE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    wait(pause).await;
}

/// Whether the commit of `account_id` must fail now; clears the flag.
pub fn take_commit_fail(account_id: u32) -> bool {
    let mut slot = COMMIT_FAIL_ONCE.lock().unwrap_or_else(|e| e.into_inner());
    if *slot == Some(account_id) {
        *slot = None;
        true
    } else {
        false
    }
}
```

In `crates/dav/Cargo.toml`, `crates/groupware/Cargo.toml` and `crates/services/Cargo.toml`, replace `test_mode = []` under `[features]` with:

```toml
test_mode = ["common/test_mode"]
```

In `crates/services/src/task_manager/manager.rs`, inside the consumer loop (`:302-304`), make the loop start:

```rust
        loop {
            // Zero-access test hook (invitations spec 11): while it is held,
            // the consumer takes no task, so a test can read the queue.
            #[cfg(feature = "test_mode")]
            common::za::test_hooks::task_consumer_pause_point().await;

            // Index any queued tasks
            let mut sleep_for = inner.build_server().process_tasks(&mut ipc).await;
```

- [ ] **Step 6: Compile checks**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo check -p stalwart --no-default-features --features rocks 2>&1 | grep -E "^(warning|error)" -A3 | grep -E "crates/(groupware|dav)/|crates/services/src/task_manager/manager.rs|crates/common/src/(za/|auth/vault.rs)" || echo clean
cargo check -p services --features test_mode 2>&1 | grep -E "^error" -A5 || echo compiled
cargo tree -p stalwart --no-default-features --features rocks -e features -i common | grep test_mode || echo "no test_mode in the product"
```

Expected: `clean`, `compiled`, `no test_mode in the product`.

Fallback (from code reading): if `cargo check -p services --features test_mode` fails inside `common` for a reason in a file this task did not touch (upstream code that needs `enterprise` under `test_mode`), record the error and run `cargo check -p tests` instead, which enables both features as the suites do.

- [ ] **Step 7: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p common -- --check
cargo fmt -p services -- --check
git add crates/common/src/lib.rs crates/common/src/za/ crates/dav/Cargo.toml crates/groupware/Cargo.toml crates/services/Cargo.toml crates/services/src/task_manager/manager.rs
git commit -m "Zero-access: the UID creation lease and the test-mode hooks"   # body and trailers per Global Constraints
```

---

### Task 2: The fan-out module

**Files:**
- Modify: `crates/groupware/src/scheduling/itip.rs:79` (status constants after `ITIP_STATUS_INVALID_USER`)
- Modify: `crates/groupware/src/calendar/seal/event.rs` (new `unseal_event_pairs` after `unseal_event`, `:271-288`; one test in the tests module)
- Modify: `crates/groupware/src/calendar/seal/mod.rs:19-21` (re-export)
- Modify: `crates/groupware/src/calendar/mod.rs:7-13` (`pub mod fanout;`)
- Create: `crates/groupware/src/calendar/fanout.rs`
- Test: `crates/groupware/src/calendar/fanout.rs` (tests module), `seal/event.rs` (tests module)

**Interfaces:**
- Consumes:
  - Task 1: `common::za::test_hooks::fan_out_pause_point` (test builds only).
  - Plan 8: `seal_pending_item(ical, recipient_id, &[u8; 32], Option<&TokenPairs>, Option<&[u8; 16]>)`, `TokenPairs::{new, insert, get, only, len, is_empty}`, `TOKEN_LEN`, `EVENT_NOTIFICATION_ZA_PENDING`, `seal_error`; `seal/event.rs`'s private `open_root`, `is_sealed`, `unseal_tree`.
  - Upstream: `Email::new` (`scheduling/mod.rs:258-265`), `inbound::itip_method` (`inbound.rs:605`), `ical_size` (`scheduling/mod.rs:276`), `CalendarEventNotification::insert` (`calendar/storage.rs:285-310`), `Server::{account_id_from_email, try_account, access_token, account, has_available_quota, commit_batch, store}`, `AccountCache::{is_key_account, za_public_key, account_tenant_ids}`.
- Produces (Tasks 3, 5 and plan 10), in `groupware::calendar::fanout`:
  - `pub trait ZaFanOut` with `fn za_fan_out(&self, sender: &AccountInfo, keys: &SessionKeys, messages: Vec<ItipMessage<ICalendar>>, stored_pairs: Option<TokenPairs>, event: &ICalendar) -> impl Future<Output = trc::Result<ZaFanOutResult>> + Send`, implemented on `Server`
  - `pub struct ZaFanOutResult { pub pairs: Option<TokenPairs>, pub statuses: Vec<ZaScheduleStatus> }`
  - `pub struct ZaScheduleStatus { pub address: String, pub on_organizer: bool, pub status: &'static str }`
  - `pub fn za_set_schedule_status(ical: &mut ICalendar, statuses: &[ZaScheduleStatus])`
  - `pub struct ZaDeleteMessage { pub message: ItipMessage<ICalendar>, pub pairs: Option<TokenPairs>, pub event: ICalendar }` (Task 5 builds it)
  - `groupware::scheduling::itip::{ITIP_STATUS_DELIVERED, ITIP_STATUS_NOT_ATTEMPTED, ITIP_STATUS_DELIVERY_FAILED}`
  - `groupware::calendar::seal::unseal_event_pairs(event: &mut CalendarEvent, keys: &SessionKeys, account_id: u32) -> Result<Option<TokenPairs>, SealError>` (Task 5)

Background:
- The entry runs after every check of the request and immediately before the sender's commit (spec 5.4). It writes the items, each its own batch, and returns the pairs the sender's bundle keeps and one status per recipient. The caller writes the statuses into its plaintext tree, then seals and commits.
- Token maintenance (3.3), when the sender is the event's ORGANIZER (the tree's ORGANIZER is one of the sender's addresses):
  - The union is the stored pairs plus a fresh 16-byte token for every scheduled attendee without one. Scheduled attendees are the ATTENDEE addresses of the tree's scheduling components other than the sender's own, sanitized to lower case as upstream's `Email::new` does.
  - A REQUEST or CANCEL item to X carries `union.only(X)`, so a removed attendee's CANCEL carries its old pair.
  - The bundle keeps the union restricted to current attendees.
  - The builders put no token in any tree, so forming the union inside the fan-out, after they ran, is the same as forming it before.
- When the sender is an attendee, its copy holds its own pair. It hands out no token and keeps its stored pairs unchanged (every rewrite carries the pairs). A REPLY item carries `stored.get(sender address)` as the visible token.
- Routing per (message, recipient), spec 5.2:
  - **REQUEST or CANCEL:** attempted only when upstream's builder marked the message as the organizer's (`from_organizer`). The organizer builders set it; the attendee builder's delegate REQUEST (`attendee.rs:256-271`) clears it.
  - **REPLY:** attempted only to the event's ORGANIZER, and only when the replier's copy holds its token. Any other REPLY recipient (a delegator) is not attempted.
  - **Any other method** is not attempted. Upstream's builders produce none.
- Resolution, per address and cached within the call: `account_id_from_email(address, true)`.
  - The result must be another account than the sender's. An alias or sub-address of the sender's own account never receives an item (Review Focus 3).
  - It must be a key account whose account-cache entry holds a public key: the `za_is_key_account` predicate (`try_account` then `is_key_account`), read once together with `za_public_key`. A pending setup has none (Review Focus 5).
  - It must hold `Permission::CalendarSchedulingReceive`, the permission upstream's ingest checks (`crates/email/src/message/ingest.rs:367-370`); it is read from `access_token(id).build()`.
  - A lookup error is that recipient's delivery failure, traced with the sender's account id.
- The item write (5.3 steps 4-5):
  - The checks upstream's ingest makes: the message's plaintext size (`ical_size`, the length of its serialization, which is what ingest measures) must be under `itip_inbound_max_ical_size` (`ingest.rs:386` accepts `len < max`), and must fit the recipient's quota (`has_available_quota`; ingest treats any error as over quota).
  - Then `seal_pending_item`, a document id in the recipient's account, `FAN_OUT_PAUSE`, and `CalendarEventNotification::insert` through the index builder as its own batch. `insert` sets `created` and `modified`.
  - Any failure is `5.1` for that recipient and the next recipient proceeds.
- No DAV resource-cache invalidation follows the item. Upstream's ingest performs none either (`calendar/itip.rs:296-334` commits and returns). The recipient's resource cache applies the change log on every `fetch_dav_resources` (`crates/groupware/src/cache/mod.rs:117-129`), on every node, and the item's commit through the index builder writes that change-log entry.
- Statuses: `1.2` delivered, `5.3` not attempted, `5.1` delivery failed. They go on the recipient's ATTENDEE properties, or on the ORGANIZER property for a reply to the organizer. Every property naming the address gets the status, so an attendee listed twice gets it twice (Review Focus 1). When a recipient gets several messages, the worst status wins (5.1 over 5.3 over 1.2). An earlier fan-out's status is replaced; upstream's `3.7` stays with `itip_set_unreachable_status`. Upstream strips SCHEDULE-STATUS from every outgoing message (`itip_export_component`, `scheduling/itip.rs:240-258`), so the statuses never travel.
- `keys` proves the call runs in the sender's own session. The fan-out never needs a key of the recipient, only its public key.

- [ ] **Step 1: The constants and the failing tests**

In `crates/groupware/src/scheduling/itip.rs`, after `pub(crate) const ITIP_STATUS_INVALID_USER: &str = "3.7";` (`:79`):

```rust
/// Invitations spec 13: the fan-out wrote the recipient's pending item
/// (RFC 6638 "delivered"; upstream's iMIP task sets no status for local
/// delivery, so there is no upstream constant to reuse).
pub const ITIP_STATUS_DELIVERED: &str = "1.2";
/// Not attempted: the recipient is not an internal key account, or the
/// message may not go to it (RFC 6638 "no scheduling support for user").
pub const ITIP_STATUS_NOT_ATTEMPTED: &str = "5.3";
/// Delivery failed: the recipient's quota, the inbound size limit or a store
/// error (RFC 6638 "could not complete delivery").
pub const ITIP_STATUS_DELIVERY_FAILED: &str = "5.1";
```

Append inside `mod tests` in `crates/groupware/src/calendar/seal/event.rs`:

```rust
    #[test]
    fn unseal_with_pairs_returns_the_bundle_pairs_once() {
        let keys = keys();
        let original = event();
        let mut sealed = original.clone();
        seal_event(&mut sealed, &keys, 9, Some(&two_pairs())).unwrap();
        let mut opened = sealed.clone();
        let pairs = unseal_event_pairs(&mut opened, &keys, 9)
            .unwrap()
            .expect("pairs");
        assert_eq!(opened, original);
        assert_eq!(pairs.len(), 2);
        assert!(pairs.matches("a@example.com", &[1; TOKEN_LEN]));
        // A format 1 bundle and a plaintext event: no pairs; the plaintext
        // event is left as it is.
        let mut v1 = original.clone();
        seal_event(&mut v1, &keys, 9, None).unwrap();
        assert!(unseal_event_pairs(&mut v1, &keys, 9).unwrap().is_none());
        assert_eq!(v1, original);
        let mut plain = original.clone();
        assert!(unseal_event_pairs(&mut plain, &keys, 9).unwrap().is_none());
        assert_eq!(plain, original);
        // Keys of another session do not open it.
        assert_eq!(
            unseal_event_pairs(&mut sealed, &SessionKeys::new(9, 1, Secret::random()), 9).err(),
            Some(SealError::Aead)
        );
    }
```

Create `crates/groupware/src/calendar/fanout.rs` with the SPDX header and only this tests module for now. Add `pub mod fanout;` to `crates/groupware/src/calendar/mod.rs` after `pub mod expand;`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduling::ItipSummary;
    use calcard::{Entry, Parser};

    /// An event as its organizer (org@example.com) stores it: bob listed
    /// twice in two cases, carol with a client-set 3.7, and the organizer
    /// listed as an attendee.
    const EVENT: &str = concat!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\n",
        "UID:fo-unit\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\n",
        "SEQUENCE:0\r\nORGANIZER:mailto:org@example.com\r\n",
        "ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:Bob@Example.com\r\n",
        "ATTENDEE;CN=Bob again:mailto:bob@example.com\r\n",
        "ATTENDEE;SCHEDULE-STATUS=3.7:mailto:carol@example.com\r\n",
        "ATTENDEE:mailto:org@example.com\r\n",
        "END:VEVENT\r\nEND:VCALENDAR\r\n"
    );

    fn parse(text: &str) -> ICalendar {
        match Parser::new(text).entry() {
            Entry::ICalendar(ical) => ical,
            other => panic!("{other:?}"),
        }
    }

    fn organizer_addresses() -> Vec<String> {
        vec!["org@example.com".to_string()]
    }

    fn message(from: &str, from_organizer: bool, method: &str) -> ItipMessage<ICalendar> {
        ItipMessage {
            from: from.to_string(),
            from_organizer,
            to: vec![],
            summary: ItipSummary::Invite(vec![]),
            message: parse(&format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:{method}\r\nBEGIN:VEVENT\r\nUID:fo-unit\r\nDTSTAMP:20240101T000000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
            )),
        }
    }

    fn route_of(
        message: &ItipMessage<ICalendar>,
        recipient: &str,
        organizer: Option<&Email>,
        union: &TokenPairs,
    ) -> Route {
        route(itip_method(&message.message).ok(), message, recipient, organizer, union)
    }

    /// (value, SCHEDULE-STATUS values) of every `property` in the tree.
    fn statuses_of(ical: &ICalendar, property: ICalendarProperty) -> Vec<(String, Vec<String>)> {
        ical.components
            .iter()
            .flat_map(|comp| comp.entries.iter())
            .filter(|entry| entry.name == property)
            .map(|entry| {
                (
                    entry.values[0].as_text().unwrap().to_string(),
                    entry
                        .params
                        .iter()
                        .filter(|param| param.name == ICalendarParameterName::ScheduleStatus)
                        .map(|param| param.value.as_text().unwrap().to_string())
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn the_union_adds_one_token_per_new_attendee_and_keeps_stored_ones() {
        let event = parse(EVENT);
        let mut stored = TokenPairs::new();
        stored.insert("carol@example.com", [3; TOKEN_LEN]);
        stored.insert("gone@example.com", [9; TOKEN_LEN]);
        let union = token_union(Some(&stored), &event, &organizer_addresses());
        assert_eq!(union.len(), 3, "carol and gone kept, bob added once");
        assert!(union.matches("carol@example.com", &[3; TOKEN_LEN]));
        assert!(union.matches("gone@example.com", &[9; TOKEN_LEN]));
        let bob = *union.get("bob@example.com").expect("bob's fresh token");
        assert!(union.get("org@example.com").is_none(), "no token for the sender");
        let current = current_attendee_pairs(&union, &event, &organizer_addresses());
        assert_eq!(current.len(), 2, "the removed attendee's pair is not kept");
        assert!(current.get("gone@example.com").is_none());
        assert!(current.matches("bob@example.com", &bob));
        // The next union keeps bob's token; a union with no stored pairs
        // draws a different one.
        let again = token_union(Some(&current), &event, &organizer_addresses());
        assert!(again.matches("bob@example.com", &bob));
        let fresh = token_union(None, &event, &organizer_addresses());
        assert!(!fresh.matches("bob@example.com", &bob));
    }

    #[test]
    fn requests_and_cancels_go_out_only_from_the_organizer() {
        let event = parse(EVENT);
        let organizer = event_organizer(&event, &organizer_addresses());
        assert!(organizer.as_ref().is_some_and(|organizer| organizer.is_local));
        let mut union = TokenPairs::new();
        union.insert("bob@example.com", [1; TOKEN_LEN]);
        for method in ["REQUEST", "CANCEL"] {
            let ours = message("org@example.com", true, method);
            match route_of(&ours, "bob@example.com", organizer.as_ref(), &union) {
                Route::Deliver {
                    on_organizer: false,
                    pair: Some(pair),
                    reply_token: None,
                } => {
                    assert_eq!(pair.len(), 1);
                    assert!(pair.matches("bob@example.com", &[1; TOKEN_LEN]));
                }
                _ => panic!("{method} from the organizer goes out with bob's pair"),
            }
            let delegate = message("bob@example.com", false, method);
            assert!(
                matches!(
                    route_of(&delegate, "dave@example.com", organizer.as_ref(), &union),
                    Route::NotAttempted { on_organizer: false }
                ),
                "{method} from an attendee is not attempted"
            );
        }
        // A removed attendee with no stored pair: delivered without a pair.
        let cancel = message("org@example.com", true, "CANCEL");
        assert!(matches!(
            route_of(&cancel, "dave@example.com", organizer.as_ref(), &union),
            Route::Deliver { pair: None, .. }
        ));
    }

    #[test]
    fn a_reply_goes_to_the_organizer_only_and_needs_the_repliers_token() {
        // Bob's copy: the organizer is another account.
        let event = parse(EVENT);
        let organizer = event_organizer(&event, &["bob@example.com".to_string()]);
        assert!(organizer.as_ref().is_some_and(|organizer| !organizer.is_local));
        let mut own = TokenPairs::new();
        own.insert("bob@example.com", [5; TOKEN_LEN]);
        let reply = message("bob@example.com", false, "REPLY");
        match route_of(&reply, "ORG@example.com", organizer.as_ref(), &own) {
            Route::Deliver {
                on_organizer: true,
                pair: None,
                reply_token: Some(token),
            } => assert_eq!(*token, [5; TOKEN_LEN]),
            _ => panic!("the reply to the organizer carries bob's token"),
        }
        assert!(
            matches!(
                route_of(&reply, "carol@example.com", organizer.as_ref(), &own),
                Route::NotAttempted { on_organizer: false }
            ),
            "a delegator is not attempted"
        );
        assert!(
            matches!(
                route_of(&reply, "org@example.com", organizer.as_ref(), &TokenPairs::new()),
                Route::NotAttempted { on_organizer: true }
            ),
            "no token: not attempted, the status on the ORGANIZER"
        );
        let refresh = message("bob@example.com", false, "REFRESH");
        assert!(matches!(
            route_of(&refresh, "org@example.com", organizer.as_ref(), &own),
            Route::NotAttempted { on_organizer: true }
        ));
    }

    #[test]
    fn statuses_land_on_every_matching_property_and_replace_earlier_ones() {
        let mut event = parse(EVENT);
        za_set_schedule_status(
            &mut event,
            &[
                ZaScheduleStatus {
                    address: "bob@example.com".into(),
                    on_organizer: false,
                    status: ITIP_STATUS_DELIVERED,
                },
                ZaScheduleStatus {
                    address: "carol@example.com".into(),
                    on_organizer: false,
                    status: ITIP_STATUS_NOT_ATTEMPTED,
                },
                ZaScheduleStatus {
                    address: "org@example.com".into(),
                    on_organizer: true,
                    status: ITIP_STATUS_DELIVERY_FAILED,
                },
            ],
        );
        let s = |values: &[&str]| values.iter().map(|v| v.to_string()).collect::<Vec<_>>();
        assert_eq!(
            statuses_of(&event, ICalendarProperty::Attendee),
            vec![
                ("mailto:Bob@Example.com".to_string(), s(&["1.2"])),
                ("mailto:bob@example.com".to_string(), s(&["1.2"])),
                ("mailto:carol@example.com".to_string(), s(&["3.7", "5.3"])),
                ("mailto:org@example.com".to_string(), s(&[])),
            ]
        );
        assert_eq!(
            statuses_of(&event, ICalendarProperty::Organizer),
            vec![("mailto:org@example.com".to_string(), s(&["5.1"]))]
        );
        za_set_schedule_status(
            &mut event,
            &[ZaScheduleStatus {
                address: "CAROL@example.com".into(),
                on_organizer: false,
                status: ITIP_STATUS_DELIVERED,
            }],
        );
        assert_eq!(
            statuses_of(&event, ICalendarProperty::Attendee)[2].1,
            s(&["3.7", "1.2"]),
            "a fan-out status is replaced, upstream's 3.7 is kept"
        );
    }

    #[test]
    fn the_worst_status_of_a_recipient_wins() {
        let mut statuses = Vec::new();
        record_status(&mut statuses, "bob@example.com", false, ITIP_STATUS_DELIVERED);
        record_status(&mut statuses, "BOB@example.com", false, ITIP_STATUS_DELIVERY_FAILED);
        record_status(&mut statuses, "bob@example.com", false, ITIP_STATUS_DELIVERED);
        record_status(&mut statuses, "bob@example.com", true, ITIP_STATUS_NOT_ATTEMPTED);
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].status, ITIP_STATUS_DELIVERY_FAILED);
        assert!(statuses[1].on_organizer);
        assert_eq!(statuses[1].status, ITIP_STATUS_NOT_ATTEMPTED);
    }
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p groupware 2>&1 | tail -10`
Expected: compile errors, `cannot find function 'unseal_event_pairs'`, `cannot find function 'token_union'` and the other unresolved names.

- [ ] **Step 3: `unseal_event_pairs`**

In `crates/groupware/src/calendar/seal/event.rs`, after `unseal_event` (`:271-288`):

```rust
/// `unseal_event`, also returning the token pairs of the bundle (invitations
/// spec 3.3, 5.5): a key account's DELETE builds its CANCEL from the
/// unsealed tree and hands the stored pairs to the fan-out, with one open of
/// the envelope. `None` for a plaintext event (left unchanged), an event
/// without a bundle, and a format 1 bundle.
///
/// On `Err` the event is partly modified and must be discarded.
pub fn unseal_event_pairs(
    event: &mut CalendarEvent,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<Option<TokenPairs>, SealError> {
    if !is_sealed(event) {
        return Ok(None);
    }
    let (dek, uid, extra) = open_root(event, keys, account_id)?;
    let mut pairs = None;
    if let Some(extra) = extra {
        event.display_name = extra.display_name;
        event.dead_properties = extra.dead_properties;
        pairs = extra.pairs;
    }
    unseal_tree(&mut event.data.event, &dek, account_id, &uid)?;
    Ok(pairs)
}
```

In `crates/groupware/src/calendar/seal/mod.rs`, replace the `event` re-export with:

```rust
pub use event::{
    archived_event_is_sealed, event_token_pairs, seal_event, unseal_event, unseal_event_archive,
    unseal_event_pairs,
};
```

- [ ] **Step 4: Implement `fanout.rs`**

Prepend to `fanout.rs`, after the SPDX header:

```rust
//! Invitations spec 5: the fan-out. Under `ItipSendStatus::SendSealed` a key
//! account's own request delivers the iTIP messages upstream's builders made
//! as sealed pending items in the scheduling inboxes of internal key
//! recipients, instead of queueing them as mail (invariant 12). It runs
//! after every check of the request and immediately before the sender's
//! commit (5.4). Each item is its own batch in the recipient's account, and
//! nothing is rolled back if the sender's commit then fails. The
//! address-to-account mapping exists only inside one call (5.2,
//! invariant 13).
//!
//! A recipient is internal when its address resolves to another local
//! account that is a key account, whose account cache entry holds a public
//! key, and that holds `Permission::CalendarSchedulingReceive`. A REQUEST
//! or CANCEL is attempted only when upstream marked it as the organizer's
//! (`from_organizer`); a REPLY only to the event's ORGANIZER, and only when
//! the replier's copy holds its token. Every other recipient is not
//! attempted.

use crate::{
    calendar::{
        CalendarEventNotification, ChangedBy, EVENT_NOTIFICATION_ZA_PENDING,
        seal::{TOKEN_LEN, TokenPairs, seal_error, seal_pending_item},
    },
    scheduling::{
        Email, ItipMessage, ical_size,
        inbound::itip_method,
        itip::{
            ITIP_STATUS_DELIVERED, ITIP_STATUS_DELIVERY_FAILED, ITIP_STATUS_INVALID_USER,
            ITIP_STATUS_NOT_ATTEMPTED,
        },
    },
};
use ahash::AHashMap;
use calcard::icalendar::{
    ICalendar, ICalendarMethod, ICalendarParameter, ICalendarParameterName, ICalendarProperty,
};
use common::{Server, auth::AccountInfo};
use registry::schema::enums::Permission;
use store::{rand, write::BatchBuilder};
use trc::AddContext;
use types::collection::Collection;
use vault::session::SessionKeys;
use zeroize::Zeroizing;

/// One SCHEDULE-STATUS the fan-out decided (spec 5.2, 5.3, 13). No `Debug`:
/// it holds an address.
#[derive(Clone, PartialEq, Eq)]
pub struct ZaScheduleStatus {
    /// The recipient's address as the message names it.
    pub address: String,
    /// On the ORGANIZER property (a reply to the organizer) instead of the
    /// recipient's ATTENDEE properties.
    pub on_organizer: bool,
    /// `ITIP_STATUS_DELIVERED`, `ITIP_STATUS_NOT_ATTEMPTED` or
    /// `ITIP_STATUS_DELIVERY_FAILED`.
    pub status: &'static str,
}

/// What `za_fan_out` returns to the sender's write path.
pub struct ZaFanOutResult {
    /// The pairs the sender's event bundle keeps (`seal_event(.., pairs)`):
    /// an organizer's union restricted to current attendees, or an
    /// attendee's stored pairs unchanged. `None` when there are none.
    pub pairs: Option<TokenPairs>,
    /// For `za_set_schedule_status` on the sender's plaintext tree, before
    /// it is sealed.
    pub statuses: Vec<ZaScheduleStatus>,
}

/// A CANCEL (organizer) or declined REPLY (attendee) that a key account's
/// DELETE built from the unsealed stored event (spec 5.5), with what the
/// fan-out needs from that event.
pub struct ZaDeleteMessage {
    pub message: ItipMessage<ICalendar>,
    /// The stored event's token pairs.
    pub pairs: Option<TokenPairs>,
    /// The unsealed stored tree.
    pub event: ICalendar,
}

pub trait ZaFanOut: Sync + Send {
    /// Delivers `messages` (upstream's builders' output for `event`, the
    /// sender's plaintext tree) to internal key recipients as pending items.
    /// `sender` is the account whose own request this is and `keys` its
    /// session keys; `stored_pairs` the token pairs of the sender's stored
    /// event (`event_token_pairs`), `None` on a create. A lookup or write
    /// failure for one recipient becomes that recipient's delivery-failed
    /// status (spec 10); the call fails only when `keys` are not the
    /// sender's. Plan 10's drain calls it for the delegation fan-out
    /// (spec 6.4).
    fn za_fan_out(
        &self,
        sender: &AccountInfo,
        keys: &SessionKeys,
        messages: Vec<ItipMessage<ICalendar>>,
        stored_pairs: Option<TokenPairs>,
        event: &ICalendar,
    ) -> impl Future<Output = trc::Result<ZaFanOutResult>> + Send;
}

impl ZaFanOut for Server {
    async fn za_fan_out(
        &self,
        sender: &AccountInfo,
        keys: &SessionKeys,
        messages: Vec<ItipMessage<ICalendar>>,
        stored_pairs: Option<TokenPairs>,
        event: &ICalendar,
    ) -> trc::Result<ZaFanOutResult> {
        let sender_id = sender.account_id();
        if keys.account_id != sender_id {
            return Err(trc::SecurityEvent::Unauthorized
                .into_err()
                .details("zero-access: fan-out without the sender's session keys")
                .account_id(sender_id)
                .caused_by(trc::location!()));
        }
        let organizer = event_organizer(event, sender.addresses());
        let is_organizer = organizer.as_ref().is_some_and(|organizer| organizer.is_local);
        // Spec 3.3, "Maintenance": only the organizer hands out tokens; an
        // attendee's copy holds its own pair.
        let union = if is_organizer {
            token_union(stored_pairs.as_ref(), event, sender.addresses())
        } else {
            stored_pairs.clone().unwrap_or_default()
        };

        let mut recipients: AHashMap<String, Recipient> = AHashMap::new();
        let mut statuses = Vec::new();
        for message in &messages {
            let method = itip_method(&message.message).ok();
            for address in &message.to {
                let (on_organizer, status) =
                    match route(method, message, address, organizer.as_ref(), &union) {
                        Route::NotAttempted { on_organizer } => {
                            (on_organizer, ITIP_STATUS_NOT_ATTEMPTED)
                        }
                        Route::Deliver {
                            on_organizer,
                            pair,
                            reply_token,
                        } => {
                            let key = address.to_ascii_lowercase();
                            let recipient = match recipients.get(&key) {
                                Some(recipient) => *recipient,
                                None => {
                                    let recipient =
                                        resolve_recipient(self, sender_id, address).await;
                                    recipients.insert(key, recipient);
                                    recipient
                                }
                            };
                            let status = match recipient {
                                Recipient::Internal {
                                    account_id,
                                    public_key,
                                } => {
                                    write_item(
                                        self,
                                        sender_id,
                                        account_id,
                                        &public_key,
                                        &message.message,
                                        pair.as_ref(),
                                        reply_token.as_deref(),
                                    )
                                    .await
                                }
                                Recipient::NotInternal => ITIP_STATUS_NOT_ATTEMPTED,
                                Recipient::Failed => ITIP_STATUS_DELIVERY_FAILED,
                            };
                            (on_organizer, status)
                        }
                    };
                record_status(&mut statuses, address, on_organizer, status);
            }
        }

        // Spec 3.3: the organizer's bundle keeps the pairs of current
        // attendees only.
        let pairs = if is_organizer {
            Some(current_attendee_pairs(&union, event, sender.addresses()))
        } else {
            stored_pairs
        };
        Ok(ZaFanOutResult {
            pairs: pairs.filter(|pairs| !pairs.is_empty()),
            statuses,
        })
    }
}

/// Writes each status as the SCHEDULE-STATUS of the matching ORGANIZER or
/// ATTENDEE properties of the sender's plaintext tree (RFC 6638 3.2.9),
/// before it is sealed: sealed with the property, shown to the sender's own
/// client. Replaces an earlier fan-out's status; upstream's `3.7` is left
/// to `itip_set_unreachable_status`.
pub fn za_set_schedule_status(ical: &mut ICalendar, statuses: &[ZaScheduleStatus]) {
    if statuses.is_empty() {
        return;
    }
    for comp in ical.components.iter_mut() {
        if !comp.component_type.is_scheduling_object() {
            continue;
        }
        for entry in comp.entries.iter_mut() {
            let on_organizer = match entry.name {
                ICalendarProperty::Organizer => true,
                ICalendarProperty::Attendee => false,
                _ => continue,
            };
            let Some(address) = entry
                .values
                .first()
                .and_then(|value| value.as_text())
                .and_then(|value| Email::new(value, &[]))
            else {
                continue;
            };
            let Some(status) = statuses.iter().find(|status| {
                status.on_organizer == on_organizer
                    && status.address.eq_ignore_ascii_case(&address.email)
            }) else {
                continue;
            };
            entry.params.retain(|param| {
                param.name != ICalendarParameterName::ScheduleStatus
                    || param.value.as_text() == Some(ITIP_STATUS_INVALID_USER)
            });
            entry
                .params
                .push(ICalendarParameter::schedule_status(status.status.to_string()));
        }
    }
}

#[derive(Clone, Copy)]
enum Recipient {
    Internal { account_id: u32, public_key: [u8; 32] },
    NotInternal,
    Failed,
}

enum Route {
    NotAttempted {
        on_organizer: bool,
    },
    Deliver {
        on_organizer: bool,
        pair: Option<TokenPairs>,
        reply_token: Option<Zeroizing<[u8; TOKEN_LEN]>>,
    },
}

/// Spec 5.2: whether `recipient` of `message` is attempted, and with which
/// carrier (spec 5.3 step 1).
fn route(
    method: Option<&ICalendarMethod>,
    message: &ItipMessage<ICalendar>,
    recipient: &str,
    organizer: Option<&Email>,
    union: &TokenPairs,
) -> Route {
    let to_organizer =
        organizer.is_some_and(|organizer| organizer.email.eq_ignore_ascii_case(recipient));
    match method {
        // An attendee-originated REQUEST (a delegate invitation) is not
        // attempted: delegates receive their token from the organizer's
        // drain (spec 6.4).
        Some(ICalendarMethod::Request | ICalendarMethod::Cancel) if message.from_organizer => {
            Route::Deliver {
                on_organizer: false,
                pair: union.only(recipient),
                reply_token: None,
            }
        }
        // A REPLY goes to the ORGANIZER only: a delegator's bundle holds no
        // pair to check it against (spec 5.5). Without the replier's own
        // token the organizer's drain would drop it as a forgery (3.3).
        Some(ICalendarMethod::Reply) if to_organizer => match union.get(&message.from) {
            Some(token) => Route::Deliver {
                on_organizer: true,
                pair: None,
                reply_token: Some(Zeroizing::new(*token)),
            },
            None => Route::NotAttempted { on_organizer: true },
        },
        _ => Route::NotAttempted {
            on_organizer: to_organizer,
        },
    }
}

/// The event's ORGANIZER: the first one in a scheduling component,
/// `is_local` when it is one of the sender's addresses.
fn event_organizer(event: &ICalendar, sender_addresses: &[String]) -> Option<Email> {
    event
        .components
        .iter()
        .filter(|comp| comp.component_type.is_scheduling_object())
        .find_map(|comp| {
            comp.entries
                .iter()
                .find(|entry| entry.name == ICalendarProperty::Organizer)
                .and_then(|entry| entry.values.first())
                .and_then(|value| value.as_text())
                .and_then(|value| Email::new(value, sender_addresses))
        })
}

/// Scheduled attendees: the ATTENDEE addresses of the scheduling components
/// other than the sender's own, sanitized and deduplicated.
fn scheduled_attendees(event: &ICalendar, sender_addresses: &[String]) -> Vec<String> {
    let mut attendees: Vec<String> = Vec::new();
    for comp in event
        .components
        .iter()
        .filter(|comp| comp.component_type.is_scheduling_object())
    {
        for entry in comp
            .entries
            .iter()
            .filter(|entry| entry.name == ICalendarProperty::Attendee)
        {
            if let Some(email) = entry
                .values
                .first()
                .and_then(|value| value.as_text())
                .and_then(|value| Email::new(value, sender_addresses))
                && !email.is_local
                && !attendees.contains(&email.email)
            {
                attendees.push(email.email);
            }
        }
    }
    attendees
}

/// Spec 3.3, "Maintenance": the stored pairs plus a fresh token for every
/// scheduled attendee without one.
fn token_union(
    stored: Option<&TokenPairs>,
    event: &ICalendar,
    sender_addresses: &[String],
) -> TokenPairs {
    let mut union = stored.cloned().unwrap_or_default();
    for address in scheduled_attendees(event, sender_addresses) {
        if union.get(&address).is_none() {
            let token = Zeroizing::new(rand::random::<[u8; TOKEN_LEN]>());
            union.insert(&address, *token);
        }
    }
    union
}

/// The union restricted to the event's current attendees.
fn current_attendee_pairs(
    union: &TokenPairs,
    event: &ICalendar,
    sender_addresses: &[String],
) -> TokenPairs {
    let mut pairs = TokenPairs::new();
    for address in scheduled_attendees(event, sender_addresses) {
        if let Some(token) = union.get(&address) {
            pairs.insert(&address, *token);
        }
    }
    pairs
}

fn status_rank(status: &str) -> u8 {
    match status {
        ITIP_STATUS_DELIVERY_FAILED => 2,
        ITIP_STATUS_NOT_ATTEMPTED => 1,
        _ => 0,
    }
}

/// One status per (address, property); the worst outcome wins when a
/// recipient gets several messages.
fn record_status(
    statuses: &mut Vec<ZaScheduleStatus>,
    address: &str,
    on_organizer: bool,
    status: &'static str,
) {
    match statuses.iter_mut().find(|existing| {
        existing.on_organizer == on_organizer && existing.address.eq_ignore_ascii_case(address)
    }) {
        Some(existing) => {
            if status_rank(status) > status_rank(existing.status) {
                existing.status = status;
            }
        }
        None => statuses.push(ZaScheduleStatus {
            address: address.to_string(),
            on_organizer,
            status,
        }),
    }
}

/// Spec 5.2: whether `address` is an internal key account. A lookup error
/// is that recipient's delivery failure, traced without the address.
async fn resolve_recipient(server: &Server, sender_id: u32, address: &str) -> Recipient {
    match resolve_inner(server, sender_id, address).await {
        Ok(recipient) => recipient,
        Err(err) => {
            trc::error!(
                err.account_id(sender_id)
                    .details("zero-access: fan-out recipient lookup failed")
                    .caused_by(trc::location!())
            );
            Recipient::Failed
        }
    }
}

async fn resolve_inner(server: &Server, sender_id: u32, address: &str) -> trc::Result<Recipient> {
    let Some(account_id) = server.account_id_from_email(address, true).await? else {
        return Ok(Recipient::NotInternal);
    };
    // Never the sender's own inbox (an alias or sub-address of its own).
    if account_id == sender_id {
        return Ok(Recipient::NotInternal);
    }
    // `za_is_key_account`'s lookup, which also yields the public key (spec
    // 3.2): a pending setup has none.
    let Some(public_key) = server
        .try_account(account_id)
        .await?
        .filter(|account| account.is_key_account())
        .and_then(|account| account.za_public_key)
    else {
        return Ok(Recipient::NotInternal);
    };
    if !server
        .access_token(account_id)
        .await?
        .build()
        .has_permission(Permission::CalendarSchedulingReceive)
    {
        return Ok(Recipient::NotInternal);
    }
    Ok(Recipient::Internal {
        account_id,
        public_key,
    })
}

/// Spec 5.3: one pending item for one recipient. A failure is `5.1`, traced
/// with the recipient's account id and no content.
async fn write_item(
    server: &Server,
    sender_id: u32,
    recipient_id: u32,
    public_key: &[u8; 32],
    message: &ICalendar,
    pair: Option<&TokenPairs>,
    reply_token: Option<&[u8; TOKEN_LEN]>,
) -> &'static str {
    match write_item_inner(server, sender_id, recipient_id, public_key, message, pair, reply_token)
        .await
    {
        Ok(true) => ITIP_STATUS_DELIVERED,
        Ok(false) => ITIP_STATUS_DELIVERY_FAILED,
        Err(err) => {
            trc::error!(
                err.account_id(recipient_id)
                    .details("zero-access: fan-out item not written")
                    .caused_by(trc::location!())
            );
            ITIP_STATUS_DELIVERY_FAILED
        }
    }
}

/// `Ok(false)` when the inbound size limit or the recipient's quota refuses
/// the item (step 5); `Ok(true)` once it is committed (steps 1-4).
async fn write_item_inner(
    server: &Server,
    sender_id: u32,
    recipient_id: u32,
    public_key: &[u8; 32],
    message: &ICalendar,
    pair: Option<&TokenPairs>,
    reply_token: Option<&[u8; TOKEN_LEN]>,
) -> trc::Result<bool> {
    // Step 5: the two checks upstream's ingest makes.
    let size = ical_size(message);
    if size >= server.core.groupware.itip_inbound_max_ical_size {
        return Ok(false);
    }
    let account = server
        .account(recipient_id)
        .await
        .caused_by(trc::location!())?;
    if server
        .has_available_quota(account.as_ref(), size as u64)
        .await
        .is_err()
    {
        return Ok(false);
    }
    // Steps 1-3 (plan 8): the tree sealed to the recipient's public key.
    let mut tree = message.clone();
    seal_pending_item(&mut tree, recipient_id, public_key, pair, reply_token)
        .map_err(|err| seal_error(err, recipient_id, u32::MAX))?;
    // Step 4: the item, its own batch in the recipient's account.
    let document_id = server
        .store()
        .assign_document_ids(recipient_id, Collection::CalendarEventNotification, 1)
        .await
        .caused_by(trc::location!())?;
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::fan_out_pause_point(recipient_id).await;
    let mut batch = BatchBuilder::new();
    CalendarEventNotification {
        event: tree,
        event_id: None,
        changed_by: ChangedBy::PrincipalId(sender_id),
        flags: EVENT_NOTIFICATION_ZA_PENDING,
        size: size as u32,
        ..Default::default()
    }
    .insert(account.account_tenant_ids(), recipient_id, document_id, &mut batch)
    .caused_by(trc::location!())?;
    server
        .commit_batch(batch)
        .await
        .caused_by(trc::location!())?;
    Ok(true)
}
```

- [ ] **Step 5: Run to see them pass**

Run: `cargo test -p groupware 2>&1 | tail -5`
Expected: PASS, 76 tests (plan 8's 70, one `unseal_event_pairs` test and five fan-out tests).

- [ ] **Step 6: Compile check**

Run the per-task compile check (Global Constraints). Expected: `clean`. `za_fan_out` has no caller yet; the trait is public, so no dead-code warning.

- [ ] **Step 7: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p groupware -- --check
git add crates/groupware/src/scheduling/itip.rs crates/groupware/src/calendar/seal/event.rs crates/groupware/src/calendar/seal/mod.rs crates/groupware/src/calendar/mod.rs crates/groupware/src/calendar/fanout.rs
git commit -m "Fan-out: sealed pending items for internal key recipients"
```

---

### Task 3: `SendSealed` and the PUT paths

**Files:**
- Modify: `crates/groupware/src/calendar/itip.rs:59-68` (`ItipSendStatus`), `:1064-1122` (its impl)
- Modify: `crates/dav/src/calendar/mod.rs:17-26` (imports), after `:126` (three helpers)
- Modify: `crates/dav/src/calendar/update.rs:7` (super import), `:30-40` (groupware import), after `:88` (`PUT_PAUSE`), `:239-339` (update branch: scheduling and seal), `:405-525` (create branch: UID check, scheduling, seal, batch, commit)
- Modify: `tests/src/utils/za.rs:7-32` (imports), new helpers after `wait_for_delivery`
- Create: `tests/src/za/fanout.rs`
- Modify: `tests/src/za/mod.rs` (`pub mod fanout;`, the call after `dav_seal::test_legacy`)
- Modify: `tests/src/za/tracing.rs:717-730`, `tests/src/za/gating.rs:482-497`, `:553-562`, `tests/src/za/pairs.rs:28-37`, `:134`, `:165`
- Modify: `tests/src/webdav/za_variants.rs:9-21` (imports), `:351-415` (`scheduling`)

**Interfaces:**
- Consumes:
  - Task 1: `ZaUidLease::acquire`, `ZaUidLeaseGuard::{new, lease, release, abandon}`, `ZaUidLease::assert_in`, `test_hooks::{put_pause_point, take_commit_fail, set_task_consumer_pause, Pause}`, `lease_key_range`.
  - Task 2: `ZaFanOut::za_fan_out`, `za_set_schedule_status`, the status constants.
  - Plan 8: `event_token_pairs`, `seal_event(.., pairs)`, `open_pending_item`, `PendingCarriers`, `Server::za_private_key`, `za_unlock`.
- Produces:
  - `ItipSendStatus::{SendSealed, builds_messages, is_send_sealed}` (`KeyAccount` removed).
  - `dav::calendar::{za_take_uid_lease, za_assert_is_unique_uid, za_commit_create}` (crate-private; Task 6 tests them through PUT).
  - Test helpers in `tests/src/utils/za.rs`, used by Tasks 4-7 and plan 10:
    - `inbox_items(test, account_id) -> Vec<(u32, CalendarEventNotification)>`
    - `inbox_ids(test, account_id) -> Vec<u32>`
    - `new_inbox_items(test, account_id, before: &[u32]) -> Vec<(u32, CalendarEventNotification)>`
    - `envelope_wrap(&ICalendar) -> Option<u8>`
    - `open_pending_item_as(test, recipient, password, document_id) -> (ICalendar, PendingCarriers)`
    - `item_method`, `item_sequence`, `item_attendees`, `item_partstat`
    - `schedule_statuses(body, property, address) -> Vec<String>`
    - `HookParked<T>` (`start`, `finish`), `hold_task_consumer(test) -> ConsumerHeld`, `itip_tasks_for(test, account_id) -> usize`, `za_uid_leases(test) -> usize`
  - `tests/src/za/fanout.rs`: `Party`, `invite`, `stored_pairs`, `attendee_statuses`, `pair_token`, the constants `CONTENT_TYPE`, `NO_REPLY`, `KEY1_CAL`, `START_A`, `START_B`, `MKCALENDAR` (Tasks 4-7 reuse them).

Background:
- Spec 5.1. `ItipSendStatus::resolve` keeps upstream's `Send` preconditions in upstream's order (enabled, a calendar address, `CalendarSchedulingSend`, an event not in the past), then answers `SendSealed` for a key account and `Send` otherwise. A key account with scheduling disabled is `SchedulingDisabled`, and so on.
- `is_send()` stays `Send` only. JMAP (`crates/jmap/src/calendar_event/set.rs:431-441`, refused for key accounts anyway) therefore never builds or queues for `SendSealed`, and its `reason()` is `None`.
- The `!= ItipSendStatus::KeyAccount` arms in both PUT branches go back to upstream's `else if let Some(reason) = itip_status.reason()`. A key account's past event now traces "lies in the past" like a plain account's: `tracing.rs` counts four such traces instead of two.
- Both branches run upstream's builders for `builds_messages()`, set the schedule tag and run `itip_set_unreachable_status` exactly as for `Send`; `itip_outbound_max_recipients` fails the PUT as for `Send`. Then `SendSealed` keeps the messages for the fan-out and never builds `ItipMessages` (invariant 12).
- The fan-out runs inside the existing `if let Some(keys) = &za_keys` seal block, after the quota check and immediately before `seal_event` and the commit (5.4). The update branch passes the stored pairs; the create branch passes `None`. An attendee's PUT that changes its participation yields upstream's REPLY through `itip_update`'s attendee path; the fan-out takes the attendee's token from the stored pairs.
- The create branch, key accounts only (5.1, 6.3):
  - **Lease first.** After `validate_headers`, it takes the lease and checks the UID against current archives (`za_take_uid_lease`), replacing `assert_is_unique_uid`. A busy lease answers 503; a hit answers `NoUidConflict`.
  - **Fenced insert.** The insert batch carries `lease.assert_in` ahead of the insert, and `za_commit_create` commits, then releases.
  - **Lost fence.** If the lease was taken over while the request ran, the commit loses its assertion. The request then takes the lease again and re-checks current archives: `NoUidConflict` on a hit, otherwise 503 so the client resends (a resend delivers fresh tokens with an equal SEQUENCE, which spec 5.4 accepts).
  - **Early returns.** `ZaUidLeaseGuard` releases the lease on every return between the lease and the commit.
- Non-key accounts take no lease and keep `assert_is_unique_uid` (invariant 9). The update branch takes no lease; the archive assertion protects it.
- Existing tests that `SendSealed` changes:
  - `gating::test_scheduling`: key1's invitation to plain now carries a schedule tag. Plain is not internal, so nothing is delivered and no mail is sent, exactly as the test already asserts.
  - `tracing::test`: four past-event traces.
  - `pairs::test`: its "events without pairs" cases used bodies with attendees, which the fan-out now gives tokens. They become bodies without attendees, which build no message, as spec 3.3 means by a rewrite without pairs.
  - The key-mode CalDAV variant: john's PUT carries a schedule tag and delivers a pending item to jane.
- None of upstream's `cal_scheduling` assertions can run unchanged in key mode before the drain: each reads the attendee's calendar copy or her inbox after mail ingest. The variant keeps its own assertions, adds the store-level delivery, and asserts the schedule tag upstream asserts on the organizer's PUT. The drained-copy half is plan 10's.

- [ ] **Step 1: Test helpers**

In `tests/src/utils/za.rs`, replace the import block (`:7-32`) with:

```rust
use crate::utils::{account::Account, http::HttpRequest, server::TestServer};
use base64::{Engine, engine::general_purpose::STANDARD};
use calcard::{
    common::{IanaString, timezone::Tz},
    icalendar::{
        ICalendar, ICalendarComponentType, ICalendarEntry, ICalendarMethod,
        ICalendarParameterName, ICalendarParameterValue, ICalendarProperty, ICalendarValue,
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
```

After `wait_for_delivery`, add:

```rust
/// The account's scheduling inbox items, by ascending document id, as
/// stored (pending items included; clients never see those).
pub async fn inbox_items(test: &TestServer, account_id: u32) -> Vec<(u32, CalendarEventNotification)> {
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

/// The SEQUENCE of the first VEVENT.
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
        .is_some_and(|value| value.trim_start_matches("mailto:").eq_ignore_ascii_case(address))
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
/// (invitations spec 11). Waits for due tasks first, so nothing earlier is
/// mid-flight, then wakes the consumer and waits until it is parked.
pub async fn hold_task_consumer(test: &TestServer) -> ConsumerHeld {
    test.wait_for_tasks_skip_not_due().await;
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
```

`HttpRequest` and `Method` stay imported for `park_login` and the vault helpers.

- [ ] **Step 2: The organizer fan-out test**

Create `tests/src/za/fanout.rs` with the SPDX header and:

```rust
//! Invitations spec 5: a key account's own request delivers sealed pending
//! items to internal key recipients instead of queueing mail. Plan 9 has no
//! drain, so the tests read items at store level and open them with the
//! recipient's private key (`open_pending_item_as`).

use super::{STRONG, dav_seal::raw_event};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        envelope_wrap, inbox_ids, item_attendees, item_method, item_sequence, mail_count,
        new_inbox_items, open_pending_item_as, schedule_statuses, wait_for_delivery, za_unlock,
    },
};
use calcard::icalendar::{ICalendarMethod, ICalendarProperty};
use groupware::calendar::{
    ChangedBy, EVENT_NOTIFICATION_ZA_PENDING,
    seal::{TOKEN_LEN, TokenPairs, event_token_pairs},
};
use hyper::StatusCode;
use vault::session::SessionKeys;

pub const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");
/// RFC 6638 `Schedule-Reply: F`: a cleanup DELETE that builds no CANCEL.
pub const NO_REPLY: (&str, &str) = ("schedule-reply", "F");
pub const KEY1_CAL: &str = "/dav/cal/key1%40example.com/default/";
pub const START_A: &str = "20990301T100000Z";
pub const START_B: &str = "20990302T100000Z";
pub const MKCALENDAR: &str = "<?xml version=\"1.0\" encoding=\"utf-8\" ?><A:mkcalendar xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"/>";

/// A key account of the suite (password `STRONG`) and its CalDAV client.
pub struct Party {
    pub id: u32,
    pub name: &'static str,
    pub client: DummyWebDavClient,
}

impl Party {
    pub fn new(test: &TestServer, name: &'static str) -> Self {
        let id = test.account(name).id().document_id();
        Party {
            id,
            name,
            client: DummyWebDavClient::new(id, name, STRONG, name),
        }
    }

    pub fn inbox(&self) -> String {
        format!("/dav/itip/{}/inbox/", self.name.replace('@', "%40"))
    }

    /// Deletes inbox items by href: upstream's DELETE, unchanged for pending
    /// items (plan 8).
    pub async fn delete_items(&self, ids: impl IntoIterator<Item = u32>) {
        for id in ids {
            self.client
                .request("DELETE", &format!("{}{id}.ics", self.inbox()), "")
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
    }

    /// Deletes every inbox item written since `before`.
    pub async fn delete_items_since(&self, test: &TestServer, before: &[u32]) {
        let ids: Vec<u32> = inbox_ids(test, self.id)
            .await
            .into_iter()
            .filter(|id| !before.contains(id))
            .collect();
        self.delete_items(ids).await;
    }
}

/// An invitation organized by key1. Each of `attendees` becomes an
/// ATTENDEE that expects a reply.
pub fn invite(uid: &str, start: &str, attendees: &[&str]) -> String {
    let mut ical = format!(
        concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\n",
            "UID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:{start}\r\nDURATION:PT1H\r\n",
            "SEQUENCE:0\r\nSUMMARY:fo-title {uid}\r\n",
            "ORGANIZER:mailto:key1@example.com\r\n"
        ),
        uid = uid,
        start = start
    );
    for attendee in attendees {
        ical.push_str(&format!(
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{attendee}\r\n"
        ));
    }
    ical.push_str("END:VEVENT\r\nEND:VCALENDAR\r\n");
    ical
}

/// The token pairs of an account's stored event at `path` (`default/x.ics`).
pub async fn stored_pairs(
    test: &TestServer,
    keys: &SessionKeys,
    account_id: u32,
    path: &str,
) -> Option<TokenPairs> {
    let (archive, _) = raw_event(test, account_id, path).await;
    event_token_pairs(&archive, keys, account_id).unwrap()
}

/// The SCHEDULE-STATUS values of `address`'s ATTENDEE properties in the
/// sender's copy, as its client reads it.
pub async fn attendee_statuses(client: &DummyWebDavClient, path: &str, address: &str) -> Vec<String> {
    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(!body.contains("X-ZA-"), "{body}");
    schedule_statuses(&body, ICalendarProperty::Attendee, address)
}

/// The token an opened item's pair carries for `address`.
pub fn pair_token(pair: Option<&TokenPairs>, address: &str) -> [u8; TOKEN_LEN] {
    let pair = pair.expect("the item carries a pair");
    assert_eq!(pair.len(), 1, "an item carries one pair");
    *pair.get(address).expect("the pair names the recipient")
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access fan-out tests...");
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let key3 = Party::new(test, "key3@example.com");
    let key5 = Party::new(test, "key5@example.com");
    let plain_id = test.account("plain@example.com").id().document_id();
    let keys1 = za_unlock(test, key1.id, STRONG).await;
    let start1 = inbox_ids(test, key1.id).await;
    let start2 = inbox_ids(test, key2.id).await;
    let start3 = inbox_ids(test, key3.id).await;
    let start5 = inbox_ids(test, key5.id).await;

    // 1. Invite with a mixed attendee list (spec 5.2, 5.3): key2 is
    // internal; plain is a non-key account; outsider is external.
    let plain_items = inbox_ids(test, plain_id).await;
    let plain_mail = mail_count(test, plain_id).await;
    let mixed = format!("{KEY1_CAL}fo-mixed.ics");
    let response = key1
        .client
        .request_with_headers(
            "PUT",
            &mixed,
            [CONTENT_TYPE],
            invite(
                "za-fo-mixed",
                START_A,
                &["key2@example.com", "plain@example.com", "outsider@remote.example"],
            ),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert!(
        response.headers.get("schedule-tag").is_some(),
        "the schedule tag is set as upstream sets it: {:?}",
        response.headers
    );
    assert_eq!(
        new_inbox_items(test, plain_id, &plain_items).await.len(),
        0,
        "plain's inbox received an item"
    );
    let items = new_inbox_items(test, key2.id, &start2).await;
    assert_eq!(items.len(), 1, "key2's inbox holds exactly one new item");
    let (item_id, item) = &items[0];
    assert_ne!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0, "the item is pending");
    assert_eq!(item.event_id, None);
    assert_eq!(item.changed_by, ChangedBy::PrincipalId(key1.id));
    assert_eq!(envelope_wrap(&item.event), Some(2), "sealed to key2's public key");
    assert!(!item.event.to_string().contains("fo-title"), "the stored item is sealed");
    let (tree, carriers) = open_pending_item_as(test, key2.id, STRONG, *item_id).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Request));
    assert_eq!(item_sequence(&tree), Some(0));
    assert!(tree.uids().any(|uid| uid == "za-fo-mixed"));
    assert_eq!(
        item_attendees(&tree),
        ["key2@example.com", "outsider@remote.example", "plain@example.com"]
    );
    assert!(!tree.to_string().contains("X-ZA-"), "no carrier left in the opened tree");
    assert!(carriers.reply_token.is_none());
    let key2_token = pair_token(carriers.pair.as_ref(), "key2@example.com");
    let pairs = stored_pairs(test, &keys1, key1.id, "default/fo-mixed.ics")
        .await
        .expect("key1's bundle holds pairs");
    assert_eq!(pairs.len(), 3, "one pair per scheduled attendee");
    assert_eq!(pairs.get("key2@example.com"), Some(&key2_token));
    assert_eq!(attendee_statuses(&key1.client, &mixed, "key2@example.com").await, ["1.2"]);
    assert_eq!(attendee_statuses(&key1.client, &mixed, "plain@example.com").await, ["5.3"]);
    assert_eq!(
        attendee_statuses(&key1.client, &mixed, "outsider@remote.example").await,
        ["5.3"]
    );
    // No mail (invariant 12). The task queue is read with the consumer held
    // in `test_task_queue` (Task 5).
    wait_for_delivery(test).await;
    assert_eq!(mail_count(test, plain_id).await, plain_mail, "plain received mail");
    assert_eq!(inbox_ids(test, key1.id).await, start1, "nothing in the sender's inbox");

    // Review Focus 1: the same attendee twice, in two cases.
    let before2 = inbox_ids(test, key2.id).await;
    let dup = format!("{KEY1_CAL}fo-dup.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &dup,
            [CONTENT_TYPE],
            invite("za-fo-dup", START_A, &["KEY2@Example.com", "key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 1, "one item for an attendee listed twice");
    let (_, carriers) = open_pending_item_as(test, key2.id, STRONG, items[0].0).await;
    let dup_token = pair_token(carriers.pair.as_ref(), "key2@example.com");
    let pairs = stored_pairs(test, &keys1, key1.id, "default/fo-dup.ics")
        .await
        .unwrap();
    assert_eq!(pairs.len(), 1, "one pair for an attendee listed twice");
    assert_eq!(pairs.get("key2@example.com"), Some(&dup_token));
    assert_eq!(
        attendee_statuses(&key1.client, &dup, "key2@example.com").await,
        ["1.2", "1.2"]
    );

    // Review Focus 3: a sub-address of the sender's own account. Not
    // attempted; nothing reaches key1's own inbox.
    let own = format!("{KEY1_CAL}fo-own.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &own,
            [CONTENT_TYPE],
            invite("za-fo-own", START_A, &["key1+fo@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(inbox_ids(test, key1.id).await, start1, "an item reached the sender");
    assert_eq!(
        attendee_statuses(&key1.client, &own, "key1+fo@example.com").await,
        ["5.3"]
    );

    // Review Focus 4: a recipient that never made a DAV request. The inbox
    // is virtual (`SCHEDULE_INBOX_ID`); nothing has to exist first.
    let fresh = format!("{KEY1_CAL}fo-fresh.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &fresh,
            [CONTENT_TYPE],
            invite("za-fo-fresh", START_A, &["key5@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let items = new_inbox_items(test, key5.id, &start5).await;
    assert_eq!(items.len(), 1, "key5's inbox holds the item");
    let (tree, carriers) = open_pending_item_as(test, key5.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Request));
    pair_token(carriers.pair.as_ref(), "key5@example.com");
    assert_eq!(attendee_statuses(&key1.client, &fresh, "key5@example.com").await, ["1.2"]);

    // 2. Updates fan out (spec 5.5). key1 moves the event: a second REQUEST
    // with SEQUENCE 1 and the same token.
    let before2 = inbox_ids(test, key2.id).await;
    key1.client
        .request_with_headers(
            "PUT",
            &mixed,
            [CONTENT_TYPE],
            invite(
                "za-fo-mixed",
                START_B,
                &["key2@example.com", "plain@example.com", "outsider@remote.example"],
            ),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 1, "the update reached key2 once");
    let (tree, carriers) = open_pending_item_as(test, key2.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Request));
    assert_eq!(item_sequence(&tree), Some(1));
    assert_eq!(
        pair_token(carriers.pair.as_ref(), "key2@example.com"),
        key2_token,
        "the token is stable across updates"
    );

    // key1 removes key2 and adds key3: key2 gets a CANCEL with its old pair,
    // key3 a REQUEST with a fresh one.
    let before2 = inbox_ids(test, key2.id).await;
    let before3 = inbox_ids(test, key3.id).await;
    key1.client
        .request_with_headers(
            "PUT",
            &mixed,
            [CONTENT_TYPE],
            invite(
                "za-fo-mixed",
                START_B,
                &["key3@example.com", "plain@example.com", "outsider@remote.example"],
            ),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 1, "the removed attendee gets one CANCEL");
    let (tree, carriers) = open_pending_item_as(test, key2.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Cancel));
    assert_eq!(
        pair_token(carriers.pair.as_ref(), "key2@example.com"),
        key2_token,
        "the CANCEL carries the removed attendee's old pair (spec 3.3)"
    );
    let items = new_inbox_items(test, key3.id, &before3).await;
    assert_eq!(items.len(), 1, "the added attendee gets one REQUEST");
    let (tree, carriers) = open_pending_item_as(test, key3.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Request));
    let key3_token = pair_token(carriers.pair.as_ref(), "key3@example.com");
    assert_ne!(key3_token, key2_token);

    // 10. Pairs maintenance: the bundle keeps the current attendees' pairs.
    let pairs = stored_pairs(test, &keys1, key1.id, "default/fo-mixed.ics")
        .await
        .unwrap();
    assert_eq!(pairs.len(), 3, "key3, plain and outsider");
    assert!(pairs.get("key2@example.com").is_none(), "the removed pair is gone");
    assert_eq!(pairs.get("key3@example.com"), Some(&key3_token));
    // A re-added attendee gets a different token.
    let before2 = inbox_ids(test, key2.id).await;
    key1.client
        .request_with_headers(
            "PUT",
            &mixed,
            [CONTENT_TYPE],
            invite(
                "za-fo-mixed",
                START_B,
                &[
                    "key2@example.com",
                    "key3@example.com",
                    "plain@example.com",
                    "outsider@remote.example",
                ],
            ),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 1, "the re-added attendee gets one REQUEST");
    let (_, carriers) = open_pending_item_as(test, key2.id, STRONG, items[0].0).await;
    let key2_new_token = pair_token(carriers.pair.as_ref(), "key2@example.com");
    assert_ne!(key2_new_token, key2_token, "a re-added attendee gets a new token");
    let pairs = stored_pairs(test, &keys1, key1.id, "default/fo-mixed.ics")
        .await
        .unwrap();
    assert_eq!(pairs.len(), 4);
    assert_eq!(pairs.get("key2@example.com"), Some(&key2_new_token));
    assert_eq!(pairs.get("key3@example.com"), Some(&key3_token), "a kept attendee keeps its token");

    // Review Focus 2: an update that removes every ATTENDEE sends only
    // CANCELs, with the old pairs, and leaves no pair in the bundle.
    let gone = format!("{KEY1_CAL}fo-gone.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &gone,
            [CONTENT_TYPE],
            invite("za-fo-gone", START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let gone_token = *stored_pairs(test, &keys1, key1.id, "default/fo-gone.ics")
        .await
        .unwrap()
        .get("key2@example.com")
        .unwrap();
    let before2 = inbox_ids(test, key2.id).await;
    key1.client
        .request_with_headers("PUT", &gone, [CONTENT_TYPE], invite("za-fo-gone", START_A, &[]))
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 1, "only a CANCEL");
    let (tree, carriers) = open_pending_item_as(test, key2.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Cancel));
    assert_eq!(pair_token(carriers.pair.as_ref(), "key2@example.com"), gone_token);
    assert!(
        stored_pairs(test, &keys1, key1.id, "default/fo-gone.ics")
            .await
            .is_none(),
        "no attendee, no pair"
    );

    // Cleanup: the events without CANCELs, then every new item.
    for name in ["fo-mixed", "fo-dup", "fo-own", "fo-fresh", "fo-gone"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.delete_items_since(test, &start1).await;
    key2.delete_items_since(test, &start2).await;
    key3.delete_items_since(test, &start3).await;
    key5.delete_items_since(test, &start5).await;
    wait_for_delivery(test).await;
    // Counting plain's mail created its default mailboxes; plain outlives
    // the suite.
    test.destroy_all_mailboxes(test.account("plain@example.com"))
        .await;
}
```

In `tests/src/za/mod.rs`, add `pub mod fanout;` after `pub mod expansion;`, and `fanout::test(&mut test).await;` right after `dav_seal::test_legacy(&mut test).await;` (Global Constraints, Tests: the new sub-modules run before `tracing`).

- [ ] **Step 3: The assertions `SendSealed` changes**

`tests/src/za/tracing.rs`, replace `:717-730` (from the comment `// Scheduling: the plain account's past event` through the `zero-access accounts` assertion) with:

```rust
    // Scheduling: a past event traces its reason on the create and the
    // update path, for the plain account and, since a key account resolves
    // its sending state under upstream's preconditions (invitations spec
    // 5.1, `SendSealed`), for the key account too.
    let itip = traced_as(EventType::Calendar(CalendarEvent::ItipMessageError));
    assert_eq!(
        itip.matches("lies in the past").count(),
        4,
        "scheduling reason not traced on both PUT paths of both accounts: {itip}"
    );
    assert!(
        !itip.contains("zero-access accounts"),
        "the removed key-account refusal was traced: {itip}"
    );
```

`tests/src/za/gating.rs`, replace `:482-497` (from `// Organizer is a key account: stored, nothing sent, no schedule tag.` through the `schedule-tag` assertion) with:

```rust
    // Organizer is a key account (invitations spec 5.1, `SendSealed`): the
    // schedule tag is set as upstream sets it. plain is a non-key account,
    // so the fan-out does not attempt it and no mail is sent.
    let response = key_client
        .request_with_headers(
            "PUT",
            &format!("{key_cal}invite.ics"),
            [CONTENT_TYPE],
            KEY_INVITE,
        )
        .await
        .with_status(StatusCode::CREATED);
    assert!(
        response.headers.get("schedule-tag").is_some(),
        "{:?}",
        response.headers
    );
```

and replace `:553-562` (from `// A CANCEL on DELETE needs a stored schedule tag` through the `.is_none()` assertion) with:

```rust
    // The stored event carries the schedule tag a CANCEL on DELETE needs
    // (`delete_all` in groupware's calendar storage).
    let (archive, _) = raw_event(test, key1_id, "default/invite.ics").await;
    assert!(
        archive
            .unarchive::<CalendarEvent>()
            .unwrap()
            .schedule_tag
            .is_some()
    );
```

`tests/src/za/pairs.rs`: after `const EVENT` (`:28-37`), add:

```rust
/// `EVENT` without attendees: upstream's builders make no message for it, so
/// no rewrite hands out tokens (invitations spec 3.3, 5.1).
const EVENT_NO_ATTENDEES: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\n",
    "UID:za-pairs-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990101T100000Z\r\n",
    "DTEND:20990101T110000Z\r\nSEQUENCE:0\r\nSUMMARY:pairs-canary\r\n",
    "ORGANIZER:mailto:key1@example.com\r\n",
    "END:VEVENT\r\nEND:VCALENDAR\r\n"
);
```

and change the two bodies of the "events without pairs" block: `:134` becomes `let r1_body = EVENT_NO_ATTENDEES.replace("za-pairs-1", "za-pairs-r1");` and `:165` becomes `let legacy_body = EVENT_NO_ATTENDEES.replace("za-pairs-1", "za-pairs-legacy");`. Extend the block's comment (`:129-132`) with one sentence: "They carry no attendee: with attendees, the fan-out hands out tokens (invitations spec 3.3)."

`tests/src/webdav/za_variants.rs`: replace the `crate::utils` import (`:10-13`) with:

```rust
use crate::utils::{
    server::TestServer,
    za::{
        envelope_wrap, hold_task_consumer, inbox_ids, itip_tasks_for, mail_count,
        new_inbox_items, plant_event, queued_recipients, schedule_statuses, wait_for_delivery,
    },
};
```

add `use calcard::icalendar::ICalendarProperty;` and change `calendar::CalendarEvent,` in the groupware import (`:17`) to `calendar::{CalendarEvent, EVENT_NOTIFICATION_ZA_PENDING},`. Replace `scheduling` (`:351-415`) with:

```rust
pub async fn scheduling(test: &TestServer) {
    println!("Running key-account scheduling tests...");
    let john = test.account("john@example.com").webdav_client();
    let jane = test.account("jane@example.com").webdav_client();
    let jane_mail = mail_count(test, jane.account_id).await;
    let jane_items = inbox_ids(test, jane.account_id).await;
    let invite = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:za-sched-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:sched-canary\r\nORGANIZER:mailto:john@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:jane@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    // Invitations spec 5: john and jane are key accounts, so john's PUT
    // delivers a sealed pending item into jane's inbox instead of queueing
    // mail. The consumer is held so the task-queue read cannot miss a task.
    let held = hold_task_consumer(test).await;
    let response = john
        .request_with_headers(
            "PUT",
            "/dav/cal/john@example.com/default/s.ics",
            [("content-type", "text/calendar; charset=utf-8")],
            invite,
        )
        .await
        .with_status(StatusCode::CREATED);
    // Upstream asserts the organizer's schedule tag; it holds in key mode
    // too (spec 5.1).
    assert!(
        response.headers.get("schedule-tag").is_some(),
        "{:?}",
        response.headers
    );
    assert_eq!(
        itip_tasks_for(test, john.account_id).await,
        0,
        "a CalendarItipMessage task was queued for john"
    );
    drop(held);
    wait_for_delivery(test).await;
    // No iMIP email reached jane's mailbox.
    assert_eq!(mail_count(test, jane.account_id).await, jane_mail);
    // Store level: exactly one new item in jane's inbox, pending and sealed
    // to her public key.
    let items = new_inbox_items(test, jane.account_id, &jane_items).await;
    assert_eq!(items.len(), 1, "jane's inbox holds {} new items", items.len());
    let (_, item) = &items[0];
    assert_ne!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0, "the item is pending");
    assert_eq!(envelope_wrap(&item.event), Some(2), "the item has a pk envelope");
    assert!(!item.event.to_string().contains("sched-canary"));
    // Clients do not see pending items (spec 6.6); hrefs() includes the
    // collection itself, so one href is an inbox that lists nothing.
    let inbox = jane
        .request_with_headers(
            "PROPFIND",
            "/dav/itip/jane@example.com/inbox/",
            [("depth", "1")],
            "",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_eq!(inbox.hrefs().len(), 1, "{:?}", inbox.hrefs());
    // The event is readable by its owner with attendees intact, and jane's
    // ATTENDEE says delivered.
    let body = john
        .request("GET", "/dav/cal/john@example.com/default/s.ics", "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(body.contains("mailto:jane@example.com") && !body.contains("X-ZA-"), "{body}");
    assert_eq!(
        schedule_statuses(&body, ICalendarProperty::Attendee, "jane@example.com"),
        ["1.2"]
    );
    let (archive, _) = crate::za::dav_seal::raw_event(test, john.account_id, "default/s.ics").await;
    assert!(
        archive
            .unarchive::<CalendarEvent>()
            .unwrap()
            .schedule_tag
            .is_some()
    );
    john.request_with_headers(
        "DELETE",
        "/dav/cal/john@example.com/default/s.ics",
        [("schedule-reply", "F")],
        "",
    )
    .await
    .with_status(StatusCode::NO_CONTENT);
    wait_for_delivery(test).await;
    assert_eq!(mail_count(test, jane.account_id).await, jane_mail);
    // Pending items are deleted by href, as any inbox item.
    let ids: Vec<u32> = inbox_ids(test, jane.account_id)
        .await
        .into_iter()
        .filter(|id| !jane_items.contains(id))
        .collect();
    for id in ids {
        jane.request("DELETE", &format!("/dav/itip/jane@example.com/inbox/{id}.ics"), "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    john.delete_default_containers().await;
    jane.delete_default_containers().await;
    // Counting jane's mail created her default mailboxes.
    test.destroy_all_mailboxes(test.account("jane@example.com"))
        .await;
    test.assert_is_empty().await;
}
```

- [ ] **Step 4: Run to see it fail**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture 2>&1 | grep -E "panicked|schedule tag|test result" | head -5`
Expected: FAIL in `fanout.rs`, `the schedule tag is set as upstream sets it`: the key account still resolves to `KeyAccount`, so nothing is built, and `fanout::test` runs before `tracing` and `gating`.

- [ ] **Step 5: `SendSealed`**

In `crates/groupware/src/calendar/itip.rs`, replace the enum (`:59-68`) with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItipSendStatus {
    Send,
    /// Invitations spec 5.1: a key account under upstream's `Send`
    /// preconditions. Upstream's builders run on the plaintext tree; the
    /// messages go to the fan-out (`calendar::fanout`), never to the task
    /// queue.
    SendSealed,
    NotRequested,
    SchedulingDisabled,
    NoCalendarAddress,
    NoPermission,
    EventInPast,
}
```

Replace `resolve`, `is_denied` and `reason` (`:1065-1122`) with:

```rust
    pub fn resolve(
        server: &Server,
        access_token: &AccessToken,
        account_info: &AccountInfo,
        event_range_end: i64,
    ) -> Self {
        if !server.core.groupware.itip_enabled {
            Self::SchedulingDisabled
        } else if account_info.addresses().is_empty() {
            Self::NoCalendarAddress
        } else if !access_token.has_permission(Permission::CalendarSchedulingSend) {
            Self::NoPermission
        } else if event_range_end <= now() as i64 {
            Self::EventInPast
        } else if account_info.account().is_key_account() {
            Self::SendSealed
        } else {
            Self::Send
        }
    }

    #[inline(always)]
    pub fn is_send(&self) -> bool {
        matches!(self, Self::Send)
    }

    /// Whether upstream's builders run: `Send`, and `SendSealed` for a key
    /// account (invitations spec 5.1).
    #[inline(always)]
    pub fn builds_messages(&self) -> bool {
        matches!(self, Self::Send | Self::SendSealed)
    }

    #[inline(always)]
    pub fn is_send_sealed(&self) -> bool {
        matches!(self, Self::SendSealed)
    }

    #[inline(always)]
    pub fn is_denied(&self) -> bool {
        matches!(
            self,
            Self::SchedulingDisabled | Self::NoCalendarAddress | Self::NoPermission
        )
    }

    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Send | Self::SendSealed | Self::NotRequested => None,
            Self::SchedulingDisabled => Some("Calendar scheduling is disabled on this server."),
            Self::NoCalendarAddress => {
                Some("This account has no calendar address to send scheduling messages from.")
            }
            Self::NoPermission => {
                Some("This account is not allowed to send calendar scheduling messages.")
            }
            Self::EventInPast => {
                Some("No scheduling messages were sent because the event lies in the past.")
            }
        }
    }
```

- [ ] **Step 6: The lease and re-check helpers**

In `crates/dav/src/calendar/mod.rs`, replace the imports (`:17-26`) with:

```rust
use crate::{DavError, DavErrorCondition};
use common::{
    DavResources, Server,
    za::lease::{ZaUidLease, ZaUidLeaseGuard},
};
use dav_proto::schema::{
    property::{CalDavProperty, CalendarData, DavProperty, WebDavProperty},
    response::CalCondition,
};
use groupware::{calendar::CalendarEvent, scheduling::ItipError};
use hyper::StatusCode;
use store::{
    ValueKey,
    write::{AlignedBytes, Archive, BatchBuilder},
};
use trc::AddContext;
use types::{collection::Collection, field::CalendarEventField};
```

After `assert_is_unique_uid` (`:94-126`), add:

```rust
/// Invitations spec 5.1, 6.3 step 1: takes the UID creation lease for a key
/// account's PUT create, then checks the UID against current archives
/// (`za_assert_is_unique_uid`) while holding it. A lease that stays held
/// through the retries (two creators of one UID in one account within a
/// second) answers 503 with a trace; a hit answers `NoUidConflict` after the
/// lease is released.
pub(crate) async fn za_take_uid_lease(
    server: &Server,
    resources: &DavResources,
    account_id: u32,
    calendar_id: u32,
    uid: &str,
) -> crate::Result<ZaUidLeaseGuard> {
    let Some(lease) = ZaUidLease::acquire(server.store(), account_id, uid)
        .await
        .caused_by(trc::location!())?
    else {
        trc::event!(
            Store(trc::StoreEvent::AssertValueFailed),
            AccountId = account_id,
            Details = "zero-access: UID creation lease busy",
        );
        return Err(DavError::Code(StatusCode::SERVICE_UNAVAILABLE));
    };
    let lease = ZaUidLeaseGuard::new(server.store().clone(), lease);
    if let Err(err) = za_assert_is_unique_uid(server, resources, account_id, calendar_id, uid).await
    {
        lease.release().await;
        return Err(err);
    }
    Ok(lease)
}

/// Invitations spec 5.1: the archive-based counterpart of
/// `assert_is_unique_uid` for a key account's create, run under the
/// creation lease. The handler's resource snapshot can predate another
/// creator's commit, so this reads the fresh UID-index hits and each hit's
/// current archive, and refuses the UID when a stored copy names the target
/// calendar (upstream's rule: a UID is unique within one calendar). The
/// snapshot only formats the href.
pub(crate) async fn za_assert_is_unique_uid(
    server: &Server,
    resources: &DavResources,
    account_id: u32,
    calendar_id: u32,
    uid: &str,
) -> crate::Result<()> {
    let hits = server
        .document_ids_matching(
            account_id,
            Collection::CalendarEvent,
            CalendarEventField::Uid,
            uid.as_bytes(),
        )
        .await
        .caused_by(trc::location!())?;
    for document_id in hits {
        let Some(stored) = server
            .store()
            .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                account_id,
                Collection::CalendarEvent,
                document_id,
            ))
            .await
            .caused_by(trc::location!())?
        else {
            continue;
        };
        let event = stored
            .unarchive::<CalendarEvent>()
            .caused_by(trc::location!())?;
        if let Some(name) = event
            .names
            .iter()
            .find(|name| name.parent_id.to_native() == calendar_id)
        {
            let href = resources
                .container_resource_path_by_id(calendar_id)
                .map(|calendar| resources.format_item(&format!("{}/{}", calendar.path(), name.name)))
                .unwrap_or_default();
            return Err(DavError::Condition(DavErrorCondition::new(
                StatusCode::PRECONDITION_FAILED,
                CalCondition::NoUidConflict(href.into()),
            )));
        }
    }
    Ok(())
}

/// Invitations spec 6.3 steps 3-4: commits a key account's create batch,
/// which carries the lease's fenced-commit assertion, then releases the
/// lease. If the lease was taken over while the request ran, the commit
/// loses its assertion: the request takes the lease again and re-checks
/// current archives (`NoUidConflict` on a hit), and otherwise answers 503 so
/// the client resends.
pub(crate) async fn za_commit_create(
    server: &Server,
    resources: &DavResources,
    batch: BatchBuilder,
    lease: ZaUidLeaseGuard,
    account_id: u32,
    calendar_id: u32,
    uid: &str,
) -> crate::Result<()> {
    #[cfg(feature = "test_mode")]
    if common::za::test_hooks::take_commit_fail(account_id) {
        lease.release().await;
        return Err(trc::StoreEvent::UnexpectedError
            .into_err()
            .details("zero-access test hook: sender commit failure")
            .caused_by(trc::location!())
            .into());
    }
    match server.commit_batch(batch).await {
        Ok(_) => {
            lease.release().await;
            Ok(())
        }
        Err(err) if err.is_assertion_failure() => {
            lease.abandon();
            let retry = za_take_uid_lease(server, resources, account_id, calendar_id, uid).await?;
            retry.release().await;
            trc::event!(
                Store(trc::StoreEvent::AssertValueFailed),
                AccountId = account_id,
                Details = "zero-access: UID creation lease lost before the commit",
            );
            Err(DavError::Code(StatusCode::SERVICE_UNAVAILABLE))
        }
        Err(err) => {
            lease.release().await;
            Err(err.caused_by(trc::location!()).into())
        }
    }
}
```

- [ ] **Step 7: Both PUT branches**

In `crates/dav/src/calendar/update.rs`:
- Replace `use super::assert_is_unique_uid;` (`:7`) with `use super::{assert_is_unique_uid, za_commit_create, za_take_uid_lease};`.
- In the groupware import (`:30-40`), replace the `calendar::{..}` entry with:

```rust
    calendar::{
        CalendarEvent, CalendarEventData,
        fanout::{ZaFanOut, za_set_schedule_status},
        itip::ItipSendStatus,
        seal::{event_token_pairs, seal_error, seal_event},
    },
```

- After the `fetch_dav_resources` statement (`:81-88`):

```rust
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::put_pause_point(account_id).await;
```

The same four scheduling edits go into both branches (update `:239-324`, create `:433-485`). Upstream's lines between them, the `Err(err)` arms included, stay as they are.
1. After `let mut itip_messages = None;` add `let mut za_messages = None;`.
2. Replace `if itip_status.is_send() {` with `if itip_status.builds_messages() {`.
3. Replace `itip_messages = Some(ItipMessages::new(messages));` with:

```rust
                            // Invitations spec 5.1: a key account's messages
                            // go to the fan-out, never to the task queue.
                            if itip_status.is_send_sealed() {
                                za_messages = Some(messages);
                            } else {
                                itip_messages = Some(ItipMessages::new(messages));
                            }
```

4. Replace the `} else if itip_status != ItipSendStatus::KeyAccount && let Some(reason) = itip_status.reason() { .. }` arm, comment included, with upstream's arm. In the update branch (`:313-324`):

```rust
            } else if let Some(reason) = itip_status.reason() {
                trc::event!(
                    Calendar(trc::CalendarEvent::ItipMessageError),
                    AccountId = account_id,
                    DocumentId = document_id,
                    Reason = reason,
                );
            }
```

   In the create branch (`:476-485`), the same without the `DocumentId` line.

Update branch: replace the seal block (`:333-339`) with:

```rust
            if let Some(keys) = &za_keys {
                // Invitations spec 3.3: every rewrite carries the stored pairs.
                let mut pairs = event_token_pairs(&event_, keys, account_id)
                    .map_err(|err| seal_error(err, account_id, document_id))?;
                // Spec 5.3-5.4: after every check of the request and
                // immediately before the sealed commit, the items go out; the
                // fan-out decides the pairs the bundle keeps.
                if let Some(messages) = za_messages {
                    let fan_out = self
                        .za_fan_out(&account_info, keys, messages, pairs, &new_event.data.event)
                        .await
                        .caused_by(trc::location!())?;
                    za_set_schedule_status(&mut new_event.data.event, &fan_out.statuses);
                    pairs = fan_out.pairs;
                }
                seal_event(&mut new_event, keys, account_id, pairs.as_ref())
                    .map_err(|err| seal_error(err, account_id, document_id))?;
            }
```

Create branch, four more edits:
1. Replace the `// Validate ical object` block (`:405-413`) with:

```rust
            // Validate ical object
            let uid = validate_ical(&ical)?.to_string();
            // Invitations spec 5.1, 6.3: a key account's create holds the UID
            // creation lease from here through the insert, and checks current
            // archives instead of the snapshot.
            let za_lease = if za_keys.is_some() {
                Some(
                    za_take_uid_lease(self, &resources, account_id, parent.document_id(), &uid)
                        .await?,
                )
            } else {
                assert_is_unique_uid(
                    self,
                    &resources,
                    account_id,
                    parent.document_id(),
                    Some(uid.as_str()),
                )
                .await?;
                None
            };
```

2. Replace the seal block (`:496-499`) with:

```rust
            if let Some(keys) = &za_keys {
                // Spec 5.3-5.4: after every check, immediately before the
                // sealed commit; the fan-out hands out the attendees' tokens.
                let mut pairs = None;
                if let Some(messages) = za_messages {
                    let fan_out = self
                        .za_fan_out(&account_info, keys, messages, None, &event.data.event)
                        .await
                        .caused_by(trc::location!())?;
                    za_set_schedule_status(&mut event.data.event, &fan_out.statuses);
                    pairs = fan_out.pairs;
                }
                seal_event(&mut event, keys, account_id, pairs.as_ref())
                    .map_err(|err| seal_error(err, account_id, u32::MAX))?;
            }
```

3. After `let mut batch = BatchBuilder::new();` (`:502`):

```rust
            // Spec 6.3 step 3: the insert is fenced on the lease, in the same
            // transaction (ahead of the insert, so no commit point splits them).
            if let Some(lease) = &za_lease {
                lease.lease().assert_in(&mut batch);
            }
```

4. Replace `self.commit_batch(batch).await.caused_by(trc::location!())?;` (`:524`) with:

```rust
            if let Some(lease) = za_lease {
                za_commit_create(
                    self,
                    &resources,
                    batch,
                    lease,
                    account_id,
                    parent.document_id(),
                    &uid,
                )
                .await?;
            } else {
                self.commit_batch(batch).await.caused_by(trc::location!())?;
            }
```

- [ ] **Step 8: Run green**

Run the per-task compile check (Global Constraints), then the za suite, then the key-mode CalDAV suite:

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture
ZA_KEY_ACCOUNTS=1 STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture
```

Expected: `clean`; both suites PASS.

Values from code reading, with their fallbacks:
- `key1+fo@example.com` resolves to key1 (sub-addressing on by default; `account_id_from_email` splits at `+`, `crates/common/src/cache/principals.rs:659-672`). If the address does not resolve, the Review Focus 3 assertions still pass but no longer reach the sender check; record a ruling, and Task 8 marks the check "layered behind" the resolution.
- The suite's key accounts hold `CalendarSchedulingReceive` through the default user role, as upstream's plain-mode `cal_scheduling` relies on for jane. If key2 gets `5.3` where the test expects `1.2`, stop and report.
- The update that moves the event yields one full REQUEST to key2 (`organizer_handle_update`, `crates/groupware/src/scheduling/organizer.rs:30-78`). If key2 gets two items there, stop and report.

- [ ] **Step 9: Red run, the recipient split (M22)**

In `crates/groupware/src/calendar/fanout.rs`, `resolve_inner`, replace everything after the `account_id == sender_id` check with `Ok(Recipient::Internal { account_id, public_key: [9u8; 32] })` (every local account, key or not, becomes internal). Run the za suite.
Expected: FAIL in `fanout.rs`, `plain's inbox received an item` (left 1, right 0).

Revert with `git checkout -- crates/groupware/src/calendar/fanout.rs` and confirm `git diff --stat -- crates/groupware/src/calendar/fanout.rs` prints nothing (this task's own `itip.rs`, `mod.rs` and `update.rs` changes stay). Write the failure line into the task report.

- [ ] **Step 10: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p groupware -- --check
cargo fmt -p dav -- --check
cargo fmt -p tests -- --check
git add crates/groupware/src/calendar/itip.rs crates/dav/src/calendar/mod.rs crates/dav/src/calendar/update.rs tests/src/utils/za.rs tests/src/za/fanout.rs tests/src/za/mod.rs tests/src/za/tracing.rs tests/src/za/gating.rs tests/src/za/pairs.rs tests/src/webdav/za_variants.rs
git commit -m "SendSealed: a key account's PUT fans out sealed items under the UID lease"
```

---

### Task 4: Replies, delegation and partial failures

**Files:**
- Modify: `tests/src/utils/za.rs` (`plant_attendee_copy` after `plant_sealed_event`)
- Create: `tests/src/za/reply.rs`
- Modify: `tests/src/za/fanout.rs` (imports; new `test_failures`)
- Modify: `tests/src/za/mod.rs` (`pub mod reply;`; calls after `fanout::test`)

**Interfaces:**
- Consumes: Task 3's helpers and `fanout.rs` items (`Party`, `invite`, `stored_pairs`, `attendee_statuses`, `pair_token`, `CONTENT_TYPE`, `NO_REPLY`, `KEY1_CAL`, `START_A`); Task 1's `test_hooks::set_commit_fail_once`; plan 8's `RSVP_TOKEN_PROP`, `TokenPairs`; `insert_event` (`tests/src/utils/za.rs`, private).
- Produces: `plant_attendee_copy(test, account_id, name, ical, password, pairs: &TokenPairs) -> u32`; `reply::test`, `fanout::test_failures`; `reply.rs`'s `copy` and `organizer_statuses` (Task 5 extends `reply::test`).

Background:
- No product code changes here. The tests pin what Tasks 2 and 3 built for the attendee side (spec 5.5) and for partial failure (5.3 step 5, 5.4), with two mutation checks.
- An attendee's copy as the drain will import it (plan 10) holds the attendee's own pair and upstream's import schedule tag (`Some(1)`, `calendar/itip.rs:270`). The tag is what sends the attendee's PUT through `itip_update` (`update.rs`: `new_event.schedule_tag.is_some()`); without it, `itip_create` refuses a copy whose ORGANIZER is another account (`NotOrganizer`). `plant_attendee_copy` writes that form; empty pairs write no bundle.
- Delegation, two cases:
  - **(a) key2 delegates to key3.** Upstream builds a REPLY to key1 and a REQUEST from key2 to key3 (`from_organizer = false`); only the REPLY is attempted.
  - **(b) key2 is key3's delegate and accepts.** Upstream addresses the REPLY to key1 and to the delegator key3 (`is_delegated_to`, `attendee.rs:137-141`); only key1 is attempted. Case (b) is the one the ORGANIZER-only rule decides; without it the brief's "no item to any delegator" would hold vacuously.
- The quota and size settings are restored before the test returns: key2's quotas map is reset to `{}` (no disk quota); `CalendarScheduling` returns to its defaults (`itipMaxSize` 524288, `maxRecipients` 100).

- [ ] **Step 1: The attendee-copy helper**

In `tests/src/utils/za.rs`, after `plant_sealed_event`:

```rust
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
```

- [ ] **Step 2: The reply and delegation test**

Create `tests/src/za/reply.rs` with the SPDX header and:

```rust
//! Invitations spec 5.5: an attendee's own PUT replies to the organizer
//! with its visible token, only to the ORGANIZER, and only when its copy
//! holds its pair; an attendee-originated delegate invitation is not
//! attempted.

use super::{
    STRONG,
    fanout::{CONTENT_TYPE, NO_REPLY, Party},
    user_permissions,
};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        inbox_ids, item_method, item_partstat, new_inbox_items, open_pending_item_as,
        plant_attendee_copy, schedule_statuses, za_setup_token,
    },
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use calcard::icalendar::{ICalendarMethod, ICalendarProperty};
use groupware::calendar::{
    EVENT_NOTIFICATION_ZA_PENDING,
    seal::{RSVP_TOKEN_PROP, TOKEN_LEN, TokenPairs},
};
use hyper::StatusCode;

pub const KEY2_CAL: &str = "/dav/cal/key2%40example.com/default/";
const TOKEN: [u8; TOKEN_LEN] = *b"fo-reply-token01";
const KEY2_NEEDS: &str = "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com";
const KEY2_ACCEPTED: &str = "ATTENDEE;PARTSTAT=ACCEPTED;RSVP=TRUE:mailto:key2@example.com";

/// An invitation as key2's copy holds it. `attendees` are whole ATTENDEE
/// lines.
pub fn copy(uid: &str, organizer: &str, attendees: &[&str]) -> String {
    let mut ical = format!(
        concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\n",
            "UID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990401T100000Z\r\n",
            "DURATION:PT1H\r\nSEQUENCE:0\r\nSUMMARY:fo-reply-title\r\n",
            "ORGANIZER:mailto:{organizer}\r\n"
        ),
        uid = uid,
        organizer = organizer
    );
    for attendee in attendees {
        ical.push_str(attendee);
        ical.push_str("\r\n");
    }
    ical.push_str("END:VEVENT\r\nEND:VCALENDAR\r\n");
    ical
}

/// The SCHEDULE-STATUS values of the ORGANIZER property in key2's copy.
pub async fn organizer_statuses(client: &DummyWebDavClient, path: &str, organizer: &str) -> Vec<String> {
    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    schedule_statuses(&body, ICalendarProperty::Organizer, organizer)
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access reply and delegation tests...");
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let key3 = Party::new(test, "key3@example.com");
    let start1 = inbox_ids(test, key1.id).await;
    let start3 = inbox_ids(test, key3.id).await;
    let mut pairs = TokenPairs::new();
    pairs.insert("key2@example.com", TOKEN);

    // 3. key2's copy holds its pair; key2 accepts. key1's inbox gets one
    // pending REPLY whose visible token is key2's.
    let reply = format!("{KEY2_CAL}fo-reply.ics");
    plant_attendee_copy(
        test,
        key2.id,
        "fo-reply.ics",
        &copy("za-fo-reply", "key1@example.com", &[KEY2_NEEDS]),
        STRONG,
        &pairs,
    )
    .await;
    key2.client
        .request_with_headers(
            "PUT",
            &reply,
            [CONTENT_TYPE],
            copy("za-fo-reply", "key1@example.com", &[KEY2_ACCEPTED]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key1.id, &start1).await;
    assert_eq!(items.len(), 1, "key1's inbox holds one reply");
    let (item_id, item) = &items[0];
    assert_ne!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0);
    let visible = item.event.components[0]
        .entries
        .iter()
        .find(|entry| {
            matches!(&entry.name, ICalendarProperty::Other(name) if name.eq_ignore_ascii_case(RSVP_TOKEN_PROP))
        })
        .and_then(|entry| entry.values.first())
        .and_then(|value| value.as_text())
        .map(str::to_string);
    assert_eq!(visible, Some(URL_SAFE_NO_PAD.encode(TOKEN)), "the visible token");
    let (tree, carriers) = open_pending_item_as(test, key1.id, STRONG, *item_id).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Reply));
    assert_eq!(item_partstat(&tree, "key2@example.com").as_deref(), Some("ACCEPTED"));
    assert_eq!(carriers.reply_token.as_deref(), Some(&TOKEN));
    assert!(carriers.pair.is_none());
    assert_eq!(
        organizer_statuses(&key2.client, &reply, "key1@example.com").await,
        ["1.2"]
    );
    // Removed here without a reply; the decline on DELETE is pinned with
    // the DELETE paths.
    key2.client
        .request_with_headers("DELETE", &reply, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);

    // A copy whose bundle holds no pair for key2: the PUT succeeds, nothing
    // is delivered (the organizer's drain would drop it as a forgery), and
    // the ORGANIZER property says not attempted.
    let nopair = format!("{KEY2_CAL}fo-nopair.ics");
    plant_attendee_copy(
        test,
        key2.id,
        "fo-nopair.ics",
        &copy("za-fo-nopair", "key1@example.com", &[KEY2_NEEDS]),
        STRONG,
        &TokenPairs::new(),
    )
    .await;
    let before1 = inbox_ids(test, key1.id).await;
    key2.client
        .request_with_headers(
            "PUT",
            &nopair,
            [CONTENT_TYPE],
            copy("za-fo-nopair", "key1@example.com", &[KEY2_ACCEPTED]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(inbox_ids(test, key1.id).await, before1, "a reply without a token went out");
    assert_eq!(
        organizer_statuses(&key2.client, &nopair, "key1@example.com").await,
        ["5.3"]
    );

    // 4(a). key2 delegates to key3: key1 gets the REPLY only; key2's
    // attendee-originated REQUEST to key3 is not attempted.
    let delegate = format!("{KEY2_CAL}fo-delegate.ics");
    plant_attendee_copy(
        test,
        key2.id,
        "fo-delegate.ics",
        &copy("za-fo-delegate", "key1@example.com", &[KEY2_NEEDS]),
        STRONG,
        &pairs,
    )
    .await;
    let before1 = inbox_ids(test, key1.id).await;
    key2.client
        .request_with_headers(
            "PUT",
            &delegate,
            [CONTENT_TYPE],
            copy(
                "za-fo-delegate",
                "key1@example.com",
                &[
                    "ATTENDEE;PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:key3@example.com\":mailto:key2@example.com",
                    "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;DELEGATED-FROM=\"mailto:key2@example.com\":mailto:key3@example.com",
                ],
            ),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        inbox_ids(test, key3.id).await,
        start3,
        "key2's delegate invitation reached key3"
    );
    let items = new_inbox_items(test, key1.id, &before1).await;
    assert_eq!(items.len(), 1, "key1 gets the REPLY");
    let (tree, carriers) = open_pending_item_as(test, key1.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Reply));
    assert_eq!(item_partstat(&tree, "key2@example.com").as_deref(), Some("DELEGATED"));
    assert_eq!(carriers.reply_token.as_deref(), Some(&TOKEN));
    let body = key2
        .client
        .request("GET", &delegate, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert_eq!(
        schedule_statuses(&body, ICalendarProperty::Attendee, "key3@example.com"),
        ["5.3"]
    );

    // 4(b). key2 is key3's delegate and accepts: upstream addresses the
    // REPLY to key1 and to the delegator key3; only the ORGANIZER gets it.
    let delegated = format!("{KEY2_CAL}fo-delegated.ics");
    let delegator = "ATTENDEE;PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:key2@example.com\":mailto:key3@example.com";
    plant_attendee_copy(
        test,
        key2.id,
        "fo-delegated.ics",
        &copy(
            "za-fo-delegated",
            "key1@example.com",
            &[
                delegator,
                "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;DELEGATED-FROM=\"mailto:key3@example.com\":mailto:key2@example.com",
            ],
        ),
        STRONG,
        &pairs,
    )
    .await;
    let before1 = inbox_ids(test, key1.id).await;
    key2.client
        .request_with_headers(
            "PUT",
            &delegated,
            [CONTENT_TYPE],
            copy(
                "za-fo-delegated",
                "key1@example.com",
                &[
                    delegator,
                    "ATTENDEE;PARTSTAT=ACCEPTED;RSVP=TRUE;DELEGATED-FROM=\"mailto:key3@example.com\":mailto:key2@example.com",
                ],
            ),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        inbox_ids(test, key3.id).await,
        start3,
        "an item reached the delegator key3"
    );
    assert_eq!(new_inbox_items(test, key1.id, &before1).await.len(), 1);
    let body = key2
        .client
        .request("GET", &delegated, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert_eq!(
        schedule_statuses(&body, ICalendarProperty::Attendee, "key3@example.com"),
        ["5.3"]
    );

    // Review Focus 5: a reply to an organizer whose vault record has no
    // public key yet (setup pending).
    let admin = test.account("admin@example.com").clone();
    let key7 = admin
        .create_passwordless_user_account(
            "key7@example.com",
            STRONG,
            "Key Seven",
            &[],
            user_permissions(),
        )
        .await;
    za_setup_token(&admin, "key7@example.com").await;
    let key7_id = key7.id().document_id();
    let pending = format!("{KEY2_CAL}fo-pending.ics");
    plant_attendee_copy(
        test,
        key2.id,
        "fo-pending.ics",
        &copy("za-fo-pending", "key7@example.com", &[KEY2_NEEDS]),
        STRONG,
        &pairs,
    )
    .await;
    key2.client
        .request_with_headers(
            "PUT",
            &pending,
            [CONTENT_TYPE],
            copy("za-fo-pending", "key7@example.com", &[KEY2_ACCEPTED]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert!(
        inbox_ids(test, key7_id).await.is_empty(),
        "an item reached an account without a public key"
    );
    assert_eq!(
        organizer_statuses(&key2.client, &pending, "key7@example.com").await,
        ["5.3"]
    );

    // Cleanup.
    for name in ["fo-nopair", "fo-delegate", "fo-delegated", "fo-pending"] {
        key2.client
            .request_with_headers("DELETE", &format!("{KEY2_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.delete_items_since(test, &start1).await;
    admin.destroy_account(key7).await;
    test.wait_for_tasks().await;
}
```

- [ ] **Step 3: The partial-failure test**

In `tests/src/za/fanout.rs`, replace the import block with:

```rust
use super::{STRONG, dav_seal::raw_event};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        envelope_wrap, inbox_ids, item_attendees, item_method, item_sequence, mail_count,
        new_inbox_items, open_pending_item_as, schedule_statuses, wait_for_delivery, za_unlock,
    },
};
use ::registry::schema::{
    enums::StorageQuota,
    prelude::{ObjectType, Property},
    structs::CalendarScheduling,
};
use calcard::icalendar::{ICalendarMethod, ICalendarProperty};
use common::za::test_hooks;
use groupware::calendar::{
    ChangedBy, EVENT_NOTIFICATION_ZA_PENDING,
    seal::{TOKEN_LEN, TokenPairs, event_token_pairs},
};
use hyper::StatusCode;
use serde_json::json;
use types::{collection::Collection, field::CalendarEventField};
use vault::session::SessionKeys;
```

Append to `fanout.rs`:

```rust
/// How many stored events of `account_id` carry `uid`.
pub async fn uid_copies(test: &TestServer, account_id: u32, uid: &str) -> u64 {
    test.server
        .document_ids_matching(
            account_id,
            Collection::CalendarEvent,
            CalendarEventField::Uid,
            uid.as_bytes(),
        )
        .await
        .unwrap()
        .len()
}

/// Spec 5.3 step 5 and 5.4: a recipient's failure is that recipient's
/// `5.1`, never a failed PUT; over the recipient limit the PUT fails before
/// any delivery; a failed sender commit after the fan-out leaves its items.
pub async fn test_failures(test: &mut TestServer) {
    println!("Running zero-access fan-out failure tests...");
    let admin = test.account("admin@example.com").clone();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let key3 = Party::new(test, "key3@example.com");
    let key2_account = test.account("key2@example.com").id();
    let keys1 = za_unlock(test, key1.id, STRONG).await;
    let start2 = inbox_ids(test, key2.id).await;
    let start3 = inbox_ids(test, key3.id).await;

    // 7a. Quota: key2's quota cannot hold the item. key2 gets 5.1 and no
    // item; key3 is unaffected.
    admin
        .registry_update_object(
            ObjectType::Account,
            key2_account,
            json!({ Property::Quotas: { StorageQuota::MaxDiskQuota.as_str(): 1 } }),
        )
        .await;
    let quota = format!("{KEY1_CAL}fo-quota.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &quota,
            [CONTENT_TYPE],
            invite("za-fo-quota", START_A, &["key2@example.com", "key3@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(inbox_ids(test, key2.id).await, start2, "an item reached key2 past its quota");
    assert_eq!(new_inbox_items(test, key3.id, &start3).await.len(), 1);
    assert_eq!(attendee_statuses(&key1.client, &quota, "key2@example.com").await, ["5.1"]);
    assert_eq!(attendee_statuses(&key1.client, &quota, "key3@example.com").await, ["1.2"]);
    admin
        .registry_update_object(ObjectType::Account, key2_account, json!({ Property::Quotas: {} }))
        .await;

    // 7b. Inbound size: a message at or over `itip_inbound_max_ical_size`.
    admin
        .registry_update_setting(
            CalendarScheduling {
                itip_max_size: 64,
                ..Default::default()
            },
            &[Property::ItipMaxSize],
        )
        .await;
    admin.reload_settings().await;
    let size = format!("{KEY1_CAL}fo-size.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &size,
            [CONTENT_TYPE],
            invite("za-fo-size", START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(inbox_ids(test, key2.id).await, start2, "an oversized item reached key2");
    assert_eq!(attendee_statuses(&key1.client, &size, "key2@example.com").await, ["5.1"]);
    admin
        .registry_update_setting(CalendarScheduling::default(), &[Property::ItipMaxSize])
        .await;
    admin.reload_settings().await;

    // 7c. Max recipients (spec 5.1): the PUT fails with the precondition
    // before any delivery, and nothing is stored.
    admin
        .registry_update_setting(
            CalendarScheduling {
                max_recipients: 2,
                ..Default::default()
            },
            &[Property::MaxRecipients],
        )
        .await;
    admin.reload_settings().await;
    let before3 = inbox_ids(test, key3.id).await;
    let response = key1
        .client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}fo-many.ics"),
            [CONTENT_TYPE],
            invite(
                "za-fo-many",
                START_A,
                &["key2@example.com", "key3@example.com", "outsider@remote.example"],
            ),
        )
        .await
        .with_status(StatusCode::PRECONDITION_FAILED);
    assert!(
        response.expect_body().contains("max-attendees-per-instance"),
        "{}",
        response.expect_body()
    );
    assert_eq!(inbox_ids(test, key2.id).await, start2);
    assert_eq!(inbox_ids(test, key3.id).await, before3);
    assert_eq!(uid_copies(test, key1.id, "za-fo-many").await, 0, "nothing was stored");
    admin
        .registry_update_setting(CalendarScheduling::default(), &[Property::MaxRecipients])
        .await;
    admin.reload_settings().await;

    // 8. The sender's commit fails after the fan-out wrote key2's item: 500.
    // The client resends and gets 201; key2 holds two items with an equal
    // SEQUENCE and different tokens, and key1's bundle holds the second.
    let commit = format!("{KEY1_CAL}fo-commit.ics");
    let body = invite("za-fo-commit", START_A, &["key2@example.com"]);
    test_hooks::set_commit_fail_once(Some(key1.id));
    key1.client
        .request_with_headers("PUT", &commit, [CONTENT_TYPE], body.clone())
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    test_hooks::set_commit_fail_once(None);
    assert_eq!(
        new_inbox_items(test, key2.id, &start2).await.len(),
        1,
        "the item was written before the failed commit"
    );
    assert_eq!(uid_copies(test, key1.id, "za-fo-commit").await, 0, "no event was stored");
    key1.client
        .request_with_headers("PUT", &commit, [CONTENT_TYPE], body)
        .await
        .with_status(StatusCode::CREATED);
    let items = new_inbox_items(test, key2.id, &start2).await;
    assert_eq!(items.len(), 2);
    let (first, first_carriers) = open_pending_item_as(test, key2.id, STRONG, items[0].0).await;
    let (second, second_carriers) = open_pending_item_as(test, key2.id, STRONG, items[1].0).await;
    assert_eq!(item_sequence(&first), item_sequence(&second), "an equal SEQUENCE");
    let first_token = pair_token(first_carriers.pair.as_ref(), "key2@example.com");
    let second_token = pair_token(second_carriers.pair.as_ref(), "key2@example.com");
    assert_ne!(first_token, second_token, "the retry hands out a fresh token");
    assert_eq!(
        stored_pairs(test, &keys1, key1.id, "default/fo-commit.ics")
            .await
            .unwrap()
            .get("key2@example.com"),
        Some(&second_token)
    );

    // Cleanup.
    for name in ["fo-quota", "fo-size", "fo-commit"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key2.delete_items_since(test, &start2).await;
    key3.delete_items_since(test, &start3).await;
}
```

In `tests/src/za/mod.rs`, add `pub mod reply;` after `pub mod registry;`, and right after `fanout::test(&mut test).await;`:

```rust
    fanout::test_failures(&mut test).await;
    reply::test(&mut test).await;
```

- [ ] **Step 4: Run green**

Run the za suite. Expected: PASS.

Values from code reading, with their fallbacks:
- The quota update invalidates key2's account cache (`quota_changed`, `crates/common/src/cache/invalidate.rs:40-57`). If key2 still gets `1.2` in 7a, stop and report.
- `reload_settings` makes the new `itipMaxSize` and `maxRecipients` visible to the next request (`core.groupware`, `crates/common/src/config/groupware.rs:136-137`). If 7b or 7c sees the old value, stop and report.
- The DELEGATED-TO and DELEGATED-FROM parameters parse as `Email::from_uri` expects (`crates/groupware/src/scheduling/snapshot.rs`). If 4(a) yields no REQUEST to key3 at all, its status reads `[]` instead of `["5.3"]`; record a ruling and keep the item assertions, which are the rule under test.

- [ ] **Step 5: Red runs (M26, M27)**

One at a time; revert each with `git checkout -- crates/groupware/src/calendar/fanout.rs` and confirm `git status --short crates/` prints nothing. Write each failure line into the task report.
1. **M26, the ORGANIZER-only rule for REPLY.** In `route`, change `Some(ICalendarMethod::Reply) if to_organizer =>` to `Some(ICalendarMethod::Reply) =>`. Run the za suite. Expected: FAIL in `reply.rs` 4(b), `an item reached the delegator key3`.
2. **M27, the quota check.** In `write_item_inner`, delete the `if server.has_available_quota(..).await.is_err() { return Ok(false); }` block. Run the za suite. Expected: FAIL in `fanout.rs` 7a, `an item reached key2 past its quota`.

- [ ] **Step 6: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p tests -- --check
git add tests/src/utils/za.rs tests/src/za/reply.rs tests/src/za/fanout.rs tests/src/za/mod.rs
git commit -m "Fan-out tests: replies, delegation and partial failures"
```

---

### Task 5: The DELETE paths

**Files:**
- Modify: `crates/groupware/src/calendar/storage.rs:11-17` (imports), `:324-374` (`delete_with_events`), `:400-496` (the event `delete` and `delete_all`)
- Modify: `crates/dav/src/calendar/delete.rs:7-35` (imports), `:78-82` (`send_itip`), `:146-161`, `:231-241` (the two calls), `:244` (before the commit)
- Modify: `tests/src/za/fanout.rs` (new `test_task_queue`; the DELETE steps in `test`), `tests/src/za/reply.rs` (the decline), `tests/src/za/gating.rs:569-571` (comment), `tests/src/webdav/za_variants.rs` (`scheduling`: the CANCEL), `tests/src/za/mod.rs`

**Interfaces:**
- Consumes: Task 2's `ZaFanOut::za_fan_out`, `ZaDeleteMessage`, `unseal_event_pairs`; Task 3's helpers and `fanout.rs` items; `ZeroAccessGate::za_session_keys` (`crates/dav/src/common/za.rs:59-80`).
- Produces:
  - `DestroyArchive<Archive<&ArchivedCalendarEvent>>::{delete_za, delete_all_za}` returning `trc::Result<Option<ZaDeleteMessage>>`, and `DestroyArchive<Archive<&ArchivedCalendar>>::delete_with_events_za` returning `trc::Result<Vec<ZaDeleteMessage>>`, each with `za_keys: Option<&SessionKeys>` before `batch`.
  - Upstream's `delete`, `delete_all`, `delete_with_events` keep their signatures and delegate with `None`.
  - `fanout::test_task_queue`.

Background:
- Spec 5.5. `delete.rs` drops `!account_info.account().is_key_account()` from `send_itip` and keeps the other preconditions.
- `DestroyArchive::delete_all` builds the CANCEL only for an event with a schedule tag that ends in the future, and builds it from the stored tree. A key account's stored tree has sealed ATTENDEE properties, so the sealed sibling unseals first with the request's session keys (present on every DAV request of a key account, release 1 section 9). From the attendee's copy, `itip_cancel(.., true)` yields upstream's declined REPLY, which goes through the same path.
- For a key account the message comes back with the stored pairs and the unsealed tree instead of being queued. The handler fans each one out after every check and immediately before its commit (5.4). A collection DELETE returns one message per scheduled event, and each fans out with its own event's pairs.
- Callers of the upstream signatures (`None`, unchanged behaviour):
  - `crates/dav/src/calendar/copy_move.rs:569`, `:703`, `:747`, `:932`, `:1022` (all `send_itip = false`);
  - `crates/jmap/src/calendar_event/set.rs:667` (`itip_status.is_send()`, false for `SendSealed`) and `crates/jmap/src/calendar/set.rs:349` (`false`); JMAP is refused for key accounts anyway;
  - `tests/src/za/setup.rs:571` (`false`).
- Account destruction calls none of these. `crates/services/src/task_manager/destroy_account.rs` removes account-keyed data by range, pending items included (spec 10), and sends no CANCEL, as upstream.
- The task-queue test holds the consumer across a key organizer's PUT and DELETE and reads the queue while it is held (spec 11). The control, a plain organizer's PUT under the same hold, does queue a `CalendarItipMessage`. After release, the plain organizer's invitation reaches key2 as mail only (test 6; spec 5.2, "Plain organizers").

- [ ] **Step 1: The tests**

Append to `tests/src/za/fanout.rs`:

```rust
const PLAIN_CAL: &str = "/dav/cal/plain%40example.com/fo-queue/";

/// A plain organizer's invitation to key2 (spec 5.2, "Plain organizers").
const PLAIN_INVITE: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\n",
    "UID:za-fo-plain\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990305T100000Z\r\n",
    "DURATION:PT1H\r\nSEQUENCE:0\r\nSUMMARY:fo-plain-title\r\n",
    "ORGANIZER:mailto:plain@example.com\r\n",
    "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com\r\n",
    "END:VEVENT\r\nEND:VCALENDAR\r\n"
);

/// Spec 11, task queue: with the consumer held, a key organizer's PUT and
/// DELETE queue no `CalendarItipMessage` task (they fan out instead); a
/// plain organizer's PUT does (control), and its invitation reaches key2 as
/// mail only (test 6).
pub async fn test_task_queue(test: &mut TestServer) {
    println!("Running zero-access task queue tests...");
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let plain = test.account("plain@example.com").clone();
    let plain_id = plain.id().document_id();
    let plain_client =
        DummyWebDavClient::new(plain_id, plain.name(), plain.secret(), plain.name());
    let start2 = inbox_ids(test, key2.id).await;

    let queue = format!("{KEY1_CAL}fo-queue.ics");
    let held = hold_task_consumer(test).await;
    key1.client
        .request_with_headers(
            "PUT",
            &queue,
            [CONTENT_TYPE],
            invite("za-fo-queue", START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    key1.client
        .request("DELETE", &queue, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        itip_tasks_for(test, key1.id).await,
        0,
        "a CalendarItipMessage task was queued for the key organizer"
    );
    drop(held);
    let methods: Vec<_> = new_inbox_items(test, key2.id, &start2)
        .await
        .iter()
        .map(|(_, item)| item_method(&item.event))
        .collect();
    assert_eq!(
        methods,
        [Some(ICalendarMethod::Request), Some(ICalendarMethod::Cancel)],
        "the PUT and the DELETE fanned out"
    );

    // Control and test 6: a plain organizer inviting key2 takes upstream's
    // path, a task and then mail.
    plain_client
        .request("MKCALENDAR", PLAIN_CAL, MKCALENDAR)
        .await
        .with_status(StatusCode::CREATED);
    let key2_mail = mail_count(test, key2.id).await;
    let before2 = inbox_ids(test, key2.id).await;
    let held = hold_task_consumer(test).await;
    plain_client
        .request_with_headers(
            "PUT",
            &format!("{PLAIN_CAL}fo-plain.ics"),
            [CONTENT_TYPE],
            PLAIN_INVITE,
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(
        itip_tasks_for(test, plain_id).await,
        1,
        "the plain organizer's PUT queued its task (control)"
    );
    drop(held);
    wait_for_delivery(test).await;
    assert_eq!(mail_count(test, key2.id).await, key2_mail + 1, "the mail reached key2");
    assert_eq!(inbox_ids(test, key2.id).await, before2, "nothing in key2's inbox");
    assert_eq!(uid_copies(test, key2.id, "za-fo-plain").await, 0, "nothing in key2's calendar");

    plain_client
        .request_with_headers("DELETE", &format!("{PLAIN_CAL}fo-plain.ics"), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    plain_client
        .request("DELETE", PLAIN_CAL, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    key2.delete_items_since(test, &start2).await;
    wait_for_delivery(test).await;
}
```

Add `hold_task_consumer` and `itip_tasks_for` to the `crate::utils::za` import of `fanout.rs`.

In `fanout::test`, after `assert_eq!(pairs.get("key3@example.com"), Some(&key3_token), "a kept attendee keeps its token");`, insert:

```rust
    // 2 (continued). key1 DELETEs the event: each internal attendee gets a
    // CANCEL built from the unsealed stored tree, with its pair (spec 5.5).
    let before2 = inbox_ids(test, key2.id).await;
    let before3 = inbox_ids(test, key3.id).await;
    key1.client
        .request("DELETE", &mixed, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key3.id, &before3).await;
    assert_eq!(items.len(), 1, "key3 gets one CANCEL");
    let (tree, carriers) = open_pending_item_as(test, key3.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Cancel));
    assert_eq!(pair_token(carriers.pair.as_ref(), "key3@example.com"), key3_token);
    assert_eq!(new_inbox_items(test, key2.id, &before2).await.len(), 1);

    // A collection DELETE cancels each scheduled event in it.
    let collection = "/dav/cal/key1%40example.com/fo-collection/";
    key1.client
        .request("MKCALENDAR", collection, MKCALENDAR)
        .await
        .with_status(StatusCode::CREATED);
    for uid in ["za-fo-coll-1", "za-fo-coll-2"] {
        key1.client
            .request_with_headers(
                "PUT",
                &format!("{collection}{uid}.ics"),
                [CONTENT_TYPE],
                invite(uid, START_A, &["key2@example.com"]),
            )
            .await
            .with_status(StatusCode::CREATED);
    }
    let before2 = inbox_ids(test, key2.id).await;
    key1.client
        .request("DELETE", collection, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 2, "one CANCEL per scheduled event");
    for (_, item) in &items {
        assert_eq!(item_method(&item.event), Some(ICalendarMethod::Cancel));
    }
```

and change the cleanup loop's list to `["fo-dup", "fo-own", "fo-fresh", "fo-gone"]` (the event `fo-mixed` is gone).

In `tests/src/za/reply.rs`, replace the block from `// Removed here without a reply;` through its DELETE's `.with_status(StatusCode::NO_CONTENT);` with:

```rust
    // The attendee's DELETE of its copy yields upstream's declined REPLY,
    // delivered the same way and with the same token (spec 5.5).
    let before1 = inbox_ids(test, key1.id).await;
    key2.client
        .request("DELETE", &reply, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key1.id, &before1).await;
    assert_eq!(items.len(), 1, "one declined REPLY");
    let (tree, carriers) = open_pending_item_as(test, key1.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Reply));
    assert_eq!(item_partstat(&tree, "key2@example.com").as_deref(), Some("DECLINED"));
    assert_eq!(carriers.reply_token.as_deref(), Some(&TOKEN));
```

In `tests/src/za/gating.rs`, replace the comment at `:569-571` (`// The DELETE gate itself (`send_itip` off for key accounts): a legacy` and its two following lines) with:

```rust
    // A legacy plaintext event with a schedule tag and a visible attendee:
    // a key account's DELETE builds its CANCEL (invitations spec 5.5), and
    // the fan-out does not attempt plain, a non-key account, so no mail
    // leaves.
```

In `tests/src/webdav/za_variants.rs`, add `item_method` to the `crate::utils::za` import and `ICalendarMethod` to the calcard import; in `scheduling`, replace john's `DELETE` with `[("schedule-reply", "F")]` and the two lines after it with:

```rust
    // john's DELETE cancels: a second pending item, a CANCEL (METHOD is
    // visible on the sealed root), and still no mail (spec 5.5).
    john.request("DELETE", "/dav/cal/john@example.com/default/s.ics", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    wait_for_delivery(test).await;
    assert_eq!(mail_count(test, jane.account_id).await, jane_mail);
    let items = new_inbox_items(test, jane.account_id, &jane_items).await;
    assert_eq!(items.len(), 2, "jane's inbox holds the REQUEST and the CANCEL");
    assert_eq!(item_method(&items[1].1.event), Some(ICalendarMethod::Cancel));
    assert_eq!(envelope_wrap(&items[1].1.event), Some(2));
```

In `tests/src/za/mod.rs`, add `fanout::test_task_queue(&mut test).await;` right before `fanout::test(&mut test).await;`.

- [ ] **Step 2: Run to see them fail**

Run the za suite.
Expected: FAIL in `fanout.rs` `test_task_queue` at the `methods` assertion: `left: [Some(Request)]`, `right: [Some(Request), Some(Cancel)]`. The DELETE still skips key accounts.

- [ ] **Step 3: The sealed `delete*` siblings**

In `crates/groupware/src/calendar/storage.rs`, extend the `crate::calendar` import (`:12-15`) with `fanout::ZaDeleteMessage` and `seal::{seal_error, unseal_event_pairs}`, and add `use vault::session::SessionKeys;`.

Replace `delete_with_events` (`:324-364`) with:

```rust
    #[allow(clippy::too_many_arguments)]
    pub async fn delete_with_events(
        self,
        server: &Server,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        children_ids: Vec<u32>,
        delete_path: Option<String>,
        send_itip: bool,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        self.delete_with_events_za(
            server,
            account_info,
            account_id,
            document_id,
            children_ids,
            delete_path,
            send_itip,
            None,
            batch,
        )
        .await
        .map(|_| ())
    }

    /// `delete_with_events`. With `za_keys` (a key account, invitations
    /// spec 5.5) each event's CANCEL is built from its unsealed tree and
    /// returned for the fan-out instead of queued.
    #[allow(clippy::too_many_arguments)]
    pub async fn delete_with_events_za(
        self,
        server: &Server,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        children_ids: Vec<u32>,
        delete_path: Option<String>,
        send_itip: bool,
        za_keys: Option<&SessionKeys>,
        batch: &mut BatchBuilder,
    ) -> trc::Result<Vec<ZaDeleteMessage>> {
        // Process deletions
        let calendar_id = document_id;
        let mut za_messages = Vec::new();
        for document_id in children_ids {
            if let Some(event_) = server
                .store()
                .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                    account_id,
                    Collection::CalendarEvent,
                    document_id,
                ))
                .await?
            {
                za_messages.extend(
                    DestroyArchive(
                        event_
                            .to_unarchived::<CalendarEvent>()
                            .caused_by(trc::location!())?,
                    )
                    .delete_za(
                        account_info,
                        account_id,
                        document_id,
                        calendar_id,
                        None,
                        send_itip,
                        za_keys,
                        batch,
                    )?,
                );
            }
        }

        self.delete(
            account_info.account_tenant_ids(),
            account_id,
            document_id,
            delete_path,
            batch,
        )?;
        Ok(za_messages)
    }
```

Replace the event `delete` and `delete_all` (`:400-496`) with:

```rust
impl DestroyArchive<Archive<&ArchivedCalendarEvent>> {
    #[allow(clippy::too_many_arguments)]
    pub fn delete(
        self,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        calendar_id: u32,
        delete_path: Option<String>,
        send_itip: bool,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        self.delete_za(
            account_info,
            account_id,
            document_id,
            calendar_id,
            delete_path,
            send_itip,
            None,
            batch,
        )
        .map(|_| ())
    }

    /// `delete`. With `za_keys`, the CANCEL `delete_all_za` builds is
    /// returned instead of queued.
    #[allow(clippy::too_many_arguments)]
    pub fn delete_za(
        self,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        calendar_id: u32,
        delete_path: Option<String>,
        send_itip: bool,
        za_keys: Option<&SessionKeys>,
        batch: &mut BatchBuilder,
    ) -> trc::Result<Option<ZaDeleteMessage>> {
        let mut za_message = None;
        if let Some(delete_idx) = self
            .0
            .inner
            .names
            .iter()
            .position(|name| name.parent_id == calendar_id)
        {
            if self.0.inner.names.len() > 1 {
                // Unlink calendar id from event
                let event = self.0;
                let mut new_event = event
                    .deserialize::<CalendarEvent>()
                    .caused_by(trc::location!())?;
                new_event.names.swap_remove(delete_idx);
                batch
                    .with_account_id(account_id)
                    .with_collection(Collection::CalendarEvent)
                    .with_document(document_id)
                    .custom(
                        ObjectIndexBuilder::new()
                            .with_changed_by(account_info.account_tenant_ids())
                            .with_current(event)
                            .with_changes(new_event),
                    )
                    .caused_by(trc::location!())?;
            } else {
                za_message = self.delete_all_za(
                    account_info,
                    account_id,
                    document_id,
                    send_itip,
                    za_keys,
                    batch,
                )?;
            }

            if let Some(delete_path) = delete_path {
                batch.log_vanished_item(VanishedCollection::Calendar, delete_path);
            }

            batch.commit_point();
        }

        Ok(za_message)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn delete_all(
        self,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        send_itip: bool,
        batch: &mut BatchBuilder,
    ) -> trc::Result<()> {
        self.delete_all_za(account_info, account_id, document_id, send_itip, None, batch)
            .map(|_| ())
    }

    /// `delete_all`. With `za_keys` (a key account, invitations spec 5.5)
    /// the stored tree is unsealed before `itip_cancel`, and the CANCEL (or
    /// an attendee's declined REPLY) is returned with the stored pairs for
    /// the fan-out instead of queued.
    #[allow(clippy::too_many_arguments)]
    pub fn delete_all_za(
        self,
        account_info: &AccountInfo,
        account_id: u32,
        document_id: u32,
        send_itip: bool,
        za_keys: Option<&SessionKeys>,
        batch: &mut BatchBuilder,
    ) -> trc::Result<Option<ZaDeleteMessage>> {
        let event = self.0;
        let mut za_message = None;
        // Delete event
        batch
            .with_account_id(account_id)
            .with_collection(Collection::CalendarEvent)
            .with_document(document_id);

        // Remove next alarm if it exists
        let now = now() as i64;
        if let Some(next_alarm) = event.inner.data.next_alarm(now, Tz::Floating) {
            next_alarm.delete_task(batch);
        }

        // Scheduling
        if send_itip
            && event.inner.schedule_tag.is_some()
            && event.inner.data.event_range_end() > now
        {
            let mut event = event
                .deserialize::<CalendarEvent>()
                .caused_by(trc::location!())?;
            let pairs = match za_keys {
                Some(keys) => unseal_event_pairs(&mut event, keys, account_id)
                    .map_err(|err| seal_error(err, account_id, document_id))?,
                None => None,
            };

            if let Ok(messages) = itip_cancel(&event.data.event, account_info.addresses(), true) {
                if za_keys.is_some() {
                    za_message = Some(ZaDeleteMessage {
                        message: messages,
                        pairs,
                        event: event.data.event,
                    });
                } else {
                    ItipMessages::new(vec![messages])
                        .queue(batch)
                        .caused_by(trc::location!())?;
                }
            }
        }

        batch
            .custom(
                ObjectIndexBuilder::<_, ()>::new()
                    .with_changed_by(account_info.account_tenant_ids())
                    .with_current(event),
            )
            .caused_by(trc::location!())?;

        Ok(za_message)
    }
}
```

- [ ] **Step 4: The DELETE handler**

In `crates/dav/src/calendar/delete.rs`:
- Add `za::ZeroAccessGate` to the `crate::common` import and `fanout::ZaFanOut` to the `groupware::calendar` import (`calendar::{Calendar, CalendarEvent, fanout::ZaFanOut}`).
- Replace `send_itip` (`:78-82`) with:

```rust
        let send_itip = self.core.groupware.itip_enabled
            && !headers.no_schedule_reply
            && !account_info.addresses().is_empty()
            && access_token.has_permission(Permission::CalendarSchedulingSend);
        // Invitations spec 5.5: a key account's CANCELs and declines are
        // built from the unsealed stored tree and go to the fan-out.
        let za_keys = self.za_session_keys(access_token, account_id).await?;
        let mut za_messages = Vec::new();
```

- Replace the collection call (`:146-161`) with:

```rust
            // Delete calendar and events
            za_messages = DestroyArchive(calendar)
                .delete_with_events_za(
                    self,
                    &account_info,
                    account_id,
                    document_id,
                    resources
                        .subtree(delete_path)
                        .filter(|r| !r.is_container())
                        .map(|r| r.document_id())
                        .collect::<Vec<_>>(),
                    resources.format_resource(delete_resource).into(),
                    send_itip,
                    za_keys.as_deref(),
                    &mut batch,
                )
                .await
                .caused_by(trc::location!())?;
```

- Replace the event call (`:231-241`) with:

```rust
            // Delete event
            za_messages.extend(
                DestroyArchive(event)
                    .delete_za(
                        &account_info,
                        account_id,
                        document_id,
                        calendar_id,
                        resources.format_resource(delete_resource).into(),
                        send_itip,
                        za_keys.as_deref(),
                        &mut batch,
                    )
                    .caused_by(trc::location!())?,
            );
```

- Before `self.commit_batch(batch)` (`:244`):

```rust
        if let Some(keys) = &za_keys {
            // Spec 5.4: the items first, immediately before the commit.
            for message in za_messages {
                self.za_fan_out(
                    &account_info,
                    keys,
                    vec![message.message],
                    message.pairs,
                    &message.event,
                )
                .await
                .caused_by(trc::location!())?;
            }
        }
```

- [ ] **Step 5: Run green**

Run the per-task compile check, the za suite, and the key-mode CalDAV suite (Task 3 Step 8's commands). Expected: `clean`; both PASS.

Fallback: if `gating::test_scheduling` fails at `a CANCEL left the key organizer`, a CANCEL to plain was queued as mail: the DELETE took `delete` (`None`) for a key account. Stop and report.

- [ ] **Step 6: Red run, the queue path (M23)**

In `crates/dav/src/calendar/update.rs`, create branch, replace

```rust
                            if itip_status.is_send_sealed() {
                                za_messages = Some(messages);
                            } else {
                                itip_messages = Some(ItipMessages::new(messages));
                            }
```

with `itip_messages = Some(ItipMessages::new(messages));`. Run the za suite.
Expected: FAIL in `fanout.rs` `test_task_queue`, `a CalendarItipMessage task was queued for the key organizer` (left 1, right 0). `test_task_queue` runs before every other module that sends a key organizer's invitation (`gating` would otherwise see the mail first).

Revert with `git checkout -- crates/dav/src/calendar/update.rs`; confirm `git status --short crates/` lists only this task's `storage.rs` and `delete.rs`. Write the failure line into the task report.

- [ ] **Step 7: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p groupware -- --check
cargo fmt -p dav -- --check
cargo fmt -p tests -- --check
git add crates/groupware/src/calendar/storage.rs crates/dav/src/calendar/delete.rs tests/src/za/fanout.rs tests/src/za/reply.rs tests/src/za/gating.rs tests/src/webdav/za_variants.rs tests/src/za/mod.rs
git commit -m "DELETE paths: a key account's CANCELs and declines fan out"
```

---

### Task 6: The creation lease, PUT side

**Files:**
- Create: `tests/src/za/uid_lease.rs`
- Modify: `tests/src/za/mod.rs` (`pub mod uid_lease;`; the call after `reply::test`)

**Interfaces:**
- Consumes: Task 1's `ZaUidLease::{acquire, release}`, `test_hooks::{set_put_pause, set_fan_out_pause, set_lease_secs_override}`; Task 3's `HookParked`, `za_uid_leases`, `new_inbox_items`, `inbox_ids`, `za_take_uid_lease`/`za_commit_create` (through PUT); `fanout.rs`'s `Party`, `invite`, `uid_copies`, `CONTENT_TYPE`, `NO_REPLY`, `KEY1_CAL`, `START_A`, `MKCALENDAR`; plan 8's `plant_sealed_event`.
- Produces: `uid_lease::test`. Plan 10 adds its drain-side cases (drain versus drain, drain versus PUT, the sweep) beside it.

Background:
- Spec 6.3 and 11, the PUT-side cases:
  - **(a) Stale snapshot.** `PUT_PAUSE` holds the PUT right after its resource fetch while the UID lands in key1's default calendar, written as a drain would write it (`plant_sealed_event`). The PUT takes the lease, re-checks current archives and answers `NoUidConflict`.
  - **(b) Expiry.** A one-second lease; `FAN_OUT_PAUSE` holds the PUT after it took the lease and checked the archives. The test waits past expiry, takes the lease itself with another fence, plants the UID and releases. The PUT's fenced commit then fails, it takes the lease again, finds the UID and answers `NoUidConflict`.
  - **(c) PUT versus PUT.** The brief names `PUT_PAUSE` for the held session, but a PUT held before it takes the lease leaves nothing for the lease to decide: the second session's commit lands first, and the held one finds it through the archive re-check alone, with or without a lease. So the held session waits at `FAN_OUT_PAUSE`, after taking the lease. The second session then waits on the lease (up to 20 × 50 ms), and the first is released 200 ms in. The second then finds the first's copy and answers `NoUidConflict`. Without the lease both would create.
  - **The busy lease.** Held by another holder for the whole retry window, the PUT answers 503.
  - **(d) A non-key account takes no lease (invariant 9).** Its PUT creates the UID while the test holds the lease for that account and UID, and the test's lease still releases. The lease range is empty at the end of the module.
- Paused and slow PUTs go through `HttpRequest::send_full` (5 s timeout); `DummyWebDavClient` gives up after 500 ms.
- The item a held PUT fanned out before losing its commit (case b) stays: spec 5.4 never rolls items back. When the drain (plan 10) imports it, it carries a token key1's planted event does not hold, so key2's reply to it would be dropped as a forgery. The test asserts the item and deletes it.
- `ZA_UID_LEASE_SECS` is read once per process, so case (b) uses the test-mode override for the one-second lease (Task 1).

- [ ] **Step 1: Write the tests**

Create `tests/src/za/uid_lease.rs` with the SPDX header and:

```rust
//! Invitations spec 6.3 and 5.1: the UID creation lease in a key account's
//! PUT create, its archive-based uniqueness re-check, its fenced commit and
//! its expiry; a non-key account takes no lease.

use super::{
    STRONG,
    fanout::{CONTENT_TYPE, KEY1_CAL, MKCALENDAR, NO_REPLY, Party, START_A, invite, uid_copies},
};
use crate::utils::{
    http::HttpRequest,
    server::TestServer,
    za::{HookParked, inbox_ids, new_inbox_items, plant_sealed_event, za_uid_leases},
};
use common::za::{lease::ZaUidLease, test_hooks};
use groupware::calendar::seal::TokenPairs;
use hyper::Method;
use std::{future::Future, time::Duration};

/// A PUT with its own client and a 5-second timeout; resolves to the status.
fn put(
    name: &'static str,
    secret: &'static str,
    path: String,
    body: String,
) -> impl Future<Output = u16> + Send + 'static {
    async move {
        HttpRequest::with_credentials(8899, name, secret)
            .send_full(
                Method::PUT,
                &path,
                Some(body.into_bytes()),
                Some("text/calendar; charset=utf-8"),
            )
            .await
            .status
            .as_u16()
    }
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access creation lease tests...");
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let start2 = inbox_ids(test, key2.id).await;
    assert_eq!(za_uid_leases(test).await, 0, "a lease outlived an earlier module");

    // (a) Stale snapshot.
    let uid = "za-lease-stale";
    let parked = HookParked::start(
        key1.id,
        test_hooks::set_put_pause,
        put(key1.name, STRONG, format!("{KEY1_CAL}fo-stale.ics"), invite(uid, START_A, &[])),
    )
    .await;
    plant_sealed_event(
        test,
        key1.id,
        "default",
        "fo-stale-drained.ics",
        &invite(uid, START_A, &[]),
        STRONG,
        &TokenPairs::new(),
    )
    .await;
    assert_eq!(parked.finish().await, 412, "the stale snapshot let a second copy in");
    assert_eq!(uid_copies(test, key1.id, uid).await, 1);

    // (b) Expiry: the held PUT's commit is fenced out.
    let uid = "za-lease-expiry";
    test_hooks::set_lease_secs_override(Some(1));
    let parked = HookParked::start(
        key2.id,
        test_hooks::set_fan_out_pause,
        put(
            key1.name,
            STRONG,
            format!("{KEY1_CAL}fo-expiry.ics"),
            invite(uid, START_A, &["key2@example.com"]),
        ),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let lease = ZaUidLease::acquire(test.server.store(), key1.id, uid)
        .await
        .unwrap()
        .expect("an expired lease is taken over");
    plant_sealed_event(
        test,
        key1.id,
        "default",
        "fo-expiry-other.ics",
        &invite(uid, START_A, &[]),
        STRONG,
        &TokenPairs::new(),
    )
    .await;
    assert!(lease.release(test.server.store()).await, "the test's lease releases");
    test_hooks::set_lease_secs_override(None);
    assert_eq!(parked.finish().await, 412, "an expired holder committed");
    assert_eq!(uid_copies(test, key1.id, uid).await, 1);
    assert_eq!(
        new_inbox_items(test, key2.id, &start2).await.len(),
        1,
        "the held PUT's item was written before its commit (spec 5.4)"
    );

    // (c) PUT versus PUT, one session holding the lease in its fan-out.
    let uid = "za-lease-race";
    let parked = HookParked::start(
        key2.id,
        test_hooks::set_fan_out_pause,
        put(
            key1.name,
            STRONG,
            format!("{KEY1_CAL}fo-race-a.ics"),
            invite(uid, START_A, &["key2@example.com"]),
        ),
    )
    .await;
    let second = tokio::spawn(put(
        key1.name,
        STRONG,
        format!("{KEY1_CAL}fo-race-b.ics"),
        invite(uid, START_A, &["key2@example.com"]),
    ));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(parked.finish().await, 201);
    assert_eq!(second.await.unwrap(), 412, "two creators of one UID both created");
    assert_eq!(uid_copies(test, key1.id, uid).await, 1);

    // The busy lease: held for the whole retry window, the PUT answers 503.
    let uid = "za-lease-busy";
    let busy = format!("{KEY1_CAL}fo-busy.ics");
    let lease = ZaUidLease::acquire(test.server.store(), key1.id, uid)
        .await
        .unwrap()
        .expect("a free lease");
    assert_eq!(put(key1.name, STRONG, busy.clone(), invite(uid, START_A, &[])).await, 503);
    assert!(lease.release(test.server.store()).await);
    assert_eq!(put(key1.name, STRONG, busy, invite(uid, START_A, &[])).await, 201);

    // (d) A non-key account takes no lease.
    let plain = test.account("plain@example.com").clone();
    let plain_id = plain.id().document_id();
    let plain_cal = "/dav/cal/plain%40example.com/fo-lease/";
    let plain_client = crate::utils::webdav::DummyWebDavClient::new(
        plain_id,
        plain.name(),
        plain.secret(),
        plain.name(),
    );
    plain_client
        .request("MKCALENDAR", plain_cal, MKCALENDAR)
        .await
        .with_status(hyper::StatusCode::CREATED);
    let uid = "za-lease-plain";
    let lease = ZaUidLease::acquire(test.server.store(), plain_id, uid)
        .await
        .unwrap()
        .expect("a free lease");
    let plain_event = invite(uid, START_A, &[]).replace("key1@example.com", "plain@example.com");
    assert_eq!(
        put(plain.name(), plain.secret(), format!("{plain_cal}fo-plain.ics"), plain_event).await,
        201,
        "the plain PUT waited on a lease"
    );
    assert!(
        lease.release(test.server.store()).await,
        "the plain PUT left the lease as it found it"
    );
    plain_client
        .request("DELETE", plain_cal, "")
        .await
        .with_status(hyper::StatusCode::NO_CONTENT);

    // Cleanup.
    for name in ["fo-stale-drained", "fo-expiry-other", "fo-race-a", "fo-busy"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(hyper::StatusCode::NO_CONTENT);
    }
    key2.delete_items_since(test, &start2).await;
    assert_eq!(za_uid_leases(test).await, 0, "every lease was released");
}
```

In `tests/src/za/mod.rs`, add `pub mod uid_lease;` after `pub mod tracing;`, and `uid_lease::test(&mut test).await;` right after `reply::test(&mut test).await;`.

`plant_sealed_event` writes plan 8's sealed form with `schedule_tag: None`; with empty pairs it carries no bundle.

- [ ] **Step 2: Run green**

Run the za suite. Expected: PASS. The module takes about four seconds of waits ((b)'s expiry, the busy lease's retries).

Values from code reading, with their fallbacks:
- A lost assertion on an absent key: `AssertValue::Hash` on a key the test released fails (`assert.rs:70`; RocksDB checks the stored value inside the transaction). If (b) answers 201, stop and report: the fence does not hold.
- `NoUidConflict` is 412 (`PRECONDITION_FAILED`, `crates/dav/src/calendar/mod.rs`). The lost-fence path answers 503 only when the re-check finds nothing. If (b) answers 503, the planted copy did not land in the default calendar; stop and report.
- Case (c) holds whatever the second session's timing: if it reaches the lease after the first released it, the re-check still finds the first's copy. If (c) answers 503 for the second session, the first session's commit took longer than the retry window; record the elapsed time and a ruling, and raise the 200 ms wait no further than the window allows.

- [ ] **Step 3: Red runs (M24, M25)**

One at a time; revert each with `git checkout -- <file>` and confirm `git status --short crates/` prints nothing. Write each failure line into the task report.
1. **M24, the lease in the PUT create branch.** In `crates/dav/src/calendar/update.rs`, replace `Some(za_take_uid_lease(self, &resources, account_id, parent.document_id(), &uid).await?)` with `{ za_assert_is_unique_uid(self, &resources, account_id, parent.document_id(), &uid).await?; None }`, and add `za_assert_is_unique_uid` to the `use super::{..}` line. The archive re-check stays; the lease and its fence go. Run the za suite. Expected: FAIL in `uid_lease.rs` (b), `an expired holder committed` (left 201, right 412). Case (c) fails the same way (two 201s) when (b) is skipped.
2. **M25, the snapshot re-check.** In `crates/dav/src/calendar/mod.rs`, `za_take_uid_lease`, replace the `za_assert_is_unique_uid(server, resources, account_id, calendar_id, uid).await` call with `assert_is_unique_uid(server, resources, account_id, calendar_id, Some(uid)).await`. Run the za suite. Expected: FAIL in `uid_lease.rs` (a), `the stale snapshot let a second copy in` (left 201, right 412).

- [ ] **Step 4: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p tests -- --check
git add tests/src/za/uid_lease.rs tests/src/za/mod.rs
git commit -m "Creation lease tests: stale snapshot, expiry, PUT versus PUT, non-key control"
```

---

### Task 7: The fan-out in the leak scan

**Files:**
- Modify: `tests/src/za/leak.rs`: imports (`:10-44`), `EXTRA_CANARIES` (`:62-84`), a constant after `SCHED_EVENT` (`:88-95`), `test` (the scheduling PUT and the pre-drain scan at `:800-817`, the item count at `:962`, the token check, the item deletes at `:1130`)

**Interfaces:**
- Consumes: Task 3's `hold_task_consumer`, `itip_tasks_for`, `inbox_ids`, `new_inbox_items`, `za_unlock`; `fanout.rs`'s `NO_REPLY`; plan 8's `token_hits`, `event_token_pairs`, `raw_event`.
- Produces: the leak scan covers a real fan-out: plaintext canaries in the organizer's event, the recipient's item and the task queue, and the fan-out's token. Plan 10's flow leak tests extend the same module.

Background:
- Spec 11, leak tests: a key organizer's invitation carries canary titles, locations, descriptions and attendee display names. The whole-store scan, every subspace including the scheduling inbox and the task queue, expects zero hits, and no token is in the clear except on a pending reply item.
- key1 organizes and invites key2, the module's scanned account. The scan walks every account's records, so key1's event is covered too.
- The consumer is held from just before the existing scheduling PUT through the existing pre-drain scan. A regression that queued a task for either invitation leaves it in the task queue at that scan, where the canary search and `itip_tasks_for` see it, instead of racing the worker.
- This scenario has no reply item, so the fan-out's token must have zero hits.

- [ ] **Step 1: Extend the module**

Imports: add `super::fanout::NO_REPLY`, `crate::utils::za::{hold_task_consumer, inbox_ids, itip_tasks_for, new_inbox_items, za_unlock}` and `groupware::calendar::seal::event_token_pairs`.

Append to `EXTRA_CANARIES`:

```rust
    "fanoutsum-canary",
    "fanoutloc-canary",
    "fanoutdesc-canary",
    "fanoutcn-canary",
```

After `SCHED_EVENT`:

```rust
/// A key organizer's invitation to the module's key account (plan 9's
/// fan-out): canaries in the title, location, description and the
/// attendee's display name.
const FANOUT_EVENT: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\n",
    "UID:leak-fanout\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990104T090000Z\r\n",
    "DTEND:20990104T100000Z\r\nSUMMARY:fanoutsum-canary\r\n",
    "LOCATION:fanoutloc-canary\r\nDESCRIPTION:fanoutdesc-canary\r\n",
    "ORGANIZER:mailto:key1@example.com\r\n",
    "ATTENDEE;CN=fanoutcn-canary;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com\r\n",
    "END:VEVENT\r\nEND:VCALENDAR\r\n"
);
```

In `test`, replace the block from the `sched.ics` PUT (`:800-808`) through the pre-drain scan assertion (`:812-817`) with:

```rust
    // Hold the consumer across both invitations and the scan: a regression
    // that queued either as a task leaves it in the task queue here, where
    // the scan and the task count see it.
    let key1_id = test.account("key1@example.com").id().document_id();
    let key1 = DummyWebDavClient::new(key1_id, "key1@example.com", STRONG, "key1@example.com");
    let key2_items = inbox_ids(test, id).await;
    let held = hold_task_consumer(test).await;
    client
        .request_with_headers(
            "PUT",
            "/dav/cal/key2%40example.com/leak/sched.ics",
            [CONTENT_TYPE],
            SCHED_EVENT,
        )
        .await
        .with_status(StatusCode::CREATED);
    // Plan 9: key1's invitation reaches key2 as a sealed pending item.
    key1.request_with_headers(
        "PUT",
        "/dav/cal/key1%40example.com/default/fo-leak.ics",
        [CONTENT_TYPE],
        FANOUT_EVENT,
    )
    .await
    .with_status(StatusCode::CREATED);
    let fan_out_items = new_inbox_items(test, id, &key2_items).await;
    assert_eq!(fan_out_items.len(), 1, "key1's invitation reached key2 once");
    let fan_out_item = fan_out_items[0].0;
    let pending = scan(test, id).await;
    assert!(
        pending.violations.is_empty(),
        "leaks found before the task queue drained:\n{}",
        pending.violations.join("\n")
    );
    assert_eq!(
        itip_tasks_for(test, key1_id).await + itip_tasks_for(test, id).await,
        0,
        "an invitation was queued as a task"
    );
    drop(held);
```

Replace `assert!(result.items >= 3, "{result:?}");` with:

```rust
    assert!(result.items >= 4, "{result:?}");
```

and update the comment above it to "Inbox items were decoded (the three planted ones and the fan-out's) and each carries its key envelope; the pending requests' pair bundles were seen before their envelopes."

After the `stray` assertion of the token check, add:

```rust
    // The fan-out's token for key2 is in the clear nowhere: its item
    // carries it in the sealed bundle, and no reply item exists here.
    let keys1 = za_unlock(test, key1_id, STRONG).await;
    let (organizer_event, _) = raw_event(test, key1_id, "default/fo-leak.ics").await;
    let fan_out_token = *event_token_pairs(&organizer_event, &keys1, key1_id)
        .unwrap()
        .expect("key1's bundle holds pairs")
        .get("key2@example.com")
        .expect("key2's pair");
    let fan_out_hits = token_hits(test, &[fan_out_token]).await;
    assert!(
        fan_out_hits.is_empty(),
        "the fan-out's token in the clear: {fan_out_hits:?}"
    );
```

Replace the final item-delete loop's list `[request_id, reply_id, drained_id]` with `[request_id, reply_id, drained_id, fan_out_item]`, and before that loop delete key1's event without a CANCEL:

```rust
    key1.request_with_headers(
        "DELETE",
        "/dav/cal/key1%40example.com/default/fo-leak.ics",
        [NO_REPLY],
        "",
    )
    .await
    .with_status(StatusCode::NO_CONTENT);
```

- [ ] **Step 2: Run green, plain and filesystem blob store**

Run the za suite, then the same with `BLOB_STORE=FileSystem`. Expected: both PASS; the summary line shows `4 items` or more.

Fallback: a stray canary hit names a record: that is a leak or a planted-data mistake. Stop and report with the hit.

- [ ] **Step 3: Format and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p tests -- --check
git add tests/src/za/leak.rs
git commit -m "Leak scan: a real fan-out, its pending item and its token"
```

---

### Task 8: Mutation runs, final verification and the outcome note

**Files:**
- Modify (temporarily, reverted after each run): the product files in the table below
- Create: `docs/superpowers/plans/2026-10-14-zero-access-plan9-outcome.md`

**Interfaces:**
- Consumes: every test from Tasks 1-7.
- Produces: the outcome note, which plan 10 reads before starting.

How a run works:
1. Apply exactly the edit in the table.
2. Run the named command.
3. Record the first failure as `file:line` plus the panic message, or "passed".
4. Revert with `git checkout -- <file>`. Confirm `git status --short crates/` is empty before the next mutation.

| Id | What it removes | Edit | Command | Expected first failure (from code reading) |
|----|-----------------|------|---------|-------------------------------------------|
| M22 | The recipient split | `crates/groupware/src/calendar/fanout.rs`, `resolve_inner`: everything after the `account_id == sender_id` check becomes `Ok(Recipient::Internal { account_id, public_key: [9u8; 32] })` | za suite | `fanout.rs` test 1, `plain's inbox received an item` |
| M23 | The queue path stays closed for key senders | `crates/dav/src/calendar/update.rs`, create branch: the `if itip_status.is_send_sealed() { .. } else { .. }` becomes `itip_messages = Some(ItipMessages::new(messages));` | za suite | `fanout.rs` `test_task_queue`, `a CalendarItipMessage task was queued for the key organizer` |
| M24 | The lease in the PUT create branch | `update.rs`: `Some(za_take_uid_lease(..).await?)` becomes `{ za_assert_is_unique_uid(..).await?; None }` (import added) | za suite | `uid_lease.rs` (b), `an expired holder committed` |
| M25 | The archive-based re-check | `crates/dav/src/calendar/mod.rs`, `za_take_uid_lease`: `za_assert_is_unique_uid(..)` becomes `assert_is_unique_uid(server, resources, account_id, calendar_id, Some(uid))` | za suite | `uid_lease.rs` (a), `the stale snapshot let a second copy in` |
| M26 | The ORGANIZER-only rule for REPLY | `fanout.rs`, `route`: `Some(ICalendarMethod::Reply) if to_organizer =>` becomes `Some(ICalendarMethod::Reply) =>` | za suite | `reply.rs` 4(b), `an item reached the delegator key3` |
| M27 | The quota check | `fanout.rs`, `write_item_inner`: delete the `has_available_quota` block | za suite | `fanout.rs` 7a, `an item reached key2 past its quota` |

- [ ] **Step 1: Run M22-M27**

Run them in order. Keep raw notes in the task report: the edit, the first failure, the revert confirmation.

If a mutation passes, decide which case applies:
- **(a) A request can still reach the removed check.** Write the missing assertion in the module that owns the check (under about forty lines), red-run it, and update the row.
- **(b) An earlier check stops every input.** Name that check and its line, and mark the row "layered behind <check>".

- [ ] **Step 2: Final verification**

Run in this order, never two at once:

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo test -p vault -p groupware
cargo test -p common za::lease
cargo test -p common auth::vault
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture
BLOB_STORE=FileSystem STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture
ZA_KEY_ACCOUNTS=1 STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture
cargo build --release -p stalwart --no-default-features --features rocks
cargo tree -p stalwart --no-default-features --features rocks -e features -i common | grep test_mode || echo "no test_mode in the product"
git status --short crates/   # must print nothing
```

Expected: all PASS. The release build adds no warning in `groupware`, `dav`, `services/src/task_manager/manager.rs` or `common/src/{za,auth/vault.rs}`. Upstream's `cal_itip` sub-test is timing-flaky; rerun once before treating a failure there as a regression.

- [ ] **Step 3: Write the outcome note**

Create `docs/superpowers/plans/2026-10-14-zero-access-plan9-outcome.md` with exactly these sections. Fill each one from the task reports.

````markdown
# Zero-access calendar: plan 9 outcome

Written <date> at the end of plan 9 (`2026-10-14-zero-access-9-fanout.md`).
Read it before plan 10.

## Where things stand

- Commits `<first>..<last>` on `release2-plan9-fanout`; the suites run in Task 8 Step 2 and their results (test counts, leak-scan summary line, build time, warning totals).
- What now exists:
  - `ItipSendStatus::SendSealed`; `KeyAccount` is gone;
  - the fan-out (`ZaFanOut`) on the PUT create and update paths and on event and collection DELETE, for organizers and attendees;
  - token maintenance: a union per message-building rewrite, the current attendees' pairs persisted;
  - SCHEDULE-STATUS `1.2` / `5.3` / `5.1` in the sender's sealed copy;
  - the UID creation lease in the PUT create branch, with the archive-based re-check and the fenced commit;
  - the test-mode hooks `FAN_OUT_PAUSE`, `PUT_PAUSE`, `COMMIT_FAIL_ONCE`, `TASK_CONSUMER_PAUSE` and the lease-duration override.
- What does not exist yet:
  - nothing drains: pending items stay pending and are read only by tests;
  - no lease sweep;
  - `calendar-auto-schedule` is still omitted from a key account's OPTIONS, and the schedule inbox and outbox URLs still answer 404 (spec 8, plan 10).

## Names settled (spec 13)

<copy the table "Names this plan settles" from the plan, with any change a task made>

## Rulings

- The resource-cache invalidation after an item: none. Upstream's ingest performs none; `fetch_dav_resources` applies the change log on every request on every node.
- The test-mode lease-duration override stands in for `ZA_UID_LEASE_SECS=1` (read once per process).
- Case (c) of the lease tests holds its first session at `FAN_OUT_PAUSE`, not `PUT_PAUSE`: a session held before it takes the lease leaves nothing for the lease to decide.
- Delegation case 4(b) (key2 as a delegate replying to its delegator) is the case the ORGANIZER-only rule decides; 4(a) alone would make "no item to any delegator" vacuous.
- The new za sub-modules run after `dav_seal::test_legacy`, before `tracing` and `gating`, so M22 and M23 fail in the tests that own them.
- Existing tests changed by `SendSealed`: `tracing` (four past-event traces), `gating::test_scheduling` (schedule tag present; the DELETE comments), `pairs` (the no-pair cases carry no attendee), the key-mode `scheduling` variant (store-level delivery, the CANCEL).
- None of upstream's `cal_scheduling` assertions runs unchanged in key mode before the drain; the variant asserts the organizer's schedule tag upstream asserts.
- Any other ruling from the task reports.

## Mutation runs (plan 9)

| Id | Check, `file:line` | Edit | First failure | Command |
|----|--------------------|------|---------------|---------|
<one row per M22-M27, from Task 8 Step 1>

## Facts for plan 10

- The lease, for the drain's import (spec 6.3, 6.4):
  - after a first UID lookup finds nothing, `ZaUidLease::acquire(server.store(), account_id, uid)`; `None` leaves the item pending and holds the cursor (spec 6.2);
  - look the UID up again under the lease; merge if it appeared;
  - `lease.assert_in(&mut batch)` ahead of the import and the item rewrite in the item's one batch;
  - `lease.release(server.store())` after the commit; a lost assertion aborts the pass;
  - `ZaUidLeaseGuard` releases on early returns;
  - the sweep iterates `lease_key_range()` and clears every key whose `lease_value_expiry` is at or below now; `za_uid_leases` (`tests/src/utils/za.rs`) counts them.
- The hooks (`common::za::test_hooks`): `FAN_OUT_PAUSE` (recipient-keyed), `PUT_PAUSE`, `COMMIT_FAIL_ONCE`, `TASK_CONSUMER_PAUSE` (global), `set_lease_secs_override`. The drain pause and the drain skip go into the same module; the `dav`, `groupware` and `services` `test_mode` features already forward `common/test_mode`.
- The delegation fan-out (spec 6.4): `server.za_fan_out(&organizer_info, &keys, messages, Some(current_pairs), &merged_tree)` with the messages of `organizer_request_full` restricted to the new attendees. It hands out the delegates' tokens and returns the pairs the organizer's bundle keeps; `za_set_schedule_status` writes the statuses before the seal.
- `open_pending_item` refuses a bundle that does not hold exactly one pair (plan 8's fix wave), so the drain need not re-check the count.
- `Server::za_private_key`: a store error is neither traced nor recorded (plan 8); the drain traces it and a later call retries.
- `token_hits` (`tests/src/za/leak.rs`) does not decode MIME bodies; mail flows that carry tokens need that first.
- Test helpers for the drain's tests: `open_pending_item_as`, `inbox_items`, `new_inbox_items`, `plant_attendee_copy`, `hold_task_consumer`, `itip_tasks_for`, `HookParked`.
- An item whose sender's commit failed (`COMMIT_FAIL_ONCE`, lease case b) stays pending; a retried create delivers a second item with an equal SEQUENCE and a fresh token, which the drain must adopt (spec 5.4, 6.4).

## Deferred findings

<findings from task and branch reviews that were not fixed, each with its reason>
````

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/plans/2026-10-14-zero-access-plan9-outcome.md
git commit -m "Record plan 9 mutation runs and rulings"
```
