# PR 84 retry follow-up implementation plan

> **For agentic workers:** Use superpowers:subagent-driven-development. The user authorized review fixes, spec updates, commits, push, CI and thread replies. PR 84 is now ready for review at the user's request.

**Goal:** Address the four follow-up review comments at `5df77aff0411c271b30a7865ea5a56ef672bab4e` without weakening stale-ID validation or transient-failure safety.

**Architecture:** Preserve atomic aborts and persistent formats. Pace actual failed message opens using a monotonic resident-entry cooldown, and avoid identical abort commits. Keep per-method created IDs/state paired; document and test their existing lifetime.

**Tech Stack:** Rust 2024, Tokio, existing Cargo workspace and GitHub Actions.

**Spec:** ../specs/2026-11-04-zero-access-search-design.md, revision 35.

## Global constraints

- Worktree `/Users/jay/Development/stalwart/.worktrees/release7-search`, branch `release7-search`, starting head `5df77aff0411c271b30a7865ea5a56ef672bab4e`.
- No stored-layout, padding, alias, chronological checkpoint, admission-reservation, credential or ordinary-account behavior change. No raw error/value logging. Keep unknown failures and missing codec keys transient.
- Primary owns Git, docs, serialized builds/server tests and external publication. No nested delegation. Keep ignored evidence. Push only `jaylaney/stalwart`; no merge or deployment.
- Independent reviewers use supported parent-model inheritance because the originating capability ceiling is unresolved. No stronger model review is claimed.

## Approaches and rulings

Retain atomic abort with bounded cooldown instead of publishing its healthy prefix: prefix publication changes cursor/checkpoint and publication contracts, while pacing suffices to bound repetition. Cost: healthy prefix and later work still wait for repair. Use a per-entry local probe streak, not a persisted timestamp/lease or the diagnostic counter: in-window absence preserves the latter at zero. Cost: per-node bounds and one fresh probe after restart/eviction. Preserve per-method returned IDs instead of rewriting earlier responses with later identities: their states must remain paired. Cost: stale IDs may need changes/query resynchronization.

## Review focus

- Own or remote diagnostic writes must not invalidate cooldown and recreate a retry storm.
- Reset, replacement, remote progress and assertion losers must not apply an old cooldown to a new generation.
- In-window absence and saturated counters must not issue identical writes; real suspension/marker changes must still persist.
- Content waits must retain the original deadline, while metadata hooks promptly defer after validation.
- Corrupt archive loss classification must be narrow; traces must identify the document without exposing stored bytes or content.

### Task 1: Bound retry work and identify permanent archive loss

**Files:** common search cache (and scoped test-only hooks if necessary); email search `pass.rs`, `ensure.rs`, `open.rs`, `write.rs`; `tests/src/za/mail/index.rs`, its focused-runner invocation in `threads/pr84.rs`, and existing trace helper only if required. Exclude identity/IMAP guard test files, docs and generated enums.

**Interfaces:** Add a deferred pass outcome carrying the fixed transient class and monotonic retry deadline, or an equivalent explicit internal contract. Entry state owns the local consecutive-probe streak and identity; it is not serialized. Preserve existing actual-abort result semantics where possible.

- [x] Write failing coverage for retry suppression, unchanged abort writes and the real malformed archive path. Primary runs the red checks before production edits.
- [x] Implement cooldown 1, 2, 4, 8, 16, 30 seconds, capped at 30, incremented per actual transient probe including in-window absence. Check under the pass lock after authoritative manifest validation and before expensive replay/copy/open. Deferred calls do not open, commit or repeat abort traces. Ensure waits against the original request deadline; hooks return promptly.
- [x] Preserve cooldown across validated diagnostic-count-only changes for the same pending document, including local post-abort commits and remote diagnostic writers. Reset for meaningful progress/lifecycle/entry changes. Failed writes retain the unchanged observed basis; assertion losers cannot arm a stale generation.
- [x] Elide an abort write only when the complete next manifest equals the base, before blob staging or batch creation. Keep changed suspension/marker data durable.
- [x] Add DocumentId and Attempt to actual message-open abort traces. Leave non-document loader/compactor trace shapes intact. Preserve fixed classes and no raw error payloads.
- [x] Classify only Store DeserializeError with fixed Details `Archive access failed` as parse loss at the opener boundary. Preserve generic DeserializeError, non-Store and CodecKey transient controls.
- [x] Test cooldown sequence/cap and time boundaries without real backoff sleeps; concurrent callers; original Content deadline; saturated/absent no-op commits; real metadata changes; own/remote count-only versus progress changes; failed write/assertion loser/replacement behavior; fixed safe traces; malformed unversioned outer metadata consumed once as loss. Adapt existing consecutive-probe tests with an explicit test-only deadline seam, never a production bypass.
- [x] Return source diff summary and exact test commands. Primary performs the green checks and independent focused review before accepting this task.

### Task 2: Make the returned-ID contract explicit in coverage

**Files:** `tests/src/za/mail/threads/trusted_references.rs`, `tests/src/za/mail/threads/imap_guards.rs`, and a comment only in `crates/imap/src/op/fetch/mod.rs`. No JMAP production change.

- [x] Add a create/import case leaving the document alive after a later method changes its mapping. Assert method-created ID and top-level createdIds retain the earlier value, trusted reference returns the current value, and a fresh request rejects the old literal/client-createdIds while accepting the current ID. Clean up through the current ID. Use existing raw JMAP helper.
- [x] Add nested identity filtering under an unavailable Map to the existing metadata guard fixture. Existing keyless nested identity coverage remains sufficient.
- [x] State the FETCH invariant: selection/filtering/truncation finishes before projection; output borrows that unchanged vector. No fallback or new identity behavior.
- [x] Keep the focused runner and real periodic-eviction timeout: canonical ZA invokes its fixtures, the inventory gate tracks baseline tests, and actual scheduler behavior needs runtime evidence.
- [x] Return a concise report for independent focused review; primary runs the focused/full suites with Task 1's final source.

### Task 3: Verification and publication

- [x] Primary updates spec revision 35, developer docs and a self-contained outcome note, with precise per-node cooldown and returned-ID residuals.
- [x] Obtain independent task reviews and a fresh whole-branch review against `92a41596a571e164d3e88b3fa5f0c9eb6d5e0660`, without feeding it prior finding conclusions.
- [x] Run common/email search units, focused PR84 integration and the full canonical ZA suite sequentially. Check shipping `--no-default-features --features rocks`, formatting and diff hygiene. Preserve failures and retries separately; no blanket repeat of historical performance/mutations.
- [ ] Commit/push fixes, reply to each new inline thread with exact correction/evidence, and verify CI on the resulting head. Keep PR ready for review; report pending CI honestly.

Publication is tracked on PR 84 after this reviewed local checkpoint; the resulting commit and CI URL belong to the PR record.
