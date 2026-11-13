# PR84 Email Get Timing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Track steps with checkboxes.

**Goal:** Identify the server stage responsible for the hosted first keyed 400-message Get timeout.

**Architecture:** Extend the existing request-local test-mode observation to the
armed singleton Get, bracket its major stages, and report complete or partial
aggregates from the fixture without altering the wire attempt or its deadline.

**Tech Stack:** Rust, Tokio, existing SearchRequestObservation and JMAP fixture.

**Spec:** `docs/superpowers/specs/2026-11-13-pr84-email-get-timing-design.md`

## Global Constraints

- Preserve 3,258 inputs, first Get's 400 IDs/seven properties, all assertions and the five-second client deadline.
- No production behavior, crypto, cache, lifecycle, scheduling, event enum or stored-layout changes.
- Scope server instrumentation to test_mode and the exact armed account/call; preserve Query consumers and unarmed behavior.
- Output only fixed labels, counts and durations; no content, identifiers, credentials or raw errors.
- Root owns Git, builds, tests and publication; no builds or runtime probes while PR85's timed filesystem run is active.
- Inherit parent model/effort because the originating capability ceiling is not exposed; no nested agents.

## Review Focus

- A client timeout may leave server spans active; do not equate unfinished duration with zero cost.
- An unarmed/wrong-account/wrong-call request must not consume or inherit the measurement.
- The first Get must use the same generic client and preserve its five-second failure behavior.
- Nested spans are inclusive and request cancellation must not leak a pending claim or reporter task.
- Shipping builds must have no dependency on test hooks or diagnostic output.

### Task 1: Add scoped Get stage observations

**Files:**
- Modify `crates/common/src/za/test_hooks.rs` for extra fixed stages, partial/completed accounting and focused controls.
- Modify `crates/jmap/src/api/request.rs` only to admit the exact armed singleton Get alongside Query.
- Modify `crates/jmap/src/email/get.rs` only for cfg(test_mode) snapshot/batch/row-open spans.
- Modify `tests/src/za/mail/differential.rs` to arm/report the first keyed inventory Get while retaining its body and existing client helper.
- `tests/src/utils/jmap.rs` is optional only if a minimal reusable call-ID entry point avoids client duplication; existing callers and client construction must remain identical.

**Interfaces:**
- Consume SearchRequestObservation, claim_search_observation, SearchStage,
  search_span/observe_search_wait, and Account::jmap_method_calls.
- Preserve existing timings()/count() consumers. Add explicit completed/partial
  diagnostics without breaking existing cold-serving or retry observations.
- Produce one bounded diagnostic block for the selected Get on return or unwind;
  stage names must convey inclusive totals and active/partial work.

- [x] Read the spec, relevant source and supplied evidence reports.
- [x] Add deterministic observation controls for claim isolation and complete/pending/drop accounting, without executing them during PR85 timing.
- [x] Implement the smallest shared observer extension and fixed-stage instrumentation; keep guards out of shipping code.
- [x] Arm only the first keyed Get using its exact 400 IDs/properties and existing generic request helper. Preserve every response assertion and the rest of the matrix.
- [x] Ensure a failed send prints whatever partial server evidence is available without swallowing/changing the original failure.
- [x] Return source refs, precise diff summary, validation commands and limitations. No worker build/test/Git mutation or nested agent.

### Task 2: Review, validate and run the diagnostic

**Files:** Root-owned outcome note and ignored evidence ledger.

**Interfaces:** Consume immutable Task1 diff and test commands; produce reviewed code, local receipts and hosted stage evidence.

- [x] Independent review checks all five failure modes and unchanged behavior/coverage.
- [ ] After PR85 timing ends, root formats, compiles affected and shipping configurations, runs observation controls and the whole canonical ZA suite sequentially.
- [ ] Freeze binary/source identity, commit/push the isolated diagnostic branch, and dispatch its Test workflow; do not silently fold it into PR85 or update PR84's base.
- [ ] Read actual hosted aggregate evidence and distinguish measured stage attribution from hypotheses. Decide the next fix from that evidence.
