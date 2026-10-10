# Zero-access calendar: plan 3 outcome and handoff to the account page and the next release

Written 2026-10-07 at the end of the plan 3 execution session. Read this
after the plan 2 outcome note and before building the key-account web page or
cutting a release. Everything here is a ruling made during execution, a
reviewer finding that was deliberately deferred, or a fact the next step
needs; none of it is derivable from the code alone. Rulings are quoted by id
(P1, R4, ...) from the session's `preflight-scan.md` (P1-P10) and ledger
`progress.md` (R1-R15); the ones that amend the spec are collected under
"Decisions that are Jay's".

## Where things stand

- Plan 3 (gating: six tasks plus a Task 0 that the plan did not have, and one
  final fix wave) is complete on branch `zero-access`: commits
  `fc493f2c..fde504f3`, starting from spec revision 6. Task 0 repeats the
  calendar-data check when a setup completes (P1). Tasks 1 to 6 are sharing
  and JMAP gating, scheduling and iMIP, alarm and index and trace, the
  key-mode CalDAV variants, the leak regression test with CI, and docs. Each
  task had an implementer, a task review, scoped re-reviews where the review
  asked for fixes, and a whole-branch review of `fc493f2c..c4fe15fa`. That
  review found no Critical issue and four Important ones, all fixed in the
  wave `c4fe15fa..fde504f3` (six commits: OPTIONS, `Principal/get`, the leak
  test's scheduling channel, the alarm variant with the alarm.rs collapse, the
  CI split, the docs). The commit trailers of the wave were rewritten from
  Opus to Fable with filter-branch.
- Suites green at the end (fix-wave tree unless noted):
  - `cargo test -p vault` (25), `-p groupware` (48), `-p common` (101, last
    run at `c4fe15fa`), `-p http@0.16.25 --features test_mode` (6, last run
    in Task 0).
  - `za_tests`, now including `gating::test`, `gating::test_scheduling`,
    `setup::test_data_check` and the closing `leak::test`.
  - `webdav_tests` in baseline mode and with `ZA_KEY_ACCOUNTS=1`. In key mode
    the sub-modules `copy_move`, `acl`, `cal_alarm` and `cal_scheduling` now
    run the four variants in `tests/src/webdav/za_variants.rs` instead of
    skipping. The fix wave's first baseline run hit the known `cal_itip`
    flake; the rerun passed.
  - `system_tests` and `jmap_tests` passed at `c4fe15fa` (Task 6's final
    run); the fix wave did not rerun them, and the wave touched only
    `request.rs`, `principal/get.rs`, `alarm.rs` and tests.
- CI: `.github/workflows/test.yml` runs on push and pull request for
  `zero-access` (and `workflow_dispatch`). The job `zero-access` (needs
  `style`) runs `cargo test -p vault -p groupware`, then `za::za_tests`, then
  `webdav::webdav_tests` plain and with `ZA_KEY_ACCOUNTS=1`. The `test` job
  keeps upstream's suites.
- The product build is warning-free in the fork crates. It shows the same 24
  upstream warnings as after plan 2 (store 1, common 18, jmap 3 in
  `registry/mapping`, services 2 in `task_manager/index.rs`).
- What now works: for a key account no JMAP calendar method runs, the
  session and `Principal/get` do not advertise calendars, sharing and
  scheduling are off (no ACL, no schedule URLs, no iTIP out or in, no RSVP),
  alarms are generic emails to the account itself, the calendar full-text
  index is skipped, no trace carries an iCalendar, and the leak test proves
  no canary reaches any store subspace.
- Nothing has been pushed.
- The scoped re-review of the fix wave (commits `23f93969..fde504f3`) found
  all seven findings addressed and no new Critical or Important breakage; its
  three minor observations are in "Deferred findings".

## Deviations from the plan that later plans must know

Task 0 and setup:

- Task 0 (P1): `za_setup` re-runs the three `za_has_documents` checks that
  `za_setup_token` runs, through one shared helper
  `za_assert_no_calendar_data`, after token verification, the enabled check
  and the password check and before any write; a hit answers 409 "account
  already holds calendar data". Issuing a setup token already writes the
  marker, so the pending account is a key account and the plan 2 gate answers
  403 to any DAV write; the test therefore plants the calendar straight in the
  store, as `setup.rs` does for key4. The check-then-commit window remains
  (calendar writes do not bump the vault revision); Task 2's ingest gate
  closes it in practice.
- P2: "revoke existing grants on key-owned calendars" and the shared-listing
  403 are closed by eligibility (no `Calendar` documents at token issue and
  at setup) plus Task 1's ACL and JMAP refusals. No per-item loader change; a
  key account's calendar can never carry a grant. Cost if wrong: a grantee
  gets a whole-listing 403 instead of a per-item one.
