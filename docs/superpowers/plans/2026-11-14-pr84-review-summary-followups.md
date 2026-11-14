# PR 84 review-summary follow-ups

> **For agentic workers:** Use subagent-driven-development. The primary owns Git,
> shared builds, integration, review and publication. Implementers own disjoint files.

**Goal:** Close the worthwhile loader, test-strength and documentation observations
from PR 84's review summaries, then answer every summary observation explicitly.

**Architecture:** Keep the authenticated manifest, resident-generation recheck and
exclusive reload fallback. Move only the warm pass's manifest I/O outside its index
read guard, then validate against the resident state reacquired after the read. Keep
deadline and retry semantics unchanged. Add regressions through production routes.

**Tech stack:** Rust/Tokio, existing ZA integration harness, GitHub review threads.

**Spec:** `../specs/2026-11-04-zero-access-search-design.md` (revision 37), particularly
authenticated view/load ordering, request deadlines and acknowledged-abort settlement.
Starting head: `fbcdba82346f4b831310a27a63c4b674b680e1b3`.

## Constraints and decisions

- Preserve authenticated target identity, stored formats, map/content guarantees,
  retry basis, ordinary-account behavior and the one-primary-abort trace contract.
- Keep unknown backend classes transient. Do not log raw errors or message content.
- Do not change CI baselines, selectors, timeouts, retries or retained coverage.
- Reuse the existing isolated `codex/pr84-search-ci-combined` worktree. No merge into
  main until final combined CI is green. Current CI qualifies the starting head only.
- The user approved these follow-ups after read-only adjudication. No new product
  mechanism or logging contract is approved by this plan.
- The alternative of merely bounding hook I/O would still hold the read guard during
  storage waits. Prefer releasing/rechecking the guard; keep exclusive reload ordering
  intact rather than broadening this change into a loader redesign.
- Model ceiling is unresolved in runtime metadata. Deliberately inherit the parent
  route and preserve independent review; do not claim named Sol/Astra reviews.

## Review focus

1. Warm manifest I/O must not block an index writer/new reader through a held read guard.
2. A resident swap during that I/O must invalidate the warm shortcut; a stale read must
   not return an obsolete head or settle the retry episode as though it were current.
3. Actual observer deadline expiration must remain neutral and release pending work.
4. Observing the manifest after acknowledged abort settlement must preserve cooldown.
5. IMAP saved selection, truncation, changed rows and unavailable-map/keyless combinations
   must retain exact selection and refusal semantics while client output is paused.

## Task 1: Warm manifest read outside the shared index guard

Files: `crates/common/src/za/search/load.rs`, test-only hooks in
`crates/common/src/za/test_hooks.rs`, new `tests/src/za/mail/pass_load.rs`, and its
module declaration in `tests/src/za/mail/mod.rs`.

- [x] Add an acknowledged-pause regression around warm manifest I/O. While parked,
  a writer must acquire the index guard. Exercise unchanged-head and moved-head paths.
- [x] Expose `pub async fn test(test: &mut TestServer)` from the new module; Task 2
  calls it from the shared focused/canonical PR84 fixture.
- [x] Run the focused suite against the old production lock scope and record the
  expected guard-acquisition failure before implementing the fix.
- [x] Preserve `za_search_load_for_pass`'s signature; read the manifest without the
  index guard, reacquire and compare to the current resident head before returning.
  If it no longer matches, use the existing serialized fresh-read reload fallback.
- [x] Verify stale-head/cancellation cases and independently review ordering.

## Task 2: Direct retry deadline and settlement coverage

Files: `tests/src/za/mail/index.rs`; test dependency configuration only if needed for
deterministic Tokio time. Do not edit Task 1's files without primary coordination.

- [x] Call `super::pass_load::test(test).await` from `pr84_retry_followup_tests`.
- [x] Add direct actual `timeout_at` coverage of the parked observer, separate from
  `SearchWaitDeadline` seam coverage. Avoid a setup race within a short wall-clock
  deadline; prefer controlled Tokio time without changing other server tasks' clocks.
- [x] In abort-bookkeeping coverage, make observation due after settlement, assert
  one manifest read and zero repeated load/open/upload/commit work, and retain the
  original retry deadline/class/streak and durable-state assertions.
- [x] Name the faulty production behavior each assertion would catch. Preserve
  existing seam coverage and do not claim mutation execution without a recorded run.

## Task 3: Focused IMAP matrix and upstream dependency documentation

Files: `tests/src/za/mail/threads/imap_guards.rs`; primary edits `CLAUDE.md`.

- [x] Add keyless metadata behavior against the unavailable-map fixture while retaining
  identity/grouping/content refusals required by the accepted contract.
- [x] Extend blocked-output FETCH coverage to saved `$` selection, message-limit
  truncation, and a nonempty CHANGEDSINCE result. Assert the selected identities and
  that a pass can publish while client output is parked.
- [x] Record `backend_error_class`'s generated StoreEvent dependency and fixed `other`
  fallback in CLAUDE's upstream-change checklist. Do not broaden logging.

## Task 4: Review, validation and comment disposition

- [x] Review each task's source and obtain fresh independent final review of the
  combined follow-up against the starting head. Reviewers do not approve their own work.
- [x] Run formatting/diff checks, relevant units, focused PR84 suite, then complete
  canonical ZA. Serialize port-sharing suites and all shared-target builds.
- [x] Record commands, source/binary identity, outcomes and any failures in an adjacent
  outcome note and ignored evidence ledger. New CI must target the published new head.
- [ ] Commit/push the fixes under the existing authorization; rerun affected CI and
  retention checks. Keep PR84 unmerged while these checks are pending.
- [ ] Post one explicit response for every low/informational summary observation.
  Resolve the 19 addressed original inline threads after verifying current applicability.
  Leave comment 4566621670 (unclassified manifest/secondary backend errors) unresolved;
  this plan does not change that diagnostic contract. Preserve any newly open finding.
