# Calendar Sharing Client Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add five required release4 sharing flows to the independent production-binary CalDAV suite.

**Architecture:** Preserve the pinned upstream and existing strict flows. Add a focused key-only
sharing module and mode-specific collection manifests; use public APIs for fixture grants and
real client requests/readbacks for the behavior under test.

**Tech Stack:** Python3.12, pinned python-caldav v3.3.1/111ae14b875f40d100d5259820f10210f00e20f9,
pytest, existing normal-mode RocksDB product fixture.

**Spec:** [approved coverage design](../specs/2026-10-25-calendar-sharing-client-coverage-design.md),
[sharing API](../../zero-access/calendar-sharing-api.md), and [revision14 of the sharing design](../specs/2026-10-22-zero-access-sharing-design.md).

## Global Constraints

- No Rust/product behavior, stored-layout, dependency-pin, compatibility-exception or native-client changes.
- Existing72 cases remain mandatory in each mode; key mode additionally requires exactly five sharing flows.
- Strict flows cannot have compatibility exceptions; missing/extra/filtered cases or reports fail.
- No real accounts/.run/existing server; no generated secrets or raw fixture stores in exported logs.
- Preserve redaction, process cleanup, pinned git-archive materialization and ordinary-account baseline.
- Poll at most15seconds, below the30-second drain retry deferral; no arbitrary timing to mask failures.
- Primary owns all Git and PR operations; implementer does not commit, stage, push or delegate.

## Review Focus

1. Same UID in source and invitation copy: explicit owned-calendar URLs and distinct resource identities.
2. Missing key-only flow or malformed mode manifest: collection must fail before execution, with no skip waiver.
3. Credential/error handling: failed public API calls must not echo credentials/request/response bodies.
4. Revoked/renamed access and sync: normal client refresh must reveal only currently eligible collections/data.
5. Scheduling: assert ORGANIZER/SENT-BY/PARTSTAT/cancelled state on actual recipient copies within15seconds.

### Task 1: Mandatory production-client sharing flows

**Files:**
- Modify `tests/caldav_client/collection.json`: explicit plain/key expected-case lists.
- Modify `tests/caldav_client/runner.py`: collection loading and key-only file selection.
- Modify `tests/caldav_client/policy_plugin.py`: mode-aware enforcement and exact new filename recognition.
- Modify `tests/caldav_client/test_runner.py`: failure-boundary and collection tests.
- Create `tests/caldav_client/test_sharing.py`: five flows and focused public-API/calendar helpers.
- Modify `docs/zero-access/client-caldav.md`: actual sharing coverage/counts/evidence boundaries.
- Modify `.github/workflows/test.yml`: descriptive suite-step name only, if needed.
- Record actual results in `docs/superpowers/plans/2026-10-25-calendar-sharing-client-coverage-outcome.md`.

**Interfaces:**
- Add `read_collection(mode: str, path: Path | None = None) -> set[str]` in runner.py.
  Default path is HERE/collection.json; plugin passes its own HERE path explicitly, including test fixtures.
  Validate exactly plain/key maps, nonempty lists of unique nonempty strings and a known requested mode.
  Convert invalid configuration into safe operational/pytest usage failure before test execution.
- run_suite retains the existing upstream classes/test_flows.py in both modes and adds test_sharing.py
  only in key mode. policy_plugin recognizes precisely the two strict-flow filenames, prefix flows/.
- test_sharing imports the existing people/event-data helpers without importing other test functions.
  Public API helpers derive the local base URL from fixture users, authenticate as that user, and
  return only needed decoded response fields/statuses. No admin credentials or private store access.
- New case IDs are flows/ followed by these exact names:
  test_shared_calendar_view_and_edit
  test_shared_calendar_properties_and_etags
  test_shared_calendar_revoke_regrant_rename_and_sync
  test_shared_calendar_delegated_invitation_update_cancel
  test_shared_calendar_own_and_shared_replies_and_sole_decline

- [ ] **1. Add harness-unit RED coverage.** Test per-mode72/77 manifests, key-only selection, missing/extra
  sharing cases, unknown/missing modes, duplicates/invalid shapes, forbidden strict exceptions/filtering,
  and safe public-API error handling. Preserve all existing harness failure-boundary assertions.
  Observe a meaningful failure before adding the new loader/selection behavior.
- [ ] **2. Implement the mode-aware loader/collector.** Keep the old72 IDs unchanged in each mode;
  add five key IDs. Fail cleanly before execution on incomplete/malformed collection or reports.
  Preserve both-mode runtime-failure cleanup and the existing compatibility policy verbatim.
- [ ] **3. Implement the five actual client flows.** Follow the approved design's assertions. Pin through
  contacts/lookup then contacts/pin, grant/revoke through the public sharing API, and use each actor's
  own CalDAV client for access, edits, replies and readbacks. Assert the exact requested property/status
  and recipient component fields. Use owned-calendar polling for Bob/Carol; principal-wide UID search
  can incorrectly return the shared source. Keep cleanup and15-second bounded polling explicit.
  A new client flow may pass initially because the product already implements the behavior;
  do not manufacture backend mutations or relabel it as I1's precise race reproduction.
- [ ] **4. Run all harness tests.** Python3.12:
  `python -m unittest discover -s tests/caldav_client -p test_runner.py -v`.
  Run against the pinned dependency environment; preserve failures and command output.
- [ ] **5. Run the complete pinned production-client matrix.** Use the existing verified release binary
  if its current Rust source/binary provenance matches the prior final release; rebuild only if needed.
  `python tests/caldav_client/runner.py --binary target/release/stalwart --caldav-source PINNED_SOURCE
  --log-dir NEW_PRIVATE_ABSOLUTE_LOG_DIRECTORY` (both modes, no filters). Require72 plain/77 key
  cases, all five new flows passed and only existing precisely recorded compatibility outcomes.
  No Rust suite,57-mutation,3,000-event benchmark or native run is required for Python-only changes.
- [ ] **6. Record evidence and docs.** Retain exact interpreter/client pin/binary identity, observed
  counts and outcomes, first failures plus diagnoses/reruns, sanitized logs and cleanup results.
  Keep native acceptance pending. A production mismatch is reported to the primary with concrete
  request/state evidence; do not edit Rust, loosen assertions or expand exception/polling policy.
- [ ] **7. Independent review and integration.** Primary reviews actual diff/evidence, obtains one
  strongest-permitted independent review of this complete bounded coverage change, resolves confirmed
  issues with scoped fix/review, then commits/pushes the coverage and updates the existing draft PR.

## Execution and boundaries

The user approved the existing-harness approach and these five flow families with “let's open a
draft PR and add that coverage.” Execute via the repository's subagent policy; no redundant
permission gate is needed. Per-mode manifest and focused-module placement are primary choices;
cost if wrong is localized harness rework, not product-format migration. Native qualification,
Circulo changes, deployment and merge remain separate. Do not modify the unrelated untracked
mail-design review. Do not change the completed release4 implementation unless the primary
adjudicates a source-backed product finding from this new coverage.

The user additionally requested an Astra xhigh review of draft PR58. Run that read-only on the
pinned current PR head while coverage is implemented, then use its scoped review of the coverage
delta as the independent gate for this task. This replaces the earlier planned Sol review seat;
implementation routing remains Sol6.1 and lower.
