# Search CI Curve Profile Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Track steps with checkboxes.

**Goal:** Test the smallest curve-cost correction and integrate an evidenced fix with PR85.
**Architecture:** One test-package optimization override; keep all product behavior and test workload intact. Promote only against hosted evidence, then qualify the combined branch.
**Tech Stack:** Rust, Cargo package profiles, existing request-local diagnostic and GitHub Actions.
**Spec:** `docs/superpowers/specs/2026-11-13-search-ci-curve-profile-design.md`

## Global Constraints

- Only curve25519-dalek test opt-level becomes 3; first-party opt-level 0 and existing debug assertions/overflow checks remain.
- Preserve dependency versions/features, production profiles, 400 IDs/seven properties/5-second deadline, 3,258 corpus and all assertions.
- No new auth/cache/crypto mechanism, stored layout, test selector, retry or CI timeout change.
- Root owns Git/execution and serializes suites; agents inherit the parent route because its capability ceiling is unresolved.
- PR84 must not merge into main before exact combined-source CI is green.

## Review Focus

- Package-profile scope must not optimize first-party tests or alter release settings.
- Optimized dependency must retain debug assertions and overflow checks; inspect actual compiler flags.
- The observation must still be claimed and record 400 complete row opens.
- PR85 conflict resolution must preserve setup barriers, observers and every response assertion.
- Local timings must not be labeled hosted attribution or product performance.

### Task 1: Implement and measure the one-variable candidate

**Files:** Modify only `Cargo.toml`; root owns this plan, spec and outcome evidence.
**Interfaces:** Consume existing `[profile.test]` and Argon2 override; produce `[profile.test.package.curve25519-dalek]` with only `opt-level = 3` and a concise rationale comment.

- [x] Implement the scoped profile override in a subagent; no code, feature, lockfile or other profile edits.
- [x] Independent reviewer checks the exact configuration diff and preservation contract.
- [x] Root checks formatting/diff, captures compile flags/source/binary identity, runs vault units and the whole canonical ZA suite serially.
- [x] Compare the first-Get fixed-stage output with the retained default-profile control; label the local observation and its limits.
- [x] Obtain hosted evidence and either promote the candidate with the supporting measurements or reject it without claiming a fix.

### Task 2: Integrate and qualify the combined branch

**Files:** Combined PR84/PR85 tree; any merge resolution limited to actual conflicting fixture edits.
**Interfaces:** Consume reviewed evidenced fix and PR85 head cc5de6a55; produce release7-search with both contracts and exact-source validation.

- [ ] Commit the evidenced correction; preserve the original diagnostic run and failed CI evidence.
- [ ] Merge PR85 into release7-search using its expected head, then integrate the fix/diagnostic without losing either side's fixture behavior.
- [ ] A separate agent reviews the complete combined follow-up diff; root verifies any fixes and relevant local tests.
- [ ] Push the final combined source head and inspect all required CI jobs, including default/filesystem ZA, retention and fresh-store.
- [ ] Merge PR84 into main only after combined CI is green; otherwise keep it open and report the precise outstanding gate.