- P3: decision 7 (`CalendarEvent.preferences`) is closed by refusing JMAP.
  Task 1 grepped every crate; the only writer of content is JMAP
  `CalendarEvent/set` (`calendar_event/set.rs`, the `useDefaultAlerts` flag).
  DAV PUT and iTIP ingest build events with empty preferences. The leak test
  asserts the field is empty on every key-account event.

JMAP (Task 1):

- R1: each JMAP gate sits after the method's own access check, not after
  `resolve_account_id`. Placed first, an authenticated user could tell key
  accounts from others by account id (`accountNotSupportedByMethod` against
  `forbidden`). Owners, impersonators and master users pass the access check
  and are refused.
- R2: the changes gate lives in `ChangesLookup::changes`
  (`jmap/src/changes/get.rs`), not in `request.rs`. That also covers
  `CalendarEvent/queryChanges` and `CalendarEventNotification/queryChanges`,
  which call `changes` and then the query functions past every gate in
  `request.rs`.
- R3: JMAP, availability and blob gates use `try_account`, because `account()`
  errors on unknown ids and would change non-key answers for admins with
  Impersonate (invariant 9). Cost: an orphan id of a deleted key account fails
  open into upstream code over sealed data with no keys. The ACL and outbox
  gates use `account()` because the id was just resolved from an existing URI
  or address.
- R4: the availability test calls as admin. A plain caller sees nothing of
  key1 with or without the gate (no shares); admin is a member of every
  account and is the impersonation surface spec 9 names. A mutation run
  confirmed the gate is load-bearing.
- R5: the linked-blob download gate (P4, `has_access_blob`) is kept untested.
  No code links Calendar or CalendarEvent documents as blobs (only FileNode
  emits `IndexValue::Blob`), so the gate is unreachable defence; a few lines
  of fork diff.
- P4, P5, P8: the blob gate was added as the plan 2 obligation; the brief's
  `assert!(.. || true)` and triple-or availability assertion were replaced by
  exact assertions (P5); WebSocket and EventSource needed no code, because
  `websocket/stream.rs:98` calls the same `handle_jmap_request` and
  EventSource carries state strings only (P8, confirmed by Task 1 and its
  reviewer). The key-account check runs per method call, not per
  connection: every calendar method looks the account up live
  (`za_assert_calendar_allowed`, `crates/jmap/src/api/request.rs:735-743`),
  so a socket opened before its account became a key account is refused on
  its next calendar call. `tests/src/za/websocket.rs` pins this (plan 7).
- Test details: `sinceState` is `"n"`, not `"0"` (the latter fails parsing
  before any gate); principal hrefs are percent-encoded.
- Key-mode `principals` (Task 4 supplement): upstream's `principals.rs`
  expects the schedule URLs; in key mode it now expects 404 for
  `schedule-inbox-URL` and `schedule-outbox-URL` of a key principal.

Scheduling and iMIP (Task 2):

- `ItipSendStatus::KeyAccount` is denied in `resolve`; the DELETE `send_itip`,
  the iMIP ingest condition and the RSVP link all refuse key accounts. A
  denied status emits only a static-reason `ItipMessageError` and no
  Schedule-Tag; nothing is enqueued.
- P6: internal delivery to a local attendee goes only through mail ingest and
  `itip_ingest` (`imip.rs` builds a message and opens a local SMTP session);
  the ingest gate therefore closes it. The test shows a plain organizer's
  invitation leaves key1's scheduling inbox and calendar empty.
- R6: the ingest gate reads the `account` already in scope in `ingest.rs`
  (the brief moved `build_account_info`); RSVP gates use `try_account`; the
  brief's `queue_rx` assertion was vacuous (the sender is dropped unless
  `capture_queue()` is called) and is replaced by recipient-inbox checks; the
  RSVP test uses a real document id and asserts `InvalidLink`. An extra gate in
  `http_rsvp_attendee_copy` closes the one calendar write outside ingest.
- R7: the DELETE `send_itip` gate and the attendee-copy gate have no red
  test. Sealing already hides ORGANIZER and ATTENDEE, so a red test would
  need an unsealed key-account archive. Kept as invariant 6 defence.

Alarm, index and traces (Task 3):

- The two trace sites that printed plaintext (`dav/src/calendar/query.rs` and
  `groupware/src/calendar/dates.rs`) now log the iCalendar UIDs. The change
  applies to every account (spec 9 table). No trace-event test exists.
- R8: `build_calendar_document` skips full-text indexing with
  `try_account(..).is_some_and(is_key_account)`, not the brief's `account()?`.
  A destroyed account's pending index task would otherwise retry forever for
  all accounts. A missing account falls through to upstream's NotFound path.
- R13: the generic alarm is post-loop clearing of the six content locals
  (summary, description, location, conference, organizer, guests, plus the
  recipient), not the brief's `if !generic` wrap that re-indented about 60
  upstream lines. Equivalent on sealed trees and safer on legacy plaintext;
  the diff in `alarm.rs` is +25/-7 against `98883a42`.
