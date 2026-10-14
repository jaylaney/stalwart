# Zero-access calendar: plan 9 outcome

Written 2026-10-14 at the end of plan 9 (`2026-10-14-zero-access-9-fanout.md`).
Read it before plan 10.

## Where things stand

- Commits `44982485..` through the final fix wave's documentation commit (`Plan 9 outcome: final review rulings and spec note wording`, the last commit of the plan; a range ending there excludes `44982485` itself), on `release2-plan9-fanout` (cut from plan 8's `68e1f1df`; plan and review fix round `621f88a4`, `44982485`; not pushed). The first code commit is `cbe5db0e`:
  - Task 1 `cbe5db0e` (lease and test-mode hooks), Task 2 `e58ef0b7` (fan-out module), Task 3 `dc9d560f` (`SendSealed`, PUT paths), Task 4 `bba98065` (reply, delegation, failure tests), Task 5 `17f105c7` (DELETE paths), Task 6 `e34e37e6` (creation lease tests), Task 7 `396dd67a`, `05a1925b`, `74870ea7` (leak scan), Task 8 `4dde5479` (mutation runs and this note);
  - final fix wave: `4902accd` (sending state follows the owner, lease hardening, test breadth), then the documentation commit (this note's final-review sections, the release 1 spec parentheticals).
- The suites run in Task 8 Step 2, in order, on `74870ea7`'s code (the spec and note edits uncommitted at the time), with no reruns:
  - `cargo test -p vault -p groupware`: groupware 76 passed, vault 30 passed.
  - `cargo test -p common za::lease`: 6 passed.
  - `cargo test -p common auth::vault`: 14 passed.
  - `za_tests`: 1 passed in 91.20s (leak scan: 1407 records, 63 archives, 6 blobs, 627 search index records (283 of the account, none calendar), 5 events, 3 calendars, 4 items, 2 emails; negative control caught 29 violations).
  - `za_tests` with `BLOB_STORE=FileSystem`: 1 passed in 48.45s (leak scan: 1402 records, 63 archives, 6 blobs, 627 search index records, 5 events, 3 calendars, 4 items, 2 emails; negative control caught 29 violations).
  - `webdav_tests` with `ZA_KEY_ACCOUNTS=1`: 1 passed in 20.57s, including the key-account scheduling variant.
  - `webdav_tests`: 1 passed in 16.84s. `cal_itip` passed on the first run.
  - `cargo build --release -p stalwart --no-default-features --features rocks`: finished in 15m 44s.
    - No warning in `groupware`, `dav`, `services/src/task_manager/manager.rs` or `common/src/{za,auth/vault.rs}`.
    - The filter `common/src/auth` matches four upstream warnings in files plan 9 does not touch: `permissions.rs:48`; `authentication.rs:661`, `:698`, `:720`.
    - Crate warning totals, all upstream's: `store` 1, `common` 18 (those four included), `jmap` 3, `services` 2 (both in `task_manager/index.rs`).
  - `cargo tree -p stalwart --no-default-features --features rocks -e features -i common | grep test_mode`: nothing (`no test_mode in the product`).
  - `git status --short crates/` printed nothing.
- The suites run in the final fix wave, after all its code changes, on `4902accd`'s code (the first runs with that code uncommitted, identical to the commit; the documentation edits uncommitted throughout), with no reruns:
  - `cargo test -p common za::lease`: 7 passed (the absent-key release test is new).
  - `cargo test -p groupware`: 76 passed.
  - `za_tests`: 1 passed in 52.40s (leak scan: 1799 records, 70 archives, 8 blobs, 987 search index records (643 of the account, none calendar), 5 events, 3 calendars, 4 items, 4 emails; negative control caught 29 violations). The two extra emails are key2's upstream invitation and CANCEL from `fanout::test_plain_owner`.
  - `za_tests` again after the M23 re-run and its revert, on the committed `4902accd`: 1 passed in 52.43s (leak scan: 1800 records, 69 archives, 8 blobs, 987 search index records (643 of the account, none calendar), 5 events, 3 calendars, 4 items, 4 emails; negative control caught 29 violations).
  - `webdav_tests` with `ZA_KEY_ACCOUNTS=1`: 1 passed in 22.37s.
  - `webdav_tests`: 1 passed in 19.29s. `cal_itip` passed on the first run.
  - `cargo check -p stalwart --no-default-features --features rocks`: no warning in `groupware`, `dav`, `jmap/src/calendar_event`, `services/src/task_manager/manager.rs` or `common/src/{za,auth/vault.rs}`; the 24 upstream warnings unchanged.
  - `cargo check -p tests --tests`: no warning in the fork's test files.
  - `cargo fmt -p common -p groupware -p dav -p jmap -p tests -- --check`: clean.
- What now exists:
  - `ItipSendStatus::SendSealed`, decided on the calendar owner's key status (final fix wave); `KeyAccount` is gone;
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

No task changed a name; the table is the plan's, with two rows amended by the final fix wave (the sending state's `owner_is_key` parameter, the knob's clamp). (`ZaUidLeaseGuard` also has `new`, `lease`, `release` and `abandon`, as the plan's code defines.)

