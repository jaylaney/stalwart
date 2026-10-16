# Zero-access calendar: plan 10 outcome

Written 2026-10-15 at the end of plan 10 (`2026-10-15-zero-access-10-drain.md`).
Plan 10 completes release 2 (invitations between key accounts). Read it before release 3.

## Where things stand

- Commits on `release2-plan10-drain`, cut from `main` at `9a1855e4`; plan and its two review rounds `3da02cac`, `3b178afa`, `318c4012`. Not pushed.
  - Task 1 `ce1f42de` (drain state, hooks, lease sweep), fix round `8e34acca` (`take_flag`, the suite clears `DRAIN_SKIP_ALL`);
  - Task 2 `eebd41ca` (the drain), fix round `1e4e7c2e` (a busy lease stops the pass, a delegating REPLY merge holds the UID lease, the pair copy in `Zeroizing`);
  - Task 3 `d62a62f9` (the `copy_href` fix), `c31320ef` (reply, stale reply, delegation and adoption tests);
  - Task 4 `df0c6c74` (lease cases A-E, the cursor and the backoff);
  - Task 5 `55f2afe2` (item errors, odd inboxes, the drain in the leak scan);
  - Task 6 `86a53361`, `f0a4f2f3` (gates reopened, key-mode variant with the update step, the OPTIONS tracing check reworded, checklist and CLAUDE.md);
  - Task 7: `11656ec0` (plan 9's lease case (d) waits for the index task; see "Rulings"), then the commit of this note and the spec's settled names (`a9b2c2e5`, `7d5c66b3`);
  - final fix wave (see "Final review"): `22902e0a` (a REPLY from a sender holding no pair is history; `import` re-reads its item under the lease; `test_reply` step 6 and `test_lease` case F), then the documentation commit (spec 3.3, 6.2, 6.5, 11, 13; this note; CLAUDE.md).
- The suites run in Task 7 Step 2, in order, one at a time:
  - `cargo test -p vault -p groupware`: groupware 80 passed, vault 31 passed (21 s).
  - `cargo test -p common za::lease`: 10 passed.
  - `cargo test -p common auth::vault`: 14 passed.
  - `za_tests`, first run, on `f0a4f2f3` (before `11656ec0`): failed at the final `assert_is_empty` (`tests/src/utils/cleanup.rs:440`, "Store is not empty"), every sub-module having passed: 13 search index keys of the plain account's event from plan 9's lease case (d), `za-lease-plain`. A rerun for classification passed (68.15 s). The cause is a hypothesis, not reproduced (see "Rulings"); `11656ec0` removes the only identified writer of those keys, and three za runs have passed since: with the edit uncommitted, plain on `11656ec0` (below) and with `BLOB_STORE=FileSystem` (below).
  - `za_tests` on `11656ec0`: 1 passed in 64.29 s (leak scan: 2099 records, 52 archives, 8 blobs, 988 search index records (644 of the account, none calendar), 5 events, 3 calendars, 4 items, 4 emails; negative control caught 29 violations).
  - `za_tests` with `BLOB_STORE=FileSystem`: 1 passed in 63.79 s (leak scan: 2093 records, 52 archives, 8 blobs, 988 search index records, 5 events, 3 calendars, 4 items, 4 emails; negative control caught 29 violations).
  - `webdav_tests`: 1 passed in 16.68 s. `cal_itip` passed on the first run.
  - `webdav_tests` with `ZA_KEY_ACCOUNTS=1`: 1 passed in 21.08 s, including the key-account scheduling variant (drained copy, update, cancel).
  - `cargo fmt --all --check`: clean.
  - Product compile check (`cargo check -p stalwart --no-default-features --features rocks`, filtered to the fork's files): clean; the upstream warning totals unchanged (`store` 1, `common` 18, `jmap` 3, `services` 2).
  - `cargo build --release -p stalwart --no-default-features --features rocks`: finished in 15m 41s; no warning in the fork's files, the same upstream warning totals.
- The final fix wave's verification, on `22902e0a`, one at a time:
  - `cargo fmt -p groupware -p tests -- --check`: clean.
  - `cargo test -p groupware`: 80 passed.
  - Product compile check (filtered to the fork's files): clean; upstream warning totals unchanged (`store` 1, `common` 18, `jmap` 3, `services` 2).
  - `cargo check -p tests --tests`: clean.
  - `za_tests`: 1 passed in 69.55 s (leak scan: 2140 records, 53 archives, 8 blobs, 988 search index records (644 of the account, none calendar), 5 events, 3 calendars, 4 items, 4 emails; negative control caught 29 violations).
  - `webdav_tests` with `ZA_KEY_ACCOUNTS=1`: 1 passed in 20.92 s.
  - Two mutation runs (M38, M39 below), each caught by the new test.
- What now exists:
  - the drain (`crates/groupware/src/calendar/drain.rs`, `ZaDrain::za_drain`), called in `handle_dav_request` for every DAV request whose token holds the account's session keys; import under the creation lease, merge under the event's archive assertion, pair adoption, REPLY token check, stale-reply check, delegate fan-out under the organizer's UID lease, item rewrite resealed with the pending bit cleared, in one store write per item;
  - the drain state in `SessionKeys` (cursor, deferral, one-pass lock), in memory only;
  - the lease sweep, `sweep_expired_uid_leases`, in the maintenance task's `PurgeData` arm after `purge_in_memory_store`;
  - the hooks `DRAIN_LOOKUP_PAUSE`, `DRAIN_LEASE_PAUSE`, `DRAIN_FAIL_ONCE`, `DRAIN_SKIP` (`DRAIN_SKIP_ALL`), `DRAIN_RETRY_OVERRIDE`;
  - the reopened gates: `calendar-auto-schedule` in every DAV OPTIONS header, the schedule inbox and outbox URLs on a key principal;
  - the drain modules of the za suite (`drain::test`, `test_reply`, `test_adoption`, `test_lease`, `test_cursor`, `test_errors`), `leak::test_drain`, the lease sweep test, and the key-mode `scheduling` variant's drained copy, update and cancel; manual checklist item 13 is the two-client invitation flow.

## Names settled (spec 13)

The plan's table, with the changes the execution made:

- **Pass outcome**, amended (Task 2 review): a busy creation lease stops the pass at that item (`drain_items` returns `Ok(false)` at once); the pass defers with the cursor unchanged, as for any item left pending. The plan had the pass continue.
- **Delegating REPLY merge**, added (Task 2 review): a REPLY merge whose fan-out would deliver new items takes `ZaUidLease::acquire(store, organizer account, UID)` first, re-reads the item under it, asserts the fence first in the batch and commits with the lease; busy: the pass stops, traced with the import's busy detail.
- **Trace details**, one added: `zero-access: delegate invitation not built` (`Calendar(ItipMessageError)`, `Reason` = a fixed variant name) when upstream's organizer builder fails for the delegates; the reply still applies.
- **Hooks**: `take_drain_fail` and `take_commit_fail` share the private `take_flag` (Task 1 review).

| Name | Value | Where |
|---|---|---|
| Drain entry | trait `ZaDrain` on `Server`, `za_drain(&self, keys: &Arc<SessionKeys>) -> ()`; called in `handle_dav_request` before `dispatch_dav_request` when `access_token.za_keys_for(access_token.account_id())` is `Some` | `crates/groupware/src/calendar/drain.rs`, `crates/dav/src/request.rs` |
| Drain state | `ZaDrainState` in `SessionKeys` (`SessionKeys::drain()`): `cursor()` / `complete(snapshot)`, `retry_after()` / `defer_until(secs)`, `lock()` (a `tokio::sync::Mutex<()>`) | `crates/vault/src/session.rs` |
| Mid-session trigger | spec 6.2 option 1: `fetch_dav_resources(.., SyncCollection::CalendarEventNotification).highest_change_id` compared with the cursor on every DAV request of a key account; no in-memory counter, no `CacheInvalidation` | `drain.rs` |
| Retry backoff | `DRAIN_RETRY_SECS: u64 = 30`, a constant (no `ZA_DRAIN_RETRY_SECS`); test-mode override `set_drain_retry_override` | `drain.rs`, `test_hooks.rs` |
| Pass outcome | all items done: `complete(snapshot)`; an item left pending without an error (a busy lease, which stops the pass) or a transient error (which aborts it): `defer_until(now + DRAIN_RETRY_SECS)`, cursor unchanged | `drain.rs` |
| Lease sweep | `sweep_expired_uid_leases(store) -> trc::Result<usize>` = `collect_expired_uid_leases(store, now)` then `clear_uid_leases(store, expired)`; each clear asserts `AssertValue::Hash(xxh3_64(value read))`; a malformed value counts as expired; called in the `PurgeData` arm of `store_maintenance` after `purge_in_memory_store` | `crates/common/src/za/lease.rs`, `crates/services/src/task_manager/maintenance.rs` |
| Sweep on Redis + data store | not tested in a container: the sweep reads and writes the data store whatever the lookup backend | ruling |
| Hooks | `DRAIN_LOOKUP_PAUSE`, `DRAIN_LEASE_PAUSE`, `DRAIN_FAIL_ONCE`, `DRAIN_SKIP` (`DRAIN_SKIP_ALL = u32::MAX`), `DRAIN_RETRY_OVERRIDE` | `crates/common/src/za/test_hooks.rs` |
| Trace details | `zero-access: drain pass failed`, `zero-access: drain creation lease busy` (also the delegating merge's busy lease), `zero-access: drain commit lost its assertion`, `zero-access: undecodable inbox item deleted`, `zero-access: inbox item kept as history`, `zero-access: delegate invitation not built`, `zero-access: reply token mismatch` | `drain.rs` |
| Item flags after the drain | import: `0`; merge: `EVENT_NOTIFICATION_IS_CHANGE`; kept as history: `0`; `event_id` = the event's document id when one was found or created, else `None`; `created` kept, `modified` = now | `drain.rs` |
| Stale reply | a REPLY is stale when any of its instances has a SEQUENCE lower than that of the governing component (the same instance's override, else the latest `RANGE=THISANDFUTURE` override at or before it, else the master; absent = 0); rejected as `OutOfSequence`, kept as history | `drain.rs` |
| Sender check | candidates: the ORGANIZER of a REQUEST or CANCEL, every ATTENDEE of a REPLY; those resolving quietly (`za_account_id_from_email`) to `ChangedBy::PrincipalId` are the sender's addresses; none, or a `CalendarAddress` sender: `SenderIsNotOrganizerNorAttendee`, kept as history. For a REPLY: no sender address holding a pair in the event's bundle is `SenderIsNotParticipant`, kept as history (final review F1); otherwise the replier is the sender's address whose pair holds the token, and none matching is a forgery | `drain.rs` |
| Quota at the drain | only `Limit(Quota)` / `Limit(TenantQuota)` is `QuotaExceeded`; any other error keeps the item pending | `drain.rs` |
| Permanent errors | any `open_pending_item` error, an unparsable or missing METHOD, a METHOD other than REQUEST, REPLY, CANCEL: the item is deleted | `drain.rs` |
| Missing reply token | treated as a token mismatch (deleted, security trace) | `drain.rs` |
| Which copy merges | the lowest document id among the account's events with the UID | `drain.rs` |
| Gates reopened | `OPTIONS` answers upstream's `DAV` header for every request (`za_is_key_account_request` removed); the principal's key-account `ScheduleInboxURL` / `ScheduleOutboxURL` arm removed | `crates/http/src/request.rs`, `crates/dav/src/principal/propfind.rs` |
| `cal_scheduling` in key mode | upstream's test still does not run in key mode; the `scheduling` variant asserts the drained copy, the update and the cancel | `tests/src/webdav/za_variants.rs` |
| Test helpers | `za_app_password`, `trigger_drain`, `pending_ids`, `copy_href` | `tests/src/utils/za.rs` |

## Rulings

Each with its reason and its cost if wrong.

- Plan corrections, before Task 2:
  - Task 2 implements `body`, `relative` and `stored_item` (what its code and Task 3's consumer list name), not the stale `pending_item_count` of its interface line. Cost: none (Task 1's `pending_ids` covers the count).
  - Task 3's `accept(client, href)` interface line is stale; `set_partstat` and `partstat_of` are the helpers, and nothing downstream calls `accept`. Cost: none.
- Task 1:
  - `take_drain_fail` and `take_commit_fail` fold into one private `take_flag`: the plan's duplication was a transcription convenience, not a design. Cost: none (test mode only).
  - The suite clears `DRAIN_SKIP_ALL` after its final `assert_is_empty`, so a process-global hook cannot leak into a later suite in the same binary. Cost: none.
- Task 2:
  - A busy creation lease stops the pass (`drain_items` returns `Ok(false)` at once), because a pass that went on would process a later CANCEL or update for the same UID before the REQUEST it depends on (the CANCEL hits `EventNotFound` and is consumed as history). The simplest rule that keeps per-UID order, with no new state; spec 6.3 said "an item left pending" without saying the pass continues, and now says it stops. Cost: items for other UIDs wait up to one backoff (30 s) behind a lease that itself lives at most `ZA_UID_LEASE_SECS`.
  - A REPLY merge that must fan out to new delegates first takes the per-UID creation lease (organizer account, UID), re-reads the item under it and skips if another session finished it, asserts the fence first in the batch and commits with the lease. Without it two sessions of the organizer could both fan out the delegate's REQUEST with different tokens, and the loser's delivered item would make the delegate adopt a token the organizer's bundle lacks, so the delegate's replies would be dropped as forgeries. It serializes the sessions through an existing primitive; Task 4's case E covers it. Cost: an organizer PUT of the same UID in flight defers the REPLY merge by one backoff; delegation replies take one lease.
  - The pair copy the merge makes (`pair.to_vec()`) lives in a `Zeroizing` buffer (release 1's rule for tokens). Cost: none.
  - The merge's busy-lease trace reuses `zero-access: drain creation lease busy` although no creation is involved, to keep the settled names. Cost: a misleading detail in a trace.
  - `ItemCtx::commit` releases rather than abandons the lease after a lost fence, so that path also emits the lease's "not released" error trace: the commit cannot tell which assertion failed, and abandoning on an item-assertion loss would hold the lease for its lifetime. Cost: one extra error trace.
- Task 3:
  - The `copy_href` fix in `tests/src/utils/za.rs` (outside the task's file list) is accepted: Task 1's helper picked any resource with the event's document id, container or not, and calendars number documents separately from events, so key3's default calendar collided; the fix keeps the href that does not end in `/`. Cost: a test helper only.
- Task 5:
  - `test_errors` step 4 makes its second copy of one UID by a PUT of the first copy's body into a second calendar: upstream's COPY within one account (`crates/dav/src/common/copy_move.rs:507-522`) links a second name to the same document and answers 201, so the plan's "COPY refuses" trigger never fired and its fallback is the only way to get two documents with one UID. Cost: the linked-COPY shape (one document, two calendar names) is untested; the drain merges by document id, so it sees one copy there.
- Task 6:
  - The fork's key-mode branches in `tests/src/webdav/basic.rs` and `tests/src/webdav/principals.rs` asserted the closed gates and are reverted to upstream's assertions; both files are upstream's again. The za suite's OPTIONS tracing check now reads as the login's positive control plus "the OPTIONS credential never reaches the trace", since OPTIONS is answered before authentication on upstream's path. Cost: none beyond the retired mutations below.
- Carried warning from Task 1, resolved: `copy_href` reads the DAV resource cache and expects the drained copy there. The drain's event write reaches that cache through the change log: `fetch_dav_resources` refreshes from it on every request, and `commit_batch` does not invalidate the cache itself. The Task 2-5 tests read drained copies through `copy_href` without any extra invalidation. Cost if wrong: none seen; a drained copy missing from the cache would fail those tests.
- Task 7:
  - Plan 9's lease case (d) (`tests/src/za/uid_lease.rs`) is taken to race the index task: the plain account's event is indexed by the background task and the case deleted the calendar at once, so an index build that had read the archive could write its keys after the DELETE's unindex. That mechanism is a hypothesis: the failure was seen once, during plan 10's verification, and not reproduced. The fix waits for tasks before the calendar delete (`11656ec0`), which removes the only identified writer of those keys; three za runs have passed since (the edit uncommitted, plain on `11656ec0`, and with `BLOB_STORE=FileSystem`). Cost if wrong: a flake remains.

## Mutation runs (plan 10)

All runs in Task 7 Step 1, one at a time, each reverted with `git checkout --` and followed by an empty `git status --short crates/`. None was committed. Line numbers are the committed files' at `f0a4f2f3`. Where the first failure is an earlier assertion than the plan expected, the expected one is named too.

| Id | Check, `file:line` | Edit | First failure | Command |
|----|--------------------|------|---------------|---------|
| M29 | The token check (spec 3.3), `crates/groupware/src/calendar/drain.rs:637` (`merge`) | the `Incoming::Reply(token)` arm's `sender_addresses.iter().find(..)` becomes `sender_addresses.first()` | `tests/src/za/drain.rs:476:5`: `a forged reply was kept` (`test_reply` step 2) | za suite |
| M30 | The lease's fence on the import (spec 6.3 step 3), `drain.rs:579` (`import`) | delete `lease.lease().assert_in(&mut batch);` | `drain.rs:1133:5`: `the expired holder committed` (left 2, right 1) (`test_lease` case C) | za suite |
| M31 | The lease on the import (spec 6.3 step 1), `drain.rs:485` (`import`) | the acquire takes `&format!("{uid}#{}", ctx.document_id)`, so two items never contend | `drain.rs:982:5`: `session 2 imported past the lease` (left 0, right 2) (`test_lease` case A; the plan's alternative) | za suite |
| M32 | Pair adoption on merge (spec 3.3, 6.4), `drain.rs:729-736` (`merge`) | the `Incoming::Request(Some(pair))` arm becomes `let _ = pair;` | `drain.rs:822:5`: `the copy did not adopt the new token` (`test_adoption` step 1) | za suite |
| M33 | The stale-reply check (spec 6.4), `drain.rs:667` (`merge`) | `is_stale_reply(&snapshots, &itip_snapshots)` becomes `false` | `drain.rs:529:5`: `a stale reply is not a change` (left 2, right 0): the decline merged and flagged the item as a change (`test_reply` step 3; the expected `the stale decline applied` is at `:543`) | za suite |
| M34 | The sender check (spec 6.4 step 2), `drain.rs:337` (`drain_item`) | `== Some(*sender_id)` becomes `== Some(*sender_id) \|\| true` | `drain.rs:1566:5`: `assert_eq!(item.event_id, None)` (left `Some(26)`, right `None`): the mismatched sender's REQUEST was imported (`test_errors` step 2; the expected `a mismatched sender's invitation was imported` is at `:1575`) | za suite |
| M35 | The cursor rule (spec 6.2), `drain.rs:174-181` (`drain_pass`) | the `Ok(false)` and `Err` arms call `state.complete(snapshot)` instead of `state.defer_until(..)` | `drain.rs:1144:5`: `assertion failed: pending_ids(test, key2.id).await.is_empty()`: after case C's lost commit the cursor had moved, so the next request did not retry the item (`test_lease` case C, which runs before `test_cursor`'s expected `the retry did not drain`) | za suite |
| M36 | The sweep's assertion (spec 6.3 step 4), `crates/common/src/za/lease.rs:126` (`clear_uid_leases`) | delete `batch.assert_value(..)` | `crates/common/src/za/lease.rs:567:9`: `the_sweep_keeps_a_lease_retaken_after_its_scan`, `clear_uid_leases(..) == 0` (left 1, right 0), the line before `the sweep removed the new holder's lease` | `cargo test -p common za::lease` (9 passed, 1 failed) |
| M37 | The delegating merge's lease (spec 6.3, "Concurrency"), `drain.rs:769-811` (`merge`) | the `if !messages.is_empty() { .. }` block (acquire, re-read, `reply_lease = Some(lease)`) deleted, `reply_lease` typed `Option<ZaUidLeaseGuard>` and left `None`, so `za_fan_out` runs unleased and the commit gets `None` | `drain.rs:1252:5`: `key3 received an item before session 1's fan-out wrote one` (left 1, right 0): session 2 fanned out while session 1 was parked (`test_lease` case E). The addendum's predicted message, "key3's inbox gained exactly one pending item", does not exist; the assertion it describes is `key3 holds exactly one invitation` (`tests/src/za/drain.rs:1265`), which comes later and would see two | za suite |
| M38 | No pair is history (final review F1), `drain.rs:651-663` (`merge`, at `22902e0a`) | the `let Some(pairs) = .. else` branch returns `ctx.drop_forgery()` instead of `ctx.reject(.., SenderIsNotParticipant(..))` | `tests/src/za/drain.rs:96:28`: `item 13 is gone` (`stored_item`, from `test_reply` step 6) | za suite (final fix wave) |
| M39 | The re-read under the import's lease (final review F2), `drain.rs:506-509` (`import`, at `22902e0a`) | the `if !ctx.still_pending().await? { .. }` block deleted | `tests/src/za/drain.rs:1452:5`: `session 2 was deferred after the race` (`test_lease` case F) | za suite (final fix wave) |

No mutation survived.

Retired by plan 10 (gates removed), from the plan 3 note's mutation table (plan 7): M11 (schedule URLs hidden, `crates/dav/src/principal/propfind.rs`) and M12 (OPTIONS header, `crates/http/src/request.rs`, which mutated `za_is_key_account_request`). Task 6 (`86a53361`) removed both branches and the function, so neither edit has a target. M12's `za_tests` catcher was the tracing check "OPTIONS authentication failure not reported", now reworded (Task 6); its key-mode catcher was the fork's branch of `tests/src/webdav/basic.rs`, now upstream's. The plan 3 note is unchanged.

## Merge hotspots

Upstream files plan 10 changed, where an upstream merge will collide:
- New: `crates/dav/src/request.rs` (`handle_dav_request`, the drain trigger between header parsing and `dispatch_dav_request`); `crates/services/src/task_manager/maintenance.rs` (the `PurgeData` arm, the sweep after `purge_in_memory_store`); `crates/groupware/src/calendar/mod.rs` (the `drain` module).
- Back to upstream: `crates/http/src/request.rs` (the OPTIONS arm; `za_is_key_account_request` removed) and `crates/dav/src/principal/propfind.rs` (the key-account schedule URL arm removed).
- A silent dependency: the drain's `import` and `merge` (`crates/groupware/src/calendar/drain.rs`) mirror by hand the import branch of `itip_ingest` and `commit_itip_merge` (`crates/groupware/src/calendar/itip.rs`): the size cap, the quota check, the alarm task swap, the schedule tag and the `<now>_<random>.ics` naming. An upstream change to those functions compiles without touching the drain, so an upstream merge that changes them needs a re-read of both.

## Final review

The whole-branch review (`9a1855e4..7d5c66b3`, 0 critical, 5 important, 9 minor, "with fixes") and its rulings:

- F1 (important 1), fixed in `22902e0a`. A REPLY from an attendee with no pair was deleted as a forgery with a security trace, so a removed attendee who declines from its cancelled copy (a routine client action: `attendee_decline` builds a declined REPLY for any attendee not yet DECLINED) raised an attack trace at the organizer and lost the item. Ruling: a REPLY whose sender holds no pair in the event's bundle is rejected as `SenderIsNotParticipant` and kept as history (flags 0, the usual `Calendar(ItipMessageError)` trace with a fixed reason); `drop_forgery` stays for a participant (an address holding a pair) whose token is missing or different. This resolves spec 3.3's two sentences: "never invited" and "removed" both mean "no pair" and both become history; spec 3.3 and 6.5 now say so. `merge`, `drain.rs:650-671`. `test_reply` step 6 (`tests/src/za/drain.rs:700-794`): key1 invites key2, key2 accepts, key1 removes key2, key2 drains the CANCEL (cancelled copy, pair kept) and declines from it; upstream's PUT built the REPLY (nothing planted); key1's drain keeps the item, pending bit 0, `event_id` the organizer's event, both copies unchanged. Cost if wrong: a never-invited sender's REPLY stays in the inbox as history instead of being deleted (harmless, visible to the organizer).
- F2 (important 2), fixed in `22902e0a`. `import` did not re-read the item under the lease, so in the benign two-session race (two devices at login, or two nodes) the loser acquired the lease after the winner's commit, found the event, merged against a stale item archive, lost the assertion, traced "drain pass failed" at error level and deferred the session 30 s. Ruling: `import` re-reads the item right after `acquire` and returns `Ok(true)` when it is gone, no longer pending or changed, as the delegating merge already did; both now call `ItemCtx::still_pending` (`drain.rs:909-921`, used at `:506` and `:820`). `test_lease` case F (`tests/src/za/drain.rs:1394-1459`): session 1 parked at `DRAIN_LEASE_PAUSE`, session 2 inside `acquire`'s retries, session 1 released inside them; one copy, nothing pending, no lease; with the default backoff in force a second invitation drains on session 2's next request. Cost if wrong: one more store read per import.
- F3 (important 3), deferred. The delegating merge's fan-out writes outside the fenced batch (the Task 2 residual under "Deferred findings") stays a follow-up together with plan 9's F2: one problem on the PUT side (update PUTs of events that carry pairs taking the UID lease, or compensating deletion). Cost if wrong: a delegate's replies dropped as forgeries for one backoff in a rare double fault.
- F4 and F5 (important 4 and 5), fixed in the documentation commit, with the Task 7 doc nits: spec 11 now says the suite calls `assert_is_empty` once at its end after `destroy_key_accounts` and every module removes what it creates; spec 6.2 names the 30 seconds once; spec 13's Names bullet is wrapped and its `cal_scheduling` item marked settled by section 11; spec 6.5 carries F1's sentence; this note and CLAUDE.md carry the merge hotspots above and the hand-mirrored `itip_ingest` / `commit_itip_merge` dependency.
- The nine minors and the triage lines marked "follow-up" are deferred to one follow-up (ruling: one fix wave only), listed below.

Follow-up list (the final review's minors, then its "follow-up" triage lines not already under "Deferred findings"):
- Minor 1, no poison-pill bound: a deterministic non-assertion `Err` on one item blocks every later item of the inbox, retried every 30 s with an error trace (`drain_items` aborts at the first `Err`); the owner can delete the item by href, but nothing names it. A per-item failure counter in `ZaDrainState`, or a troubleshooting line in the checklist.
- Minor 2, no per-pass cap: a pass drains every pending item inside the triggering request under the session lock; a cap with a "more to do, no backoff" outcome would bound the request (cancellation is already safe).
- Minor 3: store errors before the match (`fetch_dav_resources`, `itip_ids`, `read_item` in `drain_pass`) set no deferral, so a persistent store fault traces "drain pass failed" on every DAV request of every key session.
- Minor 4: `is_stale_reply` counts an absent SEQUENCE as 0, so a client that strips SEQUENCE from the attendee's copy would have every reply to a rescheduled event kept as history; a client-behaviour risk for the manual checklist.
- Minor 5: `Step::Nothing` writes history with flags 0 where upstream writes no inbox item for `MergeResult::None`; the names table's "merge: `EVENT_NOTIFICATION_IS_CHANGE`" does not mention it.
- Minor 6: no test plants a REPLY carrying the real token under another principal's `changed_by` (the REPLY sender resolution has no catcher; the Task 3 deferred minor).
- Minor 7: no test issues a DAV request for another account's data to show that it drains the caller's inbox, as the comment in `crates/dav/src/request.rs` promises.
- Minor 8: CLAUDE.md's status sentence says plan 10 is "not yet merged" and is about 1,900 characters; split it into a status line and a locations line at merge time.
- Minor 9: the `za_jmap_untraced` doc (`crates/http/src/request.rs`) lost its comparison target ("unlike the fail-open `za_is_key_account_request`") and now reads as if fail-closed were unusual.
- (Task 2 triage) A recorded private-key failure: `defer_until(u64::MAX)` on that branch instead of the 30 s churn (already listed below as a spec 10 divergence).
- (Task 4 triage) A positive "waited on the lease" signal in cases A and E (an elapsed-time assertion); case F asserts that session 2 had not finished 300 ms into its wait, which is not an elapsed-time bound.
- (Task 5 triage) `test_errors` step 3: `uid_copies("za-drain-e4") == 0`; the leak test's `itip_tasks_for` check after the reply and a key1 scan while the reply is pending.
- (Task 6 triage) The moved canary is not re-checked in the stored archive (one `raw_event` call).
- (Tasks 1 and 3 triage) The "plan 10's sweep" comments, `test_hooks.rs`'s stray `//!` line and `secs as i64`, Step 1's "trigger 1" comment, the planted REPLY with the real token from another principal, the old token rejected after a re-add: already under "Deferred findings".

Dropped by the final review's triage, with its reasons:
- `maintenance.rs`'s `?` propagation: consistent with the neighbouring steps; a failed sweep should be loud.
- The per-request since-query and full inbox read on any change: spec 6.2 accepts the cost; revisit with a pending index if inboxes grow.
- `MissingUid` / `NoSchedulingInfo` kept as history: history is the safer choice and nothing is lost that a user could want (spec 6.5 to say iTIP-level parse errors are history).
- The lost fence's extra "not released" trace, and the merge's busy trace reusing "creation lease busy": both ruled (see "Rulings").
- Task 3's prelude repetition, `set_partstat`'s suffix match, the module split: until the module grows again.
- Case E's redundant retry step, the duplicate override reset, bare asserts, the panic-path restore, a `drain_lease.rs` split.
- `request_tree`'s positive control, step 4's "lowest id" versus "default calendar", the corruption closure's `X-ZA-KEY` check, the quota restore assertion.
- `moved` built by three `replace`s; OPTIONS no longer touching the auth cache or failure accounting (upstream's behaviour by design).
- The M35 row not saying the edit removed the backoff; `11656ec0`'s commit message stating the race as fact (this note records it as a hypothesis).

## PR #36 review (2026-10-16)

Two P2 findings on the drain's delegation handling, both fixed (one commit each, then the documentation commit):

- Finding 1, fixed in `ca8d2e2b`. The UID lease fenced only the organizer's commit, so a session whose lease expired while it waited inside the delegate fan-out could deliver its token after a successor delivered its own and committed; the delegate adopted the stale token (the higher document id) and its replies were dropped as forgeries, with nothing to repair it once the successor had cleared the source REPLY. Ruling: the recipient delivery is fenced on the same lease. `za_fan_out` takes `fence: Option<&ZaUidLease>`; `write_item_inner` asserts it in the delegate's batch ahead of the insert (the lease key is global); a lost fence is the call's `Err` (not a `5.1` status) and stops the fan-out; the drain's REPLY arm passes its lease, abandons it on that `Err` and returns "zero-access: drain fan-out lost its lease", the same outcome as a lost commit fence. Test: `test_lease` case G (`za-drain-l8`; `za-drain-l7` was taken by case F): session 1 parked inside the fan-out to key3 past its 1-second lease, session 2 takes the lease over, delivers and commits; session 1 then delivers nothing; key3 holds one item whose token key1 keeps, and key3's acceptance applies to key1's copy. With the fix reverted, the case fails at "key3 still has exactly one new item" (2 items).
- Finding 2, fixed in `be0374a2`. New delegates were found by comparing event-wide attendee sets, so a delegation of the master to someone who already attended an override sent no organizer REQUEST, and the delegate's copy never learnt of its series participation. Ruling: attendance is compared per instance (`Attendance` and `instance_key` in `drain.rs`: the master as `None`, an override by RECURRENCE-ID date and range, never its entry id); an override the merge created inherits the master's attendance, so a delegator's per-occurrence delegation does not count the delegator as new; the new addresses are deduplicated for delivery. Test: `test_reply` step 7 (`za-drain-r7`): a master inviting key2 and an override of the second occurrence inviting key2 and key3; key2 delegates the master to key3; key1's drain sends key3 exactly one REQUEST, key2 nothing; key3's copy then lists key3 in the master, delegated from key2, and still in the override. With the fix reverted, the step fails at "key3 has exactly one new item" (0 items).
- Ruling: the PUT create's fan-out passes no fence yet; the update PUT and DELETE fan-outs take no lease at all (their protection is the event-archive assertion), so they have nothing to fence on. A PUT create's fan-out followed by a lost commit fence is plan 9's F2 (spec 5.4), parked under "Deferred findings": the PUT create's fence, and whether updates should take the lease at all, belong with that decision.

## Facts for release 3

What iMIP for external and plain attendees, the sealed notification email and the RSVP page (spec 15) will need from the drain:

- The drain's input is `Incoming` (`drain.rs`): `Request(Option<TokenPairs>)` (the recipient's pair, absent when the item has no bundle carrier), `Cancel`, and `Reply(Zeroizing<[u8; 16]>)` (the replier's token). A REPLY item without `X-ZA-RSVP-TOKEN` is dropped as a forgery (`drop_forgery`), and a REPLY is authenticated only by a token in the event's bundle matched by address. An external attendee's reply (iMIP, or the RSVP page) therefore needs either a token it can present (carried in the invitation mail or the page link) or a new `Incoming` arm with its own authentication; the existing arm must not be loosened.
- The sender check needs `ChangedBy::PrincipalId`: a `CalendarAddress` sender (what an external sender is) is rejected as `SenderIsNotOrganizerNorAttendee` and kept as history. A mail-borne item needs a sender rule of its own (upstream's iMIP ingest checks the mail's sender against the ORGANIZER or ATTENDEE).
- Where a mail-borne item would enter: upstream's iMIP ingest, `crates/email/src/message/ingest.rs` (the key-account gate at about `:367`, `&& !account.is_key_account()`, refuses it today). Release 3 would write a sealed pending item there, wrapped to the recipient's `pk` as the fan-out's `write_item_inner` (`crates/groupware/src/calendar/fanout.rs`) does, and the drain would take it from `drain_item` unchanged; `open_pending_item` accepts a bundle carrier only with exactly one pair, and an item with no carrier yields `Request(None)`.
- Outgoing: the drain's only outbound path is the delegate fan-out through `za_fan_out`, never `ItipMessages::queue` (invariant 12). Any mail it would send (a sealed notification, iMIP to an external delegate) goes through a new sealed path, not the task queue.
- `token_hits` (`tests/src/za/leak.rs`) does not decode MIME bodies, so a leak test of any mail that carries a token or a sealed invitation needs MIME decoding first; it also misses a token inside a larger base64 bundle at an offset that is not 3-aligned, or hex-encoded.

## Deferred findings

Open items for Jay:
- (plan 9 final review) F2, the 412 after delivered items (spec 5.4). A PUT whose fenced commit is lost to another session ends in 412 after its fan-out delivered items, and those items carry tokens the organizer never stored. Plan 10's adoption rule makes the consequence concrete: an accepted REQUEST replaces the attendee's pair with the incoming one, so the copy ends with the pair of the item processed last, and a REQUEST from the refused PUT still carries a pair the attendee's drain adopts. If that item is processed after the winning session's item, the attendee holds the orphan token and its replies are dropped as forgeries until the organizer's next REQUEST re-sends the stored pair (it self-heals then). Decide between accepting that, and compensating deletion of the items the refused request wrote (the fan-out would return `(recipient, document_id)`; a race with a drain that already cleared the pending bit must skip the delete).
- (Task 2 review, residual shared with the PUT path) A delegating merge's fan-out writes land in other inboxes outside the item's fenced batch. The drain side is now fenced (PR #36 review, finding 1): no delegate item is written after a lease takeover, so a stale token can no longer reach a delegate after its successor's. What remains on the drain side has two shapes. First, items delivered before a lost commit (the event-archive assertion after an organizer PUT update) leave the delegate holding a token the organizer lacks until the retried merge fans out again one backoff later, and a delegate REPLY in that gap is dropped as a forgery. Second, a delivery made before a lease takeover: session A delivers its token to the delegate and then loses its lease; session B delivers a fresh token later. The delegate drains both in id order and ends on B's token, which matches the organizer's copy, but a reply the delegate sends between draining A's item and B's item carries A's token and is dropped as a forgery. That window is bounded by the delegate's next reply, and the organizer's state is consistent throughout. The PUT create's fan-out (plan 9 F2, above) passes no fence; the update PUT and DELETE fan-outs take no lease.
- (plan 9 final review) F1's spec 5.1 amendment ("the sending state follows the calendar's owner") is still Jay's.

Deferred minors, one line each, tagged with the review they came from. None changes behaviour a test relies on; two diverge from the spec as noted (the private-key deferral, spec 10, and history for `MissingUid` / `NoSchedulingInfo`, spec 6.5):
- Task 1:
  - (Task 1 review) Three comments still say "plan 10's sweep" instead of naming `sweep_expired_uid_leases`: `lease.rs`'s docs of `lease_key_range` and `release`, and `crates/store/src/dispatch/lookup.rs:427`.
  - (Task 1 review) The sweep's error in `maintenance.rs` propagates with `?` like the neighbouring purge steps (consistent; informational).
  - (Task 1 review) `test_hooks.rs:27-28`, a stray `//!` blank line splits the rustdoc list; `test_hooks.rs:152-153`, `secs as i64` would read better as `i64::try_from(secs).ok()`.
- Task 2:
  - (Task 2 review) A recorded private-key failure defers 30 s and re-enumerates the inbox every backoff for the session, where spec 10 says the drain is skipped for the session; a permanent deferral would avoid the churn.
  - (Task 2 review) Every DAV request of a key account runs one change-log since-query on the scheduling collection, and any inbox change makes the next request read every inbox archive to test a flag bit (spec-accepted cost; a pending index would be the fix if inboxes grow).
  - (Task 2 review) `MissingUid` and `NoSchedulingInfo` from `itip_snapshot` keep the item as history (sealed with an empty UID in the AAD), while spec 6.5 calls an unparsable tree permanent; the plan chose history for iTIP-level errors.
  - (Task 2 review) The lost-fence path's extra "not released" trace and the merge's reuse of the creation-lease busy detail are accepted (see "Rulings").
  - (Task 2 review) The Step 1 test comment says the first request drains by "trigger 1" though `trigger_drain` shares key2's key-cache entry; the behaviour tested is right, the comment overstates.
- Task 3:
  - (Task 3 review) `test_reply` step 2 covers "right sender, wrong token" only; a planted REPLY with the real token from a different principal (the sender resolution for a REPLY) is uncovered there (M34 and `test_errors` step 2 cover the sender check on a REQUEST).
  - (Task 3 review) `test_adoption` pins the new token's adoption but does not plant a reply under the old token to show it is rejected after the re-add.
  - (Task 3 review) The invite, drain and `copy_href` prelude is repeated seven times and the reschedule-skipping-drain block twice; `set_partstat`'s `ends_with("mailto:<address>")` match is suffix-loose; the drain test module is large enough that `set_partstat`, `partstat_of` and `reply_tree` could move to utils and the reply tests to their own file.
- Task 4:
  - (Task 4 review) Cases A and E have no positive "session 2 waited on the lease" signal; an elapsed-time assertion (at least `LEASE_RETRIES * LEASE_RETRY_WAIT` = 1 s) would close it.
  - (Task 4 review) Case E's retry step (override 1 s, then a 1.5 s sleep) is redundant, since the override of 0 was in force when session 2 deferred; a duplicate `set_drain_retry_override(None)` at the end; parked requests are bounded by the client's 5 s timeout (worth a comment); bare asserts without messages at three sites; the module would read better split (`drain_lease.rs`); overrides are not restored on panic paths (as in other modules).
- Task 5:
  - (Task 5 review) Step 3 lacks `uid_copies("za-drain-e4") == 0` (a drain that created an event for an orphan REPLY while writing `event_id` `None` would pass); no positive control that `request_tree` imports with a matching sender; step 4 cannot tell "lowest id" from "default-calendar copy"; the corruption closure does not assert that the overwritten entry is `X-ZA-KEY`; the leak test could add an `itip_tasks_for` check after the reply and a key1 scan while the reply is pending; the quota restore is not asserted in-test; fixtures are split across `drain.rs` and `fanout.rs`.