- The generic email goes to the account's own address, carries start, end,
  timezone, the account as organizer and a webcal link, and no event text.

Leak test and CI (Task 5):

- R9: the one pre-existing rustfmt difference in upstream
  `tests/src/jmap/mail/set.rs` is formatted and committed on its own
  (`93f62bea`), so CI's style job, which gates the test jobs, passes. Cost: a
  one-hunk conflict in an upstream test file at the next upstream merge.
- R10: the search-store clause of spec 11 is covered structurally (no
  Calendar-index entries of the key account in `SUBSPACE_SEARCH_INDEX`),
  because canaries are tokenised and hashed in the index and the negative
  control cannot reach the gated indexer.
- R11: only calendar-index entries of the key account count as violations.
  Mail-index entries are expected: mail is unsealed in release 1 (spec 1.2)
  and the generic alarm email is indexed like any mail. The test asserts the
  account has index entries at all, so the calendar check cannot pass on a
  misread key layout.
- Carrier recognition in the scanner is exactly `SEALED_PROP`, `KEY_PROP`,
  `EXTRA_PROP`; an unexpected `X-ZA-FOO` is an ordinary X- property and must
  be sealed. The key layout is spelled with named constants (P7).

Final review and fix wave:

- R12: DAV OPTIONS (`http/src/request.rs`) authenticates only when an
  `Authorization` header is present and omits `calendar-auto-schedule` for a
  verified key account; on any failure it answers upstream's fixed header
  unchanged. Spec 9 says auto-schedule is not advertised, and the reviewer's
  lost-invitation counterexample needed it. Cost: a wrong credential sent with
  OPTIONS now counts as a failed authentication attempt (upstream never
  checked it), and a key account's OPTIONS that misses the auth cache pays one
  password verification. Upstream's `tests/src/webdav/basic.rs` asserts the
  exact OPTIONS header as john, so it gained a `key_accounts_mode()` branch.
- R14: `Principal/get` omits the calendars capability (in `accounts` and in
  `capabilities`) for key principals, consistent with the session filter.
- R15: the alarm recipient override stays defence in depth without its own red
  test. Sealing hides the VALARM ATTENDEE before the alarm task runs, and the
  seal policy keeps only TRIGGER, ACTION, REPEAT and DURATION visible in a
  VALARM; a red test would need a planted unsealed key-account event. The
  alarm variant now sets `allow_external_rcpts` in key mode, so the recipient
  assertion fails if both protections regress, and a start-time assertion can
  fail on its own.
- P9: default-calendar name validation against `$za$` stays unimplemented
  (accepted release 1 risk). P10: the CI trigger on push to `zero-access` runs
  on Jay's fork only.
- Task 4's `webdav_tests` variants: copy_move (cross-account directions and a
  group-member case), acl, alarm, scheduling. John is not a member of the
  support group, so his 403s may be plain permissions; the jane directions are
  the gate-load-bearing ones.

## Decisions that are Jay's to confirm or reverse

Decided 2026-10-08: all five candidates were accepted as recommended and
are now spec revision 7, with one refinement to candidate 2: the generic
alarm email keeps its link, because collection path names are visible by
structure and without the link a reminder cannot be traced to its event;
only the organizer row was dropped (plan 4,
`2026-10-08-zero-access-4-revision7.md`). Candidate 4 was implemented as
proposed: a sole untouched default calendar no longer blocks setup. The
three non-spec items (R3, R5/R7/R15, R9) stand as recorded. The same plan
fixed the three findings of the 2026-10-08 review of PR #5: HTTP body
traces (key-account DAV bodies and credential headers), app-password
cleanup identity, and operator CORS headers on vault routes. The same
review of that fix found the login endpoint's traced body, which was closed
in the same plan, and the app-password test needed a fourth test-mode pause
slot after the registry write. The list below is kept as the record of what
was decided.

Candidates for spec revision 7:

1. `ParticipantIdentity/changes` answers `cannotCalculateChanges` (upstream
   does not support it) for a key account, not `accountNotSupportedByMethod`
   as spec 9 words it. Either record the exception or gate the `_` arm in
   `changes/get.rs`.
2. The generic alarm email carries the account as organizer and a webcal link
   containing the calendar slug (the path name) as well as the event's
   filename. Spec 9 says "no organizer" and spec 2's visible list names the
   filename only. Pick: drop the organizer row and slug, or amend the spec.
3. The OPTIONS behaviour (R12) as a recorded exception to invariant 9: DAV
   OPTIONS now authenticates when credentials are present, so a key account's
   header differs and a wrong credential counts as a failed attempt.
4. Whether a sole empty default calendar should block setup (final review
   Minor 8). If anything builds a pending account's calendar resource cache,
   upstream creates the default calendar (`groupware/src/cache/calcard.rs:113`);
   the new setup check then answers 409 permanently. No trigger was found on
   this branch; every known path is gated before the fetch. A sole default
   calendar with no events carries no user data, so the check could ignore it.
   There is no operator path to remove it (see Deferred findings).
