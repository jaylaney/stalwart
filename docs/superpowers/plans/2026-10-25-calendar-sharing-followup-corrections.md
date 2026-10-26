# Calendar sharing follow-up corrections

> **For agentic workers:** Use `superpowers:subagent-driven-development`. The primary owns Git, review and final verification.

**Goal:** Address the seven new PR 58 comments at `ee7ebbe4e616fd09a117a801f22a59d60fe91f14` without weakening authorization, source conditions or ordinary-account behavior.

**Architecture:** Keep the existing calendar/event archives and vault schemas. Make key-account collection overwrite MOVE indivisible within a bounded physical batch, including a source-membership fence. Bind newly recorded calendar tombstones to calendar identity using an additive log, preserving the original path log for ordinary readers and refusing incremental foreign sync when legacy history cannot be authorized.

**Spec:** `docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md`, with the corrective rulings and acceptance requirements below. These refine the original MOVE prefix and identity-free tombstone provisions; they do not authorize new sharing modes.

## Global constraints and rulings

- Work only in `.worktrees/release3-mail-spec`, branch `codex/calendar-sharing`; preserve the unrelated untracked mail-design review.
- Implementers use Sol 6.1 or lower; the previously requested Astra xhigh seat remains independent and read-only.
- No stored struct layout changes, production test hooks, credentials in logs, dependency changes, deployment, merge, upstream operations or public review replies.
- One Cargo owner and one source writer at a time. Run integration suites sequentially.
- Preserve original source/calendar/event assertions and HTTP conditional semantics. A fresh source read must not authorize deletion of metadata changed after preflight.
- A 207 response, a final membership re-read alone, local locks or rollback are not solutions to the MOVE races.
- FoundationDB's inherited cached-read floor regression and independently selected SQL replicas cannot establish a scan at least as new as the sampled counter; Ephemeral does not roll back late failures. Refuse the new key collection overwrite-MOVE route on these stores before planning mutations; do not redesign generic backends in this correction.
- Retained path strings alone cannot authorize foreign tombstones. Current collision checks and token spelling changes do not establish historical identity.
- Standing user authorization is to fix worthwhile PR findings. Record compatibility changes and actual evidence; do not repeat approval for this correction pass.

## Review focus

- Repeating a failed or interrupted overwrite MOVE must preserve every source and destination event.
- A child created or linked after the source snapshot must not become pathless after calendar deletion.
- A private tombstone must remain private after colliding calendar names are deleted or renamed, including stale-path writers.
- Old log history must force an explicit resync rather than silently omit deletions.
- Fixture restoration must reverse quota/index deltas before cleanup can hide them.

### Task 1: Ordinary-path preparation and fixture/document hygiene

**Files:** `crates/dav/src/calendar/{delete,update}.rs`, `tests/src/za/sharing_review.rs`, `CLAUDE.md`, `docs/zero-access/calendar-sharing-api.md`, the previous correction plan and disposition.

**Requirements:** Comments 4372296693, 4372296697, 4372296701 and 4372296703. Read `.superpowers/pr58-followup-triage-2026-10-25/hygiene-triage.md` for verified source references.

- [x] Gate DELETE mutation-context resolution and retry archive cloning, and create write-context resolution, on existing `za_keys`. Keep current error/authorization order and key physical-write fences.
- [x] Add a quota-before/after assertion to the corrupt-calendar fixture; observe the failure before fixing restoration. Restore through indexed `Calendar::update` using a freshly fetched broken archive, not raw ARCHIVE replacement.
- [x] Update Release 4 suite order and `sharing-review-*` account allocation in CLAUDE.
- [x] Correct only newly introduced human-readable missing spaces; keep hashes, identifiers, URLs and historical evidence intact. Give COPY/MOVE conflict guidance an appropriate location, to be revised again by Task 2 if the contract changes.
- [x] Run the whole ZA suite for the quota RED check while holding exclusive Cargo ownership. Report the command, failure and source state. Final GREEN and WebDAV checks belong to final verification after all tasks.

### Task 2: Atomic bounded key collection overwrite MOVE

**Files:** `crates/dav/src/calendar/copy_move.rs`, a dedicated ZA regression module and registration, necessary owner/id-only test hooks, spec/API MOVE guidance. A small in-memory size-accounting helper in `crates/store/src/write/{batch,mod}.rs` is permitted if needed to inspect function payloads without exposing fields. Do not edit Task 1's fixture while its writer is active.

**Requirements:** Comments 4372296680 and 4372296686. Inspect `.superpowers/pr58-followup-triage-2026-10-25/move-triage.md`; implement its bounded atomic option, subject to primary review of concrete accounting and store fences.

