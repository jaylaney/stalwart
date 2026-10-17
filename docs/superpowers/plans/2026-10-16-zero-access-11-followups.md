# Zero-access plan 11: release 2 follow-ups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the items release 2 left open: the spec decisions Jay made on 2026-10-16 (sections 5.1 and 5.4), the lease fence on the PUT create's fan-out, four robustness rules in the drain (a permanent private-key deferral, a bound on a poison-pill item, a per-pass cap, a deferral on early store errors), the deferred tests of the plan 10 reviews, and the comment nits, with the outcome note and mutation runs that record them.

**Architecture:** Release 2 (invitations between key accounts) shipped in plans 8 (PR #34), 9 (PR #35) and 10 (PR #36). Nothing here adds a feature; every task hardens or documents what shipped:
- **Spec revision** (Task 1): sections 5.1, 5.4, 6.2, 6.5, 10, 11 and 13 of the invitations spec say what the code does after this plan; the manual checklist gains the SEQUENCE-stripping-client item.
- **The PUT create's fan-out fence** (Task 2, `crates/dav/src/calendar/update.rs`, `crates/dav/src/calendar/mod.rs`): the create passes its UID lease to `za_fan_out` as the fence plan 10's fix wave added (`ca8d2e2b`), so a holder whose lease was taken over delivers no attendee item after the successor's. A lost fence inside the fan-out takes the same path as a lost commit fence: trace, abandon, retake, `NoUidConflict` or 503.
- **Drain robustness** (Task 3, `crates/groupware/src/calendar/drain.rs`, `crates/vault/src/session.rs`, `crates/common/src/za/test_hooks.rs`): per-session counters and outcomes only, never a stored struct. A recorded private-key failure defers the session's drain permanently. An item that fails three times is skipped for the session, and later items of its UID wait with it. A pass handles at most 32 items and the next request continues without a backoff. A store error while enumerating sets the same deferral as one while applying.
- **Deferred tests** (Task 4): the plan 10 reviews' uncovered inputs, each pinned where the code lives.
- **Comment nits** (Task 5) and **the outcome note, mutation runs and CLAUDE.md** (Task 6).

The fork diff stays narrow: edits inside the fork's own modules (`drain.rs`, `session.rs`, `test_hooks.rs`, `lease.rs`), one argument change at the PUT create's fan-out call, and one helper extracted in `crates/dav/src/calendar/mod.rs`. No upstream file changes except the comment at `crates/store/src/dispatch/lookup.rs:427` (fork code already).

**Tech Stack:**
- Rust 2024 workspace (Stalwart 0.16.25 fork); `tokio` (`sync`), `ahash` in groupware; `std::collections::HashMap` in vault (no new dependency)
- calcard 0.3, rkyv 0.8.18
- the `tests` crate harness (`TestServer`, `DummyWebDavClient`, `HttpRequest`, `HookParked`)

**Spec:** `docs/superpowers/specs/2026-10-11-zero-access-invitations-design.md`, as amended by Task 1 of this plan. The decisions Task 1 writes down were made by Jay on 2026-10-16 (recorded in `docs/superpowers/plans/2026-10-15-zero-access-plan10-outcome.md`, "Deferred findings", and in this plan's Task 1):
- 5.4: the PUT create's fan-out passes its lease as the fence; a delivery made before a takeover is an accepted, bounded residual; no compensating deletion; update PUTs take no lease.
- 5.1: the sending state follows the calendar owner's key status.
- 6.5: iTIP-level parse errors (`MissingUid`, `NoSchedulingInfo`) are history.
- 10: a recorded private-key failure skips the drain for the rest of the session.
Sections that bind unchanged: 3.3, 6.3, 6.4, 12. Release 1's spec (`docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md`) sections 10 and 12 bind too.

Project rules: `CLAUDE.md`, `docs/superpowers/plans/README-dev.md`, `docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md`. The plan 10 outcome note's "Deferred findings", "Follow-up list" and "PR #36 review" sections are this plan's input; its "Facts for release 3" section is not touched.

## Global Constraints

Commands:
- Every cargo command starts with `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`.
- Product build: `cargo build --release -p stalwart --no-default-features --features rocks`. Never build with the `enterprise` feature.
- Per-task compile check in the shipping configuration: `cargo check -p stalwart --no-default-features --features rocks`, filtered with:
  ```bash
  ... 2>&1 | grep -E "^(warning|error)" -A3 | grep -E "crates/(groupware|dav|vault)/|crates/http/src/request.rs|crates/common/src/(za/|auth/vault.rs)|crates/store/src/dispatch/lookup.rs" || echo clean
  ```
  The 24 warnings upstream's code produces in store, common, jmap and services are known and are not this plan's.
- Test-mode compile check: `cargo check -p tests --tests`.
- Unit tests: `cargo test -p vault`, `cargo test -p groupware`, `cargo test -p common za::lease`, `cargo test -p common auth::vault`.
- Integration suite: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture` (about 75 s). One server on port 8899; one cargo command at a time; never while Jay's manual server (ports 8080/8443/1143) is under test.
- CalDAV suites: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture`, plain and with `ZA_KEY_ACCOUNTS=1`. Upstream's `cal_itip` sub-test is timing-flaky; rerun once before treating a failure there as a regression.
- Formatting: `cargo fmt --all --check`; build output stays warning-free for new code.

Invariants and code rules:
- No edits to generated code (`crates/registry/src/schema/*`, `crates/trc/src/event/enums.rs`). No new setting, permission, event type or environment knob: `DRAIN_PASS_CAP` and `DRAIN_ITEM_FAILURE_LIMIT` are constants (spec 13).
- No stored struct changes layout (release 1 invariant 1). Every new state of this plan lives in `SessionKeys` (process memory) or on the stack.
- Background code never needs a key; the drain runs only in a request that holds the recipient's session keys (invariant 11); it never fails the request that triggered it (invariant 14); upstream's builders and merge run unchanged (invariant 15); non-key accounts take unchanged paths (invariant 9).
- Traces name account ids, document ids and fixed strings only: never an address, a UID, a tree or a token. A UID remembered in session state for ordering (Task 3) is never traced.
- Key material and tokens live in `Zeroizing` buffers with a silent `Debug`.
- Test addresses use `example.com` for the suite's fixed accounts (`key1@example.com`, `key2@example.com`, `key3@example.com`, `plain@example.com`, as the existing modules do) and `.test` for any new domain; never `.example`.
- Commit messages: subject line, blank line, then the two trailer lines the session's attribution reminder specifies, verbatim.

Tests:
- Every za sub-module removes what it creates; `assert_is_empty` runs once at the suite's end after `destroy_key_accounts`. Events are deleted with `Schedule-Reply: F` (`NO_REPLY`), inbox items by href (`<document id>.ics`), imported copies by the href `copy_href` returns.
- The za suite sets `DRAIN_SKIP_ALL` after setup; drain modules take `DrainOn` (`tests/src/za/drain.rs`) for their duration. New hooks are cleared at the end of the step that set them, on the success path (the module's convention).
- `wait_for_delivery` hangs while an alarm task is not yet due (`drain::test` step 1 plants an alarm due in 2099); the fan-out is synchronous, so no new step calls it after a fan-out.
- `trigger_drain(name, secret)` (`tests/src/utils/za.rs`) is any DAV request of the account through the 5-second raw client; it returns the status (207 for the principal PROPFIND it sends).
- Mutation runs are made one at a time, each reverted with `git checkout -- <file>` and followed by an empty `git status --short crates/`; none is committed.

## Review Focus

Inputs the spec implies and no existing test exercises, most likely to bite first. Each gets its test in the task that owns the code.

1. **A stuck REQUEST and a later CANCEL for the same UID.** When the drain skips an item that failed three times, a CANCEL for the same UID further down the inbox must not be applied ahead of it: the user would see a cancellation of an event that never appeared, and the REQUEST applied on a later session would resurrect it. Task 3 step 7 plants both and asserts the CANCEL stays pending.
2. **The cap on a busy inbox with new arrivals.** A session whose first pass hits the cap must continue on its next request without the 30-second backoff, and an item that arrives meanwhile must not be lost or double-applied. Task 3 step 9 plants 33 items and asserts 32 then 1, with the default backoff in force.
3. **The private-key deferral and a new session.** A corrupt or unreadable wrap must not block the account forever: a fresh session (a new login) retries the unwrap. Task 3 step 5 drains nothing on the failed session and everything on a fresh app-password session.
4. **A fan-out failure that is not a lost fence** on the PUT create (quota, inbound size limit) must still become a `SCHEDULE-STATUS` and a 201, not a 412 or 503. `fanout::test_failures` covers it today and must still pass after Task 2 (Task 2 step 6 runs it).
5. **The PUT create whose fan-out loses its fence** must answer as a lost commit fence does: 412 when the UID now exists in the calendar, 503 otherwise, with no attendee item written after the takeover. Task 2 steps 2 and 3 rewrite `uid_lease` cases (b) and (b').

---

### Task 1: Spec revision and the manual checklist

**Files:**
- Modify: `docs/superpowers/specs/2026-10-11-zero-access-invitations-design.md` (sections 5.1, 5.4, 6.2, 6.5, 10, 11, 13)
- Modify: `docs/zero-access/manual-checklist.md` (new item 14)

**Interfaces:**
- Consumes: nothing.
- Produces: the names later tasks use: `DRAIN_PASS_CAP` (32), `DRAIN_ITEM_FAILURE_LIMIT` (3), the hook names `PRIVATE_KEY_FAIL_ONCE`, `DRAIN_ENUMERATE_FAIL_ONCE`, `DRAIN_FAIL_ITEM`.

Documentation only; no build. Keep the spec's wrap width (about 76 columns) and its voice. Do not renumber sections. Each edit below names the paragraph it changes; replace or add exactly that text.

- [ ] **Step 1: Section 5.1, the owner rule**

After the first paragraph of 5.1 (ending "Every other state keeps its upstream meaning."), add:

```
The sending state follows the calendar's owner, not the authenticated
account: `ItipSendStatus::resolve` takes `owner_is_key`, whether the
account that owns the calendar being written is a key account, and
resolves to `SendSealed` on that alone. In release 2 only the owner
writes to a key account's calendar (sharing is a later release), so the
rule is descriptive; when sharing arrives it stays the right rule, since
the copy that stores the token pairs is the owner's. JMAP callers pass
`owner_is_key = false` (key accounts have no JMAP calendar access).
```

- [ ] **Step 2: Section 5.4, the fence and the accepted residual**

Replace the whole of 5.4 with:

```
### 5.4 Order and partial failure

The fan-out runs after every check of the request (headers, schedule tag,
UID, quota) and immediately before the sender's commit. The recipients'
inbox writes come first, each its own batch; then the sender's event is
sealed and committed under the sender's MK, with tokens and schedule
statuses inside.

**The fence.** A key account's create holds the UID creation lease
(section 6.3) and passes it to the fan-out: every recipient item's batch
asserts the lease before its insert, as the drain's delegating merge does
(section 6.4). A creator whose lease expired and was taken over by another
session therefore writes no item after the successor's, so a recipient
never adopts a token that lost to the committed one. A lost fence inside
the fan-out ends the request the way a lost commit fence does (section
6.3 step 3): a trace, the lease abandoned, the lease taken again to
re-check current archives, `NoUidConflict` on a hit and 503 otherwise.

**What remains.** Items delivered before the takeover stay: the recipient
drains them in document-id order, so the successor's later item wins and
the recipient ends on the committed token. A reply the recipient sends
between draining the two items carries the earlier token and is dropped
as a forgery; the recipient's next reply is accepted. When no successor
exists (the lease expired, nobody took the UID, the create answers 503),
the recipient holds an invitation for an event that was not stored until
the sender's client resends, which 503 asks for; the resent create
delivers fresh tokens with an equal SEQUENCE, which upstream's merge
accepts (`>=` in the `Request` arm of `inbound.rs`), and the copy adopts
them under the section 6.4 rule. Both are accepted: each needs a lease
expiring under a live request, and each heals on the next message.
Compensating deletion of delivered items was considered and rejected
(section 14). A reply can also beat the sender's commit: if the attendee
drains, imports and replies before the organizer's event commits, the
organizer's drain finds no event for the UID and files the reply as
history (section 6.4 step 4), never retried; the attendee's next
participation change produces a fresh reply.

**Updates and deletes** take no lease: their commit is fenced on the
event's stored archive, and a lost update commit after a fan-out leaves
the same bounded window as above. A lease on every update of a scheduled
event would cost a store round trip and a 503 on a hot UID for no new
guarantee.
```

- [ ] **Step 3: Section 6.2, the cap and the skipped items**

After the paragraph that ends "keeps a stuck item from draining on every request.", add:

```
**The cap.** A pass handles at most `DRAIN_PASS_CAP` (32) items. When
more remain, the pass ends without a deferral and without advancing the
remembered id, so the session's next request continues where it stopped.
A pass that leaves an item pending for another reason defers as above,
whether or not more items remained.

**Skipped items.** An item whose drain failed `DRAIN_ITEM_FAILURE_LIMIT`
(3) times in one session is skipped for the rest of that session, with
one trace, and later items that name the same UID are left pending with
it, so a CANCEL never overtakes the REQUEST it cancels (section 6.3).
Neither a skipped item nor one waiting behind it holds the remembered
id: a pass whose only pending items are those completes. The counters
live in the session's drain state; a new session retries the item.
```

- [ ] **Step 4: Section 6.5, the limit and the history rule**

In the **Transient** bullet, after "retried by a later pass (section 6.2)", add: ", up to `DRAIN_ITEM_FAILURE_LIMIT` failures in one session, after which the item is skipped for that session (section 6.2)". In the **iTIP-level rejection** bullet, add a second sentence:

```
  `MissingUid` and `NoSchedulingInfo` from the item's snapshot are
  iTIP-level rejections too, kept as history (the item's tree parsed; its
  scheduling content did not), not permanent errors.
```

- [ ] **Step 5: Section 10, the private-key rule**

Replace the bullet beginning "A private-key unwrap failure in `SessionKeys`" with:

```
- A private-key unwrap failure in `SessionKeys` (a corrupt
  `private_key_wrap`, a missing or unusable record) is traced once per
  session and the drain is skipped for the rest of that session: the
  session's drain state takes a permanent deferral, so no later request
  of the session enumerates the inbox. The request proceeds. Pending
  items wait for a new session, whose `SessionKeys` retry the unwrap. A
  store error reading the record is not recorded: it is traced, the
  session defers by `DRAIN_RETRY_SECS`, and a later pass retries.
- A store error before the drain's match (the inbox change id, the item
  ids, an item read) defers the session by `DRAIN_RETRY_SECS` like one
  after it, so a persistent store fault traces once per backoff, not once
  per request.
```

- [ ] **Step 6: Section 11, the hooks and the tests**

In the hooks sentence (the one listing "a fan-out pause ..., a task-consumer pause"), append after "a task-consumer pause": ", a one-shot private-key unwrap failure (`PRIVATE_KEY_FAIL_ONCE`), a one-shot failure of the drain's enumeration (`DRAIN_ENUMERATE_FAIL_ONCE`), and a persistent commit failure for one item (`DRAIN_FAIL_ITEM`, keyed by account and document id)".

To the **Integration** bullet list, add:

```
- Drain limits (`tests/src/za/drain_limits.rs`): a session whose private
  key does not open drains nothing on any later request and a fresh
  session drains everything; an enumeration error defers the session
  like an item error; an item that fails three times is skipped, a later
  CANCEL for its UID waits with it, an unrelated item drains, and a fresh
  session applies both in order; a pass of 33 pending items drains 32 and
  the next request the rest, an arrival in between included, with the
  default backoff in force.
- The PUT create's fence: lease cases (b) and (b') assert that the held
  create writes no attendee item after its lease was taken over.
```

- [ ] **Step 7: Section 13, the names**

In the **Names** bullet, after "the drain's retry backoff (`DRAIN_RETRY_SECS`, a constant of 30 seconds)", add: ", the per-pass cap (`DRAIN_PASS_CAP`, 32) and the per-item failure limit (`DRAIN_ITEM_FAILURE_LIMIT`, 3), both constants". Add a sentence to the same bullet's end: "A merge that changes nothing (`MergeResult::None`) is written as history with flags `0`, where upstream writes no inbox item; the item stays visible as the organizer's unchanged message."

- [ ] **Step 8: Section 14, the rejected alternative**

Append to section 14:

```
- **Compensating deletion of delivered items** after a lost create fence
  (section 5.4): the fan-out would return each recipient's document id and
  the losing request would delete them, skipping any a drain already
  cleared. Rejected: a cross-account delete racing the recipient's drain,
  for a case that needs a lease to expire under a live request and that
  heals on the next message. The fence removes the lasting harm; the rest
  is accepted.
```

- [ ] **Step 9: The manual checklist**

Append item 14 to `docs/zero-access/manual-checklist.md` after item 13:

```
14. Reply handling across clients (release 2): with an organizer client and an attendee client connected to two key accounts, invite, let the attendee accept, then move the event from the organizer and let the attendee accept again. The organizer must show the second acceptance. A client that strips SEQUENCE from the attendee's copy would have every reply to a rescheduled event kept as history (the drain's stale-reply check reads an absent SEQUENCE as 0); if the organizer never shows the second acceptance, record the client and its version.
```

- [ ] **Step 10: Commit**

```bash
git add docs/superpowers/specs/2026-10-11-zero-access-invitations-design.md docs/zero-access/manual-checklist.md
git commit -m "Invitations spec: the create's fence, the owner rule, drain limits, the private-key deferral"
```
(with the two trailer lines).

### Task 2: The PUT create's fan-out passes its lease as the fence

**Files:**
- Modify: `crates/dav/src/calendar/mod.rs:228-275` (`za_commit_create`: extract the lost-fence branch into `za_lost_create_fence`)
- Modify: `crates/dav/src/calendar/update.rs:533-545` (the create branch's `za_fan_out` call)
- Test: `tests/src/za/uid_lease.rs` (cases (b) and (b'))

**Interfaces:**
- Consumes: `ZaFanOut::za_fan_out(&self, sender, keys, messages, stored_pairs, event, fence: Option<&ZaUidLease>)` (`crates/groupware/src/calendar/fanout.rs`, plan 10 fix wave `ca8d2e2b`): with a fence, every recipient item's batch asserts it and the call returns `Err(err)` with `err.is_assertion_failure()` when the fence is lost; every other recipient failure is a `SCHEDULE-STATUS`. `ZaUidLeaseGuard::lease(&self) -> &ZaUidLease`, `abandon(self)`, `release(self)` (`crates/common/src/za/lease.rs`). `za_take_uid_lease(server, resources, account_id, calendar_id, uid) -> crate::Result<ZaUidLeaseGuard>` (`mod.rs:136`).
- Produces: `pub(crate) async fn za_lost_create_fence(server: &Server, resources: &DavResources, lease: ZaUidLeaseGuard, account_id: u32, calendar_id: u32, uid: &str) -> crate::Result<()>`: the trace, the abandon, the retake and the 412-or-503 answer, shared by the fan-out and the commit.

Background. Today `za_commit_create` (`mod.rs:228`) handles a lost commit fence inline: it traces `AssertValueFailed` with details `zero-access: UID creation lease lost before the commit`, calls `lease.abandon()`, takes the lease again with `za_take_uid_lease` (which answers `NoUidConflict` when the UID now exists in the calendar), releases that lease and returns `DavError::Code(StatusCode::SERVICE_UNAVAILABLE)`. The fan-out at `update.rs:538` passes `None` as the fence. Plan 9's lease case (b) (`tests/src/za/uid_lease.rs:93-134`) parks a create inside its fan-out to key2 (`FAN_OUT_PAUSE`, between the item's document-id assignment and its commit), lets the 1-second lease expire, takes the lease over from the test, plants a copy, releases, and resumes the create: it asserts 412, one copy, and today "the held PUT's item was written before its commit" (one new item in key2's inbox). Case (b') does the same with no planted copy and asserts 503 and one item. With the fence, the parked item's batch asserts the lease after the pause and fails: the fan-out returns the assertion failure and no item is written.

- [ ] **Step 1: Extract the lost-fence helper**

In `crates/dav/src/calendar/mod.rs`, add above `za_commit_create`:

```rust
/// Invitations spec 5.4 and 6.3 step 3: what a key account's create does
/// when its lease was taken over while the request ran, found by the
/// fan-out's fence or by the commit's. Traced first, so a busy lease or a
/// 412 from the retake does not hide that the fence was lost; the lease
/// is abandoned (the key holds a successor's value, or is gone); the lease
/// is taken again to re-check current archives (`NoUidConflict` on a hit)
/// and released; otherwise 503, so the client resends.
pub(crate) async fn za_lost_create_fence(
    server: &Server,
    resources: &DavResources,
    lease: ZaUidLeaseGuard,
    account_id: u32,
    calendar_id: u32,
    uid: &str,
) -> crate::Result<()> {
    trc::event!(
        Store(trc::StoreEvent::AssertValueFailed),
        AccountId = account_id,
        Details = "zero-access: UID creation lease lost before the commit",
    );
    lease.abandon();
    let retry = za_take_uid_lease(server, resources, account_id, calendar_id, uid).await?;
    retry.release().await;
    // Reached only when another creator took the lease and did not store
    // the UID in this calendar (a PUT into another calendar, or a drain
    // that failed).
    Err(DavError::Code(StatusCode::SERVICE_UNAVAILABLE))
}
```

Then make `za_commit_create`'s `Err(err) if err.is_assertion_failure()` arm a single call: `za_lost_create_fence(server, resources, lease, account_id, calendar_id, uid).await`. Keep the test-mode `take_commit_fail` branch and the other two arms unchanged; the function's doc comment keeps its text.

- [ ] **Step 2: Pass the fence at the create's fan-out**

In `crates/dav/src/calendar/update.rs`, the create branch (around line 533):

```rust
            if let Some(keys) = &za_keys {
                // Spec 5.3-5.4: after every check, immediately before the
                // sealed commit; the fan-out hands out the attendees' tokens.
                // Fenced on the creation lease (spec 5.4): a creator whose
                // lease was taken over writes no item after its successor's.
                let mut pairs = None;
                if let Some(messages) = za_messages {
                    let fan_out = match self
                        .za_fan_out(
                            &account_info,
                            keys,
                            messages,
                            None,
                            &event.data.event,
                            za_lease.as_ref().map(|(lease, _)| lease.lease()),
                        )
                        .await
                    {
                        Ok(fan_out) => fan_out,
                        Err(err) if err.is_assertion_failure() => {
                            let (lease, uid) =
                                za_lease.take().expect("a lost fence needs a lease");
                            return za_lost_create_fence(
                                self,
                                &resources,
                                lease,
                                account_id,
                                parent.document_id(),
                                &uid,
                            )
                            .await
                            .map(|()| unreachable!("the lost-fence path never succeeds"));
                        }
                        Err(err) => return Err(err.caused_by(trc::location!()).into()),
                    };
                    za_set_schedule_status(&mut event.data.event, &fan_out.statuses);
                    pairs = fan_out.pairs;
                }
                seal_event(&mut event, keys, account_id, pairs.as_ref())
                    .map_err(|err| seal_error(err, account_id, u32::MAX))?;
            }
```

`za_lease` must be `let mut za_lease = ...` for the `take()`. If `za_lost_create_fence` returning `crate::Result<()>` makes the `map(|()| unreachable!(..))` awkward in this function's return type, change the helper to return `crate::Result<HttpResponse>` (it never returns `Ok`) and `return za_lost_create_fence(..).await;` directly; pick the one that reads cleanest and keep both call sites consistent. Add `za_lost_create_fence` to the `use super::{..}` line at the top of `update.rs`.

- [ ] **Step 3: Rewrite lease cases (b) and (b')**

In `tests/src/za/uid_lease.rs`, case (b) (`// (b) Expiry: the held PUT's commit is fenced out.`): the comment becomes

```rust
    // (b) Expiry: the held PUT's fan-out and commit are fenced out (spec
    // 5.4). Parked inside its fan-out to key2 past the 1-second lease, the
    // PUT finds its lease taken over: the item's batch loses the fence, so
    // key2 receives nothing from it, and the retake finds the planted copy
    // (412).
```

and the final assertion of the case changes from `1` to `0`:

```rust
    assert_eq!(
        new_inbox_items(test, key2.id, &start2).await.len(),
        0,
        "the held PUT delivered after its lease was taken over (spec 5.4)"
    );
```

Case (b') (`// (b') Expiry with no copy`): the comment's last sentence becomes "its item is fenced out too." and the assertion changes from `1` to `0` with the message `"the held PUT delivered after its lease was taken over (spec 5.4)"`. The `503` assertion and `uid_copies == 0` stay.

Then check that the module's cleanup still accounts for every item: `key2.delete_items_since(test, &start2)` at the end deletes whatever key2 received; with two fewer items it deletes less. Nothing else changes.

- [ ] **Step 4: Run the lease tests before the code change to see them fail**

Run (with Steps 1 and 2 stashed, or by applying Step 3 first): `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture 2>&1 | grep -E "panicked|lease" | head`
Expected: the suite fails in `uid_lease` at the new case (b) assertion with `left: 1, right: 0`.

- [ ] **Step 5: Compile checks and the suite**

Run `cargo fmt --all --check`, the product check, `cargo check -p tests --tests`, then the za suite.
Expected: clean, clean, clean; `za_tests` passes (`uid_lease` cases (b) and (b') now assert 0 items; the drain's lease cases are unaffected).

- [ ] **Step 6: Review Focus 4, the fan-out failures that are not a lost fence**

The za suite's `fanout::test_failures` (`tests/src/za/fanout.rs`) covers quota and size-limit statuses on the create path. Confirm from the suite output of Step 5 that it passed (it prints "Running zero-access fan-out failure tests..." and the suite reaches the end). Nothing to add.

- [ ] **Step 7: Key-mode CalDAV suite**

Run `ZA_KEY_ACCOUNTS=1 STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture`.
Expected: pass (the `scheduling` variant creates events with attendees; its fan-out now carries the fence).

- [ ] **Step 8: Commit**

```bash
git add crates/dav/src/calendar/mod.rs crates/dav/src/calendar/update.rs tests/src/za/uid_lease.rs
git commit -m "PUT create: the fan-out is fenced on the creation lease"
```
(with the two trailer lines).

### Task 3: Drain limits: the private-key deferral, repeated failures, the cap, early store errors

**Files:**
- Modify: `crates/vault/src/session.rs:29-82` (`ZaDrainState`, new `PassState`)
- Modify: `crates/groupware/src/calendar/drain.rs` (module doc; `drain_pass`, `drain_items`, `drain_item`, `ItemCtx::commit`; new constants and enums)
- Modify: `crates/common/src/za/test_hooks.rs` (three hooks)
- Modify: `crates/common/src/auth/vault.rs:327-352` (`za_open_private_key`: the test-mode hook)
- Create: `tests/src/za/drain_limits.rs`
- Modify: `tests/src/za/mod.rs` (register the module; call it after `drain::test_errors`)

**Interfaces:**
- Consumes: `ZaDrainState::{cursor, complete, retry_after, defer_until, lock}` (`session.rs`); `drain_pass`, `drain_items`, `drain_item` (`drain.rs:135-275`, signatures below); `test_hooks::{take_flag, set_slot}` and the `Mutex<Option<u32>>` flag pattern (`test_hooks.rs`); test helpers `Party`, `invite`, `uid_copies` (`tests/src/za/fanout.rs`), `DrainOn`, `request_tree`, `copy_href`, `pending_ids`, `trigger_drain`, `za_app_password`, `plant_inbox_item`, `PlantedItem`, `inbox_ids`, `new_inbox_items`, `NO_REPLY`, `CONTENT_TYPE`, `STRONG`.
- Produces: `pub const DRAIN_PASS_CAP: usize = 32;` and `pub const DRAIN_ITEM_FAILURE_LIMIT: u8 = 3;` in `drain.rs`; `vault::session::PassState` with `failures: HashMap<u32, u8>` and `skipped: HashMap<u32, Option<String>>`; `ZaDrainState::lock() -> MutexGuard<'_, PassState>`; hooks `set_private_key_fail(Option<u32>)`, `set_drain_enumerate_fail(Option<u32>)`, `set_drain_fail_item(Option<(u32, u32)>)`; the test module `drain_limits::test`.

Shapes (write them as given; wire the rest to fit):

```rust
// drain.rs
/// Spec 6.2, "The cap": items one pass handles at most. A constant (spec 13).
pub const DRAIN_PASS_CAP: usize = 32;
/// Spec 6.2, "Skipped items": failures of one item in one session before
/// the session skips it. A constant (spec 13).
pub const DRAIN_ITEM_FAILURE_LIMIT: u8 = 3;
/// Spec 10: a deferral no later request of the session passes.
const PERMANENT_DEFERRAL: u64 = u64::MAX;

/// What one item's drain left behind.
enum ItemOutcome {
    /// Rewritten or deleted.
    Done,
    /// Left pending; the pass stops here (a busy UID lease, spec 6.3).
    LeftPending,
    /// Left pending behind a skipped item of its UID; the pass goes on.
    Waiting,
}

/// What a pass left behind.
enum PassOutcome {
    /// Every item is done, skipped, or waiting behind a skipped one.
    Complete,
    /// An item stays pending for a reason a later pass can clear.
    LeftPending,
    /// `DRAIN_PASS_CAP` items done, more remain: the next request continues.
    More,
    /// The private key does not open: no later pass of this session.
    Suspended,
}

/// An item's error with the UID it named, if the item got that far.
struct ItemFailure {
    err: trc::Error,
    uid: Option<String>,
}
```

```rust
// session.rs
use std::collections::HashMap;

/// Per-session bookkeeping of items that failed (invitations spec 6.2,
/// "Skipped items"), held under the pass lock. A UID kept here is never
/// traced.
#[derive(Default)]
pub struct PassState {
    /// Drain failures of each item (by document id) in this session.
    pub failures: HashMap<u32, u8>,
    /// Items skipped after `DRAIN_ITEM_FAILURE_LIMIT` failures, with the
    /// UID each named, if known: later items of that UID wait.
    pub skipped: HashMap<u32, Option<String>>,
}

impl PassState {
    /// Records a failure; `true` when it reached the limit and the item is
    /// now skipped.
    pub fn record_failure(&mut self, document_id: u32, uid: Option<String>, limit: u8) -> bool {
        let count = self.failures.entry(document_id).or_insert(0);
        *count = count.saturating_add(1);
        if *count >= limit {
            self.skipped.insert(document_id, uid);
            true
        } else {
            false
        }
    }

    pub fn is_skipped(&self, document_id: u32) -> bool {
        self.skipped.contains_key(&document_id)
    }

    /// Whether a skipped item named `uid`.
    pub fn blocks_uid(&self, uid: &str) -> bool {
        self.skipped.values().flatten().any(|skipped| skipped == uid)
    }
}
```

`ZaDrainState.pass` becomes `tokio::sync::Mutex<PassState>`; `lock()` returns `MutexGuard<'_, PassState>`; `new()` uses `PassState::default()`. `cursor`, `complete`, `retry_after`, `defer_until` are unchanged.

- [ ] **Step 1: Hooks**

In `crates/common/src/za/test_hooks.rs`, add to the module doc list:

```
//! - `PRIVATE_KEY_FAIL_ONCE`: the account's next private-key unwrap records
//!   a failure, as a corrupt wrap would (`common::auth::vault`).
//! - `DRAIN_ENUMERATE_FAIL_ONCE`: the account's next drain pass fails with an
//!   internal error while enumerating, before any item is read.
//! - `DRAIN_FAIL_ITEM`: every drain commit of the named (account, document
//!   id) fails with an internal error until the slot is cleared (not
//!   one-shot).
```

Statics and functions, following `DRAIN_FAIL_ONCE`'s shape:

```rust
pub static PRIVATE_KEY_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);
pub static DRAIN_ENUMERATE_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);
pub static DRAIN_FAIL_ITEM: Mutex<Option<(u32, u32)>> = Mutex::new(None);

pub fn set_private_key_fail(account_id: Option<u32>) {
    *PRIVATE_KEY_FAIL_ONCE.lock().unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn set_drain_enumerate_fail(account_id: Option<u32>) {
    *DRAIN_ENUMERATE_FAIL_ONCE.lock().unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn set_drain_fail_item(item: Option<(u32, u32)>) {
    *DRAIN_FAIL_ITEM.lock().unwrap_or_else(|e| e.into_inner()) = item;
}

/// Whether the account's next private-key unwrap must fail; clears the flag.
pub fn take_private_key_fail(account_id: u32) -> bool {
    take_flag(&PRIVATE_KEY_FAIL_ONCE, account_id)
}

/// Whether the account's next drain enumeration must fail; clears the flag.
pub fn take_drain_enumerate_fail(account_id: u32) -> bool {
    take_flag(&DRAIN_ENUMERATE_FAIL_ONCE, account_id)
}

/// Whether the drain commit of this item must fail; the slot stays set.
pub fn drain_item_fails(account_id: u32, document_id: u32) -> bool {
    *DRAIN_FAIL_ITEM.lock().unwrap_or_else(|e| e.into_inner()) == Some((account_id, document_id))
}
```

In `crates/common/src/auth/vault.rs`, `za_open_private_key`: inside `if keys.private_key().is_none() {`, compute `outcome` as today, then before `let failed = outcome.is_err();` add:

```rust
        #[cfg(feature = "test_mode")]
        let outcome = if crate::za::test_hooks::take_private_key_fail(account_id) {
            Err(KeyError::Aead)
        } else {
            outcome
        };
```

(`outcome` must be a binding, not a temporary; the existing `let outcome = match ...` already is.)

- [ ] **Step 2: Session state**

Apply the `session.rs` shape above. `cargo test -p vault` must still pass (no vault test touches the pass lock).

- [ ] **Step 3: The drain**

In `drain.rs`:

a. `drain_pass`: take `let mut pass = state.lock().await;`. Move the enumeration (the `fetch_dav_resources` call, the cursor check, the `itip_ids` loop) into

```rust
/// Spec 6.2: the snapshot and the pending items, oldest first; `None` when
/// the cursor is at the snapshot.
async fn enumerate(server: &Server, account_id: u32, state: &ZaDrainState) -> trc::Result<Option<(u64, Vec<u32>)>>
```

with, at its top, `#[cfg(feature = "test_mode")] if common::za::test_hooks::take_drain_enumerate_fail(account_id) { return Err(trc::StoreEvent::UnexpectedError.into_err().details("zero-access test hook: drain enumeration failure").caused_by(trc::location!())); }`. In `drain_pass`:

```rust
    let Some((snapshot, mut pending)) = (match enumerate(server, account_id, state).await {
        Ok(enumerated) => enumerated,
        // Spec 10: a store error before the match defers like one after it.
        Err(err) => {
            state.defer_until(now + drain_retry_secs());
            return Err(err);
        }
    }) else {
        return Ok(());
    };
    // Spec 6.2, "Skipped items": neither a skipped item nor one waiting
    // behind it holds the cursor.
    pending.retain(|document_id| !pass.is_skipped(*document_id));
    if pending.is_empty() {
        state.complete(snapshot);
        return Ok(());
    }
    match drain_items(server, keys, pending, &mut pass).await {
        Ok(PassOutcome::Complete) => state.complete(snapshot),
        Ok(PassOutcome::LeftPending) => state.defer_until(now + drain_retry_secs()),
        // The next request continues from the cursor, at once.
        Ok(PassOutcome::More) => {}
        Ok(PassOutcome::Suspended) => state.defer_until(PERMANENT_DEFERRAL),
        Err(err) => {
            state.defer_until(now + drain_retry_secs());
            return Err(err);
        }
    }
    Ok(())
```

Waiting items are filtered by UID inside `drain_item`, not here (the UID is known only after the item is opened), so `pending.retain` removes skipped ids only.

b. `drain_items(server, keys, pending, pass: &mut PassState) -> trc::Result<PassOutcome>`: the private-key branch returns `Ok(PassOutcome::Suspended)` where it returned `Ok(false)` (the recorded failure) and keeps `Err(err)` for a store error. The loop:

```rust
    for (handled, document_id) in pending.into_iter().enumerate() {
        if handled == DRAIN_PASS_CAP {
            return Ok(PassOutcome::More);
        }
        match drain_item(server, &account_info, keys, private_key, document_id, pass).await {
            Ok(ItemOutcome::Done | ItemOutcome::Waiting) => {}
            Ok(ItemOutcome::LeftPending) => return Ok(PassOutcome::LeftPending),
            Err(ItemFailure { err, uid }) => {
                if pass.record_failure(document_id, uid, DRAIN_ITEM_FAILURE_LIMIT) {
                    // Spec 6.2, "Skipped items": traced once, then skipped
                    // for the session; its UID's later items wait.
                    trc::error!(
                        err.account_id(account_id)
                            .document_id(document_id)
                            .details("zero-access: drain item skipped after repeated failures")
                            .caused_by(trc::location!())
                    );
                } else {
                    return Err(err);
                }
            }
        }
    }
    Ok(PassOutcome::Complete)
```

(`trc::Error` has `account_id`, `document_id` and `details` builders, used elsewhere in this file; use `caused_by` as the neighbouring traces do.)

c. `drain_item(.., document_id, pass: &PassState) -> Result<ItemOutcome, ItemFailure>`: the prologue (read, pending check, open, METHOD, the UID snapshot) maps every `?` error to `ItemFailure { err, uid: None }` (a small `.map_err(|err| ItemFailure { err, uid: None })` on each, or a local closure). After `let (uid, candidates) = ...`:

```rust
    // Spec 6.2, "Skipped items": behind a skipped item of this UID, so a
    // CANCEL never overtakes the REQUEST it cancels.
    if pass.blocks_uid(&uid) {
        return Ok(ItemOutcome::Waiting);
    }
```

The remainder (sender check, `find_event`, the `match (method, existing)`) moves into `async fn apply(ctx, tree, carriers, method, uid: &str, candidates) -> trc::Result<bool>` (or stays inline inside an `async { .. }` block) whose result `drain_item` maps: `Ok(true)` to `Done`, `Ok(false)` to `LeftPending`, `Err(err)` to `ItemFailure { err, uid: Some(uid) }`. `import` and `merge` keep their `trc::Result<bool>` signatures.

d. `ItemCtx::commit`: next to `take_drain_fail`, add `|| common::za::test_hooks::drain_item_fails(self.keys.account_id, self.document_id)` in the same test-mode `if`, with details `"zero-access test hook: drain commit failure"` unchanged.

e. Module doc: after the sentence ending "defers the next pass by `DRAIN_RETRY_SECS`.", add: "A pass handles at most `DRAIN_PASS_CAP` items and, with more remaining, ends without a deferral so the next request continues. An item that failed `DRAIN_ITEM_FAILURE_LIMIT` times in the session is skipped for the session, traced once, and later items of its UID wait with it; neither holds the cursor. A private key that does not open suspends the session's drain for good (spec 10)."

- [ ] **Step 4: Compile checks**

Run `cargo fmt --all --check`, the product check, `cargo check -p tests --tests`, `cargo test -p vault`, `cargo test -p groupware`, `cargo test -p common auth::vault`.
Expected: all clean and green (the groupware unit tests of `drain.rs` touch none of the changed functions).

- [ ] **Step 5: The test module, step 1: the private key**

Create `tests/src/za/drain_limits.rs` with the SPDX header (copy from `drain.rs`), a module doc `//! Invitations spec 6.2 ("The cap", "Skipped items") and 10: the drain's per-session limits.`, and:

```rust
use super::{
    STRONG,
    drain::{DrainOn, request_tree},
    fanout::{KEY1_CAL, NO_REPLY, Party, START_A, invite, uid_copies},
};
use crate::utils::{
    http::CONTENT_TYPE,
    server::TestServer,
    za::{
        PlantedItem, copy_href, inbox_ids, pending_ids, plant_inbox_item, trigger_drain,
        za_app_password,
    },
};
use common::za::test_hooks;
use hyper::StatusCode;
use std::time::Duration;

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access drain limit tests...");
    let _drain = DrainOn::new();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let start2 = inbox_ids(test, key2.id).await;

    // 1. A private key that does not open suspends the session's drain
    // (spec 10): a fresh session whose first unwrap fails drains nothing on
    // any later request, backoff or not; another session drains the item.
    let uid1 = "za-limit-1";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}limit-1.ics"),
            [CONTENT_TYPE],
            invite(uid1, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pending_ids(test, key2.id).await.len(), 1);
    // A fresh session: the hook fires on a session's first unwrap only, and
    // key2's password session unwrapped its key in an earlier module.
    let app_fail = za_app_password(key2.name, STRONG, "drain limits, failing").await;
    test_hooks::set_private_key_fail(Some(key2.id));
    assert_eq!(
        trigger_drain(key2.name, &app_fail).await,
        207,
        "a private-key failure failed the request"
    );
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        1,
        "a session without its private key drained"
    );
    test_hooks::set_drain_retry_override(Some(0));
    assert_eq!(trigger_drain(key2.name, &app_fail).await, 207);
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        1,
        "the backoff lifted a permanent deferral"
    );
    test_hooks::set_drain_retry_override(None);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(
        pending_ids(test, key2.id).await.is_empty(),
        "another session of the account did not drain"
    );
    assert_eq!(uid_copies(test, key2.id, uid1).await, 1);
```

(`CONTENT_TYPE`, `NO_REPLY`, `KEY1_CAL`, `START_A`: import them from wherever `drain.rs` imports them; check its `use` block and copy.)

- [ ] **Step 6: Step 2 of the module: an enumeration error defers**

```rust
    // 2. A store error before the match defers the session like one after
    // it (spec 10): with the enumeration failing once, the next request
    // inside the backoff does not drain; after it, the item drains.
    let uid2 = "za-limit-2";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}limit-2.ics"),
            [CONTENT_TYPE],
            invite(uid2, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    // The deferral is an absolute deadline taken at the failing pass, so
    // the short backoff is set before the failure (as `test_cursor` does).
    test_hooks::set_drain_retry_override(Some(2));
    test_hooks::set_drain_enumerate_fail(Some(key2.id));
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(pending_ids(test, key2.id).await.len(), 1);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        1,
        "an enumeration error set no deferral"
    );
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(pending_ids(test, key2.id).await.is_empty());
    assert_eq!(uid_copies(test, key2.id, uid2).await, 1);
    test_hooks::set_drain_retry_override(None);
```

- [ ] **Step 7: Step 3 of the module: repeated failures and the UID order (Review Focus 1)**

```rust
    // 3. An item that fails three times is skipped for the session (spec
    // 6.2, "Skipped items"): a later CANCEL for its UID waits with it, an
    // unrelated item drains, and the skipped item is not retried in this
    // session even once it would succeed; a fresh session applies the
    // REQUEST and then the CANCEL, in order.
    let uid3 = "za-limit-3";
    let organizer3 = format!("{KEY1_CAL}limit-3.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer3,
            [CONTENT_TYPE],
            invite(uid3, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let request_item = pending_ids(test, key2.id).await;
    assert_eq!(request_item.len(), 1);
    let request_item = request_item[0];
    // A CANCEL for the same UID (no `Schedule-Reply: F`), then an unrelated
    // invitation.
    key1.client
        .request("DELETE", &organizer3, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let uid4 = "za-limit-4";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}limit-4.ics"),
            [CONTENT_TYPE],
            invite(uid4, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pending_ids(test, key2.id).await.len(), 3);
    test_hooks::set_drain_fail_item(Some((key2.id, request_item)));
    test_hooks::set_drain_retry_override(Some(0));
    for attempt in 1..=2 {
        assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
        assert_eq!(
            pending_ids(test, key2.id).await.len(),
            3,
            "failure {attempt} did not abort the pass"
        );
    }
    // The third failure skips the item; the pass goes on.
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let left = pending_ids(test, key2.id).await;
    assert_eq!(left.len(), 2, "the skipped item did not let the pass continue");
    assert!(left.contains(&request_item), "the skipped item was applied");
    assert_eq!(uid_copies(test, key2.id, uid3).await, 0, "the CANCEL overtook its REQUEST");
    assert_eq!(uid_copies(test, key2.id, uid4).await, 1, "the unrelated item did not drain");
    // Not retried in this session once it would succeed.
    test_hooks::set_drain_fail_item(None);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(uid_copies(test, key2.id, uid3).await, 0, "a skipped item was retried");
    assert_eq!(pending_ids(test, key2.id).await.len(), 2);
    test_hooks::set_drain_retry_override(None);
    // A fresh session retries: the REQUEST imports, then the CANCEL applies.
    let app_fresh = za_app_password(key2.name, STRONG, "drain limits, fresh").await;
    assert_eq!(trigger_drain(key2.name, &app_fresh).await, 207);
    assert!(pending_ids(test, key2.id).await.is_empty());
    assert_eq!(uid_copies(test, key2.id, uid3).await, 1);
    let copy3 = copy_href(test, key2.id, uid3).await;
    let cancelled = super::drain::body(&key2.client, &copy3).await;
    assert!(cancelled.contains("STATUS:CANCELLED"), "{cancelled}");
```

(`DELETE` without the `NO_REPLY` header builds a CANCEL; the module's cleanup section deletes `limit-4.ics` with it.)

- [ ] **Step 8: Step 4 of the module: the cap (Review Focus 2)**

```rust
    // 4. The cap (spec 6.2, "The cap"): 33 pending items drain 32 in one
    // pass; the next request continues at once, with the default backoff
    // in force, and takes an item that arrived in between as well.
    let uids: Vec<String> = (0..33).map(|i| format!("za-limit-c{i}")).collect();
    for (i, uid) in uids.iter().enumerate() {
        plant_inbox_item(
            test,
            key2.id,
            &request_tree(uid, "key1@example.com", "key2@example.com"),
            PlantedItem::Pending {
                pair: Some(("key2@example.com", [i as u8 + 1; 16])),
                reply_token: None,
            },
            key1.id,
            None,
        )
        .await;
    }
    assert_eq!(pending_ids(test, key2.id).await.len(), 33);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(pending_ids(test, key2.id).await.len(), 1, "the pass ignored the cap");
    let late = "za-limit-c33";
    plant_inbox_item(
        test,
        key2.id,
        &request_tree(late, "key1@example.com", "key2@example.com"),
        PlantedItem::Pending {
            pair: Some(("key2@example.com", [0x77; 16])),
            reply_token: None,
        },
        key1.id,
        None,
    )
    .await;
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(
        pending_ids(test, key2.id).await.is_empty(),
        "the continuation waited for the backoff"
    );
    let mut copies = 0;
    for uid in uids.iter().map(String::as_str).chain([late]) {
        copies += uid_copies(test, key2.id, uid).await;
    }
    assert_eq!(copies, 34);
```

- [ ] **Step 9: Cleanup and registration**

```rust
    // Cleanup.
    for name in ["limit-1", "limit-2", "limit-4"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for uid in [uid1, uid2, uid3, uid4]
        .into_iter()
        .chain(uids.iter().map(String::as_str))
        .chain([late])
    {
        let href = copy_href(test, key2.id, uid).await;
        key2.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key2.delete_items_since(test, &start2).await;
}
```

In `tests/src/za/mod.rs`: `pub mod drain_limits;` in the module list (alphabetical, after `drain`), and `drain_limits::test(&mut test).await;` right after `drain::test_errors(&mut test).await;`. If `request_tree` or `body` is not `pub` in `drain.rs`, make it `pub` (same crate).

- [ ] **Step 10: Run the suite**

Run `cargo check -p tests --tests`, then the za suite.
Expected: `drain_limits` passes all four steps; every other module unchanged. If step 4's second request does not find the late item, check `enumerate`: the cursor must not advance on `More`.

- [ ] **Step 11: Kill checks (record each line for Task 6)**

One at a time, each reverted:
- `PERMANENT_DEFERRAL` replaced by `now + drain_retry_secs()` in the `Suspended` arm (needs `now` in scope): step 1 fails at "the backoff lifted a permanent deferral".
- The `Err(err) => { state.defer_until(..); return Err(err); }` arm of `enumerate`'s match reduced to `Err(err) => return Err(err)`: step 2 fails at "an enumeration error set no deferral".
- `record_failure` returning `false` always: step 3 fails at "the skipped item did not let the pass continue".
- `blocks_uid` returning `false` always: step 3 fails at "the CANCEL overtook its REQUEST".
- `if handled == DRAIN_PASS_CAP` removed: step 4 fails at "the pass ignored the cap".

- [ ] **Step 12: Commit**

```bash
git add crates/vault/src/session.rs crates/groupware/src/calendar/drain.rs crates/common/src/za/test_hooks.rs crates/common/src/auth/vault.rs tests/src/za/drain_limits.rs tests/src/za/mod.rs
git commit -m "Drain limits: a suspended session, skipped items, the per-pass cap, deferral on enumeration errors"
```
(with the two trailer lines).

### Task 4: The deferred tests

**Files:**
- Modify: `tests/src/za/drain.rs` (`test` step 6; `test_reply` step 10; `test_adoption` step 1; `test_lease` cases A and E; `test_errors` step 3)
- Modify: `tests/src/za/leak.rs` (`test_drain`)
- Modify: `tests/src/webdav/za_variants.rs` (`scheduling`, the update step)
- Modify: `crates/groupware/src/calendar/drain.rs` (`mod tests`: one unit test)

**Interfaces:**
- Consumes: the helpers the touched modules already import (`plant_inbox_item`, `PlantedItem`, `reply_tree`, `stored_pairs`, `partstat_of`, `stored_item`, `inbox_ids`, `pending_ids`, `uid_copies`, `copy_href`, `trigger_drain`, `scan`, `itip_tasks_for`, `raw_event`, `HttpRequest`); `Attendance::of`, `Attendance::is_new`, `itip_snapshot`, `InstanceId` (`drain.rs`).
- Produces: nothing later tasks consume.

Each step is one test addition; keep the touched module's numbering and cleanup pattern. Every za step runs inside the suite (`STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture`); run it once after Steps 1-7 together, since one run takes about 75 s, and read each step's assertion messages in the output.

- [ ] **Step 1: `test_reply` step 10, the REPLY sender resolution**

After step 9 and before `// Cleanup.`:

```rust
    // 10. The sender of a REPLY is resolved before its token is checked
    // (spec 6.4 step 2): a REPLY naming key2 as the replier and carrying
    // key2's real token, planted as if key3 had sent it, is history for a
    // sender mismatch, not applied and not a forgery; key1's copy keeps
    // key2's status from step 1.
    let real_token = *stored_pairs(test, &keys1, key1.id, "default/drain-r1.ics")
        .await
        .expect("key1's pairs for drain-r1")
        .get("key2@example.com")
        .expect("key2's pair");
    let misattributed = plant_inbox_item(
        test,
        key1.id,
        &reply_tree(
            "za-drain-r1",
            "key1@example.com",
            "key2@example.com",
            "DECLINED",
            0,
        ),
        PlantedItem::Pending {
            pair: None,
            reply_token: Some(real_token),
        },
        key3.id,
        None,
    )
    .await;
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    let item = stored_item(test, key1.id, misattributed).await;
    assert_eq!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0, "the item is still pending");
    assert_eq!(item.flags & EVENT_NOTIFICATION_IS_CHANGE, 0, "a misattributed reply was a change");
    assert_eq!(item.event_id, None);
    assert_eq!(
        partstat_of(&key1.client, &format!("{KEY1_CAL}drain-r1.ics"), "key2@example.com")
            .await
            .as_deref(),
        Some("ACCEPTED"),
        "a reply from the wrong principal applied"
    );
```

(`stored_pairs(..).get(..)` returns `Option<&[u8; 16]>`; copy it with `*`. If the pairs type names the token array differently, follow `pair_token` in `tests/src/za/fanout.rs`.) The item is cleaned by `key1.delete_items_since(test, &start1)`.

- [ ] **Step 2: `test_adoption` step 1, the old token after a re-add**

After the step's last assertion (`"the reply under the new token did not apply"`):

```rust
    // The old token is a forgery now (spec 3.3, "Verification"): a reply
    // planted under it is deleted and the copy's status stands.
    let stale_token = plant_inbox_item(
        test,
        key1.id,
        &reply_tree(uid, "key1@example.com", "key2@example.com", "DECLINED", 0),
        PlantedItem::Pending {
            pair: None,
            reply_token: Some(token_1),
        },
        key2.id,
        None,
    )
    .await;
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    assert!(
        !inbox_ids(test, key1.id).await.contains(&stale_token),
        "a reply under the replaced token was kept"
    );
    assert_eq!(
        partstat_of(&key1.client, &organizer, "key2@example.com")
            .await
            .as_deref(),
        Some("ACCEPTED"),
        "a reply under the replaced token applied"
    );
```

(`token_1` is the `[u8; 16]` the step already holds from `key2_pair`; if `key2_pair` returns a reference or wrapper, copy the array.)

- [ ] **Step 3: `test_lease` cases A and E, the positive "waited on the lease" signal**

In case A, around `trigger_drain(key2.name, &app)` ("session 2's request"):

```rust
    let waited = std::time::Instant::now();
    assert_eq!(
        trigger_drain(key2.name, &app).await,
        207,
        "session 2's request"
    );
    assert!(
        waited.elapsed() >= Duration::from_secs(1),
        "session 2 did not wait on the lease ({:?})",
        waited.elapsed()
    );
```

(`LEASE_RETRIES * LEASE_RETRY_WAIT` is 20 × 50 ms = 1 s, `crates/common/src/za/lease.rs`.) The same three lines around case E's `trigger_drain(key1.name, &app1)` ("session 2's request"). `Duration` is already imported in the module.

- [ ] **Step 4: `test_errors` step 3, the orphan reply creates nothing**

After `assert_eq!(item.event_id, None);`:

```rust
    assert_eq!(
        uid_copies(test, key2.id, "za-drain-e4").await,
        0,
        "an orphan reply created an event"
    );
```

- [ ] **Step 5: `test` step 6, a request for another account's data drains the caller**

Before `// Cleanup:` in `drain::test`:

```rust
    // 6. A request for another account's data drains the caller's own inbox
    // (spec 6.1; the comment at the drain's call in
    // `crates/dav/src/request.rs`): key2's PROPFIND of key1's calendar
    // home, whatever it answers, drains key2's pending item first.
    let uid6 = "za-drain-6";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-6.ics"),
            [CONTENT_TYPE],
            invite(uid6, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pending_ids(test, key2.id).await.len(), 1);
    let status = HttpRequest::with_credentials(8899, key2.name, STRONG)
        .send_full(
            Method::from_bytes(b"PROPFIND").unwrap(),
            "/dav/cal/key1%40example.com/",
            None,
            None,
        )
        .await
        .status
        .as_u16();
    assert!(status < 500, "key2's request for key1's data answered {status}");
    assert!(
        pending_ids(test, key2.id).await.is_empty(),
        "a request for another account's data did not drain the caller"
    );
    assert_eq!(uid_copies(test, key2.id, uid6).await, 1);
```

Import `crate::utils::http::HttpRequest` and `hyper::Method` if the module lacks them. Add `uid6` to the cleanup's copy loop (`for uid in [uid, uid2, uid3, uid6]`) and `"drain-6"` to the organizer loop.

- [ ] **Step 6: `leak::test_drain`, the task queue after the reply and the scan while it is pending**

Before `assert_eq!(trigger_drain("key1@example.com", STRONG).await, 207);` (the organizer's drain of the reply):

```rust
    let pending_scan = scan(test, key1_id).await;
    assert!(
        pending_scan.violations.is_empty(),
        "leaks while the reply is pending:\n{}",
        pending_scan.violations.join("\n")
    );
```

After the `after_reply` assertion:

```rust
    assert_eq!(
        itip_tasks_for(test, key1_id).await + itip_tasks_for(test, key2_id).await,
        0,
        "the reply reached the task queue"
    );
```

If the pending-state scan reports the reply's visible token as a violation, stop and report (NEEDS_CONTEXT) with the violation text: the scan's rules decide whether that is by design.

- [ ] **Step 7: `za_variants::scheduling`, the moved canary in the stored archive**

After `assert_eq!(uid_copies(test, jane.account_id, "za-sched-1").await, 1);` in the update step, add the stored-archive check the earlier step makes (around line 461-467), for the moved title:

```rust
    let (copy_archive, _) =
        crate::za::dav_seal::raw_event(test, jane.account_id, &copy_relative).await;
    assert!(
        !String::from_utf8_lossy(copy_archive.as_bytes()).contains("sched-canary-moved"),
        "the drained update left the moved title in the clear"
    );
```

where `copy_relative` is whatever the earlier `raw_event` call in this function passes for jane's copy (reuse that expression; do not compute a new one).

- [ ] **Step 8: Unit test, the DELEGATED-FROM union**

In `drain.rs`'s `mod tests`:

```rust
    /// An address listed twice in one instance (one line with
    /// DELEGATED-FROM, one without), in either order.
    fn doubled(delegated_first: bool) -> ICalendar {
        let with = "ATTENDEE;PARTSTAT=NEEDS-ACTION;DELEGATED-FROM=\"mailto:att@example.com\":mailto:del@example.com\r\n";
        let without = "ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:del@example.com\r\n";
        let (first, second) = if delegated_first { (with, without) } else { (without, with) };
        parse(&format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:stale-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSEQUENCE:0\r\nORGANIZER:mailto:org@example.com\r\n{first}{second}END:VEVENT\r\nEND:VCALENDAR\r\n"
        ))
    }

    #[test]
    fn a_doubled_attendee_keeps_the_union_of_its_delegators() {
        let org = organizer();
        let merged = delegation_reply();
        let merged = itip_snapshot(&merged, &org, false).unwrap();
        let del = merged
            .components
            .get(&InstanceId::Main)
            .unwrap()
            .attendees
            .iter()
            .find(|attendee| attendee.email.email == "del@example.com")
            .unwrap();
        for delegated_first in [true, false] {
            let before = doubled(delegated_first);
            let before = Attendance::of(&itip_snapshot(&before, &org, false).unwrap());
            assert!(
                !before.is_new(None, del),
                "a doubled attendee (delegated line first: {delegated_first}) counted as new"
            );
        }
    }
```

(`delegation_reply` lists `del@example.com` with `DELEGATED-FROM=att@example.com`, so with the union `del`'s delegators before the merge are `{att}` and the merged attendee's set is a subset; with the last line winning, one order yields an empty set and `is_new` would be `true`.) Run `cargo test -p groupware`: 81 passed.

- [ ] **Step 9: Run the suites**

`cargo fmt --all --check`; `cargo check -p tests --tests`; the za suite; `ZA_KEY_ACCOUNTS=1 ... webdav_tests`.
Expected: all green. For the za suite, confirm in the output that steps 10 (`test_reply`), 6 (`test`) and the lease timing assertions passed (no panic text).

- [ ] **Step 10: Commit**

```bash
git add tests/src/za/drain.rs tests/src/za/leak.rs tests/src/webdav/za_variants.rs crates/groupware/src/calendar/drain.rs
git commit -m "Drain tests: sender resolution, the replaced token, lease waits, the other-account request, the union"
```
(with the two trailer lines).

### Task 5: Comment and doc nits

**Files:**
- Modify: `crates/common/src/za/lease.rs:80,217`
- Modify: `crates/store/src/dispatch/lookup.rs:427`
- Modify: `crates/common/src/za/test_hooks.rs` (the module doc's stray blank line; `set_drain_retry_override`)
- Modify: `crates/http/src/request.rs:1036-1039` (`za_jmap_untraced` doc)

**Interfaces:** none.

Comments and one cast; behaviour unchanged. Line numbers are those at `b13c0d64`; find each by its text.

- [ ] **Step 1: Name the sweep**

`lease.rs`, the doc of `lease_key_range`: "(plan 10's sweep iterates it)" becomes "(`sweep_expired_uid_leases` iterates it)". The doc of `release`: "already absent (plan 10's sweep removed the expired lease, ..." becomes "already absent (`sweep_expired_uid_leases` removed the expired lease, ...". `lookup.rs:427`: "lease; plan 10's sweep is to clear these with an assertion." becomes "lease; `common::za::lease::sweep_expired_uid_leases` clears these with an assertion."

- [ ] **Step 2: `test_hooks.rs`**

In the module doc, remove the empty `//!` line between the lease-duration override bullet and the `DRAIN_LOOKUP_PAUSE` bullet (the one that splits the rustdoc list). In `set_drain_retry_override`, `secs as i64` becomes `i64::try_from(secs).unwrap_or(i64::MAX)` (a value past `i64::MAX` is still "a deferral", never a negative that reads as `None`).

- [ ] **Step 3: `za_jmap_untraced`**

The doc comment's last sentence becomes: "Fails closed: a failed lookup counts as a key account, unlike `Server::za_is_key_account`'s gates, which fail open on an unknown id (`crates/common/src/auth/vault.rs`); a trace is the thing to withhold when in doubt."

- [ ] **Step 4: Checks and commit**

`cargo fmt --all --check`; the product check; `cargo check -p tests --tests`.

```bash
git add crates/common/src/za/lease.rs crates/store/src/dispatch/lookup.rs crates/common/src/za/test_hooks.rs crates/http/src/request.rs
git commit -m "Zero-access comments: name the sweep, the fail-closed trace gate, a hook cast"
```
(with the two trailer lines).

### Task 6: Mutation runs, the outcome note, CLAUDE.md

**Files:**
- Create: `docs/superpowers/plans/2026-10-16-zero-access-plan11-outcome.md`
- Modify: `docs/superpowers/plans/2026-10-15-zero-access-plan10-outcome.md` (one line at the top of "Deferred findings")
- Modify: `CLAUDE.md` (the "Where release 2 lives" and the test-harness paragraphs)

**Interfaces:**
- Consumes: Task 3 step 11's five kill lines (from its report); Task 2's case (b) assertion; Task 4 step 8's unit test.
- Produces: the note later plans read.

- [ ] **Step 1: Mutation runs**

One at a time, each reverted with `git checkout -- <file>` and an empty `git status --short crates/` after it. Record the first failing assertion (`file:line`, message, left/right) for each:

| Id | Check | Edit | Expected catcher |
|----|-------|------|------------------|
| M40 | The create's fan-out fence (spec 5.4), `crates/dav/src/calendar/update.rs` | the fence argument becomes `None` | `uid_lease` case (b): "the held PUT delivered after its lease was taken over" (1 vs 0) |
| M41 | The permanent deferral (spec 10), `drain.rs` `drain_pass` | the `Suspended` arm defers by `now + drain_retry_secs()` | `drain_limits` step 1: "the backoff lifted a permanent deferral" |
| M42 | Deferral on an enumeration error (spec 10), `drain.rs` `drain_pass` | the `Err` arm of `enumerate`'s match drops `state.defer_until(..)` | `drain_limits` step 2: "an enumeration error set no deferral" |
| M43 | The failure limit (spec 6.2), `session.rs` `record_failure` | returns `false` always | `drain_limits` step 3: "the skipped item did not let the pass continue" |
| M44 | The UID order behind a skipped item (spec 6.2), `session.rs` `blocks_uid` | returns `false` always | `drain_limits` step 3: "the CANCEL overtook its REQUEST" |
| M45 | The cap (spec 6.2), `drain.rs` `drain_items` | the `if handled == DRAIN_PASS_CAP` check removed | `drain_limits` step 4: "the pass ignored the cap" |
| M46 | The union of delegators (plan 10 fix wave M2), `drain.rs` `Attendance::of` | `.or_default().extend(..)` becomes `.insert(.., delegators(attendee))` on the entry (last line wins) | `cargo test -p groupware`: `a_doubled_attendee_keeps_the_union_of_its_delegators` |

The za suite runs M40-M45 (about 75 s each); M46 is a unit test (seconds).

- [ ] **Step 2: Full verification on the branch head**

In order, one at a time: `cargo fmt --all --check`; the product check; `cargo check -p tests --tests`; `cargo test -p vault`; `cargo test -p groupware`; `cargo test -p common za::lease`; `cargo test -p common auth::vault`; the za suite; the za suite with `BLOB_STORE=FileSystem`; `webdav_tests` plain; `webdav_tests` with `ZA_KEY_ACCOUNTS=1`; `cargo build --release -p stalwart --no-default-features --features rocks` (about 16 minutes; run it in the background with no other cargo command alongside and record its time and that the fork's files produced no warning).

- [ ] **Step 3: The outcome note**

Create `docs/superpowers/plans/2026-10-16-zero-access-plan11-outcome.md` in the shape of the plan 10 note, with these sections:
- **Where things stand**: the branch, every commit by task, and each suite's result and time from Step 2.
- **What changed and why**: one paragraph per task, naming the spec sections and the decisions of 2026-10-16 (5.1 owner rule; 5.4 fence, residual accepted, no compensation, no lease on updates; 6.2 cap and skipped items; 6.5 history for iTIP-level parse errors; 10 the permanent deferral and the early-error deferral).
- **Rulings**: every `Ruling:` line of the ledger (`.superpowers/sdd/2026-10-16-zero-access-11-followups/progress.md`), verbatim.
- **Mutation runs (plan 11)**: the table of Step 1 with the observed lines; "No mutation survived" or the exception.
- **Deferred findings**: what stays open after this plan: the delegate whose PARTSTAT is reset without a DELEGATED-FROM gain (an upstream merge-semantics question; see the plan 10 note); a pending-item index if inboxes grow; the drain test module split; anything a review of this plan deferred.
- **Merge hotspots**: unchanged from plan 10, plus `crates/dav/src/calendar/update.rs` (the create branch's fan-out call now takes the fence).

- [ ] **Step 4: The plan 10 note and CLAUDE.md**

At the top of the plan 10 note's "Deferred findings" section, add one line: "Plan 11 (`2026-10-16-zero-access-11-followups.md`, outcome note `2026-10-16-zero-access-plan11-outcome.md`) resolved the three open items for Jay (5.1, 5.4, the fan-out residual on the create side) and the follow-up list's minors 1-4, 6, 7 and 9, the Task 3-5 test minors it names, and the PR #36 review's M1, M2, M6/M10 and N1; the rest stays as listed."

In `CLAUDE.md`, the "Where release 2 lives" paragraph: after "`DRAIN_RETRY_SECS = 30` is a constant)", add "; a pass handles at most `DRAIN_PASS_CAP` (32) items, an item that fails `DRAIN_ITEM_FAILURE_LIMIT` (3) times in a session is skipped for the session with later items of its UID waiting behind it, and a private key that does not open suspends the session's drain (`vault::session::PassState`)". After "(`za_fan_out` takes an optional lease fence that every recipient item's write asserts)", add ": the drain's delegating merge and the PUT create pass theirs". In the test-harness paragraph of the "Architecture" section, the hooks list gains `PRIVATE_KEY_FAIL_ONCE`, `DRAIN_ENUMERATE_FAIL_ONCE`, `DRAIN_FAIL_ITEM`, and the drain modules list gains `drain_limits.rs`. Do not touch the status paragraphs (the controller updates them at merge time).

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/plans/2026-10-16-zero-access-plan11-outcome.md docs/superpowers/plans/2026-10-15-zero-access-plan10-outcome.md CLAUDE.md
git commit -m "Plan 11 outcome note, mutation runs, CLAUDE.md"
```
(with the two trailer lines).