| Name | Value | Where |
|---|---|---|
| Sending state | `ItipSendStatus::SendSealed`: upstream's `Send` preconditions, for a calendar owned by a key account (`resolve(.., owner_is_key)`; the PUT branches pass `za_keys.is_some()`); `KeyAccount` is removed. `builds_messages()` (`Send` or `SendSealed`), `is_send_sealed()`; `is_send()` stays `Send` only | `crates/groupware/src/calendar/itip.rs` |
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
| Lease duration | `DEFAULT_LEASE_SECS = 30`; `ZA_UID_LEASE_SECS`, read once per process (unset, malformed or 0: the default; above `MAX_LEASE_SECS = 3600`: clamped to it); test-mode override `set_lease_secs_override` | `lease.rs`, `test_hooks.rs` |
| Lease retries | `LEASE_RETRIES = 20`, `LEASE_RETRY_WAIT = 50 ms`, then `None` | `lease.rs` |
| Lease API | `ZaUidLease::{acquire, assert_in, release}`, `ZaUidLeaseGuard` (releases on drop), `lease_key`, `lease_key_range`, `lease_value_expiry` | `lease.rs` |
| PUT helpers | `za_take_uid_lease` (lease, then archive re-check; 503 when busy), `za_assert_is_unique_uid` (archive-based), `za_commit_create` (fenced commit, release, lost-fence path) | `crates/dav/src/calendar/mod.rs` |
| Busy lease | 503, traced as `Store(AssertValueFailed)` with details `zero-access: UID creation lease busy` | `mod.rs` |
| Test-mode hooks | `FAN_OUT_PAUSE`, `PUT_PAUSE`, `COMMIT_FAIL_ONCE`, `TASK_CONSUMER_PAUSE`, and the lease-duration override | `crates/common/src/za/test_hooks.rs` |

## Rulings

