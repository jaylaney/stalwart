# Zero-access plan 16: search and threads for key accounts (release 7) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Plan revision 21** (2026-11-08) pairs keyed queryChanges
with one validated view and the page's effective endpoint (spec revision 28).
Tasks 10 and 11 stage the borrowed query path and the existing bounded
cache floor. Task 13 sets the floor from the supplied token and composes
the page, endpoint projection and query with that view. The query uses
upstream current-row values and projects identities and membership at E.

**Plan revision 20** (2026-11-08) incorporates the bounded Content-view
corrections (spec revision 27). Task 10 refreshes a cache below the applied
indexing checkpoint after a deletion-only pass (P2-CONTENT-FLOOR), checks the
cap under its final guard, and preserves the original deadline across its
read/load waits. Map/FR7 and owned-pass transaction completion are unchanged.
Real deletion and lock fixtures plus a final-cap fixture distinguish each
check. These use the existing acquisition loop, with no storage or wire change.

**Plan revision 19** (2026-11-08) closes P2-DUPLICATE (spec revision 26).
Task 16a does not create a marker when both fields 150 and 152 are absent;
it still scans exact orphan links with both absence assertions in every
nonempty chunk. If only field 152 is absent, marker creation asserts the raw
vault hash too. Existing marker/reset guards stay. Real-store controls and
M64–M67 distinguish the no-marker branch and the three added assertions.
This is scoped search cleanup, not a new account-lifetime mechanism.

**Plan revision 18** (2026-11-08) closes the lifecycle execution findings
(spec revision 25). Every Task 16 reset mutation batch asserts the live Account
registry Item, including marker creation and final clear; each unlink chunk
asserts its captured marker, and an empty
resumed marker scans. The existing destroy primitive and its Principal-link
scanner run first as Task 16a, after Task 8 and before Task 9, so broad request
hooks need no temporary fixture unlink. Sol high implements that kernel; Task
16b retains the operator/recovery integrations after Task 15. No API or wire
change is required. Real-store controls and mutations M60–M63 cover the fences.

**Plan revision 17** (2026-11-08) corrects execution finding P1-PAGING
(spec revision 24). Task 13 reconstructs the initial set at pinned end E by
restoring later deletions and subtracting later insertions, and maps it at E.
Fresh Initial/Exact(0) remains admissible. An initial continuation requires
E at or above the shorter-log retention bound and the validated map's epoch;
a truncated suffix read or a bound that moves past E during that read refuses
it with cannotCalculateChanges. Arbitrary evidence loss cannot preserve a
count-based page. No new state shape or storage mechanism is added. Task 13's
fixtures distinguish retained and expired continuations; Tasks 14/15 add the
rebuild/folding forms when those mechanisms exist; Task 19 adds M56–M59.

**Plan revision 16** (2026-11-08) resolves execution finding P1-STATE: a mixed
`Email/set` cannot use its pre-update view for both input checks and the final
response after later mutations. Task 11 keeps an input view, drops it before a
final response acquisition, and renders created IDs and `newState` from that
final view, whose counter must cover the call's acknowledged writes. The floor
uses Task 10's existing validation loop, with no extra pass or live guard.
`oldState` and supplied-ID acknowledgement echoes retain their existing meaning.
The Task 11 regressions and Task 19 mutations M54–M55 cover both stale-map and
stale-own-write failures (spec revision 23).

**Plan revision 15** (2026-11-08) closes the execution review's P1-PAD finding:
a valid non-power-of-two `ZA_SEARCH_PAD_MIN` could make rounding the actual
publishing payload reveal its alias count. Task 5 now pads Publishing directly to
`M`, refusing a framed payload above `M`; only Compaction rounds its public-count
allowance. This preserves the approved fixed-size guarantee, accepted configuration
bounds and default wire behavior, with no format change (spec revision 22).
Task 5 adds the equal-record-width alias-count fixture and publishing-boundary
refusal, extends the sealed-blob check, and Task 19 adds mutation M53. The obsolete
grouped-record overshoot example is corrected to one message.

**Plan revision 14** (2026-11-08) records Jay's Task 1 benchmark ruling:
`ZA_SEARCH_ACCOUNT_MAX_MB = 512` MiB and `ZA_SEARCH_MEM_MB = 1536` MiB are the
provisional defaults (spec revision 21). The full 15,000-message benchmark measured
214.40 MiB of Term frames with positions, projecting to 428.79 MiB at 30,000
messages before record fields and postings. Positions and the Term dictionary stay.
Task 19 must confirm the limits against the completed model's loaded size; this
ruling answers Task 1's cap question and does not claim a loaded-memory measurement.
Every node in a cluster still uses the same account cap, and the one-third boot
refusal is unchanged. Task 2's defaults and boundary assertions below follow this
ruling. All other execution routing and mechanisms remain as reviewed.

**Plan revision 2** (2026-11-07) answers the Codex review of revision 1 (`2026-11-07-zero-access-16-search-codex-review.md`, findings PL1 to PL19, all accepted) and carries Jay's rulings on B1 and E1 (spec revision 19). Each change cites its finding where it is made:
- PL1, Task 6: every index blob is checked against the BLAKE3 hash its descriptor names before it is parsed (`fetch_verified`, `LossClass::Hash`), so a stripped hidden section is loss, not silence.
- PL2, Task 3: `current_thread` is the latest assignment, independent of revision rows, so a pass's later insert threads through what it staged earlier.
- PL3, Tasks 10 and 11: one validated acquisition, `za_view`, takes the cache, the read guard and the content, validates exactly what it returns, and holds no guard while a pass or reload runs; Task 11's snapshot is that view (`ViewNeed::Map`); Task 10 moves to `opus`.
- PL4, Tasks 6, 8 and 14: `scanned_to` never covers a partial checkpoint (`scanned_seed`); the shortcut refuses a partial checkpoint and a pending content cursor; a skip counts as progress.
- PL5, Task 14: the content-only rebuild's scan keeps the thread blobs the manifest names.
- PL6, Tasks 5 and 14: the reconciliation gets the spec's cursor over the difference (`gap_next`) and resumes retained records from `indexed_to` (plan call E4 revised).
- PL7, Tasks 3 and 15: compaction folds each record's whole sub-bound alias prefix (`fold_sub_bound`), so aliases one pass staged at one revision fold in chain order.
- PL8, Tasks 2, 8 and 14: the per-item alias bound is observed through test-mode instrumentation; no trace carries an alias count.
- PL9, Tasks 10 to 12: top-level `inThread`, `Page::count`, the owner's thread-keyword sorts and the attachment-keyword sorts use the map and the content.
- PL10, Tasks 7, 8 and 10: the content record stores upstream's effective sent time (received time plus the clamped offset).
- PL11, Task 6: memory stays charged until the last holder drops it (`BudgetCharge`; plan call B6 revised).
- PL12, Tasks 2, 5 and 8: `ZA_SEARCH_SEGMENTS` is refused outside `2..=255`, the manifest's one-byte count.
- PL13, Task 1: positions are kept; the size figure is reported, never acted on.
- PL14, Task 17: the `query_changes` variant's `Move` is a no-op in key mode; merge rendering is Task 13's (plan call F2 revised).
- PL15, Task 18: the G5 case subtracts each neighbour's own blob before comparing.
- PL16, Task 19: the benchmark waits on 7.4's currency predicate.
- PL17, Task 1: the attachment-free control disables the `Attachment` field.
- PL18, Global Constraints: the compile gate fails with a nonzero status.
- PL19, Task 1: the accessor calls upstream's `AccountIndexer::index_document`, and the record stop uses `R = 304`.

**Plan revision 3** (2026-11-07) answers the Fable review of revision 2 (`2026-11-07-zero-access-16-search-fable-review.md`, FR1 to FR14, all accepted; it found every PL closed):
- FR1, Tasks 8 and 14: a cursor's or a marker's completion is the pass's own `bookkeeping` decision (`CursorStep::{None, Advanced, Exhausted}`), written as a manifest-only pass even when nothing was staged or consumed, so a rebuild cursor whose remaining messages vanished, a finished content cursor and a finished reconciliation all leave the manifest on an idle account.
- FR2, Task 14: revision 3 narrowed the reconciliation's `inserted_in_log` to the records the pass would consume; revision 4 reverts that (FS3): the state FR2 described cannot arise, since a reconciliation starts only from a read that met the marker, whose record is empty, so every retained record with content is above `indexed_to`; the narrowing re-opened a retained insert skipped as loss.
- FR3, Tasks 11 and 13: a Thread method's state is the view's cache counter, never a store read after the cache (plan call D13, spec wording).
- FR4, Task 6: the PL11 test holds both generations and asserts the counter.
- FR5, Tasks 4 and 10: `ContentPart::document_ids(&self) -> &RoaringBitmap` in both.
- FR6, Tasks 7 and 8: `za_open_for_index` applies the H6 absence window itself, keyed by the blob hash only it knows.
- FR7, Tasks 3 and 11: `ThreadPart::members` (every record, present or tombstoned) feeds `thread_members`, intersected with the cache.
- FR8, Tasks 10 and 11: one view or snapshot per request task, taken after the post-commit pass hook in `Email/set` and `Email/import`, and the content view doubling as the snapshot in IMAP SEARCH.
- FR9, Task 15: the compacted public section carries the base manifest's newest row and excludes only the pass's own.
- FR10 to FR14: Task 6's stale refusal note removed; `Checkpoint` cited as Task 3's; the pass's step 0 guard scoped; the pass's thread-part clone charged; Task 1's commit no longer names `term.rs`.

**Plan revision 4** (2026-11-07) answers the scoped Fable re-review of revision 3 (the second section of the same review file, FS1 to FS10, all accepted; it found FR1 to FR14 closed, FR2 for a state that cannot arise):
- FS1, Task 14: the reconciliation reads every retained record while `indexed_to.change_id <= first_retained` (the marker may sit on `indexed_to`'s own record), and a second truncation is detected by a bound above the marker's, never by the `truncated` flag, so the reconciliation can no longer restart on every pass.
- FS2 and FS5, Tasks 8 and 14: each completion has its own step in `Staging` (`cursor`, `content_cursor`: `CursorStep`; `marker`: `MarkerStep`), `write_pass` applies three independent arms, the cursor block runs only with a pending cursor, and an `Abort` leaves every step untouched.
- FS3, Task 14: FR2's narrowing reverted (above).
- FS4, Task 14: a pass with a pending rebuild cursor probes the retained bound and folds the cursor into a reconciliation when the bound passed `indexed_to` (spec 5.4 as written); `case_f9_truncation_during_a_pending_rebuild` keeps one record so the marker lands above `indexed_to`.
- FS6, Tasks 9 and 14: `za_search_ensure_current` starts the content-only rebuild on a content blob's loss and re-enters once; `case_h23` sets `absent_retry_secs: 0`.
- FS7 to FS10: the marker clears against the pass's own checkpoint; D13 cites 6.2, 6.3 and the spec header; `State::from(Option<ChangeId>)`; the Thread stream emits `Updated(T)` only when the member sets differ (a flag-only record renders nothing, as upstream); the clone's charge sits in step 5; both thread tables are rebuilt at every fold and at compaction.

**Plan revision 5** (2026-11-07) carries the P2 and P3 findings of the scoped Fable re-review of revision 4 (the third section of the review file, FT1 to FT9), which **approved** revision 4 under the stopping rule (no P1 construction defect). These edits were applied after that approval and have not been re-reviewed:
- FT1, Task 13: the pinned-end unit test gets a membership change in its range, since FS8's rule renders nothing for a flag-only record.
- FT2, Tasks 8 and 14: `plan_pass` takes a `PassSources` struct (cursor, content, gap inserts, gap deletes, gap stamp) and gains `admit_content`, so Task 14's content cursor and gap items have a production path that stages no thread record.
- FT3, Tasks 9 and 14: one starter for a reconciliation, the pass; Task 9's `Truncated` arm only re-enters.
- FT4, Tasks 9 and 14: the pass's step 0 starts the `CapRaised` and `Behind` content-only rebuilds; `ensure_current` starts only the one a content loss causes.
- FT5, Task 14: a bound move that lost no unconsumed record rewrites the marker's bound instead of restarting (`SecondTruncation::{No, BoundMoved, Restart}`).
- FT6 to FT9: `za_retained_bound` and `second_truncation` in Task 14's interface lists, the attribution read uncapped; the `members` table in Task 3's storage sketch; the source order stated in Task 8; `case_f9` asserts its two keys above `indexed_to`. Task 7's `first_retained` sketch now sets it on the marker's key only, as its contract says.

**Plan revision 8** (2026-11-08) answers the scoped Fable re-review of revisions 5 to 7 (the fourth section of the Fable review file, FU1 to FU13: REQUEST CHANGES, one P1) and Codex's re-review of revision 7 (`4db7783de`, the "Current review" section of the Codex review file: APPROVE, two P2s in PL11):
- FU1, Tasks 8 and 14: the pass's shortcut also requires that no content-only rebuild is due, so a `CapRaised` or `Behind` rebuild starts on an idle account whose checkpoint sits at the cache's counter; `case_f6_r23` gains that variant.
- Codex PL11 and FU4, FU7, Tasks 6, 8 and 15: admission and charging are one atomic step under the search cache's admission lock (`admit`, `grow`, `admit_content_growth`; `charge` and `BudgetCharge::resize` are gone), the swaps and compactions call it on the right receiver outside the borrow, and the growth tests drive Task 6's helper, with a two-thread barrier test of the atomicity.
- FU2, FU5, FU6, Task 8: the gap deletes wait for the pass whose insert loop ran off its list (spec H3 across the cap); `s_empty` runs on empty records; over the cap the content block does not run and the pass suspends content indexing; two `plan_pass` tests drive the content and gap sources.
- FU3, FU13, Tasks 8 and 14: `Staging.marker_bound` carries a `BoundMoved` pass's bound to the manifest and the gap blocks are gated on a pending marker.
- FU8 to FU12: `case_d13` imports its bridge; the hook count, `clone_for_test`, `into_raw_bytes` and `encoded_len`'s purpose in the lists; the stale revision references; the cross-account test's second half on a fresh cache; the `CapRaised` rebuild's shape named as plan call E10, `<` made complementary, the start's assertion failure handled as `Lost`.

**Plan revision 9** (2026-11-08) answers the scoped Fable re-review of revision 8 (the fifth section of the Fable review file, FV1 to FV13: REQUEST CHANGES, two P1s, both in revision 8's own fixes):
- FV1, Task 14: `Behind` requires `!suspended` and `size < cap` (spec 5.4's "chain loadable"); without the guard, FU1's shortcut term started a chain-dropping rebuild on every account that had just crossed the cap. `case_f6_r23` asserts no start and no chain drop during the suspended phase.
- FV2, Task 8: the bound arm writes only to a marker that survives the pass; a bound move and a completion in one pass no longer panic.
- FV3, FV4, Task 8: content suspension is set on the transition only, so a suspended account with a pending cursor writes nothing per request; the `content_to` carry rule moves beside the arms as a rule of every decision but `Abort`.
- FV5 to FV13: Task 8 writes the shortcut without the `needs` term and Task 14 adds it; the two new tests' fixtures defined; the barrier test's `LoadedIndex`es live outside the map; the loader's refused shrink and the `Whole` reload's old charge; `admit_content_growth` on `None` and the over-budget trace's owner; an `Abort` keeps `marker_bound` and `suspend_content`, `Restart` returns `Recovery(Truncated)`, the `Lost` start traces; the counts and lists; `OverCap` refused at once while suspended (Task 9); E10 ordered after E9 and flagged for Jay beside the spec's backfill sentences.

**Plan revision 10** (2026-11-08) carries Jay's ruling on E10 (FV13): the raised-cap and lagging-`content_to` rebuilds keep the chain and backfill, as spec 5.4 and 9.2 describe; only a content loss drops it. `za_search_content_rebuild_start` acts by reason (Task 14); the content cursor's skip reads the loaded content part, which Task 8 step 1 loads before the content block on a node that can hold it under its cap, answering `Aborted(Budget)` otherwise; `content_cursor_items` lists every present document only when `content` is `None`; `case_f6_r23` asserts the chain is kept and only the suspended-period messages are re-opened. Not re-reviewed.

**Plan revision 11** (2026-11-08) answers the scoped Opus re-review of revisions 9 and 10 (the sixth section of the Fable review file, FW1 to FW14: REQUEST CHANGES, two P1s, both in revision 10's content load):
- FW1, FW2, FW7, Tasks 8, 9 and 14: the pass never loads the content part. Its content block runs only when the part is resident and current on the node (`PassSources.content_runnable`), otherwise it is skipped without aborting and the records, the cursors and the gap items run; a content request's `za_search_ensure_current` loads a pending backfill's part before its passes (spec 7.4: only a content request waits). Revision 10's load made every keyed request wait for a whole content-part load and stopped indexing on any node that could not load the chain.
- FW3, FW13, Tasks 6 and 14: `content_cursor_items` answers `Option` and is keyed on the manifest's content head, not on the resident part; a reload to a manifest with no head drops the stale part.
- FW4, Task 9: `OverCap` refuses when `size > cap`, or at the cap when suspended; `map_load` maps it.
- FW5, Task 8: `PassSources` carries the bound and the cache counter, and `plan_pass` applies rule 5 itself, so a bound-move-only or finish-only pass writes.
- FW6, FW8 to FW14: a skipped content block does not hold back the gap blocks; `case_f6_r23` runs with `segments: 255`, starts the backfill on a `Mailbox/get`, runs it on the first text search and counts the opens from the suspending pass; `apply_marker_arms` in Produces and its test in `write.rs`; the 2 s and chain-dropped assertions the case map cites; wording and the guard type.

**Plan revision 12** (2026-11-08; Jay confirmed FX5 the same day: a cluster's nodes share `ZA_SEARCH_ACCOUNT_MAX_MB`, an operator requirement Task 19 documents) carries the follow-ups of the two reviews that **approved** revision 11 under the stopping rule: Codex's (`6943e3698`, "Current review" in the Codex file: PL20 to PL22, P2) and the scoped Opus pass (seventh section of the Fable review file: FX1 to FX14, four P2 and ten P3, no P1):
- Codex PL20, Task 9: `is_current` also requires no pending content cursor (seven conditions), with a unit test; checkpoints can be equal while a backfill still has messages without content.
- Codex PL21 = FX4, Task 14: `case_f6_r23` keeps `segments: 255` through the backfill and states its message size; Codex PL22 and FX10, Task 8: `s3` asserts the `ManifestOnly` decision its two steps cause, and names its `gap_stamp`.
- FX1, Task 6: a reload to a manifest with no content head drops the resident part whatever the reload plan (the two-node case is an `Append`), with a load test.
- FX2, Tasks 9 and 14: after a content-loss start, the content wait reloads its manifest before re-entering (the start writes no log entry, so nothing else refreshes it), and an assertion failure there is `Lost`.
- FX3, Tasks 6 and 8: the content load installs its part only when the manifest's head is still the one it walked from, and a pass's swap drops, rather than extends, a part behind the base head; a racing-load test.
- FX5 to FX14: the cluster note on a shared cap (E10, for Jay); `<` at the loop-top load; that block is Task 14's; guard types and evictions; the FW1/FW2 wiring pinned in `case_f6_r23`; placements; "never loads for the backfill" (Task 15's merges read their inputs); Task 14's refusal sentence; case-map rows.
Not re-reviewed: both reviews approved revision 11, and these are their P2 and P3 follow-ups.

**Plan revision 13** (2026-11-08) changes execution routing at Jay's request: Sol implements
every task and Astra independently reviews it, including a final review of the integrated
branch. The Execution section and task-specific assignments below supersede historical
Opus/Sonnet assignments in the revision notes. No search mechanism changes in this revision.

**Plan revision 6** (2026-11-07) answers Codex's re-review of revision 2 (`36e125b8d`, the "Current review" section of the Codex review file), which reached the plan after revisions 3 to 5 had landed. Its one P1, the Thread state read from the store after the view's cache, is the defect Fable's FR3 found and revision 3 closed (`ZaSnapshot::state()`, plan call D13); revision 6 adds the regression case Codex asked for, `mail::threads::case_d13_thread_state_pairs_with_its_map` (Task 13), and applies its four P2s: the pass's swap and both compactions' swaps resize the `BudgetCharge`s as the parts grow, and the PL11 test releases every owner of a generation before expecting the counter to move (Task 6, 8, 15); Task 1's accessor returns the codec writer's raw buffer through a test-only `into_raw_bytes`, never `into_document`, which compresses above the watermark and appends a marker; the chain test expects `Loss(Hash)` where the hash check fires first and tests the header mismatch under the blob's real hash (Task 6); `case_content_wait_budget` drops the `indexed_to == last_change_id` equality (Task 9). Not re-reviewed.

**Plan revision 7** (2026-11-08) carries Jay's ruling on D13 (spec revision 20) and Codex's re-review of revision 6 (`ad442af44`, the "Current review" section of the Codex review file: **APPROVE** under the stopping rule, three P2s): the pass's swap admits resident content growth against the process budget with `make_room` and drops the part when room cannot be made, the growth measured by `ContentPart::loaded_bytes` as loads measure it (Task 8, Task 6; a cross-account test); the D13 regression parks at a new `SEARCH_VIEW_PAUSE`, after the view is acquired and while its guard is held, since a pause inside the acquisition loop would be answered by the loop's own reload (Tasks 10, 13); Task 1's Files and commit name `codec.rs`. Not re-reviewed.

**Goal:** A key account gets server-side search, sorting, snippets and real threads that behave as an ordinary account's do, from a per-account index built only inside requests that hold the account's keys, stored as sealed blobs under a key only those requests hold, with the thread id sealed in the index and substituted into every id a keyed session emits, while nothing new about the account's mail appears in the clear.

**Architecture:**

- **Phase A, measurement and primitives (Tasks 1–2):** the format benchmark on a 1 GB corpus (positions, per-record compression, the record bound `R`, indexing rate), whose table goes to Jay before the model is built; then the search key `swk`, principal field 152, the `ZA_SEARCH_*` configuration with its boot refusals, the format constants (`K`, `R`, `a`, the padding formula), the trace strings, the test hooks and counters, and the suite's cleanup exemptions.
- **Phase B, the index model (Tasks 3–5, `crates/store/src/za_search/`):** the thread part (records, aliases, revision rows, tombstones; `R(X)`, "the thread of `D` as of `X`", "the members of `T` as of `X`", the assignment rule of 6.1 with the survivor rule of H10), the content part (upstream's term set, positions, sort keys, `sent_at`, the attachment bit; exhaustive leaf resolution; per-record zstd frames), and the blob format: authenticated headers, the hidden and public sections, framing and padding, the manifest with its keyed tag serialized at commit.
- **Phase C, the loaded index (Task 6, `crates/common/src/za/search/`):** `Caches::za_search`, the loader with its loss and transient classes, incremental reload, the read lock, the pass lock, single-flight loads, two budgets, eviction, never swapped.
- **Phase D, the pass (Tasks 7–9, `crates/store/src/query/log.rs`, `crates/email/src/message/za/search/`):** the unfolded record reader and the openers' error classes, the pass itself (chronological consumption under the cap and the fixed reservation, the four write outcomes, one asserted batch through `commit_batch`, the post-commit swap) and setup's empty manifest, then the triggers on JMAP, IMAP and the keyed writers with the content request's wait budget.
- **Phase E, serving (Tasks 10–13, `jmap`, `imap`):** `Email/query`, IMAP SEARCH and SORT, snippets and the unavailable refusal from the loaded content part; the validated snapshot and the mapped ids at every emitting and accepting site with the method table's gates; the thread-valued reads; and the changes stream over a pinned range read unfolded, rendered as pages with upstream's state shapes, with the epoch rule.
- **Phase F, recovery and lifecycle (Tasks 14–16):** rebuild from loss with a fresh epoch, the content-only rebuild, the reconciliation after a truncated log; thread compaction with the bridge rule, the dead-alias rule, one-hop folding and the public-count pad, and tiered content compaction; destroy's marker, unlink and scan, `BlobRecode`'s walk, the `Reindex` remedy.
- **Phase G, coverage and closure (Tasks 17–19):** the key-mode variants, the leak rules and the differential test (the parity contract of 7.5), the system benchmark, the mutation sample, `CLAUDE.md`, the checklist and the outcome note.

The fork diff stays narrow: new modules (`crates/store/src/za_search/`, `crates/common/src/za/search/`, `crates/email/src/message/za/search/`, `crates/jmap/src/email/za_search.rs`, `crates/jmap/src/changes/za.rs`, `crates/imap/src/op/za_search.rs`, `tests/src/za/mail/{threads,index,differential}.rs`, `tests/src/za/search_bench.rs`) plus one-line insertions at the upstream sites spec 11 lists, each task naming its own.

**Tech Stack:** Rust 2024 on upstream Stalwart v1.0.3 (fork `0.4.0-beta.1`; internal crates `1.0.3`, `vault` `0.16.25`); upstream's `index_document`, `SearchQuery`/`Leaf` planner and forward-index codec (`crates/store/src/search/`), `MessageSortKeys`, `ThreadFields`, `commit_batch`, `hold_blob`/`BlobHold`, the blob codec; `vault::keys::{aad, seal, open}` (XChaCha20-Poly1305), HKDF sub-keys; `blake3` 1.8 (keyed tag, manifest hash; already a `store` dependency); `zstd` 0.14 through `store::write::compress::compress`; `roaring` bitmaps; tokio `RwLock`/`Mutex`; the `tests` crate harness (`TestServer`, `TestServerBuilder`, `SmtpConnection` on LMTP 11200, `za_unlock`, the JMAP and IMAP clients, `assert_is_empty`).

**Spec:** `docs/superpowers/specs/2026-11-04-zero-access-search-design.md`, revision 23 (2026-11-08): revision 18 (`9d3457e4d`), approved by Jay on 2026-11-07 after Codex approved it under the stopping rule (`24fbd54d8`, the Codex review file beside the spec; the Fable review file `2026-11-04-zero-access-search-design-fable-review.md` holds the four Fable rounds), with the wording corrections of plan calls B1 and E1 (revision 19, ruled 2026-11-07) and D13 (revision 20, ruled 2026-11-08), the provisional benchmark memory defaults (revision 21, ruled 2026-11-08), the fixed publishing target for every accepted padding minimum (revision 22), and the write-response view and acknowledged-write floor (revision 23). Cited by section number; where this plan and the spec disagree, the spec wins. The release 1, mail (release 3), invitations, key pinning, sharing and iMIP specs bind where it cites them. Project rules: `CLAUDE.md`, `docs/README-dev.md`, the plan 3 outcome note (gate inventory), the 1.0 upgrade outcome note (hook inventory, conflict ledger), the plan 13 outcome note (the mail rulings R0–R63).

## Spec questions / plan calls

Each is the plan's choice; none changes a decision of the spec. B1 and E1, marked *spec wording*, were ruled by Jay on 2026-11-07 and spec revision 19 carries their text; the rest stand as the plan's calls unless Jay rules otherwise. They are numbered by task group (A: Tasks 1–2, B: 3–6, C: 7–9, D: 10–13, E: 14–16, F: 17–19), and a task's text cites them by that number.

**Tasks 1–2**

- **A1.** **9.6's benchmark is in two halves.** 1.3 and 9.6 make the benchmark the plan's first task, but the pass rate, the compaction times, the validation read's cost, the mailbox entries' cost, the first-search latency and the design-size rebuild need the pass, the cache and compaction to exist. Task 1 measures what decides the blob format before anything is built (positions, per-record compression, the record bound, the indexing rate) and Task 19 measures the rest on the finished system; both tables go in the outcome note.
- **A2.** **No trained dictionary.** 4.2 says "the fork's static dictionary"; the workspace's `zstd` is built with `default-features = false` (`crates/store/Cargo.toml:36`), so no dictionary trainer is available without a new feature or dependency. Task 1 measures per-record frames with no dictionary and with each of upstream's `Dictionary::Term` and `Dictionary::Email` (`resources/zstd/`, `crates/store/src/write/compress.rs:13-19`), as figures; the content record's term bytes are upstream's stored search document, one zstd frame per document under upstream's `Dictionary::Term` (plan call B2), so no dictionary is chosen here. Training a fork dictionary is a follow-up if Jay wants one.
- **A3.** **A test-mode accessor on `IndexDocument`.** Its `fields` and upstream's `Document::encode` are `pub(crate)` (`crates/store/src/search/mod.rs:176-184`, `term.rs:629`), so the benchmark in the `tests` crate cannot measure term bytes; Task 1 adds `IndexDocument::za_bench_term_bytes(self) -> (Vec<u8>, Vec<u8>)` under `test_mode`, which runs upstream's own tokenization, `AccountIndexer::default().index_document(self, 0)` (`term.rs:100-135`, `pub(crate)` and so callable inside `store`; `AccountIndexer::build_batch`, `:276`, only emits postings, PL19), and encodes the result with and without its position lists. Nothing in upstream's path is lifted or changed.
- **A4.** **Attachments in the corpus are text.** The benchmark attaches `text/plain` files of 40 KB to 1 MB (upstream's extractor indexes them as attachment text); it builds no PDF or Office documents, so the "with attachment text" rate is a lower bound on extraction cost and the outcome note says so.
- **A5.** **The configuration refusal exits the process from `main`.** `Caches::parse` is infallible (`crates/common/src/config/inner.rs:122-140`), so `ZaSearchConfig::from_env()` is read there into `ZaSearchCache` with the defaults kept and the refusal's message retained, and `main` checks `za_search.refusal()` right after `verify_schema_version` (`crates/main/src/main.rs:64-72`) and exits `ExitCode::FAILURE` with `ServerEvent::StartupError`, the schema check's own pattern.
- **A6.** **The format constants live in Task 2.** The boot refusals need `R`, `a` and `K` (4.4) before the model exists, so Task 2 creates `crates/store/src/za_search/mod.rs` with the constants, `reservation()`, `padded_len` and `compaction_allowance` and their boundary tests; Tasks 3 and 5 add the model to that module. The record layout that fixes `R = 304` and `a = 15` is given in Task 2.
- **A7.** **`ZA_SEARCH_SEGMENTS` is refused outside `2..=255`** (plan revision 2, PL12). The manifest counts its thread blobs in one byte and 4.4 gives the knob no range; one segment would compact on every pass. A boot refusal beside 4.4's three, not a spec change; the alternative, a wider count, would still need a ceiling.

**Tasks 3–6**

- **B1.** *Spec wording, ruled by Jay (spec revision 19).* **`R = 304` and `a = 15` by derivation** (the layout is Task 2's comment and Task 3's constants). The spec's 296 and 24 were
   estimates; R20 asks for derived maxima. Nothing else changes: the inequality, the
   reservation (529 bytes, so `ZA_SEARCH_PASS_CAP = 48` is under `H / 529 = 61`, H7) and the
   compaction class at 30,000 messages (16 MiB) all hold. If Jay prefers the spec's figures
   as reserved room, set `ALIAS_LEN = 24` with 9 zero bytes in the alias body; the plan does
   not.
- **B2.** **Upstream's stored document is the content record's term part**, so the
   G5 dictionary is upstream's `term-v1.dict` and Task 1 need not train one; Task 1 measures
   the ratios only (A2).
- **B3.** **A compaction whose hidden content exceeds its public-count allowance is a defect**:
   `FormatError::AllowanceExceeded`, the pass aborts and traces `ZA_SEARCH_ABORTED` with
   class `defect`; it never writes a blob whose size depends on content (invariant 48).
- **B4.** **A phrase check over a corrupt record is unresolvable** (`ResolveError::Corrupt`), so
   the query refuses (7.4's third case) rather than widening.
- **B5.** **Idle eviction runs on access** (`sweep_idle` from `entry()`), not from a timer task:
   nothing new runs in the background, though the index is not key material.
- **B6.** **Memory is charged until its last holder drops it** (revised for PL11): each loaded
   thread part and content part carries a `BudgetCharge` that subtracts its bytes on `Drop`, so a
   generation evicted or invalidated while a reader still holds its guard stays charged until
   that reader finishes, and a replacement load is admitted against the bytes really alive, as
   upstream's `MemoryReservation` does (`crates/common/src/network/memory.rs:178`). The first
   revision decremented at removal from the map, which repeated invalidations could outrun.
- **B7.** **`AssignedIds` test constructor**: Task 5's tag test needs a current change id in an
   `AssignedIds`; `set_current_change_id` is `pub(crate)` and needs a reserved change id, so
   the plan adds a one-line `#[cfg(test)] pub(crate) fn with_current_change_id(u64)` beside
   `write/mod.rs:1648`. If Jay prefers no upstream-file edit, the test can
   build the ids through a `BatchBuilder` with a change-id reservation and `resolve`.
- **B8.** **`thread_of` carries a cycle guard** (`aliases.len() + 1` steps) although the
   construction has no cycles (an alias always points at a thread that survived at its
   revision); a cycle would be a defect, and the guard turns it into the singleton rather
   than a hang.
- **B9.** **Prefix matching follows `fetch`'s rule** (`query.rs:787-792`): verbatim terms of at most
   16 bytes not ending in `*`; a longer term never matches a prefix, as upstream.

**Tasks 7–9**

- **C1.** **The retry window for an absent message blob is kept in memory** (Task 6's `IndexEntry::absent_since`, keyed by the message blob's hash), the attempt count in the manifest (R34). An eviction forgets the window's start, so the window can only lengthen; the durable bound stays `ZA_SEARCH_SKIP_AFTER` passes. Plan's call; spec 5.1 names only the window and the count.
- **C2.** **A pass that works cursor items consumes no log record** (records resume on the next pass once the cursor is `None`), rather than continuing into records within the same pass when the cursor is exhausted under the cap. Simpler for the implementer; one extra pass per rebuild. Plan's call under 5.1's order.
- **C3.** **The committed manifest's hash is recomputed** from the assigned change id (`AssignedIds::change_id(account_id, SyncCollection::Email.change_group())`) and `ManifestValue::encode`, not re-read from the store, which requires `ManifestWrite` to serialize exactly as `encode` does (Task 5 pins it). Plan's call; 5.1 says only "record the new manifest hash".
- **C4.** **The setup manifest joins the vault batch through a trailing `extra` parameter** on `vault_batch_write`, `vault_commit_store`, `za_vault_commit` and the http `za_commit`, every other caller passing `None`, rather than a second batch (which could not be in "setup's vault batch", 4.2, F22). Plan's call.
- **C5.** **`is_current` is evaluated from `scanned_to`** rather than re-reading the records in `(indexed_to, scanned_to]`: the pass sets `scanned_to` only to a change id it read whole, and every insert or deletion it read moved `indexed_to` (indexed, skipped or tombstoned) or aborted (nothing moved), so the two statements of 7.4 coincide. Plan's call.
- **C6.** **The JMAP trigger runs when any method call of the request names a mail object** (`Email`, `Thread`, `Mailbox`, `SearchSnippet`, `EmailSubmission`, `Blob`), once, for the token's own account. A request of only `Core/echo` or `Identity/*` runs none. Plan's call under 5.2 and I7.
- **C7.** **The suite's "restart"** is `simulate_restart` (the account's loaded index, resident keys and message cache dropped; Task 8's helper), since the za suite runs one server; the manifest and the blobs are then re-read from the store as after a restart. Plan's call for every case that says "restart" (R1, R10, R22, R26).
- **C8.** **The transient skip's recorded loss class** when `ZA_SEARCH_SKIP_AFTER` is reached is `BlobAbsent` for `AbsentWithinWindow` and `Parse` otherwise (a persistent non-absent fault); `ZA_SEARCH_SKIPPED` carries it as the reason (H5 says "the persistent class"). Plan's call.

**Tasks 10–13**

- **D1.** **Benchmark-independent default kept (plan's call).** `za_content_current` treats `scanned_to` as proof that `(indexed_to, scanned_to]` holds only update records, which is how 5.1 step 2 defines it; the pass (Task 8) must never set `scanned_to` after consuming an insert or deletion it did not stage.
- **D2.** **`QueryResults` accessor (plan's call).** `QueryResults::into_bitmap()` exists (`local.rs:200`); no store change is needed for the pure merge.
- **D3.** **One test-mode configuration override (plan's call, Task 2's mechanism).** Cases that need a short wait budget, a small pass cap, few segments or a low account cap set `test_hooks::set_search_config_override(Some(ZaSearchConfig { .. }))`, which `Server::za_search_config()` reads before the boot value in test mode; Task 9's `search_config_guard` restores it on drop, and the cases write it as `search_config_guard!(field: value)`. Every per-knob setter a part named (`set_search_wait_override`, `set_search_pass_cap_override`, `set_segments_override`, `set_account_max_mb_override`) is this one override.
- **D4.** **A test-mode `SEARCH_SWAP_SKIP` hook (plan's call, for Task 8).** Another node's publication is simulated in one process by a pass that commits and skips its swap (`SEARCH_SWAP_SKIP: Mutex<Option<u32>>`, checked at 5.1 step 3's swap); without it the cross-node and stale-state cases cannot be driven. `SEARCH_SNAPSHOT_PAUSE`'s pause point sits in Task 10's `za_view`, at its step 2, between the state read and the manifest read.
- **D5.** **The "no map" error's event type (plan's call).** The generated `trc::JmapEvent` has no `serverUnavailable` variant; jmap-proto maps every non-JMAP `trc::Error` to `serverUnavailable` (`error/method.rs:190-198`), so `za_no_map_error` is a `StoreEvent::UnexpectedError` carrying `ZA_SEARCH_UNAVAILABLE` and the reason class, and IMAP adds `ResponseCode::Unavailable` at its call sites. The spec's wording is met; the event class in the logs is a store event, recorded in the outcome note.
- **D6.** **`EmailSubmission/changes` is gated on keys as the table says** though it emits submission ids only; the plan follows decision 1's list as written and notes the looseness for Jay.
- **D7.** **`Email/copy`'s translation sites (`copy.rs:134, 242`) are left untouched**: `za_assert_mail_copy` refuses every copy touching a key account before the lookup, so no mapped id reaches them.
- **D8.** **Interim between Tasks 11 and 13.** With mapping on and the page not yet rendered by the fork, a key account's `Email/changes` emits raw singleton-prefixed ids; no suite case runs against that interim (the variants run in Task 17), and Task 13 closes it.
- **D9.** **The retained-bound refusal (plan's reading of 6.3).** `S` is refused when `S < max(marker(Email), marker(Thread))`, the change id of `truncate_change_log`'s marker in each log (`transaction.rs:238-244`), for both `Email/changes` and `Thread/changes`: pruning is by the shorter log, so both methods refuse below either bound. Task 7's `first_retained` is read as that marker's change id.
- **D10.** **Sheet additions this part relies on**: `ThreadPart::records()` (Task 3; 13.1); `ContentPart::resolve(&[SearchFilter])`, `sort_ranks(SortField, &RoaringBitmap)` with `SortField::SentAt`, `document_ids()` (Task 4, as Task 4 defines them); `SEARCH_CACHE_HOLD` and `SEARCH_MANIFEST_FAIL_ONCE` hooks (placed by Task 11 in `email`'s cache and `common`'s loader); `ZaSnapshot::state(&self) -> u64` is the view's cache counter (plan call D13 replaces the sheet's store-reading `thread_state`); `ingested_into_object(email, thread_id)` gains a parameter.
- **D11.** **Candidate scan cost (plan's call).** The stream's candidate set is a linear scan of the thread records (≤ 30,000 at the design size, a few milliseconds); Task 19's benchmark measures a `/changes` call and a revision-indexed shortcut is deferred unless it shows.
- **D12.** **`THREAD` in `mail::search` between Tasks 10 and 12** compares UID sets, not groupings; Task 12 tightens it.
- **D13.** *Spec wording, ruled by Jay on 2026-11-08 (spec revision 20).* **A Thread method's state is the validated view's cache counter**, `cache.last_change_id`, the Email group's counter the cache was built at, which the view proved at or past the newest row and the epoch; it is not read from the store after the manifest (6.2's I5, 6.3's "`E` for `Thread/changes`, and `Thread/get`'s `state`, is the Thread log's change id", and the revision-16 summary line of the spec's header). I5's purpose, a Thread state never below a publication the map holds, is met by the view's counter check; its mechanism, a second store read, would pair a state with a cache from an earlier moment, and a message delivered between the two reads would be absent at `E` by the cache and so never rendered (Fable review FR3). Ruled; the three sentences (6.2, 6.3, the header) are spec revision 20's.

**Tasks 14–16**

- **E1.** *Spec wording, ruled by Jay (spec revision 19).* **The benchmark aside, the only wording the plan adds to 5.5 is the floor row.** 5.5 prunes "a revision-table row older than [the bound]"; after that, `R(X)` for a servable `X` between the bound and the next retained row has no row to resolve to, and a folded record's assignment `(target, r_folded)` would read as "not yet published" at such an `X`. The plan keeps the newest row below the bound (one row) so `R(X)` is defined at every servable state. No channel: the row count is public already (4.2). Jay may prefer the alternative, every folded record's revision set to the first retained row's revision, which keeps 5.5's wording but changes what `R(X)` means for those records; the plan takes the floor row.
- **E2.** **The reindex reset is marker + unlink + clear, not a marker that persists.** 5.1/I6 say the reindex task "writes the index's destroying marker" so the next keyed request rebuilds, while 5.7 says a loader treats the marker as "no map" (so a rebuild cannot sneak in between destroy's marker and its range delete and leak blobs). Both hold only if the reindex path also clears field 152 after unlinking: the next keyed request then finds no manifest, which is loss (H11). The plan does that (`za_search_destroy(account_id, clear_field = true)`).
- **E3.** **Cursor fields**: `next`/`end` for the rebuild cursor and `next_below` for the content cursor, because "last done" cannot express "nothing done" with document id 0 present.
- **E4.** **The reconciliation's progress** (revised for PL6, restoring spec 5.4's "cursor over the difference"): the marker carries the bound and `gap_next`, the cursor over the gap inserts, which moves past every gap insert indexed or skipped as loss; gap deletes need no cursor (a tombstone always applies, so it leaves the recomputed difference); the retained records are consumed from the first retained one until `indexed_to` reaches it and from `indexed_to` after, so completed work is never re-consumed; a second truncation that passes `indexed_to` restarts the reconciliation; the marker clears when the difference at or above the cursor is empty and `indexed_to` has reached the cache's change id as read at that pass. The first revision of this plan kept the bound only and recomputed the difference, which a gap insert skipped as loss never leaves.
- **E5.** **A compaction whose hidden section exceeds its allowance is refused as a defect**, traced, and the pass writes uncompacted (the next pass retries); by the injection it cannot happen, and `compact_tests` asserts the fit on R35's fixtures. The alternative, growing the pad to fit, would reopen the class channel.
- **E6.** **Over the cap, no content merge runs on that node**: the chain is not extended (4.2), and a node that cannot load the content part under its cap does not perform the full merge; a node with a higher cap does.
- **E7.** **The destroy step runs for every account** (one read of field 152 for an ordinary account, which has none): the marker written in the `None` case is what fences a rebuild that a concurrent provisioning could start; the range delete removes it.
- **E8.** **The config overrides the tests need** are the one `set_search_config_override` of Task 2 (plan call D3), written in the cases as `search_config_guard!(field: value)`.
- **E9.** **Restart is simulated**, not performed: the za suite runs one server, so `simulate_restart` evicts the account's search entry, its resident keys and its message cache, which is every in-memory state a restart would lose; `scanned_to`, the attempt count's in-memory part and the loaded index are all gone after it.
- **E10.** *Ruled by Jay on 2026-11-08.* **The content-only rebuild drops the chain only for a content loss; for a raised cap or a lagging `content_to` it keeps the chain and backfills**, as spec 5.4 and 9.2 describe: the content cursor walks the present messages without a content record, newest first, so only the messages indexed while suspended are re-opened, and the pre-suspension blobs stay linked. The backfill's cursor needs the loaded content part to know which messages have content, so it runs only on a pass whose node holds the part resident and current; the pass never loads it for the backfill (spec 7.4: only a content request waits for the content part; Task 15's merges read their inputs as spec 5.5 says, FX12), a content request's `za_search_ensure_current` loads it before its passes, so the backfill progresses when content is asked for, which is when it matters, and a pass that cannot run it runs everything else (revision 11, FW1, FW2). Plan revision 8 had the start drop the whole chain for every reason (FU12, FV13); Jay chose the backfill. In a cluster the nodes should share `ZA_SEARCH_ACCOUNT_MAX_MB`: with different caps a lower-cap node re-suspends what a higher-cap node's `CapRaised` start unsuspended, and requests alternating between them each write the manifest (FX5, a consequence of spec 4.2's one shared `suspended` flag); Jay confirmed it as an operator requirement on 2026-11-08: Task 19 records it in README-dev's search section and in the outcome note.

**Tasks 17–19**

- **F1.** **The IMAP variants `search` and `thread` are deleted, not mirrored.** A key account's IMAP session always holds keys, so no difference from upstream's body remains; the dispatch runs upstream's tests in both modes. `fetch` follows if its only difference was the refused `FROM` search (the implementer decides and reports); `store` keeps its training steps and loses its refused line.
- **F2.** **`query_changes` in key mode does not simulate upstream's `Move`** (revised for PL14). Upstream's `Move(from, to)` rewrites the stored `MessageData.thread_id`, which a key account's renderer never reads (6.2, 6.3), and no bridging import can stand in for it: the fixture's messages each carry their own subject (`test_{id}`), so they share no subject bucket and nothing a bridge references can merge, and the first `Move(0, 3)` would bridge message 0 to itself. In key mode a `Move(from, to)` therefore changes nothing and maps `to` to `from`'s unchanged id, so the later steps that name `to` act on that message; merge rendering in `queryChanges`, with and without `collapseThreads`, is pinned against the plain control by Task 13's `mail::threads::case_merge_rendering_parity`. The first revision's real-merge variant claimed a transition it never made.
- **F3.** **`can_stem` is `true` for key accounts** in the `query` and `search_snippet` variants: the content part is upstream's stemmed term set whatever the search store backs the ordinary accounts (7.5 parity by construction), so the stemming-dependent rows run on every backend.
- **F4.** **Interface additions placed in the producing tasks**: `ManifestValue::decode_unverified(bytes)` (Task 5; destroy's keyless read of 5.7 needs it anyway); the test-mode configuration override of plan call D3 (`set_search_config_override` in Task 2, `search_config_guard` in Task 9), needed by Tasks 15, 18 and 19 to reach a compaction or a refusal in seconds; the helpers `za_index_current`, `imap_index_current` (Task 17, or an equivalent from Tasks 10 to 13), `za_search_thread_part`, `log_records_since`, `compaction_class` (Task 18), and `read_manifest`, `principal_blob_links` assumed from Task 8; `public_len(tombstones, rows)` (Task 5) for the G1 byte-exact assertion (or the constants `TOMBSTONE_LEN`, `REVISION_ROW_LEN` and the seal overhead exported, from which the test computes it).
- **F5.** **The membership case and the oracle attempts live in a new `leak_search::test`** after `leak::test_mail`, not inside it: `test_mail` is already 300 lines and these cases need three accounts and a thousand-message delivery; the spec's "`leak::test_mail` gains the rules" is kept for the scan rules and the counts.
- **F6.** **The differential corpus is `tests/resources/mailbox.gz` plus eighteen generated messages**, delivered to both accounts in one LMTP transaction so bytes and Message-IDs agree, compared by Message-ID rather than by document id. The `K` deviation is constructed exactly once (`K1`) and asserted as a difference; every other form must agree.
- **F7.** **The system benchmark's networked-store figure** is measured only if a PostgreSQL store is available locally (Docker); otherwise it is recorded as pending for a later run, with the RocksDB figure and the raw point-read timing in its place.
- **F8.** **A default of 4.4 changes only on Jay's decision**: Task 19 records a recommendation per knob and the controller puts it to him before the documentation steps, as spec 1.3 and 9.6 intend ("adjusts them before release").
- **F9.** **The compaction-class case cannot observe folding or dropping directly**, so `leak_search::compaction_class` asserts the class alone; the resolution twins (bridge rule, dead-alias rule, one-hop fold) are Task 15's unit fixtures and mutations M50–M52.
- **F10.** **No CI workflow change**: the new za modules run inside `za::za_tests`, the benchmarks are `#[ignore]`d, so `ci_steps.py`'s table is untouched; the gates scripts are run in Step 5 to confirm it.

## Execution

Subagent-driven development on a new branch `release7-search` cut from `za-search-design`
(the spec at revision 23 and this plan committed there). Start execution with an Astra
primary (`gpt-6-astra`, `high` reasoning), which orchestrates, adjudicates, integrates and
performs final verification. The primary dispatches both implementers and reviewers as
siblings; Sol implementers do not dispatch Astra reviewers. This keeps both routes within
the originating agent's capability ceiling.

| Role | Model | Reasoning | Scope |
|---|---|---|---|
| Implementer | Sol (`gpt-6.1-sol`) | `high` | Tasks 3, 4, 5, 6, 8, 10, 11, 13, 14, 15 |
| Implementer | Sol (`gpt-6.1-sol`) | `medium` | Tasks 1, 2, 7, 9, 12, 16, 17, 18, 19 |
| Independent reviewer | Astra (`gpt-6-astra`) | `xhigh` | Every task and the whole branch |

Set the model and reasoning effort explicitly in each dispatch. Implementers receive
bounded task briefs; reviewers are independent and receive the requirements and actual
diff, not an instruction to endorse the implementer's conclusions. Every finding is
reported with confidence and severity and filtered at adjudication (no severity filter in
a reviewer brief). The primary retains the repository's required tests and final checks.
After Task 19 and its required checks, dispatch a fresh Astra reviewer with clean context,
the base/head refs, repository guidance and accepted spec/plan to review the whole branch.
Resolve its findings before declaring the implementation complete.

No Fable review during execution; Jay authorizes one at the PR stage. Briefs for Sol
implementers carry a scope-discipline line ("deliver at the scope of the task; don't widen
it") and never ask them to verify or double-check their work. Task 1's table goes to Jay
before Task 3 starts (a checkpoint; Task 2 may run meanwhile). Ledger:
`.superpowers/sdd/2026-11-07-zero-access-16-search/progress.md`. Commits: the plan gives each
subject; the executing session adds a blank line and its trailer lines.

## Global Constraints

Commands:
- Every cargo command starts with `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`.
- Product build: `cargo build --release -p stalwart --no-default-features --features rocks`. Never build with `enterprise`.
- The http crate is `http@1.0.3` (`crates/http/Cargo.toml` version `1.0.3`): `cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise`; format it with `cargo fmt --manifest-path crates/http/Cargo.toml -- --check`.
- Per-task compile check, shipping configuration. The warning filter runs only inside the success branch, and the failure branch ends in `false`, so the block's exit status is nonzero after a failed build and a failed build is never read as `clean`, whatever the shell's options (PL18):
  ```bash
  log="${TMPDIR:-/tmp}/za-check.log"
  if cargo check -p stalwart --no-default-features --features rocks > "$log" 2>&1; then
    grep -E "^(warning|error)" -A3 "$log" | grep -E "crates/(store|common|email|jmap|imap|vault|services|types)/|crates/http/src/api/vault\.rs|crates/main/" || echo clean
  else
    tail -40 "$log"; echo BUILD FAILED; false
  fi
  ```
  Expected: `clean` and exit status 0. Any printed warning line, `BUILD FAILED`, or a nonzero status fails the step.
  Upstream's known warnings elsewhere are not this plan's.
- Test-mode compile check: `cargo check -p tests --tests`, with no warning in `tests/src/za/mail/{threads,index,differential}.rs`, `tests/src/za/search_bench.rs`, `tests/src/utils/za.rs` or a line this plan touched.
- Unit tests, per crate (the features are those of CI's `UNIT_TEST_FEATURES`, `.github/workflows/test.yml:48-58`): `cargo test -p vault --features test_mode`; `cargo test -p store --lib --features test_mode za_search`; `cargo test -p common --lib --features test_mode za::search`; `cargo test -p email --lib --features test_mode,enterprise za::search`; `cargo test -p jmap --lib --features test_mode,enterprise za`; `cargo test -p imap --lib za`; `cargo test -p services --lib --features test_mode,enterprise za`; the http form above. If a crate's unit build needs a feature not listed here, use the one CI lists for it and say so in the task report.
- Integration: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`; at the end of Tasks 9, 13, 16 and 19 also with `BLOB_STORE=FileSystem`. Mail suites: the same prefix with `jmap::jmap_tests` and `imap::imap_tests`, plain and with `ZA_KEY_ACCOUNTS=1`. CalDAV (`webdav::webdav_tests`, both modes) runs in Task 19 only; this plan touches no calendar path. Upstream's `cal_itip` and `calendar::notification` sub-tests are timing-flaky: rerun once before calling a failure there a regression.
- Benchmarks (ignored tests, run by name): `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::search_bench::format -- --exact --ignored --nocapture` (Task 1) and `za::search_bench::system` (Task 19). Each builds its own `TestServer` and deletes it.
- One cargo command at a time; never while Jay's manual server is under test.
- Formatting: `cargo fmt -p <crate> -- --check` for `store`, `common`, `email`, `jmap`, `imap`, `vault`, `services`, `types`, `tests`; the http form above.

Spec values, verbatim (4.2, 4.4, 7.4, 8, 6.2):
- `K = 16`, a constant of format version 1 (`SEARCH_FORMAT_V1 = 1`), not a knob; a change of `K` is a format bump and a rebuild (H22).
- `R = RECORD_MAX = 304` bytes (the encoded maximum of a thread record at `K = 16`, framing included), `a = ALIAS_LEN = 15`, `t = TOMBSTONE_LEN = 12`, `REVISION_ROW_LEN = 12`; `reservation() = R + (K − 1) · a = 304 + 225 = 529`.
- The hidden section of a publishing pass is never more than `H + R + (K − 1) · a` bytes and always pads directly to `M`, rejecting a framed payload above `M` (P1-PAD); the class helper `padded_len(framed, M) = max(M, 2^ceil(log2 framed))` is used for the public compaction allowance, never the actual publishing payload; a compacted hidden section pads to `compaction_allowance(N_records) = N_records · (R + a) + 4` framed, then the same rounding (decision 2 (c), R35).
- The knobs (4.4), each read once at boot by the parse-and-fall-back convention (`KeyCacheConfig::from_env`, `crates/vault/src/cache.rs:41-53`) and logged beside the key cache's (`crates/common/src/manager/boot.rs:199-210`):

  | Variable | Default | Meaning |
  |---|---|---|
  | `ZA_SEARCH_MEM_MB` | 1536 | process budget for loaded content parts, transients included |
  | `ZA_SEARCH_THREAD_MEM_MB` | 256 | process budget for loaded thread parts, transients included (G6) |
  | `ZA_SEARCH_ACCOUNT_MAX_MB` | 512 | largest content part a node loads or extends; at most a third of `ZA_SEARCH_MEM_MB` (G6) |
  | `ZA_SEARCH_IDLE_SECS` | 900 | eviction after disuse |
  | `ZA_SEARCH_PASS_CAP` | 48 | admitted items after which a pass stops at the next insert boundary (`C`); at most `H / (R + (K − 1) · a)` (H7) |
  | `ZA_SEARCH_PASS_MB` | 32 | opened message bytes after which a pass stops at the next insert boundary |
  | `ZA_SEARCH_PASS_HIDDEN_KB` | 32 | reserved hidden thread bytes after which a pass stops at the next insert boundary (`H`) |
  | `ZA_SEARCH_PASS_MS` | 2000 | elapsed time after which a pass stops at the next insert boundary (G18) |
  | `ZA_SEARCH_WAIT_SECS` | 30 | how long a content request completes passes before refusing |
  | `ZA_SEARCH_SEGMENTS` | 16 | thread-blob count, and content-chain length, that trigger compaction |
  | `ZA_SEARCH_CHECKPOINT` | 1000 | update-only records after which a pass writes a manifest-only checkpoint (G19) |
  | `ZA_SEARCH_SKIP_AFTER` | 8 | aborted passes on one document after which it is skipped as loss (H5) |
  | `ZA_SEARCH_ABSENT_RETRY_SECS` | 10 | how long a blob a committed manifest names may be reported absent before that is loss (H6) |
  | `ZA_SEARCH_PAD_MIN` | 65536 | exact publishing target and minimum compaction class in bytes (`M`) |

- The boot refusals, with `H = ZA_SEARCH_PASS_HIDDEN_KB · 1024`: `ZA_SEARCH_PAD_MIN < H + R + (K − 1) · a + 4` (at the defaults the bound is `32768 + 529 + 4 = 33301`); `ZA_SEARCH_PASS_CAP > floor(H / (R + (K − 1) · a))` (at the defaults `floor(32768 / 529) = 61`); `ZA_SEARCH_ACCOUNT_MAX_MB > floor(ZA_SEARCH_MEM_MB / 3)` (at the defaults `512`); `ZA_SEARCH_SEGMENTS` outside `2..=255` (plan revision 2, PL12: the manifest's thread count is one byte). `R` and `a` are the constants, never measured (R20).
- Refusal texts: `ZA_MAIL_SEARCH_UNAVAILABLE = "Search for this account is not available right now; try again shortly."` (7.4, a content request only, in the three cases of 7.4), beside the unchanged keyless text `ZA_MAIL_SEARCH_REFUSED` (`crates/common/src/za/mod.rs:31`). On IMAP both are `NO [CANNOT]`; on JMAP `unsupportedFilter` / `unsupportedSort`. The no-map failure of 6.2 is `serverUnavailable` on JMAP and `NO [UNAVAILABLE]` on IMAP.
- Trace strings (section 8 and 4.3), each a `pub const &str` in `crates/common/src/za/search/trace.rs`, carried as `Details` of an existing event in release 5's fixed-constant convention (`crates/common/src/za/imip.rs:22-38`), with ids, counts, change ids, revisions and reason classes and nothing else (invariant 54): `ZA_SEARCH_PASS = "zero-access: search pass"` (a pass committed: account id, inserts, tombstones, published, revision, `indexed_to`, content bytes written, bytes merged by a compaction); `ZA_SEARCH_SKIPPED = "zero-access: search message skipped"` (account id, document id, the loss class); `ZA_SEARCH_ABORTED = "zero-access: search pass aborted"` (account id, the transient class); `ZA_SEARCH_LOST = "zero-access: search manifest write lost"` (account id); `ZA_SEARCH_REBUILD = "zero-access: search rebuild"` (account id, which of rebuild, content-only rebuild or reconciliation, started or finished, the loss class, the cursor size, the epoch, and the unlink set's source, `inventory` or `scan`); `ZA_SEARCH_SUSPENDED = "zero-access: search content indexing suspended"` (account id, the chain's size); `ZA_SEARCH_UNAVAILABLE = "zero-access: search unavailable"` (account id, the reason class); `ZA_SEARCH_EVICTED = "zero-access: search index evicted"` (account id, idle, budget or invalidation); `ZA_SEARCH_UNRESOLVED = "zero-access: search leaf unresolved"` (account id, the leaf's kind as an enum, never its field name, G9); `ZA_SEARCH_OVER_BUDGET = "zero-access: search thread budget exceeded"` (account id, the budget and the loaded bytes).
- Test hooks (`crates/common/src/za/test_hooks.rs`, test mode only, in the file's own style: a pause is a `Mutex<Option<Arc<Pause>>>` with `set_<name>_pause` and an awaited `<name>_pause_point(account_id)`; a one-shot is a `Mutex<Option<u32>>` with `set_<name>_once` and `take_<name>(account_id) -> bool`): `SEARCH_COMMIT_PAUSE`, `SEARCH_SWAP_PAUSE`, `SEARCH_SNAPSHOT_PAUSE`, `SEARCH_VIEW_PAUSE`, `SEARCH_SWEEP_PAUSE`, `SEARCH_PASS_PAUSE`, `SEARCH_DESTROY_PAUSE` (pauses); `SEARCH_CACHE_HOLD`, `SEARCH_SWAP_SKIP`, `SEARCH_OPEN_FAIL_ALL` (flags: `Mutex<Option<u32>>`, `set_search_<name>(Option<u32>)`, read with `search_<name>(account_id) -> bool`, not taken), `SEARCH_HOLD_EXPIRE` (the account's holds expire at once), `SEARCH_BLOB_FAIL_ONCE`, `SEARCH_OPEN_FAIL_ONCE`, `SEARCH_MANIFEST_FAIL_ONCE`, `SEARCH_DESTROY_FAIL_ONCE` (one-shots), `SEARCH_BLOB_SLOW` (`Mutex<Option<(u32, u64)>>`: a delay in milliseconds before the account's blob put); the counters `za_search_blob_reads(account_id) -> u64`, `za_search_message_opens(account_id) -> u64`, `za_search_max_item_aliases(account_id) -> usize` (the largest alias count one `assign` returned, PL8: test-mode instrumentation, never a trace field), `reset_za_search_counters(account_id)`. Each hook is cleared at the end of the step that set it. `SEARCH_CONFIG_OVERRIDE: Mutex<Option<ZaSearchConfig>>` with `set_search_config_override(Option<ZaSearchConfig>)`, read by `Server::za_search_config(&self) -> ZaSearchConfig` in test mode before the boot value (plan call D3; Task 9's `search_config_guard` wraps it). Traces are emitted through `za_search_trace!(CONSTANT, Key = value, ..)` in `trace.rs`, a macro over `trc::event!` on one fixed existing event type exported as `ZA_SEARCH_EVENT_TYPE: trc::EventType` so the suite can capture them (`Traced::start`).
- The method table of 6.2 for a key account (decision 1): keys required for `Email/get`, `Email/query`, `Email/changes`, `Email/queryChanges`, `Email/import`, `Email/parse`, `Email/copy`, `Email/set` create, `Thread/get`, `Thread/changes`, `Mailbox/get`, `EmailSubmission/get`, `EmailSubmission/query`, `EmailSubmission/changes`, `EmailSubmission/set`, `Blob/lookup`, `Blob/get`, `Blob/upload`, `Blob/copy`, `SearchSnippet/get`; keyless for `Email/set` update and destroy (prefix unchecked, the fork looks the id up by document id), `Mailbox/query`, `Mailbox/changes`, `Mailbox/set`, `Identity/*`, `VacationResponse/*`, `PushSubscription/*`, `Core/echo`, EventSource and WebSocket push (`StateChange` carries no Email id).
- Account names, unique across modules (the mail block's next free names are `keymail46` and `plainmail50`): Task 1 `search-bench` (its own server); Tasks 7–9 (`mail::index`, early cases) `keymail46`–`keymail49`, `plainmail50`–`plainmail51`; Tasks 10–13 (`mail::search` keeps its accounts; `mail::threads`) `keymail50`–`keymail57`, `plainmail52`–`plainmail55`; Tasks 14–16 (`mail::index`, later cases) `keymail58`–`keymail63`, `plainmail56`–`plainmail57`; Tasks 17–18 `keymail64`–`keymail68`, `plainmail58`–`plainmail59`; after this plan the next free names are `keymail69` and `plainmail60`. All at `example.com`. Task 16's destroy cases create and destroy `search-destroy-1` to `search-destroy-6` inside the task.

Invariants and code rules (the mail spec's 24–32 hold with section 10's amendments; this release adds 48–56; every task restates the ones it touches):
- 48: terms, positions, sort keys, threading fields, thread ids, thread history and revision table exist outside process memory only in blobs sealed under `swk`, bound to the account, the segment id, the kind and the predecessor; the manifest holds versions, revisions, change ids, segment ids, hashes, sizes and counts and nothing derived from content; the stored thread id, the `Thread` container and every change-log entry stay the document id; a publishing pass's hidden section always pads to `M` and its public section is a multiple of the tombstone count; a compacted hidden section pads to a function of public counts; only the time cap follows what the index found; `swk` leaves `SessionKeys` only as the zeroized copy in a manifest batch's object.
- 49: the index is loaded, built and written only inside a request holding the account's own keys, on that request's task; no background task, delivery or keyless path reads or writes it; it is dropped on every invalidation that drops the keys and never swapped by the cache swap tier.
- 50: the index is a deterministic function of the present mail and the log's records in order (after a loss, of the cursor's chronological order); `indexed_to` names a record, the last insert done in it and whether it is complete, resumed inclusively; everything advances in one asserted batch through `commit_batch`; a lost or failed batch leaves store, links and memory as the committed manifest describes; update-only passes write nothing; the order is chronological with a fixed reservation per admitted item; a logged deletion is always applied; a transient error never skips or rebuilds; no permanent link exists outside the committed manifest's reach except after a chain break the next rebuild or destroy finds by scan.
- 51: every content leaf and sort of a key account's query is resolved from a current content part or the query is refused; never from the search store, never by decrypting and scanning, never from an index behind the request's view; results are merged, masked, ranked and paged by upstream's code.
- 52: a thread id leaves the server only inside a keyed response, mapped from a map validated against the manifest's hash and identity version after the state was read, with the change counter at or past the newest publication and the epoch; every emitted Email id carries it, every supplied id is checked against it, and a thread change is rendered from the assignments, aliases and revision table over a pinned range read unfolded, existence from the raw records before the sealed record, the existing set always the message cache's; a request with no loadable or validated map fails; no thread-valued read waits for a pass's I/O.
- 53: the pass's keyless writes are the manifest, the links and unlinks, and for every pass that indexed a message one Email item update and one Thread property change of the lowest indexed document id and a container property change of every mailbox; nothing for a pass that indexed none but a manifest-only checkpoint; the boundary batch is the same shape with the lowest message present, or the mailbox entries alone.
- 54: traces carry account and document ids, counts, change ids, revisions and reason classes, never a term, header, Message-ID, sort key, query string or thread membership.
- 55: destroy fences with the destroying marker carrying the replaced manifest's inventory, then unlinks what the inventory and its chain reach, or every principal-linked blob by scan when they cannot be read, before the Property range is removed; a manifest-creating batch asserts the vault record; the purge then holds nothing of the index.
- 56: a deleted message's identity and an alias are retained, sealed, until their change id leaves the shorter of the Email and Thread logs; a rebuild allocates a fresh change id for its epoch and refuses every sync state below it whose method's log holds a record of the method's kind in `(S, epoch]`, never a fresh `Initial` or `Exact(0)`; an initial continuation requires its pinned end at or above both retention and the validated map's epoch (P1-PAGING, Task 13); a newer manifest format is never loss, and loss of a content blob alone never moves the epoch.
- No stored struct changes layout: the index's blobs are the fork's own format (`SEARCH_FORMAT_V1`), exempt from the rkyv rule; the manifest is a new field, 152; `MessageData.thread_id` and the change log are untouched.
- Non-key accounts take unchanged upstream code paths: every new branch is behind a key-account check, and the one shared helper this plan extracts (Task 16's link walk) is called by upstream's code with upstream's arguments (Task 1's accessor calls upstream's tokenizer and changes nothing of it).
- No settings, permissions or event variants are added (`crates/registry/src/schema/*` and `crates/trc/src/event/enums.rs` are generated); configuration is the `ZA_SEARCH_*` environment and tracing the fixed strings above.
- Every source file starts with the SPDX header copied from a neighbour. Never open issues, pull requests or reports against upstream `stalwartlabs/stalwart`; an upstream bug found here goes in the outcome note, described without naming the private tracker.
- Reviews: once Task 18's differential test passes, a later finding is in scope if it breaks invariants 48–56 (or an earlier one as amended) or shows the differential would pass on wrong behaviour; anything else is recorded as a residual or deferred in the outcome note.

Tests:
- New za modules `tests/src/za/mail/threads.rs`, `tests/src/za/mail/index.rs` and `tests/src/za/mail/differential.rs`, declared in `tests/src/za/mail/mod.rs` (`:50-59`) and run from `tests/src/za/mod.rs` after `mail::search::test` and before `mail::traces::test` (`:222-223`), in the order `threads`, `index`, `differential`; each `pub async fn test(test: &mut TestServer)` runs its cases as inner async fns named after the review tag they pin (`case_r21_checkpoint`), in the order its task lists. `tests/src/za/search_bench.rs` holds the two `#[ignore]`d benchmarks and is never called from `za_tests`.
- Each module deletes what it creates except its key accounts, which the suite destroys at the end (`destroy_key_accounts`, `tests/src/za/mod.rs:229`); `assert_is_empty` then runs, with field 152 and the index's blobs exempt only while their account exists (Task 2).
- Inbound mail goes over LMTP 11200 (`SmtpConnection::connect().await.ingest(from, &[to], message)`, `tests/src/utils/smtp.rs:37, 59`) or in process through `email_ingest` with `IngestSource::Smtp { .. }` as `mail::fence` calls it (`tests/src/za/mail/fence.rs:1631-1648`); a key account's message is sealed at delivery under its served public key, so every message in these modules is a stub plus a sealed blob (mail spec 5.1).
- Helpers shared by several tasks live in `tests/src/utils/za.rs`, named in the task that first needs them and repeated in Interfaces where consumed; session keys for store-level reads come from `za_unlock(test, account_id, password) -> Arc<SessionKeys>` (`tests/src/utils/za.rs:408`).
- Hooks are cleared at the end of the step that set them. An existing za assertion that fails only because a search is now served or an id is now mapped is adjusted with a comment citing the spec section and listed in the task report; any other failure is a regression.
- `webdav_tests`, `imap_tests` and `jmap_tests` are run in both modes in Task 19; the key-mode variants change in Task 17.

## Review Focus

Five inputs the spec implies but its test catalogue does not name, most likely first; each has its test in the task that owns the code (the Case map's last rows).

1. **A message with no `Date` header or an unparsable one.** A person sorting by date or searching `SENTSINCE` expects such a message where the ordinary account puts it, not dropped or first. Task 8 stores upstream's effective sent time, the received time plus `sent_at_offset`'s clamped offset (`za_effective_sent_at`, PL10), so such a message sorts and matches at its received time as on an ordinary account; Task 8's unit test `effective_sent_at_is_upstreams` pins `za_effective_sent_at(None, 100) == 100`, `za_effective_sent_at(Some(200), 100) == 200` and `za_effective_sent_at(Some(i64::MAX), 100) == 100 + i32::MAX as i64`; Task 18's corpus carries two messages without a `Date`, one with `Date: yesterday` and one dated in the year 9999, and the differential asserts equal order and equal `SENT*` results.
2. **A message whose term set exceeds upstream's 1 MiB document cap.** A newsletter or a log attachment must still be found by the terms upstream keeps, and the pass must not abort on it. Task 4's `s42_document_cap_is_upstreams` pins the truncation at unit level; Task 18's corpus carries a 1.2 MiB body and the differential asserts equal results for a word inside the kept prefix.
3. **IMAP searches in a non-UTF-8 `CHARSET` and multi-key `SORT` with a reversed content key.** Desktop clients still send `CHARSET ISO-8859-1` literals and `SORT (REVERSE SUBJECT DATE)`; upstream decodes the charset before the filter tree is built, so the resolution must see the decoded term. Task 18 adds both forms to the differential.
4. **An account invalidation between a pass's commit and its swap.** A password change on another connection drops the keys and evicts the loaded index while a pass is between step 3's commit and its swap; the swap must not put a stale or orphaned entry back, and the next request must load the committed manifest. Task 8's `case_invalidation_between_commit_and_swap` parks the pass at `SEARCH_SWAP_PAUSE`, evicts, and asserts the next load equals the store.
5. **A message with no `Message-ID` and a whitespace-only `Subject`, and duplicate Message-IDs across imports.** Both are common in imported mail. Upstream threads a message with no references as a new thread and never uses it as a candidate; duplicates share a root. Task 3's `s61_empty_threading_fields_are_a_singleton_and_never_a_candidate` and `s41_duplicate_message_ids_share_the_root` pin the rules, and Task 18's corpus carries one such message.

A sixth, folded into Task 9's `case_content_wait_budget`: a content search that times out under `ZA_SEARCH_WAIT_SECS` must leave the passes' progress in the store so that the next search needs less work, not more.

## Case map

Every test case the spec names (9.1 unit, 9.2 suite, 9.3 variants, 9.4 leak, 9.5 differential, 9.7 mutation) with the task that owns it and the test that pins it; the Review Focus cases are at the end.

| Spec case (tag) | Task | Test |
|---|---|---|
| 9.6 first half: content part size with and without positions; per-record compression against one frame (G5's cost); the thread part's record size against `R` at `K = 16`; load (decompression) time; the pass's per-message rate with and without attachment text | 1 | `za::search_bench::format` (ignored; its table in the outcome note) |
| 9.1: `swk` derivation is stable across a rewrap (3.1) | 2 | `vault::session::tests::session_keys_carry_the_search_key_and_it_survives_a_rewrap` |
| 9.1: the configuration refuses `M` below the inequality (R20), and refuses a value `C · R` leaves no margin under | 2 | `common::za::search::config::tests::the_pad_minimum_must_cover_the_budget_the_overshoot_and_the_frame`, `the_pass_cap_must_bind_under_the_budget` |
| 4.4: `ZA_SEARCH_ACCOUNT_MAX_MB` at most a third of `ZA_SEARCH_MEM_MB` (G6) | 2 | `the_account_cap_is_at_most_a_third_of_the_budget` |
| 9.1: the environment variables parse and fall back | 2 | `defaults_when_unset_or_malformed`, `a_value_parses_with_surrounding_space`, `a_zero_is_malformed_where_zero_cannot_work` |
| 9.1: the padding formula at framed lengths just below, at and above each class boundary, payloads of `M − 5`, `M − 4` and `2M` (F1, R15, R20, R21, G1), and the compaction allowance at equal counts (decision 2 (c)) | 2 | `store::za_search::tests::padding_rounds_to_a_class_above_the_minimum`, `the_compaction_allowance_is_a_function_of_the_record_count` |
| 4.2: `R`, `a` derived from `K` and the record layout, not measured (R20) | 2 | `the_record_bound_and_the_reservation_are_the_specs`; Task 1's measured maximum ≤ 304 |
| 8: traces carry fixed strings (invariant 54) | 2 | `common::za::search::trace::tests::every_search_trace_is_a_fixed_prefixed_string` |
| 5.7: `assert_is_empty` exempts field 152 and the index's blobs while their account exists | 2 | the suite's end-of-run assertion from Task 8 on (`tests/src/utils/cleanup.rs`) |
| The thread rules reproduce `find_thread_id` and `ThreadMerge::merge` on a fixture, merges included (9.1) | 3 | `s61_*`, `h10_survivor_is_the_thread_with_most_candidates_ties_lowest` |
| The survivor is the thread with the most matching candidates, ties lowest (H10) | 3 | `h10_survivor_is_the_thread_with_most_candidates_ties_lowest` |
| Duplicate Message-IDs share the root (5.1) | 3 | `s41_duplicate_message_ids_share_the_root` |
| `K` references each naming a present message in a distinct thread write exactly `K − 1` aliases, and none can write more (R20) | 3 | `r20_k_references_in_k_distinct_threads_write_exactly_k_minus_one_aliases`, `r20_no_assignment_writes_more_than_k_minus_one_aliases` |
| Insert followed by a deletion of the referenced message, in order vs deletion first (R16) | 3 | `r16_insert_then_delete_in_order_threads_as_upstream_did` |
| Cursor fixture: chronological threads the control's way, newest-first fans in (R16, R20) | 3 | `r16_r20_cursor_chronological_order_threads_as_the_control` |
| "The thread of `D` as of `X`" before, at, between and after two publications; first assignment from the singleton (R2, R5) | 3 | `r5_thread_of_d_as_of_x_before_at_between_and_after` |
| The alias chain resolves thread and members across two merges and a deletion after `X` (R25) | 3 | `r25_members_of_t_as_of_x_across_two_merges_and_a_later_deletion`, `s63_exists_at_follows_creation_and_deletion_change_ids` |
| A deleted message resolves through its retained record (R3, 6.3) | 3 | `s63_a_deleted_record_resolves_through_its_retained_record`, `r3_tombstone_keeps_identity_and_drops_fields` |
| Folding an alias at compaction leaves every resolution unchanged; one hop (5.5, R35) | 3 (helper), 15 (the rule) | `s55_fold_alias_moves_one_hop_and_keeps_every_servable_resolution` |
| Aliases one pass staged at one revision fold to the chain's end whatever their stored order (PL7) | 3 | `pl7_a_same_revision_chain_folds_to_its_end_in_either_order` |
| A pass's later insert threads through the records and aliases it staged earlier, before any row names their revision (PL2) | 3 | `pl2_dependent_inserts_in_one_pass_follow_the_staged_overlay` |
| A fetched index blob whose bytes differ from its descriptor's hash is loss, a stripped hidden section included (PL1) | 6 | `pl1_a_stripped_hidden_section_is_loss` |
| Content record from `index_document`, parity by construction (4.1, 7.5) | 4 | `s41_term_bytes_are_upstreams_stored_document`, `s42_document_cap_is_upstreams` |
| Per-record compression, nothing across records (G5) | 4, 5 | `g5_records_are_encoded_independently`, `g5_content_section_is_length_prefixed_records` |
| Each `SearchFilter` leaf answered: `Text` Standard/Exact/Prefix, `KeyValue` (4.1, 7.1) | 4 | `s71_standard_text_matches_terms_and_stems`, `s71_groups_and_across_tokens`, `s71_exact_phrase_uses_positions`, `s71_prefix_matches_verbatim_short_terms_only`, `s71_key_value_equals_contains_exists` |
| The phrase check agrees with `matches_phrase` and reduces to co-occurrence on `Language::None` fields (9.1) | 4 | `s71_exact_phrase_uses_positions`, `s71_phrase_on_a_language_none_field_is_co_occurrence` |
| Resolution is exhaustive: an unresolvable leaf refuses, never widens (7.1, invariant 51) | 4 (the error), 10 (the refusal) | `s71_integer_leaf_is_unresolvable`, `s71_resolve_keeps_operators_and_replaces_leaves` |
| Sort ranks as upstream's rule: pad to 40, sort, dense rank (7.1) | 4 | `s71_ranks_mirror_message_sort_ranks` |
| The padding formula just below, at and above each class boundary, `M − 5`, `M − 4`, `2M` (F1, R15) | 5 | `r15_padded_len_at_class_boundaries` |
| The maximal pass (`H` plus one message with `K − 1` aliases) stays under `M`; any tombstones, aliases or merges pad to `M` exactly (R20, R21, G1) | 5 | `r20_g1_maximal_pass_stays_under_m_and_every_pass_pads_to_m`, `r20_record_max_and_alias_len_are_the_encoded_maxima` |
| A compaction over equal record and tombstone counts pads to the same class whatever merged (decision 2 (c), I2) | 5 (formula), 15 (the rule) | `i2_compaction_allowance_is_a_function_of_counts` |
| The allowance carries the framing before rounding; a content over the allowance is a defect (R35) | 5 | `r35_allowance_exceeded_is_an_error` |
| P1-PAD: non-power-of-two publishing minimum hides aliases; over-minimum payloads refuse; sealed lengths agree | 5 | `publishing_non_power_of_two_minimum_hides_alias_count`, publishing boundary and sealed-blob cases |
| A blob sealed for one account, segment id, kind or predecessor does not open under another (3.1, 9.1) | 5 | `s31_a_section_opens_only_under_its_account_header_and_kind`, `s42_thread_blob_rejects_another_position` |
| The thread blob's length is the class plus a known multiple of the tombstone count (4.2) | 5 | `r9_h13_thread_blob_parses_with_and_without_a_hidden_section`, `s42_public_section_is_fixed_width` |
| A deletion-only pass's blob has no hidden section (H13) | 5 | `r9_h13_thread_blob_parses_with_and_without_a_hidden_section` |
| A content chain node names its predecessor in its authenticated header (R11) | 5, 6 | `r11_content_blob_names_its_predecessor`, `r11_chain_is_walked_from_the_head_descriptor` |
| A manifest with a bad tag is loss; a published and a rebuilt manifest reload with a verified tag computed at commit over the filled-in change id; an altered field fails it (H9, R33) | 5 | `h9_r33_tag_is_computed_at_commit_over_the_filled_change_id`, `h9_a_manifest_without_fill_keeps_its_change_ids` |
| A newer manifest format is "no map", an older one loss (F7, 5.4) | 5, 6 | `f7_newer_format_is_no_map_older_is_loss`, `h11_f7_g12_manifest_outcomes` |
| The manifest stays a few kilobytes at `ZA_SEARCH_SEGMENTS` thread blobs whatever the chain's length (R8) | 5 | `r8_manifest_stays_small_at_segments_entries` |
| The destroying marker carries the inventory and is read without a key (G12, R19) | 5 | `g12_destroying_marker_and_inventory_decode_without_a_key` |
| The codecs round-trip a populated index; the thread part loads without the content part (9.1) | 6 | `s43_round_trip_a_populated_index`, `r9_thread_load_fetches_no_content_blob` |
| A content chain reloads from its head with nothing but the manifest's head descriptor (R11) | 6 | `r11_chain_is_walked_from_the_head_descriptor` |
| An incremental reload of appended blobs and chain nodes, and a whole reload after a compaction, each end as a fresh load (F20) | 6 | `f20_incremental_reload_equals_a_fresh_load`, `f20_compaction_reloads_whole` |
| The manifest-hash comparison reloads on a deletion-only change with an equal identity version (R18) | 6 | `r18_a_deletion_only_change_moves_the_hash_and_reloads` |
| The identity version rejects a map of another epoch with an equal or lower revision (R2) | 6 (the value), 11 (the check) | `r2_identity_version_distinguishes_epochs` |
| The error classes map a store timeout to abort and an absent stub to skip; absence is loss only past the window (F3, H5, H6) | 6 (classes), 7 (stubs) | `f3_h6_error_classes` |
| A missing manifest is loss on any key account (H11) | 6 (class), 14 (rebuild) | `h11_f7_g12_manifest_outcomes` |
| A load whose transient would exceed the budget evicts first; content parts before thread parts (F18, G6) | 6 | `f18_budget_counts_the_transient_and_evicts_idle_content_first` |
| A thread-part load is never refused for budget alone (H8) | 6 | `h8_thread_parts_are_never_refused` |
| Loads are single-flight per account (G6) | 6 | `g6_loads_are_single_flight` |
| Eviction by idle and by invalidation (4.3) | 6 (unit), 8 (suite) | `s43_idle_sweep_and_invalidation_drop_entries` |
| The index is never swapped (F16, invariant 49) | 6 | `f16_never_swapped_is_pinned_by_the_type` |
| The record reader yields records whole with their change ids and reports truncation (9.1) | 7 | `records_are_whole_and_unfolded`, `the_truncation_marker_is_reported_and_the_lost_record_named` |
| A partial record resumed inclusively; a marker in its place is loss (R29, unit) | 7 | `resume_is_inclusive_for_a_partial_record_and_exclusive_for_a_complete_one`, `the_truncation_marker_is_reported_and_the_lost_record_named` |
| The `K` capture keeps In-Reply-To, then the last References first, before the sort (G10) | 7 | `the_capture_keeps_own_id_parent_then_nearest_references_before_any_sort`, `a_short_chain_keeps_everything_once_and_resent_last` |
| The error classes map a store timeout to abort and an absent stub to skip (F3, unit) | 7 | `open_errors_are_classified` |
| Two fixtures equal in public inputs stop at the same checkpoint (R21, unit) | 8 | `equal_public_inputs_stop_at_the_same_checkpoint_whatever_the_probe_merged` |
| A record of more inserts than the cap is left part-way, the checkpoint naming the record and the last id (R20, R1, unit) | 8 | `a_record_larger_than_the_cap_is_left_part_way` |
| A partial record resumed inclusively indexes its suffix and applies its deletions (R29) | 8 | `a_partial_record_finishes_then_applies_its_deletions` |
| The witness is a function of the indexed set's ids alone (R4, unit) | 8 | `the_witness_is_the_lowest_indexed_id_whatever_threaded` |
| A logged deletion is applied whatever a presence snapshot says (R17) | 8 | `a_logged_deletion_is_applied_unconditionally` |
| A consumed insert or deletion that staged nothing checkpoints; update-only is a no-op; cursor movement is durable (R22, R26, F4, G19, unit) | 8 | `the_decision_follows_what_changed` |
| Transient failures of one document reach `ZA_SEARCH_SKIP_AFTER` through the checkpointed count (R34, H5, unit) | 8 | `a_transient_open_aborts_with_the_attempt_count` |
| The cursor precedes the shortcut and the log (R10, unit form) | 8 | `cursor_items_come_first_and_take_the_epoch_as_creation` |
| The time cap stops at an insert boundary (G18, unit) | 8 | `the_time_cap_stops_at_an_insert_boundary_with_at_least_one_item` |
| The manifest is created at setup and no account without mail ever rebuilds (9.2) | 8 | `case_setup_manifest` |
| Persistence: a second session serves without opening a message blob; a thread-only load reads no content blob (R9) | 8 | `case_persistence_and_thread_only_load` |
| Catch-up after offline deliveries (9.2) | 8 | `case_catch_up_after_offline_deliveries` |
| The publishing pass's keyless writes are exactly one item update of the lowest indexed id, one Thread property change and every mailbox's container update (R4, 9.2; the bridge form is Task 18's) | 8 | `case_catch_up_after_offline_deliveries` |
| The pass caps crossed inside a grouped ingest's record with a restart between passes, every message indexed once (R1) | 8 | `case_multiappend_split_record_with_restart` |
| A hundred flag changes write no blob and no manifest; `scanned_to` keeps the shortcut cheap (F4) | 8 | `case_flag_changes_write_nothing` |
| Ten thousand flag changes write manifest-only checkpoints and never force a reconciliation (G19; 25 at a lowered threshold) | 8 | `case_update_only_checkpoints` |
| An unreadable logged insert checkpointed once, never reopened, the other mail indexed (R26) | 8 | `case_unreadable_insert_is_consumed_once` |
| A blob store that times out once aborts the pass with no index write, its attempt count checkpointed, and the next pass indexes (F3, R34) | 8 | `case_transient_blob_error_aborts_then_indexes` |
| A lost manifest assertion discards the loser's staging and serves the winner's (5.6) | 8 | `case_lost_assertion` |
| A compacted content blob staged to a slow store outlives the wait budget and is never purged (G15; the pass's blob form) | 8 | `case_slow_store_hold_outlives_the_wait` |
| A deletion-only pass writes no entry (R13; the rendering half is Task 13's) | 8 | `case_deletion_only_pass_writes_no_entry` |
| A repeated delta changes nothing (9.2) | 8 | `case_repeated_delta_changes_nothing` |
| A skipped message is absent from results and traced with ids only (9.2; the results half is Task 10's) | 8 | `case_unreadable_insert_is_consumed_once` |
| The pass runs before every keyed mail request, not on IDLE, NOOP, LOGOUT, POP3 or a keyless request; a keyed write runs one after its commit (5.2, 5.3, H20) | 9 | `case_triggers` |
| A flag update and a `Mailbox/get` answered while a pass is paused on its blob I/O; a request finding the pass running does not wait (F5) | 9 | `case_reads_do_not_wait_for_a_pass` |
| The wait budget (hook `SEARCH_PASS_PAUSE`) (9.2) | 9 | `case_content_wait_budget` |
| A pass that opens attachment-heavy mail stops at the time cap and the triggering command returns within it (G18) | 9 | `case_time_cap_bounds_the_command` |
| Non-key accounts take unchanged upstream paths (CLAUDE.md invariant) | 9 | `case_plain_control_unaffected` |
| 9.2 `search.rs` turns around: refused tables served, keyless cases kept, `nested_search_refused` stays | 10 | `mail::search::imap_served_content_keys`, `jmap_served_content_filters_and_sorts`, `keyless_search_refused`, `nested_search_refused` |
| 7.4 the unavailable text under a paused pass past the budget; a mixed served and refused key refused whole | 10 | `mail::search::unavailable_refusal` |
| 7.1 an unresolved leaf refuses, never widens (invariant 51; mutation: exhaustive resolution) | 10 | `an_unresolvable_leaf_refuses_instead_of_widening` |
| 7.1, 7.2 `sentBefore`/`sentAfter`, `SENTBEFORE/ON/SINCE`, `hasAttachment`, `KEYWORD $HasAttachment` from the index | 10 | `sent_and_attachment_conditions_read_the_index_not_the_cache`, `senton_is_a_day_window_over_the_index` |
| 7.1 `from`/`to`/`subject`/`sentAt` sorts as dense ranks | 10 | `sort_comparators_are_dense_ranks_over_the_result_set` |
| 7.4 "current" against the response's `E` (G14) | 10 | `current_means_scanned_past_the_end_with_content_at_indexed_to` |
| 9.2 `key.terms == 0`, `documents_are_ids_only()` (invariant 27) | 10 | `mail::search::visible_field_document` (unchanged) |
| 9.1 the state check rejects a counter below the newest row and passes an empty account (F2, G3) | 11 | `the_counter_check_rejects_a_cache_below_the_newest_row_and_passes_an_empty_account` |
| 9.2 the cross-node case (R2) | 11 | `mail::threads::case_r2_cross_node` |
| 9.2 the stale-state case (F2) | 11 | `mail::threads::case_f2_stale_state` |
| 9.2 a stale prefix `notFound` on `Email/get`, served on a keyless `Email/set` update and destroy (G20) | 11 | `mail::threads::case_g20_stale_prefix`, `check_id_accepts_the_current_prefix_only` |
| 9.2 a keyless token refused on every gated method, served on every keyless one (F8; the Bearer form) | 11 | `mail::threads::case_f8_keyless_table` |
| 9.2 a FETCH after a keyed APPEND carries the final `EMAILID`; `Email/set` create's response the mapped id (H21) | 11 | `mail::threads::case_h21_final_ids_after_keyed_writes` |
| P1-STATE: keyed write response map/state agree after mutations and cover acknowledged writes | 11 | `case_set_response_view_after_mutations`, `case_set_cross_node_publication_before_response`, `case_write_response_requires_own_change_id`, `case_import_response_state_includes_publication` |
| 9.2 a failed manifest read fails the thread-valued methods and never emits a singleton (R2, R3) | 11 | `mail::threads::case_failed_manifest_read` |
| 6.2 a thread-valued read intersects the map with the cache's present set (R14) | 11 | `thread_members_intersect_the_present_set` |
| 6.2 `EmailSubmission/get` returns the current `threadId` (G22) | 11 | `mail::threads::case_h21_final_ids_after_keyed_writes` (its submission step) |
| 9.2 the same conversation gives the same `Thread/get` groupings, `collapseThreads` results, IMAP `THREAD` | 12 | `mail::threads::case_conversation_parity` |
| 9.2 a reply delivered while no keyed request runs is a singleton until the next keyed request | 12 | `mail::threads::case_singleton_until_keyed_request` |
| 9.2 `Mailbox/get` thread counts match the control (G7) | 12 | `mail::threads::case_g7_mailbox_counts`, `counts_and_keyword_threads_group_by_the_map` |
| 9.2 more than `K` references thread through the nearest; the `K` deviation pinned (F1, 7.5) | 12 | `mail::threads::case_f1_k_bound` |
| 9.1 the stream over a history and a revision table gives upstream's pairs for states before, at, between and after two publications (R2, R5) | 13 | `a_first_assignment_from_the_singleton_renders_destroyed_then_created`, `two_merges_render_from_each_state` |
| 9.1 a deleted message's old mapped id (R3) | 13 | `a_deleted_members_old_mapped_id_is_destroyed` |
| 9.1 created and deleted within the range yields nothing (R14) | 13 | `created_and_deleted_within_the_range_yields_nothing` |
| 9.1 a deletion the record lacks but the range shows renders destroyed exactly once (R14, F10) | 13 | `a_deletion_the_record_lacks_but_the_range_shows_is_destroyed_once` |
| 9.1 a deletion between a pass's log read and its commit renders nothing at a state that saw it (H2) | 13 | `a_lagging_record_never_decides_presence_at_a_pinned_end` |
| 9.1 a deletion logged but not consumed, then a publication, then `/changes` from the state that saw it emits nothing (I3) | 13 | `an_unconsumed_deletion_before_s_emits_nothing` |
| 9.1 paging reproduces every item once with a publication and a deletion between pages (R12) | 13 | `paging_with_a_small_max_changes_covers_the_stream_once`, `mail::threads::case_r12_pages` |
| 9.1 the epoch rule serves below the epoch with no record in `(S, epoch]`, refuses with one, the boundary's entries included, never `from = 0` (R27, H1) | 13 | `the_epoch_refuses_only_when_the_methods_log_holds_a_record_in_s_epoch` |
| 9.1 `Initial` paged with `maxChanges = 1` over pre-existing updated messages equals its unpaged result (R28) | 13 | `the_initial_stream_is_every_present_document_in_id_order`, `mail::threads::case_r28_r31_initial` |
| 9.1 the alias chain resolves "the members of `T` as of `X`" across a deletion after `X` (R25, F14) | 13 | `thread_members_at_a_pinned_end_count_a_member_deleted_after_it` |
| 9.2 a merge renders in `Email/changes`, `Thread/changes` and `queryChanges` with `collapseThreads` as the control's | 13 | `mail::threads::case_merge_rendering_parity` |
| 9.2 two successive merges from states before, at, between and after (R5) | 13 | `mail::threads::case_r5_two_merges` |
| 9.2 the backlog case (R2) | 13 | `mail::threads::case_r2_backlog` |
| 9.2 pages of a large merge with a tiny `maxChanges`, a merge and a deletion between pages (R12) | 13 | `mail::threads::case_r12_pages` |
| 9.2 created, indexed and deleted within one range, with pages and a publication between; the insert reached and the deletion behind the cap (R14, F10) | 13 | `mail::threads::case_r14_f10_created_and_deleted` |
| 9.2 the backlog deletion, `T\|D` destroyed exactly once (R14) | 13 | `mail::threads::case_r14_backlog_deletion` |
| 9.2 the ordering case and its cap variant (R16) | 13 | `mail::threads::case_r16_ordering` |
| 9.2 a deletion-only pass writes no entry and renders the deletion, the last message included (R13) | 13 | `mail::threads::case_r13_deletion_only` |
| 9.2 initial pages stay equal after reload and truncation below E; later inserts are excluded; lost pinned-end evidence refuses continuation (R28, R31, P1-PAGING) | 13, 14, 15 | `case_r28_r31_initial`, `case_initial_pinned_end_lifecycle`, `case_initial_continuation_retention_race`, `case_initial_continuation_epoch_loss`, `case_initial_continuation_folded_alias` |
| 9.2 a pinned `Thread/changes` page across a last-member deletion and a later merge (R25) | 13 | `mail::threads::case_r25_pinned_thread_page` |
| 9.2 `Thread/changes` from a state the Thread log retains but the Email log does not is refused (G4) | 13 | `mail::threads::case_g4_two_bounds` |
| 9.2 an empty account on a warm cache with a trailing container-only record serves every `Email/*` method (G3) | 13 | `mail::threads::case_g3_empty_account` |
| 9.2 `Mailbox/changes` reports every mailbox's counts as `updatedProperties` only; `CHANGEDSINCE` silent on the reassigned message; the witness `FETCH (FLAGS ...)` with an unchanged MODSEQ (G7, R6, F15, pinned) | 13 | `mail::threads::case_g7_r6_f15_pinned` |
| A rebuild from a deleted blob, a deleted manifest and an older format moves the epoch past the latest issued state; `cannotCalculateChanges`; a resynced client works; a second loss gives a distinct boundary (R3, 9.2 index) | 14 | `mail::index::case_r3_rebuild_moves_the_epoch` |
| A manifest of a newer format gives "no map" and the unavailable refusal, no rebuild (F7) | 14 | `mail::index::case_f7_newer_format_is_no_map` |
| A rebuild of several passes with a restart between passes finishes its cursor (R10) | 14 | `mail::index::case_r10_cursor_finishes_across_restarts` |
| A cursor whose every remaining item vanished finishes through a manifest-only checkpoint, after eviction and restart, no blob, no log entry (R22) | 14 | `mail::index::case_r22_vanished_cursor_checkpoints` |
| A missing manifest on an empty account gets one and never rebuilds again (H11; 9.2 "the manifest is created at setup and no account without mail ever rebuilds") | 14 | `mail::index::case_h11_missing_manifest_on_an_empty_account` |
| Loss of a content blob is a content-only rebuild, identities unchanged (H23) | 14 | `mail::index::case_h23_content_loss_is_content_only` |
| Over the cap: threads served, searches refused, chain not extended; a raised cap takes a content-only rebuild that backfills, refusing until `content_to` catches up, identities unchanged (F6, R23) | 14 | `mail::index::case_f6_r23_over_the_cap_then_raised` |
| A reconciliation after a truncated log renders nothing a client saw (F9); a truncation during a pending rebuild (F9) | 14 | `mail::index::case_f9_reconciliation_renders_nothing_seen`, `case_f9_truncation_during_a_pending_rebuild` |
| A reconciliation whose difference includes a deletion still in the retained log renders the destroyed mapped id exactly once (R24, G2) | 14 | `mail::index::case_r24_g2_retained_deletion_rendered_once` |
| A loss after the last message was deleted: fresh resyncs round-trip empty, pre-loss states served when nothing of the kind was lost (R27) | 14 | `mail::index::case_r27_empty_rebuild_keeps_clients` |
| A loss on an account whose messages were all deleted while a client holds their mapped ids refuses that client's state (G3) | 14 | `mail::index::case_g3_loss_with_all_deleted_refuses_holders` |
| A node whose loaded map is of the previous epoch reloads or fails, never serves the old map (R2) | 14 | `mail::index::case_r2_old_epoch_map_never_served` |
| The rebuild form of the ordering case, `D` deleted through POP3 and `N2` mid-rebuild, grouped as the control, no admitted item writing more than `K − 1` aliases (test instrumentation, never a trace, PL8); its residual pinned (R16, R20, 7.5) | 14 | `mail::threads::case_r16_r20_rebuild_form` |
| Reconciliation order fixtures: a gap insert threads with a gap-deleted message (H3); a retained insert does not (I1) | 14 | `rebuild::tests::reconciliation_h3_gap_insert_threads_with_gap_deleted`, `reconciliation_i1_retained_insert_does_not_see_gap_deleted` |
| Compaction over equal-count fixtures pads to the same class; race-deleted records stay until their publication's row is pruned (decision 2 (c), I2) | 15 | `thread::compact_tests::equal_counts_equal_class`, `bridge_rule_keeps_a_deleted_bridge` |
| R35's timelines: dead aliases drop, records fit, resolution unchanged at every servable state | 15 | `compact_tests::r35_three_roots_one_bridge`, `r35_128_families_of_fifteen_roots` |
| A chain of three merges with the middle roots pruned keeps the aliases surviving records use; a folded alias moves one hop and renders the pre-merge thread before the next alias (R35) | 15 | `compact_tests::chain_keeps_the_aliases_surviving_records_use`, `fold_is_one_hop`, `two_expired_hops_fold_in_order` |
| Folding an alias leaves every resolution unchanged (5.5) | 15 | `compact_tests::chain_keeps_the_aliases_surviving_records_use` (the resolution loop) |
| The bridge rule without the dead-alias rule fails R35's fit; the dead-alias rule without the bridge rule fails the chain's resolution; a fold to the chain's end fails the pre-merge rendering (9.4 mutations) | 15 | `compact_tests::dead_alias_rule_needs_the_bridge_rule`, `fold_is_one_hop` |
| Thread compaction by a node holding no content, the manifest bounded, the chain growing (R8) | 15 | `mail::index::case_r8_thread_compaction_bounds_the_manifest` |
| Content compaction leaves one blob, old blobs unlinked and purged | 15 | `mail::index::case_content_compaction_leaves_one_blob` |
| Tiered merges during a rebuild: bytes rewritten against `⌈log_S n⌉`, chain length against `(S − 1)·⌈log_S n⌉ + 1`, the full merge after the cursor (G17, H7, R32) | 15 | `mail::index::case_g17_h7_r32_tiered_merges_during_a_rebuild`, `compact::tests::chain_bound_holds` |
| A deleted merged message, a thread compaction and a reload, then changes from a retained pre-delete state rendering the destroyed mapped id (R3, 9.2 threads) | 15 | `mail::threads::case_r3_deleted_merged_message_after_compaction` |
| The stale-snapshot deletion: tombstone persisted, after retention and compaction the record and content gone (R17) | 15 | `mail::index::case_r17_tombstone_gone_after_retention` |
| The amplified-merge probe: hidden class the same, one alias (R21, 9.2 threads) | 15 | `mail::threads::case_r21_amplified_merge_probe_class` |
| Every link and manifest transition interrupted, then restart, destroy, purge: no blob or link survives (R7) | 16 | `mail::index::case_r7_interrupted_transitions_leave_nothing` |
| A middle content blob deleted, a rebuild, a destroy, a purge: no older blob or link survives (R7) | 16 | `mail::index::case_r7_middle_node_break` |
| A destroy with an unparseable manifest, and one of a pre-release account without a manifest, take the scan and leave nothing (F23) | 16 | `mail::index::case_f23_unparseable_manifest`, `case_f23_pre_release_account_without_a_manifest` |
| A keyed pass paused after staging while destroy writes its marker, resumed, loses its assertion; the purge leaves nothing (R19) | 16 | `mail::index::case_r19_paused_pass_loses_to_the_marker` |
| A destroy that dies after writing the marker retries from the marker's inventory and leaves nothing (G12) | 16 | `mail::index::case_g12_destroy_dies_after_the_marker` |
| A codec key rotation recodes the index's blobs (4.2, 9.4) | 16 | `mail::index::case_blob_recode_recodes_the_index`, `index_task::test_blob_recode_keeps_sealed` |
| Upstream's per-account reindex task on a key account writes the marker and the next keyed request rebuilds, after a blob store that answered "not found" for every key had skipped the backlog (I6) | 16 | `mail::index::case_i6_reindex_resets_the_index` |
| A manifest recreated after destroy by a batch asserting `None` (R19, second form): the creating batch asserts the vault record | 14 (rule), 16 (pinned) | `rebuild.rs` step 3 of the first batch; `mail::index::case_r19_paused_pass_loses_to_the_marker` (the rebuild variant: start the parked request's rebuild instead of a pass) |
| The suite's destroy-then-purge-then-assert order verifies nothing remains (5.7) | 16 | `tests/src/za/mod.rs` end, `assert_is_empty` |
| 9.3: `query` variant asserts upstream's outcomes with a keyed pass in place of the queue wait, plus the keyless refusal | 17 | `jmap::za_variants::query` (calls `mail::query::query` and `query_options`; `keyless_query_refused`) |
| 9.3: `search_snippet` variant, upstream's snippets | 17 | `jmap::za_variants::search_snippet` (mirrors `search_snippet.rs:54-170`) |
| 9.3: `query_changes` variant, upstream's five queries, `from` served, the stored-id rewrite of `Move` a no-op (merge rendering is Task 13's, PL14) | 17 | `jmap::za_variants::query_changes` |
| 9.3: `thread_get` variant, one thread of five | 17 | `jmap::za_variants::thread_get` |
| 9.3: `thread_merge` variant, upstream's grouping on its corpora (6.1's superset pinned) | 17 | `jmap::za_variants::thread_merge` |
| 9.3: IMAP `search` and `thread` assert upstream's outcomes | 17 | `imap::search::test` and `imap::thread::test` in key mode (variants deleted) |
| 9.3: `websocket.rs` and `mail::gates` gain the keyless cases of 6.2's table: gated methods refused, keyless served, `StateChange` still arrives (F8, decision 1) | 17 | `mail::gates::keyless_tokens_hit_the_dispatcher_gate`; `websocket::keyless_socket_after_setup` |
| 9.4: every principal-linked blob is a sealed index blob (format byte, kind, no canary) | 18 | `leak::scan` rule; `leak::test_mail` (`index_blobs >= 1`, the planted-blob control) |
| 9.4: field 152 holds no canary and parses as a manifest | 18 | `leak::scan` rule; `leak::test_mail` (`manifests == 1`, the planted-field control) |
| 9.4: no `Threading` key, no term, ids-only documents (invariant 27) | 18 | `leak::scan` (existing rules), `mail::search::visible_field_document` kept (Task 10) |
| 9.4: a publishing pass's log holds one item update of the lowest indexed id, one Thread property change and every mailbox's container update, nothing for any other message (R4) | 18 | `leak::test_mail` log check; `leak_search::membership` |
| 9.4: after destroy and purge no blob and no permanent link remains | 18 | `za_tests`' end (`destroy_key_accounts`, `leak::purge_blobs`, `assert_is_empty`) |
| 9.4: the visible canaries searched in every blob and the manifest, with the plain account's blob-less state and a planted plaintext blob as negative control | 18 | `leak::scan`; `leak::test_mail` (`plainmail59` controls) |
| 9.4: the membership review case: two multi-message threads, a bridge with lower-id losing members, every keyless write enumerated, the writes, manifest and class equal to a bridge that merged nothing (R4, F1) | 18 | `leak_search::membership` (`keymail65` vs `keymail66`) |
| 9.4: the oracle attempt F1, a filler with a thousand references beside a probe, hidden class `M` | 18 | `leak_search::f1_filler_references` |
| 9.4: the oracle attempt G1, a thousand junk messages expunged beside a probe, public length exactly the tombstone multiple | 18 | `leak_search::g1_junk_expunged` |
| 9.4: the oracle attempt R21, two helper cohorts with a bridge, class `M` and one alias | 18 | `leak_search::r21_cohorts` |
| 9.4: a compaction after padding to a class boundary, the class equal to the public-count formula with and without the probe's merge (decision 2 (c)) | 18 | `leak_search::compaction_class_is_public_counts` (`keymail67` vs `keymail66`) |
| 9.4: a pad by content length must fail the equal-counts fixtures | 19 | mutation M29/M42 against `leak_search::compaction_class` and the unit class tests |
| 9.4: the bridge rule without the dead-alias rule must fail R35's fit | 19 | mutation M51 |
| 9.4: the dead-alias rule without the bridge rule must fail the chain fixture's resolution | 19 | mutation M50 |
| 9.4: a fold to the end of the chain must fail the pre-merge rendering | 19 | mutation M52 |
| G5: a chosen neighbour sharing a target's vocabulary does not shrink the other records' share of the content blob, its own frame subtracted (9.7 M41, PL15) | 18 | `mail::differential::case_g5` |
| 9.5: the differential test, every IMAP SEARCH and SORT form of `search.rs`'s tables, every JMAP filter, sort and `collapseThreads` combination of the `query` variant, `SearchSnippet/get` on each result, equal through the map except the pinned `K` deviation | 18 | `mail::differential::test` |
| 9.6: the pass rate with and without attachment text | 19 | `search_bench::system`, row 1 |
| 9.6: the compaction times | 19 | `search_bench::system`, rows 2 and 3 |
| 9.6: the cost of the validation read per request on RocksDB and on a networked store | 19 | `search_bench::system`, row 4 (networked: measured or pending) |
| 9.6: the cost of the mailbox property changes per publishing pass | 19 | `search_bench::system`, row 5 |
| 9.6: the first-search latency after an absence of one day's volume | 19 | `search_bench::system`, row 6 |
| 9.6: a design-size rebuild end to end: about 625 passes, the compactions, the rewritten bytes against `⌈log_S n⌉`, the refusals before the first success, the wall time (H7, R32) | 19 | `search_bench::system`, row 7 |
| 9.6: the figures confirm or adjust 4.4's defaults, `M` against `C · R` among them | 1, 19 | `search_bench::format` (Task 1) and the **Defaults** section of the outcome note |
| 9.7: every listed site | 19 | Step 2's table M1–M63, one row per site, each naming the mutant and the catcher |
| Review Focus 1: a message with no `Date`, an unparsable or an out-of-range one sorts and filters as the control's (PL10) | 8, 18 | `effective_sent_at_is_upstreams`; `mail::differential::test` (the no-`Date`, `Date: yesterday` and year-9999 messages) |
| Review Focus 2: a term set above `MAX_DOCUMENT_SIZE` is truncated as upstream's and indexed without an abort | 4, 18 | `s42_document_cap_is_upstreams`; `mail::differential::test` (the 1.2 MiB body) |
| Review Focus 3: `SEARCH CHARSET ISO-8859-1` and `SORT (REVERSE SUBJECT DATE)` equal the control's | 18 | `mail::differential::test` (the two IMAP forms) |
| Review Focus 4: an account invalidation between a pass's commit and its swap leaves no stale entry | 8 | `mail::index::case_invalidation_between_commit_and_swap` |
| Review Focus 5: no `Message-ID` and a whitespace subject is a singleton and never a candidate; duplicates share a root | 3, 18 | `s61_empty_threading_fields_are_a_singleton_and_never_a_candidate`, `s41_duplicate_message_ids_share_the_root`; `mail::differential::test` |
| Review Focus 6: a content search that times out leaves the passes' progress in the store | 9 | `mail::index::case_content_wait_budget` (its last assertion) |
| Plan revision 2, PL3: a publication by another node while a content query is parked completes and pairs results with state | 10 | `mail::search::view_publication_while_parked` |
| PL3: a local pass parked between its commit and its swap does not block or mismatch a concurrent acquisition | 10 | `mail::search::view_local_swap_while_acquiring` |
| PL3: a delivery between a query's state read and its manifest read keeps results and state from one moment | 10 | `mail::search::view_delivery_between_reads_keeps_results_and_state_paired` |
| PL4: a cap-sized unreadable prefix is consumed and the record's suffix follows; a partial checkpoint is never read whole | 8 | `a_cap_sized_unreadable_prefix_is_consumed_and_the_suffix_follows`, `scanned_to_never_covers_a_partial_checkpoint`; `mail::index::case_pl4_unreadable_prefix_of_a_group`; `case_multiappend_split_record_with_restart` (no delivery after the group) |
| PL4: a content-only rebuild runs on an idle account | 14 | `mail::index::case_h23_content_loss_is_content_only` (no delivery after the loss) |
| PL5: a content chain break leaves the thread blobs, the ids and the epoch intact across a purge and a restart | 14 | `mail::index::case_pl5_content_chain_break_keeps_the_thread_blobs` |
| PL6: unreadable gap and retained inserts are skipped once and the reconciliation finishes across restarts | 14 | `difference_resumes_gap_inserts_at_the_cursor`, `difference_resumes_retained_records_from_indexed_to`; `mail::index::case_pl6_unreadable_gap_and_retained_inserts_finish` |
| PL8: no admitted item writes more than `K − 1` aliases, observed without a trace; no trace carries an alias count | 14 | `mail::threads::case_r16_r20_rebuild_form` |
| PL9: top-level `inThread`, the `collapseThreads` total after a limit, the owner's thread-keyword sort and the attachment-keyword sort equal the control's | 10, 12 | `mail::threads::case_pl9_query_fast_paths`; `mail::search::jmap_served_content_filters_and_sorts` (the attachment-keyword sorts) |
| PL11: an evicted generation stays charged until its last reader drops it | 6 | `pl11_an_evicted_generation_stays_charged_until_its_last_reader_drops` |
| PL12: `ZA_SEARCH_SEGMENTS` is refused outside `2..=255` | 2 | `the_segment_count_fits_the_manifests_byte` |
| PL17: the attachment-free control indexes no attachment term and every body term | 1 | `search_bench::format` (the control's assertion) |
| Plan revision 3, FR1: an exhausted cursor clears whatever `end` is; a cap-stopped one advances; an empty list is a manifest-only bookkeeping pass | 8 | `the_decision_follows_what_changed`, `cursor_items_come_first_and_take_the_epoch_as_creation`; `mail::index::case_r22_vanished_cursor_checkpoints` |
| FS3 (FR2 reverted): attribution over every retained record; a retained insert skipped as loss is never re-opened | 14 | `difference_attributes_over_every_retained_record`; `mail::index::case_pl6_unreadable_gap_and_retained_inserts_finish` (exactly two skips) |
| FS1: the resume at the marker's own record reads every retained record; a second truncation is the bound's move, not the flag | 14 | `difference_resumes_retained_records_from_indexed_to` (`(40, 2, false) → None`), `second_truncation_is_detected_by_the_bound` |
| FS2, FS5: one step per completion; no cursor, no block; an abort moves no cursor; two pending completions clear independently | 8 | `the_decision_follows_what_changed`, `cursor_items_come_first_and_take_the_epoch_as_creation` (`s_abort`, `update_only.cursor`) |
| FS4: a truncation during a pending rebuild cursor is noticed on the cursor pass and folds the cursor (5.4) | 14 | `mail::index::case_f9_truncation_during_a_pending_rebuild` (`Some(1)`) |
| FS6: a content blob's loss met by a content request starts the content-only rebuild inside the wait | 9, 14 | `mail::index::case_h23_content_loss_is_content_only` |
| FS8: a flag-only record renders nothing in `Thread/changes`, as upstream's Thread log | 13 | `mail::threads::case_merge_rendering_parity` (flags set between the states); `thread_members_at_a_pinned_end_count_a_member_deleted_after_it` (its FT1 variant) |
| FT2, FU2, FU6: a content-cursor item stages a content record only; gap items take the marker's stamp; the gap deletes wait for the last gap insert; over the cap the content block suspends | 8, 14 | `content_sources_stage_content_only`, `gap_deletes_wait_for_the_last_gap_insert`; `content_cursor_items_run_newest_first_and_skip_content_present`, `reconciliation_h3_gap_insert_threads_with_gap_deleted`; `mail::index::case_h23_content_loss_is_content_only` (no thread blob, no log entry) |
| FU1: a due content-only rebuild disables the shortcut, so it starts on an idle account | 8, 14 | `mail::index::case_f6_r23_over_the_cap_then_raised` (the deletion-ending variant) |
| FV1: `Behind` never fires on a suspended account; no spurious start or chain drop while over the cap | 14 | `content_needs_rebuild_rules` (the two over-cap rows); `mail::index::case_f6_r23_over_the_cap_then_raised` (the suspended-phase assertions) |
| FV2: a bound move that also finishes the reconciliation writes no bound and does not panic | 8 | `gap_deletes_wait_for_the_last_gap_insert` (`s3`), `a_finishing_bound_move_writes_no_bound` |
| FV3: a suspended account with a pending content cursor writes nothing per request | 8 | `content_sources_stage_content_only` (`s_over_again`) |
| FV12: an over-cap account's content request refuses `OverCap` at once | 9 | `mail::index::case_f6_r23_over_the_cap_then_raised` (the text search answers within 2 s while suspended) |
| FW1, FW2: a pass never loads the content part; a pass that cannot run the backfill indexes everything else | 8, 9 | `content_sources_stage_content_only` (`s_not_resident`, `s_over_gap`); `mail::index::case_f6_r23_over_the_cap_then_raised` (the backfill runs on the text search) |
| FW5: a bound-move-only or finish-only reconciliation pass writes | 8 | `gap_deletes_wait_for_the_last_gap_insert` (`s3`, its `ManifestOnly` decision, Codex PL22) |
| FW3, FX1: the content cursor's items are keyed on the manifest; a reload to a manifest without a head drops the stale part | 6, 14 | `content_cursor_items_run_newest_first_and_skip_content_present` (its four rows), `fx1_a_reload_to_a_manifest_without_content_drops_the_part` |
| FX3: a content load racing a swap installs the current head; a swap never extends a part behind the base | 6, 8 | `fx3_a_load_racing_a_swap_installs_the_current_head` |
| FW4: an account over the cap and not yet suspended refuses `OverCap` at once | 9 | `mail::index::case_f6_r23_over_the_cap_then_raised` (the FW4 moment) |
| Codex PL20: a pending content cursor makes the index not current, whatever the checkpoints | 9 | `is_current_needs_no_pending_content_cursor` |
| FX2: a content loss met in the wait starts the content-only rebuild and the wait then runs it | 9, 14 | `mail::index::case_h23_content_loss_is_content_only` |
| E10: a raised cap backfills the suspended period and keeps the chain; only a content loss drops it (5.4, 9.2) | 8, 14 | `content_cursor_items_run_newest_first_and_skip_content_present`; `mail::index::case_f6_r23_over_the_cap_then_raised` (the chain kept, the opens counted); `mail::index::case_h23_content_loss_is_content_only` (the chain dropped) |
| Codex rev-7 PL11: admission and charging are atomic across accounts | 6 | `pl11_concurrent_growth_is_admitted_atomically` |
| FT5: a bound move that lost nothing rewrites the marker's bound; one that did restarts | 14 | `second_truncation_is_detected_by_the_bound` |
| Codex re-review PL3, D13: a Thread/get acquired before a merge committed pairs the old grouping with a state below the merge, same-node and remote | 13 | `mail::threads::case_d13_thread_state_pairs_with_its_map` |
| Codex re-review PL11: a part that grows through local passes stays charged at its size | 6 | `pl11_growth_through_local_passes_is_charged` |
| Codex re-review PL19: the benchmark's term bytes are the raw encoding, never the stored form | 1 | `za_bench_term_bytes_counts_positions` (the above-watermark assertion) |
| Codex rev-6 PL11: resident content growth is admitted against the process budget; refused, the part is dropped and progress kept | 6, 8 | `pl11_growth_across_accounts_is_admitted_against_the_process_budget` |
| FR7: a message the cache holds and the index has tombstoned is a member under the same state `Email/get` lists it | 11 | `thread_members_intersect_the_present_set` (second half) |

---

### Task 1: Format benchmark

**Files:**
- Create: `tests/src/za/search_bench.rs`
- Modify: `tests/src/za/mod.rs:16-51` (`pub mod search_bench;` in the module list; nothing added to `za_tests`)
- Modify: `crates/store/src/search/mod.rs:176-184` (`IndexDocument::za_bench_term_bytes` and `clone_for_test`, test mode; upstream's code is called, not changed), `crates/store/src/search/codec.rs:89-102` (`Writer::into_raw_bytes`, test mode, beside `into_document`; Codex rev-6 PL19)
- Create: `docs/superpowers/plans/2026-11-07-zero-access-plan16-outcome.md` (the note's header and `## Format benchmark (Task 1)`)
- Test: the `#[ignore]`d `za::search_bench::format`; `crates/store/src/search/index_za_tests.rs` (the accessor's unit test)

**Interfaces:**
- Consumes: `TestServerBuilder::new(name).await.with_default_listeners().await.build().await` (`tests/src/za/cache_swap.rs:189-196`); `create_key_user_account` (`tests/src/utils/account.rs:183`) and `za_setup_token`/`za_setup` (`tests/src/utils/za.rs:277, 284`); `Server::email_ingest(IngestEmail { raw_message, message, blob_hash: None, access_token, mailbox_ids: vec![INBOX_ID], keywords: vec![], received_at: None, source: IngestSource::Smtp { deliver_to, envelope_from, sender_authentication: SenderAuthentication::DmarcPass, is_spam: false }, metadata: None, session_id: 0 })` (`tests/src/za/mail/fence.rs:1631-1648`); `za_unlock(&test, account_id, password) -> Arc<SessionKeys>` (`tests/src/utils/za.rs:408`); `ZaMailView::za_open_metadata_row(&self, stored: &MetadataRow, keys: Option<&SessionKeys>, account_id, document_id) -> trc::Result<Option<MetadataRow>>` and `za_open_message_blob(&self, stored_is_stub: bool, keys, account_id, document_id, bytes: Vec<u8>) -> Zeroizing<Vec<u8>>` (`crates/email/src/message/za/view.rs:95-118`; the stored row and the blob read as `leak::test_mail` reads a key account's message, `tests/src/za/leak.rs`, with `za_open_stub_row` at `view.rs:364`); `MetadataRow::index_document(account_id, document_id, &headers, blob, index_fields, default_language, &extract_limits) -> IndexDocument` (`crates/email/src/message/index/search.rs:43-52`) called as `email_document` calls it (`crates/services/src/index_queue/document.rs:81-102`: `core.email.index_fields.get(&SearchIndex::Email)`, `row.raw_headers()`, `row.unarchive()`, `core.email.default_language`, `core.email.extract_limits`); `MessageSortKeys::from_envelope(EnvelopeView)` (`crates/email/src/message/sortkeys.rs:471`); `za_has_attachment(&ArchivedMessageMetadata)` (`view.rs:411`); `ThreadFields::scan(&Message)` (`crates/email/src/message/thread.rs:30-64`, `message_ids: Vec<u128>` sorted and deduped); `store::write::compress::compress(dictionary: Option<Dictionary>, input: &[u8], trailer_len: usize) -> io::Result<Vec<u8>>` and `decompress_into` (`crates/store/src/write/compress.rs:145`, `Dictionary::{Email, Term}` at `:13-19`); `mail_builder::MessageBuilder` (`tests/Cargo.toml:53`).
- Produces: `#[cfg(feature = "test_mode")] impl IndexDocument { pub fn za_bench_term_bytes(self) -> (Vec<u8>, Vec<u8>); pub fn clone_for_test(&self) -> IndexDocument }` (with positions, without positions; a field-wise clone, `IndexDocument` deriving no `Clone`) and `#[cfg(feature = "test_mode")] impl codec::Writer { pub(crate) fn into_raw_bytes(self) -> Vec<u8> }`; the outcome note's table under `## Format benchmark (Task 1)` with the three decisions Tasks 4 and 5 read: **positions** (kept, PL13; the size figure and, when it is at or above the cap, a question for Jay), **dictionary** (upstream's `Dictionary::Term`, confirmed, with the measured ratios), **defaults** (4.4 confirmed, or the adjusted values).

- [ ] **Step 1: Write the accessor's failing unit test**

In `crates/store/src/search/index_za_tests.rs` (the file exists; add beside its tests):

```rust
#[test]
fn za_bench_term_bytes_counts_positions() {
    let mut document = IndexDocument::new(SearchIndex::Email)
        .with_account_id(1)
        .with_document_id(7);
    document.index_text(EmailSearchField::Subject, "quarterly budget review", Language::English);
    document.index_text(EmailSearchField::Body, "the budget review is on thursday the review is long", Language::English);
    let (with, without) = document.za_bench_term_bytes();
    assert!(!without.is_empty());
    assert!(with.len() > without.len(), "positions add bytes: {} vs {}", with.len(), without.len());
    // Codex re-review PL19: the bytes are the raw encoding, not into_document's stored form: a body of 4,000 distinct words
    // (above COMPRESS_WATERMARK_DICTIONARY) yields `with` longer than `Writer::into_document()` of the same Document, whose
    // last byte is the storage marker, and `with` carries no marker.
    let mut big = IndexDocument::new(SearchIndex::Email).with_account_id(1).with_document_id(8);
    big.index_text(EmailSearchField::Body, &(0..4000).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" "), Language::English);
    let stored = AccountIndexer::default().index_document(big.clone_for_test(), 8).encode().into_document();
    let (raw, _) = big.za_bench_term_bytes();
    assert!(raw.len() > stored.len(), "the stored form is compressed: {} vs raw {}", stored.len(), raw.len());
}
```

Use the field-setting methods `IndexDocument` actually exposes (read `crates/store/src/search/document.rs` for the text and key-value setters) and adjust the two `index_text` lines to them; the assertions stay.

- [ ] **Step 2: Run the test and see it fail**

Run: `cargo test -p store --lib --features test_mode za_bench_term_bytes_counts_positions`
Expected: compile error, `no method named za_bench_term_bytes`.

- [ ] **Step 3: Implement the accessor**

The tokenization lives in `AccountIndexer::index_document` (`crates/store/src/search/term.rs:100-135`), which turns an `IndexDocument`'s fields into the forward-index `Document` (tokenization, term admission, the position lists) and is `pub(crate)`, so the accessor calls it as it is; `AccountIndexer::build_batch` (`:276`) only emits postings from what the indexer collected (PL19). Nothing in `term.rs` changes. Add in `crates/store/src/search/mod.rs`:

```rust
#[cfg(feature = "test_mode")]
impl IndexDocument {
    /// Plan 16 Task 1: the forward-index bytes of this document as
    /// `Document::encode` writes them, with and without the position lists.
    pub fn za_bench_term_bytes(self) -> (Vec<u8>, Vec<u8>) { .. }
}
```

`za_bench_term_bytes` runs `let mut document = AccountIndexer::default().index_document(self, 0);` (the test's `clone_for_test` is a `#[cfg(feature = "test_mode")]` clone of `IndexDocument`, which derives no `Clone`), encodes it with `Document::encode` (`term.rs:629-647`, which writes the term list, then the per-field position lists), empties the position lists (its `fields` map) and encodes again; it returns the two writers' raw buffers through a test-only accessor Task 1 adds beside `into_document`, `#[cfg(feature = "test_mode")] pub(crate) fn into_raw_bytes(self) -> Vec<u8>` on `codec::Writer` (`crates/store/src/search/codec.rs:89-102`), never `into_document`, which above `COMPRESS_WATERMARK_DICTIONARY` compresses the buffer under `Dictionary::Term` and always appends a storage marker byte, so a benchmark over its output would recompress stored bytes and measure nothing (Codex re-review PL19).

- [ ] **Step 4: Run the unit test and the store suite**

Run: `cargo test -p store --lib --features test_mode za_bench_term_bytes_counts_positions` then `cargo test -p store --lib --features test_mode search::`
Expected: PASS; every existing search test passes (nothing of upstream's changed).

- [ ] **Step 5: Write the corpus generator**

In `tests/src/za/search_bench.rs`:

```rust
pub struct Generated { pub raw: Vec<u8>, pub attachment_bytes: usize, pub references: usize }

/// Deterministic (xorshift64 from `seed`), about 70 KB a message on
/// average with the attachment shares below, so that `count` = 15,000 is about 1 GB.
pub fn corpus(seed: u64, count: usize) -> Vec<Generated>
```

Shape, per message `i` (fix these proportions; they are the design size of 1.3): a `Message-ID` `<bench-{i}@example.com>`; 60% are replies into an existing thread (`In-Reply-To` the parent's id and `References` the parent's chain plus the parent, so chains grow to 40 and about 5% of messages carry more than `K = 16` references), 10% are forwards (`Fwd: ` subject, no `References`), 30% start threads; 10% of subjects carry non-ASCII words (`Réunion`, `会議`, `Überprüfung`); 2% repeat an earlier `Message-ID` (duplicates); bodies are 1 to 8 KB of words drawn from a 5,000-word list by a Zipf-like rule (rank `r` with weight `1/r`), with one in twenty sentences repeated across messages (phrases); 25% carry a `text/plain` attachment `notes-{i}.txt` of 40 to 200 KB and 5% one of 500 KB to 1 MB, both of the same word rule; `Date` advances one to ninety seconds per message from 2025-01-01; `From`/`To` from a pool of 200 addresses. Build with `mail_builder::MessageBuilder` (`.header("Message-ID", ..)`, `.header("In-Reply-To", ..)`, `.header("References", ..)`, `.text_body`, `.attachment("text/plain", name, bytes)`, `.write_to_vec()`).

- [ ] **Step 6: Write the benchmark**

In the same file:

```rust
#[tokio::test(flavor = "multi_thread")]
#[ignore]
pub async fn format() { .. }
```

Procedure, printing each figure as it is measured and keeping it for the table: (1) build a standalone server named `search_bench` (the cache_swap shape; default listeners), create the key account `search-bench@example.com` with password `bench-secret`, provision it (setup token, setup); (2) ingest `corpus(1, env SEARCH_BENCH_MESSAGES or 15_000)` in process through `email_ingest` with `deliver_to = "search-bench@example.com"`, `envelope_from = "bench@remote.test"`, timing the whole ingest; (3) `za_unlock` the account; for every document id of the account read the stored row and blob, open both with the keys, and compute: `index_document` with the server's `index_fields`, `default_language` and `extract_limits`; the same with `index_fields` lacking `EmailSearchField::Attachment` (PL17: the attachments are `text/plain`, which `index/search.rs:75-89` indexes through `index_part_text` under the `Attachment` field, gated by `is_enabled` alone, so zeroing `ExtractLimits`, which governs binary extraction, would not remove them), asserting on the first message that carries one that a word only its attachment holds is among the with-attachment document's terms and absent from the control's, while a body word is in both; `za_bench_term_bytes()` of the with-attachment document (it consumes the document, so build that document once more for this call); `compress(None, ..)`, `compress(Some(Dictionary::Term), ..)` and `compress(Some(Dictionary::Email), ..)` of the with-positions bytes, trailer 0; `MessageSortKeys::from_envelope`, `za_has_attachment` and `ThreadFields::scan` (record `message_ids.len()` and `min(16, len) · 16 + 48` as the record's encoded size at Task 2's layout); (4) one frame over the concatenation of every with-positions byte string, `compress(None, all, 0)`; (5) the decompression time of every per-record frame in sequence (`decompress_into`); (6) the indexing rate: documents per second of step 3's open-and-index loop with attachment text and without. Then print the table of Step 7 and shut the server down (`test.shutdown()`, delete the temp dir as `cache_swap_tests` does).

- [ ] **Step 7: Run the benchmark and record the table**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::search_bench::format -- --exact --ignored --nocapture` (budget an hour; set `SEARCH_BENCH_MESSAGES=2000` for a smoke run first).
Expected: the test passes and prints the table. Create `docs/superpowers/plans/2026-11-07-zero-access-plan16-outcome.md` with the header line (`# Zero-access search and threads: plan 16 outcome`, the date, the plan and spec paths, spec revision 23) and `## Format benchmark (Task 1)` holding the machine (CPU, memory, store), the corpus (count, raw bytes, ingest seconds), and this table with the measured values:

| Figure | With positions | Without positions |
|---|---|---|
| content term bytes, total / mean / p95 per message | | |
| per-record zstd, no dictionary, total | | |
| per-record zstd, `Dictionary::Term`, total | | |
| per-record zstd, `Dictionary::Email`, total | | |
| one frame over all records, total (the G5 cost is the ratio to the row above it) | | |
| projected content part at 30,000 messages (mean × 30,000) | | |
| decompression of every record, seconds | | |

and below it: thread record encoded size mean / max (must be ≤ `R = 304`), messages with more than `K = 16` reference hashes (count and share), indexing rate with and without attachment text (documents per second), and the three decisions by these rules: positions are **kept** whatever the figure (spec 4.1: the phrase check uses the positions upstream keeps, so dropping them would change what a phrase search matches, `alpha gamma` matching `alpha beta gamma`, against the approved semantics and Task 4's phrase test; PL13); the without-positions column is recorded as a comparison only, and if the projected content part with positions at 30,000 messages is at or above the current `ZA_SEARCH_ACCOUNT_MAX_MB` in MiB the note states it as a question for Jay (raise the cap, or a separate spec change), and Task 3 still starts; the **dictionary** is upstream's `Dictionary::Term` by construction (Task 4 encodes upstream's stored document, plan call B2); the measured ratios are recorded, and a saving above 20% from `Dictionary::Email` is noted for Jay as a possible follow-up, not adopted; the **defaults** of 4.4 stand unless a figure contradicts them, which the note states as a question for Jay. If the maximum record size exceeds `R = RECORD_MAX = 304` (Task 2's derived constant, spec revision 19), stop and report: the derivation of `R` in Task 2 is wrong, and Task 3 must not start (PL19).

- [ ] **Step 8: Commit**

```bash
git add tests/src/za/search_bench.rs tests/src/za/mod.rs crates/store/src/search/mod.rs crates/store/src/search/codec.rs crates/store/src/search/index_za_tests.rs docs/superpowers/plans/2026-11-07-zero-access-plan16-outcome.md
git commit -m "Search plan 16 Task 1: format benchmark and its figures"
```

Report the table to Jay with the three decisions before Task 3 starts.

### Task 2: Key, field, configuration, constants and hooks

**Files:**
- Modify: `crates/vault/src/keys.rs:23-29` (`LABEL_SEARCH`), `:155-157` (`derive_search_key` after `derive_mail_key`); `crates/vault/src/session.rs:33-40` (the `swk` field), `:167-182` (`new`), `:192-194` (`swk()` after `mwk()`), `:600-606` (the test beside `session_keys_carry_the_mail_key`)
- Modify: `crates/types/src/field.rs:92-95` (`ZeroAccessSearch = 152`), `:209-210` (the mapping)
- Create: `crates/store/src/za_search/mod.rs` (constants, `reservation`, `padded_len`, `compaction_allowance`, the record layout comment); modify `crates/store/src/lib.rs` (`pub mod za_search;`)
- Create: `crates/common/src/za/search/mod.rs`, `config.rs`, `trace.rs`, `cache.rs` (the skeleton `ZaSearchCache { config, refusal }`; Task 6 extends it); modify `crates/common/src/za/mod.rs:14-20` (`pub mod search;`) and `:31` (`ZA_MAIL_SEARCH_UNAVAILABLE` beside `ZA_MAIL_SEARCH_REFUSED`)
- Modify: `crates/common/src/lib.rs` (`pub za_search: ZaSearchCache` on `Caches`, beside `za_keys`); `crates/common/src/config/inner.rs:140` (`za_search: ZaSearchCache::new(ZaSearchConfig::from_env())`); `crates/common/src/manager/boot.rs:199-210` (log the effective values); `crates/main/src/main.rs:64-72` (the refusal exit)
- Modify: `crates/common/src/za/test_hooks.rs` (the statics, setters, pause points, take functions and counters of Global Constraints, in the file's style at `:159-217`, `:241-376`, `:446-462`, `:515-566`)
- Modify: `tests/src/utils/cleanup.rs:409-420` (field 152 in the Properties exemption) and the `BlobLink` arm (`Document` links on `Collection::Principal` document 0 exempt while the account exists)
- Test: `crates/vault/src/session.rs` (unit), `crates/store/src/za_search/mod.rs` (unit), `crates/common/src/za/search/config.rs` (unit), `crates/common/src/za/search/trace.rs` (unit)

**Interfaces:**
- Consumes: `hkdf_expand(ikm: &[u8], label: &[u8]) -> Secret` (`keys.rs:117`), `Secret` (`keys.rs:32-34`, `Zeroizing<[u8; 32]>` with a redacting `Debug`), `SessionKeys::new(account_id: u32, generation: u64, mk: Secret) -> Self` (`session.rs:167`); `KeyCacheConfig::from_env`'s `var` helper shape (`crates/vault/src/cache.rs:41-53`) and `lease_secs_from`'s trim-and-parse (`crates/common/src/za/lease.rs:55-60`); `Pause` (`crates/store/src/za_test.rs:52-66`, re-exported by `test_hooks.rs:137`), `wait(take_for(&STATIC, account_id)).await` (`test_hooks.rs:454-456`); `verify_schema_version`'s refusal pattern (`crates/main/src/main.rs:64-72`: `eprintln!`, `ServerEvent::StartupError`, `ExitCode::FAILURE`).
- Produces (used by every later task):
  - `vault::keys::derive_search_key(mk: &Secret) -> Secret` (`LABEL_SEARCH = b"za/v1/search"`); `SessionKeys::swk(&self) -> &Secret`.
  - `types::field::PrincipalField::ZeroAccessSearch` (= 152).
  - `store::za_search::{SEARCH_FORMAT_V1: u8 = 1, K: usize = 16, RECORD_MAX: usize = 304, ALIAS_LEN: usize = 15, TOMBSTONE_LEN: usize = 12, REVISION_ROW_LEN: usize = 12}`, `pub const fn reservation() -> usize` (529), `pub fn padded_len(framed_len: usize, min_class: usize) -> usize`, `pub fn compaction_allowance(n_records: usize) -> usize`.
  - `common::za::search::config::ZaSearchConfig { pub mem_mb: u64, pub thread_mem_mb: u64, pub account_max_mb: u64, pub idle_secs: u64, pub pass_cap: usize, pub pass_mb: u64, pub pass_hidden_kb: usize, pub pass_ms: u64, pub wait_secs: u64, pub segments: usize, pub checkpoint: usize, pub skip_after: u8, pub absent_retry_secs: u64, pub pad_min: usize }` with `ZaSearchConfig::from_env() -> Result<Self, String>`, `ZaSearchConfig::from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, String>` (what `from_env` calls with `std::env::var(name).ok()`), `ZaSearchConfig::hidden_budget(&self) -> usize` (`pass_hidden_kb · 1024`) and `impl Default` (the 4.4 defaults).
  - `common::za::search::cache::ZaSearchCache::new(config: Result<ZaSearchConfig, String>) -> Self`, `config(&self) -> &ZaSearchConfig` (the defaults when refused), `refusal(&self) -> Option<&str>`; `Caches::za_search`.
  - `common::za::search::trace::{ZA_SEARCH_PASS, ZA_SEARCH_SKIPPED, ZA_SEARCH_ABORTED, ZA_SEARCH_LOST, ZA_SEARCH_REBUILD, ZA_SEARCH_SUSPENDED, ZA_SEARCH_UNAVAILABLE, ZA_SEARCH_EVICTED, ZA_SEARCH_UNRESOLVED, ZA_SEARCH_OVER_BUDGET}` with the strings of Global Constraints; `common::za::ZA_MAIL_SEARCH_UNAVAILABLE`.
  - `common::za::test_hooks::{SEARCH_COMMIT_PAUSE, SEARCH_SWAP_PAUSE, SEARCH_SNAPSHOT_PAUSE, SEARCH_VIEW_PAUSE, SEARCH_SWEEP_PAUSE, SEARCH_PASS_PAUSE, SEARCH_DESTROY_PAUSE}` with `set_search_<name>_pause(Option<Arc<Pause>>)` and `async fn search_<name>_pause_point(account_id: u32)`; `SEARCH_HOLD_EXPIRE`, `SEARCH_BLOB_FAIL_ONCE`, `SEARCH_OPEN_FAIL_ONCE` with `set_search_<name>_once(Option<u32>)` and `take_search_<name>(account_id: u32) -> bool` (`SEARCH_HOLD_EXPIRE` is read, not taken: `search_hold_expire(account_id) -> bool`); `SEARCH_BLOB_SLOW` with `set_search_blob_slow(Option<(u32, u64)>)` and `search_blob_slow(account_id) -> Option<Duration>`; `za_search_blob_reads(account_id) -> u64`, `za_search_message_opens(account_id) -> u64`, `count_za_search_blob_read(account_id)`, `count_za_search_message_open(account_id)`, `za_search_max_item_aliases(account_id) -> usize` with `note_za_search_item_aliases(account_id, n: usize)` (keeps the maximum, PL8), `reset_za_search_counters(account_id)` (a `Mutex<HashMap<u32, (u64, u64, usize)>>`). The flags `SEARCH_CACHE_HOLD`, `SEARCH_SWAP_SKIP`, `SEARCH_OPEN_FAIL_ALL` (`Mutex<Option<u32>>`, `set_search_<name>(Option<u32>)`, `search_<name>(account_id: u32) -> bool`); the one-shots `SEARCH_MANIFEST_FAIL_ONCE` and `SEARCH_DESTROY_FAIL_ONCE` (`set_search_<name>_once`, `take_search_<name>`); `SEARCH_CONFIG_OVERRIDE: Mutex<Option<ZaSearchConfig>>` with `set_search_config_override(Option<ZaSearchConfig>)` and `Server::za_search_config(&self) -> ZaSearchConfig` (`common::za::search::cache`: the override when set in test mode, else the boot value); and in `trace.rs` `macro_rules! za_search_trace` plus `pub const ZA_SEARCH_EVENT_TYPE: trc::EventType` (plan calls C6, D3, D4; the pause points, flag reads and one-shot takes are placed by the tasks that use them).

- [ ] **Step 1: Write the failing vault test**

In `crates/vault/src/session.rs`'s test module, after `session_keys_carry_the_mail_key` (`:600-606`):

```rust
#[test]
fn session_keys_carry_the_search_key_and_it_survives_a_rewrap() {
    let mk = Secret::random();
    let keys = SessionKeys::new(7, 3, mk.clone());
    assert_eq!(keys.swk().as_bytes(), derive_search_key(&mk).as_bytes());
    // A password change, a recovery or an app password rewraps mk under a
    // new generation; the derived key does not move (spec 3.1).
    let rewrapped = SessionKeys::new(7, 4, mk.clone());
    assert_eq!(rewrapped.swk().as_bytes(), keys.swk().as_bytes());
    assert_ne!(keys.swk().as_bytes(), keys.mwk().as_bytes());
    assert_ne!(keys.swk().as_bytes(), keys.ewk().as_bytes());
    let other = SessionKeys::new(7, 3, Secret::random());
    assert_ne!(other.swk().as_bytes(), keys.swk().as_bytes());
    assert_eq!(format!("{keys:?}"), "SessionKeys(account 7, generation 3)");
}
```

- [ ] **Step 2: Run it and see it fail**

Run: `cargo test -p vault --features test_mode session_keys_carry_the_search_key`
Expected: compile error, `no method named swk` / `cannot find function derive_search_key`.

- [ ] **Step 3: Implement the search key**

In `crates/vault/src/keys.rs`: `const LABEL_SEARCH: &[u8] = b"za/v1/search";` after `LABEL_MAIL` (`:29`), and after `derive_mail_key` (`:155-157`):

```rust
/// The search key (search spec 3.1): seals the index's blobs and keys the
/// manifest's tag. Derived from mk, so a rewrap does not move it.
pub fn derive_search_key(mk: &Secret) -> Secret {
    hkdf_expand(mk.as_bytes(), LABEL_SEARCH)
}
```

In `crates/vault/src/session.rs`: a `swk: Secret` field beside `mwk` (`:39`), derived in `new` (`:169`), and `pub fn swk(&self) -> &Secret` after `mwk()` (`:192-194`). `Debug` is unchanged (it prints the account and generation only).

- [ ] **Step 4: Run the vault tests**

Run: `cargo test -p vault --features test_mode`
Expected: PASS, the new test included.

- [ ] **Step 5: Add field 152**

In `crates/types/src/field.rs` after `ZeroAccessPins = 151` (`:95`):

```rust
    /// Zero-access search manifest (search spec 4.2). 152 has never been
    /// assigned to any field in upstream's history.
    ZeroAccessSearch = 152,
```

and `PrincipalField::ZeroAccessSearch => 152,` after `:210`. Grep the crate for every match on `ZeroAccessPins` (`crates/types/src/field.rs` only; the other sites, `crates/common/src/auth/vault.rs`, `crates/vault/src/pins.rs`, `crates/groupware/src/calendar/sharing/convert.rs`, `tests/src/utils/{cleanup,za}.rs`, use the variant and need no arm) and add the new variant wherever the enum is matched exhaustively.

Run: `cargo check -p types` — Expected: clean.

- [ ] **Step 6: Write the failing constants tests**

Create `crates/store/src/za_search/mod.rs` with `pub mod` nothing yet and a `#[cfg(test)] mod tests`:

```rust
#[test]
fn the_record_bound_and_the_reservation_are_the_specs() {
    assert_eq!(K, 16);
    assert_eq!(RECORD_MAX, 304);
    assert_eq!(ALIAS_LEN, 15);
    assert_eq!(TOMBSTONE_LEN, 12);
    assert_eq!(REVISION_ROW_LEN, 12);
    assert_eq!(reservation(), 529);
}

#[test]
fn padding_rounds_to_a_class_above_the_minimum() {
    const M: usize = 65_536;
    assert_eq!(padded_len(1, M), M);
    assert_eq!(padded_len(33_301, M), M);          // H + R + (K − 1)·a + 4 at the defaults
    assert_eq!(padded_len(M - 1, M), M);           // a payload of M − 5 bytes
    assert_eq!(padded_len(M, M), M);               // a payload of M − 4 bytes
    assert_eq!(padded_len(M + 1, M), 2 * M);
    assert_eq!(padded_len(2 * M, M), 2 * M);
    assert_eq!(padded_len(2 * M + 1, M), 4 * M);
    assert_eq!(padded_len(100, 4096), 4096);
}

#[test]
fn the_compaction_allowance_is_a_function_of_the_record_count() {
    assert_eq!(compaction_allowance(0), 4);
    assert_eq!(compaction_allowance(1), RECORD_MAX + ALIAS_LEN + 4);
    assert_eq!(compaction_allowance(30_000), 30_000 * 319 + 4);
    // 30,000 messages: a 16 MiB class (spec 5.5, decision 2 (c)).
    assert_eq!(padded_len(compaction_allowance(30_000), 65_536), 16 * 1024 * 1024);
    // Equal counts, equal classes, whatever merged.
    assert_eq!(padded_len(compaction_allowance(12_345), 65_536), padded_len(compaction_allowance(12_345), 65_536));
}
```

Add `pub mod za_search;` to `crates/store/src/lib.rs`.

- [ ] **Step 7: Run them and see them fail**

Run: `cargo test -p store --lib --features test_mode za_search`
Expected: compile errors for the missing items.

- [ ] **Step 8: Implement the constants**

In `crates/store/src/za_search/mod.rs`, with the SPDX header and a module comment naming the spec (4.2), the record layout that fixes the bounds, as a comment the encoders of Tasks 3 and 5 follow:

```
thread record  = u16 len | u8 flags | u32 document_id | u32 thread_id | u32 revision
               | u64 created | u64 deleted (0 = none) | u128 subject_hash | u8 n | n × u128 hash
               = 48 + 16·n bytes, n ≤ K  →  RECORD_MAX = 304
alias          = u16 len | u8 kind | u32 source | u32 target | u32 revision
               = 15 bytes  →  ALIAS_LEN
tombstone      = u32 document_id | u64 deleted  = 12  →  TOMBSTONE_LEN
revision row   = u32 revision | u64 change_id   = 12  →  REVISION_ROW_LEN
```

```rust
pub const SEARCH_FORMAT_V1: u8 = 1;
pub const K: usize = 16;
pub const RECORD_MAX: usize = 48 + 16 * K;
pub const ALIAS_LEN: usize = 15;
pub const TOMBSTONE_LEN: usize = 12;
pub const REVISION_ROW_LEN: usize = 12;

/// One admitted item's charge against the hidden budget (spec 4.2, R21).
pub const fn reservation() -> usize { RECORD_MAX + (K - 1) * ALIAS_LEN }

/// `max(min_class, 2^ceil(log2(framed_len)))` (spec 4.2).
pub fn padded_len(framed_len: usize, min_class: usize) -> usize

/// The framed length a compacted hidden section pads to:
/// `n_records · (R + a) + 4` (spec 4.2, 5.5, decision 2 (c)).
pub fn compaction_allowance(n_records: usize) -> usize
```

`padded_len` uses `usize::next_power_of_two` on `framed_len.max(1)`, then `max(min_class)`.

- [ ] **Step 9: Run the constants tests**

Run: `cargo test -p store --lib --features test_mode za_search`
Expected: PASS (three tests).

- [ ] **Step 10: Write the failing configuration tests**

Create `crates/common/src/za/search/config.rs` with a `#[cfg(test)] mod tests` using `from_lookup` over a `HashMap<&str, &str>`:

```rust
fn cfg(vars: &[(&str, &str)]) -> Result<ZaSearchConfig, String> {
    let map: HashMap<&str, &str> = vars.iter().copied().collect();
    ZaSearchConfig::from_lookup(|name| map.get(name).map(|v| v.to_string()))
}

#[test]
fn defaults_when_unset_or_malformed() {
    let c = cfg(&[("ZA_SEARCH_PASS_CAP", "lots"), ("ZA_SEARCH_IDLE_SECS", "")]).unwrap();
    assert_eq!(c, ZaSearchConfig::default());
    assert_eq!(c.mem_mb, 1536); assert_eq!(c.thread_mem_mb, 256); assert_eq!(c.account_max_mb, 512);
    assert_eq!(c.idle_secs, 900); assert_eq!(c.pass_cap, 48); assert_eq!(c.pass_mb, 32);
    assert_eq!(c.pass_hidden_kb, 32); assert_eq!(c.pass_ms, 2000); assert_eq!(c.wait_secs, 30);
    assert_eq!(c.segments, 16); assert_eq!(c.checkpoint, 1000); assert_eq!(c.skip_after, 8);
    assert_eq!(c.absent_retry_secs, 10); assert_eq!(c.pad_min, 65_536);
    assert_eq!(c.hidden_budget(), 32_768);
}

#[test]
fn a_value_parses_with_surrounding_space() {
    assert_eq!(cfg(&[("ZA_SEARCH_WAIT_SECS", " 7 ")]).unwrap().wait_secs, 7);
}

#[test]
fn the_pad_minimum_must_cover_the_budget_the_overshoot_and_the_frame() {
    assert!(cfg(&[("ZA_SEARCH_PAD_MIN", "33300")]).unwrap_err().contains("ZA_SEARCH_PAD_MIN"));
    assert_eq!(cfg(&[("ZA_SEARCH_PAD_MIN", "33301")]).unwrap().pad_min, 33_301);
    // A larger budget moves the bound with it: H = 64 KiB needs M ≥ 66069.
    assert!(cfg(&[("ZA_SEARCH_PASS_HIDDEN_KB", "64")]).unwrap_err().contains("ZA_SEARCH_PAD_MIN"));
    assert!(cfg(&[("ZA_SEARCH_PASS_HIDDEN_KB", "64"), ("ZA_SEARCH_PAD_MIN", "131072")]).is_ok());
}

#[test]
fn the_pass_cap_must_bind_under_the_budget() {
    assert!(cfg(&[("ZA_SEARCH_PASS_CAP", "62")]).unwrap_err().contains("ZA_SEARCH_PASS_CAP"));
    assert_eq!(cfg(&[("ZA_SEARCH_PASS_CAP", "61")]).unwrap().pass_cap, 61);
}

#[test]
fn the_account_cap_is_at_most_a_third_of_the_budget() {
    assert!(cfg(&[("ZA_SEARCH_ACCOUNT_MAX_MB", "513")]).unwrap_err().contains("ZA_SEARCH_ACCOUNT_MAX_MB"));
    assert_eq!(cfg(&[("ZA_SEARCH_ACCOUNT_MAX_MB", "512")]).unwrap().account_max_mb, 512);
    assert!(cfg(&[("ZA_SEARCH_MEM_MB", "1535")]).unwrap_err().contains("ZA_SEARCH_ACCOUNT_MAX_MB"));
}

#[test]
fn a_zero_is_malformed_where_zero_cannot_work() {
    // Zero means "unset" for the counts and intervals that must be positive.
    let c = cfg(&[("ZA_SEARCH_SEGMENTS", "0"), ("ZA_SEARCH_WAIT_SECS", "0"), ("ZA_SEARCH_SKIP_AFTER", "0")]).unwrap();
    assert_eq!((c.segments, c.wait_secs, c.skip_after), (16, 30, 8));
}

#[test]
fn the_segment_count_fits_the_manifests_byte() { // PL12: the manifest's thread count is a u8
    assert!(cfg(&[("ZA_SEARCH_SEGMENTS", "256")]).unwrap_err().contains("ZA_SEARCH_SEGMENTS"));
    assert_eq!(cfg(&[("ZA_SEARCH_SEGMENTS", "255")]).unwrap().segments, 255);
    assert!(cfg(&[("ZA_SEARCH_SEGMENTS", "1")]).unwrap_err().contains("ZA_SEARCH_SEGMENTS"));
    assert_eq!(cfg(&[("ZA_SEARCH_SEGMENTS", "2")]).unwrap().segments, 2);
}
```

Create `crates/common/src/za/search/trace.rs` with:

```rust
#[test]
fn every_search_trace_is_a_fixed_prefixed_string() {
    for s in [ZA_SEARCH_PASS, ZA_SEARCH_SKIPPED, ZA_SEARCH_ABORTED, ZA_SEARCH_LOST, ZA_SEARCH_REBUILD,
              ZA_SEARCH_SUSPENDED, ZA_SEARCH_UNAVAILABLE, ZA_SEARCH_EVICTED, ZA_SEARCH_UNRESOLVED, ZA_SEARCH_OVER_BUDGET] {
        assert!(s.starts_with("zero-access: search "), "{s}");
        assert!(!s.contains('{'), "{s}");
    }
    assert_eq!(crate::za::ZA_MAIL_SEARCH_UNAVAILABLE, "Search for this account is not available right now; try again shortly.");
}
```

- [ ] **Step 11: Run them and see them fail**

Run: `cargo test -p common --lib --features test_mode za::search`
Expected: compile errors for the missing module and items.

- [ ] **Step 12: Implement the configuration, the traces, the cache skeleton and the boot refusal**

`config.rs`: `ZaSearchConfig` (derive `Debug, Clone, PartialEq, Eq`), `impl Default` with the 4.4 defaults, `from_lookup` reading each variable by trim-and-parse with a zero or malformed value falling back to the default (as `lease_secs_from` does), then the four refusals in this order (the fourth, `ZA_SEARCH_SEGMENTS` outside `2..=255`, PL12: the manifest counts its thread blobs in one byte, and one segment would compact on every pass), each returning `Err` with a message that names the variable, its value and the bound, for example `"ZA_SEARCH_PAD_MIN=33300 is below 33301 = ZA_SEARCH_PASS_HIDDEN_KB·1024 + R + (K − 1)·a + 4; refusing to start"` (use `store::za_search::{reservation, RECORD_MAX, ALIAS_LEN, K}`); `from_env` calls `from_lookup(|name| std::env::var(name).ok())`.

`trace.rs`: the ten constants of Global Constraints.

`cache.rs`: `pub struct ZaSearchCache { config: ZaSearchConfig, refusal: Option<String> }` with `new`, `config`, `refusal` (Task 6 adds the entries, budgets and locks to this struct). `mod.rs`: `pub mod cache; pub mod config; pub mod trace;` and re-exports. `crates/common/src/za/mod.rs`: `pub mod search;` and `pub const ZA_MAIL_SEARCH_UNAVAILABLE: &str = ..` beside `ZA_MAIL_SEARCH_REFUSED` (`:31`).

`crates/common/src/lib.rs`: `pub za_search: za::search::cache::ZaSearchCache,` on `Caches` after `za_keys`; `crates/common/src/config/inner.rs:140`: `za_search: ZaSearchCache::new(ZaSearchConfig::from_env()),`. `crates/common/src/manager/boot.rs:199-210`: a second `Server(ServerEvent::Startup)` event, `Details = "Zero-access search"`, `Value` the effective fourteen `NAME=value` pairs joined by single spaces in one `Details` string from `cache.za_search.config()`. `crates/main/src/main.rs`, after the schema check (`:72`):

```rust
    if let Some(reason) = init.inner.cache.za_search.refusal() {
        eprintln!("{reason}");
        trc::event!(
            Server(trc::ServerEvent::StartupError),
            Details = "Zero-access search configuration refused, aborting startup.",
            Reason = reason.to_string(),
        );
        return ExitCode::FAILURE;
    }
```

- [ ] **Step 13: Run the common tests and the compile check**

Run: `cargo test -p common --lib --features test_mode za::search`, then the per-task compile check of Global Constraints.
Expected: PASS (seven tests); `clean`.

- [ ] **Step 14: Add the hooks and counters**

In `crates/common/src/za/test_hooks.rs`, under the existing `#[cfg(feature = "test_mode")]` gating, in the file's style: the seven pause statics (`SEARCH_COMMIT_PAUSE`, `SEARCH_SWAP_PAUSE`, `SEARCH_SNAPSHOT_PAUSE`, `SEARCH_VIEW_PAUSE`, `SEARCH_SWEEP_PAUSE`, `SEARCH_PASS_PAUSE`, `SEARCH_DESTROY_PAUSE`) with their `set_search_*_pause` and `search_*_pause_point` functions (`wait(take_for(&SEARCH_COMMIT_PAUSE, account_id)).await`, as `commit_pause_point` at `:454-456`); `SEARCH_HOLD_EXPIRE: Mutex<Option<u32>>` with `set_search_hold_expire(Option<u32>)` and `search_hold_expire(account_id) -> bool` (read, not taken); `SEARCH_BLOB_FAIL_ONCE` and `SEARCH_OPEN_FAIL_ONCE` with `set_search_blob_fail_once`, `take_search_blob_fail`, `set_search_open_fail_once`, `take_search_open_fail` (one-shot, as `take_commit_fail` at `:525-529`); `SEARCH_BLOB_SLOW: Mutex<Option<(u32, u64)>>` with `set_search_blob_slow` and `search_blob_slow(account_id) -> Option<Duration>`; the counters `static ZA_SEARCH_COUNTERS: Mutex<HashMap<u32, (u64, u64)>>` with `count_za_search_blob_read`, `count_za_search_message_open`, `za_search_blob_reads`, `za_search_message_opens`, `reset_za_search_counters`. No call site is added here; Tasks 6–16 await the pause points and take the one-shots at the sites they name. Add one unit test beside the existing hook tests: `take_search_blob_fail(5)` is true once after `set_search_blob_fail_once(Some(5))`, false after, and false for account 6. Then the flags `SEARCH_CACHE_HOLD`, `SEARCH_SWAP_SKIP` and `SEARCH_OPEN_FAIL_ALL` in the `SEARCH_HOLD_EXPIRE` shape (`set_search_<name>(Option<u32>)`, `search_<name>(account_id) -> bool`); the one-shots `SEARCH_MANIFEST_FAIL_ONCE` and `SEARCH_DESTROY_FAIL_ONCE` (`set_search_<name>_once`, `take_search_<name>`); `SEARCH_CONFIG_OVERRIDE: Mutex<Option<ZaSearchConfig>>` with `set_search_config_override(Option<ZaSearchConfig>)`, and in `cache.rs` `Server::za_search_config(&self) -> ZaSearchConfig` returning the override when set (test mode) and the boot value otherwise, with a unit test `the_override_replaces_the_boot_value_in_test_mode`; and in `trace.rs` the macro `za_search_trace!(CONSTANT, Key = value, ..)` expanding to `trc::event!` on the one fixed existing event type `ZA_SEARCH_EVENT_TYPE` (choose the event release 5 uses for its fixed strings, `crates/common/src/za/imip.rs:22-38`, and export the constant) with a unit test that the macro compiles with two keys.

Run: `cargo test -p common --lib --features test_mode test_hooks && cargo test -p common --lib --features test_mode za::search`
Expected: PASS.

- [ ] **Step 15: Exempt field 152 and the index's principal links in `assert_is_empty`**

In `tests/src/utils/cleanup.rs:409-420`, extend the `Rule::Properties` arm's condition with `|| key[U32_LEN + 1] == PrincipalField::ZeroAccessSearch as u8`, and update the comment ("vault, pin and search-manifest records persist for the life of the account"). In the arm that reads `Subspace::BlobLink` leftovers (`:81`, the `Rule::Leftover { with_values: true }` handling), treat a `BlobLink::Document` link whose suffix is `account_id | Collection::Principal as u8 | 0u32` as the manifest's blob: record the account id in `vault_accounts` and return `Ok(true)`, so the existence check the vault records already get covers it, and a link of a destroyed account still fails the assertion. Then `cargo check -p tests --tests` — Expected: clean. (No za case exercises the exemption until Task 8 writes a manifest; the suite's current run must still pass: run `za::za_tests` once here, since the cleanup file is shared by every module.)

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`
Expected: PASS.

- [ ] **Step 16: Format and commit**

Run: `cargo fmt -p vault -- --check && cargo fmt -p types -- --check && cargo fmt -p store -- --check && cargo fmt -p common -- --check && cargo fmt -p tests -- --check` and `cargo fmt --manifest-path crates/main/Cargo.toml -- --check`.

```bash
git add crates/vault/src/keys.rs crates/vault/src/session.rs crates/types/src/field.rs crates/store/src/lib.rs crates/store/src/za_search/mod.rs crates/common/src/za/mod.rs crates/common/src/za/search crates/common/src/lib.rs crates/common/src/config/inner.rs crates/common/src/manager/boot.rs crates/main/src/main.rs crates/common/src/za/test_hooks.rs tests/src/utils/cleanup.rs
git commit -m "Search plan 16 Task 2: search key, field 152, configuration, format constants and hooks"
```

### Task 3: Thread part model

**Files:**
- Create: `crates/store/src/za_search/thread.rs`, `crates/store/src/za_search/thread_tests.rs`
- Modify: `crates/store/src/za_search/mod.rs` (Task 2's file: `pub mod thread;`, `Checkpoint`, `ThreadFieldsK`, `FormatError`)

**Interfaces:**
- Consumes: `store::ahash::AHashMap`, `xxhash_rust` (re-exported at `lib.rs:26`); nothing of
  the search store. The rule it mirrors by hand: `find_thread_id`
  (`crates/email/src/message/ingest.rs:1098-1197`: one `ThreadMerge::add(thread_id,
  document_id)` per bucket row whose references intersect the new message's hashes,
  `has_message_id`, `:1341-1349`) and `ThreadMerge::merge` (`:1487-1512`: the survivor is the
  thread with the most rows, ties to the lowest thread id; every other matched thread is a
  `merge_id`). Spec 4.1, 5.1 ("Order"), 6.1, 6.3 ("The revision of a state", "The thread of
  `D` as of `X`", "The members of `T` as of `X`").
- Consumes (Task 2, `crates/store/src/za_search/mod.rs`): `SEARCH_FORMAT_V1`, `K`, `RECORD_MAX` (304: entry
    length `u16`, kind `u8`, document id, thread id, revision, created, deleted, subject hash `u128`, hash
    count `u8`, `K` hashes), `ALIAS_LEN` (15), `TOMBSTONE_LEN`, `REVISION_ROW_LEN`, `reservation()` (529).
- Produces (`crates/store/src/za_search/mod.rs`):
  - `pub struct Checkpoint { pub change_id: u64, pub position: u32, pub complete: bool }`
    (`Copy`, `Eq`, `Default`; `Default` is `(0, 0, true)`)
  - `pub struct ThreadFieldsK { pub hashes: Vec<u128>, pub subject_hash: u128, pub sent_at: Option<i64> }`
    (the caller caps `hashes` at `K`, G10; `assign` debug-asserts `hashes.len() <= K`)
  - `pub enum FormatError { Version(u8), Kind(u8), Truncated, Count, AllowanceExceeded }`
    (`Debug`, `Display`)
- Produces (`crates/store/src/za_search/thread.rs`):
  - `pub struct ThreadRecord { pub document_id: u32, pub thread_id: u32, pub revision: u32, pub created: u64, pub deleted: Option<u64>, pub subject_hash: u128, pub hashes: Vec<u128> }`
    (`Clone`, `Eq`, `Debug`; a tombstoned record has `deleted: Some(..)`, `hashes` empty and
    `subject_hash == 0`)
  - `pub struct Alias { pub source: u32, pub target: u32, pub revision: u32 }` (`Copy`, `Eq`)
  - `pub struct RevisionRow { pub revision: u32, pub change_id: u64 }` (`Copy`, `Eq`)
  - `pub struct Tombstone { pub document_id: u32, pub deleted: u64 }` (`Copy`, `Eq`)
  - `pub struct Assignment { pub thread_id: u32, pub aliases: Vec<Alias> }`
  - `pub struct ThreadPart { .. }` (`Default`, `Clone`) with
    - `pub fn from_parts(records: Vec<ThreadRecord>, aliases: Vec<Alias>, rows: Vec<RevisionRow>) -> ThreadPart`
    - `pub fn extend(&mut self, records: impl IntoIterator<Item = ThreadRecord>, aliases: impl IntoIterator<Item = Alias>, rows: impl IntoIterator<Item = RevisionRow>, tombstones: impl IntoIterator<Item = Tombstone>)`
      (records are inserted, aliases added by source, rows merged by revision, tombstones
      applied as `tombstone` does; a tombstone for a document the part does not hold is
      ignored, 5.1)
    - `pub fn record(&self, document_id: u32) -> Option<&ThreadRecord>`
    - `pub fn is_present(&self, document_id: u32) -> bool`
    - `pub fn records(&self) -> impl Iterator<Item = &ThreadRecord>`,
      `pub fn aliases(&self) -> impl Iterator<Item = &Alias>`,
      `pub fn rows(&self) -> &[RevisionRow]` (sorted by revision),
      `pub fn newest_revision(&self) -> u32`
    - `pub fn revision_at(&self, change_id: u64) -> u32` (`R(X)`: the greatest `revision`
      whose row's `change_id <= X`; `0` when none)
    - `pub fn revision_change_id(&self, revision: u32) -> Option<u64>` (`None` when the row
      was pruned, which only happens below every servable state, 5.5)
    - `pub fn thread_of(&self, document_id: u32, change_id: u64) -> u32` (the singleton
      `document_id` when there is no record or `record.revision > R(X)`; otherwise start at
      `record.thread_id` and follow `aliases` by source while the alias's `revision <= R(X)`,
      returning the thread where no alias applies)
    - `pub fn current_thread(&self, document_id: u32) -> u32` (the latest assignment, PL2:
      the singleton without a record; otherwise start at `record.thread_id` and follow every
      held alias by source, **whatever its revision**, with the same step bound. It is not
      `thread_of(d, u64::MAX)`: a pass stages its records and aliases at `base.revision + 1`,
      a revision no row names until the batch commits, so `thread_of` would resolve a staged
      record as its own singleton and the pass's next insert would thread against the wrong
      thread. On a loaded part, which holds published state only, the two agree. `assign`'s
      candidate count, the `threads` table and `present_members` use `current_thread`; every
      state-bounded read, `members_of`, the snapshot's map and the changes stream, uses
      `thread_of`)
    - `pub fn exists_at(&self, document_id: u32, change_id: u64) -> Option<bool>` (`None`
      without a record; else `created <= X && deleted.is_none_or(|d| d > X)`)
    - `pub fn members_of(&self, thread_id: u32, change_id: u64) -> Vec<u32>` (every record,
      present or tombstoned, with `exists_at == Some(true)` and `thread_of == thread_id`,
      ascending document ids; never read from the present thread table, R25)
    - `pub fn present_members(&self, thread_id: u32) -> &[u32]` (the derived thread table:
      present records grouped by `current_thread`, ascending)
    - `pub fn members(&self, thread_id: u32) -> &[u32]` (a second derived table: every record,
      present or tombstoned, grouped by `current_thread`, ascending; `tombstone` leaves a
      record in it. FR7: the snapshot's `thread_members` reads this table and intersects it
      with the cache's present set, since a message the view's cache still holds may be
      tombstoned in the index the same view holds, and `Email/get` lists it under its thread)
    - `pub fn assign(&mut self, fields: &ThreadFieldsK, document_id: u32, revision: u32, created: u64) -> Assignment`
      (6.1; inserts the new record; see step 4)
    - `pub fn tombstone(&mut self, document_id: u32, deleted: u64) -> bool` (`false` when
      the part holds no record; else sets `deleted`, clears `hashes` and `subject_hash`,
      removes the document from the candidate index and the present thread table, keeps it
      in `members`, keeps the assignment and `created`)
    - `pub fn fold_sub_bound(&mut self, sub_bound: impl Fn(&Alias) -> bool) -> usize` (PL7;
      the spec's one-hop folds of 5.5 applied in chain order: for every record, present or
      tombstoned, walk from `record.thread_id` along the aliases for which `sub_bound` holds,
      stopping at the first thread with no such alias; when the walk moved, set `thread_id` to
      where it stopped and `revision` to the revision of the last alias it followed; then
      remove every alias for which `sub_bound` holds and rebuild both thread tables (`present_members`' and `members`', FS10); returns the
      number of records moved. A walk, not one fold per alias in revision order: the aliases
      one pass stages share its revision and the part stores them unordered, so a revision
      sort cannot put `30 → 20` before `20 → 10` when one pass made both, and folding
      `20 → 10` first strands a record at 30 on the retired thread 20. Along any chain the
      revisions never decrease, since an alias always targets a thread alive at its revision,
      so the sub-bound aliases are a prefix of every chain and the walk never crosses an alias
      at or above the bound. Task 15 calls it once, with `sub_bound = |a|
      row_change_id(a.revision) < bound`)
    - `pub fn loaded_bytes(&self) -> usize` (an estimate for the thread budget: records
      `× 72 + hashes × 16`, aliases `× 16`, rows `× 16`, the two thread tables `× 8` per member each)

- [ ] **Step 1: Write the failing unit tests** in `crates/store/src/za_search/thread_tests.rs`
  (declared from `thread.rs` as `#[cfg(test)] #[path = "thread_tests.rs"] mod tests;`, the
  style of `crates/store/src/search/index.rs:416-417`). A fixture helper
  `msg(document_id, subject: u128, hashes: &[u128]) -> ThreadFieldsK` and a part builder
  `indexed(items: &[(u32, u128, &[u128])]) -> ThreadPart` that assigns each item in order
  at revision 1, created `document_id as u64 * 10`. The tests, named after the review tag
  they pin, with these assertions:
  - `s61_no_candidate_is_a_new_thread_of_its_own_id`: `assign(msg(5, 1, &[0x55]), 5, 1, 50)`
    on an empty part returns `thread_id == 5` and no alias; `present_members(5) == [5]`.
  - `s61_one_candidate_thread_joins_it`: after `1: (1, 1, [0x11])`, `assign(msg(2, 1,
    &[0x22, 0x11]), ..)` returns `thread_id == 1`, no alias; `present_members(1) == [1, 2]`.
  - `s61_subject_bucket_must_match`: `1: (1, 7, [0x11])` then `msg(2, 8, &[0x22, 0x11])` is a
    new thread `2`.
  - `s61_a_tombstoned_record_is_not_a_candidate`: `1: (1, 1, [0x11])`, `tombstone(1, 15)`,
    then `msg(2, 1, &[0x22, 0x11])` is a new thread `2`; `record(1)` is `Some` with
    `deleted == Some(15)`, `hashes.is_empty()` and `thread_id == 1`.
  - `h10_survivor_is_the_thread_with_most_candidates_ties_lowest`: present messages
    `3: (3, 1, [0x33])`, `5: (5, 1, [0x55])`, `6: (6, 1, [0x66, 0x55])` (joins 5),
    `9: (9, 1, [0x99])`, `4: (4, 1, [0x44, 0x33])` (joins 3); `assign(msg(10, 1, &[0xa0,
    0x33, 0x44, 0x55, 0x66, 0x99]), 10, 2, 100)` sees two candidates in thread 3 (3, 4), two
    in 5 (5, 6), one in 9: `thread_id == 3`; `aliases` sorted by source `== [Alias{5, 3, 2},
    Alias{9, 3, 2}]`; `present_members(3) == [3, 4, 5, 6, 9, 10]`, `present_members(5)` and
    `present_members(9)` are empty; `current_thread(6) == 3`.
  - `s41_duplicate_message_ids_share_the_root`: `1: (1, 1, [0x11])`, `2: (2, 1, [0x11])`
    joins 1 (same Message-ID hash); `msg(3, 1, &[0x33, 0x11])` sees two candidates in one
    thread: `thread_id == 1`, no alias.
  - `s61_empty_threading_fields_are_a_singleton_and_never_a_candidate` (Review Focus): `msg(1, h_empty, &[])` and `msg(2, h_empty, &[])` (no Message-ID, a whitespace-only subject: `hashes` empty, `subject_hash` the empty thread name's): each `assign` returns its own id with no alias, `present_members(1) == [1]` and `present_members(2) == [2]`; a later `msg(3, h_empty, &[0x33])` is a singleton too, since neither earlier message has a hash to intersect (upstream's `find_thread_id` returns the new thread when `message_ids` is empty, `crates/email/src/message/ingest.rs:1098-1110`; the implementer confirms the line and cites it in the test).
  - `r20_k_references_in_k_distinct_threads_write_exactly_k_minus_one_aliases`: `K` present
    singletons `i: (i, 1, [i as u128])` for `i in 1..=16`; `assign(msg(17, 1, &(1..=16)
    .map(u128::from).collect::<Vec<_>>()), 17, 2, 170)` returns `aliases.len() == K - 1`
    and `thread_id == 1` (all counts tie at one, lowest wins).
  - `r20_no_assignment_writes_more_than_k_minus_one_aliases`: a loop of 200 random fixtures
    (seeded `rand::rngs::SmallRng`): 60 messages in one bucket, each with 1 to `K` hashes
    drawn from a pool of 40, assigned in order; after every `assign`, `aliases.len() <= K -
    1` and the invariant "every present message holding a given hash is in one thread"
    holds (for each hash, the set of `current_thread` over present records holding it has at
    most one element).
  - `r16_insert_then_delete_in_order_threads_as_upstream_did`: `A: (1, 1, [0xa])`, `D: (2, 1,
    [0xd, 0xa])`; then, in log order, `assign(msg(3, 1, &[0xee, 0xd]), 3, 2, 30)` and
    `tombstone(2, 31)`: `current_thread(3) == 1`. The same fixture with the deletion first
    (`tombstone(2, 31)` then the assign): `current_thread(3) == 3` (the control's grouping
    only under the chronological order).
  - `r16_r20_cursor_chronological_order_threads_as_the_control`: items `A: (1, 1, [0xa])`,
    `D: (2, 1, [0xd, 0xa])`, `N: (3, 1, [0xe, 0xd])`, `N2: (4, 1, [0xf, 0xd])` assigned oldest
    first: one thread, `present_members(1) == [1, 2, 3, 4]`, no alias, and at each `assign`
    every hash of the item named at most one present thread (the invariant 5.1's order
    re-establishes; assert it through the candidate lookup). The fan-in bound itself is
    `r20_k_references_in_k_distinct_threads_write_exactly_k_minus_one_aliases`; the suite's
    `case_r16_r20_rebuild_form` (Task 14) pins the order end to end.
  - `pl2_dependent_inserts_in_one_pass_follow_the_staged_overlay`: `from_parts` with record
    `{1, thread 1, revision 1, created 10, subject 1, hashes [0xa]}` and rows `[(1, 100)]`
    (published); then, as one pass stages them at revision 2 with no row 2 installed:
    `assign(msg(2, 1, &[0xd, 0xa]), 2, 2, 20)` → `thread_id == 1`; `assign(msg(3, 1, &[0xe,
    0xd]), 3, 2, 30)` (references the staged `D` alone) → `thread_id == 1` and no alias;
    `current_thread(3) == 1`, `present_members(1) == [1, 2, 3]`; and `thread_of(3, 100) == 3`
    (revision 2 is not published at 100). Then the merge form: published roots `10: (10, 5,
    [0x10])` and `20: (20, 5, [0x20])` at revision 1; staged at revision 2, `assign(msg(30, 5,
    &[0x30, 0x10, 0x20]), 30, 2, 300)` → survivor 10, `aliases == [Alias{20, 10, 2}]`; then
    `assign(msg(40, 5, &[0x40, 0x20]), 40, 2, 400)` (references only the losing member 20) →
    `thread_id == 10`, no alias (it follows the staged alias); `current_thread(20) == 10`.
  - `r5_thread_of_d_as_of_x_before_at_between_and_after`: `from_parts` with record
    `{document_id: 7, thread_id: 7, revision: 1, created: 50, deleted: None, ..}`, aliases
    `[Alias{7, 5, 2}, Alias{5, 3, 3}]`, rows `[(1, 100), (2, 200), (3, 300)]`: `thread_of(7,
    99) == 7`, `thread_of(7, 100) == 7`, `thread_of(7, 150) == 7`, `thread_of(7, 200) == 5`,
    `thread_of(7, 250) == 5`, `thread_of(7, 300) == 3`, `thread_of(7, u64::MAX) == 3`;
    `revision_at(99) == 0`, `revision_at(250) == 2`; `thread_of(8, 1000) == 8` (no record).
  - `r25_members_of_t_as_of_x_across_two_merges_and_a_later_deletion`: records `1: thread 1,
    rev 1, created 10`; `2: thread 2, rev 1, created 20`; `3: thread 1, rev 3, created 250,
    deleted Some(350)`; alias `[Alias{2, 1, 2}]`; rows `[(1, 100), (2, 200), (3, 300)]`:
    `members_of(1, 150) == [1]`, `members_of(2, 150) == [2]`, `members_of(1, 250) == [1,
    2]`, `members_of(1, 300) == [1, 2, 3]`, `members_of(1, 400) == [1, 2]`, `members_of(2,
    400).is_empty()`, `present_members(1) == [1, 2]` (3 is deleted).
  - `s63_exists_at_follows_creation_and_deletion_change_ids`: with the fixture above:
    `exists_at(3, 249) == Some(false)`, `exists_at(3, 250) == Some(true)`, `exists_at(3,
    349) == Some(true)`, `exists_at(3, 350) == Some(false)`, `exists_at(99, 1) == None`.
  - `s63_a_deleted_record_resolves_through_its_retained_record`: `thread_of(3, 300) == 1`
    after the deletion too (`thread_of(3, 400) == 1`).
  - `s55_fold_alias_moves_one_hop_and_keeps_every_servable_resolution`: the R5 fixture;
    `fold_sub_bound(|a| a.revision <= 2)` (the bound between rows 2 and 3) returns 1:
    `record(7).thread_id == 5`, `record(7).revision == 2` (the alias's),
    `aliases()` holds only `Alias{5, 3, 3}`; for every `x in [200, 250, 300, 1000]`
    `thread_of(7, x)` equals its value before the fold (`5, 5, 3, 3`).
  - `pl7_a_same_revision_chain_folds_to_its_end_in_either_order`: roots `10`, `20`, `30`, each
    `(id, 5, [id as u128])`, at revision 1 with row `(1, 100)`; staged at revision 2: `assign
    (msg(40, 5, &[0x40, 30, 20]), 40, 2, 400)` → survivor 20, `Alias{30, 20, 2}`; `assign(msg
    (41, 5, &[0x41, 0x40, 10]), 41, 2, 410)` → survivor 10, `Alias{20, 10, 2}`; row `(2, 200)`
    added. `fold_sub_bound(|a| a.revision <= 2)`: every record's `thread_id == 10` and
    `revision == 2` where it moved (`record(30)`, `record(20)`, `record(40)`), `aliases()` is
    empty, and `thread_of(30, 200) == 10` as before the fold. The same records and aliases
    rebuilt with `from_parts` twice, the aliases listed `[Alias{20, 10, 2}, Alias{30, 20, 2}]`
    and then reversed, give the same result both times (the mutant that folds one alias at a
    time in the map's order leaves `record(30).thread_id == 20` in one of them).
  - `r3_tombstone_keeps_identity_and_drops_fields`: `tombstone(d, 350)` returns `true`,
    `record(d)` keeps `thread_id`, `revision`, `created`; `hashes.is_empty()`,
    `subject_hash == 0`, `is_present(d) == false`; `tombstone(999, 1) == false`.
  - `s43_extend_merges_rows_by_revision_and_ignores_unknown_tombstones`: `extend` with a row
    already held (same revision, same change id) keeps one row; a tombstone for an unknown
    document changes nothing.

- [ ] **Step 2: Run the tests to see them fail**
  ```bash
  export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
  cargo test -p store --lib za_search::thread 2>&1 | tail -20
  ```
  Expected: compile error, `unresolved import` or `could not find za_search in store`.

- [ ] **Step 3: Create `crates/store/src/za_search/mod.rs`** with the SPDX header copied from
  `crates/store/src/search/mod.rs:1-5`, a module doc comment ("Zero-access fork, release 7:
  the sealed per-account search index's model and formats (spec 4)"), `pub mod thread;`,
  the constants, `Checkpoint`, `ThreadFieldsK` and `FormatError` of the Interfaces block
  (each constant with a one-line comment giving its derivation), and add `pub mod
  za_search;` to `crates/store/src/lib.rs` after line 14.

- [ ] **Step 4: Implement `crates/store/src/za_search/thread.rs`.** Storage: `records:
  AHashMap<u32, ThreadRecord>`, `aliases: AHashMap<u32, Alias>` keyed by source (a thread
  is retired once, 5.5), `rows: Vec<RevisionRow>` kept sorted by revision, `threads:
  AHashMap<u32, Vec<u32>>` (present members by `current_thread`, ascending), `members:
  AHashMap<u32, Vec<u32>>` (every record, present or tombstoned, by `current_thread`, ascending;
  FT7) and `buckets: AHashMap<u128, Vec<u32>>` (present document ids by `subject_hash`); `assign`
  inserts the new record into both thread tables, `tombstone` removes from `threads` and
  `buckets` only. `assign`:
  `candidates` = the present records in `buckets[fields.subject_hash]` whose `hashes` share
  at least one value with `fields.hashes`; count one entry per candidate under its
  `current_thread` (upstream's `ThreadMerge::add` per matched row); no candidate: the new
  thread id is `document_id`; one thread: join it; several: the survivor is the thread with
  the highest count, ties to the lowest thread id (`ThreadMerge::merge`, `ingest.rs:1487-
  1512`), and every other candidate thread `L` yields `Alias { source: L, target: survivor,
  revision }`, inserted into `aliases` and applied to `threads` (move `L`'s members under
  the survivor; the members' records are not rewritten, R21). Then insert the new record
  `{ document_id, thread_id: survivor, revision, created, deleted: None, subject_hash,
  hashes }` and index it. `thread_of` follows `aliases` by source with a step bound of
  `aliases.len() + 1` (a defensive cycle stop; the construction has no cycles since an alias
  always points at a surviving thread); `current_thread` walks the same way without the
  revision test (PL2). `fold_sub_bound` computes every record's walk endpoint before
  removing any alias, then updates `records` and rebuilds both thread tables (`threads` and `members`, FS10).

- [ ] **Step 5: Run the tests to see them pass**
  ```bash
  cargo test -p store --lib za_search::thread 2>&1 | tail -30
  ```
  Expected: `test result: ok.`, every test above passing.

- [ ] **Step 6: Format and check**
  ```bash
  cargo fmt -p store -- --check && cargo check -p store 2>&1 | grep -E "^(warning|error)" -A3 | grep za_search || echo clean
  ```
  Expected: `clean`.

- [ ] **Step 7: Commit**
  ```bash
  git add crates/store/src/lib.rs crates/store/src/za_search/mod.rs crates/store/src/za_search/thread.rs crates/store/src/za_search/thread_tests.rs
  git commit -m "Search index: thread part model with upstream's assignment and resolution rules (release 7, task 3)"
  ```

### Task 4: Content part model

**Files:**
- Create: `crates/store/src/za_search/content.rs`, `crates/store/src/za_search/content_tests.rs`
- Modify: `crates/store/src/za_search/mod.rs` (add `pub mod content;`)

**Interfaces:**
- Consumes (all inside `store`, so `pub(crate)` items are reachable): `search::IndexDocument`
  (`crates/store/src/search/mod.rs:176-184`, built by upstream's `index_document`,
  `crates/email/src/message/index/search.rs:43-153`, which Task 8 calls);
  `search::term::AccountIndexer::index_document(&mut self, IndexDocument, u32) -> Document`
  (`term.rs:100-134`, with `insert_value`'s term admission, stems and positions,
  `term.rs:136-200`); `Document::encode(&self) -> codec::Writer` and
  `codec::Writer::into_document(self) -> Vec<u8>` (`term.rs:629-647`, `codec.rs:89-102`: the
  stored form, one zstd frame under `Dictionary::Term` above `COMPRESS_WATERMARK_DICTIONARY`,
  with a trailing `DOCUMENT_ZSTD`/`DOCUMENT_PLAIN` marker); `codec::read_document(stored,
  scratch) -> Option<&[u8]>` (`codec.rs:119-129`); `term::deserialize_term_fields(stored,
  scratch, cb(CheekyHash, u32 field_mask)) -> Option<()>` (`term.rs:731-748`);
  `term::matches_phrase(document, field, words, ordinals, failure) -> Option<bool>`
  (`term.rs:656-729`; a field without positions answers `Some(true)` once every word is
  present, `:728`); `leaf::Leaf` and `Leaf::plan(&SearchFilter)` (`leaf.rs:20-40, 51-162`);
  `leaf::LeafPlan::new(&[SearchFilter]) -> trc::Result<LeafPlan>` (`:232`, the query size
  limits `MAX_QUERY_LEAVES`, `MAX_QUERY_PLANNED_TOKENS`); the evaluation rules of
  `AccountContext::evaluate` and `intersect` (`crates/store/src/search/query.rs:574-629`) and
  the prefix rule of `fetch` (`:787-792`: a term is a prefix candidate when its stored length
  is at most `CheekyHash::HASH_SIZE` and its key does not end in `b'*'`); `SearchField::u8_id`;
  `utils::cheeky_hash::CheekyHash` (`new`, `as_key`, `len`, `crates/utils/src/cheeky_hash.rs:32-76`);
  `search::{SearchFilter, SearchComparator}`; the rank rule of `message_sort_ranks`
  (`crates/email/src/message/sortkeys.rs:134-218`: truncate each key to `MAX_SORT_KEY_LEN =
  40` bytes, zero-pad, sort, dense rank from 1); `roaring::RoaringBitmap`.
- Produces (`crates/store/src/za_search/content.rs`):
  - `pub struct SortKeys { pub from: String, pub to: String, pub subject: String }`
    (upstream's three keys as `MessageSortKeys` holds them, each at most 40 bytes; `email`
    implements `From<&MessageSortKeys>` in Task 8)
  - `pub enum SortField { From, To, Subject, SentAt }` (`SentAt` ranks by `sent_at`, equal timestamps sharing a dense rank; the pass stores upstream's effective sent time in every record (Task 8's `za_effective_sent_at`, PL10: the received time plus the clamped Date offset, so a message with no or an unparsable `Date` sorts at its received time, as `sortkeys.rs:396-408` sorts it), so `None`, which ranks after every `Some`, occurs only in unit fixtures)
  - `pub struct ContentRecord { .. }` (`Clone`, `Eq`; `pub fn encoded_len(&self) -> usize`, the bytes `encode` would write, for Task 4's size tests; the loaded part's size is `ContentPart::loaded_bytes`) with
    - `pub fn new(document: IndexDocument, document_id: u32, sort_keys: SortKeys, sent_at: Option<i64>, has_attachment: bool) -> ContentRecord`
      (runs `AccountIndexer::default().index_document(document, document_id)` and keeps
      `encode().into_document()` as the term bytes)
    - `pub fn document_id(&self) -> u32`, `pub fn encoded_len(&self) -> usize`
    - `pub fn encode(&self) -> Vec<u8>`: `u32 LE document_id | u8 flags (bit 0 has_attachment,
      bit 1 sent_at present) | i64 LE sent_at (0 when absent) | u8 len, from | u8 len, to |
      u8 len, subject | u32 LE len, term bytes`
    - `pub fn decode(bytes: &[u8]) -> Result<ContentRecord, FormatError>`
  - `pub struct ContentPart { .. }` (`Default`) with
    - `pub fn insert(&mut self, record: ContentRecord)` (replaces an existing record of the
      same document id), `pub fn remove(&mut self, document_id: u32) -> bool`,
      `pub fn extend(&mut self, records: impl IntoIterator<Item = ContentRecord>)`
    - `pub fn contains(&self, document_id: u32) -> bool`, `pub fn len(&self) -> usize`,
      `pub fn is_empty(&self) -> bool`, `pub fn document_ids(&self) -> &RoaringBitmap` (a field `insert` and `remove` keep current; FR5)
    - `pub fn record_bytes(&self, document_id: u32) -> Option<&[u8]>` (the encoded record,
      kept for compaction's rewrite without re-encoding, Task 15)
    - `pub fn resolve(&self, filters: &[SearchFilter]) -> Result<Vec<SearchFilter>, ResolveError>`
      (every `Text` and `KeyValue` filter replaced by `SearchFilter::DocumentSet`; an `Integer` filter is `Err(Unresolvable(LeafKind::Integer))`, the caller having stripped the account-id leaf, Task 10;
      `And`, `Or`, `Not`, `End` and existing `DocumentSet`s copied; the size limits of
      `LeafPlan::new` applied first)
    - `pub fn sort_ranks(&self, field: SortField, documents: &RoaringBitmap) -> AHashMap<u32, u32>`
    - `pub fn sent_at(&self, document_id: u32) -> Option<i64>`,
      `pub fn has_attachment(&self, document_id: u32) -> bool`
    - `pub fn loaded_bytes(&self) -> usize` (record bytes plus postings: 16 bytes per
      posting key plus the bitmaps' `serialized_size()`)
  - `pub enum ResolveError { Unresolvable(LeafKind), TooLarge(trc::Error), Corrupt(u32) }`,
    `pub enum LeafKind { Text, KeyValue, Integer }` (`Copy`, `Debug`; the trace of 8 carries
    `LeafKind`, never a field name, G9)

- [ ] **Step 1: Write the failing unit tests** in `crates/store/src/za_search/content_tests.rs`
  (declared from `content.rs` as `#[cfg(test)] #[path = "content_tests.rs"] mod tests;`).
  Helpers: `doc(document_id) -> IndexDocument` = `IndexDocument::new(SearchIndex::Email)
  .with_account_id(1).with_document_id(document_id)`; `record(document_id, build: impl
  FnOnce(&mut IndexDocument)) -> ContentRecord` with `SortKeys::default()`, `sent_at
  Some(1_700_000_000)`, `has_attachment false`; `body(text)` = `index_text(EmailSearchField::
  Body, text, Language::English)`; `part(records) -> ContentPart`; `resolve_one(part,
  filter) -> RoaringBitmap` = `part.resolve(&[filter])` then the single `DocumentSet`. The
  tests:
  - `s41_record_round_trips_through_its_encoding`: a record with body "the quick brown fox",
    subject "Quick", `insert_key_value(EmailSearchField::Headers, "x-custom", "Hello World")`,
    sort keys `("ALICE", "BOB", "QUICK")`, `sent_at Some(42)`, `has_attachment true`:
    `ContentRecord::decode(&r.encode()) == Ok(r)`; `decode` of a truncated encoding is
    `Err(FormatError::Truncated)`.
  - `s41_term_bytes_are_upstreams_stored_document`: build the same `IndexDocument` twice; the
    record's term bytes equal `AccountIndexer::default().index_document(doc2, 7).encode()
    .into_document()` (parity by construction, 7.5).
  - `g5_records_are_encoded_independently`: `encode(a)` is byte-identical whether or not `b`
    was built in the same `AccountIndexer`; a record above the watermark decodes with
    `read_document` into the same plaintext as its own `Document::encode()`.
  - `s71_standard_text_matches_terms_and_stems`: body "the connections dropped"; `resolve_one
    (SearchFilter::has_english_text(EmailSearchField::Body, "connection"))` contains the
    document (stem `connect*`); `has_english_text(Body, "dropping")` contains it;
    `has_english_text(Body, "elephant")` is empty.
  - `s71_groups_and_across_tokens`: body "alpha beta gamma": `has_english_text(Body, "alpha
    gamma")` matches, `"alpha delta"` is empty.
  - `s71_exact_phrase_uses_positions`: body "alpha beta gamma": `has_english_text(Body,
    "\"alpha beta\"")` matches; `"\"beta alpha\""` is empty; `"\"alpha gamma\""` is empty.
  - `s71_phrase_on_a_language_none_field_is_co_occurrence`: `index_text(EmailSearchField::
    From, "alice example", Language::None)`: `has_keyword(From, "\"example alice\"")`
    matches (`term.rs:728`: no positions, every word present).
  - `s71_prefix_matches_verbatim_short_terms_only`: doc 1 body (`Language::None`) "alphabet",
    doc 2 body "alphabetical_sequence_of_terms" (more than 16 bytes, stored hashed): `has_
    keyword(Body, "alpha*")` is `{1}`; `has_keyword(Body, "alphabetical*")` is empty.
  - `s71_key_value_equals_contains_exists`: `insert_key_value(Headers, "x-custom", "Hello
    World")`: `KeyValue { key: "X-Custom", op: Equals("hello world") }` matches, `Contains
    ("world")` matches, `Exists` matches, `KeyValue { key: "x-other", op: Exists }` is empty.
  - `s71_integer_leaf_is_unresolvable`: `SearchFilter::integer_eq(SearchField::Email(
    EmailSearchField::Size), 10u64)` → `Err(ResolveError::Unresolvable(LeafKind::Integer))`
    (use whichever `EmailSearchField` variant exists for an integer; `grep -n 'enum
    EmailSearchField' -A 12 crates/store/src/search/mod.rs`).
  - `s71_resolve_keeps_operators_and_replaces_leaves`: `[And, has_english_text(Body, "alpha"),
    Not, has_keyword(From, "alice"), End, End]` resolves to `[And, DocumentSet(_), Not,
    DocumentSet(_), End, End]`, and `SearchQuery::new(SearchIndex::Email).with_filters
    (resolved).with_mask(all).filter().into_bitmap()` equals the hand-computed set.
  - `s71_too_many_leaves_is_too_large`: `MAX_QUERY_LEAVES + 1` text leaves → `Err(ResolveError::
    TooLarge(_))`.
  - `s71_ranks_mirror_message_sort_ranks`: sort keys `from` of docs 1..=4 = `"B", "a", "B",
    ""`: `sort_ranks(SortField::From, &all) == {4: 1, 1: 2, 3: 2, 2: 3}`; two keys that differ
    only after byte 40 tie; a document outside `documents` is absent from the map.
  - `s71_sent_at_and_attachment_bit_per_document`: records 1 (`sent_at Some(100)`, attachment
    true) and 2 (`None`, false): `sent_at(1) == Some(100)`, `sent_at(2) == None`,
    `has_attachment(1)`, `!has_attachment(2)`, `has_attachment(9) == false` (unknown document).
  - `s71_remove_drops_postings_and_the_document`: after `remove(1)`, every earlier match of
    doc 1 is empty, `contains(1) == false`, `record_bytes(1) == None`.
  - `s42_document_cap_is_upstreams`: a body of 2,000,000 distinct tokens: the record's term
    bytes equal upstream's for the same document (truncated the same way), and `encoded_len
    () <= MAX_STORED_DOCUMENT_SIZE + 256` (`crates/store/src/search/mod.rs:40`).

- [ ] **Step 2: Run the tests to see them fail**
  ```bash
  cargo test -p store --lib za_search::content 2>&1 | tail -20
  ```
  Expected: compile error, `could not find content in za_search`.

- [ ] **Step 3: Implement `crates/store/src/za_search/content.rs`.** Storage: `documents:
  AHashMap<u32, Stored>` with `Stored { encoded: Box<[u8]>, term_range: Range<usize>, sent_at:
  Option<i64>, has_attachment: bool, sort: [Box<str>; 3] }`; `postings: AHashMap<(u8,
  CheekyHash), RoaringBitmap>`; `verbatim: AHashMap<u8, BTreeMap<Vec<u8>, RoaringBitmap>>`
  for the prefix rule (terms with `len() <= CheekyHash::HASH_SIZE` whose key does not end in
  `b'*'`). `insert` runs `deserialize_term_fields` over the stored document and adds the
  document to every `(field, term)` of its field mask (`term.rs:238-250` shows the mask walk).
  `resolve`: `LeafPlan::new(filters)?` for the limits (`TooLarge`), then for each filter:
  `And | Or | Not | End | DocumentSet` copied; otherwise `Leaf::plan(filter)` and
  - `Leaf::Empty` → empty set; `Leaf::Resolved(bm)` → `bm`;
  - `Leaf::Groups { field, groups }` → for each group the union of the postings of its
    present hashes, intersected across groups, empty on the first empty intersection
    (`query.rs:576-596`);
  - `Leaf::Terms { field, terms }` → the intersection of every term's postings, empty when
    any is missing (`:605-629`);
  - `Leaf::Phrase { field, words }` → candidates = the intersection; for each candidate
    `matches_phrase(read_document(term bytes, &mut scratch).ok_or(ResolveError::Corrupt(d))?, field, &words, &mut ordinals,
    &mut failure)`; `None` → `Err(ResolveError::Corrupt(document_id))`; keep `Some(true)`;
  - `Leaf::Prefix { field, prefix }` → the union over `verbatim[field].range(prefix..)` while
    the key `starts_with(prefix)`;
  - `Leaf::UnsupportedRange` → `Err(Unresolvable(LeafKind::Integer))`; an `Integer` filter
    is `Unresolvable(Integer)` (the caller strips the account-id leaf, Task 10; an Email query
    never carries one, 7.1).
  `sort_ranks`: collect `(padded_key, document_id)` for every document in `documents` ∩
  the part, pad as `message_sort_ranks` does (`sortkeys.rs:169-172`), `sort_unstable_by_key`,
  dense rank from 1 (`:199-213`).

- [ ] **Step 4: Run the tests to see them pass**
  ```bash
  cargo test -p store --lib za_search::content 2>&1 | tail -30
  ```
  Expected: `test result: ok.`, every test above passing.

- [ ] **Step 5: Format, check, commit**
  ```bash
  cargo fmt -p store -- --check && cargo check -p store 2>&1 | grep -E "^(warning|error)" -A3 | grep za_search || echo clean
  git add crates/store/src/za_search/mod.rs crates/store/src/za_search/content.rs crates/store/src/za_search/content_tests.rs
  git commit -m "Search index: content part model resolving upstream's leaves from in-memory postings (release 7, task 4)"
  ```

### Task 5: Blob format, sealing and the manifest

**Files:**
- Create: `crates/store/src/za_search/blob.rs`, `crates/store/src/za_search/blob_tests.rs`,
  `crates/store/src/za_search/manifest.rs`, `crates/store/src/za_search/manifest_tests.rs`,
  `crates/common/src/za/search/mod.rs`, `crates/common/src/za/search/seal.rs`,
  `crates/common/src/za/search/seal_tests.rs`
- Modify: `crates/store/src/za_search/mod.rs` (`pub mod blob; pub mod manifest;`, the header
  types), `crates/store/Cargo.toml` (add `zeroize = "1.9"` beside `blake3 = "1.8"`, the
  version `crates/vault/Cargo.toml:13` uses), `crates/common/src/za/mod.rs:14-20` (add
  `pub mod search;`)

**Interfaces:**
- Consumes: Task 3's and Task 4's types; `store::write::{SerializeWithIds, AssignedIds}`
  (`crates/store/src/write/mod.rs:1148-1152`, `AssignedIds::current_change_id` `:1648-1651`,
  `set_current_change_id` `:1660`, `pub(crate)`, for the test); `types::blob_hash::BlobHash`
  (`[u8; 32]`, `crates/types/src/blob_hash.rs:9, 28`); `blake3::{hash, keyed_hash}`;
  `zeroize::Zeroizing`; `vault::keys::{Secret, aad, seal, open, KeyError}`
  (`crates/vault/src/keys.rs:36-48, 151-158, 181-235`: `seal` returns `nonce || ciphertext`,
  `open` returns `Zeroizing<Vec<u8>>` or `KeyError::{Length, Aead}`); the framing convention
  of `seal_padded` (`crates/groupware/src/calendar/seal/tree.rs:276-284`: a `u32 LE` length
  prefix, the plaintext, zero padding, then `seal`; its 256-byte rounding is not this
  policy); the byte-hash assertion form `AssertValue::Hash(xxh3_64(bytes))`
  (`crates/common/src/auth/vault.rs:264-268`, `crates/store/src/write/assert.rs:57`).
- Produces (`crates/store/src/za_search/mod.rs`):
  - `pub struct SegmentId(pub [u8; 16]);` (`Copy`, `Eq`, `Hash`; `SegmentId::random()` via
    `rand::random`) `pub enum BlobKind { Thread = 1, Content = 2 }`
    `pub enum SectionKind { Hidden = 1, Public = 2, Content = 3 }` (both `Copy`, `Eq`,
    `TryFrom<u8>`)
  - `pub struct BlobHeader { pub kind: BlobKind, pub tier: u8, pub segment: SegmentId, pub predecessor: Option<(BlobHash, SegmentId)> }`
    (`Clone`, `Eq`) with `pub const LEN: usize = 1 + 1 + 1 + 16 + 32 + 16;` (67),
    `pub fn encode(&self) -> [u8; Self::LEN]` (`SEARCH_FORMAT_V1`, kind, tier, segment,
    predecessor hash and segment or zeros) and
    `pub fn decode(bytes: &[u8]) -> Result<(BlobHeader, &[u8]), FormatError>` (the rest of
    the blob follows; `Version(v)` for another first byte, `Kind(k)` for an unknown kind;
    an all-zero predecessor decodes as `None`)
  - `pub struct ZaSearchKey(Zeroizing<[u8; 32]>)` with `pub fn new([u8; 32]) -> Self`,
    `pub fn as_bytes(&self) -> &[u8; 32]`, `Clone`, and `Debug` printing `ZaSearchKey(..)`
  - (Task 2's, consumed) `padded_len(framed_len: usize, min_class: usize) -> usize` =
    `max(min_class, framed_len.next_power_of_two())`
  - (Task 2's, consumed) `compaction_allowance(n_records: usize) -> usize` = `n_records * (RECORD_MAX +
    ALIAS_LEN) + 4` (the framed length of a section holding `n` maximal records and `n`
    aliases, decision 2 (c), R35)
  - `pub enum PadTarget { Publishing, Compaction { n_records: usize } }`
- Produces (`crates/store/src/za_search/blob.rs`, plaintext only):
  - `pub struct HiddenSection { pub records: Vec<ThreadRecord>, pub aliases: Vec<Alias> }`,
    `pub struct PublicSection { pub tombstones: Vec<Tombstone>, pub rows: Vec<RevisionRow> }`
  - `pub fn encode_hidden(section: &HiddenSection) -> Vec<u8>` (entries, each self-delimiting, `len` being the entry's total length including the prefix: `u16 LE len | u8
    kind (1 record, 2 alias) | body`; record body `u32 document_id | u32 thread_id | u32
    revision | u64 created | u64 deleted (0 = present) | u128 subject_hash | u8 n | n × u128`;
    alias body `u32 source | u32 target | u32 revision`; all little-endian; records first)
  - `pub fn decode_hidden(bytes: &[u8]) -> Result<HiddenSection, FormatError>`
  - `pub fn frame_and_pad(hidden: &[u8], target: PadTarget, min_class: usize) -> Result<Vec<u8>, FormatError>`
    (`u32 LE hidden.len() | hidden | zeros` to exactly `min_class` for `Publishing`,
    after rejecting `hidden.len() + 4 > min_class` with `Err(AllowanceExceeded)`;
    for `Compaction`, reject a framed payload above `compaction_allowance(n_records)`
    and pad to `padded_len(compaction_allowance(n_records), min_class)`. Preserve checked
    length arithmetic/conversions. A publishing overflow aborts; never retry at a larger
    content-dependent class. Compaction callers follow E5's refusal policy. P1-PAD: do
    not call the class helper on the actual publishing payload, even when it fits `M`.)
  - `pub fn unframe(padded: &[u8]) -> Result<&[u8], FormatError>` (`Truncated` when the
    prefix exceeds what follows)
  - `pub fn encode_public(section: &PublicSection) -> Vec<u8>` (`u32 LE n | n × (u32
    document_id | u64 deleted) | u32 LE m | m × (u32 revision | u64 change_id)`; `u32` for the rows too, since a compaction keeps every row at or above the bound) and
    `pub fn decode_public(bytes: &[u8]) -> Result<PublicSection, FormatError>`;
    `pub fn public_len(tombstones: usize, rows: usize) -> usize` = `4 + 12·t + 4 + 12·m`
  - `pub fn encode_content(records: &[&[u8]]) -> Vec<u8>` (`u32 LE n | n × (u32 LE len |
    record)`) and `pub fn decode_content(bytes: &[u8]) -> Result<Vec<ContentRecord>, FormatError>`
  - `pub const SEAL_OVERHEAD: usize = 24 + 16;` (nonce and tag of `vault::keys::seal`)
- Produces (`crates/store/src/za_search/manifest.rs`):
  - `pub struct ThreadBlobEntry { pub segment: SegmentId, pub hash: BlobHash, pub size: u32 }`,
    `pub struct ContentHead { pub hash: BlobHash, pub segment: SegmentId, pub length: u32, pub size: u64 }`,
    `pub struct ContentTo { pub checkpoint: Checkpoint, pub suspended: bool, pub cursor: Option<ContentCursor> }`,
    `pub struct ContentCursor { pub next_below: u32 }` (newest first: the exclusive upper bound
    of what is left, `u32::MAX` at the start; plan call E3),
    `pub struct RebuildCursor { pub next: u32, pub end: u32 }` (`next` the lowest document id not yet done, 0 at the start; `end` the highest id present at the snapshot, inclusive; E3),
    `pub struct ReconciliationMarker { pub bound: u64, pub gap_next: u32 }` (`gap_next`: the cursor over the difference's gap inserts, spec 5.4, PL6),
    `pub struct FailingDocument { pub document_id: u32, pub attempts: u8 }`,
    `pub struct Manifest { pub revision: u32, pub newest_row: RevisionRow, pub indexed_to: Checkpoint, pub epoch: u64, pub thread_blobs: Vec<ThreadBlobEntry>, pub content: Option<ContentHead>, pub content_to: ContentTo, pub cursor: Option<RebuildCursor>, pub reconciliation: Option<ReconciliationMarker>, pub failing: Option<FailingDocument> }`
    (all `Clone`, `Eq`, `Debug`, `Default`) with `pub fn identity_version(&self) -> (u64, u32)`
    (`(epoch, revision)`) and `pub fn inventory(&self) -> Inventory`
  - `pub struct Inventory { pub thread_blobs: Vec<ThreadBlobEntry>, pub content: Option<ContentHead> }`
  - `pub enum ManifestValue { Index(Manifest), Destroying(Inventory) }` with
    - `pub fn encode(&self, key: &ZaSearchKey) -> Vec<u8>`: `[SEARCH_FORMAT_V1][1][fields][tag]`
      for `Index`, the tag `blake3::keyed_hash(key, &bytes[..len - 32])` written last;
      `[SEARCH_FORMAT_V1][2][inventory]` for `Destroying`, no tag (destroy holds no key and
      ignores the tag, 4.2). Field order and widths: `u32 revision | u32 row.revision | u64
      row.change_id | u64 indexed_to.change_id | u32 indexed_to.position | u8 complete | u64
      epoch | u8 n_thread | n × (16 segment | 32 hash | u32 size) | u8 has_content | (32 hash
      | 16 segment | u32 length | u64 size) | content_to (13 checkpoint | u8 suspended | u8
      has_cursor | u32 next_below) | u8 has_cursor | u32 next | u32 end | u8 has_marker |
      u64 bound | u32 gap_next | u8 has_failing | u32 document_id | u8 attempts`, little-endian; an absent optional field writes its `has_*` byte as `0` and omits the field's bytes. `n_thread` is a u8 (PL12): Task 2's boot refusal keeps `ZA_SEARCH_SEGMENTS` in `2..=255`, compaction keeps the list below `segments`, and Task 8's `write_pass` aborts as a defect (`ZA_SEARCH_ABORTED`, reason `"segments"`, nothing written) a pass whose new list would exceed 255 entries, which only repeated compaction defects can reach (plan call E5); `encode` is therefore never called with more than 255 and `debug_assert!`s it.
    - `pub fn decode(bytes: &[u8], key: &ZaSearchKey) -> Result<ManifestValue, ManifestError>`
      (`NewerFormat(v)` when the first byte is above `SEARCH_FORMAT_V1`, `OlderFormat(v)`
      below; `Tag` when the keyed hash differs, compared as `blake3::Hash` values, which is
      constant-time; `Parse` otherwise)
    - `pub fn decode_unverified(bytes: &[u8]) -> Result<ManifestValue, ManifestError>` (either state parsed, the tag read but not checked: destroy, the reindex reset and the suite's `read_manifest` helper, 5.7), `pub fn decode_inventory(bytes: &[u8]) -> Result<Inventory, ManifestError>` (either
      state's inventory, no key, no tag check: destroy's read, 5.7) and `pub fn encode_destroying(inventory: &Inventory) -> Vec<u8>` (`Destroying(inv).encode` needs no key: the same bytes without a tag)
  - `pub enum ManifestError { Tag, Parse, NewerFormat(u8), OlderFormat(u8) }`
  - `pub fn manifest_hash(bytes: &[u8]) -> [u8; 32]` (`blake3::hash`, the loaded index's
    version, R18; distinct from the write assertion's `xxh3_64`)
  - `pub fn manifest_assertion(previous: Option<&[u8]>) -> AssertValue` (`Hash(xxh3_64
    (bytes))` or `None`, as `ZaPinRead::assertion`, `crates/common/src/auth/vault.rs`)
  - `pub struct ManifestFill { pub newest_row: bool, pub epoch: bool }` (`Copy`, `Default`)
  - `pub struct ManifestWrite { .. }` with `pub fn new(value: ManifestValue, key: ZaSearchKey, fill: ManifestFill) -> ManifestWrite`
    and `impl SerializeWithIds`: `serialize_with_ids` sets `newest_row.change_id` and/or
    `epoch` to `ids.current_change_id()` when the flag is set, then `Ok((value.encode(&key),
    None))`, deterministic so a FoundationDB retry reruns it (R33); `size_hint` is the
    encoded size; the key is zeroized when the write is dropped (invariant 48)
- Produces (`crates/common/src/za/search/seal.rs`):
  - `pub const AAD_SEARCH: &str = "search";`
  - `pub fn section_aad(account_id: u32, header: &BlobHeader, kind: SectionKind) -> Vec<u8>` =
    `aad(AAD_SEARCH, account_id) || b"|" || header.encode() || [kind as u8]`
  - `pub fn seal_section(key: &Secret, account_id: u32, header: &BlobHeader, kind: SectionKind, plaintext: &[u8]) -> Vec<u8>`
  - `pub fn open_section(key: &Secret, account_id: u32, header: &BlobHeader, kind: SectionKind, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, LossClass>`
    (`KeyError::{Aead, Length}` → `LossClass::Aead`)
  - `pub fn build_thread_blob(key: &Secret, account_id: u32, header: &BlobHeader, hidden: Option<(&HiddenSection, PadTarget)>, public: &PublicSection, min_class: usize) -> Result<Vec<u8>, FormatError>`
    (`header.encode() | u32 LE hidden_sealed.len() | hidden_sealed | public_sealed`; a
    non-publishing pass passes `hidden: None` and writes length 0, H13)
  - `pub fn parse_thread_blob(key: &Secret, account_id: u32, expected: &BlobHeader, bytes: &[u8]) -> Result<(Option<HiddenSection>, PublicSection), LossClass>`
    (`Parse` when the header decodes to anything but `expected`, so a blob cannot be moved
    to another position; `Aead` from either section; `Parse` from a section decoder)
  - `pub fn build_content_blob(key: &Secret, account_id: u32, header: &BlobHeader, records: &[&[u8]]) -> Vec<u8>`
    (`header.encode() | content_sealed`)
  - `pub fn parse_content_blob(key: &Secret, account_id: u32, expected_segment: Option<SegmentId>, expected_hash_of_predecessor: Option<&(BlobHash, SegmentId)>, bytes: &[u8]) -> Result<(BlobHeader, Vec<ContentRecord>), LossClass>`
    (the head is checked against the manifest's descriptor, each later node against the
    predecessor its successor's header named, R11)
  - `pub fn za_search_key(secret: &Secret) -> ZaSearchKey` (`ZaSearchKey::new(*secret.as_bytes())`; a `From` impl would break the orphan rule, `ZaSearchKey` being `store`'s and `Secret` `vault`'s)
  - `LossClass` lives in Task 6's `error.rs`; this task creates it there with the variants
    of the Interfaces block of Task 6 and Task 6 adds the other enums.

- [ ] **Step 1: Write the failing unit tests.** In `crates/store/src/za_search/blob_tests.rs`:
  - `r20_record_max_and_alias_len_are_the_encoded_maxima`: a record with `K` hashes encodes
    (as one entry) to exactly `RECORD_MAX`; a record with 0 hashes to `RECORD_MAX - 16 * K`;
    an alias to `ALIAS_LEN`; `reservation() == 529`; `RECORD_MAX == 304 && ALIAS_LEN == 15`.
  - `r15_padded_len_at_class_boundaries`: with `M = 65536`: `padded_len(100, M) == M`,
    `padded_len(M - 1, M) == M`, `padded_len(M, M) == M`, `padded_len(M + 1, M) == 2 * M`,
    `padded_len(2 * M, M) == 2 * M`, `padded_len(2 * M + 1, M) == 4 * M`; payloads: a hidden
    section of `M - 5` bytes frames to `M - 1` and Publishing pads to `M`; `M - 4`
    succeeds at `M`; `M - 3` and `2 * M` return `AllowanceExceeded` for Publishing.
    Keep arbitrary class-boundary assertions on the helper; they are not valid reasons
    to grow a publishing blob. Successful frames unframe to the complete input.
  - `r20_g1_maximal_pass_stays_under_m_and_every_pass_pads_to_m`: with `H = 32768` and
    `M = 65536`: `H + reservation() + 4 <= M`; fixtures (a) one record with 0 hashes, no
    alias; (b) one record, 15 aliases; (c) 61 records with `K` hashes and `61 * 15` aliases
    (`61 * 529 = 32269 <= H`) plus one message with `K - 1` aliases as the conservative
    one-message overshoot bound (the pass may split a grouped record, R20): each
    `frame_and_pad(encode_hidden(..), PadTarget::Publishing, M)` has length exactly `M`;
    `encode_hidden` of (c) is at most `H + reservation()`, and its framed length is under `M`.
  - `publishing_non_power_of_two_minimum_hides_alias_count` (P1-PAD): valid configuration
    `H = 120 * 1024`, `C = 232`, `M = 123413` satisfies both boot bounds. Two publishing
    sections each hold 200 maximal records with 16 hashes: one has no alias, the other
    has 400 aliases (two losing threads per new message, from disjoint prior cohorts).
    Reservations are `200 * 529 = 105800 < H` and 200 is below C. Assert encoded lengths
    60800 and 66800, framed lengths 60804 and 66804, and that both Publishing outputs
    have length exactly 123413 and round-trip through `unframe` and `decode_hidden`.
    The old actual-payload rounding produces 123413 versus 131072 and must fail this test.
    With the same non-power-of-two M, a hidden payload of M-4 succeeds and M-3 is refused.
  - `i2_compaction_allowance_is_a_function_of_counts`: two `HiddenSection`s with 1,000
    records each (500 present with `K` hashes, 500 tombstoned), one with no alias and one
    with 999 aliases: `frame_and_pad(.., PadTarget::Compaction { n_records: 1000 }, M)`
    lengths are equal; `compaction_allowance(30_000) == 30_000 * 319 + 4` and `padded_len
    (compaction_allowance(30_000), M) == 16 * 1024 * 1024` (the 16 MiB class of 1.3 and
    13); `padded_len(compaction_allowance(0), M) == M`. Repeat the two 1,000-record
    sections with `M = 123413`: each must use `padded_len(compaction_allowance(1000), M)`,
    not the exact Publishing target; retain the allowance-exceeded refusal.
  - `r35_allowance_exceeded_is_an_error`: 10 records and 11 aliases with `n_records: 10`
    whose encoding exceeds `compaction_allowance(10)` (add a 12th alias if needed) →
    `Err(FormatError::AllowanceExceeded)`.
  - `s42_hidden_section_round_trips_records_then_aliases`: `decode_hidden(encode_hidden(s))
    == Ok(s)` for a section with tombstoned and present records and aliases; a stray kind
    byte 9 → `Err(Kind(9))`; a cut entry → `Err(Truncated)`.
  - `s42_public_section_is_fixed_width`: `encode_public(s).len() == public_len(t, m)` for
    `t in [0, 1, 1000]`, `m in [0, 1]`; round trip.
  - `g5_content_section_is_length_prefixed_records`: three encoded records round-trip; the
    section's length is the sum of record lengths plus `4 + 4 * 3`.
  - `s42_blob_header_round_trips_and_rejects_other_versions_and_kinds`: with and without a
    predecessor; first byte `2` → `Err(Version(2))`; kind `3` → `Err(Kind(3))`;
    `BlobHeader::LEN == 67`.
  - `s42_zero_access_search_key_debug_is_silent`: `format!("{:?}", ZaSearchKey::new([7; 32]))
    == "ZaSearchKey(..)"`.
  In `crates/store/src/za_search/manifest_tests.rs`:
  - `s42_manifest_round_trips_every_field`: a `Manifest` with 3 thread blobs, a content head,
    a suspended `content_to` with a cursor, a rebuild cursor, a marker and a failing
    document: `decode(&encode(key), key) == Ok(Index(m))`.
  - `h9_r33_tag_is_computed_at_commit_over_the_filled_change_id`: `ManifestWrite::new(Index
    (m), key, ManifestFill { newest_row: true, epoch: true })`; `let mut ids = AssignedIds::
    default(); ids.set_current_change_id(..)` is `pub(crate)` and needs a change id in the
    struct: add
    `#[cfg(test)] pub(crate) fn with_current_change_id(change_id: u64) -> AssignedIds` (plan call B7)
    beside `:1648` (one test-only constructor, the plan's call); `serialize_with_ids(&mut
    write, &ids)` → `(bytes, None)`; `decode(&bytes, key)` → `Index(m2)` with
    `m2.newest_row.change_id == 4242 && m2.epoch == 4242`; flipping byte 10 → `Err(Tag)`;
    decoding with another key → `Err(Tag)`; serializing twice gives identical bytes
    (deterministic, the FoundationDB retry).
  - `h9_a_manifest_without_fill_keeps_its_change_ids`: `ManifestFill::default()` leaves
    `newest_row.change_id` and `epoch` as given.
  - `f7_newer_format_is_no_map_older_is_loss`: first byte `2` → `Err(NewerFormat(2))`; `0`
    → `Err(OlderFormat(0))`.
  - `r8_manifest_stays_small_at_segments_entries`: 16 thread blobs and a head with `length
    == 10_000` encode to fewer than 2,048 bytes.
  - `g12_destroying_marker_and_inventory_decode_without_a_key`: `Destroying(inv)` encodes
    without a tag and `decode_inventory` returns `inv`; `decode_inventory` of an `Index`
    manifest returns its inventory even when a byte of the tag was altered.
  - `r18_manifest_hash_changes_on_any_byte`: `manifest_hash(a) != manifest_hash(a')` for a
    one-byte difference in `indexed_to`; `manifest_assertion(Some(&a)) == AssertValue::Hash
    (xxh3_64(&a))`, `manifest_assertion(None) == AssertValue::None`.
  In `crates/common/src/za/search/seal_tests.rs` (declared from `seal.rs`):
  - `s31_a_section_opens_only_under_its_account_header_and_kind`: seal under `(key, 1, h1,
    Hidden)`; opening with account 2, with a header whose segment differs, with a header
    whose predecessor differs, with `SectionKind::Public`, or with another key is
    `Err(LossClass::Aead)`; the right tuple returns the plaintext.
  - `r9_h13_thread_blob_parses_with_and_without_a_hidden_section`: `build_thread_blob` with
    `Some((&hidden, PadTarget::Publishing))` and with `None`; `parse_thread_blob` returns
    `(Some(hidden), public)` and `(None, public)`; the blob with a hidden section has length
    `BlobHeader::LEN + 4 + (M + SEAL_OVERHEAD) + (public_len(t, m) + SEAL_OVERHEAD)` exactly
    (the length formula of 4.2). Repeat with `M = 123413` and the P1-PAD twins above:
    equal public sections give equal final blob lengths, and each blob parses back to
    its own hidden/public contents. Parsing remains independent of the current config;
    do not require a historical blob's padding to equal the node's current M.
  - `s42_thread_blob_rejects_another_position`: `parse_thread_blob` with an `expected` header
    of another segment → `Err(LossClass::Parse)`.
  - `r11_content_blob_names_its_predecessor`: build node 1 (no predecessor) and node 2
    (predecessor = (hash of node 1's bytes, node 1's segment)); `parse_content_blob(key, 1,
    Some(seg2), None, node2)` returns header with `predecessor == Some((h1, seg1))`; parsing
    node 1 with `expected_hash_of_predecessor = Some(&(h1, seg1))` where its own hash or
    segment differs → `Err(Parse)`.
  - `s31_sealed_bytes_hold_no_plaintext`: a record with a 64-byte recognisable `subject_hash`
    and hash pattern; the blob bytes do not contain the pattern.

- [ ] **Step 2: Run the tests to see them fail**
  ```bash
  cargo test -p store --lib za_search::blob 2>&1 | tail -5; cargo test -p store --lib za_search::manifest 2>&1 | tail -5; cargo test -p common --lib za::search::seal 2>&1 | tail -5
  ```
  Expected: compile errors (`could not find blob`, `manifest`, `search`).

- [ ] **Step 3: Implement the store side**: the header types, `ZaSearchKey`, `padded_len`,
  `compaction_allowance` and `PadTarget` in `mod.rs`; `blob.rs` and `manifest.rs` as the
  Interfaces block specifies. `frame_and_pad` builds the framed buffer in a `Zeroizing<Vec<u8>>`
  before copying out, as `seal_padded` does (`tree.rs:278`). `ManifestWrite` holds its
  `ManifestValue` and `ZaSearchKey` by value. Add `zeroize = "1.9"` to `crates/store/Cargo.toml`.

- [ ] **Step 4: Implement the common side**: `crates/common/src/za/search/mod.rs` with
  `pub mod error; pub mod seal;` (`error.rs` created here with `LossClass`; Task 6 extends
  it), `pub mod search;` in `crates/common/src/za/mod.rs` after line 17; `seal.rs` as
  specified, opening each section into a `Zeroizing<Vec<u8>>` and decoding from it.

- [ ] **Step 5: Run the tests to see them pass**
  ```bash
  cargo test -p store --lib za_search 2>&1 | tail -5; cargo test -p common --lib za::search 2>&1 | tail -5
  ```
  Expected: `test result: ok.` in both.

- [ ] **Step 6: Format, check, commit**
  ```bash
  cargo fmt -p store -- --check && cargo fmt -p common -- --check && cargo check -p common 2>&1 | grep -E "^(warning|error)" -A3 | grep -E "za_search|za/search" || echo clean
  git add crates/store/Cargo.toml Cargo.lock crates/store/src/za_search/ crates/common/src/za/mod.rs crates/common/src/za/search/
  git commit -m "Search index: blob format with padded hidden sections, sealed sections and the tagged manifest serialized at commit (release 7, task 5)"
  ```

### Task 6: The loaded index: cache, loader and error classes

**Files:**
- Create: `crates/common/src/za/search/load.rs`, `crates/common/src/za/search/load_tests.rs`
- Modify: `crates/common/src/za/search/cache.rs` (Task 2's skeleton gains the fields and methods below), `crates/common/src/za/search/error.rs` (the other enums), `crates/common/src/za/search/mod.rs`,
  `crates/common/src/cache/invalidate.rs:615, 642` (`cache.za_search.remove_account(*id,
  EvictReason::Invalidation)` beside each `za_keys.remove_account(*id)`)

**Interfaces:**
- Consumes: Tasks 3 to 5; Task 2's `ZaSearchConfig::from_env() -> Result<ZaSearchConfig,
  String>` (`crates/common/src/za/search/config.rs`) and its test-mode counter
  incrementer `count_za_search_blob_read(account_id)` in `crates/common/src/za/test_hooks.rs`;
  `Server::get_blob(&self, key: &[u8], range: Range<usize>) -> impl Future<Output =
  trc::Result<Option<Vec<u8>>>>` (`crates/common/src/storage/blob.rs:416-422`, which
  decodes the blob codec with `core.blob.keys`); `self.store().get_value::<RawValue>(key)`
  with `RawValue` (`crates/common/src/auth/vault.rs:46-52`) and a `ValueKey { account_id,
  collection: Collection::Principal.into(), document_id: 0, class: ValueClass::Property
  (PrincipalField::ZeroAccessSearch.into()) }` as `vault_key` builds field 150's
  (`vault.rs:60`); the codec's error shapes (`crates/store/src/write/blob_codec/mod.rs:216-
  238`: `StoreEvent::DecryptError` with `Key::Id` set for a missing codec key, `DecryptError`
  without it for a truncated blob, `StoreEvent::DecompressError`); `trc::Error::matches
  (EventType)`, `value(Key)` (`crates/trc/src/event/mod.rs:142, 216`); `vault::session::
  SessionKeys::swk()` (Task 2); `store::write::now()` (`write/mod.rs:1338`); the
  invalidation arms (`invalidate.rs:609-616, 633-642`); `SwapCached` (`crates/common/src/
  cache/swap/evict.rs:16-30`: `ZaSearchCache` and `LoadedIndex` never implement it, F16).
- Produces (`error.rs`):
  - `pub enum LossClass { ManifestMissing, StubAbsent, BlobAbsent, Aead, Parse, Tag, OlderFormat, Hash }`
    (`Hash`: a fetched index blob whose BLAKE3 hash is not the one its descriptor named, PL1)
  - `pub enum TransientClass { Store, BlobStore, Budget, AbsentWithinWindow, CodecKey, Validation }`
  - `pub enum NoMapReason { NewerFormat, Destroying, Transient(TransientClass) }`
  - `pub enum LoadFailure { Loss(LossClass), Transient(TransientClass), NoMap(NoMapReason), OverCap }`
    (all `Copy`, `Eq`, `Debug`; each with `pub fn as_str(&self) -> &'static str`, the reason
    class the traces carry, invariant 54)
- Produces (`cache.rs`):
  - `pub enum PartKind { Thread, Content }`, `pub enum EvictReason { Idle, Budget, Invalidation }`
  - `pub struct ZaSearchCache { .. }` with `pub fn new(config: Result<ZaSearchConfig, String>) -> Self`
    (Task 2's skeleton; this task adds the fields and methods below), `pub fn config(&self) -> &ZaSearchConfig`
    (the boot value; `Server::za_search_config()` is the effective value with the test-mode override, Task 2), `pub fn entry(&self, account_id: u32) ->
    Arc<IndexEntry>` (get or create, touches `last_used`, then `sweep_idle`), `pub fn get
    (&self, account_id: u32) -> Option<Arc<IndexEntry>>`, `pub fn remove_account(&self,
    account_id: u32, reason: EvictReason)` (drops the entry from the map and traces
    `ZA_SEARCH_EVICTED` with the reason; its bytes are released when the last holder of its
    parts drops them, through their `BudgetCharge`, PL11), `pub fn drop_content(&self, account_id: u32,
    reason: EvictReason)`, `pub fn admit(&self, part: PartKind, bytes: u64, keep: u32)
    -> Option<BudgetCharge>` (Codex rev-7 PL11: admission and charging are one step under the
    cache's admission lock, `admission: std::sync::Mutex<()>`, held across the eviction, the
    check and the counter's add, so two accounts cannot both see room and both charge; inside
    it the private `make_room(part, bytes, keep) -> bool` evicts least-recently-used parts not in
    use and not `keep`'s, content parts of every account before any thread part, until `used +
    bytes <= budget`; `Some(charge)` with `bytes` added when it fits, `None` when a content part
    does not fit after the evictions; a thread part is always admitted, `Some`, over the budget
    if need be, H8; the loader alone traces `ZA_SEARCH_OVER_BUDGET` when `thread_used() >
    budget` after its `admit`, the pass's clone charge of Task 8 step 5 traces nothing, FV9), `pub fn grow(&self, charge: &mut BudgetCharge, part: PartKind, new_bytes: u64,
    keep: u32) -> bool` (the same lock: shrinking always succeeds; growing evicts for the delta
    and adds it, or answers `false` and leaves the charge as it was; a thread part's growth is
    always admitted), `pub fn admit_content_growth(&self, index: &mut LoadedIndex, account_id:
    u32) -> bool` (FU4, FU7: after a swap or an incremental reload appended records, `grown =
    content.part.loaded_bytes()`; `grow(&mut content.charge, Content, grown, account_id)` sets
    `content.bytes = grown` and answers `true`; refused, it sets `index.content = None`, dropping
    the part and its charge at the old size, traces `ZA_SEARCH_EVICTED` (`Budget`) and answers
    `false`; on `content: None` it answers `true` at once (FV9); the committed progress is untouched and the next content request reloads through
    `za_search_load_content`),
    `pub fn sweep_idle(&self)` (every entry whose `last_used` is older than `idle_secs`),
    `pub fn thread_used(&self) -> u64`, `pub fn content_used(&self) -> u64` (the two counters the charges move, so they include generations a reader still holds), and under `test_mode` `pub fn set_content_bytes_for_test(&self, account_id: u32, bytes: u64)` (records a content part's size without a load, for the budget tests)
  - `pub struct IndexEntry { pub account_id: u32, pub index: Arc<tokio::sync::RwLock<Option<LoadedIndex>>>, pub pass_lock: tokio::sync::Mutex<()>, pub load_flight: tokio::sync::Mutex<()>, .. }`
    (`za_search_entry` returns an entry whose `index` is `Some`; a holder takes `entry.index.clone().read_owned().await` and maps it with `tokio::sync::OwnedRwLockReadGuard::map(guard, |o| o.as_ref().expect("loaded"))`, so where Tasks 8, 10 and 11 write `OwnedRwLockReadGuard<LoadedIndex>` they mean the mapped guard `OwnedRwLockReadGuard<Option<LoadedIndex>, LoadedIndex>`; `pub type LoadedGuard = tokio::sync::OwnedRwLockReadGuard<Option<LoadedIndex>, LoadedIndex>` and `pub async fn read_loaded(self: &Arc<IndexEntry>) -> Option<LoadedGuard>` are this task's)
    with `pub fn touch(&self)`, `pub fn in_use(&self) -> bool` (a reader holds the lock or a
    pass holds `pass_lock`), and the H6 window `pub fn absent_since(&self, hash: &BlobHash,
    now: u64) -> u64` (records the first absence, returns it) and `pub fn seen(&self, hash:
    &BlobHash)` (clears it)
  - `pub struct LoadedIndex { pub manifest_bytes_hash: [u8; 32], pub manifest: Manifest, pub thread: ThreadPart, pub thread_segments: Vec<SegmentId>, pub content: Option<LoadedContent>, pub scanned_to: u64, pub update_records: usize, pub thread_bytes: u64, pub thread_charge: BudgetCharge }`
    with `pub fn identity_version(&self) -> (u64, u32)`, `pub fn empty(manifest: Manifest) -> LoadedIndex` (a zero hash, empty parts, `scanned_to = scanned_seed(&manifest.indexed_to)`; the first load's seed and the unit tests' base) and `pub fn scanned_seed(indexed_to: &Checkpoint) -> u64` (PL4: `indexed_to.change_id` when `complete`, else `indexed_to.change_id - 1`. `scanned_to` means "every record at or below it was consumed whole", and a partial checkpoint's own record was not, so seeding it with that record's change id would let the shortcut answer `Current` with the record's remaining inserts unread)
  - `pub struct BudgetCharge { counter: Arc<AtomicU64>, bytes: u64 }` with `impl Drop` (subtracts `bytes`) no public `resize`: every change of a charge goes through `ZaSearchCache::grow` (up or down, under the admission lock), and `pub fn detached() -> BudgetCharge` (zero bytes on a private counter: `LoadedIndex::empty`'s, the pass's `base` view's and the unit tests'; it is not `Clone`, so a copy of a loaded part never double-counts) (PL11)
  - `pub struct LoadedContent { pub part: ContentPart, pub segments: Vec<SegmentId>, pub bytes: u64, pub charge: BudgetCharge }` (`bytes` is `part.loaded_bytes()`, the one estimate loads, reloads and the swap all use, Codex rev-6 PL11)
    (`segments` head first, the chain as loaded)
- Produces (`load.rs`):
  - `pub trait BlobSource: Sync { fn fetch(&self, hash: &BlobHash) -> impl Future<Output = trc::Result<Option<Vec<u8>>>> + Send; }`
    implemented for `Server` (`self.get_blob(hash.as_slice(), 0..usize::MAX)`, counting
    `za_search_blob_reads` in test mode) and, in tests, for `AHashMap<BlobHash, Vec<u8>>`
  - `pub fn classify_blob_error(err: &trc::Error) -> LoadFailure` (`DecryptError` with
    `Key::Id` → `Transient(CodecKey)`; `DecryptError` without, or `DecompressError` → `Loss
    (Parse)`; anything else → `Transient(BlobStore)`)
  - `pub fn absence_class(first_seen: u64, now: u64, window_secs: u64) -> LoadFailure`
    (`Transient(AbsentWithinWindow)` while `now - first_seen < window_secs`, else `Loss
    (BlobAbsent)`)
  - `pub enum ReloadPlan { Unchanged, Append { thread: Vec<ThreadBlobEntry>, content_head_moved: bool }, Whole }`
    and `pub fn plan_reload(loaded: &LoadedIndex, manifest: &Manifest) -> ReloadPlan`
    (`Whole` when a held thread segment is absent from the manifest; `Append` when
    every held thread segment is still listed, with the unheld entries in manifest order)
  - `pub async fn fetch_verified<S: BlobSource>(source: &S, entry: &IndexEntry, hash: &BlobHash, window_secs: u64, now: u64) -> Result<Vec<u8>, LoadFailure>`
    (PL1, spec 4.2 "a loader authenticates a blob from the blob itself and the descriptor
    that led it there": `fetch`; `Err(e)` → `classify_blob_error(&e)`; `None` →
    `absence_class(entry.absent_since(hash, now), now, window_secs)`; `Some(bytes)` →
    `entry.seen(hash)`, then `BlobHash::generate(&bytes) == *hash` or `Err(Loss(Hash))`.
    The blob store does not recompute the hash on read (`crates/store/src/dispatch/blob.rs:29-45`),
    and a thread blob's hidden section is optional, so without this check a store writer could
    strip a publishing blob's hidden section, keep its public section, and the parse would
    succeed with the records silently gone. Every read of an index blob whose descriptor names
    a hash goes through it: the thread load, the content walk (the head by the manifest's hash,
    each later node by its successor's `predecessor` hash), the reload, and Task 15's merge
    inputs. The rebuild's and destroy's header walks read `0..BlobHeader::LEN` only to find
    what to unlink, never to serve, and stay unverified.)
  - `pub async fn load_thread_part<S: BlobSource>(source: &S, entry: &IndexEntry, key: &Secret, manifest_bytes: &[u8], manifest: Manifest, window_secs: u64, now: u64) -> Result<LoadedIndex, LoadFailure>`
    (fetch each thread blob with `fetch_verified`, `parse_thread_blob` against `BlobHeader { kind: Thread, tier: 0, segment: entry.segment, predecessor: None }` (every thread blob has tier 0 and no predecessor), `ThreadPart::
    extend` in manifest order, tombstones applied, then `extend([], [], [manifest.newest_row], [])` so that `revision_at` sees the newest publication, whose row the manifest carries and no blob does; `scanned_to = scanned_seed(&manifest.indexed_to)`, `update_records = 0`)
  - `pub async fn load_content<S: BlobSource>(source: &S, entry: &IndexEntry, key: &Secret, manifest: &Manifest, stop_at: Option<&[SegmentId]>, window_secs: u64, now: u64) -> Result<LoadedContent, LoadFailure>`
    (walk from the head by each header's predecessor, every node fetched with `fetch_verified`; stop when the next segment is in
    `stop_at`, the incremental case, and when the walk ends at `predecessor: None` without meeting one, replace the loaded content whole (a content compaction happened); `parse_content_blob` with the expected segment of the
    descriptor or the predecessor; `ContentPart::extend`; apply the thread part's tombstones
    by removing their documents)
  - `impl Server`:
    - `pub async fn za_search_read_manifest_raw(&self, account_id: u32) -> trc::Result<Option<Vec<u8>>>`
    - `pub async fn za_search_read_manifest(&self, account_id: u32, key: &ZaSearchKey) -> Result<Option<(Vec<u8>, ManifestValue)>, LoadFailure>`
      (store error → `Transient(Store)`; `ManifestError::{Tag, Parse}` → `Loss(Tag)` /
      `Loss(Parse)`; `OlderFormat` → `Loss(OlderFormat)`; `NewerFormat` → `NoMap(NewerFormat)`)
    - `pub async fn za_search_entry(&self, account_id: u32, keys: &SessionKeys) -> Result<Arc<IndexEntry>, LoadFailure>`
      (the entry, loaded under `load_flight` when `index` is `None`: a missing manifest is
      `Loss(ManifestMissing)`, a `Destroying` marker `NoMap(Destroying)`; the thread budget
      is asked through `admit(Thread, 3 × the manifest's thread blob sizes, account)`, always
      granted, with `ZA_SEARCH_OVER_BUDGET` traced when `thread_used()` is then over the budget,
      H8; the loaded part's `thread_bytes` is `thread.loaded_bytes()`, and `grow` sets the charge
      to it)
    - `pub async fn za_search_reload_if_moved(&self, entry: &IndexEntry, keys: &SessionKeys) -> Result<bool, LoadFailure>`
      (reads the manifest; `manifest_hash` equal → `Ok(false)`; else `plan_reload` and apply
      under `index.write()`; `Whole` reloads the thread part and, when content was loaded,
      the content too (the old content part dropped, its charge with it, before the new
      one's `admit`, so a chain near the budget can replace itself, FV8); and before either arm, whatever `plan_reload` answers, a manifest whose `content` is `None` drops the resident content part with its charge (FW3, FX1: another node's `ContentLoss` start left it stale, and that reload is an `Append` when the thread segments are unchanged); `Append` extends; the new `manifest_bytes_hash`, `manifest` and
      `scanned_to = max(old, scanned_seed(&manifest.indexed_to))` are set; `Ok(true)`)
    - `pub async fn za_search_load_content(&self, entry: &IndexEntry, keys: &SessionKeys) -> Result<(), LoadFailure>`
      (`Err(OverCap)` when `manifest.content.size > account_max_mb << 20`; `admit(Content,
      3 × size, account)` or `Err(Transient(Budget))`; `load_content` from the head, walked outside `index.write()` (FX3: the call answers at once only when the resident part's head segment is the loaded manifest's head, and otherwise loads and replaces it; under `index.write()` it installs the part only when `index.manifest.content`'s head is still the head it walked from, and otherwise walks again from the new head with `stop_at` set to the segments it holds and installs once they agree, since a pass's swap during the walk moves the head); then
      `grow(&mut charge, Content, part.loaded_bytes(), account)` sets the charge to the loaded
      size (down from the 3× ask in the common case; when `loaded_bytes()` exceeds the ask and
      `grow` answers `false`, the part is dropped and the load answers `Err(Transient(Budget))`,
      as a refused admission does, FV8); sets `content` under `index.write()`)
  - Each `Server` method returns at once with `Err(NoMap(Transient(..)))`-free semantics: a
    transient fault is `Err(Transient(..))`, and the caller (Tasks 8, 9, 11) decides between
    "no map" and the unavailable refusal.

- [ ] **Step 1: Write the failing unit tests** in `crates/common/src/za/search/load_tests.rs`
  (declared from `load.rs`; `#[tokio::test]`). Fixture: `fn fixture() -> (Secret, u32, AHashMap
  <BlobHash, Vec<u8>>, Vec<u8> /* manifest bytes */, Manifest)` building two thread blobs
  (pass 1: records 1 to 3 in one thread, row (1, 100); pass 2: record 4 joining with alias
  none, a tombstone of 2 at 150, row (2, 200)) and a content chain of two nodes (node 1
  with records 1 to 3, node 2 with record 4, predecessor node 1), hashes `blake3` of the
  bytes as `BlobHash`, every blob inserted into the map, and the `Manifest` naming them;
  `fn counting(map) -> CountingSource` wraps the map and counts fetches per hash. Tests:
  - `s43_round_trip_a_populated_index`: `load_thread_part` → `thread.present_members(1) ==
    [1, 3, 4]`, `record(2).deleted == Some(150)`, `rows() == [(1, 100), (2, 200)]`,
    `scanned_to == manifest.indexed_to.change_id`; `load_content` → `part.contains(1)`,
    `contains(4)`, `!contains(2)` (tombstone applied), `segments == [seg2, seg1]`.
  - `r9_thread_load_fetches_no_content_blob`: after `load_thread_part`, the counter of both
    content hashes is 0.
  - `r11_chain_is_walked_from_the_head_descriptor`: `load_content` fetches node 2 then node
    1 and nothing else; a map where node 1's bytes are replaced, under node 1's hash, by a blob
    of another segment → `Err(Loss(Hash))` (the descriptor's hash is checked before the parse,
    PL1; Codex re-review PL1); and the header mismatch on its own: a blob of another segment
    stored under its real hash, with node 2 rebuilt so that its header's predecessor names
    that hash and node 1's segment → `Err(Loss(Parse))` (the hash agrees, the segment does not).
  - `pl1_a_stripped_hidden_section_is_loss`: take pass 1's publishing blob, rebuild its bytes
    as `header.encode() | 0u32 LE | public_sealed` (the hidden section removed, the public
    section's ciphertext kept, so `parse_thread_blob` alone would accept it), and store them
    in the map under pass 1's original hash: `load_thread_part` → `Err(Loss(Hash))`; content
    node 1's bytes replaced, under node 1's hash, by `build_content_blob` with node 1's own
    header and records 1 and 2 only (a validly sealed blob the parse would accept) →
    `load_content` returns `Err(Loss(Hash))`; the unmodified fixture still loads.
  - `f20_incremental_reload_equals_a_fresh_load`: load v1; build v2 = v1 + a third thread
    blob (record 5) + a third chain node (record 5, predecessor node 2); `plan_reload` is
    `Append { thread: [entry3], content_head_moved: true }`; applying it (the same code path
    `za_search_reload_if_moved` uses, factored as `pub async fn apply_reload<S: BlobSource>(source: &S, entry: &IndexEntry, key: &Secret, config: &ZaSearchConfig, manifest_bytes: &[u8], manifest: Manifest, now: u64) -> Result<(), LoadFailure>`) fetches
    exactly the two new blobs, and the result equals a fresh load of v2 in `thread.records()`,
    `aliases()`, `rows()`, `content.part.document_ids()` and `manifest_bytes_hash`.
  - `f20_compaction_reloads_whole`: v3 replaces the three thread blobs with one and the chain
    with one node: `plan_reload == Whole`; after applying, held segments are `[seg_new]`
    and the result equals a fresh load of v3.
  - `r18_a_deletion_only_change_moves_the_hash_and_reloads`: v2' = v1 + a non-publishing
    blob (tombstone of 3, no hidden section, no row): identity versions equal, `manifest_hash`
    differs, `plan_reload` is `Append`, and after it `thread.is_present(3) == false`.
  - `r2_identity_version_distinguishes_epochs`: two manifests with `revision 5` and epochs
    `100` and `900`: `identity_version()` differs; equal revision and epoch → equal.
  - `f3_h6_error_classes`: `classify_blob_error(&trc::StoreEvent::BlobNotFound.into_err())`
    → `Transient(BlobStore)`; `DecryptError` with `.ctx(trc::Key::Id, 7u64)` → `Transient
    (CodecKey)`; `DecryptError` with details only → `Loss(Parse)`; `DecompressError` →
    `Loss(Parse)`; `absence_class(100, 105, 10) == Transient(AbsentWithinWindow)`;
    `absence_class(100, 111, 10) == Loss(BlobAbsent)`; a manifest naming a hash the map
    lacks: the first `load_thread_part` at `now = 100` → `Err(Transient(AbsentWithinWindow))`,
    at `now = 111` → `Err(Loss(BlobAbsent))`; inserting the blob between the two calls →
    `Ok` and `entry.absent_since` cleared.
  - `h11_f7_g12_manifest_outcomes`: `za_search_read_manifest`'s mapping tested on the pure
    function `pub fn classify_manifest(decoded: Result<ManifestValue, ManifestError>) ->
    Result<ManifestValue, LoadFailure>`: `Err(NewerFormat(2))` → `NoMap(NewerFormat)`,
    `Err(OlderFormat(0))` → `Loss(OlderFormat)`, `Err(Tag)` → `Loss(Tag)`, `Err(Parse)` →
    `Loss(Parse)`, `Ok(Destroying(..))` stays `Ok` for the caller to map to `NoMap
    (Destroying)` (assert `za_search_entry`'s mapping through a `pub fn entry_outcome
    (value: Option<ManifestValue>) -> Result<Manifest, LoadFailure>`: `None` → `Loss
    (ManifestMissing)`, `Destroying` → `NoMap(Destroying)`).
  - `f18_budget_counts_the_transient_and_evicts_idle_content_first`: `ZaSearchCache::new
    (config with mem_mb = 1, thread_mem_mb = 1)`; entries for accounts 1 and 2 with content
    of 300 KiB each, admitted through `admit(Content, 900 KiB, 1)` then `admit(Content, 900 KiB,
    2)` (the second evicts account 1's content: `get(1).index...content.is_none()`,
    `content_used` back under the budget); when account 1's reader holds `index.read()`
    (`in_use`), `admit(Content, 900 KiB, 3)` for a third account returns `None`.
  - `pl11_an_evicted_generation_stays_charged_until_its_last_reader_drops`: `ZaSearchCache::new(config with mem_mb = 1)`; account 1's entry loaded with a 300 KiB content part (`set_content_bytes_for_test`, which admits the bytes through `admit` and stores the charge on the entry, FV11); a reader holds `entry.read_loaded()`; `remove_account(1, Invalidation)` → `content_used() == 300 KiB` still; a new entry for account 1 admitted at 300 KiB, a second reader holding it → `content_used() == 600 KiB` and `admit(Content, 600 KiB, 2).is_none()` (both generations in use, nothing evictable, `600 + 600 > 1024`); drop the local `Arc<IndexEntry>` handle of the first generation (the map no longer holds it after `remove_account`, so the reader is its last owner) and then the first reader → `content_used() == 300 KiB` and `admit(Content, 600 KiB, 2).is_some()` (FR4: the assertion is on the counter, and the option only where nothing is evictable; drop that charge at once); `remove_account(1, Invalidation)` again, drop the second handle and the second reader → `0` (Codex re-review PL11: a generation is freed by its last owner, the map and every local handle included, so the test releases each before expecting the counter to move). Then `pl11_growth_through_local_passes_is_charged` (FU4: these two tests drive Task 6's `admit_content_growth` on a `LoadedIndex` the test builds, never Task 8's swap): an entry loaded with a 100 KiB content part; five rounds of `index.content.part.insert(..)` with records whose `loaded_bytes` growth is 40 KiB (new posting keys among them) each followed by `admit_content_growth(&mut index, 1)` → `true` each time and `content_used() == 300 KiB`; `admit(Content, 800 KiB, 2)` evicts it (not in use) and answers `Some` with `content_used() == 800 KiB`. And `pl11_growth_across_accounts_is_admitted_against_the_process_budget` (Codex rev-6 PL11): `mem_mb = 1`, `account_max_mb = 1` is refused by the boot rule, so the test builds the config directly with `account_max_mb: 1`; four accounts each loaded with a 200 KiB content part (800 KiB, under the budget); account 1's part grows by 300 KiB (its 500 KiB still under its cap) and `admit_content_growth(&mut index1, 1)` → the least-recently-used unheld part of another account is evicted, the growth is admitted, `true`, and `content_used() <= 1024 KiB`; on a fresh cache of the same shape (FU11) with every other part held by a reader, the same growth is refused: `false`, account 1's `content` is `None`, and `content_used() == 600 KiB`, the three other parts. And `pl11_concurrent_growth_is_admitted_atomically` (Codex rev-7 PL11): a 1 MiB budget, two accounts holding 400 KiB each (800 KiB), each `LoadedIndex` living outside the map with a charge from `admit`, while the map's two entries are what readers hold so neither is evictable (FV7: `admit_content_growth` takes `&mut LoadedIndex`, which no thread can hold while a reader holds the entry's lock, so the test is a `#[test]` over std threads); two threads behind a `std::sync::Barrier` each grow their account's part by 150 KiB and call `admit_content_growth`; over 200 rounds (the parts rebuilt each round), every round admits exactly one (`800 + 150 <= 1024`, `950 + 150 > 1024`), the refused account's `content` is `None`, and `content_used()` never exceeds 1024 KiB.
  - `fx1_a_reload_to_a_manifest_without_content_drops_the_part`: v1 loaded with content; v2 the same thread blobs with `content: None` → `plan_reload == Append`, and after `apply_reload`, `content.is_none()` and `content_used() == 0` (FX1).
  - `fx3_a_load_racing_a_swap_installs_the_current_head`: in `g6`'s pattern, a source whose first content fetch parks while the test swaps in a manifest with a newer head (one more chain node); released, the installed part's `segments.first()` is the index's manifest head and it holds the newer node's records (FX3).
  - `h8_thread_parts_are_never_refused`: `admit(Thread, 2 MiB, 1)` on a 1 MiB thread budget
    answers `Some` with `thread_used() == 2 MiB`, and `grow(&mut that, Thread, 3 MiB, 1)` answers
    `true`; `pub fn thread_budget_outcome(fits: bool) -> Option<&'static str>` returns
    `Some(ZA_SEARCH_OVER_BUDGET)` when `!fits`; and thread parts are never evicted by
    `admit(Content, ..)`.
  - `g6_loads_are_single_flight`: two `tokio::spawn`ed loads of one entry through the
    counting source with a `tokio::time::sleep(50ms)` inside the source's first fetch; the
    counter of the first thread blob is 1 and both tasks see the same `manifest_bytes_hash`.
  - `s43_idle_sweep_and_invalidation_drop_entries`: `sweep_idle` with `idle_secs = 1` after
    `last_used` set 2 s back drops the entry and subtracts its bytes; `remove_account(1,
    Invalidation)` drops it; `get(1) == None`.
  - `f16_never_swapped_is_pinned_by_the_type`: a compile-time check: `fn assert_not_swap
    <T>() {}` cannot express a negative bound on stable, so this test is a doc test of the
    reviewer's duty instead: the test asserts that `SwapCached`'s implementors in
    `crates/common/src/cache/swap/evict.rs` are exactly `MessageStoreCache` and
    `GroupwareResources` by reading the file with `include_str!` and counting `impl
    SwapCached for` occurrences (2). A third implementation fails the test.

- [ ] **Step 2: Run the tests to see them fail**
  ```bash
  cargo test -p common --lib za::search::load 2>&1 | tail -10
  ```
  Expected: compile error, `could not find load in search`.

- [ ] **Step 3: Implement `error.rs` and `cache.rs`.** Byte accounting: the private `make_room`, called only under
  `admission`, reads `thread_bytes` and `content.bytes` of each entry under a short read lock
  try (`try_read`), treating an entry whose lock is held as in use; `admit`, `grow` and
  `admit_content_growth` are synchronous and never await under the lock. `sweep_idle` uses
  `store::write::now()` seconds. Traces: `ZA_SEARCH_EVICTED` through the fixed-constant
  convention (`za_search_trace!(ZA_SEARCH_EVICTED, AccountId = account_id, Reason = reason.as_str())`, Task 2's macro on the event the release 5
  constants use, `crates/common/src/za/imip.rs`, with `AccountId` and `Details = constant`
  and `Reason = reason.as_str()`; never an id other than the account's).

- [ ] **Step 4: Implement `load.rs`.** The `Server` methods take `SessionKeys` and derive
  `za_search_key(keys.swk())` for the manifest and pass `keys.swk()` to the section
  functions. `za_search_entry` loads under `load_flight` and re-checks `index` after taking
  it (the second waiter finds it loaded, G6). `plan_reload` compares `loaded.thread_segments`
  with `manifest.thread_blobs` by segment id in order. The `Append` path fetches the unheld
  thread entries and, when content is loaded, the manifest names a head and the head moved, calls `load_content` with
  `stop_at = &loaded.content.segments` (FX1: a manifest without a head has already dropped the part). After a `Whole` reload with content previously
  loaded, content is reloaded only if `admit(Content, 3 × size, account)` answers `Some`;
  otherwise `content` is dropped with `ZA_SEARCH_EVICTED` (`Budget`) and the next content
  request reloads it (4.3); an `Append` reload that extends a loaded content part ends in
  `admit_content_growth`, which drops the part when its growth is refused.

- [ ] **Step 5: Wire the cache**: `pub za_search: crate::za::search::cache::ZaSearchCache,`
  in `Caches` after `za_keys` (`lib.rs:230`); in `config/inner.rs:140` after the `za_keys`
  line: `za_search: ZaSearchCache::new(ZaSearchConfig::from_env())` (Task 2's line, unchanged: the refusal is checked in `main`, plan call A5);
  `cache.za_search.remove_account(*id, EvictReason::Invalidation);` after each
  `cache.za_keys.remove_account(*id);` at `invalidate.rs:615` and `:642`.

- [ ] **Step 6: Run the tests to see them pass**
  ```bash
  cargo test -p common --lib za::search 2>&1 | tail -10
  ```
  Expected: `test result: ok.` with 21 load tests and Task 5's seal tests.

- [ ] **Step 7: Format, check (shipping configuration), commit**
  ```bash
  cargo fmt -p common -- --check
  log="${TMPDIR:-/tmp}/za-check.log"; cargo check -p stalwart --no-default-features --features rocks > "$log" 2>&1 || { tail -40 "$log"; echo BUILD FAILED; false; }
  grep -E "^(warning|error)" -A3 "$log" | grep -E "crates/common/src/za/search|crates/store/src/za_search|crates/common/src/lib.rs|crates/common/src/config/inner.rs|crates/common/src/cache/invalidate.rs" || echo clean
  git add crates/common/src/lib.rs crates/common/src/config/inner.rs crates/common/src/cache/invalidate.rs crates/common/src/za/search/
  git commit -m "Search index: per-process cache with budgets and eviction, loader with incremental reload and error classes (release 7, task 6)"
  ```

### Task 7: The record reader, the openers and the `K` capture

**Files:**
- Modify: `crates/store/src/query/log.rs:117-184` (add `Store::za_changes_records`, `Store::za_changes_records_range`, `Store::za_log_first_retained` and `ZaChangeRecords` beside `changes`; include the tests with `#[cfg(test)] #[path = "log_za_tests.rs"] mod za_tests;`)
- Create: `crates/store/src/query/log_za_tests.rs`
- Create: `crates/email/src/message/za/search/mod.rs` (declares `fields`, `open`; later tasks add `pass`, `write`, `ensure`, `snapshot`, `changes`, `rebuild`, `compact`)
- Create: `crates/email/src/message/za/search/fields.rs`, `crates/email/src/message/za/search/open.rs`
- Modify: `crates/email/src/message/za/mod.rs:9-14` (add `pub mod search;`)
- Test: unit tests inline in `fields.rs` and `open.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `Checkpoint { change_id: u64, position: u32, complete: bool }` (Task 3's `mod.rs`; `complete == true` means the whole record is consumed and `position` is `u32::MAX`); `ThreadFieldsK { hashes: Vec<u128>, subject_hash: u128, sent_at: Option<i64> }` and `K` (Task 3); `LossClass`, `TransientClass` (Task 6, `crates/common/src/za/search/error.rs`); the hooks `SEARCH_OPEN_FAIL_ONCE`, `SEARCH_BLOB_FAIL_ONCE` and the counter `za_search_message_opens` (Task 2, `common::za::test_hooks`); upstream: `Change` and `Changes::{deserialize, finalize}` (`crates/store/src/query/log.rs:25-35, 432-455, 418-430`), `LogCollection::{log_key, is_prefixed}` (`crates/store/src/write/mod.rs:600-650`), the truncation marker (an empty value at the first retained key, `crates/common/src/storage/transaction.rs:228-244`), `ZaMailView::{za_open_metadata_row, za_open_message_blob}` (`crates/email/src/message/za/view.rs:95-118`), `za_has_attachment` (`view.rs:411`), `ZA_REASON_UNREADABLE` (`view.rs:43`), `ThreadFields::scan` (`crates/email/src/message/thread.rs:30-64`), `BlobKeyring::unavailable_key`'s `DecryptError` with `Key::Id` (`crates/store/src/write/blob_codec/mod.rs:216-222`).
- Produces (store):
  ```rust
  pub struct ZaChangeRecords {
      pub records: Vec<(u64, Vec<Change>)>, // (change id, the record's changes, unfolded), ascending
      pub truncated: bool,                   // a marker (empty value) was met in the range read
      pub resumed_record_lost: bool,         // the marker sits at the resumed partial record's own change id
      pub first_retained: Option<u64>,       // the truncation marker's change id (the oldest surviving key, `truncate_change_log`'s empty value); `None` when the log holds no marker
  }
  impl Store {
      pub async fn za_changes_records(&self, account_id: u32, collection: SyncCollection, from: Option<Checkpoint>, max_records: usize) -> trc::Result<ZaChangeRecords>;
      pub async fn za_changes_records_range(&self, account_id: u32, collection: SyncCollection, from_exclusive: u64, to_inclusive: u64) -> trc::Result<ZaChangeRecords>;
      pub async fn za_log_first_retained(&self, account_id: u32, collection: SyncCollection) -> trc::Result<Option<u64>>;
  }
  ```
  An item id in an Email record is `(thread_id << 32) | document_id` (`build_id`, `crates/store/src/write/log.rs:299-314`); consumers take `id as u32`.
- Produces (email, `crates/email/src/message/za/search/`):
  ```rust
  pub fn za_thread_fields_k(message: &mail_parser::Message<'_>) -> ThreadFieldsK;      // fields.rs
  pub struct OpenedMessage { pub row: MetadataRow, pub raw: Zeroizing<Vec<u8>>, pub bytes: usize, pub received_at: u64 } // open.rs; received_at: PL10
  impl OpenedMessage { pub fn parse(&self) -> Option<mail_parser::Message<'_>>; }    // MessageParser::new().parse(&self.raw)
  pub enum OpenFailure { Loss(LossClass), Transient(TransientClass) }
  pub fn classify_open_error(err: &trc::Error) -> OpenFailure;                           // open.rs, pure
  impl Server { pub async fn za_open_for_index(&self, account_id: u32, document_id: u32, keys: &SessionKeys, entry: &IndexEntry, window_secs: u64, now: u64) -> Result<OpenedMessage, OpenFailure>; } // entry, window_secs, now: the H6 window, FR6
  ```

- [ ] **Step 1: Write the failing store tests** in `crates/store/src/query/log_za_tests.rs` (set the store up as `crates/store/src/search/index_za_tests.rs` does; read its first 40 lines first). Log through `BatchBuilder` with `with_account_id(7).with_collection(Collection::Email).with_document(d).log_item_insert(SyncCollection::Email, Some(PendingId::Assigned(d)))` and `log_item_delete` the same way, one `write_batch` per record:

```rust
#[tokio::test]
async fn records_are_whole_and_unfolded() {
    // record c1: insert 1, insert 2; record c2: insert 3, delete 1
    let read = store.za_changes_records(7, SyncCollection::Email, None, 10).await.unwrap();
    assert_eq!(read.records.len(), 2);
    assert!(!read.truncated && !read.resumed_record_lost);
    let (c1, r1) = &read.records[0];
    assert_eq!(r1.iter().filter(|c| matches!(c, Change::InsertItem(_))).count(), 2);
    let (c2, r2) = &read.records[1];
    assert!(c2 > c1);
    assert!(r2.contains(&Change::InsertItem((3u64 << 32) | 3)));
    assert!(r2.contains(&Change::DeleteItem((1u64 << 32) | 1)), "unfolded: the delete of 1 is kept");
    // the folded reader cancels the pair
    let folded = store.changes(7, SyncCollection::Email.into(), Query::All).await.unwrap();
    assert!(!folded.changes.iter().any(|c| c.item_id() == Some((1u64 << 32) | 1)));
    assert_eq!(read.first_retained, None, "no marker: the log was never truncated");
}

#[tokio::test]
async fn resume_is_inclusive_for_a_partial_record_and_exclusive_for_a_complete_one() {
    let partial = Checkpoint { change_id: c1, position: 1, complete: false };
    let read = store.za_changes_records(7, SyncCollection::Email, Some(partial), 10).await.unwrap();
    assert_eq!(read.records[0].0, c1);
    let complete = Checkpoint { change_id: c1, position: 0, complete: true };
    let read = store.za_changes_records(7, SyncCollection::Email, Some(complete), 10).await.unwrap();
    assert_eq!(read.records[0].0, c2);
}

#[tokio::test]
async fn the_truncation_marker_is_reported_and_the_lost_record_named() {
    // three records c1 < c2 < c3; truncate as `truncate_change_log` does: an empty value at c2, the range below c2 deleted
    let read = store.za_changes_records(7, SyncCollection::Email, None, 10).await.unwrap();
    assert!(read.truncated);
    assert_eq!(read.first_retained, Some(c2));
    assert_eq!(read.records.iter().map(|(c, _)| *c).collect::<Vec<_>>(), vec![c3]);
    let resumed = store.za_changes_records(7, SyncCollection::Email, Some(Checkpoint { change_id: c2, position: 0, complete: false }), 10).await.unwrap();
    assert!(resumed.truncated && resumed.resumed_record_lost);
    let after = store.za_changes_records(7, SyncCollection::Email, Some(Checkpoint { change_id: c2, position: 0, complete: true }), 10).await.unwrap();
    assert!(!after.truncated, "an exclusive resume at the marker skips it, as upstream's `Since` does");
    assert_eq!(store.za_log_first_retained(7, SyncCollection::Email).await.unwrap(), Some(c2));
    assert_eq!(store.za_log_first_retained(8, SyncCollection::Email).await.unwrap(), None);
}

#[tokio::test]
async fn max_records_bounds_the_read_and_the_range_form_is_closed_at_its_end() {
    let read = store.za_changes_records(7, SyncCollection::Email, None, 1).await.unwrap();
    assert_eq!(read.records.len(), 1);
    let range = store.za_changes_records_range(7, SyncCollection::Email, c1, c2).await.unwrap();
    assert_eq!(range.records.iter().map(|(c, _)| *c).collect::<Vec<_>>(), vec![c2]);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `export PATH="/opt/homebrew/opt/rustup/bin:$PATH" && cargo test -p store --lib za_changes`
Expected: compile error, `no method named za_changes_records`.

- [ ] **Step 3: Implement the reader** in `log.rs` after `changes` (`:184`). Procedure, the same iteration `changes` makes (`:142-179`) without `Changes`' cross-record folding:
  - `let collection = LogCollection::Sync(collection); let is_prefixed = collection.is_prefixed();`
  - `za_changes_records`: `(from_change_id, inclusive) = match from { None => (0, true), Some(cp) => (cp.change_id, !cp.complete) }`; keys `collection.log_key(account_id, from_change_id)` to `collection.log_key(account_id, u64::MAX)`, ascending. For each `(key, value)`: `change_id = key.deserialize_be_u64(key.len() - U64_LEN)?`; if `!inclusive && change_id == from_change_id` → `Ok(true)` (skip the from-key); if `value.is_empty()` → `first_retained = Some(change_id)` (the marker's key only, as the contract says; a log without a marker answers `None`), `truncated = true`, and `resumed_record_lost = true` when `inclusive && change_id == from_change_id`, `Ok(true)`; else `let mut record = Changes::default(); record.deserialize(value, is_prefixed).ok_or_else(|| trc::Error::corrupted_key(key, value.into(), trc::location!()))?; record.finalize(); records.push((change_id, record.changes));` and `Ok(records.len() < max_records)`. (Within one record an id sits in one list only, `log_item_insert` at `write/log.rs:369-382`, so a fresh `Changes` per record is unfolded.)
  - `za_changes_records_range`: the same with keys `log_key(account_id, from_exclusive)` to `log_key(account_id, to_inclusive)`, skipping `change_id == from_exclusive`, no `max_records`.
  - `za_log_first_retained`: `IterateParams::new(log_key(account_id, 0), log_key(account_id, u64::MAX)).ascending().only_first()` with values, `Some(change_id)` only when the first key's value is the empty marker, else `None`, as `get_last_change_id` does descending (`:308-333`).

- [ ] **Step 4: Run the store tests**

Run: `export PATH="/opt/homebrew/opt/rustup/bin:$PATH" && cargo test -p store --lib za_changes`
Expected: 4 passed.

- [ ] **Step 5: Write the failing email tests** in `fields.rs` and `open.rs`:

```rust
// fields.rs
#[test]
fn the_capture_keeps_own_id_parent_then_nearest_references_before_any_sort() {
    // Message-ID <m>, In-Reply-To <p>, References <r1> .. <r20>, Resent-Message-ID <x>
    let fields = za_thread_fields_k(&message);
    assert_eq!(fields.hashes.len(), K);
    let h = |id: &str| xxh3_128(format!("<{id}>").as_bytes());
    let mut expected = vec![h("m"), h("p")];
    expected.extend((7..=20).rev().map(|i| h(&format!("r{i}")))); // r20 .. r7
    assert_eq!(fields.hashes, expected, "none of r1..r6 and not x");
}
#[test]
fn a_short_chain_keeps_everything_once_and_resent_last() {
    // References <r1> <r2> <r3>, In-Reply-To <r3>, Resent-Message-ID <x>
    assert_eq!(fields.hashes, vec![h("m"), h("r3"), h("r2"), h("r1"), h("x")]);
}
#[test]
fn subject_and_sent_at_are_upstreams() {
    let scanned = ThreadFields::scan(&message);
    assert_eq!(fields.subject_hash, xxh3_64(scanned.subject.as_bytes()));
    assert_eq!(fields.sent_at, scanned.sent_at);
}
// open.rs
#[test]
fn open_errors_are_classified() {
    assert!(matches!(classify_open_error(&trc::StoreEvent::NotFound.into_err()), OpenFailure::Loss(LossClass::StubAbsent)));
    assert!(matches!(classify_open_error(&za_unreadable(SealError::Aead, 1, 2) /* `crates/email/src/message/za/view.rs:378` */), OpenFailure::Loss(LossClass::Aead)));
    assert!(matches!(classify_open_error(&trc::StoreEvent::DecryptError.into_err().ctx(trc::Key::Id, 3u32)), OpenFailure::Transient(TransientClass::CodecKey)));
    assert!(matches!(classify_open_error(&trc::StoreEvent::DecryptError.into_err()), OpenFailure::Loss(LossClass::Parse)));
    assert!(matches!(classify_open_error(&trc::StoreEvent::DecompressError.into_err()), OpenFailure::Loss(LossClass::Parse)));
    assert!(matches!(classify_open_error(&trc::StoreEvent::UnexpectedError.into_err()), OpenFailure::Transient(TransientClass::Store)));
}
```

- [ ] **Step 6: Run them to see them fail**

Run: `export PATH="/opt/homebrew/opt/rustup/bin:$PATH" && cargo test -p email --lib --features test_mode za::search`
Expected: compile errors, `za_thread_fields_k` and `classify_open_error` not found.

- [ ] **Step 7: Implement `za_thread_fields_k`** (G10). Take `subject` and `sent_at` from `ThreadFields::scan(message)` (`thread.rs:30-64`); `subject_hash = xxh3_128(scanned.subject.as_bytes())` (`u128`, upstream's `thread_hash` width, `ingest.rs:1330-1338`; mail_parser's Message-ID values carry no angle brackets, and `scan` hashes `id.as_bytes()` as given). Then build `hashes` in header order, not `scan`'s reversed one: the own Message-ID is `scanned.message_id` (the one `scan` chose) hashed with `xxh3_128`; then every `In-Reply-To` value in header order; then every `References` value from the last to the first; then every `Resent-Message-Id` value; each `visit_text` value with the brackets as `scan` hashes them (`id.as_bytes()` of the raw id string); dedup keeping the first occurrence; `truncate(K)`.

- [ ] **Step 8: Implement `za_open_for_index` and `classify_open_error`**. Procedure:
  1. Test mode: `if (search_open_fail_all(account_id) || take_search_open_fail(account_id)) { return Err(Loss(LossClass::Aead)) }`; `if take_search_blob_fail(account_id) { return Err(Transient(TransientClass::BlobStore)) }`.
  2. Read the stored row: `self.store().get_value::<MetadataRow>(ValueKey::immutable(account_id, Collection::Email, document_id, EmailField::Metadata))` (as `tests/src/za/mail/mod.rs`'s `metadata_key` builds it; find the production reader with `grep -n "EmailField::Metadata" crates/email/src/message/*.rs`): `Ok(None)` → `Loss(StubAbsent)`; `Err(e)` → `classify_open_error(&e)`. Beside it read the document's `MessageData` archive (`crates/email/src/message/messagedata.rs:43-52`, the archive the message cache is built from; find its key and reader with `grep -n "EmailField::Archive" crates/email/src/cache/*.rs`) for `received_at` (PL10), with the same `None` and error mapping.
  3. `self.za_open_metadata_row(&stored, Some(keys), account_id, document_id).await`: `Ok(Some(row))` is the opened row; `Ok(None)` uses `stored` as it is; `Err(e)` → `classify_open_error(&e)`.
  4. `let hash = row.unarchive()?.blob_hash()` (the message blob's hash the stub names, known only here, FR6); `self.get_blob(hash.as_slice(), 0..usize::MAX).await`: `Ok(None)` → `match absence_class(entry.absent_since(&hash, now), now, window_secs) { LoadFailure::Transient(c) => Err(Transient(c)), LoadFailure::Loss(c) => Err(Loss(c)), _ => unreachable!() }` (Task 6's H6 window, keyed by that hash: `Transient(AbsentWithinWindow)` inside it, `Loss(BlobAbsent)` past it); `Ok(Some(bytes))` → `entry.seen(&hash)` first; `Err(e)` → `classify_open_error(&e)`.
  5. `self.za_open_message_blob(opened, Some(keys), account_id, document_id, bytes).await`: `Err(e)` → `classify_open_error(&e)`; `Ok(raw)`: `bytes = raw.len()`; in test mode `count_za_search_message_open(account_id)`; `Ok(OpenedMessage { row, raw, bytes, received_at })`.
  - `classify_open_error`: `Store(NotFound)` → `Loss(StubAbsent)`; `Store(DataCorruption)` with `Key::Reason == ZA_REASON_UNREADABLE` → `Loss(Aead)`, any other `DataCorruption` → `Loss(Parse)`; `Store(DecryptError)` with a `Key::Id` value → `Transient(CodecKey)` (the keyring's missing key, `blob_codec/mod.rs:216-222`), without one → `Loss(Parse)` (a truncated encrypted blob, `:224-230`); `Store(DecompressError)` → `Loss(Parse)`; everything else → `Transient(Store)`.

- [ ] **Step 9: Run the email tests**

Run: `export PATH="/opt/homebrew/opt/rustup/bin:$PATH" && cargo test -p email --lib --features test_mode za::search`
Expected: 4 passed.

- [ ] **Step 10: Compile check and format**

Run: `cargo check -p stalwart --no-default-features --features rocks` (the Global Constraints form) and `cargo fmt -p store -p email -- --check`.
Expected: clean.

- [ ] **Step 11: Commit**

```bash
git add crates/store/src/query/log.rs crates/store/src/query/log_za_tests.rs crates/email/src/message/za/mod.rs crates/email/src/message/za/search/
git commit -m "Search: the unfolded change-log reader, the index openers and the K-bounded thread fields"
```

### Task 8: The pass and setup's empty manifest

**Files:**
- Create: `crates/email/src/message/za/search/pass.rs` (steps 0–2: `za_search_pass`, `Opener`, `Staging`, `plan_pass`), `crates/email/src/message/za/search/write.rs` (step 3: `stage_blobs`, `build_batch`, `commit_and_swap`)
- Modify: `crates/email/src/message/za/search/mod.rs` (declare `pass`, `write`)
- Modify: `crates/common/src/auth/vault.rs:202-237` (`vault_batch_write`), `:363-372` (`vault_commit_store`), `:518-528` (`za_vault_commit`): a trailing `extra: Option<ZaVaultExtra>` parameter; every other caller passes `None` (find them with `grep -rn "za_vault_commit\|vault_commit_store" crates`)
- Create: `crates/common/src/za/search/setup.rs` (`za_setup_manifest`)
- Modify: `crates/http/src/api/vault.rs:485-507` (`za_commit` gains `extra`), `:1081-1139` (`za_setup` builds the manifest)
- Modify: `crates/common/src/za/test_hooks.rs` (the pause points of `SEARCH_SWEEP_PAUSE`, `SEARCH_PASS_PAUSE`, `SEARCH_COMMIT_PAUSE`, `SEARCH_SWAP_PAUSE`, `SEARCH_HOLD_EXPIRE`, `SEARCH_BLOB_SLOW` are called from `pass.rs`/`write.rs`; the statics are Task 2's)
- Create: `tests/src/za/mail/index.rs`; Modify: `tests/src/za/mail/mod.rs` (`pub mod index;`), `tests/src/za/mod.rs:224` (`mail::index::test(&mut test).await;` after `mail::search::test`), `tests/src/utils/za.rs` (helpers below)
- Test: unit tests inline in `pass.rs` and in `write.rs`'s `mod tests` (FX11); the za cases in `tests/src/za/mail/index.rs`

**Interfaces:**
- Consumes: Task 7's reader, `OpenedMessage` (with `received_at`, PL10), `OpenFailure`, `za_thread_fields_k`; Task 6's `scanned_seed`, `BudgetCharge::detached`, `PartKind` and `ZaSearchCache::{admit, grow, admit_content_growth}` (PL4, PL11, FV11); Task 3's `ThreadPart::{assign, tombstone, exists_at, current_thread}`, `Assignment`, `ThreadRecord`, `Alias`, `Tombstone`, `RevisionRow`; Task 4's `ContentRecord::new(IndexDocument, u32, SortKeys, Option<i64>, bool)` and `SortKeys` (this task implements `From<&MessageSortKeys> for SortKeys` in `email`); Task 5's `BlobHeader`, `BlobKind`, `SectionKind`, `SegmentId::random()`, `HiddenSection`, `PublicSection`, `PadTarget`, `build_thread_blob`, `build_content_blob`; Task 2's `padded_len`, `reservation()`; `Manifest`, `ManifestValue`, `ManifestWrite::new(value, key, ManifestFill)`, `manifest_hash`, `ZaSearchKey`, `ContentHead`, `ThreadBlobEntry`, `ContentTo`, `FailingDocument`; Task 6's `Server::{za_search_entry, za_search_read_manifest, za_search_reload_if_moved}`, `IndexEntry { index: Arc<RwLock<Option<LoadedIndex>>>, pass_lock, .. }` with `IndexEntry::{absent_since(&BlobHash, now) -> u64, seen(&BlobHash), read_loaded()}`, `LoadedIndex { manifest_bytes_hash, manifest, thread, content, scanned_to }`, `LoadFailure`, `seal_section`; Task 2's `Server::za_search_config()`, the traces (`za_search_trace!`, `ZA_SEARCH_PASS`, `ZA_SEARCH_SKIPPED`, `ZA_SEARCH_ABORTED`, `ZA_SEARCH_LOST`, `ZA_SEARCH_SUSPENDED`), the hooks; upstream: `index_document` (`crates/email/src/message/index/search.rs:43-153`) called as the index queue calls it (`crates/services/src/index_queue/document.rs:81-101`: `row.raw_headers()`, `row.unarchive().index_document(account_id, document_id, &headers, raw, core.email.index_fields[&SearchIndex::Email], core.email.default_language, &core.email.extract_limits)`), `MessageSortKeys::from_message` (`sortkeys.rs:452-469`), `za_has_attachment(row.unarchive())` (`view.rs:411`), `hold_blob` (`crates/common/src/storage/blob.rs:308-360`), `BlobHold::release` (`:108-122`), `BlobOp::Link`/`BlobLink::Document` (`crates/store/src/write/mod.rs:1296-1306`; key `hash | account_id | Collection::Principal (7) | 0`, `key.rs:410-421`), `BlobLinkValue::new(len)` (as `za/ingest.rs:230-236` writes a `Document` link), `AssertValue::Hash(xxh3_64(bytes))` (`assert.rs:63`; the vault's `cas`, `vault.rs:117`), `SetValue::serializable` (`write/mod.rs:1192-1198`), `AssignedIds::change_id(account_id, SyncCollection::Email.change_group())` (`write/mod.rs:1625-1635`), `Server::commit_batch` (`transaction.rs:27`), `log_item_update(SyncCollection::Email, Some(PendingId::Assigned(w)))` (as `merge_keywords`, `messagedata.rs:618`), `log_container_property_change(collection, PendingId)` (`batch.rs:463-474`), `MessageStoreCache::{last_change_id, mailboxes.items[i].document_id}` (`common/src/lib.rs:288-296, 337-342, 399-401`), `get_cached_messages` (`crates/email/src/cache/mod.rs:59-68`), `core.jmap.upload_tmp_ttl` (`crates/common/src/config/mailstore/jmap.rs:34`), `BlobOpts::new(CompressionAlgo::None, core.blob.email.encryption.clone())` (`za/ingest.rs:176-179`), `is_assertion_failure()`.
- Produces:
  ```rust
  // pass.rs
  pub enum PassOutcome { Current, Advanced { published: bool, indexed_to: Checkpoint }, Running, Aborted(TransientClass), Lost, NoMap(NoMapReason), Recovery(RecoveryCause) }
  pub enum RecoveryCause { Loss(LossClass), Truncated } // with `pub fn as_str(&self) -> &'static str` for the traces
  impl Server { pub async fn za_search_pass(&self, account_id: u32, keys: &SessionKeys) -> trc::Result<PassOutcome>; }
  pub(crate) struct PassLimits { pub cap: usize, pub hidden_bytes: usize, pub message_bytes: u64, pub elapsed: Duration, pub checkpoint: usize, pub skip_after: u8, pub content_allowed: bool }
  pub(crate) struct Indexed { pub fields: ThreadFieldsK, pub content: Option<ContentRecord>, pub bytes: usize }
  pub(crate) trait Opener { async fn open(&mut self, document_id: u32) -> Result<Indexed, OpenFailure>; }
  pub(crate) enum PassDecision { Nothing, ManifestOnly, NonPublishing, Publishing, ContentOnly, Abort { class: TransientClass, document_id: u32 } } // ContentOnly: a content cursor's pass (Task 14), a content blob and a manifest update, no thread blob, no revision, no log entry
  pub(crate) struct Staging { pub revision: u32, pub records: Vec<ThreadRecord>, pub aliases: Vec<Alias>, pub tombstones: Vec<Tombstone>, pub content: Vec<(u32, ContentRecord)>, pub skipped: Vec<(u32, LossClass)>, pub checkpoint: Checkpoint, pub cursor: CursorStep, pub content_cursor: CursorStep, pub marker: MarkerStep, pub marker_bound: Option<u64> /* FU3: a BoundMoved pass's new bound */, pub suspend_content: bool /* FU6 */, pub bookkeeping: bool, pub scanned_to: u64, pub update_records: usize, pub witness: Option<u32>, pub max_item_aliases: usize /* PL8: test instrumentation only */, pub decision: PassDecision, pub failing: Option<FailingDocument> }
  #[derive(Default)] pub(crate) struct PassSources { pub cursor: Vec<u32>, pub content: Vec<u32>, pub content_runnable: bool /* FW1: the content part resident and current on this node */, pub gap_inserts: Vec<u32>, pub gap_deletes: Vec<u32>, pub gap_stamp: Option<u64> /* bound − 1 while a marker is set */, pub marker_bound: Option<u64> /* FW5: rule 4's BoundMoved */, pub finish_at: Option<u64> /* FW5: rule 5's cache counter, while a marker is set */ } // FT2: Task 8 fills `cursor` only; Task 14 fills the rest
  pub(crate) async fn plan_pass(base: &LoadedIndex, sources: PassSources, records: &ZaChangeRecords, limits: &PassLimits, started: Instant, opener: &mut impl Opener) -> Staging;
  // write.rs
  pub(crate) fn apply_marker_arms(manifest: Manifest, marker: MarkerStep, marker_bound: Option<u64>) -> Manifest; // step 6's marker arms, factored for their test (FW9, FX11); the test module's `manifest_with_marker(bound, gap_next) -> Manifest` builds a `Manifest::default()` with that marker
  pub(crate) async fn write_pass(server: &Server, account_id: u32, keys: &SessionKeys, entry: &IndexEntry, previous_bytes: &[u8], base_hash: [u8; 32], base_manifest: Manifest, staging: Staging) -> trc::Result<PassOutcome>;
  // common/src/za/search/setup.rs
  pub fn za_setup_manifest(swk: &vault::keys::Secret, indexed_to: u64) -> Vec<u8>;
  // common/src/auth/vault.rs
  pub struct ZaVaultExtra { pub field: PrincipalField, pub value: Vec<u8> } // asserted `AssertValue::None` and set in the vault batch
  ```
  Test helpers (`tests/src/utils/za.rs`): `run_search_pass(test: &TestServer, account_id: u32, password: &str) -> PassOutcome` (`za_unlock` then `za_search_pass`); `read_manifest(test, account_id) -> Option<ManifestValue>` (field 152 through `store().get_value::<RawValue>(ValueKey::property(account_id, Collection::Principal, 0, PrincipalField::ZeroAccessSearch))`, decoded with `ManifestValue::decode_unverified`, so a destroying marker reads too; the tag's verification is the loader's and has Task 5's unit tests); `simulate_restart(test: &TestServer, account_id: u32)` (evicts the account's search entry through `ZaSearchCache::remove_account(account_id, EvictReason::Invalidation)`, removes its resident keys with `za_keys.remove_account`, and invalidates its message cache with `CacheInvalidation::MessageCache`: every in-memory state a restart loses, plan call C7); `manifest_bytes(test, account_id) -> Option<Vec<u8>>`; `principal_blob_links(test, account_id) -> Vec<(BlobHash, usize)>` (hash and the link value's stored size, `BlobLinkValue`; iterate the blob-link subspace, keep keys whose suffix is `account_id | 7 | 0`; model it on `tests/src/utils/cleanup.rs`'s link scan); `index_holds(test, account_id, document_id) -> bool` (`cache.za_search.get(account_id)` and its loaded index, then `thread.exists_at(document_id, u64::MAX) == Some(true)`); `log_records_since(test, account_id, collection: SyncCollection, since: u64) -> Vec<(u64, Vec<Change>)>` (`za_changes_records` from `Checkpoint { change_id: since, position: 0, complete: true }`, or `None` when `since == 0`, with `usize::MAX` records).

**Rules this task implements** (spec 5.1, stated as procedures; the implementer follows them and does not reopen them):
- **Order.** A pending cursor's items first, oldest document id first, and no log record in a pass that worked cursor items; then records in change-id order; within a record, inserts in document-id order, then its deletions; a record may be left part-way at an insert boundary, and its deletions are applied only when it completes.
- **Discard.** A tombstoned message keeps only its identity (document id, assignment, creation and deletion change ids): `ThreadPart::tombstone` drops its hashes. Nothing else is discarded here (compaction is Task 15).
- **Checkpoints.** `indexed_to = (c, p, complete)`: records below `c` whole; of `c`, inserts up to `p`; `complete` when `c`'s inserts and deletions are done. A pass resumes a partial `c` inclusively (Task 7's reader does), skips its inserts `<= p`, finishes its inserts, applies its deletions. `scanned_to` (memory only) is the highest change id the pass read whole.
- **Error classes.** Loss of a message (Task 7's `Loss`) skips it: traced `ZA_SEARCH_SKIPPED` with account id, document id and the class, recorded in `skipped`, never retried. A transient error aborts: nothing indexed this pass is written; the one write is a manifest-only checkpoint carrying `failing = FailingDocument { document_id, attempts: previous + 1 }` (previous = the manifest's `failing` for the same document, else 0); when `attempts` reaches `ZA_SEARCH_SKIP_AFTER` the document is skipped as loss instead (`ZA_SEARCH_SKIPPED` with the transient class as the reason) and the pass continues. A message blob reported absent is `Transient(AbsentWithinWindow)` while `entry.absent_since(&blob_hash, now)` (Task 6's window, keyed by the message blob's hash from the stub) is younger than `ZA_SEARCH_ABSENT_RETRY_SECS`, and `Loss(BlobAbsent)` after.
- **Charges.** Every admitted item (a cursor item or an insert that was opened, whatever happened next) adds `reservation()` to `reserved`, 1 to `items` and the raw size to `bytes`. Before admitting an item, if `items >= 1` and (`items >= cap` or `reserved >= hidden_bytes` or `bytes >= message_bytes` or `started.elapsed() >= elapsed`), stop at this insert boundary. Nothing about a record's aliases or its actual size enters the decision.
- **Keyless writes** (spec 6.3, invariant 53): for a publishing pass exactly one Email item update of the witness `W` (the lowest document id among the messages this pass indexed, computed from `records`' ids alone), one Thread container property change of `W`, one Email container property change per mailbox of the account; nothing for a non-publishing or manifest-only pass.

- [ ] **Step 1: Write the failing unit tests** in `pass.rs` with a `FakeOpener` (`HashMap<u32, Result<Indexed, OpenFailure>>`; a fake `Indexed` has `fields.hashes` chosen per fixture, `subject_hash` fixed, `content: None`, `bytes: 1000`) and a `base` built from `LoadedIndex::empty(manifest)` (Task 6's constructor; `indexed_to` complete at change id 10, `epoch 0`, `revision 3`). Limits: `cap 4, hidden_bytes 32 * 1024, message_bytes 32 MiB, elapsed 2 s, checkpoint 1000, skip_after 8, content_allowed true`. Records are built as `(change_id, vec![Change::InsertItem((d as u64) << 32 | d as u64), ..])`.

```rust
#[tokio::test]
async fn equal_public_inputs_stop_at_the_same_checkpoint_whatever_the_probe_merged() { // R21
    // 37 fillers (docs 1..=37, distinct subjects, no references) in records c11..c47, then a probe (doc 38) whose two
    // references name docs 5 and 6: in fixture A, 5 and 6 share subject and thread; in B they are distinct threads.
    // cap 48 > 38, hidden budget lowered to 38 * reservation() - 1 so the budget, not the count, binds.
    let a = plan_pass(&base, PassSources::default(), &records_a, &limits, Instant::now(), &mut opener_a).await;
    let b = plan_pass(&base, PassSources::default(), &records_b, &limits, Instant::now(), &mut opener_b).await;
    assert_eq!(a.checkpoint, b.checkpoint);
    assert_eq!(a.witness, b.witness);
    assert_ne!(a.aliases.len(), b.aliases.len(), "the fixtures differ only in what merged");
}
#[tokio::test]
async fn a_record_larger_than_the_cap_is_left_part_way() { // R20, R1
    // one record c11 with inserts 1..=10, cap 4
    let s = plan_pass(&base, PassSources::default(), &records, &limits, Instant::now(), &mut opener).await;
    assert_eq!(s.records.len(), 4);
    assert_eq!(s.checkpoint, Checkpoint { change_id: c11, position: 4, complete: false });
    assert!(s.tombstones.is_empty(), "the record's deletions wait for its completion");
    // resumed: base.indexed_to = that checkpoint, records read inclusively hold c11 again
    let s2 = plan_pass(&resumed_base, PassSources::default(), &records, &limits, Instant::now(), &mut opener).await;
    assert_eq!(s2.records.iter().map(|r| r.document_id).collect::<Vec<_>>(), vec![5, 6, 7, 8]);
}
#[tokio::test]
async fn a_cap_sized_unreadable_prefix_is_consumed_and_the_suffix_follows() { // PL4
    // one record c11 with inserts 1..=6, cap 4; the opener answers Loss(BlobAbsent) for 1..=4, Ok for 5 and 6
    let s = plan_pass(&base, PassSources::default(), &records, &limits, Instant::now(), &mut opener).await;
    assert!(s.records.is_empty());
    assert_eq!(s.skipped.len(), 4);
    assert!(matches!(s.decision, PassDecision::ManifestOnly), "the skips are progress and are checkpointed");
    assert_eq!(s.checkpoint, Checkpoint { change_id: c11, position: 4, complete: false });
    assert_eq!(s.scanned_to, c11 - 1, "a record stopped inside is not read whole");
    // resumed from that checkpoint (as after a restart: base = LoadedIndex::empty(manifest at it))
    let s2 = plan_pass(&resumed_base, PassSources::default(), &records, &limits, Instant::now(), &mut opener).await;
    assert_eq!(s2.records.iter().map(|r| r.document_id).collect::<Vec<_>>(), vec![5, 6]);
    assert_eq!(s2.checkpoint, Checkpoint { change_id: c11, position: 0, complete: true });
    assert_eq!(s2.scanned_to, c11);
}
#[test]
fn effective_sent_at_is_upstreams() { // PL10: received_at + sent_at_offset, as sent_at_absolute
    assert_eq!(za_effective_sent_at(None, 100), 100);
    assert_eq!(za_effective_sent_at(Some(200), 100), 200);
    assert_eq!(za_effective_sent_at(Some(i64::MAX), 100), 100 + i32::MAX as i64);
    assert_eq!(za_effective_sent_at(Some(i64::MIN), 100), 100 + i32::MIN as i64);
}
#[tokio::test]
async fn scanned_to_never_covers_a_partial_checkpoint() { // PL4
    assert_eq!(scanned_seed(&Checkpoint { change_id: 11, position: 4, complete: false }), 10);
    assert_eq!(scanned_seed(&Checkpoint { change_id: 11, position: 0, complete: true }), 11);
    assert_eq!(LoadedIndex::empty(manifest_at(Checkpoint { change_id: 11, position: 4, complete: false })).scanned_to, 10);
}
#[tokio::test]
async fn a_partial_record_finishes_then_applies_its_deletions() { // R29
    // record c11: inserts 1..=5, delete 9 (held present in base); cap 10; base.indexed_to = (c11, 3, false)
    assert_eq!(s.records.iter().map(|r| r.document_id).collect::<Vec<_>>(), vec![4, 5]);
    assert_eq!(s.tombstones, vec![Tombstone { document_id: 9, deleted: c11 }]);
    assert_eq!(s.checkpoint, Checkpoint { change_id: c11, position: 0, complete: true });
}
#[tokio::test]
async fn the_witness_is_the_lowest_indexed_id_whatever_threaded() { // R4
    // inserts 20, 7, 13 in one record; in fixture A 7 replies to a base message 2, in B nothing threads
    assert_eq!(a.witness, Some(7)); assert_eq!(b.witness, Some(7));
}
#[tokio::test]
async fn a_logged_deletion_is_applied_unconditionally() { // R17
    // base holds 3 present; record c11: delete 3; the opener is never consulted
    assert_eq!(s.tombstones, vec![Tombstone { document_id: 3, deleted: c11 }]);
    assert!(matches!(s.decision, PassDecision::NonPublishing));
    // a deletion of a document the index does not hold stages nothing but counts as consumed
    assert!(matches!(s_unknown.decision, PassDecision::ManifestOnly));
}
#[tokio::test]
async fn the_decision_follows_what_changed() { // R13, R22, R26, F4, G19
    assert!(matches!(update_only.decision, PassDecision::Nothing));                 // 5 UpdateItem records (the shared `base` has no cursor; `cursor_only` below uses `base_with_cursor({next: 4, end: 5})`, FS5)
    assert_eq!(update_only.scanned_to, c15);
    assert!(matches!(many_updates.decision, PassDecision::ManifestOnly));         // 1000 UpdateItem records (limits.checkpoint)
    assert!(matches!(skipped_only.decision, PassDecision::ManifestOnly));         // one insert whose opener answers Loss(Aead)
    assert_eq!(skipped_only.skipped, vec![(1, LossClass::Aead)]);
    assert!(matches!(cursor_only.decision, PassDecision::ManifestOnly));          // cursor_items [4, 5] all Loss(StubAbsent)
    assert!(matches!(cursor_only.cursor, CursorStep::Exhausted) && cursor_only.bookkeeping); // base_with_cursor({next: 4, end: 5}): the list ran out, the cursor clears whatever `end` is (FR1)
    assert!(matches!(update_only.cursor, CursorStep::None) && !update_only.bookkeeping);  // no pending cursor: the block never ran (FS5)
    assert!(matches!(published.decision, PassDecision::Publishing));              // one indexed insert
    assert_eq!(published.revision, base.manifest.revision + 1);
}
#[tokio::test]
async fn a_transient_open_aborts_with_the_attempt_count() { // F3, R34, H5
    // record c11: inserts 1 (ok), 2 (Transient(BlobStore)), 3 (ok)
    assert!(matches!(s.decision, PassDecision::Abort { class: TransientClass::BlobStore, document_id: 2 }));
    assert_eq!(s.failing, Some(FailingDocument { document_id: 2, attempts: 1 }));
    assert!(s.records.is_empty(), "nothing of an aborted pass is written");
    // base.manifest.failing = Some((2, 7)), skip_after 8: document 2 is skipped as loss and the pass goes on
    assert_eq!(s8.skipped, vec![(2, LossClass::Parse)]); // a transient skip records BlobAbsent for AbsentWithinWindow, else Parse (plan call C8)
    assert_eq!(s8.records.iter().map(|r| r.document_id).collect::<Vec<_>>(), vec![1, 3]);
    assert_eq!(s8.failing, None);
}
#[tokio::test]
async fn cursor_items_come_first_and_take_the_epoch_as_creation() { // 5.4 order, R10
    // base.manifest.cursor = Some(RebuildCursor { next: 3, end: 6 }), epoch 100; cursor_items [3, 5, 6]; records c11.. present
    assert_eq!(s.records.iter().map(|r| (r.document_id, r.created)).collect::<Vec<_>>(), vec![(3, 100), (5, 100), (6, 100)]);
    assert!(matches!(s.cursor, CursorStep::Exhausted), "the list ran out under the cap");
    assert_eq!(s.checkpoint, base.manifest.indexed_to, "no record is consumed in a cursor pass");
    // cap 2 on the same fixture: the cap stops the loop with 6 left
    assert!(matches!(s_cap2.cursor, CursorStep::Advanced(6)) && !s_cap2.bookkeeping);
    // base.manifest.cursor = Some({next: 49, end: 60}) over a cache holding nothing in 49..=60 (expunged), and empty `records` (FU5: an
    // exhausted-empty cursor pass reads the records after indexed_to, so with records present the decision would be theirs): an empty list is exhausted (FR1)
    assert!(matches!(s_empty.cursor, CursorStep::Exhausted) && s_empty.bookkeeping && matches!(s_empty.decision, PassDecision::ManifestOnly));
    // an Abort inside the cursor loop (item 5's opener answers Transient(BlobStore)) leaves every step untouched (FS5)
    assert!(matches!(s_abort.cursor, CursorStep::None) && !s_abort.bookkeeping && matches!(s_abort.decision, PassDecision::Abort { .. }));
}
#[tokio::test]
async fn content_sources_stage_content_only() { // FT2, FU2: Task 14's content cursor through plan_pass
    // base.manifest.content_to.cursor = Some(ContentCursor { next_below: u32::MAX }); sources.content = [9, 7, 4]; cap 2; empty records;
    // this test's FakeOpener answers Indexed { content: Some(ContentRecord::new(..)) } (the step-1 fixture's `content: None` would stage nothing; FV6)
    assert_eq!(s.content.len(), 2); assert!(s.records.is_empty() && s.aliases.is_empty() && s.witness.is_none());
    assert!(matches!(s.decision, PassDecision::ContentOnly) && matches!(s.content_cursor, CursorStep::Advanced(7)));
    assert!(matches!(s_resumed.content_cursor, CursorStep::Exhausted) && s_resumed.bookkeeping); // sources.content = [4]
    assert_eq!(s_loss.skipped, vec![(7, LossClass::Aead)]); assert!(matches!(s_loss.content_cursor, CursorStep::Advanced(7))); // 7's opener answers Loss
    assert!(matches!(s_abort.decision, PassDecision::Abort { .. }) && matches!(s_abort.content_cursor, CursorStep::None)); // 7 Transient
    // FU6: over the cap (limits.content_allowed = false) the block does not run and the pass suspends
    assert!(matches!(s_over.content_cursor, CursorStep::None) && s_over.suspend_content && matches!(s_over.decision, PassDecision::ManifestOnly)); // base unsuspended, empty records
    // FV3: already suspended (base.manifest.content_to.suspended = true), content_allowed false, empty records: nothing is written
    assert!(!s_over_again.suspend_content && matches!(s_over_again.decision, PassDecision::Nothing));
    // FW6: the same suspended base with a reconciliation marker, gap_stamp Some(39) and gap_inserts [2]: the skipped content block does not hold the gap blocks back
    assert!(matches!(s_over_gap.marker, MarkerStep::GapNext(3)) && matches!(s_over_gap.decision, PassDecision::Publishing));
    // FW1: content cursor pending, under the cap, `content_runnable: false` (the part not resident on this node), one insert record: the record runs, nothing suspends
    assert!(matches!(s_not_resident.content_cursor, CursorStep::None) && !s_not_resident.suspend_content && matches!(s_not_resident.decision, PassDecision::Publishing));
}
#[tokio::test]
async fn gap_deletes_wait_for_the_last_gap_insert() { // FT2, FU2, spec H3 across the cap
    // base.manifest.reconciliation = Some({bound: 40, gap_next: 0}); sources.gap_stamp = Some(39); gap_inserts [2, 5] (5 replies to 8, held present); gap_deletes [8]; cap 1
    assert_eq!(s1.records.iter().map(|r| (r.document_id, r.created)).collect::<Vec<_>>(), vec![(2, 39)]);
    assert!(matches!(s1.marker, MarkerStep::GapNext(3)) && s1.tombstones.is_empty(), "the deletes wait for the last gap insert");
    // pass 2 (gap_next 3): 5 admitted and threaded with 8, then 8 tombstoned
    assert_eq!(s2.records[0].document_id, 5); assert_eq!(staged_thread_of(&s2, &base, 5), staged_thread_of(&s2, &base, 8)); // a test helper: the record's thread_id when `s.records` holds `d`, else `base.thread.current_thread(d)`, resolved through `s.aliases` (FV6)
    assert_eq!(s2.tombstones, vec![Tombstone { document_id: 8, deleted: 39 }]); assert!(matches!(s2.marker, MarkerStep::GapNext(6)));
    // FV2, FW5, PL22, FX10: sources { marker_bound: Some(43), finish_at: Some(60), gap_stamp: Some(42), gap lists empty }, records holding only
    // update-only records up to 60 (none indexed, fewer than the checkpoint threshold): both steps, and the write is chosen because of them
    assert!(s3.marker_bound == Some(43) && matches!(s3.marker, MarkerStep::Finished) && matches!(s3.decision, PassDecision::ManifestOnly));
}
#[test] // in `write.rs`'s `mod tests` (FW9), not `pass.rs`'s
fn a_finishing_bound_move_writes_no_bound() { // FV2, write.rs: the arms applied to a manifest with a marker {bound: 40, gap_next: 9}
    let new = apply_marker_arms(manifest_with_marker(40, 9), /* marker */ MarkerStep::Finished, /* marker_bound */ Some(43));
    assert!(new.reconciliation.is_none()); // no panic, the marker cleared
    let new = apply_marker_arms(manifest_with_marker(40, 9), MarkerStep::GapNext(12), Some(43));
    assert_eq!(new.reconciliation, Some(ReconciliationMarker { bound: 43, gap_next: 12 }));
}
#[tokio::test]
async fn the_time_cap_stops_at_an_insert_boundary_with_at_least_one_item() { // G18
    // limits.elapsed = Duration::ZERO, started = Instant::now() - 1 s; record c11 inserts 1..=3
    assert_eq!(s.records.len(), 1);
    assert_eq!(s.checkpoint, Checkpoint { change_id: c11, position: 1, complete: false });
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `export PATH="/opt/homebrew/opt/rustup/bin:$PATH" && cargo test -p email --lib --features test_mode za::search::pass`
Expected: compile errors (`plan_pass`, `Staging` not found).

- [ ] **Step 3: Implement `plan_pass`** in `pass.rs` by the rules above. Sketch of the state: `items, reserved, bytes: usize`, `consumed: bool`, `update_records: usize`, `staged: ThreadPart` overlay (clone `base.thread` once; `ThreadPart: Clone`; a copy per pass is the thread part's size, megabytes at most, and only on the pass's task; charged for the pass's duration by step 5, FR13 and FS9), `witness: Option<u32>`, `checkpoint = base.manifest.indexed_to`, `revision = base.manifest.revision + 1` used for assignments (kept only if `Publishing`). Sources, in order, under the one cap (FT2, FT8): the cursor block, then the content block, then the gap blocks (Task 14's three; empty lists in Task 8), each later block running only when every earlier one is not pending, is empty, ran off its list this pass, or did not run this pass (over the cap, or not runnable on this node; FW6); a pass that worked any item of the four reads no records (plan call C2) and returns after them. Cursor items: `for d in sources.cursor { if cap → break; admit(d, created = base.manifest.epoch) }`, this block runs only when `base.manifest.cursor.is_some()` (FS5: without a pending cursor `cursor = CursorStep::None`, `bookkeeping` stays false and the records below are read; `cursor_items` alone cannot tell "no cursor" from "a pending cursor whose list is empty"); the loop ends one of two ways: the cap stopped it with items left → `cursor = CursorStep::Advanced(last admitted or skipped id + 1)`; it ran off the list, every item admitted or skipped, or the list was empty → `cursor = CursorStep::Exhausted` and `bookkeeping = true` (FR1: completion is a fact of the loop, never inferred from the last id against `end`, since the message with id `end`, or every remaining one, may have vanished through POP3 or an expunge; `pub enum CursorStep { None, Advanced(u32), Exhausted }` and `pub enum MarkerStep { Untouched, GapNext(u32), Finished }` in `pass.rs`; `content_cursor` and `marker` are set by the content and gap blocks Task 14 fills, `CursorStep::None` and `MarkerStep::Untouched` while their lists are empty and no such cursor is pending, FS2). Content items (`sources.content`, pending while `base.manifest.content_to.cursor.is_some()`): if `!limits.content_allowed` the block does not run and `suspend_content = !base.manifest.content_to.suspended` (FU6: over the cap a content cursor would pass its items without content records and finish with holes; the arm sets `content_to.suspended = true`, the cursor stays, and `za_search_content_needs_rebuild` decides on a later pass. FV3: the step is set on the transition only; on an already-suspended manifest the skip stages nothing and the decision falls through, so a suspended account with a pending content cursor writes nothing per request, 5.1, though each keyed request still runs a pass to decide `Nothing`, the pending cursor disabling the shortcut, FW14); and when `limits.content_allowed` holds but `!sources.content_runnable` (the part not resident, or stale, on this node) the block does not run either, with `content_cursor = None` and nothing suspended (FW1: the cursor waits for a content request on this node, or another node); else `for d in sources.content { if cap → break; admit_content(d) }`, ending as the cursor block does in `content_cursor = Advanced(the last admitted or skipped id)` or `Exhausted`. Gap items (pending while `base.manifest.reconciliation.is_some()`, FU13; inside that gate `stamp = sources.gap_stamp.expect("a marker is set")`, so Task 8's own passes, whose `gap_stamp` is `None`, never reach it): `for d in gap_inserts { if cap → break; admit(d, created = stamp); marker = GapNext(d + 1) }` (a skipped item advances it too); then, only if the insert loop ran off its list this pass, `for d in gap_deletes { if staged.exists_at(d, u64::MAX) == Some(true) { staged.tombstone(d, stamp); push Tombstone } }` (FU2, spec H3: a gap insert admitted on a later pass must still find the messages the gap deleted; the deletes stay in the difference until applied, so deferring them loses nothing); `plan_pass` copies `sources.marker_bound` into `Staging.marker_bound` at its start, before any block, so an `Abort` keeps it (FV10, FX10), and, while a marker is set, applies rule 5 itself before deciding: `sources.gap_inserts.is_empty() && sources.gap_deletes.is_empty() && sources.finish_at.is_some_and(|f| checkpoint.change_id >= f)` → `marker = Finished` (FW5: the bound and the cache counter are Task 14's, filled into `PassSources` at step 0, so the decision `plan_pass` computes sees both; set on the returned `Staging` afterwards, a pass whose only work was a bound move or the marker's completion would decide `Nothing` and the marker would never clear on an idle account). `admit_content(d)`: `items += 1; reserved += reservation(); match opener.open(d)`: `Ok(indexed)` → `bytes += indexed.bytes; if let Some(c) = indexed.content { content.push((d, c)) }` (no `assign`, no record, no alias, no witness: a content record for a document the part already holds, so the decision stays `ContentOnly`, 5.4); `Err(Loss(class))` → `skipped.push((d, class))` (the cursor still advances past it); `Err(Transient(class))` → the attempts rule of `admit`. Return before the record loop. Records: `for (change_id, changes)`: `let partial = Some(change_id) == Some(base.manifest.indexed_to.change_id) && !base.manifest.indexed_to.complete`; inserts `= changes InsertItem ids as u32, sorted, filtered(> position if partial)`; `deletes` likewise; if both empty → `update_records += 1`, `checkpoint = complete(change_id)`, continue; for each insert: `held = staged.exists_at(d, u64::MAX).is_some()` → skip (`consumed = true`); cap check → set `checkpoint = Checkpoint { change_id, position: last_done, complete: false }` where `last_done` is the previous insert's id (or `position` when resuming) and return; `admit(d, created = change_id)`; after the inserts, for each delete `d`: if `staged.exists_at(d, u64::MAX) == Some(true)` → `staged.tombstone(d, change_id)`, push `Tombstone`; `consumed = true`; `checkpoint = complete(change_id)`. `admit`: `items += 1; reserved += reservation(); match opener.open(d)`: `Ok(indexed)` → `bytes += indexed.bytes; let a = staged.assign(&indexed.fields, d, revision, created); max_item_aliases = max_item_aliases.max(a.aliases.len()); records.push(the record staged.assign created); aliases.extend(a.aliases); if let Some(c) = indexed.content { content.push((d, c)) }; witness = min`; `Err(Loss(class))` → `skipped.push((d, class)); consumed = true` (PL4, plan call C5: a skipped insert moves `indexed_to` like an indexed one, so its checkpoint is written even when the pass staged nothing; otherwise a cap-sized run of unreadable inserts would be re-opened by every pass and the record's later inserts never reached); `Err(Transient(class))` → attempts rule: `prev = base.manifest.failing.filter(|f| f.document_id == d).map_or(0, |f| f.attempts)`; if `prev + 1 >= limits.skip_after` → `skipped.push((d, loss_of(class))); consumed = true` where `loss_of(AbsentWithinWindow) = BlobAbsent`, otherwise `Parse`; else `decision = Abort { class, document_id: d }; failing = Some(FailingDocument { document_id: d, attempts: prev + 1 })`, discard `records`, `aliases`, `tombstones`, `content`, keep `checkpoint = base.manifest.indexed_to`, and leave `cursor = CursorStep::None`, `content_cursor = CursorStep::None`, `marker = MarkerStep::Untouched`, `bookkeeping = false`, and keep `marker_bound` and `suspend_content` as the pass found them (facts of the pass, written by the `Abort`'s manifest-only write, FV10) (FS5: an aborted cursor pass moves no cursor, since the items it admitted were discarded and are in no log record after `indexed_to`), return. Decision at the end: `Publishing` if `!records.is_empty()`; else `ContentOnly` if `!content.is_empty()` (a content cursor's pass, Task 14: content records for documents already indexed); else `NonPublishing` if `!tombstones.is_empty()`; else `ManifestOnly` if `consumed || bookkeeping || matches!(cursor, CursorStep::Advanced(_)) || matches!(content_cursor, CursorStep::Advanced(_)) || matches!(marker, MarkerStep::GapNext(_)) || marker_bound.is_some() || suspend_content || base.update_records + update_records >= limits.checkpoint` (`bookkeeping = matches!(cursor, Exhausted) || matches!(content_cursor, Exhausted) || matches!(marker, Finished)`, derived, FR1 and FS2: a cursor or a marker leaving the manifest is a write of its own, made even by a pass that staged nothing and consumed nothing, which is how a rebuild cursor, a content cursor or a reconciliation marker clears on an idle account; Task 8 produces the rebuild cursor's step, Task 14 the content cursor's and the marker's) (`LoadedIndex.update_records`, memory only, counts the update-only records read whole since `indexed_to` across this node's passes; a `ManifestOnly` checkpoint reached this way sets `indexed_to = complete(scanned_to)` and the swap resets the count; a `Nothing` decision adds this pass's count to it); else `Nothing`. `scanned_to = scanned_seed(&checkpoint)` (Task 6: the checkpoint's change id when complete, else one below it, so a record the pass stopped inside is never counted as read whole, PL4).

- [ ] **Step 4: Run the unit tests**

Run: `export PATH="/opt/homebrew/opt/rustup/bin:$PATH" && cargo test -p email --lib --features test_mode za::search::pass`, then the same with `za::search::write`
Expected: 14 passed, then 1 passed (FW9).

- [ ] **Step 5: Implement `za_search_pass`** (steps 0–2 glue) in `pass.rs`:
  0. `let config = self.za_search_config();` `let entry = match self.za_search_entry(account_id, keys).await { Ok(e) => e, Err(LoadFailure::Loss(l)) => return Ok(PassOutcome::Recovery(RecoveryCause::Loss(l))), Err(LoadFailure::Transient(t)) => return Ok(PassOutcome::Aborted(t)), Err(LoadFailure::NoMap(r)) => return Ok(PassOutcome::NoMap(r)), Err(LoadFailure::OverCap) => unreachable!("a thread load never answers OverCap") };` `let Ok(_pass) = entry.pass_lock.try_lock() else { return Ok(PassOutcome::Running) };` Shortcut: `let cache = self.get_cached_messages(account_id).await?;` read `(cursor, reconciliation, content_to, indexed_to, scanned_to, hash, needs = za_search_content_needs_rebuild(&manifest, &config))` in a block whose `entry.index.read()` guard is dropped before anything else in this step (the reload below takes `index.write()`; FR12); if `cursor.is_none() && reconciliation.is_none() && content_to.cursor.is_none() && needs.is_none() && indexed_to.complete && cache.last_change_id <= scanned_to` → `Current` (FU1: a due content-only rebuild, `CapRaised` or `Behind`, is work the log's change id does not show, so the shortcut never answers while one is due; the term makes `Behind`'s `!suspended` guard load-bearing, FV1: without it the gate itself would start a rebuild on every idle over-cap account. FV5: Task 8 writes the shortcut without this term and Task 14 step 4 adds it, since `za_search_content_needs_rebuild` and `ContentRebuildReason` are Task 14's) (PL4: the shortcut never answers while a rebuild cursor, a content cursor or a reconciliation is pending, or while the checkpoint is partial, since each has work the log's change id does not show; when the checkpoint is complete `scanned_to >= indexed_to.change_id` by `scanned_seed`). `search_sweep_pause_point(account_id).await` (`SEARCH_SWEEP_PAUSE`). `let Some((bytes, value)) = (match self.za_search_read_manifest(account_id, &za_search_key(keys.swk())).await { Ok(v) => v, Err(LoadFailure::Loss(l)) => return Ok(Recovery(Loss(l))), Err(LoadFailure::Transient(t)) => return Ok(Aborted(t)), Err(LoadFailure::NoMap(r)) => return Ok(NoMap(r)), Err(LoadFailure::OverCap) => unreachable!("the manifest read never answers OverCap") }) else { return Ok(Recovery(Loss(LossClass::ManifestMissing))) /* H11 */ };` `ManifestValue::Destroying(_)` → `NoMap(NoMapReason::Destroying)`; if `manifest_hash(&bytes) != loaded hash` → `self.za_search_reload_if_moved(&entry, keys).await` (map its `LoadFailure` as above), then re-read the loaded state. Snapshot the base: `let base = entry.index.read().await.clone()` is too large; instead hold the read guard only to clone `manifest` and `thread` into `plan_pass`'s `base` (a `LoadedIndex` view with `content: None`), as `plan_pass` takes `&LoadedIndex`; a pending content cursor's items are computed under that same read guard, from the loaded index with its content part, before the guard is dropped (Task 14 step 4 adds this, FW7, FW12), and the pass never loads the content part for the backfill (FW2: spec 7.4, only a content request waits for it; Task 15's merges read their inputs, spec 5.5, FX12).
  1. Cursor items: `if let Some(c) = base.manifest.cursor { cache.emails.document_ids().filter(|d| *d >= c.next && *d <= c.end).sorted() }` else empty. Content cursor items (Task 14 step 4 adds them; E10, FW1, FW2, FW3, FW7): in step 0's read block, under the guard that builds `base`, using the `limits` this step defines below, computed once, from that guard's manifest, before the items (FX11): when `manifest.content_to.cursor.is_some()` and the rebuild cursor is `None`, `sources.content_runnable = limits.content_allowed && content_cursor_items(&cursor, &loaded).map(|items| sources.content = items).is_some()`, where `loaded` is the guard's index with its content part (Task 14's `content_cursor_items` answers `None` when the manifest names a content head and the resident part is absent or stale, FW3); the pass never calls `za_search_load_content` for the backfill (FX12: Task 15's merges read their inputs, spec 5.5; a load belongs to a content request, which may wait: Task 9's `za_search_ensure_current` loads the part before its passes, FW2), and a pass whose content block cannot run on this node runs everything else, the block counting as not pending (FW1, FW6). Records: `if cursor_items.is_empty() { self.store().za_changes_records(account_id, SyncCollection::Email, Some(if base.scanned_to > base.manifest.indexed_to.change_id { Checkpoint { change_id: base.scanned_to, position: 0, complete: true } } else { base.manifest.indexed_to }), config.checkpoint.max(config.pass_cap + 1)).await? }` else an empty `ZaChangeRecords` (the records in `(indexed_to, scanned_to]` were read whole by an earlier pass of this node and held only updates, so the read resumes after `scanned_to`; it is wide enough that a run of update-only records is consumed in `ZA_SEARCH_CHECKPOINT`-sized steps instead of being re-read on every pass, and inserts beyond the cap are left unconsumed); if `records.truncated` → `Ok(PassOutcome::Recovery(RecoveryCause::Truncated))`. `limits = PassLimits { cap: config.pass_cap, hidden_bytes: config.pass_hidden_kb * 1024, message_bytes: config.pass_mb << 20, elapsed: Duration::from_millis(config.pass_ms), checkpoint: config.checkpoint, skip_after: config.skip_after, content_allowed: !base.manifest.content_to.suspended && base.manifest.content.map_or(0, |c| c.size) < config.account_max_mb << 20 }`. `let mut opener = ServerOpener { server: self, account_id, keys, entry: &entry, limits: &limits, config: &config }` whose `open` runs `za_open_for_index(account_id, d, keys, entry, config.absent_retry_secs, now)` (the H6 window is applied inside it, FR6), then `index_document` as `document.rs:88-101` calls it, `MessageSortKeys::from_message(&parsed)`, `za_has_attachment(row.unarchive()?)`, `za_thread_fields_k(&parsed)`, `content = limits.content_allowed.then(|| ContentRecord::new(document, d, SortKeys::from(&sort_keys), Some(za_effective_sent_at(fields.sent_at, opened.received_at)), has_attachment))` with `pub(crate) fn za_effective_sent_at(date: Option<i64>, received_at: u64) -> i64 { received_at as i64 + MessageData::sent_at_offset(date, received_at) as i64 }` in `pass.rs` (PL10: upstream's `sent_at_absolute`, `crates/common/src/cache/email.rs:1887-1889`, over `sent_at_offset`, `messagedata.rs:330-337`: a missing Date is offset 0, an extreme one clamped to an i32 offset; every sort, SENT filter and date window of a key account then compares what an ordinary account compares); a parse failure (`parse()` is `None`) is `Loss(Parse)`. `let _clone_charge = self.inner.cache.za_search.admit(PartKind::Thread, base.thread.loaded_bytes() as u64, account_id).expect("a thread part is always admitted");` (FR13, FS9: the overlay clone `plan_pass` makes is a transient the thread budget counts, spec 4.4, never refused, H8; dropped after `write_pass`) `let staging = plan_pass(&base, PassSources { cursor: cursor_items, ..Default::default() }, &records, &limits, Instant::now(), &mut opener).await;` (Task 14 fills the other sources, FT2) `#[cfg(feature = "test_mode")] note_za_search_item_aliases(account_id, staging.max_item_aliases);` (PL8: `Staging.max_item_aliases`, the largest alias count of one admitted item, is memory and test instrumentation only, never written or traced) Trace `ZA_SEARCH_SKIPPED` for each `skipped` (account id, document id, the class) and `ZA_SEARCH_ABORTED` for an `Abort`.
  2. `match staging.decision { Nothing => { let mut g = entry.index.write().await; if let Some(i) = g.as_mut() { i.scanned_to = staging.scanned_to; i.update_records += staging.update_records; } Ok(Current) }, _ => write_pass(self, account_id, keys, &entry, &bytes, hash, base.manifest.clone(), staging).await }`.

- [ ] **Step 6: Implement `write_pass`** in `write.rs` (step 3):
  - `let key = za_search_key(keys.swk()); let config = ...; let cache = server.get_cached_messages(account_id).await?;`
  - **New manifest** from `base.manifest` (the `base_manifest` argument): `new.indexed_to = staging.checkpoint` unless `Abort`; three independent arms (FR1, FS2: two completions can be pending at once, a content loss during a rebuild or a truncation during a content cursor, and each arm reads its own step only): `new.cursor = match staging.cursor { CursorStep::None => new.cursor, CursorStep::Advanced(next) => Some(RebuildCursor { next, end: c.end }), CursorStep::Exhausted => None }` (the loop's own verdict, never `next > end`); `match staging.content_cursor { CursorStep::None => {}, CursorStep::Advanced(next_below) => new.content_to.cursor = Some(ContentCursor { next_below }), CursorStep::Exhausted => { new.content_to.cursor = None; new.content_to.checkpoint = new.indexed_to } }`; the marker arms, factored as `apply_marker_arms(new, staging.marker, staging.marker_bound)` (FW9): `match staging.marker { MarkerStep::Untouched => {}, MarkerStep::GapNext(g) => new.reconciliation.as_mut().expect("a marker is set").gap_next = g, MarkerStep::Finished => new.reconciliation = None }`; `if let (Some(b), Some(m)) = (staging.marker_bound, new.reconciliation.as_mut()) { m.bound = b }` (FU3; FV2: a `BoundMoved` pass may also be the pass that finishes the reconciliation, the purge having moved the bound while the last records were consumed, and the `Finished` arm above has then cleared the marker, so the bound is written only to a marker that survives, never `expect`ed); `if staging.suspend_content { new.content_to.suspended = true; trace ZA_SEARCH_SUSPENDED once per transition }` (FU6) (Task 14 produces the marker steps; Task 8 writes every arm so that Task 14 adds no code to `write.rs`); the `content_to` carry, a rule of every decision but `Abort` (FV4: it stands here, beside the arms, and not inside the content clause, since a deletion-only or update-only pass that left the checkpoint behind would make `Behind` fire through the shortcut's gate on the next request): `new.content_to.checkpoint = new.indexed_to` whenever `base.content_to.checkpoint == base.indexed_to` and every record this pass indexed has a content record (`staging.content.len() == staging.records.len()`, vacuously true for a pass that indexed none, so deletion-only and manifest-only passes carry `content_to` along with `indexed_to`); it stays behind only when a record was indexed without content (over the cap) or was already behind; `new.failing = staging.failing` (an `Abort`) or `None` when the previous failing document was indexed or skipped this pass; `Publishing` → `new.revision += 1; let previous_row = replace(&mut new.newest_row, RevisionRow { revision: new.revision, change_id: 0 })` (filled at commit); `new.scanned` nothing (memory only).
  - **Blobs** (`NonPublishing` and `Publishing`): `header = BlobHeader { kind: BlobKind::Thread, tier: 0, segment: SegmentId::random(), predecessor: None }`; `hidden = HiddenSection { records: staging.records.clone(), aliases: staging.aliases.clone() }`; `public = PublicSection { tombstones: staging.tombstones.clone(), rows: if publishing { vec![previous_row] } else { vec![] } }`; `blob = build_thread_blob(keys.swk(), account_id, &header, publishing.then_some((&hidden, PadTarget::Publishing)), &public, config.pad_min)?` (Task 5's builder frames, pads and seals both sections and writes the layout; a `NonPublishing` pass passes `None`, so its blob has no hidden section, H13; `AllowanceExceeded` cannot occur for `PadTarget::Publishing`); `hold_for = if search_hold_expire(account_id) { 0 } else { server.core.jmap.upload_tmp_ttl }` (G15: the upload TTL class, an hour by default); `if let Some(d) = search_blob_slow(account_id) { tokio::time::sleep(d).await }` (`SEARCH_BLOB_SLOW`); `let (hash, hold) = server.hold_blob(account_id, BlobHash::generate(&blob), &blob, hold_for, &BlobOpts::new(CompressionAlgo::None, server.core.blob.email.encryption.clone())).await?` (a store error here → trace `ZA_SEARCH_ABORTED`, return `Aborted(TransientClass::BlobStore)`); `search_pass_pause_point(account_id).await` (`SEARCH_PASS_PAUSE`, "a pass paused on its blob I/O"); `new.thread_blobs.push(ThreadBlobEntry { segment: header.segment, hash: hash.clone(), size: blob.len() as u32 })`; if `new.thread_blobs.len() > 255` after any compaction of step 4 (only repeated compaction defects reach it, plan call E5), drop the holds, trace `ZA_SEARCH_ABORTED` with reason `"segments"` and return `Aborted(TransientClass::Validation)` without writing (PL12: the manifest counts thread blobs in one byte). Content, when the decision is `Publishing` or `ContentOnly` and `!staging.content.is_empty()`: `header = BlobHeader { kind: Content, tier: 0, segment: random, predecessor: new.content.as_ref().map(|h| (h.hash.clone(), h.segment)) }`; `let encoded: Vec<Vec<u8>> = staging.content.iter().map(|(_, c)| c.encode()).collect(); let refs: Vec<&[u8]> = encoded.iter().map(Vec::as_slice).collect(); blob = build_content_blob(keys.swk(), account_id, &header, &refs)`; hold it the same way; `new.content = Some(ContentHead { hash, segment, length: prev.length + 1, size: prev.size + blob.len() as u64 })`. When `Publishing` and `!limits.content_allowed` (over the cap or suspended): `new.content_to.suspended = true`, trace `ZA_SEARCH_SUSPENDED` (account id, the chain's size) once per transition.
  - **Batch**:
    ```rust
    let mut batch = BatchBuilder::new();
    batch.with_account_id(account_id).with_collection(Collection::Principal).with_document(0)
        .assert_value(PrincipalField::ZeroAccessSearch, AssertValue::Hash(xxh3_64(previous_bytes)))
        .set(PrincipalField::ZeroAccessSearch, SetValue::serializable(ManifestWrite::new(
            ManifestValue::Index(new.clone()), key.clone(), ManifestFill { newest_row: publishing, epoch: false })));
    for hold in holds { hold.release(&mut batch); }           // clears the Temporary link, writes BlobOp::Commit
    for (hash, len) in &new_blobs {                           // release reset the account only; re-select
        batch.with_account_id(account_id).with_collection(Collection::Principal).with_document(0)
            .set(BlobOp::Link { hash: hash.clone(), to: BlobLink::Document }, BlobLinkValue::new(*len));
    }
    if publishing {
        let w = staging.witness.expect("a publishing pass indexed a message");
        batch.with_collection(Collection::Email).with_document(w)
            .log_item_update(SyncCollection::Email, Some(PendingId::Assigned(w)))
            .log_container_property_change(SyncCollection::Thread, PendingId::Assigned(w));
        for mailbox in cache.mailboxes.items.iter() {
            batch.log_container_property_change(SyncCollection::Email, PendingId::Assigned(mailbox.document_id));
        }
    }
    ```
    `search_commit_pause_point(account_id).await` (`SEARCH_COMMIT_PAUSE`). `match server.commit_batch(batch).await`: `Err(e) if e.is_assertion_failure()` → trace `ZA_SEARCH_LOST` (account id), drop the staging, `server.za_search_reload_if_moved(entry, keys).await` (its error traced, not returned), `Ok(PassOutcome::Lost)`; `Err(e)` → trace `ZA_SEARCH_ABORTED` (class `Store`), `Ok(PassOutcome::Aborted(TransientClass::Store))`; `Ok(assigned)` → continue.
  - **Committed bytes**: `if publishing { new.newest_row.change_id = assigned.change_id(account_id, SyncCollection::Email.change_group()).expect("the witness update allocated a change id") }`; `let committed = ManifestValue::Index(new.clone()).encode(&key); let new_hash = manifest_hash(&committed);` (plan call C3: `ManifestWrite` serializes exactly this).
  - **Swap** (I4): `search_swap_pause_point(account_id).await` (`SEARCH_SWAP_PAUSE`); `if search_swap_skip(account_id) { return Ok(PassOutcome::Advanced { published: publishing, indexed_to: new.indexed_to }) }` (`SEARCH_SWAP_SKIP`, plan call D4: the store holds the publication and this node's index does not, as another node's would); `let mut guard = entry.index.write().await; if let Some(index) = guard.as_mut() && index.manifest_bytes_hash == base_hash { index.thread.extend(staging.records, staging.aliases, publishing.then_some(new.newest_row), staging.tombstones); if index.content.as_ref().is_some_and(|c| c.segments.first() != base_manifest.content.as_ref().map(|b| &b.segment)) { index.content = None; /* FX3: a part behind the base manifest's head (a load that raced an earlier swap) is dropped, never extended or prepended to; the next content request reloads it */ } if let Some(content) = index.content.as_mut() { for (_, c) in staging.content { content.part.insert(c) } } let _admitted = server.inner.cache.za_search.admit_content_growth(index, account_id); /* Task 6; Codex rev-6 and rev-7 PL11, FU4, FU7: atomic admission against the process budget, on the search cache, outside the borrow; refused, the content part is dropped and reloaded on the next content request, the committed progress untouched */ index.manifest = new.clone(); index.manifest_bytes_hash = new_hash; index.scanned_to = staging.scanned_to.max(scanned_seed(&new.indexed_to)); /* PL4: never a partial checkpoint's own change id */ index.update_records = 0; index.thread_segments = new.thread_blobs.iter().map(|e| e.segment).collect(); if let (Some(c), Some(h)) = (index.content.as_mut(), new.content.as_ref()) && c.segments.first() != Some(&h.segment) { c.segments.insert(0, h.segment) } index.thread_bytes = index.thread.loaded_bytes() as u64; server.inner.cache.za_search.grow(&mut index.thread_charge, PartKind::Thread, index.thread_bytes, account_id); /* always true for a thread part, H8; Codex re-review PL11: a part that grows through local passes stays charged at its size */ } else { /* evicted, or a validated snapshot reloaded meanwhile and holds this content or newer */ }` (Task 3's `extend`, Task 4's `insert`). Trace `ZA_SEARCH_PASS` (account id, inserts `= records.len()`, tombstones, `published`, revision, `indexed_to`, content bytes written, bytes merged by a compaction, 0 when none). `Ok(PassOutcome::Advanced { published: publishing, indexed_to: new.indexed_to })`. A `ManifestOnly` or `Abort` decision writes the manifest alone (no blob, no log entry) and swaps only `manifest`, `manifest_bytes_hash`, `scanned_to` and `update_records`; an `Abort` returns `Aborted(class)` after that write. A `ContentOnly` decision writes the content blob and the manifest (`content`, `content_to`, Task 14's cursor), no thread blob, no revision row and no log entry, and returns `Advanced { published: false, .. }`. `failing` is cleared only when the failing document was indexed or skipped this pass; a pass that did not reach it leaves it unchanged.

- [ ] **Step 7: Setup's empty manifest.** In `crates/common/src/za/search/setup.rs`: `za_setup_manifest(swk, indexed_to)` returns `ManifestValue::Index(Manifest { revision: 0, newest_row: RevisionRow { revision: 0, change_id: 0 }, indexed_to: Checkpoint { change_id: indexed_to, position: 0, complete: true }, epoch: 0, thread_blobs: vec![], content: None, content_to: ContentTo { checkpoint: <the same checkpoint>, suspended: false, cursor: None }, cursor: None, reconciliation: None, failing: None }).encode(&za_search_key(swk))`. In `vault_batch_write` (`vault.rs:202-237`): after the pin assertions, `if let Some(extra) = &extra { batch.assert_value(extra.field, AssertValue::None); }` and after the pin set `if let Some(extra) = extra { batch.set(extra.field, extra.value); }`; thread the parameter through `vault_commit_store` and `za_vault_commit`; every other caller passes `None`. In `za_setup` (`http/src/api/vault.rs:1081`), after `let mk = Secret::random();`: `let swk = vault::keys::derive_search_key(&mk); let indexed_to = server.store().get_last_change_id(account_id, SyncCollection::Email.into()).await?.unwrap_or(0); let extra = ZaVaultExtra { field: PrincipalField::ZeroAccessSearch, value: za_setup_manifest(&swk, indexed_to) };` and pass `Some(extra)` through `za_commit` (`:485-507`, a new trailing parameter; `za_password`, `za_recover` and the others pass `None`). The vault path never touches field 152 again (F22).

- [ ] **Step 8: Compile check and format**

Run: the Global Constraints check and `cargo fmt -p email -p common -p store -- --check; cargo fmt --manifest-path crates/http/Cargo.toml -- --check`.
Expected: clean.

- [ ] **Step 9: Write the za cases** in `tests/src/za/mail/index.rs` (`pub async fn test(test: &mut TestServer)`, accounts `keymail46`, `keymail47` created with `key_mail_account` and password `"Keymail-46!"` etc., `plainmail50` a plain control created with `create_test_user`). Deliveries go over LMTP with `deliver` (`mail/mod.rs`); flag changes through JMAP `Email/set` as a keyed request (the key account's JMAP client with its password, as `search.rs`'s `Fixture` does). Each case ends by deleting the messages it added (`Email/set` destroy over JMAP, then `run_search_pass`), so the next case starts from an empty index. Cases, in order, each a `async fn case_<tag>(test: &mut TestServer, key: &Account, ..)`:
  - `case_setup_manifest`: right after `key_mail_account`, `read_manifest` is `Some(Index(m))` with `m.revision == 0`, `m.epoch == 0`, `m.thread_blobs.is_empty()`, `m.indexed_to.complete`; `run_search_pass` answers `Current`; `manifest_bytes` unchanged after it ("the manifest is created at setup and no account without mail ever rebuilds").
  - `case_persistence_and_thread_only_load` (R9, persistence): deliver 3 messages (one reply among them); `run_search_pass` → `Advanced { published: true, .. }`; `read_manifest` has `revision 1`, one thread blob, a content head with `length 1`; `principal_blob_links` has 2 hashes equal to the manifest's; `simulate_restart`; `reset_za_search_counters`; `run_search_pass` → `Current` (the shortcut after a thread-only load) and `za_search_blob_reads(id) == 1` (the thread blob only, no content blob, no message blob: `za_search_message_opens(id) == 0`); `index_holds` true for all 3 documents.
  - `case_catch_up_after_offline_deliveries`: deliver 5 messages with no keyed request in between; `run_search_pass` once → every document held, `indexed_to.change_id == cache.last_change_id`; the log (`log_records_since`, Email) gained exactly one item update (of the lowest of the 5 document ids), the Thread log one container property change of that id, and the Email log one container property change per mailbox of the account (count `cache.mailboxes.items.len()`), nothing else beyond the deliveries' own records (R4, invariant 53). A plain control (`plainmail50`) receiving the same 5 shows no such entries.
  - `case_multiappend_split_record_with_restart` (R1, R20): `set_search_config_override(Some(ZaSearchConfig { pass_cap: 3, ..ZaSearchConfig::from_env().unwrap() }))`; an IMAP MULTIAPPEND of 8 messages in one command on a keyed IMAP session (`imap_session` as `search.rs:244` builds it; the APPEND's own post-commit pass is Task 9's, so here the record is consumed by `run_search_pass`); pass 1 → `Advanced`, `read_manifest().indexed_to == Checkpoint { change_id: c, position: p3, complete: false }` with 3 held; `simulate_restart` (the restart); pass 2 → 3 more held, still partial; pass 3, with no restart before it (so `scanned_to` is the one pass 2's swap set, PL4), → `complete == true`, all 8 held, `revision == 3`; `log_records_since` of the pass entries: three item updates (one per publishing pass). No mail is delivered after the MULTIAPPEND, so every pass after the first must be refused the shortcut by the partial checkpoint alone. Then `case_pl4_unreadable_prefix_of_a_group` (same override): a second MULTIAPPEND of 5 messages, the first 3 made unreadable with `corrupt_blob(hash)` before any pass; pass → `Advanced { published: false, .. }` with 3 `ZA_SEARCH_SKIPPED` and `indexed_to` partial after the third; the next pass on the same node (no new delivery, no restart) holds the last 2 and `indexed_to.complete`; the same form repeated with `simulate_restart` between the two passes gives the same result. Clear the override.
  - `case_flag_changes_write_nothing` (F4): bytes `= manifest_bytes`, links `= principal_blob_links`; 100 keyword changes over JMAP (each request runs the dispatcher's pass from Task 9; before Task 9, call `run_search_pass` after each); `manifest_bytes` equal, `principal_blob_links` equal, and `peek`'s `scanned_to >= cache.last_change_id`.
  - `case_update_only_checkpoints` (G19): override `checkpoint: 10`; 25 flag changes with a pass after each; `manifest_bytes` changed twice (checkpoints after the 10th and the 20th update), `indexed_to.change_id` at the 20th update's change id and `scanned_to` (`za_search.get(id)`'s loaded index) at the 25th, no new blob (`principal_blob_links` equal), no pass entry in the log (the Email log's new records are exactly the 25 updates).
  - `case_unreadable_insert_is_consumed_once` (R26): deliver one message, `corrupt_blob(its hash)` (`mail/mod.rs`); `Traced::start(&[ZA_SEARCH_EVENT_TYPE])`; pass → `Advanced { published: false, .. }` with the manifest's `indexed_to` past the insert, no blob written; one `ZA_SEARCH_SKIPPED` event whose text holds the account id and document id and no subject; `reset_za_search_counters`; `run_search_pass` twice more and after `simulate_restart` → `za_search_message_opens == 0` (never reopened); a second, readable delivery is indexed by the next pass.
  - `case_transient_blob_error_aborts_then_indexes` (F3, R34): deliver one message; `set_search_blob_fail_once(Some(id))`; pass → `Aborted(BlobStore)`; `read_manifest().failing == Some(FailingDocument { document_id: d, attempts: 1 })`, `indexed_to` unchanged, `principal_blob_links` unchanged; next pass → `Advanced { published: true, .. }`, `failing == None`, `index_holds(d)`; then with `set_search_config_override(skip_after: 2)` and `SEARCH_BLOB_FAIL_ONCE` armed before each of two passes on a fresh delivery: the first aborts with `attempts 1`, the second skips it as loss (`ZA_SEARCH_SKIPPED`), `failing == None`, the document not held.
  - `case_lost_assertion` (5.6): deliver one message; `HookParked::start(id, set_search_commit_pause, run_search_pass(..))`; while parked, write a manifest-only change to field 152 directly (`read_manifest`, bump nothing but re-encode with `indexed_to` unchanged and a different `failing` of a nonexistent document, store it with `BatchBuilder` as `vault_batch_write` sets field 150) so the stored bytes differ; `finish` → `Lost`; `principal_blob_links` unchanged (the staged blob holds only a temporary link); `read_manifest` is the competing write; the next pass → `Advanced` and `index_holds(d)`.
  - `case_invalidation_between_commit_and_swap` (Review Focus): deliver one message; `HookParked::start(id, set_search_swap_pause, run_search_pass(..))` parks the pass after its commit and before its swap; while parked, `test.server.inner.cache.za_search.remove_account(id, EvictReason::Invalidation)` and `test.server.inner.cache.za_keys.remove_account(id)`, as the `CacheInvalidation::Account` arm does for a password change on another connection; `finish` → the pass returns `Advanced` and inserts no stale entry: `za_search.get(id)` is `None` or holds no loaded index, or its `manifest_bytes_hash` equals `manifest_hash(&manifest_bytes(test, id).unwrap())`; the next `run_search_pass` → `Current` and `index_holds(test, id, the message)`.
  - `case_slow_store_hold_outlives_the_wait` (G15): `set_search_blob_slow(Some((id, 2000)))` with `set_search_config_override(wait_secs: 1)`; deliver, pass → `Advanced`; `expire_and_purge_blobs(test, &admin)` (`utils/za.rs:1028`); the manifest's blobs still exist (`stored_blob` succeeds for each hash in the manifest), `principal_blob_links` equal to the manifest's set.
  - `case_deletion_only_pass_writes_no_entry` (R13): with 2 messages indexed, destroy one over JMAP; `log_records_since` lengths before; pass → `Advanced { published: false, .. }`; the manifest gained a thread blob entry whose blob is `header + 4 + 0 + sealed public` bytes long (no hidden section: the blob's length is below `config.pad_min`); no new Email item update and no Thread entry beyond the destroy's own records; `index_holds(deleted) == false` while `peek`'s `thread.exists_at(deleted, u64::MAX) == Some(false)` (a tombstone).
  - `case_repeated_delta_changes_nothing`: with the index current, re-run `run_search_pass` five times → `Current` each time, `manifest_bytes` equal; then with a delivery replayed (`deliver` the same bytes again: a new document id, a new insert; the first is already held and the second is indexed once): every document held exactly once (`thread.present_members` counts).
  Account cleanup at the end: destroy every message of `keymail46`, `keymail47`, `plainmail50` and run a last pass, so `assert_is_empty`'s exemptions (field 152 and the principal-linked blobs) are the only remains.

- [ ] **Step 10: Run the suite**

Run: `export PATH="/opt/homebrew/opt/rustup/bin:$PATH" && STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`
Expected: passes through `mail::index::test` and `assert_is_empty` at the end (with Task 2's exemptions in place).

- [ ] **Step 11: Commit**

```bash
git add crates/email/src/message/za/search/ crates/common/src/auth/vault.rs crates/common/src/za/search/setup.rs crates/common/src/za/search/mod.rs crates/common/src/za/test_hooks.rs crates/http/src/api/vault.rs tests/src/za/mail/index.rs tests/src/za/mail/mod.rs tests/src/za/mod.rs tests/src/utils/za.rs
git commit -m "Search: the pass (consume, decide, write, swap) and the empty manifest written at setup"
```

### Task 9: Triggers and the content wait

**Files:**
- Create: `crates/email/src/message/za/search/ensure.rs` (`za_search_pass_hook`, `za_search_ensure_current`, `is_current`)
- Modify: `crates/email/src/message/za/search/mod.rs` (declare `ensure`)
- Modify: `crates/jmap/src/api/request.rs:89-104` (`handle_jmap_request`, one pass before the method loop)
- Modify: `crates/imap/src/core/client.rs:113-128` (the request loop, one pass before `match request.command`)
- Modify: `crates/imap/src/op/append.rs:248-258` (after `group.flush`, before the response), `crates/jmap/src/email/set.rs:1116-1128` (after `flush`), `crates/jmap/src/email/import.rs` (after its grouped flush; find it with `grep -n "flush" crates/jmap/src/email/import.rs`)
- Modify: `tests/src/za/mail/index.rs` (the cases below), `tests/src/utils/za.rs` (`search_config_guard`)
- Test: the za cases

**Interfaces:**
- Consumes: Task 8's `za_search_pass`, `PassOutcome`, `RecoveryCause`; Task 6's `IndexEntry::pass_lock`, `Server::za_search_load_content`, `LoadFailure`; Task 2's `Server::za_search_config()` (`wait_secs`), traces (`ZA_SEARCH_UNAVAILABLE`, `ZA_SEARCH_REBUILD` for a `Recovery` outcome until Task 14 replaces the arm), `SEARCH_PASS_PAUSE`; upstream: `AccessToken::za_keys_for` (`crates/common/src/auth/access_token.rs:555-559`), `Session::za_key_session` and `SessionData::za_key_session` (`crates/imap/src/core/mod.rs:282-286, 305-307`), `MethodObject` (`crates/jmap-proto/src/request/method.rs:24-47`), `Call { name: MethodName { obj, .. } }` (`request/mod.rs:68-72`, `method.rs:18-21`).
- Produces:
  ```rust
  impl Server {
      /// Runs one pass and never fails the request: every outcome is traced where section 8 says and swallowed.
      pub async fn za_search_pass_hook(&self, account_id: u32, keys: &SessionKeys);
      /// 5.2: for a content request. Runs passes, or waits on the running one, until the index is current (7.4) with its content part loaded, or `deadline`.
      pub async fn za_search_ensure_current(&self, account_id: u32, keys: &SessionKeys, deadline: Instant) -> Result<Arc<IndexEntry>, Unavailable>;
  }
  pub enum Unavailable { Rebuilding, Transient(TransientClass), OverCap, Budget, NewerFormat, NotCurrent, Unresolved }
  pub(crate) fn is_mail_method(obj: MethodObject) -> bool; // Email | Thread | Mailbox | SearchSnippet | EmailSubmission | Blob
  pub(crate) fn is_current(index: &LoadedIndex, state: u64) -> bool;
  ```
  Test helper: `search_config_guard!(field: value, ..)` (a `macro_rules!` in `tests/src/utils/za.rs` expanding to `SearchConfigGuard::new(ZaSearchConfig { field: value, .., ..test.server.za_search_config() })`) over `SearchConfigGuard::new(ZaSearchConfig) -> SearchConfigGuard` (sets the override, restores the prior value on drop, as `PauseSlotGuard` does for a pause slot).

**Rules** (spec 5.2, 7.4): one pass per keyed request and account, before the methods or the command; `IDLE`, `NOOP` and `LOGOUT` run none; POP3 runs none; a keyed write that adds a message runs one more after its commit and before its response: on JMAP once per request, after the last `Email/set` create or `Email/import` call of the request has committed, so a request runs at most two (H20); on IMAP once per APPEND command; a request that finds the pass lock held proceeds without waiting; only a content request waits, up to `ZA_SEARCH_WAIT_SECS`, and is never answered from an index behind the request's view; **current** against the state `E` the request reads means `scanned_to >= E`, `indexed_to.complete`, `content_to.checkpoint == indexed_to`, `!content_to.suspended`, `content_to.cursor.is_none()` (Codex PL20: a pending content cursor means content is missing although the checkpoints may be equal, a rebuild's cursor not moving `indexed_to`), `cursor.is_none()`, `reconciliation.is_none()` (G14; `scanned_to` is the highest change id a pass read whole, so every record in `(indexed_to, scanned_to]` was update-only or checkpointed).

- [ ] **Step 1: Write the failing za cases** in `tests/src/za/mail/index.rs` (accounts `keymail48`, `keymail49`, control `plainmail51`):
  - `case_triggers`: deliver one message to `keymail48` (no keyed request yet; `read_manifest().indexed_to` is below the delivery's change id). (a) JMAP `Mailbox/get` from the keyed client → the manifest's `indexed_to` now covers the delivery and `index_holds(d)`. Deliver again. (b) IMAP `NOOP` on a keyed session → `indexed_to` unchanged; `SELECT INBOX` → advanced. Deliver again. (c) POP3 `STAT` (a keyed POP3 session, `mail/login.rs` shows how) → unchanged. (d) `Email/set` keyword update over a WebSocket session (keyless, `tests/src/za/websocket.rs` shows how) → unchanged (a keyless request runs no pass). (e) IMAP `APPEND` of one message on the keyed session: right after the tagged `OK [APPENDUID ..]`, `read_manifest().indexed_to` covers the appended document and `index_holds(appended)` (the post-commit pass); the same for `Email/import` (its response's `created` present and the manifest covering the id) and `Email/set` create.
  - `case_reads_do_not_wait_for_a_pass` (F5): `search_config_guard!(pass_cap: 1)`; deliver 2 messages to `keymail49`; `HookParked::start(id, set_search_pass_pause, <a keyed JMAP Email/get of an existing id>)`; while parked (the pass holds its blob I/O), from a second keyed client: `Email/set` keyword update and `Mailbox/get` each complete within 2 s (`tokio::time::timeout`), and `run_search_pass` from a third task answers `Running` at once; `finish`.
  - `case_content_wait_budget` (7.4): `search_config_guard!(wait_secs: 1, pass_cap: 1)`; deliver 6 messages; `HookParked::start(id, set_search_pass_pause, run_search_pass(..))`; while parked, `za_search_ensure_current(id, &keys, Instant::now() + Duration::from_secs(1))` → `Err(Unavailable::NotCurrent)` after about 1 s; `finish`; then `za_search_ensure_current(.., now + 30 s)` → `Ok(entry)` after the remaining passes, and `is_current(&*entry.read_loaded().await.expect("loaded"), cache.last_change_id)` (Codex re-review PL16: never `indexed_to.change_id == cache.last_change_id`, since the last publication's own witness record lies above the checkpoint and is consumed in memory only). The parked pass's work is not lost by the waiter's timeout (Review Focus): after `finish` releases it, `read_manifest().indexed_to.change_id` has advanced by that pass, and the second call needs fewer passes than the full backlog of six (count the `ZA_SEARCH_PASS` traces).
  - `case_time_cap_bounds_the_command` (G18): `search_config_guard!(pass_ms: 1)`; deliver 12 messages each with a 300 KiB `text/plain` attachment (`index_document` extracts it inline, `search.rs:91-99`); a keyed IMAP `SELECT INBOX` returns within 5 s and the manifest's `indexed_to` advanced by at least one insert and fewer than 12 (`position` partial or a change id below the last delivery); repeated `SELECT`s finish the backlog.
  - `case_plain_control_unaffected`: the same deliveries to `plainmail51`; its `Mailbox/get`, `NOOP` and `SELECT` leave no field 152 (`manifest_bytes == None`) and no principal link.

- [ ] **Step 2: Run the suite to see them fail**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`
Expected: `case_triggers` fails at (a): the manifest did not advance (no trigger yet).

- [ ] **Step 3: Implement `ensure.rs`**:
  - `za_search_pass_hook`: `match self.za_search_pass(account_id, keys).await { Ok(Current | Advanced { .. } | Running | Lost) => {}, Ok(Aborted(_)) => {} /* traced by the pass */, Ok(NoMap(reason)) => za_search_trace!(ZA_SEARCH_UNAVAILABLE, AccountId = account_id, Reason = reason.as_str()), Ok(Recovery(cause)) => za_search_trace!(ZA_SEARCH_REBUILD, AccountId = account_id, Reason = cause.as_str()) /* Task 14 starts the recovery here */, Err(err) => trc::error!(err.account_id(account_id)) }`.
  - `is_current(index, state)`: the seven conditions above, with a unit test in `ensure.rs`, `is_current_needs_no_pending_content_cursor` (Codex PL20): a loaded index with complete, equal checkpoints, not suspended, no rebuild cursor, no marker and `scanned_to >= E`, and `content_to.cursor = Some(ContentCursor { next_below: 7 })` → `false`; the same after the rebuild cursor was exhausted in the last pass, the content cursor still pending → `false`; with `content_to.cursor = None` → `true`.
  - `za_search_ensure_current`: `let config = self.za_search_config(); let mut content_recovered = false; /* FS6: one content-only rebuild start per call; Task 14 supplies za_search_content_rebuild_start, until then this arm is `Err(map_load(e))` */ loop { /* E10, FW2: a content request loads a pending backfill's content part before its passes, since the pass never loads it for the backfill. FX7: this block and both content-loss arms name Task 14's `za_search_content_rebuild_start` and `ContentRebuildReason`, so Task 14 step 4 adds them; until then Task 9 writes no loop-top block and both arms answer `Err(map_load(e))` */ if let Ok(e) = self.za_search_entry(account_id, keys).await && let Some(g) = e.read_loaded().await && g.manifest.content_to.cursor.is_some() && let Some(head) = g.manifest.content.clone() && head.size < (config.account_max_mb << 20) /* FX6: `<`, as `content_allowed` */ && g.content.as_ref().map_or(true, |c| c.segments.first() != Some(&head.segment)) { drop(g); match self.za_search_load_content(&e, keys).await { Ok(()) => {}, Err(LoadFailure::OverCap) => return Err(Unavailable::OverCap), Err(LoadFailure::Transient(TransientClass::Budget)) => return Err(Unavailable::Budget), Err(LoadFailure::Loss(class)) if !content_recovered => { content_recovered = true; match self.za_search_content_rebuild_start(account_id, keys, ContentRebuildReason::ContentLoss(class)).await { Ok(()) => {}, Err(e) if e.is_assertion_failure() => za_search_trace!(ZA_SEARCH_LOST, AccountId = account_id), Err(_) => return Err(Unavailable::Transient(TransientClass::Store)) } /* FX2: the start writes no log entry, so nothing else refreshes this node's loaded manifest; reload it, so the next iteration sees the cursor and no head, the shortcut is refused and the content block lists every present document */ if let Ok(e) = self.za_search_entry(account_id, keys).await { self.za_search_reload_if_moved(&e, keys).await.map_err(map_load)?; } continue } Err(other) => return Err(map_load(other)) } } match self.za_search_pass(account_id, keys).await { Ok(Running) => { let entry = self.za_search_entry(account_id, keys).await.map_err(map_load)?; match tokio::time::timeout_at(deadline.into(), entry.pass_lock.lock()).await { Ok(guard) => drop(guard), Err(_) => return Err(Unavailable::NotCurrent) } }, Ok(Current) | Ok(Advanced { .. }) | Ok(Lost) => { let entry = self.za_search_entry(..).map_err(map_load)?; let state = self.get_cached_messages(account_id).await.map_err(|_| Unavailable::Transient(TransientClass::Store))?.last_change_id; { let Some(g) = entry.read_loaded().await else { continue /* FX8: evicted meanwhile; the next iteration reloads */ }; let size = g.manifest.content.as_ref().map_or(0, |c| c.size); let cap = config.account_max_mb << 20; if size > cap || (g.manifest.content_to.suspended && size >= cap) { return Err(Unavailable::OverCap) } } /* FV12, FW4: an over-cap account refuses at once, as 7.4 lists it, whether or not a publishing pass has marked it suspended yet (`size > cap`), and a suspended one at the cap too; a suspended account under the cap is a pending `CapRaised` and is not refused */ let current = match entry.read_loaded().await { Some(g) => is_current(&g, state), None => continue /* FX8 */ }; if current { match self.za_search_load_content(&entry, keys).await { Ok(()) => return Ok(entry), Err(LoadFailure::Loss(class)) if !content_recovered => { content_recovered = true; match self.za_search_content_rebuild_start(account_id, keys, ContentRebuildReason::ContentLoss(class)).await { Ok(()) => {}, Err(e) if e.is_assertion_failure() => za_search_trace!(ZA_SEARCH_LOST, AccountId = account_id), Err(_) => return Err(Unavailable::Transient(TransientClass::Store)) } /* FX2: the start writes no log entry, so nothing else refreshes this node's loaded manifest; reload it, so the next iteration sees the cursor and no head, the shortcut is refused and the content block lists every present document */ if let Ok(e) = self.za_search_entry(account_id, keys).await { self.za_search_reload_if_moved(&e, keys).await.map_err(map_load)?; } continue } Err(e) => return Err(map_load(e)) } } tokio::time::sleep(Duration::from_millis(20)).await }, Ok(Aborted(class)) => { if Instant::now() >= deadline { return Err(Unavailable::Transient(class)) } tokio::time::sleep(Duration::from_millis(50)).await }, Ok(NoMap(NoMapReason::NewerFormat)) => return Err(Unavailable::NewerFormat), Ok(NoMap(NoMapReason::Destroying)) => return Err(Unavailable::Transient(TransientClass::Store)), Ok(NoMap(NoMapReason::Transient(t))) => return Err(Unavailable::Transient(t)), Ok(Recovery(_)) => return Err(Unavailable::Rebuilding), Err(_) => return Err(Unavailable::Transient(TransientClass::Store)) } if Instant::now() >= deadline { return Err(Unavailable::NotCurrent) } }` with `map_load: LoadFailure → Unavailable` (`Loss → Rebuilding`, `Transient(Budget) → Budget`, `Transient(t) → Transient(t)`, `NoMap(NewerFormat) → NewerFormat`, other `NoMap → Transient(Store)`, `OverCap → OverCap`, FW4). The caller passes `deadline = Instant::now() + Duration::from_secs(config.wait_secs)`. Every `Err` is traced `ZA_SEARCH_UNAVAILABLE` with the reason class by the caller (Task 10).

- [ ] **Step 4: Place the triggers.**
  - JMAP (`request.rs:89-104`): before `for mut call in request.method_calls` add `if let Some(keys) = access_token.za_keys_for(access_token.account_id()) && request.method_calls.iter().any(|call| is_mail_method(call.name.obj)) { self.za_search_pass_hook(access_token.account_id(), keys).await; }` (the owner's account only, I7; a keyless token has no keys and runs none).
  - IMAP (`client.rs:113-128`): after `assert_selected_readable` and before `let result = match request.command`: `if !matches!(request.command, Command::Idle | Command::Noop | Command::Logout) && let State::Authenticated { data } | State::Selected { data, .. } = &self.state && let Some(keys) = data.access_token.za_keys_for(data.account_id) { data.server.za_search_pass_hook(data.account_id, keys).await; }`.
  - IMAP APPEND (`append.rs:248-258`): after the `match group.flush(..)` arm succeeds and before the broadcast: `if let Some(keys) = self.access_token.za_keys_for(account_id) { self.server.za_search_pass_hook(account_id, keys).await; }` (the `SessionData`'s token; check its field name at the top of `append_messages`).
  - `Email/set` create (`set.rs:1116-1128`): after `flush(..)` returns `Ok` and `recheck.finish`, before the updates are processed: `if let Some(keys) = access_token.za_keys_for(account_id) { self.za_search_pass_hook(account_id, keys).await; }`. `Email/import`: the same after its grouped flush and before `response` is returned.
  - POP3: nothing.

- [ ] **Step 5: Run the suite**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`
Expected: `mail::index::test` passes, every earlier module unchanged (a key account's existing JMAP and IMAP cases now run a pass per request; `mail::search`'s refused tables are unaffected until Task 10).

- [ ] **Step 6: Mail-suite regression, both modes** (ordinary accounts share the dispatcher and the IMAP command loop): `imap::imap_tests` and `jmap::jmap_tests` plain and with `ZA_KEY_ACCOUNTS=1`, then the za suite with `BLOB_STORE=FileSystem` (the end of phase D).

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests jmap::jmap_tests -- --nocapture` and the same for `imap::imap_tests`.
Expected: pass (the pass runs on every keyed request and writes only ids; a variant that fails here is a regression in Task 8 or 9, not a variant to adjust).

- [ ] **Step 7: Compile check, format, commit**

Run: the Global Constraints check; `cargo fmt -p email -p jmap -p imap -p tests -- --check`.

```bash
git add crates/email/src/message/za/search/ crates/jmap/src/api/request.rs crates/jmap/src/email/set.rs crates/jmap/src/email/import.rs crates/imap/src/core/client.rs crates/imap/src/op/append.rs tests/src/za/mail/index.rs tests/src/utils/za.rs
git commit -m "Search: the pass runs before every keyed mail request and after keyed writes; content requests wait for a current index"
```

### Task 10: Search serving: `Email/query`, IMAP SEARCH and SORT, snippets, the unavailable refusal

Implementer: Sol (`gpt-6.1-sol`, `high`); reviewer: Astra (`gpt-6-astra`, `xhigh`).
This task owns the validated acquisition `za_view`, a concurrency kernel that Task 11's
snapshot reuses (PL3); the rest is integration into upstream's query engine.

**Files:**
- Create: `crates/email/src/message/za/search/view.rs` (`ZaView`, `ViewNeed`, `ViewError`, `za_view`, `counter_at_or_past`: the one validated acquisition of a key account's cache, loaded index and content, PL3; declared in `crates/email/src/message/za/search/mod.rs`)
- Create: `crates/email/src/message/za/search/query.rs` (`za_resolve_residual`: the store-typed part, shared by jmap and imap), `crates/jmap/src/email/za_search.rs` (the resolution of a key account's content leaves and sorts; `pub mod za_search;` in `crates/jmap/src/email/mod.rs:20-27`)
- Create: `crates/imap/src/op/za_search.rs` (the IMAP side of the same; declared in `crates/imap/src/op/mod.rs`)
- Modify: `crates/jmap/src/email/za.rs` (`za_refuse_content_query` `:148-171` becomes `za_content_query`; `za_refuse_snippet_terms` `:176-189` and `first_snippet_term` `:136-144` are deleted with their tests; the classifiers `filter_needs_content` `:27-61`, `query_filter_needs_content` `:70`, `sort_needs_content` `:79`, `first_refused_filter` `:118`, `first_refused_sort` `:126` stay)
- Modify: `crates/jmap/src/email/query.rs:112-124` (the refusal call), `:148-154` (`QueryFilter::parse` gains the content argument), `:165-179` (`windowed = None` for a key account), `:181-196` (the `filter_account` call replaced), `:631-688` (`sort_comparators` gains the content argument for `from`, `to`, `subject`, `sentAt`)
- Modify: `crates/jmap/src/email/query/filter.rs:212-291` (`QueryFilter::parse`: one fork arm before `row.absorb`), `:112-175` (`RowFilter::absorb`'s `HasAttachment` `:152` and `SentBefore`/`SentAfter` `:168-171` arms are bypassed by that fork arm for a key account; unchanged otherwise)
- Modify: crates/jmap/src/changes/query.rs:74-103 (acquire the content
  view before the unchanged-state return and lend it to the query evaluator).
- Modify: `crates/jmap/src/email/snippet.rs:61` (the refusal call removed; `terms` are now read for a key account with keys at `:164-172`)
- Modify: `crates/imap/src/op/search.rs:310-319` (the refusal becomes the content guard), `:401-410` (`Keyword`) and `:501` (`Unkeyword`) for `$HasAttachment`/`$HasNoAttachment`, `:431-458` (`SentBefore`, `SentOn`, `SentSince`), `:740-775` (`Sort::Date`, `From`, `DisplayFrom`, `Subject`, `To`, `DisplayTo`), `:777-782` (the `query_emails` call: the residual resolved first)
- Modify: `tests/src/za/mail/search.rs` (the turnaround: `imap_refused_keys` `:454`, `refused_commands` `:413`, `jmap_refused_filters_and_sorts` `:845` become served tables; `nested_search_refused` `:252`, `visible_field_document` `:1143`, `query_changes_collapse_threads` `:764` stay; two new cases)
- Test: `cargo test -p jmap --lib za_search`, `cargo test -p imap --lib za_search`, the za suite's `mail::search`

**Interfaces:**
- Consumes (Task 9): `Server::za_search_ensure_current(&self, account_id: u32, keys: &SessionKeys, deadline: Instant) -> Result<Arc<IndexEntry>, Unavailable>`; `Unavailable::{Rebuilding, Transient(TransientClass), OverCap, Budget, NewerFormat, NotCurrent, Unresolved}`.
- Consumes (Task 6): `IndexEntry { index: Arc<RwLock<Option<LoadedIndex>>>, .. }` with `read_loaded()`, `LoadedIndex { manifest, content: Option<LoadedContent>, scanned_to, .. }` (`content.as_ref().map(|c| &c.part)` is the `ContentPart`), the `Manifest` fields `indexed_to`, `content_to`, `cursor`, `reconciliation`.
- Consumes (Task 4): `ContentPart::resolve(&self, filters: &[SearchFilter]) -> Result<Vec<SearchFilter>, ResolveError>` (plans each `Leaf` inside `store`, since `Leaf::plan` is `pub(crate)`, `crates/store/src/search/leaf.rs:51`, and returns the list with every `Text` and `KeyValue` leaf replaced by a `DocumentSet`; `ResolveError::{Unresolvable(LeafKind), TooLarge(trc::Error), Corrupt(u32)}`), `ContentPart::sort_ranks(&self, field: SortField, documents: &RoaringBitmap) -> AHashMap<u32, u32>` with `SortField::{From, To, Subject, SentAt}` (dense ranks over `documents`, as `message_sort_ranks` computes them, `sortkeys.rs:202-218`), `ContentPart::sent_at(&self, document_id: u32) -> Option<i64>`, `ContentPart::has_attachment(&self, document_id: u32) -> bool`, `ContentPart::document_ids(&self) -> &RoaringBitmap` (every document with a content record).
- Consumes (Task 2): `ZA_MAIL_SEARCH_UNAVAILABLE` (the text), `ZA_SEARCH_UNAVAILABLE`, `ZA_SEARCH_UNRESOLVED` (`common::za::search::trace`), `ZA_MAIL_SEARCH_REFUSED` (`common::za`, `crates/common/src/za/mod.rs:31`), the test-mode configuration override (`set_search_config_override`, Task 2; `search_config_guard`, Task 9; plan call D3).
- Consumes (upstream): `SearchQuery::new(SearchIndex::Email).with_filters(..).with_mask(..).filter() -> QueryResults` and `QueryResults::into_bitmap()` (`crates/store/src/search/local.rs:16-77, 176-204`: a pure in-memory evaluation of `DocumentSet` leaves; `Text`, `Integer` and `KeyValue` leaves are skipped by `filter`, `:109-113`, which is why every leaf must be a `DocumentSet` before the call), `SearchFilter::is_in_set` (`document.rs:292`), `SearchComparator::sorted_set`, `MessageComparator::{Search, SortKey, Cache}` (`sortkeys.rs:46-61`), `query_emails` (`sortkeys.rs:66-130`: for a key account it receives only `Search` comparators, so `message_sort_ranks`, `:134-200`, which reads the store's sort keys a stub does not hold, is never called for one).
- Consumes (Task 6): `Server::za_search_entry`, `Server::za_search_read_manifest`, `Server::za_search_reload_if_moved`, `IndexEntry::read_loaded() -> Option<LoadedGuard>`, `LoadFailure`, `NoMapReason`; (Task 5) `manifest_hash`, `Manifest { newest_row, epoch, .. }`, `ManifestValue`, `za_search_key`; (upstream) `get_cached_messages` (`crates/email/src/cache/mod.rs:59-70`, which pairs each immutable cache with its own change state, `:449-483`) and the per-account stale mark `commit_batch` applies (`crates/common/src/cache/invalidate.rs:49-53`; find the entry point and call it).
- Produces, in `email::message::za::search::view` (PL3):
  - `pub struct ZaView { pub entry: Arc<IndexEntry>, pub index: LoadedGuard, pub cache: Arc<MessageStoreCache>, pub epoch: u64 }` with `pub fn content(&self) -> &ContentPart` (`self.index.content.as_ref().expect("a content view").part`: present by construction for a view acquired with `ViewNeed::Content`, never called on another). Task 11 adds the map methods to it and names it `ZaSnapshot` (`pub type ZaSnapshot = ZaView;`).
  - `pub enum ViewNeed { Map, Content { deadline: Instant } }`, `pub enum ViewError { NoMap(NoMapReason), Unavailable(Unavailable) }`.
  - `pub fn counter_at_or_past(last_change_id: u64, manifest: &Manifest) -> bool` (`last_change_id >= max(manifest.newest_row.change_id, manifest.epoch)`).
  - `pub async fn za_view(server: &Server, account_id: u32, keys: &SessionKeys, need: ViewNeed) -> Result<ZaView, ViewError>`: the procedure **The validated view** below. The cache, the guard and the content the handler uses all come out of one round of it; the handler never pairs them with a cache or a guard taken elsewhere.
- Produces, in `email::message::za::search::query` (`za_resolve_residual`, since `imap` does not depend on `jmap`) and in `jmap::email::za_search` (the rest):
  - `pub async fn za_content_query(server: &Server, access_token: &AccessToken, account_id: u32, filters: &[Filter<EmailQueryFilter>], sorts: &[Comparator<EmailComparator>]) -> trc::Result<Option<ZaView>>`: `Ok(None)` when no filter or sort needs content (`first_refused_filter`, `first_refused_sort`) or the account is not a key account; for a key account without keys on the token (`za_mail_keys` is `Err`), the keyless refusal of today: `UnsupportedFilter`/`UnsupportedSort` naming the first property with `ZA_MAIL_SEARCH_REFUSED`; with keys, `za_view(server, account_id, keys, ViewNeed::Content { deadline: now + ZA_SEARCH_WAIT_SECS })`: `Ok(view)` → `Some(view)`; `Err(Unavailable(r))` → the unavailable refusal (below) with `ZA_SEARCH_UNAVAILABLE` traced carrying the account id and the reason class; `Err(NoMap(r))` → the unavailable refusal too, with `NewerFormat` → `Unavailable::NewerFormat`, `Destroying` → `Transient(Store)`, `Transient(t)` → `Transient(t)` (a content request names a content property, so its refusal is the content refusal; Task 11's `za_no_map_error` is for the id-emitting reads).
  - `pub use email::message::za::search::ensure::is_current as za_content_current;` (7.4, G14: Task 9's seven conditions, re-exported, not a second definition).
  - `pub fn za_unavailable(property: &dyn Display, is_sort: bool, reason: &Unavailable) -> trc::Error`: `trc::JmapEvent::UnsupportedSort` when `is_sort`, else `UnsupportedFilter`, `.details(format!("{property}: {ZA_MAIL_SEARCH_UNAVAILABLE}"))`.
  - `pub fn za_resolve_residual(filters: Vec<SearchFilter>, content: &ContentPart) -> Result<Vec<SearchFilter>, Unavailable>`: strips `Integer{field: SearchField::AccountId, ..}`, calls `content.resolve(&filters)` (Task 4), and maps any `ResolveError` to `Err(Unavailable::Unresolved)` traced `ZA_SEARCH_UNRESOLVED` with the leaf's kind as `text`, `key-value` or `integer` and never its field or value (G9); operators and `DocumentSet`s pass through.
  - `pub fn za_resolve_condition(condition: &EmailFilter, content: &ContentPart, cache: &MessageStoreCache) -> Option<RoaringBitmap>`: `SentBefore(t)` → documents with `Some(s) if s < t`; `SentAfter(t)` → `Some(s) if s > t` (every stored record holds the effective time, Task 8, PL10, so a message with no `Date` is compared at its received time, as upstream's `sent_at_absolute` is at `filter.rs:94`) (the comparisons `RowFilter::absorb` makes on the cache, `filter.rs:168-171`, over the index's `sent_at` instead); `HasAttachment(b)` → the attachment bit equal to `b`; `HasKeyword(k)`/`NotKeyword(k)` with `is_attachment_keyword(k)` (`za.rs:103-110`) → the bit set or clear (`$hasattachment` set, `$hasnoattachment` clear, and the complements); `AllInThreadHaveKeyword`/`SomeInThreadHaveKeyword`/`NoneInThreadHaveKeyword` of an attachment keyword → `thread_keywords`'s rule (`email/cache/email.rs:279`) with the bit in place of the keyword, grouped by the cache's `thread_id()` until Task 12 switches it to the map; every other condition → `None` (upstream's path).
  - `pub fn za_content_condition(condition: &EmailFilter) -> bool`: true for exactly the conditions `za_resolve_condition` answers.
  - `pub fn za_sort_comparator(property: &EmailComparator, ascending: bool, content: &ContentPart, documents: &RoaringBitmap) -> Option<MessageComparator>`: `From`/`To`/`Subject` → `MessageComparator::Search(SearchComparator::sorted_set(content.sort_ranks(field, documents), ascending))`; `SentAt` → the same over `SortField::SentAt`; `HasKeyword(k)` with `is_attachment_keyword(k)` → `MessageComparator::Search(SearchComparator::set(the documents whose attachment bit is set for `$hasattachment`, clear for `$hasnoattachment`, ascending))`; `AllInThreadHaveKeyword(k)` / `SomeInThreadHaveKeyword(k)` with an attachment keyword → `SearchComparator::set` over `thread_keywords`'s rule (`email/cache/email.rs:279`) with the bit in place of the keyword, grouped by the cache's `thread_id()` until Task 12 switches it to the map (PL9: `sort_needs_content` classifies these sorts as content, `za.rs:79-94`, so a key account always has the view for them, and the cache comparator upstream builds, `query.rs:663-679`, would read keyword bits the stub does not carry); others `None`.
- Produces, in `imap::op::za_search`: `pub async fn za_imap_content_guard(data: &SessionData<T>, mailbox: &SelectedMailbox, filters: &[Filter], sorts: &[Comparator]) -> trc::Result<Option<ZaView>>` (the IMAP form of `za_content_query`: `za_filter_needs_content` and `za_sort_needs_content`, `search.rs:797, 853`, decide; the session's keys are `data.access_token.za_keys_for(account_id)`; the keyless case is `ImapEvent::Error` with `ZA_MAIL_SEARCH_REFUSED` and `ResponseCode::Cannot` as today, `:316-319`; the unavailable case the same shape with `ZA_MAIL_SEARCH_UNAVAILABLE`), `pub enum SentOp { Before, On, Since }` and `pub fn za_imap_sent(content: &ContentPart, op: SentOp, date: i64) -> RoaringBitmap` for `SENTBEFORE` (`sent_at < date`), `SENTON` (`date <= sent_at < date + 86400`, `:438-450`), `SENTSINCE` (`sent_at >= date`), `pub fn za_imap_attachment(content: &ContentPart, has: bool) -> RoaringBitmap`.

**The validated view, as a procedure** (PL3; spec 6.2 and 7.4). One loop takes the cache, the read guard and, for a content request, the current content, and validates exactly the guard and the cache it returns. It holds no guard while anything that takes `index.write()` runs (a pass, a reload, a content load), so it cannot deadlock against itself, and a swap or a publication between two of its reads only sends it round again. A `LoadFailure` maps to `ViewError::NoMap` for `Map` (`Loss` → trace `ZA_SEARCH_REBUILD` and `NoMap(Transient(Store))` until Task 14 replaces the arm with `za_search_rebuild_start` and one retry; `Transient(t)` → `NoMap(Transient(t))`; `NoMap(r)` → `NoMap(r)`) and through Task 9's `map_load` to `ViewError::Unavailable` for `Content`.

0. `rounds = 0`; `Map` only: `entry = za_search_entry(account_id, keys)`. `Content` has no preliminary entry load and enters through step 1.
1. `Content` only: `entry = za_search_ensure_current(account_id, keys, deadline)?` (Task 9: the passes, the wait on a running pass, the content load; `Err(u)` → `Err(Unavailable(u))`). Nothing is held here.
2. `cache = get_cached_messages(account_id)`; then `search_snapshot_pause_point(account_id).await` (`SEARCH_SNAPSHOT_PAUSE`, plan call D4: between the state read and the manifest read).
3. `(bytes, value) = za_search_read_manifest(account_id, &za_search_key(keys.swk()))` (one point read): `None` → step 0's loss arm; `Destroying` → `NoMap(Destroying)`; an error → its class.
4. `guard = entry.read_loaded().await`; `None` → `rounds += 1`; `Map` reloads its entry through `za_search_entry(..)`, while `Content` re-enters through step 1 without a separate entry load.
5. `manifest_hash(&bytes) != guard.manifest_bytes_hash` → `drop(guard)`, `za_search_reload_if_moved(&entry, keys)` (its error → its class), `rounds += 1`, back to step 1.
6. `!counter_at_or_past(cache.last_change_id, &guard.manifest)`, or for `Content` only `cache.last_change_id < guard.manifest.indexed_to.change_id` → `drop(guard)`, mark the account's Email group stale (the call `commit_batch` makes; name it in a comment), `rounds += 1`, back to step 1. The additional floor compares the group counter, whose empty value is zero; Map/FR7 is unchanged. Do not require equality or use `scanned_to` as the floor.
7. `Content` only: call Task 9's `check_content_cap(&guard, account_max_mb << 20)` under this guard; refusal maps to `Unavailable(OverCap)`. Then `guard.content.is_none() || !is_current(&guard, cache.last_change_id)` → `drop(guard)`; `Instant::now() >= deadline` → `Err(Unavailable(NotCurrent))`; else back to step 1 (not a round; the deadline bounds it).
8. `search_view_pause_point(account_id).await` (`SEARCH_VIEW_PAUSE`, test mode: a view acquired and held, before any rendering; Codex rev-6 PL3's regression parks here, since a pause inside the loop, at `SEARCH_SNAPSHOT_PAUSE`, would be answered by the loop's own reload), then `Ok(ZaView { entry, epoch: guard.manifest.epoch, index: guard, cache })`.

For `Content`, convert the original deadline to Tokio time once and apply
`timeout_at` to the cache read, authenticated manifest read, loaded-guard read,
and standalone reload after dropping the guard. Timeout is
`Unavailable(NotCurrent)`; no retry receives a fresh deadline. Check expiry
after ensure-current and before returning a Content view. Do not wrap the
whole ensure-current future: its owned pass must finish its transaction.
Map retains its direct waits and existing validation-round limit. A cancelled
read-side reload may discard optional content, but leaves the old committed
manifest/thread coherent for a later load. Test-only pauses retain their
existing placement.

`rounds > 3` → `NoMap(Transient(Validation))` for `Map`, `Unavailable(Transient(Validation))` for `Content`. The handler reads selected content from `view.content()`, its map from `view.index` (Task 11) and its cache-based state and presence from `view.cache`. Write responses use Task 11's final view for `newState`, preserving pre-mutation `oldState`. Content is checked current against that same cache. Standalone content-query results and state use that view. Keyed queryChanges follows section 6.3's combination of current readable rows and content with its effective historical identity endpoint. A view lives for one method call or one IMAP command and is dropped before the call returns; no path that runs a pass, a reload or a content load (Task 9's triggers, the post-commit pass of `Email/set` create, `Email/import` and APPEND) is reached while one is held, since each takes `index.write()`; and a request task holds one view or snapshot at a time, never two (tokio's `RwLock` is write-preferring, so a second read guard taken behind a queued writer waits for that writer, which may be a pass waiting for the first guard; FR8).

- [ ] **Step 1: Write the failing unit tests in `za_search.rs`'s `mod tests`**

```rust
#[test]
fn every_text_and_key_value_leaf_becomes_a_document_set() {
    let content = fixture_content(); // the module's one fixture: subjects 1 "bravo", 2 "alpha", 3 "alpha"; header X-Private: yes on 1; sent_at 1 → 100, 2 → 200, 3 → 300; the attachment bit on 2
    let filters = vec![SearchFilter::And,
        SearchFilter::has_english_text(EmailSearchField::Subject, "alpha"),
        SearchFilter::has_key_value(EmailSearchField::Headers, "x-private", KeyValueMatch::Exists),
        SearchFilter::End];
    let resolved = za_resolve_residual(filters, &content).unwrap();
    assert!(matches!(resolved[1], SearchFilter::DocumentSet(ref s) if s.iter().eq([2, 3])));
    assert!(matches!(resolved[2], SearchFilter::DocumentSet(ref s) if s.iter().eq([1])));
    assert_eq!(SearchQuery::new(SearchIndex::Email).with_filters(resolved).with_mask((1..=3).collect()).filter().into_bitmap().len(), 0);
}

#[test]
fn an_unresolvable_leaf_refuses_instead_of_widening() {
    let content = fixture_content();
    let filters = vec![SearchFilter::integer_eq(EmailSearchField::Size, 7)];
    assert!(matches!(za_resolve_residual(filters, &content), Err(Unavailable::Unresolved)));
}

#[test]
fn sent_and_attachment_conditions_read_the_index_not_the_cache() {
    let content = fixture_content(); // sent_at: 1 → 100, 2 → 200, 3 → 300; attachment on 2
    let cache = empty_cache(); // the stub's sent_at is 0 for every key message (mail spec 5.4)
    assert_eq!(za_resolve_condition(&EmailFilter::SentBefore(UTCDate::from_timestamp(200)), &content, &cache).unwrap().iter().collect::<Vec<_>>(), [1]);
    assert_eq!(za_resolve_condition(&EmailFilter::SentAfter(UTCDate::from_timestamp(200)), &content, &cache).unwrap().iter().collect::<Vec<_>>(), [3]);
    assert_eq!(za_resolve_condition(&EmailFilter::HasAttachment(true), &content, &cache).unwrap().iter().collect::<Vec<_>>(), [2]);
    assert_eq!(za_resolve_condition(&EmailFilter::NotKeyword(Keyword::HasAttachment), &content, &cache).unwrap().iter().collect::<Vec<_>>(), [1, 3]);
    assert!(za_resolve_condition(&EmailFilter::InMailbox(Id::from(1u32)), &content, &cache).is_none());
}

#[test]
fn sort_comparators_are_dense_ranks_over_the_result_set() {
    let content = fixture_content(); // subjects: 1 "bravo", 2 "alpha", 3 "alpha"
    let MessageComparator::Search(SearchComparator::SortedSet { set, ascending }) =
        za_sort_comparator(&EmailComparator::Subject, true, &content, &(1..=3).collect()).unwrap() else { panic!() };
    assert!(ascending);
    assert_eq!((set[&2], set[&3], set[&1]), (1, 1, 2));
}

#[test]
fn current_means_scanned_past_the_end_with_content_at_indexed_to() {
    let mut index = loaded_index_at(Checkpoint { change_id: 40, position: 0, complete: true }, 40);
    index.scanned_to = 45;
    assert!(za_content_current(&index, 45));
    assert!(!za_content_current(&index, 46));
    index.manifest.content_to.suspended = true;
    assert!(!za_content_current(&index, 45));
}
```

And in `view.rs`'s `mod tests` (`manifest_with(row, epoch)` builds a `Manifest::default()` with those two fields):

```rust
#[test]
fn the_counter_check_rejects_a_cache_below_the_newest_row_and_passes_an_empty_account() {
    // F2: the cache's counter is behind a publication another node committed.
    assert!(!counter_at_or_past(/* last_change_id */ 41, &manifest_with(RevisionRow { revision: 3, change_id: 42 }, /* epoch */ 0)));
    assert!(counter_at_or_past(42, &manifest_with(RevisionRow { revision: 3, change_id: 42 }, 0)));
    // G3: an empty account whose boundary touched only the mailbox log: the group counter moved, the item state did not.
    assert!(counter_at_or_past(/* group counter */ 7, &manifest_with(RevisionRow { revision: 1, change_id: 7 }, /* epoch */ 7)));
    // the epoch binds too
    assert!(!counter_at_or_past(6, &manifest_with(RevisionRow { revision: 1, change_id: 5 }, 7)));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p jmap --lib za_search` and `cargo test -p email --lib --features test_mode za::search::view`
Expected: compile errors naming `za_search`, `za_resolve_residual`, `za_content_current`, `counter_at_or_past`.

- [ ] **Step 3: Implement `za_search.rs` and the five `query.rs` insertions**

In the public Email/query wrapper (query.rs:105–124), acquire with
za_content_query as described above, then invoke an internal query
evaluator that accepts the already acquired optional view by reference
and receives the selected Map or Content mode separately. The evaluator
borrows the view through filtering, sorting and rendering. It performs
no view acquisition. The cache read (:124–128) uses the borrowed view's
cache when present, else upstream's read. The standalone query takes its
item state from that cache (`query_state = cache.get_state(false)`, `:156`).
Derive an optional `ContentPart` only for selected Content and pass it to
`QueryFilter::parse` (`:148-154`), `sort_comparators` (`:155`) and residual
resolution. Choose the mode from the request, independent of optional
content residency in the guard. The keyed evaluator disables the
PostgreSQL windowed walk (`:165-179`, `window.rs:242-354`). In the residual
block (`:181-196`), resolve content leaves with `za_resolve_residual` in
Content mode, preserving its unavailable mapping. Map uses the borrowed
cache and identity projection and supplies its already-resolved
`DocumentSet` and operator residuals to the same in-memory evaluator
without calling `view.content()`. For either keyed mode, evaluate with
`SearchQuery::new(SearchIndex::Email).with_filters(residual).with_mask(filter.readable(&cache)).filter().into_bitmap()`.
Ordinary `None` retains the upstream search-store path.

In `QueryFilter::parse` (`filter.rs:212-291`): the fork arm before `row.absorb(condition)` and before the non-top-level `push_condition`: `if let Some(content) = za && za_content_condition(&condition) { residual.push(SearchFilter::is_in_set(za_resolve_condition(&condition, content, cache).unwrap())); continue; }` (a condition that is not top level goes to `residual` as upstream puts it, so the operator structure is kept). In `sort_comparators` (`:631-688`): for a key account, `From`, `To`, `Subject`, `SentAt` and the three keyword sorts on an attachment keyword go through `za_sort_comparator` with `documents = filter.readable(&cache)`; the `threadId` sort stays `unsupportedSort` (`:681-685`).

At this stage, queryChanges still reads the upstream changes page, then
checks content availability even when it will return early. Keep the
resulting Content view through any query evaluation by calling the
internal evaluator with that borrow. Leave the raw page and intermediate
grouping in place until their owning tasks. Task 11 adds the equivalent
Map acquisition for the keyed metadata-only path, and Task 13 moves
the selected acquisition before keyed page rendering.

- [ ] **Step 4: Write the IMAP unit test and the insertions**

```rust
#[test]
fn senton_is_a_day_window_over_the_index() {
    let content = fixture_dates(); // a second fixture for the day window: sent_at 1 → 86_400, 2 → 172_799, 3 → 172_800
    assert_eq!(za_imap_sent(&content, SentOp::On, 86_400).iter().collect::<Vec<_>>(), [1, 2]);
    assert_eq!(za_imap_sent(&content, SentOp::Since, 172_800).iter().collect::<Vec<_>>(), [3]);
    assert_eq!(za_imap_sent(&content, SentOp::Before, 172_800).iter().collect::<Vec<_>>(), [1, 2]);
}
```

In `SessionData::query` (`search.rs:292`): `let za = za_imap_content_guard(self, mailbox, &imap_filter, &imap_comparator).await?;` replaces `:310-319`, and when it is `Some(view)` the `cache: &MessageStoreCache` parameter (`search.rs:297`) is shadowed by `&*view.cache` for the rest of the function (`let cache = za.as_ref().map_or(cache, |v| &*v.cache);`: the content was checked against that cache, PL3, as on JMAP); the `Filter::Keyword`/`Unkeyword` arms (`:401`, `:501`) push `za_imap_attachment` for the two attachment flags when `za` is some; `SentBefore`/`SentOn`/`SentSince` (`:431-458`) push `za_imap_sent`; `Header` (`:615`), `Text` (`:676`) and the address and body filters keep upstream's `SearchFilter` construction; before `query_emails` (`:777`) the fork runs `za_resolve_residual` over `filters` and converts `Sort::Date` (`:753`, `MessageCacheField::SentAt`), `From`/`DisplayFrom` (`:757`), `Subject` (`:767`), `To`/`DisplayTo` (`:771`) through `za_sort_comparator`'s IMAP twin with `documents = message_ids` (`:324`). `Sort::Cc` stays refused (`:749-753`, upstream). The `NO [CANNOT]` keyless text stays `ZA_MAIL_SEARCH_REFUSED`; the unavailable text is `ZA_MAIL_SEARCH_UNAVAILABLE`.

- [ ] **Step 5: Lift the snippet refusal**

Remove the call at `snippet.rs:61` and its import `:7`; delete `za_refuse_snippet_terms`, `first_snippet_term` and their tests in `za.rs`. A key account's request now reaches `za_mail_keys` (`:167`), which refuses a keyless token as it does every content read (mail spec 10). The supplied `emailIds` keep `email_by_jmap_id` (`:183`, `:213`); Task 11 translates them.

- [ ] **Step 6: Turn `tests/src/za/mail/search.rs` around**

`imap_refused_keys` becomes `imap_served_content_keys`: every command of `refused_commands()` (`:413-446`) is sent to both fixtures and the key fixture's result line equals the plain fixture's (`served`, `:296`), the `THREAD REFERENCES UTF-8 FROM x` line included (singletons until Task 12 groups them, so compare the sorted UID set, not the grouping); the `Traced` assertion stays: no `SecurityEvent::Unauthorized`, and `TERM` (`:456`) appears in no event. `jmap_refused_filters_and_sorts` becomes `jmap_served_content_filters_and_sorts`: each filter of `:847-880` and each content sort runs on both fixtures and the key account's subject list equals the plain account's (the seeds' headers at `:42-90` decide the expected sets: `text`/`subject`/`body` on a word of one seed, `from`/`to`/`cc`/`bcc` on the seeds' addresses, `header` on a header only `fifth()` carries, `hasAttachment` true and false, `sentBefore`/`sentAfter` on the seeds' `Date` headers, the attachment keywords in `hasKeyword`/`notKeyword` and the three thread forms; and the sorts `hasKeyword`, `allInThreadHaveKeyword` and `someInThreadHaveKeyword` on `$hasattachment`, each ascending and descending, PL9); the `metadataExists` filter stays refused (ruling 57, unchanged). New cases: `keyless_search_refused` (a Bearer token on `Email/query` with `{"text": "x"}` answers `unsupportedFilter` with `ZA_MAIL_SEARCH_REFUSED`; the token is obtained as `gates.rs:84-213` obtains its `Keyless::Bearer`); `unavailable_refusal` (`search_config_guard!(wait_secs: 1)`; deliver one message; park a first keyed IMAP session's pass on `SEARCH_PASS_PAUSE` (`HookParked`); on a second keyed IMAP session of the account, whose pre-command pass finds `Running` and waits on the budget, `SEARCH SUBJECT alpha` answers `NO [CANNOT] <ZA_MAIL_SEARCH_UNAVAILABLE>` within 2 s; JMAP `Email/query` `{"text": "alpha"}` answers `unsupportedFilter` with the same text; `SEARCH OR SUBJECT alpha SEEN` is refused whole; release the pause and clear the override; both are served). Three acquisition cases (PL3), each on the module's key fixture, each request wrapped in `tokio::time::timeout(Duration::from_secs(20))` so a deadlock fails the case instead of hanging the suite: `view_publication_while_parked` (park a keyed `Email/query` `{"text": "alpha"}` at `SEARCH_SNAPSHOT_PAUSE`, after its cache read; deliver a message whose body holds `alpha` and, on a second keyed connection with `set_search_swap_skip(Some(id))`, run one keyed request so its pass commits without swapping, as another node's would; clear the flag; release: the parked query completes, its `ids` hold the new message and its `queryState` is the post-delivery state; a fresh `Email/query` returns the same `ids` and state); `view_local_swap_while_acquiring` (park a pass at `SEARCH_SWAP_PAUSE` after its commit, the delivery's message not yet swapped in; on a second keyed connection `Email/get` of every id and `Thread/get` of their threads complete while the pass is still parked (map acquisitions: a content request would wait on the parked pass's lock, Task 9's rule, which `unavailable_refusal` covers); release; a fresh `Email/get` and `Thread/get` then return the same ids, threads and states as the two answered while parked); `view_delivery_between_reads_keeps_results_and_state_paired` (park a keyed `Email/query` `{"text": "alpha"}` at `SEARCH_SNAPSHOT_PAUSE`; deliver a matching message with no keyed request, so no pass runs; release: the answer omits the new message and its `queryState` is the pre-delivery state, never the new state without the message; the next `Email/query` holds the message under the new state). `visible_field_document` (`:1143`) stays: `key.terms == 0` and `documents_are_ids_only()` (invariant 27).

Additional acquisition cases:

- `content_view_resamples_after_nonpublishing_deletion` (P2-CONTENT-FLOOR):
  index readable D/subject-alpha and S/subject-beta; positively establish D's
  posting and the NOT-alpha result `{S}`. Park a second NOT-alpha query after
  cache sampling. Actually delete D and run a deletion-only nonpublishing pass.
  Require changed manifest hash, complete/equal advanced checkpoints, unchanged
  newest row/epoch/content head, and D removed from resident content. Resume:
  the answer must be `{S}` under the refreshed post-deletion state. Removing
  only the Content checkpoint floor must return D and fail. Zero counters and
  `indexed_to < E <= scanned_to` remain valid; partial, pending, or suspended
  content remains unavailable under existing conditions. Map's floor is unchanged.
- Final-guard cap: after ensure-current, use the existing snapshot pause to
  change only an authenticated durable/resident descriptor's size while its
  counter and content stay aligned. Re-encode and authenticate the durable
  manifest, set the resident manifest to that descriptor, and set its
  `manifest_bytes_hash` to `manifest_hash(encoded_bytes)` before releasing the
  snapshot pause; the final hash comparison must succeed without reload or
  retry. Exact cap serves; one byte over refuses the whole query; Map remains
  available. Restore the original durable bytes, resident manifest and resident
  hash. This is synthetic descriptor coverage, not a physical crossing-pass
  test. Removing only the final-view cap check must fail the case.
- Original read deadline: hold the actual pass mutex and index write guard
  before Content acquisition, then separately hold a writer after the snapshot
  pause. Require `NotCurrent` under the original deadline in both schedules,
  and successful unchanged queries after releasing the locks. No unbounded
  preliminary entry load or refreshed wait window may mask the original limit.

- [ ] **Step 7: Run the tests and checks** (including `imap::imap_tests` and `jmap::jmap_tests` plain and with `ZA_KEY_ACCOUNTS=1`: ordinary accounts share `email_query`, the filter parser and IMAP's `query`)

Run: `cargo test -p jmap --lib za_search`; `cargo test -p imap --lib za_search`; the per-task compile check; `cargo check -p tests --tests`; `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`.
Expected: pass; `clean`; clean; the za suite passes with `mail::search`'s served tables.

- [ ] **Step 8: Commit**

```bash
git add crates/email/src/message/za/search/ crates/jmap/src/email/za_search.rs crates/jmap/src/email/za.rs crates/jmap/src/email/mod.rs crates/jmap/src/email/query.rs crates/jmap/src/email/query/filter.rs crates/jmap/src/changes/query.rs crates/jmap/src/email/snippet.rs crates/imap/src/op/za_search.rs crates/imap/src/op/mod.rs crates/imap/src/op/search.rs tests/src/za/mail/search.rs
git commit -m "Release 7: search and sorts served from the sealed index, with the unavailable refusal"
```

### Task 11: Ids and the validated snapshot

Implementer: Sol (`gpt-6.1-sol`, `high`); reviewer: Astra (`gpt-6-astra`, `xhigh`).
A kernel: the consistency rule between a response's state and the thread map, and the gate
that keeps singleton ids off every keyed response.

**Files:**
- Create: `crates/email/src/message/za/search/snapshot.rs` (`ZaSnapshot`, the procedure, the id helpers)
- Modify: crates/email/src/message/za/search/view.rs (optional caller minimum
  cache counter in the existing validation loop, used here by write responses)
  and crates/jmap/src/changes/query.rs (borrow the same Map view for keyed
  metadata-only queries).
- Modify: `crates/jmap/src/email/get.rs:174-199` (the cache and the ids), `:210` (keys, already read), `:229-236`, `:257-262`, `:272-277` (`email_by_jmap_id` on supplied ids), `:415-425` (`Id`, `ThreadId`)
- Modify: `crates/jmap/src/email/query.rs:484-492` (`Page::emit`), `:600-603` (`Page::top`), `:456-461` (`count_matches`, Task 12 completes it): the prefix passed to `builder.add` is the map's
- Modify: `crates/jmap/src/email/set.rs:1172-1174` (update's prefix filter), `:1620-1623` (destroy's lookup), `:1758` (create's response)
- Modify: `crates/jmap/src/email/import.rs:347` (the response), `crates/jmap/src/email/mod.rs:29-37` (`ingested_into_object` gains the thread id to use)
- Modify: `crates/jmap/src/email/snippet.rs:183, 213` (supplied ids)
- Modify: `crates/jmap/src/thread/get.rs:35-106` (the state from the view's cache, D13; members by the map are Task 12's), `crates/jmap/src/blob/get.rs:188-193`, `crates/jmap/src/submission/set.rs:449-450`, `crates/jmap/src/submission/get.rs:237-249`
- Modify: `crates/jmap/src/api/request.rs:296` (`Mailbox/get`), `:304` (`Thread/get`), `:318` (`EmailSubmission/get`), `:439` (`Email/query`), `:455` (`EmailSubmission/query`), `:816` (`Blob/lookup`): `za_assert_mail_route` becomes `za_assert_mail_keys`; `crates/jmap/src/changes/get.rs:217-229` (`Email`, `Thread`, `EmailSubmission` changes need keys; `Mailbox` keeps the route)
- Modify: `crates/imap/src/op/fetch/mod.rs:585-592` (`ObjectId`), `crates/imap/src/op/search.rs:565-586` (`EmailId`, `ThreadId`)
- Modify: `crates/email/src/cache/mod.rs:59-70` (`get_cached_messages`: the test-mode `SEARCH_CACHE_HOLD` line), `crates/common/src/za/search/load.rs` (`za_search_read_manifest`: the test-mode `SEARCH_MANIFEST_FAIL_ONCE` line)
- Create: `tests/src/za/mail/threads.rs` (`pub mod threads;` in `tests/src/za/mail/mod.rs:50-59`; called in `tests/src/za/mod.rs` after `mail::search::test` and before `mail::traces::test`)
- Test: `cargo test -p email --lib --features test_mode za::search::snapshot`, the za suite's `mail::threads`

**Interfaces:**
- Consumes (Task 6): `Server::za_search_entry(&self, account_id, keys) -> Result<Arc<IndexEntry>, LoadFailure>`, `Server::za_search_read_manifest(&self, account_id, key: &ZaSearchKey) -> Result<Option<(Vec<u8>, ManifestValue)>, LoadFailure>`, `Server::za_search_reload_if_moved(&self, entry, keys) -> Result<bool, LoadFailure>`, `LoadedIndex { manifest_bytes_hash, manifest, thread: ThreadPart, .. }`, `NoMapReason::{NewerFormat, Destroying, Transient(TransientClass)}`.
- Consumes (Task 3): `ThreadPart::current_thread(&self, document_id) -> u32`, `members(&self, thread_id) -> &[u32]` (every record, present or tombstoned; FR7).
- Consumes (Task 5): `manifest_hash`, `Manifest { newest_row: RevisionRow { change_id, .. }, epoch, .. }`.
- Consumes (upstream): `MessageStoreCache { last_change_id, emails, .. }` (`crates/common/src/lib.rs:288-296`), `MessageCacheFetch::get_cached_messages` (`crates/email/src/cache/mod.rs:59-70`, revalidating when older than the interval or marked stale, `:55-57`), the stale mark the message cache honours (the call `commit_batch` makes, `crates/common/src/cache/invalidate.rs:49-53`; find the per-account "mark stale" entry point and call it), `StateManager::get_state(account_id, SyncCollection::Thread)` (`crates/jmap/src/changes/state.rs:58-66`, upstream's store read of the Thread state, which the fork replaces for a key account with the view's cache counter, D13; Thread shares Email's group, `crates/types/src/collection.rs:228-231`), `Id::from_parts`, `Id::prefix_id`, `Id::document_id` (`crates/types/src/id.rs:67-85`), `email_by_jmap_id` (`crates/email/src/cache/email.rs:620-624`: a lookup by document id filtered on the prefix), `ResponseCode::Unavailable` (`crates/imap-proto/src/protocol/mod.rs:734`), the JMAP mapping of any non-JMAP `trc::Error` to `serverUnavailable` (`crates/jmap-proto/src/error/method.rs:190-198`).
- Consumes (Task 10): `ZaView { entry, index: LoadedGuard, cache, epoch }`, `ViewNeed`, `ViewError`, `za_view`, `counter_at_or_past` (`email::message::za::search::view`, PL3).
- Produces, in `email::message::za::search::snapshot`:
  - `pub type ZaSnapshot = ZaView;` (Task 10's view; this task adds the map methods to it in `snapshot.rs`).
  - `Server::za_snapshot(&self, account_id: u32, keys: &SessionKeys) -> Result<ZaSnapshot, NoMapReason>`: `za_view(self, account_id, keys, ViewNeed::Map)`, `ViewError::NoMap(r)` → `r` (a `Map` acquisition never answers `Unavailable`).
  - Generalize Task 10's loop through za_view_with_minimum(server, account_id,
    keys, need, minimum_cache_counter: Option<u64>). The normal za_view and
    za_snapshot wrappers pass None. In step 6 add cache.last_change_id <
    minimum_cache_counter as a stale-cache condition when a minimum is
    supplied. Drop the guard, mark the Email group stale, and use the same
    validation-round budget. Exhaustion returns `NoMap(Transient(Validation))`
    for Map and `Unavailable(Transient(Validation))` for Content. Route Map
    through `za_no_map_error` and Content through the existing named-property
    unavailable adapter. Content deadline expiry remains
    `Unavailable(NotCurrent)`, using the original deadline. The Content cap,
    checkpoint and currency checks still apply. Use the real cache state.
  - Server::za_response_snapshot(account_id, keys, write_floor: Option<u64>)
    keeps its Map-only interface and delegates to that minimum-aware entry
    with the acknowledged-write floor. Task 13 also supplies a read floor.
  - `impl ZaView` (in `snapshot.rs`): `pub fn map_id(&self, document_id: u32) -> Id` (`Id::from_parts(self.thread_of(document_id), document_id)`); `pub fn thread_of(&self, document_id: u32) -> u32` (`self.index.thread.current_thread(document_id)`: the singleton when the index holds no record); `pub fn check_id(&self, id: Id) -> Option<u32>` (`Some(id.document_id())` when `self.cache.email_by_id(&id.document_id())` is present and `self.thread_of(id.document_id()) == id.prefix_id()`, else `None`); `pub fn thread_members(&self, thread_id: u32) -> Vec<u32>` (`self.index.thread.members(thread_id)`, every record present or tombstoned, filtered by `self.cache.email_by_id(..).is_some()`, plus `thread_id` itself when the cache holds it and its record is absent: a never-indexed message is its own singleton. FR7: the cache decides existence, the record only the thread, so a message the cache holds and the index has tombstoned is listed, as `Email/get` lists it under the same state); `pub fn state(&self) -> u64` (`self.cache.last_change_id`: the Email group's counter the cache was built at, which `za_view`'s step 6 proved at or past the newest row and the epoch; the Thread collection shares that group, `crates/types/src/collection.rs:228-231`, so it is the Thread state the view returns. FR3, plan call D13: never a store read after the cache. A delivery between the cache read and such a read would give an `E` above the cache, and Task 13's stream, which takes existence at `E` from the cache, would call the new message absent and emit nothing for it; the client would never learn of that thread).
  - `pub fn za_no_map_error(reason: NoMapReason) -> trc::Error`: `trc::StoreEvent::UnexpectedError.into_err().details(ZA_SEARCH_UNAVAILABLE).ctx(trc::Key::Reason, reason.as_str())` (a store event maps to `serverUnavailable` on JMAP; the IMAP call sites add `.code(ResponseCode::Unavailable)`), traced once at the site with the account id.
  - `pub fn za_submission_prefix(document_id: u32) -> u32` = `document_id` (the singleton the submission record stores, 6.2).
- Produces, in `jmap::email::mod`: `fn ingested_into_object(email: IngestedEmail, thread_id: u32) -> Map<..>` (upstream's callers pass `email.thread_id`; the two key-account sites pass the snapshot's).
- Places the calls of Task 2's hooks: `search_cache_hold(account_id)` in `get_cached_messages` (while set, the resident cache of that account is returned without revalidating), `take_search_manifest_fail(account_id)` in `za_search_read_manifest` (the next manifest read of the account fails with a store error), (`search_snapshot_pause_point(account_id)` is Task 10's, in `za_view`'s step 2).

**The validated snapshot** (6.2) is Task 10's **validated view** with `ViewNeed::Map` (PL3: the hash and the counter are checked against the very guard and cache the handler receives, and no guard is held across a reload). The handler reads each read-response state from `snapshot.cache` (Email, Mailbox) or `snapshot.state()` (Thread; D13, FR3), and each existence decision from that decision's validated cache, never paired with another cache or later store state. A keyed write selects `newState` and newly advertised mapped IDs from its final response view; `oldState` remains the original precondition state, and supplied-ID acknowledgement keys remain unchanged. `Email/set` uses the separate sequential input and response phases below (P1-STATE). A handler that already holds a content view (`Email/query` with a content filter or sort) uses that view as its snapshot and takes no second one: a second acquisition could pair the content checked against one cache with the state of another.

Handlers of a key account take the snapshot first and use `snapshot.cache` where they used `get_cached_messages`; an ordinary account's path is unchanged (`keys.is_none()`).

- [ ] **Step 1: Write the failing unit tests in `snapshot.rs`'s `mod tests`**

(The counter check's unit test is Task 10's, beside `counter_at_or_past` in `view.rs`.)

```rust
#[test]
fn check_id_accepts_the_current_prefix_only() {
    let snapshot = snapshot_with(/* records */ [(5, /* thread */ 2), (2, 2)], /* present */ [2, 5]);
    assert_eq!(snapshot.check_id(Id::from_parts(2, 5)), Some(5));
    assert_eq!(snapshot.check_id(Id::from_parts(5, 5)), None); // the stale singleton prefix
    assert_eq!(snapshot.check_id(Id::from_parts(2, 9)), None); // not present
    assert_eq!(snapshot.map_id(7), Id::from_parts(7, 7)); // no record: the singleton
}

#[test]
fn thread_members_intersect_the_present_set() {
    let snapshot = snapshot_with([(5, 2), (2, 2), (8, 2)], /* present */ [2, 5]);
    assert_eq!(snapshot.thread_members(2), vec![2, 5]); // 8 is a retained tombstone the cache lacks, never emitted (R14)
    // FR7: a message the cache still holds but the index has tombstoned (the view's cache predates the deletion's pass) stays a member
    let snapshot = snapshot_with_tombstoned([(5, 2), (2, 2)], /* tombstoned */ [(8, 2)], /* present */ [2, 5, 8]);
    assert_eq!(snapshot.thread_members(2), vec![2, 5, 8]);
    assert_eq!(snapshot.map_id(8), Id::from_parts(2, 8)); // and Email/get lists it under the same thread
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p email --lib --features test_mode za::search::snapshot`
Expected: compile errors naming `ZaSnapshot`, `za_snapshot`.

- [ ] **Step 3: Implement `snapshot.rs` and the two hooks**

`za_snapshot` is Task 10's `za_view` with `ViewNeed::Map`; this step adds the type alias, the map methods and the hooks `SEARCH_CACHE_HOLD` and `SEARCH_MANIFEST_FAIL_ONCE`. Add the optional caller minimum to that same validation loop, preserving
normal callers and the shared retry bound. Retain the predicate and
bounded-acquisition regression below an acknowledged write and above the
publication and epoch. Exercise None and equality as positive controls,
one refresh to the floor, and repeated stale returns that exhaust the
existing bound with the selected mode's validation error. For Content
with a caller minimum, prove a no-minimum acquisition serves the same
cache and content, then supply a minimum it misses and assert
`Unavailable(Transient(Validation))` through the existing named-property
adapter. Map exhaustion answers `NoMap(Transient(Validation))` through
`za_no_map_error`. A separate Content deadline control asserts
`Unavailable(NotCurrent)` at the original deadline across retries.
Task 11's real write fixture below still exercises the write floor.

- [ ] **Step 4: Map on the way out, check on the way in, at every site**

- `Email/get` (`get.rs:174-199`): with keys (`:210` already reads them; move that read above `:174`), `snapshot = za_snapshot(..)` and `cache = snapshot.cache.clone()`; the `all_ids` closure maps `Id::from_parts(snapshot.thread_of(item.document_id()), item.document_id())`; each supplied id goes through `snapshot.check_id(id)` and a `None` is `push_not_found(id)` (`:229-236`, `:257-262`, `:272-277` use `email_by_id(&document_id)` after the check instead of `email_by_jmap_id`); `EmailProperty::Id` and `ThreadId` (`:415-425`) use the mapped id; `state` (`:198`) stays `cache.get_state(false)` of the snapshot's cache.
- `Email/query` (`query.rs`): `Page::emit` (`:484-492`), `Page::count` (`:499-505`: the `collapseThreads` total counted after the limit, through `count_rest` `:522-530` and the tail loop `:618-620`), `Page::top` (`:600-603`) and `count_matches` (`:456-461`) take the thread id from `snapshot.thread_of(document_id)` when a snapshot is present (a `Option<&ZaSnapshot>` threaded through `Page`; every place `Page` reads `message.thread_id()` takes it, PL9); the `anchor` then matches as a whole u64 (`api/query.rs:45, 98`: a stale prefix is `anchorNotFound`, upstream's own outcome). When Task 10's `za_content_query` returned a view, that view is the snapshot; otherwise `za_snapshot` is taken once, in place of the cache read at `:124-128` (PL3: never both).
- Keyed Email/queryChanges at this stage borrows the Content view obtained
  in Task 10 or one Map view when the query needs only metadata. The public
  Email/query acquires its own view and uses the same evaluator. For the
  keyed composite path the upstream page remains in use until Task 13.
  Pass the selected mode to the evaluator so it reads content only for
  Content, even when a Map guard happens to hold a content part. Keep the
  acquisition and evaluator split for Task 13.
- `Email/set` (`set.rs`, P1-STATE): retain the original pre-ingest `ifInState`/`oldState`. After create flush, `recheck.finish` and Task 9's selected post-write hook, acquire an **input view** before updates. Both prefix and existence lookups use that view's cache: replace update's stored-prefix check (`:1172-1174`) and destroy's `email_by_jmap_id` (`:1620-1623`) with `input.check_id(id)` for keyed calls; preserve the approved keyless document-only lookup. Acknowledgement keys in `updated`, `destroyed`, `notUpdated` and `notDestroyed` echo the supplied IDs, as upstream does. Keep same-call creation-reference objects available during ingestion, but finalize their externally advertised `id` and `threadId` only at the response seam.
  After all acknowledged update/destroy commits, drop the input view before acquiring the **final response view** through `za_response_snapshot(.., last_change_id)`. Normal commits already mark the local mail cache stale synchronously; the explicit floor additionally refuses a view below this call's acknowledged writes even when the manifest is unchanged. Map each created object's document ID through this final view and choose keyed `newState` from its `cache.get_state(false)`; do not append a later store/viewer state or use the Thread counter as the Email item state. A view with no later writes may be reused only if it meets the same floor. Do not replay mutations, rerun preconditions or add a search pass. At most one live guard; drop it before any reacquisition/reload (FR8). Ordinary-account metadata/state behavior stays unchanged.
- `Email/import` (`import.rs:277-304,347`): after final flush/recheck and the selected hook, acquire its single final response view with `za_response_snapshot(.., last_change_id)`. Render created objects and keyed `newState` from that view, including a selected call with only `notCreated` when the hook published an earlier call's pending successful inserts. Preserve original precondition state, result references and ordinary-account behavior.
- `SearchSnippet/get` (`snippet.rs:183, 213`): `snapshot.check_id(*email_id)` in place of `email_by_jmap_id`; `not_found` on `None`.
- `Thread/get` (`thread/get.rs:35-45, 80-85`): the snapshot's cache; `state` = `State::from((snapshot.state() != 0).then_some(snapshot.state()))` (`State` implements `From<Option<ChangeId>>` only, `crates/jmap-proto/src/types/state.rs:33-40`; FS8) in place of the store read at `:80-85` (D13, FR3); the members by the map are Task 12's (until then upstream's `thread_map` over the cache stands, under the mapped state).
- `Blob/lookup` (`blob/get.rs:188-193`): `Id::from_parts(snapshot.thread_of(*document_id), *document_id)` and `Id::from(snapshot.thread_of(*document_id))`.
- `EmailSubmission/set` (`submission/set.rs:449-450`): for a key account `snapshot.check_id(value)` must be `Some` (else `notFound` for the create), and `submission.thread_id = za_submission_prefix(value.document_id())`; `EmailSubmission/get` (`get.rs:237-249`): `EmailId` = `snapshot.map_id(email_id)`, `ThreadId` = `snapshot.thread_of(email_id)` (G22: the current thread, not the stored prefix).
- IMAP `FETCH` `ObjectId` (`fetch/mod.rs:585-592`): one snapshot per FETCH command of a `za_key_session()` (`crates/imap/src/core/mod.rs:304-307`), taken before the window loop; `email_id: Id::from_parts(snapshot.thread_of(id), id)`, `thread_id: Id::from(snapshot.thread_of(id))`; `Err` → `NO [UNAVAILABLE]`. `SEARCH EMAILID` (`search.rs:565-574`): `snapshot.check_id(id)` decides the match; `THREADID` (`:576-586`): `snapshot.thread_members(thread_id)`; the snapshot is the command's content view when Task 10's `za_imap_content_guard` returned one, else one `za_snapshot` taken where the view would have been (FR8: one view or snapshot per request task at a time).
- The gates: `request.rs:296, 304, 318, 439, 455, 816` call `za_assert_mail_keys(self, access_token, req.account_id)`; `changes/get.rs:217-229`: `Email | Thread | EmailSubmission` → `za_assert_mail_keys`, `Mailbox` → `za_assert_mail_route`. The doc comment at `request.rs:853-864` lists the gated methods; extend it with 6.2's table.
- "No map, no ids": every site above answers `za_no_map_error(reason)` when `za_snapshot` fails, traced `ZA_SEARCH_UNAVAILABLE` with the account id and the reason class, and emits nothing.

- [ ] **Step 5: Write the failing `mail::threads` cases**

`tests/src/za/mail/threads.rs`: `pub async fn test(test: &mut TestServer)` creates `keymail50` and `keymail51` (`key_mail_account`, `mail/mod.rs:60-70`) and runs, in order:

```rust
/// R2, 5.6: the handler's map is validated after its state read. `SEARCH_SNAPSHOT_PAUSE`
/// holds `Email/get` between its pass and its snapshot; a publication lands in the
/// store without this node's swap (`SEARCH_SWAP_SKIP`); the resumed response's ids
/// reconcile with `Email/changes` since the state it returned.
async fn case_r2_cross_node(test: &TestServer, key: &Account) {
    // deliver A, run a keyed Email/get (pass, index A); deliver B with In-Reply-To: A
    // park Email/get at SEARCH_SNAPSHOT_PAUSE; with SEARCH_SWAP_SKIP set: run_search_pass(test, key) (store has the publication, the loaded index does not)
    // resume: the response's ids carry A and B under one thread id; its state S
    // Email/changes since S is empty or renders nothing the response contradicts (no destroyed id the response emitted)
}

/// F2: the cache is held below the competing publication (`SEARCH_CACHE_HOLD`); the response
/// fails as "no map", never a mapped id under the stale state; released, it serves.
async fn case_f2_stale_state(test: &TestServer, key: &Account) {
    // as above, with set_search_cache_hold(Some(key_id)) before run_search_pass and the resume
    // assert: methodResponses[0][0] == "error" with type "serverUnavailable"; no "Email/get" response
    // set_search_cache_hold(None); the same Email/get succeeds and its queryChanges adds no id the client holds
}

/// G20: a stale prefix is notFound on Email/get, served on a keyless Email/set update and destroy.
async fn case_g20_stale_prefix(test: &TestServer, key: &Account, bearer: &Keyless) {
    // old = the singleton id A|A fetched before B merged A's thread under a lower id; after the merge
    // Email/get [old] → notFound; keyless Email/set update {old: {"keywords/$seen": true}} → updated; destroy [old] → destroyed
}

/// F8: the method table over a Bearer token (the WebSocket is Task 17's).
async fn case_f8_keyless_table(test: &TestServer, key: &Account, bearer: &Keyless) {
    // the gated methods of 6.2's table refused (`Blob/get|upload|copy`, `SearchSnippet/get` and `EmailSubmission/set`, keyed already, stay refused); the keyless ones served; a StateChange still arrives
    // refused (forbidden, ZA_MAIL_KEYLESS): Email/get, Email/query, Email/changes, Email/queryChanges, Email/import,
    //   Email/parse, Email/copy, Email/set create, Thread/get, Thread/changes, Mailbox/get, EmailSubmission/get,
    //   EmailSubmission/query, EmailSubmission/changes, Blob/lookup
    // served: Mailbox/query, Mailbox/changes, Mailbox/set, Email/set update and destroy, Identity/get,
    //   VacationResponse/get, PushSubscription/get, Core/echo
}

/// H21: a FETCH after a keyed APPEND of a reply carries the final EMAILID and THREADID; Email/set
/// create's response carries the mapped id.
async fn case_h21_final_ids_after_keyed_writes(test: &TestServer, key: &Account) {
    // a keyed IMAP APPEND of a reply into an existing thread: APPENDUID, then UID FETCH (EMAILID THREADID) carries the thread's mapped ids, not the singleton
    // Email/set create of a reply: the response's id carries the thread's prefix and equals a following Email/get's id
    // EmailSubmission/set for that message, then EmailSubmission/get: emailId and threadId equal the current mapped ids (G22)
}

/// R2, R3: a failed manifest read fails every thread-valued method and emits no singleton.
async fn case_failed_manifest_read(test: &TestServer, key: &Account) {
    // set_search_manifest_fail_once(Some(key_id)); Email/get → serverUnavailable; Thread/get → serverUnavailable;
    // IMAP FETCH 1 (EMAILID) → NO [UNAVAILABLE]; then each succeeds and the ids match the keyed view
}
```

P1-STATE regression cases in the same Task 11 suite (proposed execution ordering is part of each fixture):
- `case_set_response_view_after_mutations`: in a quiescent keyed account, one Set creates C, updates U's flags and destroys D. Assert the original `oldState`, all outcomes, final `newState` equality with a following read, C's mapped IDs, changed U and absent D; a no-op Set with that `ifInState` succeeds when no intervening publication occurs. Cover keyed update-only/destroy-only and ordinary controls, plus unchanged supplied-ID acknowledgement echoes.
- `case_set_cross_node_publication_before_response`: pause after the input view, publish a bridge/merge through a separate node/index entry, then let this Set commit a later update/destroy. The merge counter must precede this call's last acknowledged mutation. Created IDs and final state must reflect the final map, or the existing no-map error must fail closed. A local pass blocked behind the same held guard is not this fixture. Inspect manifest/log directly; no later request may repair the missing publication. A narrow test-only pause at the input-view seam is allowed if existing hooks cannot express this ordering.
- `case_write_response_requires_own_change_id`: hold the input cache at the unchanged manifest publication, acknowledge a later flag/delete write, and attempt the final response. It must not succeed below that write's counter; releasing the hold permits refresh. Also fail the final manifest read after writes: no stale response, mutation replay or extra post-write pass. Exercise the optional floor inside the existing bounded acquisition loop.
- `case_import_response_state_includes_publication`: Import's returned IDs and `newState` include its selected hook's later publication; also cover only `notCreated` on the final candidate after an earlier call committed, same-call creation references, following-call result references and the request-wide pass count. Earlier method responses are not remapped later.

Helpers the module uses from Task 8 (`tests/src/utils/za.rs`): `run_search_pass(test, account_id, password)` and `read_manifest(test, account_id)`; it adds `Keyless` lifted out of `gates.rs:280-330` into `tests/src/utils/za.rs` with its Bearer constructor (`gates.rs:84-213`'s token flow) so `threads.rs` and Task 17 share it.

- [ ] **Step 6: Run the module to see it fail, implement, run the suite**

Run: the za suite.
Expected before: `case_r2_cross_node` fails (singleton ids, or ids under a state below the publication); after: `mail::threads` passes, `mail::search`, `mail::gates` and `websocket::test` still pass (the gates test's `keyless_tokens_hit_the_dispatcher_gate`, `gates.rs:333`, now expects `Mailbox/get`, `Email/query`, `Thread/get`, `EmailSubmission/get|query`, `Blob/lookup` and the three changes methods refused: adjust it with a comment citing 6.2's table and list the change in the task report).

- [ ] **Step 7: Checks and commit**

Run: `cargo test -p email --lib --features test_mode za::search::snapshot`; the per-task compile check; `cargo check -p tests --tests`; formatting for `email`, `jmap`, `imap`, `common`, `tests`.

```bash
git add crates/email/src/message/za/search/snapshot.rs crates/email/src/message/za/search/view.rs crates/email/src/message/za/search/mod.rs crates/email/src/cache/mod.rs crates/common/src/za/search/load.rs crates/common/src/za/test_hooks.rs crates/jmap/src/email/get.rs crates/jmap/src/email/query.rs crates/jmap/src/email/set.rs crates/jmap/src/email/import.rs crates/jmap/src/email/mod.rs crates/jmap/src/email/snippet.rs crates/jmap/src/thread/get.rs crates/jmap/src/blob/get.rs crates/jmap/src/submission/set.rs crates/jmap/src/submission/get.rs crates/jmap/src/api/request.rs crates/jmap/src/changes/get.rs crates/imap/src/op/fetch/mod.rs crates/imap/src/op/search.rs tests/src/za/mail/threads.rs tests/src/za/mail/mod.rs tests/src/za/mod.rs tests/src/za/mail/gates.rs tests/src/utils/za.rs
git commit -m "Release 7: mapped Email ids under a validated snapshot, the method table's gates, no map no ids"
```

### Task 12: Thread-valued reads

Implementer: Sol (`gpt-6.1-sol`, `medium`); reviewer: Astra (`gpt-6-astra`, `xhigh`).
Each site is one substitution of the cache's thread id by the map's, with the control
comparisons as the test.

**Files:**
- Create: `crates/email/src/message/za/search/threads.rs` (`za_mailbox_counters`, `za_thread_keywords`, `za_group_by_thread`)
- Modify: `crates/jmap/src/thread/get.rs:46-70` (the thread map), `crates/jmap/src/mailbox/get.rs:117-135` (the counters), `crates/jmap/src/email/query.rs:319-335` (`restrict_thread_keywords`), `:456-461` (`count_matches`, completing Task 11's threading of the snapshot), `crates/jmap/src/email/query/filter.rs:621-648` (the three `*InThreadHaveKeyword` arms), `:692-696` (`InThread`), `crates/jmap/src/changes/query.rs:129, 133` (`collapseThreads` re-emission by the map), `crates/jmap/src/submission/query.rs:155-164` (`ThreadIds` filter), `:247-252` (`ThreadId` sort), `crates/imap/src/op/thread.rs:91-104` (`THREAD` grouping)
- Modify: `crates/jmap/src/email/za_search.rs` (`za_resolve_condition`'s thread forms take the snapshot)
- Modify: `tests/src/za/mail/threads.rs` (the cases below)
- Test: `cargo test -p email --lib --features test_mode za::search::threads`, the za suite's `mail::threads`

**Interfaces:**
- Consumes (Task 11): `ZaSnapshot::{thread_of, thread_members, map_id, cache}`.
- Consumes (upstream): `MessageCacheAccess::mailbox_counters` (`crates/email/src/cache/email.rs:583-614`, `MailboxCounters::add(thread_id, is_unread, count_threads)` `:362`), `thread_keywords` (`:279-345`), `in_thread` (`:486-490`), `Thread/get`'s map (`thread/get.rs:46-70`), IMAP `THREAD`'s grouping (`thread.rs:91-104`).
- Produces, in `email::message::za::search::threads`:
  - `pub fn za_mailbox_counters(snapshot: &ZaSnapshot, mailbox_ids: impl IntoIterator<Item = u32>, count_threads: bool) -> AHashMap<u32, MailboxCounters>`: `mailbox_counters`'s body with `counter.add(snapshot.thread_of(message.document_id()), ..)`.
  - `pub fn za_thread_keywords(snapshot: &ZaSnapshot, keyword: &Keyword, match_all: bool, documents: Option<&RoaringBitmap>, visible: Option<&RoaringBitmap>) -> RoaringBitmap`: `thread_keywords`'s body with `snapshot.thread_of(item.document_id())` for every `item.thread_id()`.
  - `pub fn za_group_by_thread<'x>(snapshot: &ZaSnapshot, messages: impl Iterator<Item = MessageRef<'x>>) -> AHashMap<u32, Vec<u32>>` (thread id → present member document ids, ascending).

- [ ] **Step 1: Write the failing unit test**

```rust
#[test]
fn counts_and_keyword_threads_group_by_the_map() {
    // records: 1, 2 in thread 1; 3 in thread 3; the cache's thread_id() is the singleton for all three; 1 and 2 are unread, 3 is seen
    let snapshot = snapshot_with([(1, 1), (2, 1), (3, 3)], [1, 2, 3]);
    let counters = za_mailbox_counters(&snapshot, [INBOX_ID], true);
    assert_eq!((counters[&INBOX_ID].total, counters[&INBOX_ID].threads.len(), counters[&INBOX_ID].unread_threads.len()), (3, 2, 1));
    // some-in-thread $seen: 1 is seen → thread 1's members 1 and 2
    assert_eq!(za_thread_keywords(&snapshot, &Keyword::Seen, false, None, None).iter().collect::<Vec<_>>(), [1, 2]);
    assert_eq!(za_group_by_thread(&snapshot, snapshot.cache.emails.iter())[&1], vec![1, 2]);
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p email --lib --features test_mode za::search::threads`
Expected: compile errors naming `za_mailbox_counters`.

- [ ] **Step 3: Implement the helpers and the eight substitutions**

`Thread/get` (`thread/get.rs:46-70`): with a snapshot, supplied ids → `thread_map[id.document_id()] = snapshot.thread_members(id.document_id())` filtered by `access`; no ids → `za_group_by_thread(&snapshot, cache.emails.iter().filter(access))`; an unknown thread id is `notFound` (`:104-106`, upstream). `Mailbox/get` (`mailbox/get.rs:117-135`): `za_mailbox_counters` when a snapshot is present (the method is keyed from Task 11, so a key account always has one). `restrict_thread_keywords` (`query.rs:319-335`): for a key account the replacement runs whatever `filter.visible(cache)` answers (PL9: owner access answers `None`, `filter.rs:410-417`, and upstream's guard `:325-332` would then leave the cache comparator, which groups by the stored singleton), as `*comparator = MessageComparator::Search(SearchComparator::set(za_thread_keywords(&snapshot, keyword, match_all, None, filter.visible(cache)), ascending))`; the filter arms (`filter.rs:621-648`) call `za_thread_keywords` likewise; for an attachment keyword both use the content's attachment bit in place of the keyword bit (Task 10's forms, now grouped by the map). `InThread`: `QueryFilter::parse` gains `snapshot: Option<&ZaSnapshot>` (Task 11's, present on every keyed request), and the fork arm before `row.absorb` (Task 10's) takes `EmailFilter::InThread(id)` at the top level too when a snapshot is present, pushing `SearchFilter::is_in_set(snapshot.thread_members(id.document_id()))` into `residual` (PL9: otherwise `RowFilter::absorb` narrows `thread_id`, `filter.rs:126-128`, and `RowFilter::matches` compares the stored singleton, `:84-86`); the nested arm (`:692-696`) → the same set; `count_matches` (`query.rs:456-461`) → `snapshot.thread_of`. For Email/queryChanges, keep the staged borrowed query evaluator and raw
page/re-emission until Task 13. That task supplies the explicit historical
endpoint and end-presence projection to the query's mapped grouping, filter,
sort and re-emission seams. Task 12's standalone current-thread behavior
continues through the same evaluator. `EmailSubmission/query`: the `ThreadIds` filter keeps a submission when `ids.contains(&snapshot.thread_of(s.email_id))` (`:155-164`); the `ThreadId` sort keys by `snapshot.thread_of(s.email_id)` (`:247-252`). IMAP `THREAD` (`thread.rs:98-104`): `threads.entry(snapshot.thread_of(document_id))`.

- [ ] **Step 4: Write the failing `mail::threads` cases**

Accounts: `keymail52` with the plain control `plainmail52`; `keymail53` with `plainmail53`.

Beside the cases below, `case_pl9_query_fast_paths` on `keymail53`/`plainmail53`: root `A`, reply `B` (In-Reply-To `A`), unrelated `C` carrying a `text/plain` attachment, delivered to both; `B` flagged `$seen` on both. Each of these equals the control (ids compared by Message-ID): `Email/query` `{"inThread": <A's thread on that account>}` at the top level → `[A, B]`; `{"collapseThreads": true, "limit": 1, "calculateTotal": true}` → `total == 2`; sort `someInThreadHaveKeyword $seen` then `receivedAt` on the owner's own request → the same order; sort `hasKeyword $hasattachment` (ascending and descending) → the same order (PL9).

```rust
/// 6.1, 9.2: the same conversation to a key account and a plain control groups the same.
async fn case_conversation_parity(test: &TestServer, key: &Account, plain: &Account) {
    // deliver, to both: root R (Message-ID r); reply A (In-Reply-To r); reply B (References r a); forward F (new subject "Fwd: ..", References r); unrelated U;
    // a reply C with the subject changed and In-Reply-To a (upstream threads by subject thread-name: C joins when its thread name matches, else not: the control decides)
    // Thread/get (all) : the sets of subjects per thread are equal; Email/query collapseThreads=true sorted by receivedAt: the subject lists are equal;
    // IMAP THREAD REFERENCES UTF-8 ALL and THREAD ORDEREDSUBJECT UTF-8 ALL: the UID groupings are equal; Thread/get [one id]: emailIds equal by subject
}

/// 5.2: a reply delivered while no keyed request runs is a singleton until the next keyed request.
async fn case_singleton_until_keyed_request(test: &TestServer, key: &Account) {
    // deliver reply A2 to R2 with no keyed request; read_manifest(..).indexed_to.change_id < the Email log's last change id (the store counter)
    // a keyed Thread/get: R2 and A2 share a thread; read_manifest(..).indexed_to has moved
}

/// G7: Mailbox/get thread counts match the control.
async fn case_g7_mailbox_counts(test: &TestServer, key: &Account, plain: &Account) {
    // Mailbox/get [inbox] totalEmails, unreadEmails, totalThreads, unreadThreads equal on both, after marking one reply $seen on each
}

/// F1, 7.5: more than K references thread through the nearest ones; the K deviation is pinned.
async fn case_f1_k_bound(test: &TestServer, key: &Account, plain: &Account) {
    // 20 roots r1..r20 present on both; message M with References r1..r20 (r20 nearest): M joins on both (the `K = 16` hashes kept are its own id and its fifteen nearest references r6..r20, and those roots are present)
    // delete r5..r20 on both (keyed Email/set destroy); message N with References r1..r20: the control joins r1's thread, the key account threads N as a singleton:
    // assert_ne!(key_thread_of(N), key_thread_of(r1)); assert_eq!(plain_thread_of(N), plain_thread_of(r1))  // pinned deviation, 7.5
}
```

- [ ] **Step 5: Run, implement, run**

Run: the za suite.
Expected: `mail::threads` passes; `mail::search`'s `THREAD` line now groups as the plain fixture's (Task 10 compared UID sets; tighten the comparison to the grouping).

- [ ] **Step 6: Checks and commit**

Run: the unit test; the per-task compile check; `cargo check -p tests --tests`; `jmap::jmap_tests` and `imap::imap_tests` plain and with `ZA_KEY_ACCOUNTS=1` (the thread-valued reads are shared paths); formatting.

```bash
git add crates/email/src/message/za/search/threads.rs crates/email/src/message/za/search/mod.rs crates/jmap/src/thread/get.rs crates/jmap/src/mailbox/get.rs crates/jmap/src/email/query.rs crates/jmap/src/email/query/filter.rs crates/jmap/src/email/za_search.rs crates/jmap/src/changes/query.rs crates/jmap/src/submission/query.rs crates/imap/src/op/thread.rs tests/src/za/mail/threads.rs tests/src/za/mail/search.rs
git commit -m "Release 7: thread-valued reads group by the sealed thread map"
```

### Task 13: Changes: `Email/changes`, `Thread/changes`, `Email/queryChanges`

Implementer: Sol (`gpt-6.1-sol`, `high`); reviewer: Astra (`gpt-6-astra`, `xhigh`).
A kernel: the rendering of thread history as upstream's destroyed and created pairs over
a pinned range, with existence decided from raw records first.

**Files:**
- Create: `crates/email/src/message/za/search/changes.rs` (the stream, pure over records and a present set; the epoch rule; the retained bound)
- Create: `crates/jmap/src/changes/za.rs` (the key account's page inside `ChangesLookup::changes`; `pub mod za;` in `crates/jmap/src/changes/mod.rs`)
- Modify: crates/jmap/src/changes/get.rs (keyed page entry after authorization
  and maxChanges validation) and crates/jmap/src/changes/query.rs (keyed
  composition with one borrowed view, preserving the ordinary call path).
- Modify: crates/jmap/src/email/{query.rs,query/filter.rs,za_search.rs} (borrowed
  evaluator's endpoint projection and minimum-aware Map/Content selection).
  Extend the Task 12 thread-query helpers only as needed to accept that
  projection.
- Modify: `tests/src/za/mail/threads.rs` (the cases below)
- Test: `cargo test -p email --lib --features test_mode za::search::changes`, the za suite's `mail::threads`

**Interfaces:**
- Consumes: Store::za_changes_records_range(account_id, collection,
  from_exclusive, to_inclusive) -> trc::Result<ZaChangeRecords> and
  Store::za_log_first_retained(account_id, collection)
  (crates/store/src/query/log.rs:210–310). Range applies both endpoints in
  the store iterator. Its truncated flag records an encountered marker.
  The first-retained read supplies the actual marker independently of the
  bounded range. The unbounded record reader still serves existing callers.
- Consumes (Task 3): `ThreadPart::{revision_at, thread_of(document_id, change_id), members_of(thread_id, change_id), exists_at(document_id, change_id) -> Option<bool>, records() -> impl Iterator<Item = &ThreadRecord>}` (`records()` iterates present and tombstoned records; plan call D10).
- Consumes (Tasks 10–12): the borrowed Email/query evaluator, existing
  Map/Content classification and error adapters, za_view_with_minimum,
  ZaSnapshot { index, cache, epoch, .. }, ZaSnapshot::state and
  za_no_map_error. Use ThreadPart::thread_of(d,E) for historical lookup.
- Consumes (upstream): `Change::{InsertItem, UpdateItem, UpdateItemMetadata, DeleteItem, InsertContainer, UpdateContainer, UpdateContainerPartial, DeleteContainer}` (`crates/store/src/query/log.rs:25-34`; an item id is `thread << 32 | document`, so the document id is `id as u32`), `State::{Initial, Exact, Intermediate(JMAPIntermediateState { from_id, to_id, items_sent })}` and `State::new_intermediate`, `new_exact` (`crates/jmap-proto/src/types/state.rs:18-30, 83-93`), `ChangesResponse<NullObject>` and `IntermediateChangesResponse` (`changes/get.rs:69-73`), `max_changes` (`:230-238`), `JmapCacheState::get_state(false)` = `cache.emails.change_id` (`changes/state.rs:132-139`), the kind filter (`page.rs:173`: item changes for Email, container changes for Thread), `fill_page`'s classification (`page.rs:365-410`), `Email/queryChanges`'s diff (`changes/query.rs:348-378`).
- Produces, in `email::message::za::search::changes`:
  - `pub enum StreamKind { Created, Updated, Destroyed }`; `pub struct StreamItem { pub document_id: u32, pub kind: StreamKind, pub old: Option<Id>, pub new: Option<Id>, pub order: (u64, u32) }` (`Destroyed` carries `old`, `Created` carries `new`, a reassignment is two items, `Destroyed` then `Created`, with the same `order`).
  - `pub struct RangeView<'x> { pub records: &'x ZaChangeRecords, pub later: &'x ZaChangeRecords, pub present: &'x dyn Fn(u32) -> bool, pub end_is_current: bool }` (`records` = the unfolded records of `(S, E]`; `later` = the records after `E`, empty when `E` is current).
  - `pub fn za_email_stream(thread: &ThreadPart, view: &RangeView, since: u64, end: u64) -> Vec<StreamItem>` (the decision table below).
  - `pub struct ThreadStreamItem { pub thread_id: u32, pub kind: StreamKind, pub order: (u64, u32) }`; `pub fn za_thread_stream(thread: &ThreadPart, view: &RangeView, since: u64, end: u64) -> Vec<ThreadStreamItem>`.
  - `pub fn za_present_at_end(present_now: impl Iterator<Item = u32>, later: &ZaChangeRecords) -> RoaringBitmap`: `(present_now ∪ later_deletes) \ later_inserts`, item lifecycle entries only (P1-PAGING).
  - `pub fn za_initial_stream(thread: &ThreadPart, present_at_end: impl Iterator<Item = u32>, end: u64) -> Vec<StreamItem>` (the `S = 0` form: every document in the corrected set as `Created(thread_of(d, end)|d)`, document-id order; the historical end is explicit).
  - `pub fn za_epoch_refuses(records_of_kind: &ZaChangeRecords, is_container: bool, since: u64, epoch: u64) -> bool`: `since > 0 && since < epoch && records_of_kind.records.iter().any(|(id, changes)| *id <= epoch && changes.iter().any(|c| is_kind(c, is_container)))` (the closed interval `(S, epoch]`, R27, H1).
  - `Server::za_retained_bound(&self, account_id) -> trc::Result<u64>`: `max(first_retained(Email), first_retained(Thread))`, `0` when neither log holds a marker (G4: the shorter log's bound).
- Produces, in jmap::changes::za: a parsed since-state with its caller
  minimum, and an internal za_changes_page that borrows the admitted
  view. Its result carries IntermediateChangesResponse plus the effective
  since, end and corrected end-presence context. Current presence can
  refer to the admitted cache; a historical end can hold the reconstructed
  bitmap. The public wrapper returns only IntermediateChangesResponse.

**The stream of `(S, E]`, as a decision table** (6.3). Inputs: `records` (unfolded, `(S, E]`), `later` (unfolded, after `E`; empty for a current `E`), `present(d)` (the snapshot's cache), the thread part. `R(X) = revision_at(X)`.

1. **Candidates** (a set of document ids): every `d` with any raw record in `records` (item changes only; the mailbox container changes do not count); every record `r` of `thread.records()` with `thread.thread_of(r.document_id, S) != thread.thread_of(r.document_id, E)`; every `r` with `r.deleted` in `(S, E]`. A linear scan of the records is the implementation; a revision-indexed shortcut is a later optimization, not this task's.
2. **Existence at `S`** of `d`: if `records` holds an `InsertItem` of `d` → absent. Else if `records` holds a `DeleteItem` of `d` → present (it existed to be deleted). Else if `!present(d)` and neither `records` nor `later` holds a `DeleteItem` of `d` → absent (I3: deleted at or before `S` by a deletion no pass has consumed, whatever the record says). Else if the index holds `d` (`exists_at(d, S)` is `Some(b)`) → `b` (created at or below `S` and not deleted at or below `S`). Else (no record, no raw entry): `present(d)` or `later` holds a `DeleteItem` of `d` (it exists now or was deleted after `E`, so it existed at `S` too).
3. **Existence at `E`** of `d`: if `records` holds a `DeleteItem` of `d` → absent (whatever the record says: the record lags the log by the unconsumed backlog). Else if `end_is_current` → `present(d)`. Else → membership in `(present_now ∪ later_deletes) \ later_inserts` (H2, P1-PAGING). The sealed record is never consulted for existence at `E`, and a `d` the cache lacks with no `DeleteItem` anywhere in `records` or `later` is absent at both ends (I3: an unconsumed deletion at or before `S` must not emit a second `destroyed`).
4. **Threads**: `T0 = thread.thread_of(d, S)`, `T1 = thread.thread_of(d, E)` (the singleton `d` when the index holds no record, or `r0 > R(X)`).
5. **Outcome**: absent, absent → nothing (R14); absent, present → `Created(T1|d)`; present, absent → `Destroyed(T0|d)`; present, present and `T0 != T1` → `Destroyed(T0|d)` then `Created(T1|d)`; present, present, `T0 == T1` and `records` holds an `UpdateItem` or `UpdateItemMetadata` of `d` → `Updated(T1|d)`; otherwise nothing.
6. **Order**: by the earliest change id that made `d` a candidate (its first raw entry in `records`, the change id of the earliest revision in `(S, E]` on its chain or assignment, its deletion change id), then by document id.
7. **`S = 0`**: first compute `present_at_end = za_present_at_end(present_now, later)`; `za_initial_stream(thread, present_at_end.iter(), E)` renders exactly that set as created in document-id order. Initial Thread membership uses the same corrected set. A document inserted and deleted after E is excluded, never restored into the page (R31, P1-PAGING).

**`Thread/changes`** (F14, R25): over the same candidates, `members_of(T, S)` and `members_of(T, E)` for every `T` in `{T0, T1}` of every candidate (members intersected with existence at that end by rules 2 and 3; a candidate `d` the index holds no record for is its own thread at both ends, its only member `d` where it exists): no member at `S`, some at `E` → `Created(T)`; some at `S`, none at `E` → `Destroyed(T)`; some at both and the two member sets differ → `Updated(T)`; some at both and the sets equal → nothing (FS8: a flag-only `UpdateItem` makes `d` a candidate but changes no membership, and upstream's Thread log carries no flag changes, so its `Thread/changes` reports nothing for one; the Email stream keeps its `Updated(T|d)`); ordered as the Email stream, by the earliest change id, then thread id. For `S = 0`: every thread with a member at `E` (a present document without a record is its own thread), `Created`, ordered by its lowest member's document id.

**The page** (Email item or Thread container).

1. Parse the state before acquiring a view. Initial and Exact(0) set S=0
   without a minimum. Exact(s>0) sets S and the minimum to s. Intermediate
   sets S=from, pins E=to, sets skip=items_sent, and sets the minimum to
   to. Keep the actual request state for oldState.
2. Standalone changes acquires Map once with that minimum; keyed
   queryChanges acquires its selected Map or Content view once. A caller
   minimum uses the selected mode's existing validation-round budget and
   error routing from Task 11. Derive current E from cache.emails.change_id for Email and
   cache.last_change_id for Thread, after acquisition. Write C for this
   cache's last_change_id. A pinned E comes from the token and must satisfy
   E<=C before any reconstruction or fallback. It stays pinned while
   rendering that range. Keep ordinary invalid-state behavior unchanged.
3. For S>0, refuse with cannotCalculateChanges ("Change log is truncated")
   if S is below the shorter-log bound. Check the method's kind in
   (S,epoch] with its bounded log read, and refuse with "State predates the
   index's epoch" if it has a relevant record. An initial Intermediate
   requires E at or above both the shorter-log bound and the validated
   epoch; use the corresponding retention or epoch refusal. Fresh Initial
   and Exact(0) are exempt from these old-history checks. Content-only
   rebuild and reconciliation keep the existing identity epoch.
4. If S==E for Exact, mark the stream empty and prepare its effective
   endpoint and presence context. Before returning Exact(E), complete
   the required method-kind epoch read and the shared final retained-bound
   and truncation checks in step 5. Skip the item and suffix reads for this
   empty current range. Fresh Exact(0) retains its exemption. Only then
   return the empty response or evaluate calculateTotal from that context.
5. Physically read the Email records in (S,E] using
   za_changes_records_range. For a pinned E, read its Email lifecycle
   suffix (E,C] with the same API. After all required reads, including the
   method-kind epoch read, recheck the shorter retained bound. For S>0
   refuse if S<B_after or any required read reports truncation. For an
   initial Intermediate refuse if E<B_after or the suffix is truncated.
   Do this before rendering, skipping, or falling through. A marker
   beyond C may be invisible to both range reads. Fresh Initial and
   Exact(0) still ignore truncation of their unused early history. Equal
   endpoints at the bound and at the identity epoch remain admissible. Thread's log supplies its
   marker and epoch check; Email's log supplies member candidates.
6. Derive present-at-E from the admitted cache for a current end or by
   restoring suffix deletes and subtracting suffix inserts for a pinned
   end. Reuse that exact set for the stream and the page context. Render
   the appropriate stream, skip the sent count, and take maxChanges;
   expose the mapped IDs in their created, updated or destroyed lists.
7. Return Exact(E) when the returned page exhausts the stream or
   Intermediate(S,E,skip+maxChanges) with hasMoreChanges when it does
   not. If an Intermediate already exhausts the stream at its input
   skip, after its admission and final bound checks continue once as
   Exact(to) with current E from the same view, as upstream continues
   with Since(to). Reapply the range and retention checks to that
   effective range. Return its actual endpoints and presence context,
   preserving the input oldState and using no second acquisition.
8. Keep updated_properties None for the keyed page.

**The keyed queryChanges composition.** Preserve account authorization
and maxChanges checks, and select Map or Content with Task 10's
classification before acquiring one view using the token minimum. Route
Map exhaustion through `za_no_map_error` and Content exhaustion or
deadline expiry through the existing named-property unavailable adapter.
Content availability remains an early check even when the page is
unchanged; ordinary queries keep their existing availability behavior.
The borrowed page and evaluator run only after ensure, pass, reload and
content loading have finished. The page's effective end and presence
context control the identity projection through both query and
re-emission. The query evaluates the existing masks, filters and sort
keys on current readable rows intersected with present-at-E. Apply the E
projection before any thread filter or comparator, count, collapse or
representative selection, and emit those projected IDs. Use the full
projected ID for upToId in the existing cutoff branch. Thread-dependent
query operations group these eligible current rows at E. Nonidentity
filter values and sort keys use the current rows and the same view's
content when required. A document deleted after E can supply its
historical ID and membership for re-emission. It has no query row.
Take touched prefixes from the page's mapped IDs and re-emit other
present-at-E members with their thread at E. Exclude the page's existing
created/updated IDs. Keep the upstream diff over the projected results
and the page's state, and leave the standalone current Email/query and
ordinary queryChanges paths as before.

- [ ] **Step 1: Write the failing unit tests in `changes.rs`'s `mod tests`** (a fixture builder: `history()` with records, aliases and revision rows; `records_of(&[(change_id, Change)])`; `present(&[u32])`)

```rust
// R2, R5: two publications; states before, at, between and after.
#[test] fn a_first_assignment_from_the_singleton_renders_destroyed_then_created() {
    // D=7 inserted at 10 (raw), assigned (T0=3, r0=1) by the publication at 12; S=11 (saw 7|7), E=12 current, present {3, 7}
    let items = za_email_stream(&h, &view(records(&[(12, UpdateItem(witness))]), &none(), present(&[3, 7]), true), 11, 12);
    assert_eq!(kinds(&items), [(Destroyed, Some(id(7, 7))), (Created, Some(id(3, 7)))]);
    // S=12: nothing; S=9 (before the insert): Created 3|7 only
}
#[test] fn two_merges_render_from_each_state() { /* record 7 assigned (5, r0 at change 5); aliases 5→3 at r1 (change 12), 3→1 at r2 (change 20). (S, E) = (11, 12): Destroyed 5|7, Created 3|7; (12, 15): nothing; (15, 20): Destroyed 3|7, Created 1|7; (20, 21): nothing; (11, 21): Destroyed 5|7, Created 1|7 */ }
// R3: a deleted message's old mapped id, from a retained pre-delete state
#[test] fn a_deleted_members_old_mapped_id_is_destroyed() { /* record 7 (T0=3), deleted at 30; S=25, E=30: Destroyed 3|7 */ }
// R14: created and deleted within the range yields nothing, singleton and threaded
#[test] fn created_and_deleted_within_the_range_yields_nothing() { /* Insert 9 at 13, Delete 9 at 14, record with assignment; S=12, E=15: [] */ }
// F10: insert reached by a pass, deletion behind the cap: the unfolded range shows the delete
#[test] fn a_deletion_the_record_lacks_but_the_range_shows_is_destroyed_once() { /* record 9 present (no deleted), Delete 9 at 14 in range; S=13: Destroyed T|9 once; S=14: nothing */ }
// H2: a deletion committed between the pass's log read and its commit; pinned E
#[test] fn a_lagging_record_never_decides_presence_at_a_pinned_end() { /* record 9 present, Delete 9 at 16 in `later`, E=15 pinned, Insert 9 at 13 in range: Created at E (exists at E: later shows the delete); S=15,E=16: Destroyed */ }
// I3: a deletion logged at or before S and not yet consumed
#[test] fn an_unconsumed_deletion_before_s_emits_nothing() { /* record 9 present (lagging), cache lacks 9, no DeleteItem in (S, now]: [] */ }
// R12: paging reproduces every item exactly once with a publication and a deletion between pages
#[test] fn paging_with_a_small_max_changes_covers_the_stream_once() { /* a five-item (S,E] stream paged by 1 through Intermediate{from: S, to: E, sent} gives five single-item pages equal to the unpaged stream in order, has_more_changes false only on the fifth, and the sixth call continues as Exact(E) with nothing */ }
// R27, H1: the epoch rule
#[test] fn the_epoch_refuses_only_when_the_methods_log_holds_a_record_in_s_epoch() {
    assert!(!za_epoch_refuses(&records(&[]), false, 5, 9));
    assert!(za_epoch_refuses(&records(&[(9, UpdateItem(1))]), false, 5, 9)); // the boundary's own witness at the epoch
    assert!(!za_epoch_refuses(&records(&[(9, UpdateContainerPartial(1, counts()))]), false, 5, 9)); // a mailbox entry does not count for Email
    assert!(!za_epoch_refuses(&records(&[(9, UpdateItem(1))]), false, 0, 9)); // never from 0
    assert!(!za_epoch_refuses(&records(&[(10, UpdateItem(1))]), false, 5, 9)); // past the epoch
}
// R28, R31: Initial paged equals unpaged while E and the required suffix remain retained
#[test] fn the_initial_stream_is_every_present_document_in_id_order() { /* present {2, 5, 9} with 5 updated in a retained record: Created 2, Created T|5, Created 9; the same after `records` is emptied */ }
// R25, F14: thread membership at a pinned end
#[test] fn thread_members_at_a_pinned_end_count_a_member_deleted_after_it() { /* T=3 with member 3 (created before 19); 7 inserted at 20 into T=3 (InsertItem(7) in range, its record created at 20, the publication's row at 20); 7 deleted at 25 (in `later`); za_thread_stream S=19 E=20: [Updated(3)] (members_of(3, 19) == [3], members_of(3, 20) == [3, 7]: the sets differ), and members_of(3, 20) == [3, 7] though 7 is deleted now. FT1: a flag-only UpdateItem of 7 in the same range, with 7 created before 19, gives [] (equal sets, FS8) */ }
// P1-PAGING: undo lifecycle changes after E, then map at E, not now.
#[test] fn initial_pinned_end_excludes_later_inserts_and_restores_later_deletes() {
    /* E=100 present {2,5}; later insert9 at110, delete2 at120, insert11 at130 then delete11 at140;
       present_now {5,9} -> present_at_end {2,5}; assignment/merge after E must not alter either prefix at E. */
}
// H10 is Task 3's; the order key
#[test] fn items_are_ordered_by_the_change_id_that_admitted_them_then_by_id() { /* D=9 updated at 13, D=4 and D=2 reassigned by the publication at 15: order [(13, 9), (15, 2), (15, 4)] */ }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p email --lib --features test_mode za::search::changes`
Expected: compile errors naming `za_email_stream`.

- [ ] **Step 3: Implement `changes.rs`, `za.rs` and the branch in `ChangesLookup::changes`**

Standalone Email/changes and Thread/changes branch after the existing key,
route and maxChanges checks. Parse, acquire once, and return the public
part of the borrowed page result. Keyed Email/queryChanges calls the
same page helper after its own one acquisition, then borrows that view
for its query evaluator. Keep the ordinary self.changes and email_query
calls and all other /changes methods on their existing path.

- [ ] **Step 4: Write the failing `mail::threads` cases**

Accounts: `keymail54` with the control `plainmail54` (rendering parity), `keymail55` (backlog, pages, ordering), `keymail56` (deletion-only, pinned pages, G4, G3), `keymail57` with `plainmail55` (`Initial` and truncation). Helpers added to `tests/src/utils/za.rs`: `truncate_changes(test, account_id, keep: usize)` (calls `test.server.delete_changes(account_id, Some(keep), None)`, `transaction.rs:116-139`: both logs cut to `keep` records), `log_records_since(test, account_id, collection, 0)` (Task 8's helper: the unfolded records, for the R4 and R13 assertions), `pop3_delete(account, uid)` (a POP3 session that runs `DELE` and `QUIT`, which runs no pass), `jmap_changes(account, method, since: &str, max: Option<usize>) -> Value` and `collect_pages(account, method, since) -> (created, updated, destroyed, final_state)`.

```rust
/// 6.3: a merge renders as the control's in Email/changes, Thread/changes and queryChanges with collapseThreads.
async fn case_merge_rendering_parity(test: &TestServer, key: &Account, plain: &Account) {
    // on both: A (new thread), B (new thread), state S; bridge C with References a b; a keyed Email/get on the key account
    // Email/changes since S: both show {created: [C'], destroyed: [the losing member's old id], created: [its new id]} up to id mapping: compare by subject sets per kind
    // Thread/changes since S: both show the losing thread destroyed, the surviving thread updated (or created/updated per the control)
    // Email/queryChanges since S, collapseThreads, sort receivedAt: `added` and `removed` subject sets equal
    // FS8: record state S2 on both; flag an unrelated message U $seen on both (a flag-only record, no membership change);
    // Thread/changes since S2 is empty on both (upstream's Thread log carries no flag changes; the key account's stream renders
    // Updated(T) only when a thread's member sets differ), while Email/changes since S2 lists U as updated on both
}
/// R5: two successive merges from states before, at, between and after the two publications.
async fn case_r5_two_merges(..) { .. }
/// R2: the backlog: a capped pass, a fetch of an unindexed message under a state, the catch-up, Email/changes applied, the id set equals Email/query's.
async fn case_r2_backlog(test: &TestServer, key: &Account) {
    // search_config_guard!(pass_cap: 2) (plan call D3); deliver 6 replies to one root; one keyed Email/get (indexes 2): the response carries singletons for the 4 unindexed; keep S
    // repeat keyed requests until read_manifest(..).indexed_to reaches the log; collect_pages(Email/changes since S); apply to the held set; equals Email/query's ids
}
/// R12: pages with a tiny maxChanges, a merge and a deletion between pages, reconstruct the query's id set.
async fn case_r12_pages(..) { .. }
/// R14: created and deleted within one range renders as the control's folded log; with pages and a publication between; F10 with the deletion behind the cap.
async fn case_r14_f10_created_and_deleted(..) { .. }
/// R14 second form: the backlog deletion; T|D destroyed exactly once across paged changes and the finished catch-up.
async fn case_r14_backlog_deletion(..) { .. }
/// R16: A, D (reply to A), N (In-Reply-To D only), D deleted through POP3 before the next keyed request; A and N grouped as the control groups them with D absent; repeated with the cap between N's insert and D's deletion.
async fn case_r16_ordering(..) { .. }
/// R13: a deletion-only pass writes no log entry and renders the deletion from the retained record, the account's last message included.
async fn case_r13_deletion_only(..) { /* read_log before and after: no new entry; Email/changes since the pre-delete state: destroyed T|D */ }
/// R28, R31: Initial/Exact(0) pages equal unpaged after reload and truncation below or at E; explicitly include retained_bound == E. An admission boundary test also accepts E == epoch. Truncating zero alone must not refuse this continuation.
async fn case_r28_r31_initial(test: &TestServer, key: &Account, plain: &Account) { .. }
/// P1-PAGING: at E, {2,5,9}; first page sends 2. Later insert and delete operations with their full suffix retained leave exactly the original remaining pages and mapped prefixes, for Email and Thread. Once retention passes E and erases deletion of 2, the continuation refuses instead of skipping survivor 5; fresh Initial still succeeds.
async fn case_initial_pinned_end_lifecycle(..) { .. }
/// P1-PAGING: exercise from=0 and S>0 Intermediate pages. Park after the first bound check, advance retention beyond C=snapshot.cache.last_change_id, and assert B_before <= endpoint <= E <= C < B_after. Both bounded reads must miss the new marker; the post-read guard refuses before rendering, skipping or fallback. For S>0 retain sealed-transition candidates plus a raw-only candidate erased by the purge, so a mutant returns a wrong nonempty page. Use a narrow test-only page-read pause, not copied page logic.
async fn case_initial_continuation_retention_race(..) { .. }
/// R25: a pinned Thread/changes page across the deletion of a thread's last member and a merge after the page's end keeps its items and classification.
async fn case_r25_pinned_thread_page(..) { .. }
/// G4: Thread/changes from a state the Thread log retains but the Email log does not is refused (flags churned, then truncate_changes).
async fn case_g4_two_bounds(..) { .. }
/// Codex re-review PL3, D13: a Thread/get whose view was acquired before a merge committed returns the pre-merge grouping with a state
/// the merge is above, so the client's next Thread/changes from that state renders the merge; same-node and remote forms.
async fn case_d13_thread_state_pairs_with_its_map(..) {
    // threads A and B (two roots, same subject), a keyed Email/get (indexed); park a keyed Thread/get (no ids) at SEARCH_VIEW_PAUSE,
    // its view acquired and its read guard held, before it renders (Codex rev-6 PL3: parked inside the loop it would reload and answer
    // the new grouping); same-node form: on a second keyed connection `Email/import` bridge C (References a b; FU8: an import runs Task 9's post-commit pass, a delivery
    // does not) and park that pass at SEARCH_SWAP_PAUSE (committed, not swapped); release the Thread/get: it answers the pre-merge grouping (A and B apart)
    // with state s0; release the pass; Thread/changes since s0 lists the losing thread destroyed and the survivor updated, and
    // Thread/get then equals a fresh read; remote form: the same with set_search_swap_skip(Some(id)) in place of the swap pause;
    // in both, the parked Thread/get's state is below the merge's change id (never "the new state with the old grouping")
}
/// G3: an empty account provisioned on a warm cache with a trailing container-only record serves every Email/* method.
async fn case_g3_empty_account(..) { /* a fresh key account: Mailbox/set create a mailbox (a container record), then Email/get, Email/query, Email/changes Initial, Thread/get, Mailbox/get all 200 */ }
/// G7, R6, F15: Mailbox/changes after a publishing pass reports every mailbox with updatedProperties of the counts only; IMAP CHANGEDSINCE does not report the reassigned member; the witness's unsolicited FETCH (FLAGS ..) has an unchanged MODSEQ (all pinned).
async fn case_g7_r6_f15_pinned(..) { .. }
```

**Required composition and minimum regressions.** Use the actual JMAP
methods, real persisted changes, and the retained paging inputs; keep
the preceding parity, deletion, and retention tests.

- **Fresh view and pinned history.** Start with independent same-subject
  roots A and D, with descending receivedAt putting D first. Insert C
  referencing A in the same thread T, with receivedAt between A and D.
  Call that published end E. Add bridge B referencing A and D after E,
  with receivedAt below all three. Its publication F merges D's thread
  into T. Require real raw records and published prefixes. For the
  fresh form, park a keyed queryChanges after the one view is validated,
  publish F on another entry or park it after durable commit and before
  swap, then resume. Check the diff's exact IDs, representative and
  state: C represents T at E, and D retains its old identity. For the
  pinned form, put an unrelated raw update before C and take a real
  maxChanges=1 first page so the continuation's next item is C. Publish
  F before making that call, establish E<C_cache, and assert the same E
  identity result. Apply each diff and call again from E to prove the
  F identity of D is added once and was absent from the previous page.
  The pinned fixture must fail if collapse selects the representative
  with current grouping before any prefix rewrite.
- **Token minimum and exhausted fallback.** Warm a separate node's cache
  before a real nonpublishing delete, with newest publication and epoch
  below that warm cache counter. Issue an actual initial Intermediate
  token with maxChanges=1 on the refreshed node after the delete and
  before the other node refreshes. Require C_cache<to and unchanged
  publication/epoch, with retained suffix and bounds. Continue on the
  stale node. Require refresh to at least to or bounded no-map refusal
  with no data. Holding the old cache through the retries isolates the
  refusal; after release the same actual token must return the survivor
  and preserve to. Include a refreshed cache with C_cache==to. For the
  exhausted branch use that issued from/to
  with a controlled sent count equal to the verified stream length.
  Assert the floor is applied before fallback. With a later durable
  insert, verify the refreshed fallback returns the new effective end
  and current presence from the same view, including that insert.
  A naturally emitted token has remaining items; the exhausted control
  changes only the count. Also exercise the positive Exact since-floor
  and the fresh Initial/Exact(0) absence of a floor.
- **Current-row compatibility.** With an actual pinned E, insert a new
  high-ranked candidate after E and delete a historical member of a
  touched thread after E. Verify the insert is excluded before ranking
  and collapse. The deleted member's old E ID may be re-emitted for
  removal, and its absent current row contributes neither an addition
  nor a total. A still-present row changed after E uses its current
  nonidentity filter/sort values while retaining its E thread identity.
  Exercise a supported thread-keyword filter or sort and the existing
  immutable upToId cutoff with IDs whose prefix changes at F. Also hold
  the page unchanged in a Content-mode case and assert its unavailable
  refusal still occurs before the early return.
- **Isolated mutations.** Independently bypass the caller-minimum check;
  reacquire the query view after page selection; and use current
  grouping before historical prefix emission. The corresponding tests
  above distinguish each failure while keeping retention, cap and
  deadline checks active. Preserve the separate P1-PAGING mutations.

- [ ] **Step 5: Run, implement, run**

Run: the za suite.
Expected: `mail::threads` passes; `mail::search`'s `query_changes_collapse_threads` (`search.rs:764`) passes with the page now rendered by the fork.

- [ ] **Step 6: Checks and commit**

Run: `cargo test -p email --lib --features test_mode za::search::changes`; the per-task compile check; `cargo check -p tests --tests`; `jmap::jmap_tests` plain and with `ZA_KEY_ACCOUNTS=1` (`changes/get.rs` and `changes/query.rs` are shared paths); the za suite with `BLOB_STORE=FileSystem` (the end of phase E); formatting.

```bash
git add crates/email/src/message/za/search/changes.rs crates/email/src/message/za/search/mod.rs crates/jmap/src/changes/za.rs crates/jmap/src/changes/mod.rs crates/jmap/src/changes/get.rs crates/jmap/src/changes/query.rs crates/jmap/src/email/query.rs crates/jmap/src/email/query/filter.rs crates/jmap/src/email/za_search.rs tests/src/za/mail/threads.rs tests/src/utils/za.rs
git commit -m "Release 7: Email/changes and Thread/changes rendered from the sealed thread history"
```

### Task 14: Rebuild, content-only rebuild and reconciliation

**Files:**
- Create: `crates/email/src/message/za/search/rebuild.rs`
- Modify: `crates/email/src/message/za/search/mod.rs` (`pub mod rebuild;`), `crates/email/src/message/za/search/pass.rs` (the cursor sources of step 1, the truncation probe of a cursor pass, the content cursor's and the marker's steps, and the marker's start position replace Task 8's inline cursor filter; `write.rs`'s three arms are Task 8's and need no change, FS2), `crates/common/src/za/search/load.rs` (a `Destroying` marker read by a loader is `NoMap(Destroying)`; a lost thread blob or a bad tag is `Loss`, unchanged), `crates/store/src/write/blob.rs` (the link scan, after `user_blob_hashes`, `:291-355`), `tests/src/utils/za.rs` (`simulate_restart`, `lower_changes_history`, `purge_account`), `tests/src/za/mail/index.rs`, `tests/src/za/mail/threads.rs`
- Test: `crates/email/src/message/za/search/rebuild.rs` (unit, `#[cfg(test)]`), `tests/src/za/mail/index.rs`, `tests/src/za/mail/threads.rs`

**Interfaces:**
- Consumes: `Manifest`, `ManifestValue`, `ManifestWrite::new(value, key, ManifestFill)`, `manifest_hash`, `RevisionRow`, `RebuildCursor`, `ContentTo`, `ContentCursor`, `ReconciliationMarker` (Task 5); `Checkpoint` (Task 3); `Server::za_search_read_manifest`, `Server::za_search_entry`, `LoadFailure`, `LossClass`, `TransientClass`, `NoMapReason` (Task 6), `ZaSearchConfig` (Task 2); `Store::za_changes_records`, `ZaChangeRecords { records, truncated, resumed_record_lost, first_retained }`, `Store::za_log_first_retained` (Task 7), `Server::za_retained_bound` (Task 13; the probe of a cursor pass and rule 4's second-truncation test, FT6); the pass's staging and batch helpers of Task 8 (`Staging`, `PassSources`, `admit`, `admit_content`, `write_pass(..)`), `ZA_SEARCH_REBUILD` (Task 2); `Server::za_vault_record(account_id) -> trc::Result<Option<VaultRead { record, cas: u64 }>>` (`crates/common/src/auth/vault.rs:488`, `cas` = `xxh3_64` of the stored bytes, `:56-57`); `get_cached_messages(account_id) -> Arc<MessageStoreCache>` (`crates/email/src/cache/mod.rs:59-68`; `cache.last_change_id`, `cache.emails.document_ids()`, `cache.emails.contains(id)` at `crates/common/src/cache/email.rs:1484-1493`, `cache.mailboxes.items[..].document_id`); `BatchBuilder::{assert_value, set, clear, log_item_update, log_container_property_change}` (`crates/store/src/write/batch.rs:186, 292, 340, 396, 463`), `PendingId::Assigned(u32)` (`write/mod.rs:104-107`), `AssertValue::{Hash(u64), None}` (`write/assert.rs:11-18`, `Hash` is `xxh3_64` of the bytes, `:63`); `Server::commit_batch`; `Server::get_blob(key, range) -> Option<Vec<u8>>` (`crates/common/src/storage/blob.rs:416`); `BlobHeader::decode` (Task 5).
- Produces: `Server::za_search_rebuild_start(&self, account_id: u32, keys: &SessionKeys, loss: LossClass) -> trc::Result<RebuildStarted>` with `pub enum RebuildStarted { Written, LostToAnotherNode, Destroying }`; `Server::za_search_content_rebuild_start(&self, account_id: u32, keys: &SessionKeys, reason: ContentRebuildReason) -> trc::Result<()>` with `pub enum ContentRebuildReason { ContentLoss(LossClass), CapRaised, Behind }`; `pub fn za_search_content_needs_rebuild(manifest: &Manifest, config: &ZaSearchConfig) -> Option<ContentRebuildReason>` (called by the pass, step 0 of `za_search_pass`, after the shortcut is refused and the manifest read: `if let Some(r) = za_search_content_needs_rebuild(&base.manifest, &config) { match self.za_search_content_rebuild_start(account_id, keys, r).await { Ok(()) => return Ok(Advanced { published: false, indexed_to: base.manifest.indexed_to }), Err(e) if e.is_assertion_failure() => { za_search_trace!(ZA_SEARCH_LOST, AccountId = account_id); self.za_search_reload_if_moved(&entry, keys).await.ok(); return Ok(Lost) } Err(e) => return Err(e) } }` (FU12: another node's write is `Lost` plus a reload, the pass's convention, never an error), which serves `case_f6_r23`'s "one keyed request starts the content-only rebuild" and the content wait alike, whose next `za_search_pass` runs the content cursor, FT4; Task 9's `za_search_ensure_current` does not call it, and calls `za_search_content_rebuild_start(.., ContentRebuildReason::ContentLoss(class))` only when `za_search_load_content` answers `Err(Loss(class))`, then re-enters its loop once, FS6); `Server::za_search_reconcile_start(&self, account_id: u32, keys: &SessionKeys, first_retained: u64) -> trc::Result<()>` (the pass calls it when `za_changes_records` reports `truncated` or `resumed_record_lost`); `pub(crate) fn second_truncation(marker_bound: u64, bound_now: u64, indexed_to: &Checkpoint) -> SecondTruncation` and `pub(crate) enum SecondTruncation { No, BoundMoved, Restart }` (rule 4, FS1, FT5); `pub(crate) fn cursor_items(cursor: &RebuildCursor, cache: &MessageStoreCache) -> Vec<u32>` (ascending); `pub(crate) fn content_cursor_items(cursor: &ContentCursor, index: &LoadedIndex) -> Option<Vec<u32>>` (FW3: keyed on the manifest, not on the resident part: when `index.manifest.content.is_none()` every present document below `next_below`, descending, whatever part is resident (the `ContentLoss` case); when the manifest names a head, `Some` only if `index.content` is resident with `segments.first() == Some(&head.segment)`, listing the present documents below `next_below` whose id is not in its `document_ids()`, the backfill's skip, E10; else `None`, the block not runnable on this pass); `pub(crate) struct Difference { pub gap_inserts: Vec<u32>, pub gap_deletes: Vec<u32>, pub retained_from: Option<Checkpoint> }` (`None`: every retained record; `Some(indexed_to)`: the ordinary resume, step 4) and `pub(crate) fn difference(index: &LoadedIndex, cache: &MessageStoreCache, retained: &ZaChangeRecords, marker: &ReconciliationMarker) -> Difference`; `Store::za_principal_blob_links(&self, account_id: u32) -> trc::Result<Vec<BlobHash>>`; test helpers `lower_changes_history(test, admin, max: Option<u64>)` (sets `DataRetention { max_changes_history }` through `registry_update_setting(.., &[Property::MaxChangesHistory])` and `reload_settings`, as `tests/src/system/purge.rs:39-46`), `purge_account(test, admin, account) ` (`Task::AccountMaintenance { maintenance_type: TaskAccountMaintenanceType::Purge }` then `wait_for_tasks`, as `purge.rs:144-152`).

Rules this task implements (the implementer applies them as written; spec 5.4, 5.1's error classes, invariants 50, 53, 55, 56):

- **Loss** (and only loss) starts a rebuild: no manifest on a key account; `ManifestError::OlderFormat`; `ManifestError::Tag` or `Parse`; a thread blob absent past `ZA_SEARCH_ABSENT_RETRY_SECS` or failing `open_section`/decode. `NewerFormat` and a `Destroying` marker are `NoMap`, never loss. A content blob's loss is `ContentLoss`, never a rebuild. A transient error is never loss.
- **The first batch of a rebuild** is one `commit_batch` on `Collection::Principal` document 0 of the account:
  1. `cache = get_cached_messages(account_id)` (revalidated); `W = cache.emails.document_ids().min()`.
  2. The manifest: `revision = old.revision + 1`, or `1` when the old manifest was missing or failed its tag or parse (Task 8's rule: after a publication `revision == newest_row.revision`); `newest_row = RevisionRow { revision, change_id: 0 }` with `ManifestFill { newest_row: true, epoch: true }` so both take the batch's change id at commit; `indexed_to = Checkpoint { change_id: cache.last_change_id, position: 0, complete: true }`; `epoch = 0` (filled); `thread_blobs = []`; `content = None`; `content_to = ContentTo { checkpoint: indexed_to, suspended: false, cursor: None }`; `cursor = Some(RebuildCursor { next: 0, end: cache.emails.document_ids().max().unwrap_or(0) })` (no cursor when the account holds no message); `reconciliation = None`; `failing = None`.
  3. `assert_value(PrincipalField::ZeroAccessSearch, AssertValue::Hash(xxh3_64(old_bytes)))`, or `AssertValue::None` when there were none; when `None`, also `assert_value(PrincipalField::ZeroAccessVault, AssertValue::Hash(vault_read.cas))` (R19: `za_vault_record` read in the same function; no vault record is `Err`, the account is not a key account).
  4. `set(PrincipalField::ZeroAccessSearch, SetValue::serializable(ManifestWrite::new(ManifestValue::Index(manifest), key, fill)))`.
  5. The unlinks: for every thread blob of the old inventory and every content blob the chain from the old head reaches (`get_blob(hash, 0..BlobHeader::LEN)` then `BlobHeader::decode`, following `predecessor` until `None`): `clear(ValueClass::Blob(BlobOp::Link { hash, to: BlobLink::Document }))` under `with_collection(Collection::Principal).with_document(0)`. If the old manifest was unreadable, or any node of the chain is absent or does not decode, the unlink set is instead `za_principal_blob_links(account_id)` (every principal-linked blob of the account; R7, F23).
  6. The boundary entries, when `W` exists: `with_collection(Collection::Email).with_document(W).log_item_update(SyncCollection::Email, None)`; `log_container_property_change(SyncCollection::Thread, PendingId::Assigned(W))`; and for every mailbox of `cache.mailboxes.items`: `log_container_property_change(SyncCollection::Email, PendingId::Assigned(mailbox_id))`. When no message exists, the mailbox entries alone (G3: the boundary still allocates a fresh change id).
  7. `commit_batch`. An assertion failure (`err.is_assertion_failure()`) is `LostToAnotherNode` (another node's first batch, or destroy's marker): reload and return; nothing else is retried here. The committed batch moves the manifest hash, so this node's next manifest read (the pass's step 0 or the snapshot's step 4) reloads `Whole` (the thread list is empty), which is how the loaded index is replaced.
  Trace `ZA_SEARCH_REBUILD` with the account id, `"rebuild"`, the loss class, the cursor size (`end + 1 − next` or 0) before the commit and `"boundary"` after it.
- **The cursor**: `cursor_items` = `cache.emails.document_ids()` filtered to `next ..= end`, sorted ascending. The pass (Task 8) works them before any log record after `indexed_to`, as inserts with `created = epoch`; its `CursorStep` is `Advanced(id + 1)` when the cap stopped it and `Exhausted` when it ran off the list or the list was empty, which clears the cursor in its batch, a manifest-only write when nothing was staged (R22; Task 8's `bookkeeping`, FR1). Step 0's shortcut does not apply while `cursor.is_some()` (R10). A message that vanished between the snapshot and its turn is absent from the list and needs nothing; one that arrived after the snapshot has an id above `end` and is in the log after `indexed_to` (H15).
- **Content-only rebuild** (`za_search_content_rebuild_start`): a manifest-only write (assert the previous bytes), by reason (E10): for `ContentLoss(_)`, `content = None` with the chain's nodes unlinked (walk from the old head; a break takes the scan **minus the thread blobs the asserted manifest still names**, `manifest.thread_blobs[..].hash`, PL5: `za_principal_blob_links` returns every principal document-0 link of the account, the thread blobs included, and unlinking those would let the next purge delete them, so a content loss would become a thread loss and a full rebuild with a new epoch) and `content_to = ContentTo { checkpoint: Checkpoint { change_id: 0, position: 0, complete: true }, suspended: false, cursor: Some(ContentCursor { next_below: u32::MAX }) }`; for `CapRaised` and `Behind`, `content` and the chain are kept and nothing is unlinked, and `content_to = ContentTo { checkpoint: unchanged, suspended: false, cursor: Some(ContentCursor { next_below: u32::MAX }) }` (the backfill of spec 5.4 and 9.2: the cursor skips every message that has a content record, so only the messages indexed while suspended, or after the lag began, are re-opened). `content_cursor_items` = the document ids the thread part holds as present, below `next_below`, without a content record, descending (newest first); the pass admits them under the same cap, opening each and staging a content record only (no thread assignment, no alias; the decision is `ContentOnly`, Task 8: a content blob and a manifest update, no thread blob, no revision, no log entry); a content cursor's pass consumes no log record; the content cursor steps as the rebuild cursor does (FR1, FS2), through Task 8's content block over `PassSources.content` and its `admit_content` (FT2): `content_cursor = CursorStep::Advanced(the last admitted or skipped id)` when the cap stopped the loop (Task 8's arm writes it as `next_below`); when the loop ran off the list, or the list was empty, `content_cursor = CursorStep::Exhausted`, and Task 8's arm sets `content_to.checkpoint = indexed_to` and `content_to.cursor = None`, a manifest-only write when it staged nothing; the block runs only while `base.manifest.content_to.cursor.is_some()` and the rebuild cursor is `None` or ran off its list this pass (Task 8's source order, FT8). No epoch move, no identity change. The content cursor runs on a pass whose node holds the content part resident and current, except while the manifest names no content head (the first pass after a `ContentLoss` start, when every present document is listed; after its first blob the same residency rule applies, FW14); the pass never loads the part for the backfill (FX12), a content request's `za_search_ensure_current` does (FW2), and a pass that cannot run the block runs everything else (FW1); plan call E10. `za_search_content_needs_rebuild` answers `Some(CapRaised)` when `manifest.content_to.suspended` and `manifest.content.size < config.account_max_mb · 1 MiB` (FU12: `<`, the complement of `limits.content_allowed`'s `size < cap`, so a suspended account never starts a rebuild under a cap it would meet again at once); `Some(Behind)` when `!content_to.suspended` and `content.size < cap` (FV1: spec 5.4's "with the chain loadable"; an account that has just crossed the cap is `suspended` with its checkpoint behind, and without this guard FU1's shortcut term would start a chain-dropping rebuild on it, re-index its content newest first until the cap, and suspend with the cursor pending for good, against 4.2's "the chain is not extended") and `content_to.cursor.is_none()` and `content_to.checkpoint.change_id < indexed_to.change_id` and the manifest is not mid-rebuild (`cursor.is_none()`); else `None`. With that guard `Behind` is a safety net: after a cap crossing the manifest is `suspended`, and after a content cursor's `Exhausted` arm the checkpoint equals `indexed_to`, so no ordinary path reaches it. While a content cursor is pending, content requests wait up to `ZA_SEARCH_WAIT_SECS` while their passes run the cursor, and refuse `NotCurrent` if it is still pending (FX13; `is_current` requires no pending content cursor, Codex PL20).
- **Reconciliation** (`za_search_reconcile_start` + `difference`): when `za_changes_records` from `indexed_to` reports `truncated` or `resumed_record_lost`, the pass writes a manifest-only checkpoint with `reconciliation = Some(ReconciliationMarker { bound, gap_next: 0 })`, `bound = max(za_log_first_retained(Email).unwrap_or(0), za_log_first_retained(Thread).unwrap_or(0))` as read now (the shorter log's bound, G4, the same value as Task 13's `za_retained_bound` and Task 15's compaction bound; at least one marker exists when a reconciliation starts, so `bound >= 1` and the stamps `bound − 1` are defined), and `cursor = None` if a rebuild cursor was pending (its remaining items are in the difference; the epoch stays). Each later pass, while the marker is set, works in this order, under the ordinary cap, and never applies step 0's shortcut:
  1. `retained = za_changes_records(Email, from: None /* every retained record */, max_records: usize::MAX)` (FT6: uncapped, the retained log being at most `maxChangesHistory` records; a capped read would turn a retained insert above the cap into a gap insert a client between its record and `E` would never be shown), read once per pass, unfolded; `inserted_in_log` = the document ids of its insert changes, `deleted_in_log` = of its delete changes, over every retained record (spec 5.4's attribution; FS3 restores it after revision 3 narrowed it to the records step 4 would consume: the state that narrowing addressed cannot arise, since a reconciliation starts only from a read that met the marker, whose record is empty, so every retained record with content is above `indexed_to` when the marker is written; and the narrowing re-opened a retained insert skipped as loss once `indexed_to` had passed its record).
  2. `gap_inserts` = `cache.emails.document_ids()` minus the index's present set minus `inserted_in_log`, **at or above the marker's `gap_next`**, ascending (PL6: the spec's cursor over the difference; a gap insert skipped as loss never enters the index, so without the cursor it would stay in the difference and be re-opened by every pass, and a cap-sized run of them would starve every later gap item); `gap_deletes` = the index's present set minus the cache's present set minus `deleted_in_log`, any order. Items of the difference the retained log *does* show are not gap items: they take their record's change id when the retained records are consumed (step 4).
  3. Gap inserts first, under the pass's cap, through Task 8's gap blocks over `PassSources { gap_inserts, gap_deletes, gap_stamp: Some(bound − 1) }` (FT2; `bound_now − 1` on a `BoundMoved` pass, FU3): each opened and indexed as an insert with `created = bound − 1` or skipped as loss, and after each, indexed or skipped, `marker = MarkerStep::GapNext(id + 1)` (written as the marker's `gap_next` by Task 8's arm, FS2); then, only in the pass whose insert loop ran off its list, the gap deletes, each tombstoned with `deleted = bound − 1` (F9, H3, I1; FU2: a gap insert admitted after the cap must still find the messages the gap deleted, so the deletions wait for the last gap insert; a tombstone never fails to apply, so a tombstoned gap delete leaves the recomputed difference and needs no cursor).
  4. Then the retained records in change-id order, by the pass's ordinary rules (an insert the index holds is skipped; one that does not open is skipped as loss and consumed; a deletion is unconditional), moving `indexed_to` as a pass does, from `retained_from`: every retained record (`from: None`, the first retained key inclusively, which is the marker's and empty) while `indexed_to.change_id <= first_retained` (FS1: at `==`, the marker landed on `indexed_to`'s own record, so a read from `indexed_to` would meet the marker on every pass; reading from the first retained key loses nothing, every record with content being above it), and from `indexed_to` (the ordinary resume, partial or complete) once retained processing has moved it past `first_retained` (PL6: completed retained work is never re-consumed; the attribution of step 1 reads every retained record, unconsumed). While the marker is set, a second truncation is detected by the bound, never by the `truncated` flag (FS1): when a read reports `truncated` and the log's bound now read (`za_retained_bound`, the max of the two logs' markers this rule's first sentence defines as `bound`; FU13) is above `marker.bound`, then if the move lost unconsumed records, `bound_now > indexed_to.change_id || (bound_now == indexed_to.change_id && !indexed_to.complete)` (the probe's test of step 4's wiring), the pass restarts the reconciliation (`za_search_reconcile_start` with that bound and `gap_next = 0`) and returns `Recovery(Truncated)`, which Task 9 re-enters once (`SecondTruncation::Restart`, FV10), else it fills `PassSources.marker_bound = Some(bound_now)` (copied into `Staging.marker_bound` by `plan_pass`, FW5), which Task 8's marker arm writes as the marker's `bound`, uses `gap_stamp = bound_now − 1` for this pass's gap items, and consumes on (`BoundMoved`; a pass with nothing else staged is a `ManifestOnly` write, FU3; a `BoundMoved` pass that also finishes the reconciliation writes no bound, FV2; FT5: a move that lost nothing resets no progress, and later stamps use the new bound, still below every servable state); a `truncated` read whose bound equals `marker.bound` is the expected meeting with the marker's own key, and its records are consumed.
  5. When `gap_inserts` (at or above `gap_next`) and `gap_deletes` are empty and the pass's checkpoint after step 4, `staging.checkpoint.change_id` (FS7: never `base.manifest.indexed_to`, which the update-only records after the last publication have not raised), is `>= cache.last_change_id` as read at this pass, the pass sets `marker = MarkerStep::Finished` (computed inside `plan_pass` from `PassSources.finish_at = cache.last_change_id` read at step 0, FW5; Task 8: `bookkeeping`, so the manifest is written even when nothing else was staged or consumed, FR1) and its batch clears the marker; a gap insert skipped as loss is then absent from the index, as a skipped insert of an ordinary pass is. Aliases are kept; the epoch does not move.
  The message cache is consulted for existence here and nowhere else in the pass (G11).
- **Lifecycle of the marker reads** (`load.rs`): `ManifestValue::Destroying` → `NoMap(Destroying)`; the pass and the snapshot return at once.

- [ ] **Step 1: Unit tests for the pure parts** (`rebuild.rs`, `#[cfg(test)]`, no store): `cursor_items_are_the_snapshot_between_next_and_end` (ids `{3, 5, 9, 12}`, cursor `{next: 5, end: 9}` → `[5, 9]`); `content_cursor_items_run_newest_first_and_skip_content_present` (records `{1, 2, 3}`, content `{2}` resident with the manifest's head, `next_below: u32::MAX` → `Some([3, 1])`; FW13: manifest `content: None` with a stale resident part holding `{2}` → `Some([3, 2, 1])`; a manifest head with no resident part → `None`; a resident part whose head segment differs from the manifest's → `None`); `difference_excludes_what_the_retained_log_shows` (cache `{1, 2, 4}`, index present `{1, 3}`, retained records: insert 4, delete 3 → `gap_inserts = [2]`, `gap_deletes = []`); `difference_orders_gap_inserts_ascending` (`[7, 2, 5]` → `[2, 5, 7]`); `difference_resumes_gap_inserts_at_the_cursor` (gap inserts `{2, 5, 7}` with `gap_next: 5` → `[5, 7]`; with `gap_next: 8` → `[]`, so a skipped gap insert below the cursor does not keep the marker set, PL6); `difference_resumes_retained_records_from_indexed_to` (`first_retained = 40`: `indexed_to = (30, 0, true)` → `retained_from == None`; `indexed_to = (40, 2, false)` → `None` and `indexed_to = (40, 0, true)` → `None` (FS1: the marker on `indexed_to`'s own record); `indexed_to = (44, 2, false)` → `Some((44, 2, false))`); `second_truncation_is_detected_by_the_bound` (FS1, FT5: `pub(crate) fn second_truncation(marker_bound: u64, bound_now: u64, indexed_to: &Checkpoint) -> SecondTruncation` with `pub(crate) enum SecondTruncation { No, BoundMoved, Restart }`: `(40, 40, (44, 0, true)) == No`, `(40, 41, (44, 0, true)) == BoundMoved`, `(40, 45, (44, 0, true)) == Restart`, `(40, 44, (44, 2, false)) == Restart`, `(40, 44, (44, 0, true)) == BoundMoved`); `difference_attributes_over_every_retained_record` (FS3: `indexed_to = (49, 0, true)` after step 4 consumed records 46 to 49, `first_retained = 45`, retained inserts at 46..=50 of documents 46..=50, the index holding 46, 47 and 49 (48 skipped as loss), the cache holding 46..=50 → `gap_inserts == []`, since 48's and 50's inserts are retained, and `retained_from == Some((49, 0, true))`; a comment that a retained record at or below `indexed_to` at the marker's writing is unreachable, the marker's own record being empty); `reconciliation_h3_gap_insert_threads_with_gap_deleted` (a `ThreadPart` fixture: `X` present and indexed, gap insert `Y` with `In-Reply-To: X`, gap delete `X`; after the order gap-insert-then-gap-delete `thread_of(Y, now) == thread_of(X record, now)`; applying the delete first leaves `Y` a singleton: the mutant's assertion); `reconciliation_i1_retained_insert_does_not_see_gap_deleted` (gap delete `X`, retained insert `Z` replying to `X`: `Z` is a singleton, as upstream); `first_batch_fields` (build the manifest value with `old = None`, `cache.last_change_id = 40`, ids `{2, 7}` → `revision 1`, `newest_row.revision 1`, `indexed_to == (40, 0, true)`, `cursor == Some({next: 0, end: 7})`, `content_to.checkpoint == indexed_to`, `content == None`; with an old manifest at revision 6 → `revision 7`, row `7`); `content_needs_rebuild_rules` (suspended + size under the cap → `CapRaised`; not suspended, size under the cap, checkpoint behind `indexed_to` with no cursors → `Behind`; a pending rebuild cursor → `None`; suspended + size at or over the cap + checkpoint behind + no cursors → `None`, FV1; not suspended + size at or over the cap + checkpoint behind → `None`).
- [ ] **Step 2: Run them to see them fail**: `cargo test -p email --lib --features test_mode,enterprise za::search::rebuild` → FAIL with unresolved names (`cursor_items`, `difference`, …).
- [ ] **Step 3: Implement `rebuild.rs`**: the functions of Produces with the rules above. `za_search_rebuild_start` reads the old manifest with `za_search_read_manifest` (raw bytes for the assertion, decoded or not), the vault record, the cache, walks the chain's headers with `get_blob(hash, 0..BlobHeader::LEN)`, builds the batch field by field as listed, commits through `commit_batch`, and traces. `za_search_content_rebuild_start` and `za_search_reconcile_start` are manifest-only writes asserting the previous bytes. `Store::za_principal_blob_links` iterates, for `shard in 0u8..=255`, the purge's range (`ValueKey { class: ValueClass::Blob(BlobOp::Commit { hash: from }) }` to `BlobOp::Link { hash: to, to: BlobLink::Document }` with the shard in the hash's first byte, `crates/store/src/write/blob.rs:162-182`), keeps the keys of length `BLOB_HASH_LEN + U32_LEN + 1 + U32_LEN` whose suffix is `account_id.to_be_bytes() ‖ Collection::Principal as u8 ‖ 0u32.to_be_bytes()`, and returns their hashes.
- [ ] **Step 4: Wire the pass** (`pass.rs`): at step 0, `cursor.is_some() || reconciliation.is_some() || za_search_content_needs_rebuild(&manifest, &config).is_some()` disables the shortcut (FU1: on an idle account whose checkpoint sits at the cache's counter, after a deletion-only pass or an update-only checkpoint, nothing else would ever start a `CapRaised` or `Behind` rebuild, and every text search would spin to its deadline), and after the manifest read and the reload-if-moved the due rebuild is started as Produces says; at step 1 the item source is `cursor_items`, then `content_cursor_items` (computed in step 0's read block from the loaded index, `None` making the block not runnable on this pass, FW3, FW7, FW12), then the difference's gap items, then the log (while the marker is set, from `Difference.retained_from`: every retained record until `indexed_to` has passed `first_retained`, then from `indexed_to`; else from `indexed_to`); the marker's `gap_next` and the content cursor's `next_below` reach the manifest through `Staging.marker` and `Staging.content_cursor` and Task 8's three arms in `write.rs` (FS2); `truncated || resumed_record_lost` → `za_search_reconcile_start` and return `Recovery(Truncated)` while `reconciliation.is_none()`; and on a pass with a pending rebuild cursor, which reads no log, the truncation is probed instead (FS4, spec 5.4 "if the log truncates while a rebuild cursor is pending, the difference cursor subsumes the rebuild's"): before the cursor block, `bound = za_retained_bound()` (one point read per log), and `bound > indexed_to.change_id || (bound == indexed_to.change_id && !indexed_to.complete)` → `za_search_reconcile_start` (which clears the cursor, its remaining items being in the difference) and return `Recovery(Truncated)`; with the marker set, the read of every retained record (`retained_from == None`) meets the log's truncation marker by design, so `truncated` is then expected and the records after it are consumed, while a `truncated` read whose bound is above `marker.bound` is judged by `second_truncation(marker.bound, bound_now, &indexed_to)` (rule 4, PL6, FS1, FT5, FU3): `Restart` restarts the reconciliation, `BoundMoved` fills `PassSources.marker_bound = Some(bound_now)` (FW5; written by Task 8's arm) and consumes on with `gap_stamp = bound_now − 1`, `No` consumes on; a cursor exhausted or a marker finished is cleared in the batch (manifest-only when nothing was staged). In `load.rs`: `Destroying` → `NoMap(Destroying)`. In Task 10's `za_view` (which Task 11's `za_snapshot` calls), `Err(LoadFailure::Loss(class))` from `za_search_entry` calls `za_search_rebuild_start(account_id, keys, class)` and re-enters once (Task 10 wrote that arm as a trace and `NoMap(Transient(Store))`; replace it); the pass's inline cursor filter of step 1 becomes `rebuild::cursor_items`, with `content_cursor_items` and `difference` added beside it. Task 14 also adds the loop-top content load of Task 9's `za_search_ensure_current` and its two content-loss arms, each reloading after its start (FX2, FX7). In Task 9's `za_search_pass_hook` and `za_search_ensure_current`, the `Ok(Recovery(cause))` arm calls `za_search_rebuild_start` for `RecoveryCause::Loss(class)` and, for `Truncated`, only re-enters (the pass wrote the marker before returning it, with the bound and the cursor in hand; FT3: one starter, one manifest write), then re-enters the pass once; a second `Recovery` in the same request is traced and left for the next request.
- [ ] **Step 5: Run the unit tests**: `cargo test -p email --lib --features test_mode,enterprise za::search::rebuild` → PASS; `cargo test -p store --lib za_search` → PASS (nothing there changes).
- [ ] **Step 6: Suite cases in `mail::index`** (accounts `keymail58` for the rebuild cases, `keymail59` for the content cases, `keymail60` for the reconciliation cases; the control `plainmail56`). Each case is an inner `async fn` called from `test`, in this order; "force a loss" is the helper `force_loss(test, account_id)` this task adds to `tests/src/utils/za.rs` (deletes the manifest's first thread blob from the blob store with `blob_store().delete_blobs(&[hash])`, then `simulate_restart`), unless the case names another loss:
  - `case_r3_rebuild_moves_the_epoch`: deliver three messages, run a keyed `Email/get` (the pass), record `s0 = Email/changes state`; delete the manifest's first thread blob from the blob store directly (`test.server.blob_store().delete_blobs(&[hash])`) and `simulate_restart`; a keyed `Email/get` succeeds (the rebuild ran: `read_manifest(..).cursor == Some(..)` or already `None`, `epoch > 0`); `Email/changes` with `sinceState: s0` answers `cannotCalculateChanges` (`reply.error_type_at(0) == Some("cannotCalculateChanges")`); `Email/query` then `Email/changes` from the new state is an empty round trip; repeat with the manifest deleted (clear field 152 directly) and with the manifest's format byte lowered to 0 (`OlderFormat`): three rebuilds, three strictly increasing epochs, no mail activity between the last two (`epoch2 != epoch1`).
  - `case_f7_newer_format_is_no_map`: write field 152 with the format byte set to `SEARCH_FORMAT_V1 + 1` (and a valid tag is not needed: the format check comes first); `Email/get` answers `serverUnavailable`, `Email/query` with a text filter answers `unsupportedFilter` with `ZA_MAIL_SEARCH_UNAVAILABLE`; `read_manifest` still shows the newer byte (no rebuild); restore the bytes.
  - `case_r10_cursor_finishes_across_restarts`: 200 delivered messages, `ZA_SEARCH_PASS_CAP` honoured (48 per pass); force a loss; after each keyed request `simulate_restart`; count requests until `read_manifest(..).cursor.is_none()` (≤ 6) and every message is in `Email/query` with a text filter (a content request, which waits).
  - `case_r22_vanished_cursor_checkpoints`: force a loss with 60 messages, run one keyed request (48 indexed), delete the remaining 12 through POP3 `DELE` + `QUIT` (`pop3_delete`, Task 13's helper; a POP3 session runs no pass, so the 12 are never indexed), `simulate_restart`, one more keyed request: `cursor == None`, the manifest's `thread_blobs.len()` unchanged by that request (no blob), and no log entry was written by it (the Email change id moved only by the expunge).
  - `case_h11_missing_manifest_on_an_empty_account`: a fresh key account, clear field 152 directly, `Mailbox/get` (keyed) succeeds; `read_manifest` is `Some(Index)` with `epoch > 0`, `thread_blobs == []`, `cursor == None`; a second `Mailbox/get` does not change `manifest_hash` (no second rebuild).
  - `case_h23_content_loss_is_content_only`: `search_config_guard!(absent_retry_secs: 0)` (FS6: with the default ten-second window the loss would first read as `Transient(AbsentWithinWindow)` and the query would refuse); with mapped ids recorded from `Email/get`, record every content blob hash of the chain, delete the content head blob from the blob store, `simulate_restart`; `Email/get` returns the same ids (identities unchanged, `epoch` unchanged); no mail is delivered from here on (PL4: the shortcut must not answer while `content_to.cursor` is pending, so the content rebuild runs on an idle account); the first `Email/query` with a text filter waits for the content cursor and succeeds; after the start none of the recorded hashes is in `principal_blob_links` (the loss drops the chain, E10, FW10); `read_manifest(..).content_to.cursor == None` afterwards.
  - `case_pl5_content_chain_break_keeps_the_thread_blobs` (`keymail59`): `search_config_guard!(segments: 64, absent_retry_secs: 0)` so no compaction runs; three deliveries, each followed by a keyed request (three content nodes, three thread blobs); record the mapped ids, `read_manifest(..).epoch` and the thread blobs' hashes; delete the middle content node from the blob store and `simulate_restart`; a keyed `Email/query` `{"text": ..}` (the content loss starts the content-only rebuild, whose walk meets the break and takes the scan); `expire_and_purge_blobs(test, &admin)`; `simulate_restart`; every recorded thread blob hash is still in `principal_blob_links` and readable (`stored_blob`), `Email/get` returns the recorded ids, `epoch` is unchanged, no `ZA_SEARCH_REBUILD` trace of kind `rebuild` was emitted (content-only ones only), and the text query is answered once the content cursor finishes.
  - `case_f6_r23_over_the_cap_then_raised`: set `ZA_SEARCH_ACCOUNT_MAX_MB` low for the case (the config is read once at boot, so the test sets `search_config_guard!(account_max_mb: 1, segments: 255)`, plan call D3; `segments: 255` so no content compaction merges the chain during the case, FW8); deliver one message at a time (each a body of about 200 KiB of distinct words, so the 1 MiB cap is crossed within about six deliveries, FX4), each followed by a keyed `Mailbox/get`, until `read_manifest(..).content_to.suspended`, and at the first manifest whose `content.size` is over the cap while `suspended` is still false, a text search answers `ZA_MAIL_SEARCH_UNAVAILABLE` within 2 s (FW4: `size > cap` refuses `OverCap` before the flag is set); `Thread/get` works, a text search answers `ZA_MAIL_SEARCH_UNAVAILABLE` within 2 s (`tokio::time::timeout`; `OverCap` at once, FV12, FW10), `content.length` stops growing across two more deliveries, each followed by a keyed `Mailbox/get` (FX4), and during this suspended phase, after the `Thread/get` and the text search, `read_manifest(..).content_to.cursor.is_none()`, `content.length` and `principal_blob_links` are unchanged by them and no `ZA_SEARCH_REBUILD` trace was emitted (FV1: no spurious start, no chain drop); replace the guard by `search_config_guard!(segments: 255)` (the cap back to its default while compaction stays held off through the backfill, Codex PL21, FX4), `simulate_restart`, one keyed `Mailbox/get` starts the content-only rebuild (`content_to.cursor.is_some()`; the pre-suspension chain is kept: `content.length` and every pre-suspension blob hash in `principal_blob_links` unchanged by the start, E10; FW8: a non-content request, so nothing of the backfill runs in it); FX9: `simulate_restart`, deliver one message, `reset_za_search_counters(id)`, one keyed `Mailbox/get`: `index_holds(new)`, `indexed_to` covers it, and `za_search_blob_reads(id)` equals the manifest's thread blob count (no content blob read by a non-content request, FW1, FW2); `reset_za_search_counters(id)`; the first text search loads the content part and waits while its passes run the backfill (E10, FW2), then is served and finds the messages indexed while suspended and the ones before; over that search `za_search_message_opens(id)` equals the number of messages indexed without content, counted from the pass that set `suspended` on, that pass's own delivery included (FW8), and `content.length` grew by the backfill's blobs only (the backfill, not a re-index of the chain); the Email ids of `Email/get` are unchanged throughout. Then the FU1 variant on the same account: suspend again (`search_config_guard!(account_max_mb: 1, segments: 255)`, deliveries each followed by a keyed `Mailbox/get`), destroy one message on the keyed session and run one keyed request (a `NonPublishing` pass: `indexed_to` at the delete record's change id, the account's newest, no log entry), replace the guard by `search_config_guard!(segments: 255)`, `simulate_restart`; the first keyed request starts the content-only rebuild (`content_to.cursor.is_some()`) although the shortcut's other terms all hold, and the text search is served within the wait.
  - `case_f9_reconciliation_renders_nothing_seen` (`keymail60`): deliver 30, keyed request, record `s1` and the ids; deliver 20 more without a keyed request; `lower_changes_history(.., Some(5))` and `purge_account` (truncates both logs); keyed request: `read_manifest(..).reconciliation.is_some()` after the first pass or `None` if one pass finished it; run keyed requests until `None`; `Email/changes` since `s1` answers `cannotCalculateChanges` (the truncated log, as upstream); from a fresh `Initial`, every present message is created exactly once; restore `lower_changes_history(.., None)`.
  - `case_pl6_unreadable_gap_and_retained_inserts_finish` (`keymail60`): `search_config_guard!(pass_cap: 4)`; deliver 30, keyed request; deliver 20 more with no keyed request, and `corrupt_blob` one of the first 15 of them (a gap insert once truncated) and one of the last 5 (a retained insert); `lower_changes_history(.., Some(5))` and `purge_account`; `Traced::start(&[ZA_SEARCH_EVENT_TYPE])`; keyed requests, with `simulate_restart` after every third, until `read_manifest(..).reconciliation.is_none()`, at most 20; exactly two `ZA_SEARCH_SKIPPED` events over the whole run (each unreadable message skipped once, never re-opened after a restart, PL6); every readable message is in `Email/query` with a text filter; a delivery after the marker cleared is indexed by the next keyed request; restore `lower_changes_history(.., None)`.
  - `case_f9_truncation_during_a_pending_rebuild`: force a loss with 100 messages, one keyed request (cursor pending, 48 indexed, `indexed_to` at the pre-boundary change id); assert `read_manifest(..).cursor.is_some()` and `log_records_since(.., L).len() == 2` (FT9: exactly two Email keys above `indexed_to = L`, the boundary at `L + 1` and the re-entered cursor pass's witness at `L + 2`, so the marker below lands on `L + 1`); `lower_changes_history(.., Some(1))` and `purge_account` (FS4: the marker lands on the boundary's record, above `indexed_to`; with `Some(5)` it would land below it and no pass could notice); next keyed request: `cursor == None && reconciliation.is_some()` (the cursor pass's probe, Task 14 step 4), the epoch unchanged from the rebuild's; finish; `Email/query` lists all 100; restore `lower_changes_history(.., None)`.
  - `case_r24_g2_retained_deletion_rendered_once`: a message `D` indexed and its mapped id recorded at state `s`; truncate the log so that `D`'s later deletion record is retained but `indexed_to` is below the truncation (deliver 12, delete `D`, `lower_changes_history(.., Some(8))`, purge, with no keyed request in between); reconcile; `Email/changes` since `s` lists `T|D` destroyed exactly once and `Email/changes` from the state it returns lists nothing for `D`.
  - `case_r27_empty_rebuild_keeps_clients`: delete every message of `keymail58`, keyed request, record `s`; force a loss; keyed request (the boundary writes mailbox entries only); `Email/changes` and `Thread/changes` since `s` both succeed and are empty.
  - `case_g3_loss_with_all_deleted_refuses_holders`: record `s` (`Email/changes` and `Thread/changes` states) while two messages `a`, `b` exist and their mapped ids are held; delete both on a keyed session (their delete records land in the Email log above `s`, and `log_threads_emptied`'s container deletes in the Thread log); force a loss; one keyed `Mailbox/get` runs the boundary, which writes the mailbox entries alone (no message is present) and still allocates a fresh `epoch`; `Email/changes` since `s` answers `cannotCalculateChanges` (the Email log holds item records, the two deletions, in `(s, epoch]`), and `Thread/changes` since `s` answers it too (the Thread log holds the two container deletes there); a client that resyncs from `Initial` gets an empty account. The contrast is `case_r27_empty_rebuild_keeps_clients`, whose `s` is recorded after the deletions, so `(s, epoch]` holds the mailbox entries only and both methods are served.
  - `case_r2_old_epoch_map_never_served`: `SEARCH_SNAPSHOT_PAUSE` parks a keyed `Email/get` between its pass and its snapshot; in the gap, force a loss and run a keyed request on a second connection (the rebuild); release: the parked response either carries the new epoch's ids (identical to a fresh `Email/get`) or fails `serverUnavailable`; never the old mapped ids under the new state.
- [ ] **P1-PAGING integration (Task 14):** add `case_initial_continuation_epoch_loss` in `mail::threads`: obtain an initial intermediate page, lose the thread index and rebuild without truncating the logs, then assert the old pinned end below the new epoch refuses for Email and Thread and fresh Initial succeeds. Content-only rebuild and reconciliation controls use their own still-valid token and identity era, and do not add an epoch refusal while E remains retained. This extends Task 13's actual page path, not a fake refusal helper.

- [ ] **Step 7: Suite cases in `mail::threads`** (`keymail58`, control `plainmail56`): `case_r16_r20_rebuild_form`: chain `A`, `D` (replies to `A`), `N` (replies to `D`) indexed on both accounts; force a loss on the key account; `ZA_SEARCH_PASS_CAP` override 2 so the first rebuild pass indexes `A` and `D` only; delete `D` through POP3 (`Pop3Connection`: `DELE`, `QUIT`; no pass); deliver `N2` with `In-Reply-To: D` to both accounts; keyed requests until the cursor and the log are consumed; `Thread/get` groups `A`, `N`, `N2` as the control groups them with `D` absent; `D` absent from every read; no admitted item wrote more than `K − 1` aliases (the bound is per item, not per pass), observed through the test-mode counter `za_search_max_item_aliases(account_id)` (`common::za::test_hooks`, beside `za_search_message_opens`: the largest `Assignment.aliases.len()` any `assign` of the account returned, recorded by the pass after each `assign` in test mode only), never through a trace; and the captured `ZA_SEARCH_PASS` events carry no alias count (PL8: the trace schema of Global Constraints omits it on purpose, since the number of aliases a pass wrote tells whether its bridges merged threads, which the padding and the fixed reservation exist to hide). Then its residual, pinned: `D` deleted while still pending in the cursor (cap 1, delete `D` after `A`'s pass), where `N2` stays a singleton on the key account and joins on the control (`assert_ne!`, with the spec's 7.5 bullet cited in a comment).
- [ ] **Step 8: Run the suite**: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture` → PASS, every case printed; `cargo fmt -p email -p store -p tests -- --check` clean.
- [ ] **Step 9: Commit**: `git add crates/email/src/message/za/search/ crates/common/src/za/search/load.rs crates/store/src/write/blob.rs tests/src/utils/za.rs tests/src/za/mail/index.rs tests/src/za/mail/threads.rs` and `git commit -m "Search: rebuild, content-only rebuild and reconciliation (spec 5.4)"`.

### Task 15: Compaction

**Files:**
- Create: `crates/email/src/message/za/search/compact.rs`
- Modify: `crates/store/src/za_search/thread.rs` (`ThreadPart::compact`, new), `crates/store/src/za_search/mod.rs` (`compaction_allowance` is Task 2's; nothing new), `crates/email/src/message/za/search/pass.rs` (step 4: the two compaction calls between staging and the batch), `crates/email/src/message/za/search/mod.rs`, `tests/src/za/mail/index.rs`, `tests/src/za/mail/threads.rs`
- Test: `crates/store/src/za_search/thread.rs` (`#[cfg(test)] mod compact_tests`), `crates/email/src/message/za/search/compact.rs` (unit), the suite modules above

**Interfaces:**
- Consumes: `ThreadPart`, `ThreadRecord`, `Alias`, `RevisionRow`, `Tombstone`, `ThreadPart::{revision_at, thread_of, members_of}` (Task 3); `ContentPart`, `ContentRecord::{encode, decode}` (Task 4); `BlobHeader { kind, tier, segment, predecessor }`, `SectionKind`, `frame_and_pad`, `PadTarget`, `HiddenSection`, `PublicSection`, `build_thread_blob`, `build_content_blob`, `ThreadBlobEntry`, `ContentHead` (Task 5); `padded_len`, `compaction_allowance`, `RECORD_MAX`, `ALIAS_LEN` (Task 2); `seal_section`/`open_section`, `LoadedIndex`, `IndexEntry` (Task 6); `Store::za_log_first_retained` (Task 7); the pass's `Staging` and batch assembly (Task 8: the batch builder is open when step 4 runs, so compaction adds its set/clear/link operations to it); `ZaSearchConfig::segments` (Task 2).
- Produces: `ThreadPart::compact(&self, bound: u64) -> CompactedThreadPart` with `pub struct CompactedThreadPart { pub part: ThreadPart, pub floor_row: Option<RevisionRow>, pub n_records: usize, pub dropped_aliases: usize, pub folded: usize }`; `Server::za_search_compact_threads(&self, account_id: u32, keys: &SessionKeys, index: &LoadedIndex, staging: &Staging, batch: &mut BatchBuilder, config: &ZaSearchConfig) -> trc::Result<Option<ThreadBlobEntry>>` (called by the pass when `index.manifest.thread_blobs.len() + 1 >= config.segments` after its own blob is staged; `None` when the compaction was refused as a defect, see rule 8); `Server::za_search_compact_content(&self, account_id: u32, keys: &SessionKeys, index: &LoadedIndex, chain: &[ChainNode], new_blob: Option<StagedContent>, cursor_pending: bool, batch: &mut BatchBuilder, config: &ZaSearchConfig) -> trc::Result<ContentChainUpdate>` with `pub struct ChainNode { pub hash: BlobHash, pub segment: SegmentId, pub tier: u8, pub size: u32 }` (the chain young first, from the headers or the loaded content's `segments`), `pub struct StagedContent { pub header: BlobHeader, pub bytes: Vec<u8>, pub hold: BlobHold }` (the pass's own content blob, staged) and `pub struct ContentChainUpdate { pub head: Option<ContentHead>, pub unlinked: Vec<BlobHash> }`; `pub fn tier_merge_plan(tiers_young_first: &[u8], segments: usize) -> Vec<MergeStep>` with `pub struct MergeStep { pub count: usize, pub into_tier: u8 }` (pure; unit-tested).

**Thread compaction, the procedure** (spec 5.5, decision 2 (c), R35; invariants 48 and 56). Input: the loaded thread part merged with the pass's staged records and aliases (the pass's own additions compact with the rest). Steps, in this order:

1. `bound = max(za_log_first_retained(Email).unwrap_or(0), za_log_first_retained(Thread).unwrap_or(0))`: the oldest retained change id of the shorter log (G4: the Thread log keeps older change ids than the Email log, so the higher marker is the shorter log's; a log without a marker contributes 0). Both `/changes` methods refuse every state below this bound (Task 13's `za_retained_bound` is the same value), so nothing a servable state needs is pruned.
2. `row_change_id(r)` = the change id of revision `r` from the revision table (the manifest's newest row included). Compute it for every record and alias before any row is dropped; a revision below the floor row's (its row dropped by an earlier compaction) takes the floor row's change id, which is below every later bound, so it compares as below the bound; the pass's own revision (`base.revision + 1`, staged records and aliases, no row until the batch commits) takes `u64::MAX`, since its change id is the batch's own and above every bound.
3. **Floor row**: `floor = the newest row with change_id < bound`, kept (plan call E1, ruled by Jay; spec revision 19, 5.5). Every other row with `change_id < bound` is dropped; rows at or above the bound are kept.
4. **Record pruning (the bridge rule)**: a deleted record (`deleted.is_some()`) is dropped only when `deleted < bound` **and** `row_change_id(record.revision) < bound`. A present record is never dropped. A deleted record kept by this rule keeps its tombstone entry.
5. **Folding, the sub-bound prefix of each chain** (PL7): `part.fold_sub_bound(|a| row_change_id(a.revision) < bound)` (Task 3) on the retained records: each record walks from its assignment along the sub-bound aliases to the first thread with none, takes that thread and the last followed alias's revision, and every sub-bound alias is then dropped. This is the spec's one-hop folds applied in chain order (an `A → B` below the bound then `B → C` below the bound moves a record at `A` to `C`, through both, whatever revision each carries and in whatever order they are stored); a sub-bound alias never moves a record past an alias at or above the bound, since revisions never decrease along a chain. Folding one alias at a time in ascending revision order is wrong: the aliases one pass staged share a revision.
6. **Dead-alias rule, to a fixed point**: repeat until no alias drops: drop every remaining alias whose `source` has no retained record with `thread_id == source`.
7. Rebuild both derived thread tables from the folded assignments: the present table from the retained present records, `members` from every retained record, present or deleted (FS10: a `members` table left over from before the fold would list a document under its pre-fold thread); `n_records` = retained records, present and deleted.
8. **Encode and pad**: hidden plaintext = the retained records (a deleted record carries no hashes) and the retained aliases; `allowance = compaction_allowance(n_records)`; if `hidden.len() + 4 > allowance`, the compaction is refused as a defect (trace `ZA_SEARCH_ABORTED` with reason `"allowance"`, return `None`; the pass writes its blob uncompacted and the next pass retries), since by the bound of step 6 it cannot happen; else `frame_and_pad(&hidden, PadTarget::Compaction { n_records }, config.pad_min)?` (its `AllowanceExceeded` is this refusal). Public plaintext = one `Tombstone` per retained deleted record and every retained row (floor and above) the input part holds, the base manifest's newest row included (it is in the loaded part, since `load_thread_part` extended the part with it, and in no blob yet), excluding only the pass's own new row, whose change id is unknown until the batch commits and which the new manifest carries (FR9). Seal both with `BlobHeader { kind: Thread, tier: 0, segment: fresh, predecessor: None }`.
9. In the pass's batch: `hold_blob` the new blob (Task 8's staging helper), `BlobHold::release`, `BlobOp::Link` to principal document 0; `clear` the `Document` link of every old thread blob; the manifest's `thread_blobs = vec![new_entry]`; the pass's own thread blob is not written separately (its records, aliases, tombstones and `previous_row` are inputs of the compaction, so the input set is the loaded part plus the whole staging), and after the commit the swap puts `compacted.part` in place of `index.thread`, `[segment]` in place of `thread_segments`, recomputes `thread_bytes` and sets `thread_charge` to it through `grow` (always admitted for a thread part); a content compaction's swap likewise calls `admit_content_growth` after replacing the part, which sets `content.bytes` and the charge to the new chain's loaded size or drops the part when the budget refuses (Codex re-review PL11; Task 6's atomic admission). The pass's publication rules are unchanged: a compaction does not publish by itself.

**Content compaction** (5.5, R32, G17, H7): every content blob's header carries its `tier`; a pass's own blob is tier 0. After staging the pass's blob, with the chain's tiers listed young first (the new blob first): while the first `segments` entries are all of one tier `t`, merge them into one blob of tier `t + 1` whose predecessor is the entry after the run (or `None`), and continue with the new list (`tier_merge_plan` gives the steps; a cascade within one pass is expected). A merge reads each input blob (from the loaded content part when loaded, else Task 6's `fetch_verified` against the node's hash + `parse_content_blob`; a `Loss(Hash)` there is a content loss, Task 14), concatenates the record frames (per-record frames, no recompression), drops the records of documents the thread part holds as deleted or does not hold, and writes one blob. When no cursor is pending (`cursor`, `content_to.cursor` and `reconciliation` all `None`) and the chain holds `>= segments` blobs, the full merge replaces the tiered step: every node into one blob with `predecessor: None` and `tier = max input tier + 1`, on a node whose content part is loaded or loadable under the cap; otherwise (over the cap) nothing is merged and nothing is extended (4.2). The manifest's `content = ContentHead { hash, segment, length: new chain length, size: new chain's sealed size }`; the replaced blobs are unlinked in the batch. Bounds, tested: the chain never exceeds `(segments − 1) · ⌈log_segments n⌉ + 1` nodes over a rebuild of `n` passes, and the bytes rewritten over the rebuild are at most `⌈log_segments n⌉` times the content written.

- [ ] **Step 1: Unit tests of `ThreadPart::compact`** (`crates/store/src/za_search/thread.rs`, `mod compact_tests`; fixtures are built with the public constructors of Task 3, publication revisions `r1 < r2 < r3` with rows at change ids `c1 < c2 < c3`):
  - `r35_three_roots_one_bridge`: roots `a`, `b`, `c` (threads `A`, `B`, `C`, each assigned at `r0`, row `c0`), deleted at `d_a, d_b, d_c` with `c0 < d_* < c1`; bridge `m` indexed at `r1` referencing all three: `assign` gives `thread_id = A`, aliases `B → A (r1)`, `C → A (r1)` (ties lowest); `compact(bound)` with `d_* < bound <= c1` → `n_records == 1` (`m`), `part.aliases.is_empty()`, `thread_of(m, c1) == A`.
  - `r35_128_families_of_fifteen_roots`: 128 × (15 roots deleted before the bound, one live bridge at `r1`) → `n_records == 128`, aliases `0`, `dropped_aliases == 128 · 14`, and `compaction_allowance(128) <= 65536 − 4` (the hidden section fits one 64 KiB class), `encode_hidden(..).len() + 4 <= compaction_allowance(128)`.
  - `chain_keeps_the_aliases_surviving_records_use`: threads `T1..T5` as in spec 9.1's chain (bridge `b1` merges `T2 → T1` at `r1`; `b2` with two references into `T3` merges `T1 → T3` at `r2`; `b3` merges `T3 → T5` at `r3`); roots `m1` (of `T1`) and `m3`, `m3'` (of `T3`) deleted before the bound, `c1 < bound <= c2`: after `compact`, `m2`'s assignment is `(T1, r1)` (one hop), aliases `T1 → T3` and `T3 → T5` are kept (their sources have `m2`/`b1` and `b2` assigned), `m1`, `m3`, `m3'` are pruned; for every `X in {c2, c2 + 1, c3, c3 + 1}` and every retained `D`, `compacted.thread_of(D, X) == original.thread_of(D, X)` and `compacted.members_of(T, X) == original.members_of(T, X)` for every `T` named by a retained record.
  - `fold_is_one_hop`: the same fixture, assert `m2.thread_id == T1`, not `T3` or `T5`; `thread_of(m2, X)` for `c1 <= X < c2` is `T1`, for `c2 <= X < c3` is `T3`, for `X >= c3` is `T5`.
  - `two_expired_hops_fold_in_order`: `A → B (r1)`, `B → C (r2)`, both below the bound: a record at `A` ends at `(C, r2)`.
  - `bridge_rule_keeps_a_deleted_bridge`: a bridge deleted at `d` with `d < bound <= row(r_bridge)` is retained, its aliases kept while their sources' records (the bridge itself, assigned to the target) remain; the same bridge with `row(r_bridge) < bound` is pruned and its aliases drop.
  - `equal_counts_equal_class`: four fixtures with equal `n_records` and tombstone counts (no merge; everything in one thread; a chain through one survivor; bridges deleted after their publication) give equal `padded_len(compaction_allowance(n), 65536)` and encode within the allowance.
  - `floor_row_defines_revision_at`: rows `(1, 10), (2, 20), (3, 30)`, bound `25` → rows `(2, 20)` (floor) and `(3, 30)` kept; `revision_at(27) == 2`.
  - `dead_alias_rule_needs_the_bridge_rule` (the two mutation controls, written as positive assertions): with the bridge rule skipped (a test-only flag on a copy of the procedure), the chain fixture loses `T1 → T3`'s witness and `thread_of(m2, c2)` changes; with the dead-alias rule skipped, `r35_128_families` keeps 1,792 aliases and `encode_hidden(..).len() + 4 > compaction_allowance(128)`.
- [ ] **Step 2: Run to see them fail**: `cargo test -p store --lib za_search::thread::compact_tests` → FAIL (`compact` unresolved).
- [ ] **Step 3: Implement `ThreadPart::compact`** per the procedure's steps 1–7 (the bound is a parameter; the caller reads the logs). Signature as in Produces; `floor_row` returned for the public section.
- [ ] **Step 4: Run them**: `cargo test -p store --lib za_search::thread::compact_tests` → PASS.
- [ ] **Step 5: Unit tests of the content plan** (`compact.rs`): `tier_merge_plan_cascades` (`segments = 4`, tiers young first `[0, 0, 0, 0, 1, 1, 1, 2]` → steps `[{4, 1}, {4, 2}]`: the first merge makes four tier-1 blobs, the second leaves two tier-2 blobs, fewer than `segments`, so the cascade stops); `tier_merge_plan_stops_at_a_mixed_run` (`[0, 0, 1]` → `[]`); `chain_bound_holds` (simulate `n = 4096` passes at `segments = 16`, the chain length never above `(16 − 1) · 3 + 1 = 46`, and the bytes rewritten `<= 3 ×` the content written, with each pass's blob 1 byte); `merge_drops_tombstoned_content` (records for `{1, 2, 3}`, thread part with `2` deleted → the merged blob holds `{1, 3}`).
- [ ] **Step 6: Implement `compact.rs`**: `za_search_compact_threads` (steps 8–9) and `za_search_compact_content` (the tiered and full merges), and the pass's step 4 calls in `pass.rs` (`thread_blobs.len() + 1 >= segments` → compact threads; the content plan after staging the content blob; the full merge when no cursor is pending and `content.length + 1 >= segments`).
- [ ] **Step 7: Run**: `cargo test -p email --lib --features test_mode,enterprise za::search::compact` → PASS.
- [ ] **Step 8: Suite cases** (`mail::index`, account `keymail61`; `mail::threads` reuses `keymail52`'s conversation from Task 12 where noted; `ZA_SEARCH_SEGMENTS` is lowered for the module through `search_config_guard!(segments: 4)` (plan call D3)):
  - `case_r8_thread_compaction_bounds_the_manifest`: on a node that never loaded content (no content request in the case), run 12 publishing passes (12 deliveries, each followed by one keyed `Mailbox/get`): `read_manifest(..).thread_blobs.len()` never exceeds 3 and drops to 1 at passes 4, 7 and 10 (`len + 1 >= 4` fires on the pass that would make a fourth blob), while `content.length` grows to 3 and the same passes' full merges leave 1 (no cursor is pending; the merged blob's tier, read from its header with `get_blob(hash, 0..BlobHeader::LEN)`, is the inputs' maximum plus one).
  - `case_content_compaction_leaves_one_blob`: with no cursor pending, after `segments` passes the chain is one blob with `predecessor == None`; the old blobs' `Document` links are gone (`principal_blob_links(test, id)` names the manifest's blobs only) and after `expire_and_purge_blobs` the old hashes answer `None` from `get_blob`.
  - `case_g17_h7_r32_tiered_merges_during_a_rebuild`: 200 messages, force a loss, `search_config_guard!(pass_cap: 8)` → 25 passes; after each keyed request record `content.length` and, from the `ZA_SEARCH_PASS` traces, the content bytes written and the bytes merged; assert `content.length <= (4 − 1) · ⌈log_4 25⌉ + 1 == 10` at every step, merged bytes `<= 3 × written` at the end, and after the cursor is exhausted, the first pass with content to stage and `content.length + 1 >= 4` leaves `content.length == 1`.
  - `case_r3_deleted_merged_message_after_compaction` (`mail::threads`): a thread of two, a reply bridging it to another (a merge at `r`), record `s` and the mapped id `T|D` of a member; delete `D` (keyed); run passes until a thread compaction (deliveries + keyed requests), `simulate_restart`; `Email/changes` since `s` lists `destroyed T|D` (the retained tombstone and the bridge rule kept the record, and the alias chain resolves `T` at `s`).
  - `case_r17_tombstone_gone_after_retention`: `D` deleted with the pass's log read parked by `SEARCH_SWEEP_PAUSE` (Task 8's hook; the deletion commits between the cache comparison and the log iteration), the tombstone persisted (`read_manifest` + a load shows `D` deleted); churn 20 flag changes and `lower_changes_history(.., Some(8))` + purge so both logs' bounds pass `D`'s deletion and its assignment's publication; a thread compaction; a fresh load shows no record for `D` and the content blob after the next content merge holds no record for `D`.
  - `case_initial_continuation_folded_alias` (P1-PAGING, `mail::threads`): obtain an initial intermediate page at E with a member mapped to T3; publish T3→T1 after E, advance retention past that publication, compact and reload. The old continuation refuses rather than returning a changed prefix; a fresh Initial succeeds. Assert the real fold occurred, not merely a lowered setting.
  - `case_r21_amplified_merge_probe_class` (`mail::threads`): two threads of 40 helper messages each referencing two known Message-IDs, then a bridge referencing both: the publishing pass's thread blob has hidden class exactly `65536` (its sealed length minus the public section's `t · tombstones + rows` is `65536 + the seal's overhead`), the alias table gained exactly one entry (`za_search_thread_part(test, id, password).aliases().count() == 1`; the helper is this task's: `pub async fn za_search_thread_part(test, account_id, password) -> ThreadPart` in `tests/src/utils/za.rs` loads the account's thread blobs with the unlocked keys through `parse_thread_blob`), and the same delivery sequence with the two threads already one thread (the bridge delivered first) gives the same class.
- [ ] **Step 9: Run the suite**: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture` → PASS; `cargo fmt -p store -p email -p tests -- --check` clean.
- [ ] **Step 10: Commit**: `git add crates/store/src/za_search/thread.rs crates/email/src/message/za/search/ tests/src/za/mail/index.rs tests/src/za/mail/threads.rs tests/src/za/leak.rs` and `git commit -m "Search: thread compaction with the bridge and dead-alias rules, tiered content compaction (spec 5.5)"`.

### Task 16: Account lifecycle and the operator paths

**Execution split:** Task 16a (Sol high, Astra xhigh review) runs after Task 8
and before Task 9: the destroy primitive, Task 14's `za_principal_blob_links`
prerequisite, applicable service/guard cases, and removal of the temporary
Task 8 fixture unlink. Task 16b (Sol medium, Astra xhigh review) runs after
Task 15: reset/recode call sites and recovery-dependent integration. The public
`clear_field` branch receives the guards below when implemented in 16a; it is
not left unsafe until its later call site. Task 16 is complete only after both
parts pass. Reuse completed helpers and fixtures rather than implementing twice.

**Files:**
- Create: `crates/common/src/za/search/destroy.rs`
- Modify: `crates/common/src/za/search/mod.rs` (`pub mod destroy;`), `crates/services/src/task_manager/tasks/destroy_account.rs:214` (one line before `destroy_account_blobs(server, account_id).await?;`), `crates/services/src/index_queue/reindex.rs:102-150` (one gated call after the Email loop of `reindex_account`), `crates/store/src/write/blob.rs:333-338` (`user_blob_hashes`: `Collection::Principal` joins the `matches!`), `tests/src/za/mail/index.rs`, `tests/src/za/index_task.rs` (one assertion beside `test_blob_recode_keeps_sealed`, `:636`)
- Test: `crates/common/src/za/search/destroy.rs` (unit), `tests/src/za/mail/index.rs`, `tests/src/za/index_task.rs`

**Interfaces:**
- Consumes: `ManifestValue::{decode_unverified, encode_destroying}`, `Inventory { thread_blobs, content }`, `BlobHeader::{LEN, decode}` (Task 5); `Store::za_principal_blob_links` (Task 14); `Server::za_is_key_account(account_id) -> trc::Result<bool>` (`crates/common/src/auth/vault.rs:454`); `AssertValue`, `BatchBuilder::{assert_value, set, clear}`, `BatchBuilder::is_large_bulk_batch` (as `clear_links`, `destroy_account.rs:394-419`); `Server::get_blob`; `SEARCH_DESTROY_PAUSE` (Task 2); the loader's `NoMap(Destroying)` (Task 14); the suite's `destroy_key_accounts` + `assert_is_empty` (`tests/src/za/mod.rs:250-262`), `expire_and_purge_blobs` (`tests/src/utils/za.rs:1028`), `create_blob_codec_key` and `recode_blobs` (`tests/src/za/leak.rs:1977, 1990`), `Task::AccountMaintenance { maintenance_type: TaskAccountMaintenanceType::Reindex }` through `admin.registry_create_object` and `test.wait_for_tasks()` (`tests/src/za/index_task.rs:467-474`).
- Produces: `Server::za_search_destroy(&self, account_id: u32, clear_field: bool) -> trc::Result<()>` (`clear_field = false` from destroy, where the range delete removes the marker; `true` from the reindex reset); `Server::za_search_reset(&self, account_id: u32) -> trc::Result<()>` = `za_search_destroy(account_id, true)` for a key account, nothing for an ordinary one; test helper `write_manifest_bytes(test, account_id, bytes)`.

**The destroy step** (spec 5.7, invariant 55), `za_search_destroy`:

1. Read field 152 raw (`ValueKey::property(account_id, Collection::Principal, 0, PrincipalField::ZeroAccessSearch)`, as `vault_key` at `vault.rs:60-66` reads field 150). Four cases: `None` → `previous = AssertValue::None`, `inventory = None`; bytes that `decode_unverified` to `Index(m)` → `previous = Hash(xxh3_64(bytes))`, `inventory = Some(Inventory { thread_blobs: m.thread_blobs, content: m.content })`; bytes that decode to `Destroying(inv)` → a previous attempt died after its marker: skip step 2 and continue with `inv`, taking the scan when that inventory is empty because it may represent an earlier missing/garbage manifest; bytes that do not decode → `previous = Hash(xxh3_64(bytes))`, `inventory = None` (the scan).
2. Select this attempt's fence. In false/permanent mode with absent field 152, read field 150 as raw bytes (decode failure is not absence). If both are absent, write no marker, use `inventory = None`, and carry the fixed assertion pair `(field150 None, field152 None)` into step 5. Otherwise write the marker in one Principal/document-0 batch: true/reset mode first asserts the live Account registry Item (`AssertValue::Some`, step 6); false mode with absent field 152 and present field 150 first asserts the exact raw vault hash. Then assert `ZeroAccessSearch == previous`, set the inventory-carrying marker, and write. Assertion loss re-reads from step 1 under `ZA_PIN_WRITE_ATTEMPTS`; a peer's Property range delete cannot be crossed using a removed vault hash. Existing markers resume unchanged. For this path carry the captured marker hash to step 5. The marker fences old-manifest passes and absent-manifest rebuilds and loads as `NoMap(Destroying)`. The double-absence exception retains exact orphan cleanup without allowing a stale false worker to recreate field 152 after its peer finished and removed the task; it is not a general proof that stale vault issuance cannot recreate field 150 (spec 5.7).
3. `SEARCH_DESTROY_PAUSE` pause point (test mode).
4. Collect the hashes to unlink: when `inventory` is `Some`, its thread blobs plus the chain walked from `content.hash` through each node's header (`get_blob(hash, 0..BlobHeader::LEN)`, `BlobHeader::decode`, follow `predecessor`); if any node is absent or fails to decode, or when `inventory` is `None` or a resumed marker has an empty inventory, the set is `za_principal_blob_links(account_id)` instead (R7, F23).
5. Every nonempty unlink batch first asserts the fence captured in step 2: the marker hash normally, or BOTH field-150 and field-152 absence for false-mode double absence. True/reset mode additionally asserts the live Account registry Item. Keep these assertions fixed for this attempt. Emit no batch for zero links, including no assertion-only batch. Any failed assertion stops this attempt; never refresh assertions and continue an old scan. Clear each `BlobOp::Link { hash, to: BlobLink::Document }` under Principal/document 0, flushing at `is_large_bulk_batch()` as in `clear_links`.
6. When `clear_field`: one more batch asserting both `Hash(xxh3_64(marker_bytes))` on field 152 and `AssertValue::Some` for `ValueClass::Registry(RegistryClass::Item { object_id: ObjectType::Account.to_id(), item_id: u64::from(account_id) })`, before any set/clear. Use the existing `rebuild_shares.rs` Account assertion pattern; a cached gate or field-150 assertion is insufficient. Propagate assertion failure and leave the marker; do not clear after discovering the Account Item is absent. This batch clears field 152 (`clear(ZeroAccessSearch)`), so the next keyed request finds no manifest, which is loss on a key account (H11) and rebuilds (I6). Destroy leaves the marker for `danger_destroy_account`'s Property range delete (`crates/store/src/dispatch/store.rs:450-468`).

The call sites: `destroy_account.rs:214` gains `server.za_search_destroy(account_id, false).await?;` as the line before `destroy_account_blobs(server, account_id).await?;` (every account: missing manifests still scan exact Principal/document-0 links; use the raw-vault-guarded marker when a vault remains, or guarded double-absence chunks with no marker otherwise). `reindex.rs`, after the Email loop (`:138-150`) and before the groupware loops: `server.za_search_reset(account_id).await?;` (gated inside on `za_is_key_account`). `user_blob_hashes` (`blob.rs:333-338`): `Collection::Email | Collection::SieveScript | Collection::FileNode | Collection::Principal`, so `BlobRecode` (`crates/services/src/task_manager/tasks/blob_recode.rs:93-100`) recodes the index's blobs; the loader reads through `get_blob`, which undoes the codec, so a recoded blob opens unchanged.

- [ ] **Step 1: Unit tests** (`destroy.rs`, `#[cfg(test)]`, pure parts): `marker_bytes_round_trip_the_inventory` (two thread entries and a head → `decode_unverified` gives `Destroying(inv)` equal); `unlink_set_takes_the_inventory_when_the_chain_is_complete` (a fake header reader closure: head → node 2 → node 1 → `None` gives 3 content hashes + the thread hashes); `unlink_set_falls_back_to_the_scan_on_a_broken_chain` (node 2 absent → the scan's result is used, a closure returning a sentinel set); `garbage_bytes_take_the_scan_and_assert_their_hash` (`previous == Hash(xxh3_64(garbage))`).
- [ ] **Step 2: Run to see them fail**: `cargo test -p common za::search::destroy` → FAIL.
- [ ] **Step 3: Implement `destroy.rs`**, the two call-site lines and the `user_blob_hashes` edit.
- [ ] **Step 4: Run**: `cargo test -p common --lib --features test_mode za::search::destroy` → PASS; `cargo test -p services --lib --features test_mode,enterprise za` → PASS; `cargo test -p store --lib write::blob` → PASS (the existing `user_blob_hashes` tests, if any, still pass; a new one asserts a `Collection::Principal` document link is returned).
- [ ] **Step 5: Suite cases** (`mail::index`; accounts `search-destroy-1` to `-6`, created with `create_key_user_account` at the start of each case and destroyed by it; `keymail62` for the recode case, `keymail63` for the reindex case). After each destroy: `test.wait_for_tasks()`, `expire_and_purge_blobs(test, &admin)`, then `principal_blob_links(test, id).is_empty()`, `read_manifest(test, id).is_none()`, and every hash the case recorded answers `None` from `get_blob`.
  - `case_r7_interrupted_transitions_leave_nothing` (`search-destroy-1`): deliver 5; for each of the two pause points around the batch (`SEARCH_COMMIT_PAUSE` before `commit_batch`, `SEARCH_SWAP_PAUSE` after it), park a keyed request's pass, record the staged hashes (`principal_blob_links` plus the temporary links read as the purge reads them), release with `SEARCH_HOLD_EXPIRE` set so the holds expire at once, `simulate_restart`; then destroy, purge, assert nothing remains of any recorded hash.
  - `case_r7_middle_node_break` (`search-destroy-2`): 6 passes with `search_config_guard!(segments: 8)` (no merge yet), delete the chain's third-youngest content blob from the blob store; `simulate_restart`; a text search triggers the content-only rebuild (Task 14) whose unlink set took the scan (the trace `ZA_SEARCH_REBUILD` carries reason `"scan"`); then destroy and purge: nothing remains, including the blobs the scan found that the readable part of the chain did not name.
  - `case_f23_unparseable_manifest` (`search-destroy-3`): deliver 3, keyed request, overwrite field 152 with 40 random bytes (`write_manifest_bytes`); destroy: the step wrote a marker asserting the garbage's hash (observed through `SEARCH_DESTROY_PAUSE`: at the pause `read_manifest` is `Destroying` with an empty inventory), took the scan, and after purge nothing remains.
  - `case_f23_pre_release_account_without_a_manifest` (`search-destroy-4`): a key account provisioned, field 152 cleared directly, never a keyed mail request; destroy takes the `None` branch (the marker at the pause has an empty inventory), finds no link, leaves nothing.
  - `case_r19_paused_pass_loses_to_the_marker` (`search-destroy-5`): park a keyed request's pass after staging (`SEARCH_COMMIT_PAUSE`, before the batch); destroy the account on the admin's connection with `SEARCH_DESTROY_PAUSE` set; release the pass first: its batch fails its assertion (`ZA_SEARCH_LOST` traced, the pass returns `Lost`), its holds expire; release the destroy; purge; nothing remains and no `Document` link of the pass's blobs ever existed (`principal_blob_links` taken at the destroy pause named only the manifest's).
  - `case_g12_destroy_dies_after_the_marker` (`search-destroy-6`): destroy with `SEARCH_DESTROY_PAUSE`; at the pause, `read_manifest == Destroying(inv)` with `inv` naming every blob; abort the task's first run by making the step return a store error after the pause (`SEARCH_DESTROY_FAIL_ONCE`, Task 2's one-shot, taken right after the pause point; the task's `Outcome` on `Err` is a retry, `destroy_account.rs:54-66`); the retry reads the marker, skips the marker write, unlinks from `inv`; purge; nothing remains.
  - `case_blob_recode_recodes_the_index` (`keymail62`): deliver 3, keyed request; `let codec_key = create_blob_codec_key(&admin).await; recode_blobs(test, &admin, TaskBlobRecodeCompression::Zstd, TaskBlobRecodeEncryption::Encrypt(TaskBlobRecodeKey { key_id: codec_key })).await;` then every hash of the manifest's blobs read raw from the blob store carries the codec's encrypted trailer (`BlobFormat::of(raw).key_id == Some(..)`, `crates/store/src/write/blob_codec/mod.rs:127-150`), `simulate_restart`, and `Email/get` plus a text search still succeed (the seal opened through the codec). Add the same `BlobFormat::of` assertion for an index blob to `index_task::test_blob_recode_keeps_sealed` so the two tests pin the rule together.
  - `case_i6_reindex_resets_the_index` (`keymail63`): deliver 4, keyed request (indexed); set `SEARCH_OPEN_FAIL_ALL` for the account, deliver 3 more, run keyed requests until the three are skipped as loss (`ZA_SEARCH_SKIPPED` × 3; a text search for their words finds nothing; `Thread/get` shows them as singletons); clear the hook (the "repair"); `admin.registry_create_object(Task::AccountMaintenance { account_id: key.id(), maintenance_type: TaskAccountMaintenanceType::Reindex, status: TaskStatus::now() })` and `test.wait_for_tasks()`: `read_manifest(test, id).is_none()` and `principal_blob_links(..).is_empty()` (the reset unlinked everything; the vault record and the mail are untouched: `za_vault_record(..).is_some()`, `mail_count == 7`); the next keyed `Email/get` rebuilds (`epoch` moved), and a text search finds all seven.
- [ ] **Lifecycle fence regressions (16a and 16b):** `case_reset_after_destroy_creates_no_marker` admits the reset while live, delays it before marker read until permanent destroy has removed the Account Item and the Property range, then resumes the actual true branch: it must refuse without creating field 152 or any links; removing only the marker-write Account assertion must fail this case even while the final-clear guard remains. `case_reset_cannot_clear_after_registry_deletion` uses a genuinely empty index inventory and zero Principal/document-0 links, with mail and the vault still present. It admits a real reset while the account is live, parks after the marker, deletes the Account registry Item and parks the real destroy task after it adopts that same marker; release reset while destroy stays parked. There must be zero unlink batches (including no assertion-only batch), so the final clear is the sole post-deletion reset mutation and removing its live-Item assertion cannot be masked by an earlier guard. Reset's clear must fail, the exact marker and field 150 remain, and a real creating batch asserting absent field 152 plus the field-150 hash must fail. Release destroy and verify ordinary purge/emptiness. Its live-account control clears and preserves mail/vault; 16b adds the next keyed rebuild. `case_stale_reset_scan_keeps_new_links` exercises the production unlink chunk after its captured marker changed: a stale scan may find the new generation's links, but its chunk must fail and leave them intact; an unchanged-marker control unlinks normally. In 16b drive the full overlapping reset/rebuild sequence representing a stale worker after lease loss. `case_empty_marker_retry_scans` interrupts the garbage-manifest path after writing the empty marker, retries with orphan Principal/document-0 links, and requires all those links removed, with neighboring account/collection/document controls untouched. Each case must fail when its specific guard is removed; do not let a range-deleted marker mask the Account assertion, or a final-clear assertion mask unguarded unlink chunks.

- [ ] **Stale permanent-destroy regressions (16a, P2-DUPLICATE):** after real service destruction and task completion, call the actual false helper again and stop after it (also inject the existing after-fence failure); field 152 must remain absent. This models the stale worker whose later error cannot persist a retry after its peer removed the task row. With both fields absent, seed exact Principal/document-0 orphan links and neighboring account/collection/document controls; normal cleanup removes only exact links and creates no marker. Park the production helper before its scan/unlink, separately introduce field 150 or field 152, then require its unlink chunk to refuse and preserve the link; restore the fixture state and clean normally. Using the actual production marker batch builder and a real store, read a raw vault with no manifest, remove it, then try the marker write with that captured hash: it must fail without field 152; include the unchanged-vault successful control. An admitted setup creator holding the removed PendingSetup vault must also lose. Remove each added guard separately (M64–M67), require its discriminating failure, then restore. Preserve M60–M63 and all normal destroy cases.

- [ ] **Step 6: The suite's end** (`tests/src/za/mod.rs:230-231`): `destroy_key_accounts` already runs before `assert_is_empty`; insert `expire_and_purge_blobs(&test, &admin).await;` between them so the destroy-then-purge-then-assert order holds for the index's blobs (the vault/pin exemption of `cleanup.rs:407-418` does not cover blobs; Task 2 exempted field 152 and principal-linked blobs only while their account exists).
- [ ] **Step 7: Run**: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture` → PASS (the suite's final `assert_is_empty` included); the same with `BLOB_STORE=FileSystem` → PASS; `cargo fmt -p common -p services -p store -p tests -- --check` clean.
- [ ] **Step 8: Commit**: `git add crates/common/src/za/search/ crates/services/src/task_manager/tasks/destroy_account.rs crates/services/src/index_queue/reindex.rs crates/store/src/write/blob.rs tests/src/za/mail/index.rs tests/src/za/index_task.rs tests/src/za/mod.rs tests/src/utils/za.rs` and `git commit -m "Search: destroy fences with a marker and unlinks the index, reindex resets it, BlobRecode recodes it (spec 5.7)"`.

### Task 17: Variants that narrow

Implementer: Sol (`gpt-6.1-sol`, `medium`); reviewer: Astra (`gpt-6-astra`, `xhigh`).
Test-harness work; every expected outcome is upstream's own body, cited by file and line.

The key-mode variants of mail spec 11 assert, today, the refusals and singletons that release 3 required. With the index served they assert upstream's outcomes (spec 9.3). Rule (plan 13 R44): where a variant becomes upstream's body plus one difference, the variant says so in its doc comment and either calls upstream's public body or mirrors it line for line; the one difference for JMAP is the index wait (a keyed content request in place of `test.wait_for_tasks()`'s queue wait) and the keyless refusal on a keyless token (6.2's table); for IMAP there is none, since a key account's IMAP session always holds keys (mail spec 3.3).

**Files:**
- Modify: `tests/src/jmap/za_variants.rs` (`query` `:254-331`, `query_refusals` `:333-445`, `query_served` `:447-666`, `query_options` `:668-731`, `search_snippet` `:733-833`, `query_changes` `:835-1165`, `thread_get` `:1291-1332`, `thread_merge` `:1360-1565`; the module doc `:7-16`)
- Modify: `tests/src/imap/za_variants.rs` (delete `search` `:87-276` and `thread` `:545-649`; `fetch` `:278-465` and `store` `:467-543` lose their refused-`FROM` lines; the module doc `:7-12`)
- Modify: `tests/src/imap/mod.rs:258-275` (the `search` and `thread` dispatch)
- Modify: `tests/src/za/mail/gates.rs:333-551` (`keyless_tokens_hit_the_dispatcher_gate`)
- Modify: `tests/src/za/websocket.rs` (one new case)
- Modify: `tests/src/za/tracing.rs:562-599` (`ws_send` and `ws_recv` become `pub(super)`)
- Create (helpers): `tests/src/utils/za.rs` gains `za_index_current` and `imap_index_current`

**Interfaces:**
- Consumes: Task 9's trigger (every keyed JMAP mail method and every keyed IMAP command but IDLE, NOOP and LOGOUT runs a pass; `Email/import`, `Email/set` create and APPEND run one after their commit); Task 10's content serving and `ZA_MAIL_SEARCH_UNAVAILABLE`; Task 11's gates (`za_assert_mail_keys` on the table's methods) and `za_keyless_refusal` (`crates/common/src/za/mail.rs:28`), asserted by the existing `assert_keyless` (`za_variants.rs:121`) and `assert_refused` (`gates.rs:186`); Task 12's `Thread/get` members and `collapseThreads`; Task 13's `Email/changes` and `Email/queryChanges` pages; `za_bearer_token(server, name) -> String` (`tests/src/utils/za.rs:2081`); `ws_connect`, `ws_call`, `ws_close` (`tests/src/za/tracing.rs:600, 642, 658`).
- Produces: `pub async fn za_index_current(account: &Account)` (`tests/src/utils/za.rs`): one `Email/query` with `{"text": "za-index-current"}` over the account's keyed JMAP client; returns when the method answers a result list (the index is current, 7.4), panics with the reply when it answers `unsupportedFilter` carrying `ZA_MAIL_SEARCH_UNAVAILABLE` (the wait budget ran out) or any other error. `pub async fn imap_index_current(imap: &mut ImapConnection)`: `SEARCH TEXT za-index-current`, `OK` expected, `NO [CANNOT]` panics. Both are the suite's replacement for upstream's index-queue wait. (If Tasks 10 to 13 defined an equivalent helper first, use theirs and delete these.)

- [ ] **Step 1: The IMAP dispatch runs upstream's bodies**

In `tests/src/imap/mod.rs:258-275` the two `if key_accounts_mode()` arms for `search` and `thread` go: `search::test(&mut imap, &mut imap_check, &test).await` and `thread::test(&mut imap, &mut imap_check, &test).await` run in both modes (the `fetch`, `store`, `acl` and `antispam` arms stay). Delete `za_variants::search` and `za_variants::thread`. In `za_variants::fetch`, replace `assert_search_refused(imap, "UID SEARCH RETURN (SAVE) FROM \"nathaniel\"").await;` (`:436`) and the rebuilt `UID SEARCH RETURN (SAVE) UID 1,4,6` (`:437`) with upstream's `fetch.rs` lines for that step (the served `FROM` search saves 1, 4 and 6, the same UIDs); in `za_variants::store`, replace `assert_search_refused(imap, "SEARCH RETURN (SAVE) FROM nathaniel").await;` (`:519`) and the `SEARCH RETURN (SAVE) SEEN` beneath it with upstream's `store.rs` step. If no other difference remains in `fetch`, delete it too and run `fetch::test` in both modes (say so in the task report); `store`'s other differences (the training steps) stay. Rewrite the module doc (`:7-12`): searches, sorts and threads are served (release 7 spec 9.3); the remaining variants are sharing, copying and training.

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests imap::imap_tests -- --nocapture`
Expected: `search::test` reaches `* SEARCH 1 3 4 6` for `UID SEARCH OR FROM nathaniel SUBJECT argentina` (`tests/src/imap/search.rs:40-46`) and `thread::test` reaches `(1 2 3 4)`, `(5 6 7 8)`, `(9 10 11 12)` (`thread.rs:76-81`) on key accounts; the suite passes.

- [ ] **Step 2: The `query` variant calls upstream's `query` and `query_options`**

In `za_variants::query` (`:254-331`): keep the virtual-mailbox batch and `create_query_corpus`; replace the singleton assertion (`:290-299`, "Spec 5.4: one thread per message") with upstream's, `MAX_THREADS` distinct `threadId`s read through `Email/get` of every id (the cache's `thread_id()` column stays the singleton for a key account, 6.1, so the count is taken from the mapped ids, not from `get_cached_messages`), after `za_index_current(account)`; keep `test.wait_for_tasks()` beside it (the ids-only index documents still queue). Then `crate::jmap::mail::query::query(&client, true).await` and `crate::jmap::mail::query::query_options(&client).await` (`tests/src/jmap/mail/query.rs:117, 421`, both `pub`; `can_stem` is `true` for a key account: its content part is upstream's own stemmed term set whatever the search store, 7.5). Delete `query_refusals`, `query_served`, the variant's `query_options`, `load_corpus`, `CorpusMessage`, `email_query`, `ids` and `date` where nothing else uses them (`search_snippet` uses none; check with the compiler). Add `keyless_query_refused(test, account)`: a Bearer token (`za_bearer_token`) calling `Email/query` without a filter, `Email/query` with `{"text": "study"}` and `SearchSnippet/get` with that filter and one id each answer the keyless refusal (`assert_keyless`); the same three over the account's keyed client answer results. The doc comment: "`mail::query::test` in key mode: upstream's corpus, assertions and options, with the index made current by a keyed content request in place of the queue wait; a keyless token is refused on `Email/query` and `SearchSnippet/get` (release 7 spec 6.2)."

- [ ] **Step 3: The `search_snippet`, `thread_get` and `thread_merge` variants mirror upstream's bodies**

`search_snippet` (`:733-833`): the imports stay; `za_index_current(account)` replaces nothing (each `Email/import` ran its own pass, H20) but is called once before the table as a guard; the refusal table (`:768-806`) becomes upstream's table of `(filter, email_name, snippet_subject, snippet_preview)` rows mirrored from `tests/src/jmap/mail/search_snippet.rs:54-170`, with `can_stem = true`, asserting `snippet.subject()` and `snippet.preview()` equal to upstream's values and `preview.len() <= 255`; the bare-snippet block (`:808-825`) stays; add the keyless refusal of `SearchSnippet/get` over a Bearer token.

`thread_get` (`:1291-1332`): mirror `tests/src/jmap/mail/thread_get.rs:10-52`: five imports with `References: <1234>` share one thread; `thread_id` is the last response's `threadId` (the import's response carries the final id, 6.2); `expected_result[num − 1]` is each import's id; `Thread/get` of that thread returns `expected_result` in that order; plus `Thread/get` over a Bearer token is refused. Delete the per-import singleton assertion.

`thread_merge` (`:1360-1565`): the per-account part keeps its client selection (`:1372-1398`) and the admin's refused `Mailbox/set` (`:1399-1405`); after the imports call `za_index_current` on each key account's client and `test.wait_for_tasks()`; the assertion at `:1468-1476` becomes upstream's for every account, `assert_eq!(messages_per_thread_db, messages_per_thread)`, with the `is_key` branch removed. The multi-threaded part keeps the store-level pin `cached.emails.iter().all(|m| m.thread_id() == m.document_id())` (6.1: the stored thread id stays the singleton) and adds, after `za_index_current(account)`: the mapped ids of `Email/query` are consistent with `Thread/get` (the union of `Thread/get(prefix).email_ids()` over the distinct prefixes of the query's ids equals the query's id set), no claim on the thread count. Doc comments say which body each mirrors.

- [ ] **Step 4: The `query_changes` variant runs upstream's log without the simulated merges**

`query_changes` (`:835-1165`) mirrors `tests/src/jmap/mail/query_changes.rs:30-355` with two differences. (a) The `from` filter is served: the `test_num == 1` branch (`:1081-1093`) that counted `refused` goes, and `test_num == 1` takes upstream's assertions (`query_changes.rs:280-303`: `removed ⊆ removed_ids`, `added ⊆ type1_ids`); `assert!(refused > 0)` goes. (b) Upstream's `Move(from, to)` (`:1040-1079`) simulates a thread merge by rewriting the stored `MessageData.thread_id`, which a key account's renderer never reads (6.2, 6.3), and the fixture cannot make a real one (each message has its own subject, so none shares a subject bucket; plan call F2, PL14). In key mode `Move(from, to)` writes nothing and records `id_map.insert(to, id_map[&from])`: the message keeps its id, nothing is added to `removed_ids`, `type1_ids` or `thread_id_map`, and the later `Update(to)` acts on that message. Everything else (`Update` through the raw log write and `mark_cache_stale`, `Delete`, the five queries, the four state checks) stays upstream's. Doc comment: "`mail::query_changes::test` in key mode: upstream's log and its five queries per state; `Move` is a no-op here, since the stored thread id it rewrites is not what a key account's clients see (release 7 spec 6.3), and a real merge's rendering is `mail::threads::case_merge_rendering_parity`'s."

- [ ] **Step 5: Run the JMAP and IMAP suites in both modes** (`imap/mod.rs`'s dispatch changed for plain mode too)

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests jmap::jmap_tests -- --nocapture`
Expected: passes; the output shows `passed test# 0/1` through `2/5` for every account of `thread_merge`, and `Skipping the JMAP compliance tests in key mode.` stays (out of scope).

- [ ] **Step 6: `mail::gates` and `websocket.rs` carry 6.2's table**

In `keyless_tokens_hit_the_dispatcher_gate` (`gates.rs:333-551`): move from `keyless` (`:447-500`) to `key_needing` (`:391-437`) the entries `Mailbox/get`, `Email/query`, `Email/queryChanges`, `Email/changes`, `Thread/get`, `Thread/changes`; add to `key_needing` `Blob/lookup` (`{ "accountId", "ids": [&missing_blob], "typeNames": ["Email"] }`) and `EmailSubmission/get` (`{ "accountId", "ids": [] }`); add to `keyless` `Mailbox/query` (`{ "accountId" }`), `Mailbox/changes` (`{ "accountId", "sinceState": mailbox_state }`, the state read beforehand with keys through `Mailbox/get`), `Mailbox/set` (create then destroy a mailbox named `keyless-mailbox` in two calls), `Core/echo` (`{ "hello": true }`). `email_state`, `thread_state` and `query_state` are still read with keys before the loop. The count assertion (`:510-514`) stays `2 * key_needing.len()`. Then the push check on the WebSocket channel, before `ws_close`: send `{"@type":"WebSocketPushEnable","dataTypes":["Email"]}` with `ws_send` (no reply is defined for it, RFC 8887 §4.3.5), set `$flagged` on `email_id` over the keyed HTTP client, and `ws_recv` the next text frame within 5 s: its `@type` is `StateChange`, `changed[account]` names `Email`, and the frame's text contains neither `email_id` nor `thread_id`. `ws_send` and `ws_recv` (`tracing.rs:562, 584`) become `pub(super)`.

In `tests/src/za/websocket.rs` add `keyless_socket_after_setup(test, &account)`, called at the end of `test` once the account is set up: `ws_connect("ws1@example.com", STRONG)` (a password login yields a socket that the HTTP layer strips of keys, `crates/http/src/request.rs:383`), then one gated call (`Email/query`) answers the keyless refusal (`first_response` gives `("error", "forbidden")`-shaped arguments: assert as `gates.rs`'s `assert_refused` does, by `description == ZA_MAIL_KEYLESS` or the fixed type the dispatcher uses; copy the predicate from `assert_refused`) and one keyless call (`Mailbox/query`) answers `Mailbox/query`; then the same push-enable and state-only frame check as above after a keyed `Mailbox/set` over HTTP.

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`
Expected: `keyless_tokens_hit_the_dispatcher_gate` and the WebSocket conversion test pass; the za suite passes.

- [ ] **Step 7: Commit**

```bash
git add tests/src/jmap/za_variants.rs tests/src/imap/za_variants.rs tests/src/imap/mod.rs tests/src/za/mail/gates.rs tests/src/za/websocket.rs tests/src/za/tracing.rs tests/src/utils/za.rs
git commit -m "Release 7: the key-mode variants assert upstream's outcomes; the method table's keyless cases"
```

### Task 18: Leak coverage and the differential test

Implementer: Sol (`gpt-6.1-sol`, `medium`); reviewer: Astra (`gpt-6-astra`, `xhigh`).
The rules, the controls and the corpus are specified; the comparisons are mechanical.

**Files:**
- Modify: `tests/src/za/leak.rs` (`Scan` `:340-375`; the `BlobLink` arm `:1347-1363`; the blob decode loop `:1518-1535`; a new field-152 arm in the `Property` match `:1380-1395`; `test_mail` `:2265-2572`; `purge_blobs` `:1946` becomes `pub(super)`)
- Create: `tests/src/za/leak_search.rs` (`pub async fn test(test: &mut TestServer)`: the membership review case and the oracle attempts of 9.4), declared in `tests/src/za/mod.rs` (`pub mod leak_search;` in the module list, `:16-51`) and called between `leak::test_mail(&mut test).await;` (`mod.rs:203`) and `leak::test_imip(&mut test).await;` (`:204`)
- Create: `tests/src/za/mail/differential.rs`, declared in `tests/src/za/mail/mod.rs` and called in `tests/src/za/mod.rs` after `mail::index::test` and before `mail::traces::test` (the sheet's order `threads`, `index`, `differential`)
- Modify: `tests/src/za/mod.rs` (the two calls; `leak::purge_blobs` between `destroy_key_accounts` and the final `assert_is_empty`, `:230-232`)
- Modify: `tests/src/utils/za.rs` (helpers below)

**Interfaces:**
- Consumes: `SEARCH_FORMAT_V1`, `BlobKind` (`store::za_search`, Task 5: the blob's first byte is the format byte and its second the kind byte, `BlobHeader::encode`); `ManifestValue::decode_unverified(bytes) -> Result<ManifestValue, ManifestError>` (Task 5; the destroy path of 5.7 reads the manifest without a key); `Store::za_changes_records` (Task 7) for the log check; `read_manifest(test, account_id) -> Option<ManifestValue>` and `principal_blob_links(test, account_id) -> Vec<(BlobHash, usize)>` (hash and stored size; Task 8's helpers, `tests/src/utils/za.rs`); `za_index_current` (Task 17); `za_unlock(test, account_id, password) -> Arc<SessionKeys>` (`za.rs:408`); `Server::za_search_entry` and `LoadedIndex` (Task 6) through Task 15's helper `za_search_thread_part(test, account_id, password) -> ThreadPart`; the test-mode configuration override (`search_config_guard`, Task 9; plan call D3); `plant_plain_message` (`za.rs:888`); `BlobOp::Link { hash, to: BlobLink::Document }` under `Collection::Principal` document 0 (`crates/store/src/write/mod.rs:1296-1306`, key `hash | account_id | collection | document_id`, `key.rs:410-421`, 41 bytes).
- Produces: `Scan.index_blobs: usize` (principal-linked blobs that passed the index-blob rule), `Scan.manifests: usize` (0 or 1); `pub async fn compaction_class(test, account_id) -> usize` (the size of the single thread blob the manifest lists after a thread compaction).

- [ ] **Step 1: The scan's rules (9.4)**

In `scan` (`leak.rs:1257`):
- `BlobLink` arm: a key of 41 bytes whose suffix is `account_id (BE) | Collection::Principal as u8 | 0u32` is a principal `Document` link: push its hash into `principal_links: Vec<Vec<u8>>` beside `blob_hashes`.
- Blob loop: for every hash in `principal_links` that decodes, `scan.index_blobs += 1` when `blob.first() == Some(&SEARCH_FORMAT_V1)` and `blob.get(1)` is `BlobKind::Thread as u8` or `BlobKind::Content as u8`, else a violation `"{what}: principal-linked blob is not a sealed index blob"`; a `VISIBLE_CANARIES` hit inside such a blob is a violation (`"{what}: index blob contains {canary}"`), not a `found` entry (a keyword or mailbox name is visible in the clear elsewhere, never inside the index).
- `Property` arm, `of_account`, key length `U32_LEN + 2 + U32_LEN`, `key[U32_LEN] == Collection::Principal as u8`, `key[U32_LEN + 1] == PrincipalField::ZeroAccessSearch as u8`: `scan.manifests += 1`; `ManifestValue::decode_unverified(&value)` must be `Ok`, else `"{what}: field 152 does not parse as a manifest: {err:?}"`; a `VISIBLE_CANARIES` hit in its raw value is a violation (`"{what}: manifest contains {canary}"`). The existing `check_index_property` (`Threading`, `:481`) and `check_search` (`:531`: no term, ids-only documents) stay as the "as today" rules.

- [ ] **Step 2: `test_mail`'s assertions and controls**

After the scan assertions at `:2401-2425`: `assert_eq!(result.manifests, 1)`; `assert!(result.index_blobs >= 1, "{result:?}")` (the `Email/query` and `Email/set` at `:2286-2310` ran passes that indexed the two messages, so a thread blob exists). Before them, the log check: `let since = ` the Email log's change id read before the two JMAP calls (`email_state` as `gates.rs:252` reads it, parsed), and after them `log_records_since(test, id, SyncCollection::Email, since)` holds exactly one record of the pass's batch (the one that follows the delivery and APPEND records): one `Change::UpdateItem` whose document id is the lowest of `documents` and one container property change per mailbox of the account (the default set: INBOX, Drafts, Junk, Sent, Trash, and `mailbox-canary`, created in the same `Mailbox/set` call; count the account's mailboxes through `Mailbox/get` rather than hard-coding), nothing naming `documents[1]`; and `log_records_since(.., SyncCollection::Thread, since)` holds one container property change of that same id (R4). Negative controls, after the planted-message control (`:2439-2487`) and before the ordinary-account control: on the plain account `plainmail59@example.com` (created here beside `MAIL_PLAIN`), (a) a fresh scan shows `manifests == 0` and `index_blobs == 0`; (b) `put_blob` a plaintext blob holding `mailbody-canary` and link it with `BlobOp::Link { hash, to: BlobLink::Document }` under `Collection::Principal` document 0 in a `BatchBuilder` batch, scan: a violation ending `principal-linked blob is not a sealed index blob` and one ending `decoded blob contains mailbody-canary`; clear the link, `purge_blobs`; (c) write field 152 as the bytes `b"mailbox-canary"` with `BatchBuilder::set(PrincipalField::ZeroAccessSearch, ..)` on principal document 0, scan: `field 152 does not parse as a manifest` and `manifest contains mailbox-canary`; clear the field. The ordinary-account control's rule list (`:2520-2535`) is unchanged (an ordinary account has no index blob and no manifest: assert both counts are 0 for it).

- [ ] **Step 3: The suite's end**

In `tests/src/za/mod.rs:230-232`: `destroy_key_accounts(&test).await; leak::purge_blobs(&test).await; test.assert_is_empty().await;` (`purge_blobs` made `pub(super)`). `assert_is_empty` (Task 2 exempted field 152 and the principal links while an account exists) then finds no field 152, no principal `Document` link and no blob of any destroyed key account; the `cleanup.rs` `Leftover` rules report any.

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`
Expected: `Mail leak scan: ... ` prints and the two new counts are asserted; the suite ends clean.

- [ ] **Step 4: `leak_search::test`, the membership review case and the oracle attempts**

Accounts: `keymail65@example.com` (the target), `keymail66@example.com` (the twin whose bridge merges nothing), `keymail67@example.com` and `keymail68@example.com` (the compaction-class case and its twin), key accounts through `key_mail_account`. Messages are `message_with`/`reply_to` (`tests/src/za/mail/mod.rs`) delivered over LMTP with `deliver`; a keyed pass is one `Email/get` with `ids: []` over the account's client (Task 9: every keyed mail method runs one), a content wait `za_index_current`. Each sub-case prints its name and ends with the account's mail destroyed (`destroy_all_mailboxes`).

(a) **The membership case (R4, F1), `membership`.** On `keymail65`: thread A, three messages (`A1`, replies `A2`, `A3`), then thread B, three messages, all delivered, then one keyed pass; record `state0` (the Email change id). Deliver the bridge `X`: `reply_to(A3, ..)` with `References` naming `A3`'s and `B3`'s Message-IDs (thread name `Re: ` of A's subject; B's subject must be the same thread name, so thread B is `message_with` of the same subject with different Message-IDs). One keyed pass. On `keymail66`: the same six messages and a seventh `X'` with the same byte length and two References naming two Message-IDs that no message of the account carries (public inputs equal: seven messages of equal sizes, the same reference counts), one keyed pass. Assert, for both accounts: `log_records_since(.., Email, state0)` after the bridge's pass holds one `UpdateItem` of `X`'s document id (the only message the pass indexed, so the witness), one container property change per mailbox, nothing else of the pass; the Thread log holds one container property change of `X`'s id; the manifest's newest thread blob entry has the same `size` on both accounts; `principal_blob_links` sizes are pairwise equal; `za_search_thread_part(.., keymail65)` holds one alias and `keymail66`'s none (the hidden difference); and the plain `Thread/get` groupings differ as expected (one thread of seven on 65, two of three plus a singleton on 66).

(b) **F1: a filler of a thousand references, `f1_filler_references`.** On `keymail65`, deliver a probe `P` and a filler `F` whose `References` holds 1,000 generated Message-IDs, then one keyed pass: the newest thread blob's `size` equals the one of the account `keymail66` gets for two plain messages of the same sizes with two references (the hidden section pads to `M = 65536` either way, 4.2: `Task 5::padded_len`), and `za_search_thread_part`'s record for `F` holds exactly `K = 16` hashes.

(c) **G1: a thousand junk messages expunged beside a probe, `g1_junk_expunged`.** On `keymail65`: deliver 1,000 one-line messages (`message_with("junk", "x")`) and one probe with no keyed request between (`deliver` waits for delivery only); then `za_index_current` (about 21 passes of 48 items); then destroy the 1,000 junk ids in one keyed `Email/set` (the pass before the method finds nothing new; the next keyed request's pass tombstones them); one keyed pass; read the newest thread blob entry: its `size` is `BlobHeader::LEN + 4 + public_len(1000, 0) + SEAL_OVERHEAD` (a non-publishing blob: no hidden section and no revision row; Task 5's `public_len` and `SEAL_OVERHEAD`), asserted to the byte, no hidden section (`len = 0`, a deletion-only pass, H13).

(d) **R21: two helper cohorts around two known Message-IDs with a bridge, `r21_cohorts`.** On `keymail65`: `M1` and `M2` (two messages, distinct subjects' thread names made equal), twenty helpers each replying to `M1` and twenty to `M2`, one keyed pass per cohort of 20 (two passes; both publishing, hidden `M`); then the bridge replying to both; one keyed pass: the bridge's thread blob entry has `size` equal to the entry a single plain message of the same size produced earlier in the case (the first pass of `M1` alone), and `za_search_thread_part` holds exactly one alias more than before the bridge.

(e) **The compaction class (decision 2 (c)), `compaction_class_is_public_counts`.** With `search_config_guard!(segments: 4)` for the case (restored on drop): on `keymail67` deliver 10 messages and run a keyed pass, three times, then a fourth group of 10 that includes a bridge merging two of the earlier threads; the fourth pass compacts (its own blob makes four, Task 15's `thread_blobs.len() + 1 >= segments`) and `compaction_class(test, keymail67)` is `padded_len(compaction_allowance(40), 65536)` (Task 2's functions). Repeat on a fresh `keymail68` with the same counts and no bridge: the same class (a fresh account, since `keymail66` holds tombstoned records from (a) to (d) that would change `n_records`). Then the twin assertions that the mutation rows of Task 19 rely on: the folded or dropped aliases are not observable here, so the case asserts the class alone; `mail::threads`'s R35 cases (Task 15) carry the resolution checks.

Run: the za suite.
Expected: every sub-case passes; the run prints `leak_search: membership`, `F1`, `G1`, `R21`, `compaction class`.

- [ ] **Step 5: The differential test (9.5)**

`tests/src/za/mail/differential.rs`: `keymail64@example.com` (key) and `plainmail58@example.com` (plain control). The corpus: every message of `tests/resources/mailbox.gz` (`crate::store::deflate_test_resource("mailbox.gz")`, `MessageIterator`, as `thread_merge`'s multi-threaded part reads it) plus eighteen generated messages: four with a `text/plain` attachment part carrying a distinct word each (`attword-alpha` .. `-delta`), three with non-ASCII subjects (`Ärger im Büro`, `孫子兵法 heute`, `café résumé`), three with a body phrase (`"the quick brown fox"` and two variants that share the words in another order), and two for the `K` bound: `K1` with `References` of 20 ids whose oldest, `<k-root@za.test>`, is a message delivered earlier and whose other 19 are absent (upstream threads it with the root, the index does not: the pinned deviation of 7.5), and `K2` with 20 references of which the newest is present (both thread it); and six for the Review Focus: two without a `Date` header, one with `Date: yesterday` (unparsable), one dated `Fri, 31 Dec 9999 23:59:59 +0000` (an offset beyond `i32`, clamped, PL10), one whose body holds 1.2 MiB of distinct words (above `MAX_DOCUMENT_SIZE`, `crates/store/src/search/mod.rs:39`, so upstream truncates its term set and the key account must truncate the same way), and one with a whitespace-only `Subject` and no `Message-ID`. Each message is delivered once to both recipients in one LMTP transaction (`SmtpConnection::ingest(from, &[KEY, PLAIN], message)`), so both hold the same bytes and Message-IDs; then `za_index_current(key)` and `test.wait_for_tasks()` (the control's index queue). Identity is by `Message-ID`: `by_message_id(account) -> BTreeMap<String, Id>` from `Email/get` with `properties: ["id", "messageId"]` over every id; two result lists compare equal when their Message-ID sequences do.

JMAP forms, each run on both accounts with `collapseThreads` false and true, `sort` `receivedAt` unless the form is a sort: the filters of `za_variants::query_refusals` as they stood at this plan's base (`git show za-search-design:tests/src/jmap/za_variants.rs`, since Task 17 rewrites the function;`from`, `to`, `cc`, `bcc`, `subject`, `body`, `text`, `header` `[name, value]` and `[name]`, `hasAttachment` true and false, `sentBefore`, `sentAfter`, and the five `AND` combinations with `after`, `inMailbox`, `hasKeyword`/`NOT`, `before`, and a quoted phrase) with values taken from the corpus's first message's From, To, Subject and a body word, the attachment words, the non-ASCII subjects and the phrase; the sorts `from`, `to`, `cc`, `subject`, `sentAt`, each ascending and descending, with and without `receivedAt` as the second key; the thread-keyword sorts (`hasKeyword`, `allInThreadHaveKeyword`, `someInThreadHaveKeyword` with `$seen` after flagging the first ten messages on both accounts). For each form: equal Message-ID sequences; for at least one form per filter kind the result is non-empty (`assert!(!ids.is_empty(), "{form}: the corpus has no match")`). Threads: the partition of Message-IDs into threads by `Thread/get` over every distinct prefix is equal for every thread except `K1`'s: on the control `K1` is in `k-root`'s thread, on the key account `K1` is a singleton (assert exactly that). `SearchSnippet/get`: for every text-bearing form, on each result id, `subject` and `preview` equal by Message-ID.

IMAP forms, over two IMAP sessions on INBOX with UID→Message-ID maps from `UID FETCH 1:* (UID BODY.PEEK[HEADER.FIELDS (MESSAGE-ID)])`: every search of `mail::search`'s `imap_served_keys` list (`search.rs:322-352`) and every command of `refused_commands()` (`search.rs:413-452`) with the corpus's values (`FROM <word>`, `SUBJECT <word>`, `BODY <word>`, `TEXT <word>`, `HEADER Message-ID <id>`, `SENTBEFORE`/`SENTON`/`SENTSINCE` on the corpus's dates, `OR FROM <word> SEEN`, `KEYWORD $HasAttachment`, `UNKEYWORD $HasNoAttachment`), the sorts `SORT (SUBJECT|DATE|FROM|DISPLAYFROM|TO|DISPLAYTO|CC|REVERSE SIZE|ARRIVAL) UTF-8 ALL`, `THREAD REFERENCES UTF-8 ALL` and `THREAD REFERENCES UTF-8 FROM <word>`: equal Message-ID sequences (for THREAD, equal nested groupings after mapping UIDs, with `K1`'s pinned difference). The mixed form `UID SEARCH NOT (FROM <word> ANSWERED)` included, and two Review Focus forms: `UID SEARCH CHARSET ISO-8859-1 SUBJECT {4+}\r\ncaf\xe9` (a literal in a non-UTF-8 charset, the `é` as the single Latin-1 byte `0xE9`) and `UID SORT (REVERSE SUBJECT DATE) UTF-8 ALL` (a reversed content key before a second key). `case_g5` (mutation M41's catcher; PL15: the neighbour's own frame is a function of its own text, so the comparison subtracts it): with `search_config_guard!(segments: 64)` so no content compaction runs, four deliveries to the key account, each followed by one keyed pass, and after each the growth `Δ` of `read_manifest(..).content.size` (the sealed size of that pass's content blob): (1) `nA` alone, a message sharing the twenty's vocabulary; (2) the twenty generated messages beside a byte-identical copy of `nA`; (3) `nB` alone, a message of disjoint vocabulary; (4) the same twenty beside a copy of `nB`. With per-record frames each blob is its header and seal plus the sum of its records' frames, so `Δ2 − Δ1 == Δ4 − Δ3` (the twenty records' frames, unchanged by their neighbour, G5); one frame over the blob makes `Δ2 − Δ1 < Δ4 − Δ3`, since the shared vocabulary compresses away inside it.

Run: the za suite.
Expected: `differential: N forms compared, 0 differences beyond the pinned K deviation` printed; the module leaves both accounts empty.

- [ ] **Step 6: Commit**

```bash
git add tests/src/za/leak.rs tests/src/za/leak_search.rs tests/src/za/mail/differential.rs tests/src/za/mail/mod.rs tests/src/za/mod.rs tests/src/utils/za.rs
git commit -m "Release 7: leak rules for the sealed index, the membership and oracle cases, the differential test"
```

### Task 19: System benchmark, mutation sample, documentation, outcome note, full verification

Implementer: Sol (`gpt-6.1-sol`, `medium`); reviewer: Astra (`gpt-6-astra`, `xhigh`).
Measurements by the given method, mechanical mutation runs, and the specified documentation.

**Files:**
- Modify: `tests/src/za/search_bench.rs` (Task 1's file; `#[tokio::test(flavor = "multi_thread")] #[ignore] pub async fn system()`)
- Modify: `CLAUDE.md` (the release 7 status and the sections named in Step 3)
- Modify: `docs/README-dev.md` (a section after "Operating zero-access accounts", `:31-38`)
- Modify: `docs/zero-access/manual-checklist.md` (item 8 of "## Mail", `:65`; a new section after "## Invitations by email (release 5)")
- Modify: `docs/superpowers/plans/2026-11-07-zero-access-plan16-outcome.md` (created by Task 1 with the format benchmark's table)

**Interfaces:**
- Consumes: everything above; Task 1's corpus generator (`search_bench::corpus(seed: u64, count: usize) -> Vec<Generated>`, `Generated { raw, attachment_bytes, references }`; the no-attachment run keeps the items with `attachment_bytes == 0`) and its delivery loop; `read_manifest`, `principal_blob_links`, `za_index_current`; the trace constants `ZA_SEARCH_PASS`, `ZA_SEARCH_REBUILD` on their event with `Traced` (`tests/src/za/mail/mod.rs`, `Traced::start`); `Server::za_search_pass` through the keyed requests; `search_config_guard!`.
- Produces: the outcome note later plans read; the `CLAUDE.md` text every later session reads.

- [ ] **Step 1: The system benchmark (9.6, second half)**

`za::search_bench::system`, its own `TestServerBuilder::new("search_bench")`, the account `search-bench@example.com` (Task 1's; the two benchmarks never run in one process), run by name: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::search_bench::system -- --ignored --nocapture`. It prints one table, copied into the outcome note under "## Benchmarks", and the implementer records the machine (CPU, RAM, disk) beside it. Each row's method:

| Figure | Method |
|---|---|
| Pass rate, messages/s, without and with attachment text | Deliver 2,000 generated messages (`generate(2000, false)`, then a second run with `true`) with no keyed request; then one keyed `Email/get` (`ids: []`) every 100 ms until the account is current by 7.4's own predicate, checked without running a pass: `jmap::email::za_search::za_content_current(&loaded, cache.last_change_id)` over the account's loaded index (`test.server.inner.cache.za_search.get(id)`, read-locked as Task 8's `peek` reads it) and its message cache (`get_cached_messages`). PL16: the persisted `indexed_to` never reaches the account's current change id, because the last publishing pass's own log entries (its item update, Thread change and mailbox changes) land after it and the next pass consumes them in memory only, below the `ZA_SEARCH_CHECKPOINT` threshold; the predicate accepts that, as content requests do, and needs no further mail change; rate = messages / (sum of the request latencies that ran a pass, from `ZA_SEARCH_PASS` traces' count and the wall clock) |
| Thread compaction time | During the run above, the latency of the requests whose pass compacted (every `ZA_SEARCH_SEGMENTS`th publishing pass; the manifest's thread-blob count drops to 1), at the design size reached by the rebuild row below |
| Content compaction time, tiered and full | The same requests' latencies where the chain's tier count changed, and the full merge after the rebuild's cursor is exhausted |
| Validation read cost per request | Median and p99 latency of 500 keyed `Email/get` of one id with the index current, against the same 500 for the plain control account `search-bench-plain@example.com` holding one message; and the raw `store().get_value` of field 152 timed 500 times. On RocksDB locally; if `STORE=PostgreSQL` is available (Docker, as `directory_tests` uses it), repeat and record; else the networked figure is recorded as pending |
| Mailbox property changes per publishing pass | With 10 mailboxes, then 100 (`Mailbox/set` creates): the latency of a publishing pass's request, and `Mailbox/changes` since the state before it (count of `updated` and the response time) |
| First-search latency after a day's volume | 200 messages delivered with no keyed request, then one `Email/query` with `{"text": <a word of the last message>}`: time to the result, and the number of passes it ran (`ZA_SEARCH_PASS` traces) |
| Design-size rebuild end to end (H7, R32) | With 30,000 messages present and indexed (`generate(30000, true)`, indexed by the pass-rate method), delete field 152 with a `BatchBuilder` `clear`; a client loop issues a keyed `Email/get` every 2 s until `read_manifest` shows no cursor, while a second loop issues `Email/query` `{"text": ..}` every 5 s and counts the refusals (`unsupportedFilter` with `ZA_MAIL_SEARCH_UNAVAILABLE`) before the first success; record: wall time, pass count (expected about 625 at `C = 48`), thread compactions, tiered content compactions by tier, bytes rewritten by content compactions (the sum of content blob sizes the manifest's head went through, read after every request from `principal_blob_links`) against `⌈log_16 625⌉ = 3` times the content bytes written by the passes (R32: the ratio must not exceed 3 plus the final full merge), the chain's maximum length against `(S − 1) · ⌈log_S n⌉ + 1 = 46`, the content requests refused before the first success, and the loaded thread part's bytes against the thread blob's class (about 2×, decision 2 (c)) |

After the table, the task records in the outcome note, under "## Defaults", one line per knob of 4.4 (`ZA_SEARCH_MEM_MB` .. `ZA_SEARCH_PAD_MIN`): confirmed, or the value the figures recommend and why (a 48-item pass over 2,000 ms recommends a lower `ZA_SEARCH_PASS_CAP` or a higher `ZA_SEARCH_PASS_MS`; a thread part transient over 256 MB at 30,000 messages recommends a higher `ZA_SEARCH_THREAD_MEM_MB`). A default changes only on Jay's decision: the task stops with the recommendation and the controller puts it to him before Step 2.

- [ ] **Step 2: The mutation sample (9.7)**

Procedure, plan 15's (its outcome note, "Mutation runs (plan 15)"): against the committed branch head, one row at a time, an exact string replacement that matches once (the edit written down in the row), the named test run (the za suite stops at its first panic; a unit row runs its crate's test by name), the edited files restored with `git checkout -- <file>` and `git status --short crates/ tests/` empty before the next row; nothing committed; the first failing assertion (`file:line`, message, left and right) recorded whether or not it is the named catcher; where an earlier module fails first, a follow-up run with that assertion relaxed by a tests-only edit (applied and reverted with the mutation) until the named catcher is reached or the row is a finding. Logs `t19-mut-<id>.log` and diffs `t19-mut-<id>.diff` in the git-ignored workspace. The rows are 9.7's sites; the test names are those the owning tasks gave their cases (the sub-case named after the review tag):

| Id | Site (9.7) | Mutant | Named catcher |
|---|---|---|---|
| M1 | the AAD's segment id, kind and predecessor (Task 5 `seal_section`) | drop the header bytes from the AAD | unit: a blob sealed for another segment id, kind or predecessor opens; `mail::index` R11 chain reload |
| M2 | the assertion on the manifest write (Task 8) | `AssertValue::Hash` becomes no assertion | `mail::index` lost-assertion case (`SEARCH_COMMIT_PAUSE` with a competing write): `indexed_to` or the revision advances twice |
| M3 | the record boundary (Task 8) | the cap stops inside a record without recording `(c, p)` | `mail::index` R1 split MULTIAPPEND record: a message indexed never or twice |
| M4 | the revision row's change id filled at commit (Task 5 `ManifestFill`) | the row takes the change id read at step 0 | `mail::threads` R2 backlog case; a tag computed before the fill fails R33's reload (unit) |
| M5 | the validation read and its state check (Task 11) | skip the manifest read; separately, skip the counter check | `mail::threads` cross-node case (`SEARCH_SNAPSHOT_PAUSE`); the stale-state case (`SEARCH_CACHE_HOLD`) |
| M6 | the manifest-hash comparison (Task 6) | compare `(epoch, revision)` instead of the hash | Task 6's unit `r18_a_deletion_only_change_moves_the_hash_and_reloads` |
| M7 | the history lookup's direction (Task 3 `revision_at`) | newest row at or above `X` | `mail::threads` R5 states before/at/between/after |
| M8 | the range end of the stream (Task 13) | `now` in place of `E` | `mail::threads` R12 paging with a merge and a deletion between pages |
| M9 | the exhaustive leaf resolution (Task 10) | an unresolved leaf is dropped instead of refusing | `mail::search`: a search mixing a served and an unresolvable key widens instead of refusing |
| M10 | the prefix check on supplied ids (Task 11) | accept any prefix | `mail::threads` G20: a stale prefix is served on `Email/get` |
| M11 | the key gates of 6.2's table (Task 11) | `Mailbox/get` keyless | `mail::gates` F8: the keyless token is served |
| M12 | the witness rule (Task 8) | the witness is the lowest reassigned id | `leak_search` membership case: the log names a losing member |
| M13 | the publishing rule (Task 8) | publish only when an assignment changed | `leak_search` membership case: the twin's log differs |
| M14 | the retention of a deleted record (Task 15) | prune a deleted record at its deletion change id alone | `mail::threads` R3 deleted merged message, compaction, changes from a pre-delete state |
| M15 | the epoch's fresh change id (Task 14) | `epoch` = the change id current at the loss | `mail::index` R3: the latest pre-loss state is served |
| M16 | the cursor's precedence over the shortcut (Task 8 step 0) | the shortcut applies with a cursor pending | `mail::index` R10 rebuild of several passes over an idle mailbox |
| M17 | the chain header's segment id (Task 5) | a content blob's header omits its predecessor's segment id | unit: the chain reloads from the head with the descriptor alone (R11) |
| M18 | the staging swap after commit (Task 8) | swap before the batch | `mail::index` lost-assertion case: a mapped id in memory after a failed batch |
| M19 | the links in the manifest's batch (Task 8) | `BlobOp::Link` written before the batch | `mail::index` R7 interrupted transitions: a permanent link survives |
| M20 | the scan on an incomplete traversal (Task 16) | skip the scan when the chain breaks | `mail::index` R7 middle-node case: an older blob survives destroy and purge |
| M21 | the existence rule (Task 13) | classify by the entry that made the document a candidate | `mail::threads` R14 created-and-deleted in range |
| M22 | the unfolded overlay (Task 13) | read the range with `Store::changes` | `mail::threads` F10: the deletion behind the cap resurrects |
| M23 | the strict order (Task 8) | apply a record's deletions before its inserts | `mail::threads` R16 ordering case |
| M24 | the cursor evidence (Task 8) | drop a tombstone's fields with a cursor pending (apply a log deletion before the cursor is exhausted) | `mail::threads` R16/R20 rebuild form |
| M25 | the unconditional tombstone (Task 8) | skip a deletion the cache still shows | `mail::index::case_r17_tombstone_gone_after_retention` |
| M26 | the destroying marker (Task 16) | inventory and unlinks before the marker | `mail::index` R19 paused pass losing to destroy: a link survives |
| M27 | the error classes (Tasks 6, 7) | a store timeout is loss | `mail::index` F3: `SEARCH_BLOB_FAIL_ONCE` rebuilds |
| M28 | the empty-staging rule (Task 8) | write a blob for an update-only delta; separately, move the cursor without a checkpoint | `mail::index` F4 hundred flag changes (blob count); R22 vanished cursor |
| M29 | the record bound, the hidden budget and `M` (Tasks 4, 5, 8) | unbounded references; tombstones in the padded section; `M` under the inequality | `leak_search` F1; `leak_search` G1; unit: the configuration refusal |
| M30 | the reservation charge (Task 8) | charge actual alias bytes | Task 8's unit `equal_public_inputs_stop_at_the_same_checkpoint_whatever_the_probe_merged`; `mail::threads::case_r21_amplified_merge_probe_class` |
| M31 | the cursor order (Task 8) | newest first | Task 3's unit `r16_r20_cursor_chronological_order_threads_as_the_control`; `mail::threads::case_r16_r20_rebuild_form` |
| M32 | the intra-record checkpoint (Task 8) | whole-record stops only | `mail::index` R1 MULTIAPPEND case (a record larger than the cap never completes) |
| M33 | the skipped checkpoint (Task 8) | a skipped-only pass is a no-op | `mail::index` R26 unreadable insert reopened on every pass |
| M34 | the epoch rule (Task 13) | refuse every state below the epoch; separately, an open interval `(S, epoch)` | `mail::index::case_r27_empty_rebuild_keeps_clients`; Task 13's unit `the_epoch_refuses_only_when_the_methods_log_holds_a_record_in_s_epoch` (the boundary's own entry at the epoch) |
| M35 | the fresh/retained `Initial` exemption (Task 13) | apply retention to from=0 itself while E is still retained | `case_r28_r31_initial` rejects an unnecessary refusal after truncation below E |
| M36 | the inclusive resume (Tasks 7, 8) | exclusive resume of a partial record | unit R29; `mail::index` R1 restart between passes loses a suffix |
| M37 | the existence rule's cache step (Task 13) | the record consulted before the cache | Task 13's unit `a_lagging_record_never_decides_presence_at_a_pinned_end` |
| M38 | the reconciliation order (Task 14) | tombstones before the gap inserts; separately, gap deletions after the retained records | unit H3 fixture; unit I1 fixture |
| M39 | the survivor rule (Task 3) | most members instead of most matching candidates | unit H10 fixture |
| M40 | the manifest tag (Task 5) | accept an unverified manifest | unit H9 rewrite accepted; `mail::index` R3 bad-tag loss not detected |
| M41 | per-record compression (Task 4) | one frame over the blob | `differential`'s G5 twin: the twenty records' share of the blob, each neighbour's own blob subtracted, differs between a shared-vocabulary and a disjoint neighbour (`mail::differential::case_g5`: `Δ2 − Δ1 == Δ4 − Δ3` fails) |
| M42 | the thread blob's padding (Task 5) | unpadded; separately, rounded to a 256-byte multiple | unit R15 byte-class checks |
| M43 | the alias resolution (Tasks 3, 15) | a member rewritten at merge; separately, an alias dropped without folding | `mail::threads` R25 pinned page |
| M44 | the reconciliation's attribution (Task 14) | stamp without reading the retained records | `mail::index::case_r24_g2_retained_deletion_rendered_once` (rendered twice) |
| M45 | the shorter log's bound (Task 15) | prune by the Email log alone | `mail::threads` G4 `Thread/changes` refusal |
| M46 | the counter comparison (Task 11) | compare the item state | `mail::threads` G3 empty account refused |
| M47 | the vault assertion on creation (Task 14) | the creating batch asserts `None` alone | `mail::index` R19 second form: the manifest recreated after destroy |
| M48 | the skip of an already-held insert (Task 8) | re-index a held document | `mail::index` repeated-delta case: the record count or the blob count moves |
| M49 | the destroy step's chain walk and scan (Task 16) | unlink the head only | `mail::index` R7 destroy and purge: an older content blob survives |
| M50 | the bridge rule (Task 15) | prune a deleted record when its deletion alone is below the bound | unit: the chain fixture's resolution fails (R35) |
| M51 | the dead-alias rule (Task 15) | keep every alias above the bound | unit: R35's 128-family fit fails the class |
| M52 | one-hop folding (Task 15) | fold to the end of the chain | unit: the pre-merge rendering fails (R35) |
| M53 | the Publishing padding target (Task 5) | use `padded_len(actual_framed, M)` in place of exact M | unit: `publishing_non_power_of_two_minimum_hides_alias_count` produces unequal lengths (P1-PAD) |
| M54 | keyed mixed Set response view (Task 11) | map created IDs from the input view while returning the later mutation state | `case_set_cross_node_publication_before_response` rejects old IDs under the final state |
| M55 | acknowledged-write floor (Task 11) | omit the floor from the final view acceptance check | `case_write_response_requires_own_change_id` rejects a cache at the manifest but below the write |
| M56 | pinned initial lifecycle set (Task 13) | restore later deletes but do not subtract later inserts | `initial_pinned_end_excludes_later_inserts_and_restores_later_deletes` rejects post-E candidates |
| M57 | initial continuation retention (Task 13) | bypass all retention refusals for from=0 Intermediate: pre-read E/bound, truncated suffix, and post-read E/bound; keep epoch refusal | `case_initial_pinned_end_lifecycle` with actual lost post-E deletion and unchanged epoch refuses instead of skipping a survivor |
| M58 | initial continuation identity epoch (Tasks 13/14) | omit the E versus epoch refusal | `case_initial_continuation_epoch_loss` rejects lost historical prefixes without log truncation |
| M59 | retention racing the page read (Task 13) | omit only the final retained-bound recheck | `case_initial_continuation_retention_race` exercises from=0 and S>0 Intermediate; B_after > C prevents either bounded read from seeing the marker |
| M60 | reset vs permanent destroy (Task 16) | omit the live Account Item assertion from final clear only | `case_reset_cannot_clear_after_registry_deletion`, empty inventory/no links/zero unlink batches with marker, mail and vault still present; live reset control succeeds |
| M61 | stale reset unlink (Task 16) | omit the marker assertion from unlink chunks only | `case_stale_reset_scan_keeps_new_links`; final-clear failure cannot restore removed new links |
| M62 | empty marker retry inventory (Task 16) | treat a resumed empty inventory as complete instead of scanning | `case_empty_marker_retry_scans` leaves orphan links after interrupted garbage handling |
| M63 | admitted reset after completed destroy (Task 16) | omit the live Account Item assertion from marker writing only, keep the final-clear guard | `case_reset_after_destroy_creates_no_marker` must find field152 absent, not an orphan marker after a refused clear |
| M64 | stale permanent destroy after completion (Task 16) | double absence writes the former empty marker | repeated false helper after completed service destroy must leave field 152 absent, including its injected-error path |
| M65 | absent-manifest destroy with a read vault (Task 16) | omit the raw vault-hash assertion from marker creation | production batch attempted after removal of that vault must refuse and leave field 152 absent |
| M66 | double-absence unlink (Task 16) | omit only field-152 absence from unlink chunks | manifest appears before delayed chunk; original orphan link must remain |
| M67 | double-absence unlink (Task 16) | omit only field-150 absence from unlink chunks | vault appears before delayed chunk; original orphan link must remain |

Expected: every row killed, or recorded as a finding with the test that should have caught it; an equivalent mutant is argued in the note as plan 13's row 3 was.

- [ ] **Step 3: `CLAUDE.md`**

Text to add, in the file's voice (edit the sentences the executing session finds changed by Tasks 2 to 18; the controller adds the merge status at merge time):

- In "## Zero-access fork", "Where the code lives", after the release 5 bullet: "New at release 7 (search and threads, issue #23; spec `docs/superpowers/specs/2026-11-04-zero-access-search-design.md`, revision 23; plan `docs/superpowers/plans/2026-11-07-zero-access-16-search.md`; outcome note `docs/superpowers/plans/2026-11-07-zero-access-plan16-outcome.md`): the index model in `crates/store/src/za_search/` (`ThreadPart`, `ContentPart`, `BlobHeader`, `Manifest`, `ManifestWrite`, the padding formula `padded_len` and `compaction_allowance`, `SEARCH_FORMAT_V1`, `K = 16`), the record reader `Store::za_changes_records` beside `changes` in `crates/store/src/query/log.rs`; the sealed sections, the cache `Caches::za_search` (`ZaSearchCache`, `IndexEntry`, `LoadedIndex`), the loader, `ZaSearchConfig::from_env` and the `ZA_SEARCH_*` trace constants in `crates/common/src/za/search/`; the pass (`Server::za_search_pass`), the openers, the `K` capture `za_thread_fields_k`, the validated snapshot `ZaSnapshot`, the changes stream, the rebuild, the reconciliation, the compactions and the content wait `za_search_ensure_current` in `crates/email/src/message/za/search/`; the query resolution in `crates/jmap/src/email/za_search.rs` and `crates/imap/src/op/za_search.rs`, the changes page in `crates/jmap/src/changes/za.rs`; the search key `SessionKeys::swk()` (`crates/vault/src/session.rs`, HKDF label `za/v1/search`); the manifest field `PrincipalField::ZeroAccessSearch = 152`."
- Invariants, one bullet after the release 5 one: "Search and threads (release 7 spec section 10, 48-56): a key account's terms, positions, sort keys, threading fields, thread ids and thread history exist outside process memory only in blobs sealed under the search key, bound to the account, segment id, kind and predecessor; the manifest (field 152, in the clear, keyed-tagged) holds versions, change ids, segment ids, hashes, sizes and counts only; the stored thread id, the `Thread` container and every change-log entry stay the message's own document id; a publishing pass's thread blob pads its hidden section to the smallest class (`ZA_SEARCH_PAD_MIN`, 64 KiB) and a compacted one to a function of the public record count (decision 2 (c)), the public section being a multiple of the tombstone count; the one residual is the time cap (48); the index is loaded, built and written only inside a request holding the account's keys, on that request's task, dropped on every invalidation and never swapped (49); the pass is a deterministic function of the mail and the log in chronological order, advancing only in one batch that asserted the previous manifest (50); every content leaf is resolved from a current content part or the query is refused with `ZA_MAIL_SEARCH_UNAVAILABLE` (51); a thread id leaves the server only mapped from a map validated after the response's state was read, and a request with no loadable or validated map fails rather than emit a singleton (52); the pass's keyless writes are the manifest, the links and, for a publishing pass, one Email item update and one Thread property change of the lowest indexed id and a container property change of every mailbox (53); traces carry ids, counts and reason classes (54); destroy fences with the marker and unlinks before the range delete (55); identities are retained until their change id leaves the shorter log, and a rebuild's epoch refuses every earlier state whose method log holds a record in `(S, epoch]`, never fresh `Initial`/`Exact(0)`, while an initial continuation must retain evidence at its pinned end at or above the retained bound and identity epoch (56, P1-PAGING). The method table of release 7 spec 6.2 amends mail spec 3.3: for a key account `Email/get|query|changes|queryChanges|import|parse|copy`, `Email/set` create, `Thread/*`, `Mailbox/get`, `EmailSubmission/get|query|changes` and `Blob/lookup` require keys; a keyless session keeps `Mailbox/query|changes|set`, `Email/set` update and destroy (prefix unchecked), identities, vacation, Sieve, push and `Core/echo`."
- The Registry paragraph's environment list gains `ZA_SEARCH_MEM_MB`, `ZA_SEARCH_THREAD_MEM_MB`, `ZA_SEARCH_ACCOUNT_MAX_MB`, `ZA_SEARCH_IDLE_SECS`, `ZA_SEARCH_PASS_CAP`, `ZA_SEARCH_PASS_MB`, `ZA_SEARCH_PASS_HIDDEN_KB`, `ZA_SEARCH_PASS_MS`, `ZA_SEARCH_WAIT_SECS`, `ZA_SEARCH_SEGMENTS`, `ZA_SEARCH_CHECKPOINT`, `ZA_SEARCH_SKIP_AFTER`, `ZA_SEARCH_ABSENT_RETRY_SECS`, `ZA_SEARCH_PAD_MIN` (defaults as spec 4.4; the four boot refusals, the fourth the segments range of plan call A7; a cluster shares `ZA_SEARCH_ACCOUNT_MAX_MB`, FX5).
- The test-harness paragraph: the pauses `SEARCH_COMMIT_PAUSE`, `SEARCH_SWAP_PAUSE`, `SEARCH_SNAPSHOT_PAUSE`, `SEARCH_VIEW_PAUSE`, `SEARCH_SWEEP_PAUSE`, `SEARCH_PASS_PAUSE`, `SEARCH_DESTROY_PAUSE`, the flags `SEARCH_CACHE_HOLD`, `SEARCH_SWAP_SKIP`, `SEARCH_OPEN_FAIL_ALL`, `SEARCH_HOLD_EXPIRE`, the one-shots `SEARCH_BLOB_FAIL_ONCE`, `SEARCH_OPEN_FAIL_ONCE`, `SEARCH_MANIFEST_FAIL_ONCE`, `SEARCH_DESTROY_FAIL_ONCE`, the delay `SEARCH_BLOB_SLOW` (`common::za::test_hooks`, each with its setter), the counters `za_search_blob_reads` and `za_search_message_opens`, the configuration override `set_search_config_override` read by `Server::za_search_config()`; the helpers `run_search_pass`, `read_manifest`, `manifest_bytes`, `principal_blob_links`, `index_holds`, `simulate_restart`, `search_config_guard`, `lower_changes_history`, `purge_account`, `za_index_current`, `imap_index_current`, `za_search_thread_part`, `log_records_since`, `compaction_class` in `tests/src/utils/za.rs`.
- The za suite paragraph: "the mail modules of release 3 ... `search` (its tables now served, the keyless cases and `nested_search_refused` kept), then release 7's `threads`, `index` and `differential` (`tests/src/za/mail/{threads,index,differential}.rs`: the thread, changes and id cases of spec 9.2 by review tag; the pass, rebuild, compaction and lifecycle cases; the parity corpus of 9.5), then `traces` and `gateway`; `leak_search::test` after `leak::test_mail` (the membership case and the oracle attempts of 9.4); the benchmarks `za::search_bench::{format,system}` are `#[ignore]`d and run by name. The mail modules' next free names are `keymail69` and `plainmail60`" (replacing both occurrences of `keymail46`/`plainmail50`).
- The key-mode suites sentence: the `za_variants` `query`, `search_snippet`, `query_changes`, `thread_get` and `thread_merge` assert upstream's outcomes with `za_index_current` in place of the queue wait; IMAP's `search` and `thread` run upstream's bodies in both modes (and `fetch`, if Task 17 deleted it).
- Merge hotspots and silent dependencies: spec 11's two paragraphs in substance, as a new paragraph "Release 7 (search and threads) added merge hotspots ..." in the style of the release 5 one, listing every file of spec 11's first paragraph and every dependency of its second (the term set from `index_document` and `tokenize`; the leaf semantics from `Leaf::plan` and `SearchFilter::has_text`; the thread rules mirroring `find_thread_id` and `ThreadMerge::merge`; the change rendering mirroring `merge_index`, 1.0.2's `Email/queryChanges` and upstream's paging; `push_change`'s folding; the Mailbox state on container changes; the cache's 5 s revalidation; `commit_batch`'s broadcast; `SetValue::Serializable` resolving at commit; the one ordered change-id space of Email and Thread; document ids never reused; `Id::from_parts`, `build_id`, `email_by_jmap_id`; `message_sort_ranks` and `query_emails`' comparators; `SearchQuery::filter`'s unresolved-leaf widening; the purge's and `hold_blob`'s rules; `user_blob_hashes`'s walk; `destroy_account_blobs`'s order; the swap tier's `SwapCached` set; `sync_view` and `View::sync`), each with the spec's file and line citations re-verified on the branch.
- Status, release 3's open question 2 (`emailQuerySortOptions`) is marked closed by release 7; mail spec section 8's refusals are described as the keyless and unavailable fallbacks.

- [ ] **Step 4: `docs/README-dev.md` and the manual checklist**

README-dev, a section "## Searching key accounts" after "## Operating zero-access accounts": the index is per account, sealed under a key derived from the master key, stored as blobs linked to the principal document plus the clear manifest in field 152, built inside keyed requests and loaded into process memory while the account is in use; the knobs (the fourteen `ZA_SEARCH_*` variables with defaults and one clause each; `K = 16` is a format constant; the four boot refusals and their messages); in a cluster every node must run with the same `ZA_SEARCH_ACCOUNT_MAX_MB` (plan call E10, FX5, confirmed by Jay: the suspended flag is shared through the manifest, so nodes with different caps undo each other's suspension and every request rewrites the manifest); memory sizing (`ZA_SEARCH_MEM_MB` for content parts, `ZA_SEARCH_THREAD_MEM_MB` for thread parts, which are never refused for budget; the account cap); what the operator sees (`ZA_SEARCH_*` traces: ids, counts, reason classes); the remedies: a lost or corrupt index rebuilds on the next keyed request with every client resynced; after repairing a blob store that answered "not found", run upstream's per-account reindex task, which for a key account writes the destroying marker so the next keyed request rebuilds; the unavailable text `Search for this account is not available right now; try again shortly.`; the benchmarks' two commands and that their figures are in the plan 16 outcome note; the test commands (the za suite's `threads`, `index`, `differential`, `leak_search` modules run inside `za::za_tests`).

Manual checklist: item 8 of "## Mail" (`:65`) becomes: "Search on the server for a word in a subject and in a body, then for a sender; confirm the client shows the matching messages (release 7: the server searches the sealed index built during the session; record the first search's latency after a fresh login and whether the client ever shows `Search for this account is not available right now; try again shortly.`), and that the client's local search agrees. A search on flags, size or date still works." Add "## Search and threads (release 7)" after the release 5 section: the spec and the plan named; clients Apple Mail (macOS, iOS), Thunderbird, an Android client, and the web client over JMAP; items: 1. search by subject word, body word, sender and recipient; a phrase in quotes; a word inside a PDF attachment; a non-ASCII subject; 2. sort by sender, subject and date on the server (IMAP SORT clients: Thunderbird's server-side sort setting); 3. the conversation view groups a reply with its original, and a reply received while the client was closed appears in the thread after the next sync; 4. a second client of the same account shows the same thread; 5. delete one message of a thread, then the whole thread; 6. move a thread's message to another folder and confirm the thread view follows; 7. an import of a large mailbox (IMAP APPEND of a few thousand messages, or the client's migration tool), then the first search: record the latency and any unavailable text; 8. restart the server and search again without re-login: the index persists; 9. change the password through the account page, reconnect, search again; 10. a WebSocket-only web client stays connected for push while its calls go over HTTP. Server-side: the za suite and both key-mode suites pass against the build; the `Mail leak scan` line shows `index blobs` and `manifests` counts for the key accounts. Native status: pending, as for release 5.

- [ ] **Step 5: Full verification on the branch head**

In order, one at a time, each recorded with its result and time: `cargo fmt --all --check` (and the http form if the http crate changed); the per-task compile check of Global Constraints (shipping configuration; warning-free in every file this plan touched); `cargo check -p tests --tests`; `cargo test -p vault`; `cargo test -p store --lib --features test_mode za_search`; `cargo test -p store --lib --features test_mode za_test`; `cargo test -p common --lib za::`; `cargo test -p email --features test_mode za::`; `cargo test -p jmap --lib za` and `cargo test -p imap --lib za` where unit tests exist; `cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise`; `cargo test -p smtp --lib --bins --features test_mode`; `cargo test -p tests za::leak::tests::try_archive_short_values_are_not_archives -- --exact`; the za suite; the za suite with `BLOB_STORE=FileSystem`; `za::cache_swap::cache_swap_tests -- --exact` alone without `BLOB_STORE`; `webdav::webdav_tests` plain and with `ZA_KEY_ACCOUNTS=1`; `imap::imap_tests` and `jmap::jmap_tests` plain and key mode; `cargo test -p tests smtp` with the six known flakes rerun by name; the upstream suites (`webdav::cal_query::`, `store::store_tests`, `store::blob::blob_tests`, `store::lookup::lookup_tests`, `store::search_tests`, `telemetry::telemetry_tests`, `system::system_tests`; `automation_tests` and `directory` if Docker is up, else recorded as left to CI); `python3 -I scripts/tests/ci_steps.py` and `python3 -I scripts/tests/test_gates.py` (no gated step changed: the new modules run inside `za::za_tests`); the product build `cargo build --release -p stalwart --no-default-features --features rocks` in the background with no other cargo command alongside; the two benchmarks by name (Step 1 and Task 1, repeated on the head).
Expected: every one passes, or a known flake passes on its one rerun; the product build is warning-free in the fork's files.

- [ ] **Step 6: The outcome note**

`docs/superpowers/plans/2026-11-07-zero-access-plan16-outcome.md`, in plan 15's shape, with: **Where things stand** (the branch, every commit by task, each suite's result and time from Step 5); **What changed and why** (one paragraph per task naming the spec sections); **Benchmarks** (Task 1's format table and Step 1's system table, the machine, and the per-knob **Defaults** lines with Jay's decisions); **Spec questions / plan calls** (the plan header's list with what execution found); **Rulings** (every `Ruling:` line of the ledger); **Mutation runs (plan 16)** (Step 2's table with the observed lines, the follow-up runs, "No mutation survived" or the exceptions, and the kill checks the tasks recorded); **Other harnesses** (the key-mode suites, the CalDAV client and the fresh-store gate if they ran, as plan 15's Task 8 recorded them); **Adjusted assertions** (every existing assertion a task changed, with its reason: Task 17's variants, `mail::search`'s turned tables, `mail::gates`'s table move, `leak::test_mail`'s counts); **Accepted residuals** (spec 7.5's list verbatim in substance; section 2's time-cap channel, R30/H4; 5.8's two missing-evidence residuals; the `K` deviation; the IMAP `CHANGEDSINCE` difference; the networked-store figure if pending); **Deferred findings** (anything a review deferred, by task); **Merge hotspots** (spec 11 plus Step 3's additions); **Open questions for Jay** (the defaults recommended and not yet taken; the differential test's corpus coverage; anything Task 19 found).

- [ ] **Step 7: Commit**

```bash
git add tests/src/za/search_bench.rs CLAUDE.md docs/README-dev.md docs/zero-access/manual-checklist.md docs/superpowers/plans/2026-11-07-zero-access-plan16-outcome.md
git commit -m "Plan 16 outcome note, the system benchmark, mutation runs, the release 7 checklist, CLAUDE.md"
```