- [x] Add deterministic regressions for unconditional identical retry after concurrent source metadata change; source child creation after membership scan; a held creator released after deletion; target/source metadata conditions; event linked in both source and target; and oversized refusal before mutations.
- [x] Prepare target overwrite deletions, target replacement, source child name changes and original source deletion in one builder, coalescing event documents present in both collections. Preserve non-source/non-target links and original event versions. Scheduling remains off.
- [x] Fence membership using the account change counter sampled before the direct source membership scan and a transactional predicate in the final physical batch. Account-counter changes may conservatively return 409. Retain all original source/target/other-parent context assertions.
- [x] Apply the independently confirmed FoundationDB, SQLReadReplica and Ephemeral exceptions, returning 409 before the direct scan or any mutations, after existing source/target shared-domain refusals. Existing `Store::is_foundationdb()` and `is_ephemeral()` may be combined with a small Store-owned `is_read_replica()` helper so feature unification remains correct. Retain all other routes. Record source-only exception proofs and unavailable backend runtime qualification.
- [x] Commit exactly one physical slice. Refuse oversized operations with 409 before any mutation. Enforce at most 1,000 structural operations and at most 5,000,000 accounted bytes, including archive payloads hidden in function operations; `BatchBuilder::len()` alone is insufficient. Document the accounting actually proved.
- [x] Keep COPY, event MOVE, ordinary accounts and absent-target calendar rename on their existing routes. Update the previous committed-prefix regression to assert atomic rollback and safe retry rather than expecting a moved prefix.
- [x] Run meaningful RED/GREEN regressions through the whole ZA entrypoint when granted Cargo ownership; record exact commands and limitations. No SQL/FDB runtime claim from RocksDB.

### Task 3: Identity-bound tombstone authorization

**Files:** calendar vanished writers in groupware/DAV/JMAP; additive vanished-log identity type/collection; calendar log purge; `crates/dav/src/common/propfind.rs`; a dedicated ZA regression module and registration; spec/API sync guidance.

**Requirements:** Comment 4372296690. Preserve the existing Calendar path-log layout and ordinary sync behavior. Additive identity records must be committed atomically with their original path records; no archive/ring migration or new token grammar.

- [x] Source-check the additive log design before editing. Enumerate every calendar vanished writer and parent-ID provenance, collection-number allocation and purge behavior. If a complete bounded design cannot be established, return the obstacle to the primary before implementation.
- [x] Record calendar ID plus path in a separate vanished-log collection at every current calendar tombstone producer. Reuse existing serialization infrastructure without changing stored struct layouts. Purge the sidecar with Calendar history.
- [x] Publish Calendar's truncation marker before deleting any Calendar sync/raw/identity history, so a reader cannot accept matched-empty streams in the deletion-before-marker gap. Add owner-scoped test-mode `CALENDAR_HISTORY_PURGE_PAUSE` / `set_calendar_history_purge_pause` after both vanished ranges are deleted, and prove an overlapping foreign sync returns `valid-sync-token`. Keep other collections' purge ordering unchanged.
- [x] Reserve additive vanished collection byte 250 after a complete numeric collision audit. Preserve original collection bytes. Match coverage by `(change_id, path)` using a keyed log reader and conservative bounded-range or mismatch rejection; path-only matching is insufficient.
- [x] Foreign key incremental sync emits only identity records matching fresh eligible calendar IDs and authorized path prefixes. Any requested raw-path history lacking matching identity coverage returns `valid-sync-token`, requiring full resync. Handle concurrent log reads conservatively; do not silently ignore legacy deletions or infer identity from paths.
- [x] Prove both collision winner orders, three-way/current and removed/renamed private collisions, stale-path delete after rename, valid percent-escaped granted names, legacy untyped history, pagination and ordinary-owner controls. Current live-item authorization remains by calendar ID.
- [x] Run RED/GREEN regressions through the whole ZA entrypoint when granted Cargo ownership. Describe additive-log compatibility and any conservative resync behavior.

### Task 4: Independent review, verification and integration

**Files:** this plan, a new follow-up disposition, PR description and evidence ledger. The primary owns this task.

- [x] Inspect actual diffs and source-backed reports. Obtain fresh independent Astra xhigh review of the branch against the accepted contract, plus bounded fix reviews when changes require them.
- [x] Address the whole-branch review's CK-vector growth finding without changing the archived representation: wipe retired key storage while owned before release; verify the decoded full-capacity growth path and key preservation. Use one bounded fix wave and scoped Astra re-review.
- [x] Run formatting; core unit tests; HTTP test-mode tests if hooks affect that crate; full ZA and WebDAV in plain/key modes sequentially. Build the shipping RocksDB product without test mode, then run the pinned client matrix against that exact binary. Record source identity and distinct evidence limits.
- [x] Commit only intended files, push the existing draft branch, verify the PR head/draft state and report checks that are still pending. Do not post replies or resolve review threads.

Qualification results and exact source/artifact identities are in the [follow-up disposition](2026-10-25-calendar-sharing-followup-disposition.md). The PR remains a draft; local acceptance does not establish current-head CI, optional-backend runtime or native-client acceptance.