5. The mail index of key accounts (R11). Mail is unsealed in release 1, so
   its full-text index and the generic alarm email's index entries are
   expected; a later mail release revisits it. Record it in the spec as an
   accepted scope limit.

Also Jay's, but not spec text:

- R3's orphan-id fail-open (an unknown account id reaches upstream code
  that has no key to unseal with; the data is ciphertext, so the cost is a
  confusing error, not a leak).
- R5, R7 and R15: three gates with no red test, kept as defence in depth.
- R9: the upstream fmt commit in a test file.

## Deferred findings (reviewed, not fixed), by area

Plan 5 (`docs/superpowers/plans/2026-10-09-zero-access-5-deferred-findings.md`,
2026-10-09) closed the items marked **Closed (plan 5)** below. Spec
revision 7 had already decided the OPTIONS fail-open, the anonymous rate
limit on an unparseable OPTIONS header and the check-then-commit window; they
are marked **Decided (revision 7)**. The sole-default-calendar decision (spec
4.1) has no bullet of its own here. **No action (plan 5)** marks a finding
that records no hole to fix. Plan 6
(`docs/superpowers/plans/2026-10-10-zero-access-6-index-and-traces.md`,
2026-10-10) closed the items marked **Closed (plan 6)**.

Security and robustness:

- A pending account refused at setup for stray data has no operator path to
  remove it except direct store writes (Task 0; Minor 8 above).
- **Decided (revision 7)** OPTIONS fail-open (R12, re-review): a key account whose OPTIONS
  authentication fails transiently (in-flight limit, store error, fail2ban
  ban) receives upstream's header including `calendar-auto-schedule`. Clients
  cache OPTIONS capabilities at setup, so a failure at that moment leaves the
  client believing the server sends invitations.
- **Closed (plan 5)** Authentication errors on OPTIONS are swallowed, so the usual `Auth(Failed)`
  or `AuthenticationBan` error is not emitted through request error reporting
  (fail2ban accounting still runs inside `authentication_failure`).
- **Decided (revision 7)** `za_is_key_account_request` checks for the presence of an `Authorization`
  header while `authenticate_headers` parses it, so an unparseable or
  non-Basic/Bearer header on OPTIONS charges the anonymous rate limit, which
  upstream never did there.
- **Decided (revision 7)** The check-then-commit window at setup remains in principle; ingest gating
  closes it in practice.
- **Closed (plan 5)** `ItipMessageError` is logged on every key-account PUT (static reason, no
  content, but trace noise).
- **Closed (plan 7)** Blob decode in the leak scanner works only when the blob store is the data
  store; with an FS or S3 blob store only mail reachable through
  `fetch_email` is decoded. Plan 7 decodes every blob the walk finds a
  record or link for through the configured blob store (`scan` in
  `tests/src/za/leak.rs`); a blob that fails to read is a violation, and CI
  runs `za_tests` a second time with `BLOB_STORE=FileSystem`. S3 stays
  covered only by that code path, since no CI step runs it, and on a
  filesystem store an orphan blob with no link record is not scanned. The archive marker bits are mirrored from private
  constants in `store/src/write/serialize.rs`; the event and calendar counters
  fail loudly if they drift.
- **Closed (plan 5)** A destroyed account's pending index task has no regression test; the
  `try_account` fix (R8) is covered only by the plain-mode suites.
- **Closed (plan 6)** `RuleExpansionError` reasons still carry calcard error strings (RRULE is
  visible) and the `query.rs` trace carries no account or document id.
- **Closed (plan 5)** Account lookups: `acl.rs` and `scheduling.rs` use `account()` where every
  other gate uses `try_account`; four inline gates in JMAP and three in
  groupware could reuse dav's private `is_key_account` helper.
- **No action (plan 5)** The gates in Task 2 key off the authenticated account
  (`scheduling_account_info`); a plain writer into a key calendar is stopped by
  Task 1 and plan 2's gate.
- **Closed (plan 6)** R8's fall-through can index a destroyed key account's sealed archive:
  if a calendar index task runs after the registry delete but before
  `DestroyAccount` removes the data, the key-account gate sees no account
  and the builder indexes the sealed tree, which exposes only visible
  metadata. `DestroyAccount` unindexes calendars before destroying the
  data, so a write landing in between leaves an orphan search entry. The
  window is milliseconds after a PUT in the product build; no test can
  open it deterministically. Plan 6 skips sealed archives in
  `build_calendar_document`.

Tests:

- **Closed (plan 5)** The `zero-access` CI job's plain-mode `webdav_tests` step has no retry for
  the known `cal_itip` flake, so CI will go red intermittently until that
  sub-test is fixed or retried.
