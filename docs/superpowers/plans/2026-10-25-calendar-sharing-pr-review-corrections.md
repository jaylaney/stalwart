# Calendar Sharing PR Review Corrections Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Address the worthwhile current-head review comments on PR58 with focused fixes,
regressions and explicit dispositions.

**Architecture:** Keep revision14's authority, key domains, per-write fences, bounded retries and
ordinary-account behavior. Correct three demonstrated runtime edge cases, retain safe MOVE conflicts, restore trace/work
boundaries and seed an existing request-local memo. Document accepted response/corruption limits;
defer broad scheduling, cache and authorization-kernel refactors.

**Tech Stack:** Rust2024/stable, existing Tokio/RocksDB integration harness, pinned Python3.12
CalDAV client suite, non-enterprise shipping build.

**Spec:** [revision14 sharing design](../specs/2026-10-22-zero-access-sharing-design.md);
[accepted outcome rulings](2026-10-23-zero-access-plan14-outcome.md).
Review input is PR58's17 inline threads, captured at head
`2eec6b61546a4c6dde17bcd819e0e153f1a9cc83` in ignored
`.superpowers/pr58-comment-triage-2026-10-25/comments.json`.

## Global Constraints

- Existing worktree /Users/jay/Development/stalwart/.worktrees/release3-mail-spec, branch
  codex/calendar-sharing, fork PR https://github.com/jaylaney/stalwart/pull/58; keep draft.
- No stored layout, enterprise product build, pin/dependency/compatibility policy, native account,
  .run, upstream, unrelated mail-indexing or deployment changes.
- Preserve vault/Account/calendar/event assertions, owner/recipient identity, original event CAS,
  no repeated scheduling fan-out, precise error distinctions and accepted prefix semantics.
- Primary owns Git, PR and final integration. One source writer and one local Cargo/client owner;
  no nested agents, concurrent server suites or shared build-output owners.
- Model ceiling Sol6.1/lower for implementation. Use requested Astra/xhigh independent PR-review
  seat for the completed correction delta and related contracts.
- Preserve the unrelated untracked mail-design review, SHA256
  1d7f59cb0c0cf5deb34445061efc74314cc64757748eeb9214127c8d162a6cd0.
- If a cybersecurity filter blocks execution, report the bounded task and stop automatic
  retries/rephrasing; prepare the user's requested task-specific handoff.

## Review Focus

1. URI pchar and already-escaped slugs must preserve allowed tombstones without sibling disclosure.
2. Collection MOVE must retain the source snapshot/version fence and newer source metadata
   on conflict, with fresh source/target domain checks and accepted committed prefixes.
3. Only precisely missing calendars are skipped; corrupt archives and genuine storage failures
   must remain errors.
4. Pin-only writes cannot spend conversion retries, while final grant/re-key still asserts the
   pin used for the decision and all conversion writes retain their other fences.
5. Plain-account paths retain trace locations and avoid key-only identity/archive preparation;
   assisted discovery still rechecks access even when its fresh contexts seed a memo.

### Task 1: Focused runtime and regression corrections

**Files:**
- Modify crates/dav/src/common/propfind.rs.
- Modify crates/dav/src/calendar/copy_move.rs.
- Modify crates/dav/src/calendar/update.rs and delete.rs.
- Modify crates/http/src/api/shares.rs.
- Modify crates/common/src/za/test_hooks.rs for necessary test-only list/final-MOVE pauses and observations.
- Modify crates/common/src/za/mod.rs for feature-safe HTTP hook forwarding functions.
- Modify tests/src/za/sharing_races.rs and/or sharing.rs.
- Optional focused tests/src/za/sharing_review.rs plus its module/call in tests/src/za/mod.rs,
  if useful for fail-fast full-suite regressions.
- No groupware/vault authorization/cache/storage production refactor is in this task.
- Primary owns documentation/disposition edits while this writer works.

**Interfaces and rulings:**
- 4371584284: construct vanished prefixes with groupware::encode_path_segment, matching resource
  paths. Preserve all eligibility, boundary and requested-subtree filtering. Reuse
  requested_calendar instead of recalculating addressed_id (part of4371584310).
- 4371584303: seed data.calendar_memo with freshly resolved assisted foreign contexts, keyed by
  (shared_account_id, calendar_id), before item emission. Keep per-item check_access.
- 4371584291: retain the original preflight source archive as the final indexed-delete CAS.
  The proposed refresh would delete newer metadata that was not copied to the destination and
  could satisfy a stale conditional MOVE. Keep fresh source/target domain checks and original
  version fence for conditional and unconditional requests. Metadata/slug changes return409,
  preserving newer source metadata; committed child prefixes remain accepted. Document this
  adjudicated correction to the earlier plan instead of adding an atomic metadata-transfer design.
- 4371584292: own and shared_with_me list loops omit only ZaCalendarError::Store whose event is
  precisely StoreEvent::NotFound. Preserve all other errors and existing Seal(Aead) name:null.
- 4371678740: pass conversion an otherwise identical guard with pin_assertion:None. Retain
  original grant guard and final za_share_apply pin CAS. Do not remove vault/Account/calendar/
  event fences or relax the final pin decision.
- 4371584294: restore lost caused_by(trc::location!()) context on update/create/queue/commit and
  delete error paths. Gate key-only role rebuilding and extra event deserialization behind the
  actual key-owner path; retain ordinary scheduling control flow and results.
