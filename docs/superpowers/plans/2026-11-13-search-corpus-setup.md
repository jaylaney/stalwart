# Differential Corpus Setup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox syntax for tracking.

**Goal:** Speed up the complete differential corpus setup and expose its progress.

**Architecture:** Preserve the serial durable delivery barriers and every coverage
assertion. Select faster polling only for this fixture, reuse keys per readback
batch, and report progress without changing awaited work.

**Tech Stack:** Rust, Tokio, existing Stalwart test harness; unchanged Cargo profiles.

**Spec:** `docs/superpowers/specs/2026-11-13-search-corpus-setup-design.md`

## Global Constraints

- Retain all 3,258 paired inputs, the three headerless fallbacks, serial ordering,
  timestamp guards, authenticated stored-MIME checks, 183 forms, K exception, G5,
  canonical inclusion and cleanup.
- Keep production code, corpus resources, query assertions, CI selection, test
  skips, deadlines, build profiles, and environment defaults unchanged.
- Existing callers retain 200 ms task and 100 ms SMTP polling; corpus setup selects
  10 ms with identical predicates and skip flags.
- No credentials, keys, message content/headers/IDs, or account names in new logs.
- Root owns docs, Git, builds, tests and publication. Implementation runs in a
  subagent; reviews are independent. No builds during timed runs; suites serialize.
- Inherit the parent route because the originating capability ceiling is unresolved.

## Review Focus

- An acknowledged message can still await durable removal or ordinary indexing.
- Headerless refusal and duplicate/suffix inputs must retain exact association.
- Keys must stay in their account/batch and every document must authenticate anew.
- Heartbeats must neither restart/cancel work nor delay completion or survive cancellation.
- Faster polling must not flood logs or alter other callers' predicates/deadlines.

### Task 1: Optimize the fixture and expose progress

**Files:**
- Modify: `tests/src/utils/storage.rs` — one task-drain implementation with interval parameter and existing default wrapper.
- Modify: `tests/src/utils/za.rs` — interval-selected delivery barrier and SMTP drain, retaining default wrappers.
- Modify: `tests/src/za/mail/mod.rs` — borrowed-key authenticated open, existing fresh-unlock wrapper unchanged to callers.
- Modify: `tests/src/za/mail/differential.rs` — scoped interval/key reuse, setup phase and heartbeat output.

**Interfaces:**
- Consume existing persisted TaskQueue/SearchQueue/SMTP predicates, `SessionKeys`,
  `DiagnosticStep`, and fixture assertions; inspect actual signatures before edits.
- Produce `wait_for_tasks_with_poll_interval` and `wait_for_delivery_with_poll_interval`
  with a `Duration` argument, preserving all existing entry points.
- Produce a borrowed-key message open helper used by the fresh-unlock wrapper and
  the identification loop; account identity must be bound to the borrowed keys.
- Keep progress observation private to differential setup; use a pinned future
  with a timer, no spawned task. Existing diagnostics may be reused/extended if
  their other callers' semantics remain unchanged.

- [x] Read the spec and ignored `recon.md` and `contract.md` reports.
- [x] Preserve all existing assertions. Make the smallest extractions so the
  default and optimized routes execute the same scan and decrypt implementations.
- [x] Add batch-local key reuse after inventory/count validation. Do not reuse in
  headerless verification, G5, authentication lifecycle cases, or across batches.
- [x] Add setup and bulk-index phase boundaries, completed-pair/batch progress,
  elapsed delivery-barrier and verification timing, and ten-second pending
  heartbeats. Avoid per-message begin/end output and nested active timers.
- [x] Add meaningful focused tests only where needed to establish new observer
  behavior (single future, prompt completion, cancellation); avoid tests that
  merely restate polling constants. Root executes all validation.
- [x] Return exact diff summary, invariants inspected, risks and proposed checks.
  Do not build, test, commit, push, or spawn agents.

### Task 2: Validate, independently review, and publish

**Files:**
- Create: `docs/superpowers/plans/2026-11-13-search-corpus-setup-outcome.md`.
- Update this plan's completion boxes and the ignored task ledger.

**Interfaces:**
- Consume Task 1's frozen diff/binary and the source-bound unchanged baseline.
- Produce full default/filesystem receipts, A/B setup measurements, independent
  source review, and a separate follow-up PR.

- [x] Root runs unchanged baseline `za::za_tests --exact --nocapture` with external
  timestamps; record binary SHA/source binding, environment and fresh fixture.
- [x] Independent reviewer checks the patch against the complete preservation
  checklist and the five review risks before final compilation.
- [x] Root runs rustfmt check, builds the tests target, and runs the final complete
  canonical suite in default and filesystem blob modes serially. Unit observer
  tests, if added, run separately. Record initial failures and any retries.
- [x] Compare the same coarse setup interval and full suite time. Report observed
  gain and remaining cost; do not claim hosted CI improvement from local data.
- [x] Fresh clean-context whole-follow-up-branch reviewer checks base/head,
  contracts and receipts. Root adjudicates and verifies any necessary fix.
- [x] Root commits, pushes to the fork and creates/attaches a follow-up PR against
  `release7-search` while #84 is open. The Test workflow targets main, so dispatch
  it manually for the follow-up branch and report pending CI as pending.