- **Closed (plan 7)** The post-DELETE CANCEL checks in `za_variants::scheduling` and
  `gating::test_scheduling` are smoke checks: key accounts never get a
  schedule tag, so they cannot fail. The scheduling variant asserts that
  jane's itip inbox holds one href, which is only the collection. Plan 7:
  `gating::test_scheduling` deletes a planted plaintext event that carries a
  schedule tag (`plant_event`, `tests/src/utils/za.rs`) and asserts no
  CANCEL reaches plain's mailbox (M5 fails it); both tests now assert the
  stored event has no schedule tag, the variant's comment points to the
  planted-event test, and a comment says why its inbox holds one href.
- **Closed (plan 7)** Negative inbox assertions rely on fixed sleeps (about 700 ms, upstream's
  pattern); the cleanup loop in `test_scheduling` asserts nothing; the ETag
  check's comment overstates what it covers. Plan 7: `wait_for_delivery`
  (`tests/src/utils/za.rs`) waits until the SMTP queue is empty, and the
  sender and recipient mailbox counts make the negative checks fail when
  mail is sent (M6b); the loop asserts exactly one reply; the attendee copy
  is a planted plaintext event and its comment says why only the gate keeps
  it unchanged (M7 fails the ETag check).
- Variants are not mutation-tested except where recorded (outbox, availability
  and the OPTIONS and `Principal/get` red runs) (**closed, plan 7**: "Mutation
  runs (plan 7)"); some 403s in the variants
  come from ordinary permissions, not the gate; refused COPY or MOVE sources
  and destinations are not checked directly; the inbox `len() == 1` needs a
  comment (**closed, plan 7**); client selection uses `path.contains("john")`.
- The `CalendarEvent/copy` test cannot tell the `from_account_id` gate from
  the target gate (**closed, plan 7**: `gating::test` copies into plain's
  account from key1's, so only the source gate can refuse it; M13 fails it);
  the secondary-account session filter is untested (no
  non-key account can reach a key account now; unchanged in plan 7); the shared-calendar read
  asserts status only.
- Task 0's test asserts the 409 over HTTP only, not the unchanged vault
  record; its planting block duplicates key4's; `za_assert_no_calendar_data`
  returns `Ok(Some(409))` where `za_assert_enabled` returns an error.
- The leak scanner: no positive control plants a calendar search-index entry,
  so the index check proves the key layout through mail entries only (**closed, plan 5**); the
  task-queue channel is scanned both before and after the drain, but the
  alarm email assertion does not prove the alarm fired for the right reason;
  silent blob decode failures are not asserted (**closed, plan 7**: a blob that
  fails to read is a violation); the fixed `SUBSPACES` list
  needs updating whenever upstream adds a subspace; allprop PROPFIND may omit
  `calendar-timezone`; CI step ordering is now behind the `style` job only.
- Final review "can stay": no trace-event test for the removed trace
  content (**closed, plan 5**); the alarm override has no red test (R15)
  (**closed, plan 7**: `za_variants::alarm_override`, M10); `webdav_tests` has no
  queue capture, so nothing asserts that no mail was queued to the
  `john_doe@unknown.com` attendee in the alarm variant (**closed, plan 7**,
  differently from the ask: `capture_queue()` is unusable in these suites
  because it stops local delivery, so `wait_for_delivery` and
  `queued_recipients` read the persisted SMTP queue instead;
  `alarm_override` plants a plaintext event with an external VALARM
  ATTENDEE, waits for the queue to drain, and asserts the one email is the
  alarm itself, sent to the account).
- The key-mode branch in upstream `tests/src/webdav/basic.rs` (OPTIONS header
  as john) is a new merge surface in an upstream test file, like
  `principals.rs` and `mod.rs`.

Structure:

- Fork diff in `alarm.rs` after R13 is small; before it, the `if !generic`
  re-indent would have been the largest merge surface of the plan.
- Task 0's `CLAUDE.md` rustfmt line for http was wrong and is corrected in
  Task 6 (`cargo fmt --manifest-path crates/http/Cargo.toml -- --check`).
- The plan 3 task text predates P1 to P10 and the final rulings; this note
  and the ledger win.

Plan 4 (revision 7 follow-ups), whole-branch review:

- Mail-protocol raw-input traces (`imap.raw-input` and the POP3,
  ManageSieve and SMTP equivalents) are upstream's and still record the
  authentication exchange, so an operator who enables them can capture a
  key account's password or app password. Spec section 10 records this as
  a release 1 limit, since the calendar is served over HTTP only.
- A WebDAV `Error` event's `Reason` carries a parse error's
  `UnexpectedToken.found`, and a DAV precondition's `condition.details` is
  traced as `Reason` on the method's event; both can carry text from a key
  account's calendar-query. Pre-existing upstream behaviour, and not a body
  trace, so the section 10 redaction does not cover it.
- Operator `Vary` or `Cache-Control` entries in `http.headers` are inserted
  after the vault handler builds its response and overwrite the vault's
  `Vary: Origin` and `no-store`; only `access-control-*` entries are kept
  off vault routes. Pre-existing.
- The setup check-then-commit window (see "Security and robustness") is
  easier to hit now that setup tolerates an untouched default calendar: a
  client syncing that calendar during setup can leave a plaintext event
  that is sealed only on its next write (spec section 4.1).

## Mutation runs (plan 7)

Plan 7 (`docs/superpowers/plans/2026-10-10-zero-access-7-test-hardening.md`,
Task 7, 2026-10-10) removed each gate the key-mode suites cover, one edit at
a time, ran `za_tests` and then key-mode `webdav_tests`, recorded each
suite's first failure and reverted the edit before the next run. Line numbers
are those of the product files at `main`. A failure inside a helper
(`with_status` at `tests/src/utils/webdav.rs:599`, the property check at
`:1061`, the header check at `:614`) is given at the calling test line.
`alarm_override` was tightened before M10 so that it identifies the alarm
email by its sender: under M10 the mail that reaches john is a delivery
failure bounce, which is addressed to john and also carries
`Auto-Submitted: auto-generated`.

| Id | Gate | Edit | `za_tests` first failure | Key-mode `webdav_tests` first failure |
|----|------|------|--------------------------|----------------------------------------|
| M1 | Cross-account COPY/MOVE, `crates/dav/src/common/za.rs:96` (`za_refuse_cross_account`) | `if false && from_account_id != ...` | `tests/src/za/dav_gate.rs:307`: key1's COPY to plain's calendar, "Expected 403 Forbidden but got 201 Created" | `tests/src/webdav/za_variants.rs:124` (`copy_move`): jane's COPY to support's calendar, "Expected 403 Forbidden but got 201 Created" |
| M2 | ACL on key calendars, `crates/dav/src/common/acl.rs:131` | `if false && collection == ...` | `tests/src/za/gating.rs:102`: key1's ACL on its calendar, "Expected 403 Forbidden but got 200 OK" | `za_variants.rs:192` (`acl`): john's ACL, "Expected 403 Forbidden but got 200 OK" |
| M3 | DAV URI gate, `crates/dav/src/common/uri.rs:113` | the `za_session_keys` call removed | `dav_gate.rs:78`: admin PROPFIND of `/dav/itip/key1@example.com/`, "Expected 403 Forbidden but got 207 Multi-Status". The PROPFINDs of `/dav/cal/key1@example.com/` before it still answer 403: the PROPFIND loader's per-item gate (`za_archive_view`, `za.rs:171`) refuses them; the scheduling inbox has only the URI gate. | passed. Every client holds its own keys; jane's PROPFIND of john's calendar is refused by upstream's access check (`uri.rs:101-104`), and no grant can exist because of M2's gate. |
| M4 | Keyless token refused, `crates/dav/src/common/za.rs:66-73` (`za_session_keys`) | `None => Ok(None)` | `dav_gate.rs:69`: master-user PROPFIND, "Expected 403 Forbidden but got 207 Multi-Status" | passed (as M3) |
| M5 | DELETE `send_itip`, `crates/dav/src/calendar/delete.rs:81` | the `is_key_account` line deleted | `gating.rs:586`: planted DELETE, "a CANCEL left the key organizer" (left 1, right 0) | passed: a key account's events carry no schedule tag, so no CANCEL can be built |
| M6 | `ItipSendStatus::KeyAccount`, `crates/groupware/src/calendar/itip.rs:1075` | `} else if false && account_info...` | `tests/src/za/tracing.rs:721`: "plain-account scheduling reason not traced on both PUT paths" (left 4, right 2): the key account's PUTs now trace upstream's reasons. `tracing` runs before `gating`. | `za_variants.rs:362` (`scheduling`): `response.headers.get("schedule-tag").is_none()` |
| M6b | as M6 | M6's edit, with the two schedule-tag assertions (`gating.rs:492-496`, `za_variants.rs:362`) and `tracing.rs:721-725` commented out for the run | `gating.rs:499`: `mail_count(test, plain_id) == plain_mail` after the key organizer's PUT (left 1, right 0) | `za_variants.rs:368`: `mail_count(jane) == jane_mail` after john's PUT (left 1, right 0) |
| M7 | RSVP attendee copy, `crates/groupware/src/calendar/itip.rs:667` | `if false && server` | `gating.rs:688`: the copy's ETag changed after plain's RSVP. With that assertion disabled, `:696-699` fails with "the key attendee's copy was rewritten" (Task 1's red run). | passed: john sends no RSVP |
| M8 | iMIP ingest, `crates/email/src/message/ingest.rs:367` | `&& !account.is_key_account()` deleted | `gating.rs:620`: key1's scheduling inbox holds `inbox/1.ics` after plain's invitation | passed: john sends nothing |
| M9 | Generic alarm, `crates/services/src/task_manager/alarm.rs:473` | `let generic = false;` | passed: no alarm fires in `za_tests` | `za_variants.rs:316` (`alarm`): "organizer row present in the generic alarm email" |
| M10 | Alarm recipient, `alarm.rs:541` | `rcpt_to = None;` deleted | passed | `za_variants.rs:476` (`alarm_override`): "the email is the alarm", From `MAILER-DAEMON@example.org` (the bounce of the alarm sent to the external attendee) |
| M11 | Schedule URLs hidden, `crates/dav/src/principal/propfind.rs:312` | `if false && account.is_key_account()` | `gating.rs:143`: key1's `schedule-inbox-URL`, "Expected status 404 Not Found, but got 200 OK" | `tests/src/webdav/principals.rs:139`: the same property of a key principal, same message |
| M12 | OPTIONS header, `crates/http/src/request.rs:341` | `if false && za_is_key_account_request(...)` | `tracing.rs:707`: "OPTIONS authentication failure not reported" (with the check skipped, OPTIONS no longer authenticates the credential) | `tests/src/webdav/basic.rs:19`: "Header dav:1, 2, 3, access-control, extended-mkcol, calendar-access, calendar-no-timezone, addressbook not found." |
| M13 | Copy source, `crates/jmap/src/api/request.rs:648` | the `from_account_id` line deleted | `gating.rs:125`: `CalendarEvent/copy` answered, left `None`, right `Some("accountNotSupportedByMethod")` | passed: the variants make no JMAP call |
| M14 | Bearer on key calendars | M4's edit, with `dav_gate.rs:65-78` (master-user and admin PROPFINDs) commented out for the run | `dav_gate.rs:129`: the Bearer PROPFIND loop, "Expected 403 Forbidden but got 207 Multi-Status" | passed (as M4) |

Every gate fails at least one suite, so no gap needed a new test or a
"layered behind" ruling. The key-mode variants miss M3, M4, M5, M7, M8 and
M13 because they never send the request that reaches the gate (no keyless
client, no schedule tag, no RSVP, no iMIP, no JMAP); `za_tests` covers each.
M6b shows the sender-side mail counts catch a key organizer that sends, apart
from the schedule-tag assertions that fire first.

The red runs recorded earlier are not repeated here: the outbox free-busy
loop, the availability gate (R4), the OPTIONS and `Principal/get` runs, and
the first JMAP gate run (see "Deviations from the plan").

## Facts for the account web page and the next release

Gate inventory (every place a key account is refused or treated differently;
all use `is_key_account()` on the cached account, via `try_account` unless
noted):

- `crates/http/src/api/vault.rs`: `za_assert_no_calendar_data` (setup token
  and setup, 409).
- `crates/http/src/request.rs`: the DAV OPTIONS arm and
  `za_is_key_account_request` (omits `calendar-auto-schedule`, R12).
- `crates/dav/src/common/za.rs` (plan 2): the URI gate, free-busy and
  cross-account refusals.
- `crates/dav/src/common/acl.rs`: `handle_acl_request` (403 on a Calendar).
- `crates/dav/src/principal/propfind.rs`: schedule inbox and outbox URLs are
  404 for a key principal.
- `crates/dav/src/calendar/scheduling.rs`: outbox free-busy attendee loop
  (3.7 item per key attendee).
- `crates/dav/src/calendar/delete.rs`: `send_itip` off.
- `crates/dav/src/calendar/query.rs` and `crates/groupware/src/calendar/dates.rs`:
  traces log UIDs (all accounts).
- `crates/groupware/src/calendar/itip.rs`: `ItipSendStatus::KeyAccount`,
  `http_rsvp_handle` (InvalidLink) and `http_rsvp_attendee_copy`.
- `crates/email/src/message/ingest.rs`: the iMIP ingest condition.
- `crates/services/src/task_manager/index.rs`: `build_calendar_document`
  (key-account gate; sealed-archive check, plan 6).
- `crates/services/src/task_manager/alarm.rs`: generic alarm clearing.
- `crates/jmap/src/api/request.rs`: `za_assert_calendar_allowed` in the Get,
  Query and Set arms of Calendar, CalendarEvent, CalendarEventNotification,
  ParticipantIdentity, `CalendarEvent/copy` (both accounts) and
  `CalendarEvent/parse`.
- `crates/jmap/src/changes/get.rs`: `ChangesLookup::changes` (also
  queryChanges).
- `crates/jmap/src/api/session.rs`: calendars capabilities filtered.
- `crates/jmap/src/principal/get.rs`: `za_shown_capability`.
- `crates/jmap/src/principal/availability.rs`: key accounts skipped.
- `crates/jmap/src/blob/download.rs`: `has_access_blob`.

Test modules and what each covers:

- `tests/src/za/gating.rs`: `test` (ACL, schedule URLs, outbox free-busy,
  every JMAP method refusal with plain controls, session, `Principal/get`,
  availability as admin, OPTIONS three ways), `test_scheduling` (key
  organizer, RSVP, invitation to a key attendee, CANCEL).
- `tests/src/za/leak.rs`: the leak scan (below). `tests/src/za/setup.rs`:
  `test_data_check`.
- `tests/src/webdav/za_variants.rs`: the four key-mode variants; `mod.rs`
  wires them in place of the plan 2 skips.

Running the leak scanner: it runs last inside `za_tests` (before key accounts
are destroyed), so use
`STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests --
--nocapture`; there is no separate test. It walks every listed subspace for a
fixed canary list in raw keys and values, decodes blobs and archives, checks
the structure of every calendar and event archive of the account, scans alarm
emails, and prints a line such as `Leak scan: 910 records, 31 archives, 3
blobs, 460 search index records (116 of the account, none calendar), 4
events, 3 calendars, 1 emails`. A failure panics with violation lines of the
form `subspace 'p' key [..]: raw value contains <canary>`, `decoded archive
contains <canary>`, `component N (VEvent) has sealed-class property X`, or
`missing X-ZA-KEY`; the subspace letter and key locate the record. The
counters are asserted (`events >= 4`, `calendars >= 2`, `blobs > 0`,
`emails >= 1`), and a planted unsealed copy of the fixture (the negative
control) must produce violations, so a clean run cannot mean a blind scanner.

CI layout: job `style` (`cargo fmt --all --check`); job `test` (upstream
suites, needs `style`); job `zero-access` (needs `style`; vault and groupware
unit tests, `za::za_tests`, `za::za_tests` again with `BLOB_STORE:
FileSystem` (plan 7), `webdav::webdav_tests`, and the same with
`ZA_KEY_ACCOUNTS: "1"`). The env sets `STORE: RocksDb` and `RUST_MIN_STACK`.
Upstream's `cal_itip` flake can still fail the job; rerun once.

Known upstream warnings: 24 in the product build (store 1, common 18, jmap 3
in `registry/mapping`, services 2 in `task_manager/index.rs`), all from
upstream's non-enterprise stubs; the store crate also emits an unused
`LookupStore` import when building units.

Merge hotspots this plan added: the `request.rs` DAV OPTIONS arm and
`za_is_key_account_request`; `alarm.rs` (post-loop clearing block and the
subject change); the JMAP `request.rs` arms and `changes/get.rs`; plus
`principal/get.rs`, `blob/download.rs` and, in upstream tests,
`tests/src/webdav/{basic,principals,mod}.rs` and
`tests/src/jmap/mail/set.rs` (R9).

For the account page: key accounts now see no calendars in JMAP, so the page
must not offer sharing or scheduling; a pending account that holds calendar
events, scheduling notifications, or any calendar collection other than a sole
default calendar created by the server with untouched preferences gets 409
(`account already holds calendar data`) at `setup` (spec 4.1; Minor 8). The manual client checklist is
`docs/zero-access/manual-checklist.md`
(item 13 is the invitation check).

## Tests worth adding

- **Added (plan 5)** A positive control that plants a calendar search-index entry for a key
  account, so the index check proves the Calendar class byte and not only the
  mail entries.
- **Added (plan 5)** A destroyed account's pending index task (R8): delete a key account with
  an `IndexDocument` task queued and assert the task drains.
- **Added (plan 5)** A trace event with an in-process trace subscriber that asserts the `RuleExpansionError`
  traces carry UIDs and no iCalendar text.
- **Closed (plan 7)** Mutation runs for the variants (remove each gate and confirm the variant
  fails), in particular `copy_move`, `acl` and the scheduling variant. See
  "Mutation runs (plan 7)".
- **Closed (plan 7)** An OAuth Bearer gate case for a key account's calendar without keys (still
  no helper in the tests crate). `tests/src/za/dav_gate.rs` mints the token
  in process (`encode_access_token`) and expects 403 on the calendar,
  default-calendar and itip PROPFINDs and the calendar-query REPORT, with a
  plain-account Bearer control (M14).
- **Closed (plan 7)** A red test for the alarm recipient override (R15) with a planted unsealed
  key-account event carrying an external VALARM ATTENDEE; queue capture in
  `webdav_tests` to assert nothing is queued to the external recipient.
  `za_variants::alarm_override` (M10); the queue is read through
  `wait_for_delivery`, not `capture_queue()` (see "Deferred findings",
  Tests).
- The `from_account_id` gate of `CalendarEvent/copy` on its own
  (**closed, plan 7**: `gating::test`, M13), and the
  secondary-account session filter once a key account can be a secondary
  account (still untested, unchanged: no non-key account can reach a key
  account now).
- `ParticipantIdentity/changes` stays untested, unchanged: it answers
  upstream's `cannotCalculateChanges` for every account (candidate 1 under
  "Decisions that are Jay's"; spec revision 7 records the exception), so
  there is no key-account gate to test.
- The plan 2 list still stands (outbox `Withheld` path now unreachable for key
  attendees; tampered-event 500; 304 and `If-Match` cases). Plan 7 marks the
  items it closes there.