- 4371678756: short invariant comment at both queue/retry sites: retries are sealed/key-only, so
  consuming plain itip_messages is not a reachable retry path. No replay redesign.
- 4371679240: correct the barrier-only concurrency comment, assert default-calendar DELETE
  status204|404, remove the tautological403 assertion while preserving the real match check.
- Keep broad scheduling-helper extraction and repeated collection-delete resolver optimization
  deferred. The latter requires per-physical-chunk fence coverage, not merely a hoisted assertion.

- [x] **1. Add meaningful regression fixtures before runtime changes.** Public DAV/API or real
  store-backed tests for pchar/valid-percent tombstone sync with no private sibling paths;
  conditional and unconditional source-PROPPATCH collection MOVE returning409 with newer
  source property retained, original destination property and one committed destination child;
  concurrent source rename retained on409; both list enumeration/delete races with200 omission
  and non-NotFound failures still errors; pin-only update during conversion with no conversion
  assertion losses and final-grant pin CAS retained. A final grant decision loss may still
  require a successful conversion rewalk before publication; do not claim that is eliminated.
  Reuse COPY_PAUSE, SHARE_REWRAP_PAUSE and existing
  loss/pin fixtures where possible. Add narrow test-mode own/foreign list pause seams,
  CALENDAR_MOVE_DELETE_PAUSE immediately before the final indexed-delete commit, and a
  conversion-loss observation if required. The final-MOVE pause must prove a post-read source
  metadata change returns409 with source retained and the moved child present exactly once. Include assisted-discovery resolver read-count and
  preserved forbidden-path checks for the memo change. Whole suites only; no hidden filters or
  weakened oracles. A focused new module may run early within za_tests to fail fast without
  introducing a second server framework or changing the old suite's execution.
- [x] **2. Observe and retain RED for demonstrated fixes.** Use existing suite entrypoints and
  permitted unit tests. Diagnose setup/compiler failures separately; no production mutations,
  fake assertions or test_mode deploys. Cover ordinary create/update/delete behavior and trace
  locations with existing tests or a narrow meaningful trace check.
- [x] **3. Implement the smallest fixes above and run covering GREEN.** Preserve exact requested
  domains, path identities and assertions. Confirm late source/archive changes still conflict;
  do not retry/replay collection creation or fan-out. Append complete command/output evidence.
- [x] **4. Finish source and test cleanup.** Format only after code is settled, record all changed
  paths/hashes, hand back source ownership and report exact outcomes/unknowns.

### Task 2: Primary documentation, qualification and integration

**Files:** docs/zero-access/calendar-sharing-api.md;
docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md only for precise residual/hook
clarifications; a neighboring PR-comment disposition/outcome;
this plan's completion checkboxes. Read the immutable input and three read-only triage reports.

- [x] **1. Record every thread's disposition.** Selected runtime fixes above; document rather
  than change4371678749's specified403 existence split and4371678745's degraded caller-private-key
  request-level500 behavior. Distinguish grant-ring AEAD name:null from earlier private-key/
  vault/store failures. Record whether discovery uses request/item500.
- [x] **2. Clarify API recovery.** 4371678753: an error can follow acknowledged conversion/grant/
  revoke and invalidation; re-list/retry idempotently, no no-write promise. 4371678759: ledger/ACL
  mismatch fails closed, no plaintext ACL-derived repair; preserve readable content and use a
  verified consistent backup or owner export/recreate/regrant where possible. Do not prescribe
  blind deletion or claim universal recovery from arbitrary corruption.
- [x] **3. Explicitly defer lower-value changes.** 4371584299 memo-hit allocation,4371584307 home
  allocation/corrupt-state empty-home policy,4371584314 pruning,4371584301 collection-delete
  resolver optimization, and broad parts of4371584310/4371584294. State why without calling
  accepted behavior a fixed bug. Record selected parts of multi-part comments separately.
- [x] **4. Run fresh relevant verification serially.** Core vault/groupware/common units, HTTP
  test_mode units, whole ZA suite, ordinary and key WebDAV, workspace formatting, shipping RocksDB
  release build without enterprise/test_mode. Run the existing30 harness tests and full72plain/
  77key pinned client matrix on the new binary. Do not reuse the old product binary after Rust
  changes. No57-row mutation or3,000-event benchmark rerun is required; their old revision evidence
  remains historical. The unrelated key-JMAP CI cleanup failure stays separately qualified.
- [x] **5. Independent review.** One fresh Astra/xhigh read-only review of the complete correction
  diff, all selected/disposition contracts and relevant neighboring paths. Fix confirmed new
  issues with the original implementer and one scoped re-review per changed round. No redundant
  whole-PR scan absent a new reason.
- [x] **6. Commit and push exact reviewed source/docs to existing draft PR.** Verify actual staged
  bytes, protected file, current PR head and validation provenance; update description with
  corrections and current qualification limits. No merge/ready transition or public comment
  replies without explicit authorization.

## Execution authorization and design decision

The user requested consideration of the actual PR comments, then explicitly said “Go ahead and
address the ones that you feel are worth it.” This authorizes selected local fixes, their
regressions and normal updates to the existing draft PR. The architecture remains the approved
revision14 design; no new product policy or approval gate is introduced. Retaining the original
MOVE source-version fence and deferring private-key partial-listing, cache and optimization
changes are primary rulings following independent review. Cost if wrong is visible focused
rework, not a stored-format migration.
