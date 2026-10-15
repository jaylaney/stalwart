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
  - Task 7: `11656ec0` (plan 9's lease case (d) waits for the index task; see "Rulings"), then the commit of this note and the spec's settled names.
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
| Sender check | candidates: the ORGANIZER of a REQUEST or CANCEL, every ATTENDEE of a REPLY; those resolving quietly (`za_account_id_from_email`) to `ChangedBy::PrincipalId` are the sender's addresses; none, or a `CalendarAddress` sender: `SenderIsNotOrganizerNorAttendee`, kept as history. For a REPLY the replier is the sender's address whose pair holds the token; none matching is a forgery | `drain.rs` |
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

No mutation survived.

Retired by plan 10 (gates removed), from the plan 3 note's mutation table (plan 7): M11 (schedule URLs hidden, `crates/dav/src/principal/propfind.rs`) and M12 (OPTIONS header, `crates/http/src/request.rs`, which mutated `za_is_key_account_request`). Task 6 (`86a53361`) removed both branches and the function, so neither edit has a target. M12's `za_tests` catcher was the tracing check "OPTIONS authentication failure not reported", now reworded (Task 6); its key-mode catcher was the fork's branch of `tests/src/webdav/basic.rs`, now upstream's. The plan 3 note is unchanged.

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
- (Task 2 review, residual shared with the PUT path) A delegating merge's fan-out writes land in other inboxes outside the item's fenced batch. If the merge's commit then fails (the event-archive assertion after an organizer PUT update, or a lease takeover after expiry), the delegate holds a token the organizer lacks until the retried merge fans out again one backoff later, and a delegate REPLY in that gap is dropped as a forgery.
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