- The resource-cache invalidation after an item: none. Upstream's ingest performs none; `fetch_dav_resources` applies the change log on every request on every node.
- The test-mode lease-duration override stands in for `ZA_UID_LEASE_SECS=1` (read once per process).
- Case (c) of the lease tests holds its first session at `FAN_OUT_PAUSE`, not `PUT_PAUSE`: a session held before it takes the lease leaves nothing for the lease to decide.
- Delegation case 4(b) (key2 as a delegate replying to its delegator) is the case the ORGANIZER-only rule decides; 4(a) alone would make "no item to any delegator" vacuous.
- The new za sub-modules run after `dav_seal::test_legacy`, before `tracing` and `gating`, so M22 and M23 fail in the tests that own them.
- Existing tests changed by `SendSealed`: `tracing` (four past-event traces), `gating::test_scheduling` (schedule tag present; the DELETE comments), `pairs` (the no-pair cases carry no attendee), the key-mode `scheduling` variant (store-level delivery, the CANCEL).
- None of upstream's `cal_scheduling` assertions runs unchanged in key mode before the drain; the variant asserts the organizer's schedule tag upstream asserts.
- `pairs::test`: the alarm-only PUT on the planted event (no schedule tag) now runs `itip_create` and fans out REQUESTs to `pair-a` and `pair-b`, marked `5.3`; plan 8's M18 row ("no token pairs after an alarm-only PUT") now reads through the fan-out's union, which keeps the stored pairs.
- The trace volume plan 5 removed for `KeyAccount` returns through upstream's `Err` arm for key accounts under `SendSealed`: every key-account PUT of an event without an ORGANIZER traces `NoSchedulingInfo` at Debug level, as a plain account's does. Correct under invariant 9; those traces carry ids and fixed strings only; no code change.
- The leak test's CN canaries (`fanoutcn-canary` on key2's line, `fanoutextcn-canary` on the external attendee's) stay, and spec 11's attendee-address canary is now covered too: the plan's CN-only form did not satisfy spec 11, so Task 7's fix rounds added an external attendee, `ATTENDEE;CN=fanoutextcn-canary:mailto:fanoutaddr-canary@remote.test`, with `fanoutaddr-canary` in `EXTRA_CANARIES`. Key2's own address cannot be a canary: an address must resolve for the item to be delivered.
- The external attendee's line is 67 octets, under the 75-octet fold, so no iCalendar fold can split the address canary in a text form (a queued task, a mail). PARTSTAT and RSVP were dropped to get there; upstream treats a missing RSVP as true (`itip.rs:144`, `:250`; `attendee.rs:300`) and a missing PARTSTAT as NEEDS-ACTION, so the scheduling treatment is unchanged. If a regression queued a REQUEST, upstream's export would add PARTSTAT and fold the address; the task value's `to` map still holds it unfolded, the CN canary survives the fold, and `itip_tasks_for` is the structural backstop. Accepted.
- The leak scan's other additions (Task 7 fix round 1): `item_extras >= 2`; the fan-out token is searched while the task consumer is held, so the queue is scanned for it; the organizer's (key1's) stored event gets the structural `check_event` and the `X-ZA-EXTRA` then `X-ZA-KEY` ending.
- A disabled key account still receives items: `resolve_inner` does not check `Authenticate`. It cannot log in, so the items wait and expire under the inbox purge; upstream's mail path would refuse at RCPT. Accepted.
- The lost-fence 503 can follow a fully delivered fan-out: items stay (spec 5.4), so the client's resend delivers a second set of items with fresh tokens, test 8's situation.
- A key account's DELETE of a sealed event whose envelope does not open answers 500 when `send_itip` is true (the unseal before `itip_cancel` fails), per release 1 spec section 10's unseal-failure rule; `Schedule-Reply: F` deletes it without reading it.
- `za_session_keys` runs on every DELETE, as on every PUT (`update.rs:81`): one account-cache lookup for a non-key DELETE, which answers `None`. A key account without keys gets 403 from the existing gate before any delete work.
- A `za_fan_out` error on DELETE aborts the DELETE before the commit (nothing deleted), the PUT's shape and spec 5.4's "items first". The DELETE discards the `ZaFanOutResult`: the event is going away.
- `COMMIT_FAIL_ONCE` fires only in `za_commit_create`, the PUT create branch; the drain gets its own hook in plan 10.
- Deviations from the plan's text, each a ruling during execution:
  - External attendee domain: `remote.test`, not the plan's `remote.example`. `utils::is_valid_domain` (`crates/utils/src/lib.rs:428`) accepts only a PSL suffix or its own reserved-TLD list (`test`, `localhost`, `local`, ...); `.example` is neither, so `Email::new` drops the address and upstream builds no message to it. Every test uses `outsider@remote.test`; the plan file keeps `.example`.
  - Test 7b's `itipMaxSize` is 100, not 64: the generated registry schema's minimum is 100 (`crates/registry/src/schema/structs_impl.rs:4800`). Still far below any invitation in the test; the fan-out compares with `>=`.
  - `reply.rs`'s setup-pending organizer is `key11@example.com`, not `key7`: `app_password.rs` already creates key7 for the suite's lifetime.
  - `uid_lease.rs` case (e) was first ruled to answer 412 and was inverted to 201: spec 6.3 step 6 keeps upstream's per-calendar UID rule, so a PUT of an existing UID into key1's second calendar creates a second copy. The test asserts 201, two copies, one new key2 item, none for key1, no lease left; dropping the `parent_id == calendar_id` filter in `za_assert_is_unique_uid` would turn it into a 412, so the inverse pins the filter. A per-account rule would be a spec change.
  - `za_variants::scheduling` unfolds the body (RFC 5545 3.1) before its substring checks: the new `SCHEDULE-STATUS=1.2` lengthens jane's ATTENDEE line and the serializer folds it mid-address. Same assertion strength.
  - `gating.rs`'s DELETE of key1's `invite.ics` sends `Schedule-Reply: F` (`NO_REPLY`), keeping the module's "nothing leaves the key organizer" framing once DELETE CANCELs fan out.
  - Compile facts: `fanout.rs` (tests) imports `registry::types::EnumImpl` for `StorageQuota::MaxDiskQuota.as_str()`; `uid_lease.rs`'s `put()` takes `&str` (not `&'static str`) and returns `impl Future + use<>` (edition 2024 captures), because case (d) borrows a cloned `Account`; the brief's unused `CONTENT_TYPE` import was dropped. Task 2's fan-out imports `common::auth::BuildAccessToken` for `.build()`.
  - Test-mode compile check: `cargo check -p tests --tests`. The plan's `cargo check -p services --features test_mode` (and `-p dav`, `-p groupware`) fails in upstream `common` code that needs `enterprise` under `test_mode` (`telemetry/metrics/test_data.rs`, `network/dns/update.rs`), now that those crates forward `common/test_mode`; a plain `cargo check -p tests` checks only `tests/src/lib.rs`, since every dependency of the tests crate is a dev-dependency.
  - M24's first catcher is `fanout::test_failures` step 8, not `uid_lease` (b); and the plan's table omitted spec 11's fence-removal mutation, run here as M24b (both in the table below).

## Mutation runs (plan 9)

All runs in Task 8 Step 1, one at a time, each reverted with `git checkout --` and followed by an empty `git diff --stat`. None was committed. M22, M23 and M26-M28 failed at the same assertions in Tasks 3, 4 and 5; M24 and M25 as in Task 6. Line numbers are the committed files' at `74870ea7`.

| Id | Check, `file:line` | Edit | First failure | Command |
|----|--------------------|------|---------------|---------|
| M22 | The recipient split, `crates/groupware/src/calendar/fanout.rs:444-465` (`resolve_inner`) | everything after the `account_id == sender_id` check becomes `Ok(Recipient::Internal { account_id, public_key: [9u8; 32] })` | `tests/src/za/fanout.rs:190:5`: `plain's inbox received an item` (left 1, right 0). The primary expectation; the fallback was not needed | za suite |
| M23 | The queue path stays closed for key senders, `crates/dav/src/calendar/update.rs:481-485` (create branch; the same text at `:285` in the update branch was not touched) | the `if itip_status.is_send_sealed() { .. } else { .. }` becomes `itip_messages = Some(ItipMessages::new(messages));` | `tests/src/za/fanout.rs:906:5` (`test_task_queue`): `a CalendarItipMessage task was queued for the key organizer` (left 1, right 0) | za suite |
| M24 | The lease in the PUT create branch, `crates/dav/src/calendar/update.rs:428-432` | `Some(za_take_uid_lease(..).await?)` becomes `{ za_assert_is_unique_uid(..).await?; None::<ZaUidLeaseGuard> }`, with `za_assert_is_unique_uid` in the `use super::{..}` line and `use common::za::lease::ZaUidLeaseGuard;` added | First: `tests/src/utils/webdav.rs:599:13`: `Expected 500 Internal Server Error but got 201 Created`, from `fanout::test_failures` step 8 (`tests/src/za/fanout.rs:807-810`): without the lease the create commits through `commit_batch`, not `za_commit_create`, so `COMMIT_FAIL_ONCE` never fires. With step 8 skipped (temporary `if false { .. }` around it and `fo-commit` out of its cleanup list, reverted with the mutation): `tests/src/za/uid_lease.rs:127:5`: `an expired holder committed` (left 201, right 412), the plan's catcher, case (b) | za suite (twice) |
| M24b | The fence on the create commit, `crates/dav/src/calendar/update.rs:551` (spec 11's fence-removal mutation; the plan's table omitted it) | delete `lease.lease().assert_in(&mut batch);` in the create branch | `tests/src/za/uid_lease.rs:127:5`: `an expired holder committed` (left 201, right 412), case (b) | za suite |
| M25 | The archive-based re-check, `crates/dav/src/calendar/mod.rs:155` (`za_take_uid_lease`) | `za_assert_is_unique_uid(..)` becomes `assert_is_unique_uid(server, resources, account_id, calendar_id, Some(uid))` | `tests/src/za/uid_lease.rs:81:5`: `the stale snapshot let a second copy in` (left 201, right 412), case (a) | za suite |
| M26 | The ORGANIZER-only rule for REPLY, `crates/groupware/src/calendar/fanout.rs:296` (`route`) | `Some(ICalendarMethod::Reply) if to_organizer =>` becomes `Some(ICalendarMethod::Reply) =>` | `tests/src/za/reply.rs:277:5`: `an item reached the delegator key3` (left `[6]`, right `[]`), case 4(b) | za suite |
| M27 | The quota check, `crates/groupware/src/calendar/fanout.rs:523-529` (`write_item_inner`) | delete the `has_available_quota` block | `tests/src/za/fanout.rs:613:5`: `an item reached key2 past its quota` (left `[15]`, right `[]`), case 7a | za suite |
| M28 | The `CalendarSchedulingReceive` check, `crates/groupware/src/calendar/fanout.rs:454-461` (`resolve_inner`) | delete the `has_permission(Permission::CalendarSchedulingReceive)` block | `tests/src/za/fanout.rs:709:5`: `an item reached key3 without CalendarSchedulingReceive` (left `[4, 5]`, right `[4]`), case 7d | za suite |

No mutation survived.

Final fix wave: P1 moved the `SendSealed` decision into `ItipSendStatus::resolve`'s new `owner_is_key` parameter, so M23 was re-run once on `4902accd` (the same edit at `crates/dav/src/calendar/update.rs:484-488`, create branch). It still fails first at `test_task_queue`: `tests/src/za/fanout.rs:962:5`, `a CalendarItipMessage task was queued for the key organizer` (left 1, right 0). Reverted with `git checkout --`; `git diff --stat` printed nothing.

## Facts for plan 10

- The lease, for the drain's import (spec 6.3, 6.4):
  - after a first UID lookup finds nothing, `ZaUidLease::acquire(server.store(), account_id, uid)`; `None` leaves the item pending and holds the cursor (spec 6.2);
  - look the UID up again under the lease; merge if it appeared;
  - `lease.assert_in(&mut batch)` ahead of the import and the item rewrite in the item's one batch;
  - `lease.release(server.store())` after the commit; a lost assertion aborts the pass;
  - `ZaUidLeaseGuard` releases on early returns;
  - the sweep iterates `lease_key_range()` and clears every key whose `lease_value_expiry` is at or below now; `za_uid_leases` (`tests/src/utils/za.rs`) counts them. Clear with an `AssertValue::Hash` of the expired value the sweep read, so a holder that re-took the key in between keeps it (Task 1 review).
- The hooks (`common::za::test_hooks`): `FAN_OUT_PAUSE` (recipient-keyed), `PUT_PAUSE`, `COMMIT_FAIL_ONCE`, `TASK_CONSUMER_PAUSE` (global), `set_lease_secs_override`. The drain pause and the drain skip go into the same module; the `dav`, `groupware` and `services` `test_mode` features already forward `common/test_mode`.
- The delegation fan-out (spec 6.4): `server.za_fan_out(&organizer_info, &keys, messages, Some(current_pairs), &merged_tree)` with the messages of `organizer_request_full` restricted to the new attendees. It hands out the delegates' tokens and returns the pairs the organizer's bundle keeps; `za_set_schedule_status` writes the statuses before the seal.
- `open_pending_item` refuses a bundle that does not hold exactly one pair (plan 8's fix wave), so the drain need not re-check the count.
- `Server::za_private_key`: a store error is neither traced nor recorded (plan 8); the drain traces it and a later call retries.
- `token_hits` (`tests/src/za/leak.rs`) does not decode MIME bodies; mail flows that carry tokens need that first. It also misses a token inside a larger base64 bundle at an offset that is not 3-aligned, or hex-encoded; and `try_archive` does not decode large LZ4 registry pickles, which matters if plan 10 plants large invitations.
- Test helpers for the drain's tests: `open_pending_item_as`, `inbox_items`, `new_inbox_items`, `plant_attendee_copy`, `hold_task_consumer`, `itip_tasks_for`, `HookParked`.
- An item whose sender's commit failed (`COMMIT_FAIL_ONCE`, lease case b) stays pending; a retried create delivers a second item with an equal SEQUENCE and a fresh token, which the drain must adopt (spec 5.4, 6.4).
- `ZaUidLease::release` answers `true` without a trace when the key is already absent (the maintenance purge or the sweep removed an expired lease); it answers `false` and traces "UID creation lease not released" only when the key holds another holder's value or the store fails (final fix wave).
- `COMMIT_FAIL_ONCE` fires only in `za_commit_create`, the PUT create branch; the drain's commits need their own hook.
- Two attendee addresses that resolve to one account produce two items in one inbox for one UID, with different pairs (`fanout.rs`, recipient loop); the drain must accept two pending items per UID.
- Test data: external attendees need a domain `utils::is_valid_domain` accepts (`remote.test`, not `remote.example`); `key7` belongs to `app_password.rs` and `key11` to `reply.rs` for the suite's lifetime; `itipMaxSize` cannot go below 100.

## Final review

The whole-branch review (`68e1f1df..4dde5479`, 0 critical, 2 important, 13 minor, "with fixes") and its rulings:

- F1 (important), fixed. `SendSealed` was decided on the authenticated account while the fan-out ran only for a key-owned calendar, so a key account writing an invitation into a plain account's shared calendar got a 201 and the messages were dropped (and its DELETE mailed a CANCEL). Ruling: the sending state follows the calendar owner's key status. `ItipSendStatus::resolve` takes `owner_is_key` (`crates/groupware/src/calendar/itip.rs:1073-1093`); both PUT branches pass `za_keys.is_some()` (`crates/dav/src/calendar/update.rs:250`, `:473`); the three JMAP callers pass their previous value (`account_info.account().is_key_account()`; JMAP calendar methods are refused for key accounts, and a key account arriving there would build nothing); the DELETE already followed the owner. Such an event is stored in plaintext under its plain owner, so the invitation travels as upstream mail, a task and then mail, as `fanout::test_plain_owner` asserts for the PUT and the DELETE. For Jay: spec 5.1 says "a key account resolves to `SendSealed`" without saying whose calendar; amend it to "the sending state follows the calendar's owner".
- F2 (important), parked for Jay (spec 5.4). A lost fence that ends in 412 (another session of the account stored the UID first) leaves the recipients holding items whose tokens the organizer never stored; under spec 6.4 the drain adopts the pair of the item it processes last, so a recipient's copy can end with the orphan token and its replies are dropped as forgeries until the organizer's next REQUEST re-sends the stored pair. Decide between accepting it with that consequence (it self-heals) and compensating deletion of the items this request wrote on the 412 path (the fan-out would return the `(recipient, document_id)` pairs; a race with a drain that already cleared the pending bit must skip the delete), designed together with plan 10's adoption rule.
- F4, migration note. Release 1 stored key-account events with attendees and no schedule tag (it never sent anything). The first rewrite of such an event by its organizer after the upgrade runs `itip_create` and invites its internal key attendees (`tests/src/za/pairs.rs` documents the case). Upstream behaves the same for events PUT while scheduling was off; arguably what the organizer wants, but it belongs in the release notes or the manual checklist.
- F5, catch-all delivery. `resolve_inner` uses `account_id_from_email(address, true)`, which falls back to the domain's catch-all address: an organizer's typo on a local domain whose catch-all is a key account yields a `1.2` item in the catch-all's inbox. Consistent with mail delivery and with upstream's iMIP sender lookup; spec 5.2's list of outcomes could say so.
- F13, fork-diff width. `storage.rs`'s `delete`, `delete_all` and `delete_with_events` stay wrappers around `_za` twins (about 105 lines). The alternative is one `za_keys: Option<&SessionKeys>` parameter on the three upstream functions with their eleven callers updated, which keeps one body per function; the plan's shape is kept.

Fixed in the final wave (`4902accd` and the documentation commit): F1; F3 (the release 1 spec parentheticals now read "release 2: superseded; see the invitations spec, section 5"); F6 (the non-key PUT create no longer allocates the UID, `update.rs:425-445`); F7 (an attendee's rewrite keeps exactly its own pair, `reply.rs:143-150`); F8a (the lost-fence trace precedes the retake, `crates/dav/src/calendar/mod.rs:251-266`); F8b (lease case (b'): a lost fence with no copy answers 503, zero copies, one leftover item, no lease, `uid_lease.rs:135-173`); F8c (the 503 branch's comment, `mod.rs:262-264`); F9 in part (`lease.rs:164-197`: an absent key counts as released, untraced; see the deferred lease item for the level); F10 (`lease.rs:134` `saturating_add`, `ZA_UID_LEASE_SECS` clamped to `1..=3600`, `lease.rs:46-55`); F11 (only `Limit(Quota)`/`Limit(TenantQuota)` maps to over quota, `crates/groupware/src/calendar/fanout.rs:523-537`); F12 in part (the DELETE CANCELs' SEQUENCE and pairs, `fanout.rs:473-552`); F14 (`reply.rs:383-386`); F15 in part (the `item_sequence` doc); Task 4's failed REPLY on the ORGANIZER (`reply.rs:168-218`) and 4(b)'s REPLY content (`reply.rs:349-357`); Task 5's mail assertion after the mixed DELETE, for plain (`fanout.rs:502-507`).

## Deferred findings

Open items parked for Jay:
- (Task 2 review) Upstream's `account_id_from_email` traces `StoreEvent::CacheHit` / `CacheMiss` with `Key = local_part` (`crates/common/src/cache/principals.rs:220-247`, `rcpt_id_from_parts`, reached through `account_id_from_email` at `:649`). Pre-existing upstream behaviour at debug level on every mail delivery and plain-account scheduling lookup, now also reached by the fan-out's recipient resolution. If the rule that traces name ids only is read as "no address in any trace during the fan-out", the fix is a fork-side non-tracing lookup.
- (Task 2 review) Two addresses resolving to one account produce two items per UID (plan 10, above).
- (final review) F2, the 412-after-delivery consequence (spec 5.4, above).
- (final review) F1's spec 5.1 amendment (above).

Deferred minors (the ledger, `.superpowers/sdd/2026-10-14-zero-access-9-fanout/progress.md`, has the detail), one line each, tagged with the review they came from; none changes behaviour a test or the spec relies on:
- Lease:
  - (Task 6 review) Removing the explicit `release` after the commit is masked by the guard's `Drop`; `abandon` versus `release` is an equivalent mutation.
  - (Task 1 review) `ZaUidLeaseGuard::release` takes the lease before its await, so a request future dropped mid-release leaves the key until expiry.
  - (Task 1 review; final review F9) `release` rebuilds the assertion `assert_in` builds. A release that fails because the key holds another value, or because the store fails, still traces at error level: trace levels are fixed per event type (`Store(AssertValueFailed)` is Error) and the fork adds no event types, so the final review's "warn" was not available. A guard dropped outside a runtime leaves the lease silently.
  - (Task 1 review) The module doc says the duration only decides how long a crashed holder blocks others; a live holder slower than the lease also loses its commit.
  - (Task 1 review) Unit tests: `expired_value(7)` evaluated twice can straddle a second; no concurrent-acquire test; the abandon half of the guard test is a timed negative; `LEASE_RETRY_WAIT` has no doc line.
  - (final review) `ZaUidLeaseGuard::lease()` panics after `release` or `abandon` (internal use only).
- Fan-out:
  - (Task 2 review) The token is drawn into an un-zeroized stack temporary before `Zeroizing` (fork-wide pattern).
  - (Task 2 review) A property can carry two SCHEDULE-STATUS parameters (upstream's `3.7`, then a fan-out status); RFC 6638's grammar is one comma-separated parameter. Upstream already produces the same shape (spec owner).
  - (Task 2 review) A second `account()` lookup in `write_item_inner` (cache hit); the unit fixture is one VEVENT, so cross-component dedup and override-instance statuses are untested.
  - (Task 5 review; final review F13) `storage.rs` grew about 105 lines for the `*_za` delete variants; (Task 5 review) `za_session_keys` is evaluated twice per DELETE (URI gate and handler, as in every calendar handler).
- Tests:
  - (Task 5 review) After the sealed mixed DELETE, plain's mail count is asserted (final wave); the outsider's absence of mail is covered only indirectly, by `wait_for_delivery` panicking on a message still in the SMTP queue (from code reading).
  - (Task 4 review) The size check's boundary (`>=` versus `>`) is unpinned; 7a's 1-byte quota cannot tell a size-based refusal from an exhausted quota; `Recipient::Failed` and the item-write-error `5.1` paths are untested; "others unaffected" relies on attendee order.
  - (Task 4 review) `organizer_statuses`'s doc comment nit.
  - (Task 6 review) Lease case (c) may pass uncontended on a slow runner (the plan accepts this) and does not check key2's item delta; (b)'s leftover item is checked by count only; the override reset is not panic-safe; the margins assume `ZA_UID_LEASE_SECS` is unset in CI; a UID-only lease key is caught only by Task 1's unit test, and `LEASE_RETRIES = 0` only by (c).
  - (Task 6 review) `uid_lease.rs` has unlabelled asserts and full paths at call sites (`crate::utils::webdav::DummyWebDavClient`, `hyper::StatusCode`).
  - (Task 5 review; final review F12) The DELETE fan-out moved after `commit_batch` is not caught (no DELETE failure hook; plan 10 material); (Task 3 review) the PUT's fan-out-after-commit is pinned only by the commit-fail hook.
  - (Task 3 review) `open_pending_item_as` logs in per call; (Task 5 review) `PLAIN_INVITE` duplicates `invite()`'s shape; (Task 7 review) the leak scan walks the store a second time for the token.
  - (Task 7 review, report only) Task 7's report mis-stated that the scan's events count includes key1's event; the scan counts only the scanned account.
