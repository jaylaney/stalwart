# PR 84 search review fixes implementation plan

> **For agentic workers:** Use superpowers:subagent-driven-development. The user authorized implementation, spec updates, commits, push, and affected CI reruns on 2026-11-12. Keep PR 84 draft.

**Goal:** Address the eight PR comments and the Automation retry-observation failure.

**Architecture:** Preserve validated index generations and existing persistent layouts. Transient failures remain pending; owned response projections release shared index guards before client output. Trusted same-request creation references retain document identity with account and provenance checks.

**Tech Stack:** Rust 2024, Tokio, existing Cargo workspace and GitHub Actions.

**Spec:** ../specs/2026-11-04-zero-access-search-design.md, amended by this authorized fix wave.

## Global constraints

- Worktree: `/Users/jay/Development/stalwart/.worktrees/release7-search`, branch `release7-search`; starting head `239d2ec23141c47b03aee9572d712c46b4bb2ea5`.
- Preserve stored layouts, account isolation, keyed validation, ordinary account behavior except documented destroy inventory scans and the composite SAVE deadlock correction, and atomic `za_commit` mutations.
- No merge, release or deployment. Push only to `jaylaney/stalwart`.
- The primary owns Git operations and serialized build/test execution. Writers own disjoint paths; no nested delegation.
- Prior runtime evidence is historical, not validation of these new changes. Keep original failures and retries separately.

## Review focus

- A brief or prolonged transport outage must never silently consume a message; recovery must index it without a rebuild.
- A stalled FETCH client must not pin the account index read guard while a publisher waits.
- Metadata-only SEARCH/SORT must work without a usable map; identity and content operations retain their refusals.
- Server-created references survive a mapping change, but client-supplied createdIds and stale literal IDs gain no privilege.
- Optimized passes preserve partial-record checkpoints, pending cursors, truncation recovery, accounting and alias compaction.

## Task 1: Retry contract and pass copying (comments 1 and 6)

Own email search pass/write/rebuild code, search configuration and boot logging, and `tests/src/za/mail/index.rs` retry tests. Every transient failure aborts and checkpoints its diagnostic count without consuming the item. Definitive loss still skips. Retire the skip threshold from active configuration and document the old environment variable as ignored. Keep the serialized failing record and saturating counter compatible. Eliminate the redundant planner copy and avoid copying the full thread map on flag-only passes, preserving all checkpoint and cursor semantics. Add focused recovery and non-content pass coverage; report exact commands to the primary for serialized execution.

## Task 2: IMAP view lifetimes and metadata routing (comments 3 and 4)

Own IMAP FETCH, SEARCH guard routing and IMAP-focused ZA protocol regressions. Project the selected IDs using the validated cache and map, drop the view before any network write, and retain owned response data. Return no map guard when neither grouping nor identity filtering requires it. Cover paused output with an independent publication/read, and metadata SEARCH/SORT against an unavailable map with identity refusals retained.

## Task 3: Trusted JMAP references (comment 5)

Own JMAP request dispatch, submission and Email/set reference handling and focused JMAP ZA regressions; start after Task 2 releases shared protocol tests. Track server-produced creation references by account/document and original occurrence within the request, distinct from incoming createdIds, result-reference values and equal numeric literals. Cover Email/get and other supported Email-ID creation-reference consumers, not only submission. Resolve trusted references inside the handler’s actual validated view (after any creates/pass), never in a dispatcher preflight view; explicitly preserve generated onSuccess actions. Do not globally relax check_id or pin an index guard across methods. Test mapping changes between create/submit and generated follow-up, plus stale literal, incoming createdIds, wrong-account and missing-document refusal.

## Task 4: Cache eviction and shipping helper cleanup (comments 2 and 8)

Own common search cache/load/seal helpers, scheduler sweep integration and their unit tests. Run search idle eviction from the existing periodic sweep, remove per-entry global scans, and retain active-use safeguards. Move unused APIs behind compatible test/test_mode gates or migrate their callers to the production entry points; do not remove meaningful shared-component coverage. Test idle eviction without another keyed request and compile shipping and integration configurations.

## Task 5: Automation observation failure

Own `tests/src/automation/dkim.rs`. The existing CI failure is the red case: the fixture deliberately refuses DNS connections, and final diagnostics show Stalled(attempt=17) while the helper recognizes only Retry. Accept both retry-bearing states while retaining domain and minimum-attempt checks. Do not alter task scheduling or weaken failure-reason assertions. Run the full Automation suite in CI (locally only if Docker is available).

## Task 6: Documentation, integration and verification

The primary owns spec, CLAUDE.md, developer docs and this outcome/plan. Record the superseded retry rule, trusted-reference exception, guard lifetime and periodic eviction contracts, and the accepted ordinary-account full-link-scan cost. Review each task independently, then obtain a fresh whole-branch review against main. Run focused unit tests, the full canonical ZA suite, affected protocol suites and shipping build as appropriate, serially. Commit and push the reviewed fixes; the push must run affected jobs on the new SHA. Retry failed/cancelled affected jobs once only when the diagnosed condition permits it, preserving first-run evidence. Verify PR remains draft and report live CI results and unresolved limits.

## Execution status

- [x] Task 1 — implemented and independently reviewed.
- [x] Task 2 — implemented and independently reviewed.
- [x] Task 3 — implemented and independently reviewed.
- [x] Task 4 — implemented and independently reviewed.
- [x] Task 5 — implemented and independently reviewed.
- [ ] Task 6 — source reviews, focused checks, shipping check, full ZA and ordinary JMAP/IMAP suites passed. Commit/push and new-head CI remain the publication step at this checkpoint; the PR records live status.

Task 5's complete Docker-backed runtime check remains assigned to new-head CI.
See [the outcome](2026-11-12-zero-access-search-pr84-fixes-outcome.md) for evidence and limits.
