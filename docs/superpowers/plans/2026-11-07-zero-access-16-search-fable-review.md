# Zero-access plan 16: Fable review of plan revision 2

Date: 2026-11-07. Plan commit: `584096fe5` (branch `za-search-design`). Spec: revision 19
(the same commit). Previous round: the Codex review of revision 1 (PL1 to PL19).

**Verdict: REQUEST CHANGES.** Three P1 construction defects remain (FR1, FR2, FR3). None of
them is in the code the Codex findings asked to change; FR1 and FR2 sit in the cursor and
reconciliation bookkeeping that Tasks 8 and 14 share, FR3 in the Thread methods' state
read. Every Codex finding is closed as stated, with one correction (PL11's test) that
fails on correct code and one (PL3's view) whose rule is not carried into `Email/set`.

Counts: 3 P1, 6 P2, 5 P3 (14 findings).

## Per-finding assessment of revision 2 (PL1 to PL19)

| Finding | Closed as Codex stated it? | New defect introduced? |
|---|---|---|
| PL1 (hidden section stripped) | Yes. `fetch_verified` recomputes `BlobHash::generate` over the decoded bytes before any parse, on every descriptor-led read (thread blobs, the content walk by predecessor hash, reloads, merge inputs). `get_blob` undoes the codec, so the hash of the plaintext blob is what `hold_blob` stored. `pl1_a_stripped_hidden_section_is_loss` stores the stripped bytes under the original hash and expects `Loss(Hash)`; passes on correct code. | None. |
| PL2 (staging resolution) | Yes. `current_thread` follows every held alias whatever its revision and `assign`, the thread table and `present_members` use it; `thread_of` stays state-bounded. Checked the `pl2_` fixture by hand (D staged at revision 2, N referencing D alone threads to 1; the losing member 20 resolves to 10 through the staged alias). | None. |
| PL3 (one validated view) | Yes. `za_view` acquires cache, guard and content in one loop, validates the very guard it returns, and holds no guard while a pass, reload or content load runs; `Email/query` takes the state from `view.cache`; `query_changes` drops the early view before `email_query`. Traced the three Codex interleavings and the three suite cases against the procedure: each converges. | FR8: `Email/set` is told to use a snapshot for updates and destroys and to take one "after the post-commit pass" for the create response, without saying the first must also come after the pass hook; a snapshot held across the hook deadlocks the request on `index.write()`. FR3 is adjacent: the Thread methods still read their state from the store after the cache. |
| PL4 (partial checkpoints, content cursor) | Yes. `scanned_seed` seeds one below a partial checkpoint; the swap uses `max(staging.scanned_to, scanned_seed(new))`; the shortcut refuses while `cursor`, `content_to.cursor` or `reconciliation` is set or the checkpoint is partial; a skipped insert sets `consumed`. The `a_cap_sized_unreadable_prefix` fixture was checked step by step. | None. |
| PL5 (content-only cleanup keeps thread blobs) | Yes (the scan minus `manifest.thread_blobs[..].hash`). | None. |
| PL6 (reconciliation progress) | Partly. `gap_next` and the resume from `indexed_to` close the two cases Codex named. | FR2: the resume rule `Some(indexed_to)` once `indexed_to >= first_retained` is wrong when a rebuild cursor was folded into the reconciliation (the plan's own `case_f9_truncation_during_a_pending_rebuild`). FR1 (part): the marker's clearing is not a write trigger in Task 8's decision rule. |
| PL7 (same-revision folds) | Yes. `fold_sub_bound` computes every record's walk endpoint before removing any alias. Checked the `pl7_` fixture (30 → 20 → 10 in either stored order) and the `s55_` fixture; the argument that sub-bound aliases form a prefix of every chain (revisions never decrease along a chain, because an alias targets a thread alive at its revision and a record is assigned only to a live thread) holds. | None. |
| PL8 (alias counts out of traces) | Yes (`za_search_max_item_aliases`, test mode; the suite asserts the traces carry none). | None. |
| PL9 (query fast paths) | Yes in the text: top-level `inThread` through the fork arm, `Page::count`/`count_rest`/the tail loop through the snapshot, `restrict_thread_keywords` for the owner, attachment-keyword sorts through `za_sort_comparator`. Spot-checked `query.rs:319-335` and `filter.rs:410-417`: `visible_ids` answers `None` for `MessageAccess::All`, as Codex said. | None. |
| PL10 (effective date) | Yes. `za_effective_sent_at = received_at + sent_at_offset(date, received_at)`; checked `messagedata.rs:330-337` and `cache/email.rs:1887`: the four unit expectations (including `i64::MIN → 100 + i32::MIN`) are what upstream computes. | None. |
| PL11 (charge until last holder) | Yes in design (`BudgetCharge` on `Drop`, inside the `LoadedIndex` the owned guard keeps alive). | FR5: the unit test asserts `make_room(Content, 600 KiB, 2) == false` while account 1's new, unheld 300 KiB generation is evictable, so correct code answers `true`. |
| PL12 (manifest count vs configuration) | Yes (`2..=255` refusal; `write_pass` aborts above 255). | None. |
| PL13 (positions) | Yes (kept; the figure reported). | None. |
| PL14 (Move fixture) | Yes (`Move` a no-op in key mode; rendering parity pinned by Task 13's case). | None. |
| PL15 (G5 normalisation) | Yes. `Δ2 − Δ1 == Δ4 − Δ3` isolates the twenty records' frames; the header and seal overhead cancel, the neighbour's own frame cancels since its copy is byte-identical. | None. |
| PL16 (benchmark currency) | Yes (7.4's predicate without a pass). | None. |
| PL17 (attachment control) | Yes (the `Attachment` field disabled; a term check on both documents). | None. |
| PL18 (compile gate) | Yes. The `if/else` block exits nonzero on a failed build and never prints `clean` there. | None. |
| PL19 (tokenizer target, `R`) | Yes (`AccountIndexer::index_document`; the stop uses 304). | None. |

## New findings

### FR1 — P1 — Cursor and marker completion is not a pass decision (Tasks 8 and 14)

Confidence: high. Plan lines 2107 (`plan_pass`'s decision rule and `cursor_next`), 2121
(`write_pass`'s `new.cursor` formula), 2839-2840 and 2846 (Task 14's clearing rules), 2853.

Three bookkeeping transitions must be written to the manifest, and the construction
cannot write them:

1. **The rebuild cursor never clears.** `plan_pass` sets `cursor_next = Some(last admitted
   or skipped id + 1)` and, for an empty list, `Some(cursor.next)`. `write_pass` keeps the
   cursor unless `next > c.end`. Counterexample: a rebuild with `end = 60` (the highest id at
   the snapshot); messages 49..=60 are expunged through POP3 before the second pass
   (`case_r22_vanished_cursor_checkpoints`). The second pass sees an empty list,
   `cursor_next = Some(49)`, `49 <= 60`, so the manifest keeps `Some({49, 60})`; every later
   pass repeats the same manifest-only write and the shortcut, `is_current` and every
   content request (`cursor.is_none()`) stay refused for ever. The same happens whenever
   the message with id `end` alone has vanished: the last admitted id is below `end`. Task
   14's prose ("when the list is empty the pass clears the cursor") contradicts Task 8's
   formula, and Task 8 is implemented first.
2. **The content cursor's last pass.** Task 14 says "when none is left the pass sets
   `content_to.checkpoint = indexed_to` and `cursor = None`", but Task 8's decision rule
   answers `Nothing` for a pass whose content-cursor list is empty (nothing staged, nothing
   consumed, no `cursor_next`), and a `Nothing` pass writes no manifest. If "none is left"
   is read as the pass after the last `ContentOnly` one, the content cursor never clears
   and `case_h23_content_loss_is_content_only` hangs on its text query.
3. **The reconciliation marker.** Step 5 clears the marker "when `gap_inserts` and
   `gap_deletes` are empty and `indexed_to.change_id >= cache.last_change_id`", in "the
   batch". If the last gap pass stopped at the cap, the next pass finds an empty difference
   and only the retained update-only records after `indexed_to` (its own witness and mailbox
   entries): `Nothing`, nothing written, the marker stays, and on an idle account content
   search is refused until new mail arrives.

Correction: add to `plan_pass`'s decision a `Bookkeeping` reason that forces `ManifestOnly`
whenever a pending cursor was exhausted (the list consumed without hitting the cap, or
empty), a content cursor was exhausted, or the marker's clearing condition holds; express
cursor completion as a distinct value (`cursor_next = Done` or `Some(end + 1)` when the loop
ran off the list), not by the last id. State the rule once, in Task 8, and have Task 14
cite it.

### FR2 — P1 — Reconciliation after a truncation during a pending rebuild loses inserts (Task 14)

Confidence: high. Plan lines 2841-2846 (steps 1, 2 and 4), 2866 (the case), plan call E4.

Step 2 excludes from `gap_inserts` every document whose insert the retained log shows
(`inserted_in_log`), on the ground that step 4 consumes those records. Step 4 reads "from
`indexed_to`" as soon as `indexed_to.change_id >= first_retained`. When a rebuild cursor is
folded into the reconciliation, `indexed_to` is the rebuild's (`cache.last_change_id` at the
loss), above every pending item's insert. Counterexample, the plan's own
`case_f9_truncation_during_a_pending_rebuild`: 100 inserts at change ids 1..=100, loss,
boundary at 101, one pass (48 indexed, `indexed_to = 101`), `lower_changes_history(Some(5))`
and purge: the retained log is 98..=102 and `first_retained = 98`. Documents 98, 99, 100 are
in `inserted_in_log` (not gap inserts); `retained_from = Some(indexed_to)` since
`101 >= 98`; the read from 101 never meets records 98..=100; the difference is empty, the
marker clears, and the three messages are never indexed (the case's `Email/query lists all
100` fails on the prescribed construction). The same hole opens for a partial `indexed_to`
whose record is the marker's (`resumed_record_lost`) only for its own record, which the
difference covers because the marker emptied it.

Correction: compute `inserted_in_log` and `deleted_in_log` only over the retained records
the pass will consume (change id above the resume point, or every record when
`retained_from` is `None`); equivalently, when `za_search_reconcile_start` clears a rebuild
cursor, force `retained_from = None` for that reconciliation until its retained records are
consumed once. Add a unit fixture: `indexed_to = (101, 0, true)`, `first_retained = 98`,
retained inserts 98..=100 of documents the index lacks → they are gap inserts (or are
consumed).

### FR3 — P1 — The Thread methods' state is read after the cache, so `Thread/changes` can lose a thread (Tasks 11 and 13)

Confidence: medium. Plan lines 2448 (`thread_state`), 2494, 2703 (`E = snapshot.thread_state(server)`),
2692-2693 (existence rules 2 and 3, from `snapshot.cache`), 2699.

`thread_state` reads `get_state(account_id, SyncCollection::Thread)`, the Email group's
store counter, *after* the snapshot's cache was read (I5). The stream then takes the
records of `(S, E]` from the log but existence at `E` from `snapshot.cache`, built at an
earlier counter. Counterexample: `S = 10`, cache at 10, a delivery of `D` lands at 11 between
the view's step 2 and the `thread_state` read, `E = 11`. `records` holds `InsertItem(D)`,
`present(D)` is false (the cache predates it), `later` is empty (`end_is_current`), so rule 3
says absent at `E`: nothing is emitted and `new_state = Exact(11)`. The client's next
`Thread/changes` since 11 never sees `D`'s insert; thread `D` is lost until another record
touches it. The spec's own 6.3 text wants `E` for `Thread/changes` to be a state that
"lags" or equals what the cache reflects; reading any store state after the cache breaks
the pairing the validated view exists to keep. (The same shape, deletion after `E`, emits a
`destroyed` the next page repeats, against R14.)

Correction: take `E` for `Thread/get` and `Thread/changes` from the validated view's cache
(`cache.last_change_id`, the group counter the cache was built at, which step 6 already
proves is at or past the newest row and the epoch), not from a fresh store read; delete
`thread_state`'s re-read loop. If I5's "read after the manifest" wording is kept in the
spec, note in the plan header that the view's counter check is that read.

### FR4 — P2 — `pl11_an_evicted_generation_stays_charged_until_its_last_reader_drops` fails on correct code (Task 6)

Confidence: medium. Plan line 1706.

With `mem_mb = 1` (1024 KiB), the test holds a reader on account 1's first 300 KiB
generation, invalidates, charges a new 300 KiB entry for account 1 (unheld) and asserts
`make_room(Content, 600 KiB, 2) == false`. `make_room` "evicts least-recently-used parts not
in use and not `keep`'s": account 1's new generation is neither in use nor `keep`'s, so a
correct `make_room` evicts it, reaches `300 + 600 = 900 <= 1024` and answers `true`.

Correction: hold a reader on the second generation too, or ask for `800 KiB` (so even after
evicting the new generation `300 + 800 > 1024`), and assert `content_used()` rather than the
boolean for the "still charged" claim.

### FR5 — P2 — `ContentPart::documents(&self) -> &RoaringBitmap` is consumed but never produced (Tasks 4 and 10)

Confidence: high. Plan lines 1166 (Task 4 produces `document_ids(&self) -> RoaringBitmap`),
2276 (Task 10 consumes `documents(&self) -> &RoaringBitmap`).

Different name and return type. Correction: pick one (`document_ids(&self) -> &RoaringBitmap`
kept as a field is the cheaper one for Task 10's per-query use) and state it in both tasks.

### FR6 — P2 — The absence window needs the message blob's hash, which `za_open_for_index` does not return (Tasks 7 and 8)

Confidence: high. Plan lines 1805 (the signature), 1927 (step 4 answers
`Transient(AbsentWithinWindow)` alone), 1987 and 2116 (Task 8 keys `entry.absent_since(&blob_hash, now)`
by "the message blob's hash from the stub").

`OpenFailure::Transient(AbsentWithinWindow)` carries no hash and `za_open_for_index` takes no
`IndexEntry`, so `ServerOpener` cannot apply the H6 window per blob as Task 8 prescribes; an
implementer will either re-read the stub (a second store read per absence) or key the
window by document id, which the plan does not say. Correction: `OpenFailure::Transient
(TransientClass::AbsentWithinWindow)` gains the hash (`Absent(BlobHash)` as a variant of
`OpenFailure`), or `za_open_for_index` takes `&IndexEntry` and `window_secs` and answers
`Loss(BlobAbsent)` itself.

### FR7 — P2 — `thread_members` drops a member the cache still holds (Task 11)

Confidence: medium. Plan line 2448 (`thread_members`), spec 6.2 "the existing set always
the message cache's".

`thread_members` filters `present_members(thread_id)` (present records only) by the cache,
and adds `thread_id` itself only when its record is absent. Counterexample: a view whose
cache was read before a deletion (D present) and whose index was swapped after the pass
consumed it (D tombstoned). `Email/get` returns D with `threadId T` (the record keeps its
assignment), `Thread/get T` omits D, both under the same state; the control would list D.
Correction: build members from every record (tombstoned included) whose `current_thread`
is `T`, intersected with the cache's present set, plus the cache-present documents with no
record whose own id is `T`.

### FR8 — P2 — `Email/set`'s snapshot can be held across the post-commit pass (Tasks 9 and 11)

Confidence: high (that two readings exist). Plan lines 2234 (the hook "before the updates
are processed"), 2492 (updates and destroys through `za_prefix_ok(&snapshot, id)`; the
create response "after the post-commit pass, `za_snapshot` once"), 2307 (the rule that no
pass runs while a view is held).

The update and destroy checks need a snapshot; the plan does not say when it is taken. A
reasonable executor takes one snapshot at the handler's top for every id check and reuses
it for the response, and then the pass hook at `set.rs:1116-1128` runs while an owned read
guard is held on the same task: `write_pass`'s `entry.index.write().await` never returns.
Correction: in `Email/set`, take the one snapshot right after the post-commit pass hook
(after line 1128) and use it for updates, destroys and the create response; state the same
for any handler that both writes and maps.

### FR9 — P2 — Thread compaction's row set names "the manifest's newest row" without saying which manifest (Task 15)

Confidence: medium. Plan line 2895 (step 8).

The public plaintext is "the retained rows (floor and above) except the manifest's newest
row". After a publishing pass the base manifest's newest row (the only row not yet in any
blob) must be written into the compacted blob and the new manifest's row (change id filled
at commit) must not. An executor who reads "the manifest" as the base drops the base row;
after a reload `revision_at(X)` for `X` between that row and the new one falls to the row
before, and every record assigned at the base revision resolves one revision early.
Correction: "every retained row the loaded part holds (the base manifest's newest row
included, since `load_thread_part` extended the part with it), excluding the pass's own
new row, which has no change id until the batch commits".

### FR10 — P3 — Task 6 step 5 still describes revision 1's boot refusal

Plan line 1750. The parenthesis about `utils`' `Failed` trait exiting the process and "if
Task 2 placed the refusal elsewhere" contradicts plan call A5 and Task 2 (the refusal is
read in `main`). An executor could add a second exit in `inner.rs`. Replace with "Task 2's
line, unchanged".

### FR11 — P3 — `Checkpoint`'s producing task is cited three ways

Plan lines 892 (Task 3 produces it in `mod.rs`), 1782 (Task 7: "Task 5"), 2824 (Task 14:
"Task 3"). Cite Task 3 everywhere.

### FR12 — P3 — The pass's read-guard scope in step 0 should be explicit

Plan line 2115. "read `(cursor, …, hash)` under `entry.index.read()`" is followed in the
same step by `za_search_reload_if_moved`, which takes `index.write()`. The intent (a scoped
read that ends before the reload) is clear to a careful reader; say "in a block that ends
before the manifest read" so a `let guard = …` cannot be held across the reload.

### FR13 — P3 — The per-pass clone of the thread part is an uncharged transient

Plan line 2107. `plan_pass` clones `base.thread` for every non-shortcut pass (about 10 MB at
30,000 messages). Spec 4.4 counts "transients included" against `ZA_SEARCH_THREAD_MEM_MB`.
Either charge the clone through a detached `BudgetCharge` for the pass's duration or note
the omission in the outcome note's residuals.

### FR14 — P3 — Task 1's commit adds `term.rs`, which the task says it does not change

Plan line 563. Drop `crates/store/src/search/term.rs` from the `git add`, or the task
report must explain a change the task's own text forbids.

## Method and limits

A reading review of the plan (every line), the Codex review, the revision-2 diff of plan and
spec, and the spec passages the findings depend on (4.2, 5.1 R22, 5.4, 5.5, 6.2 I5, 6.3).
Source checked where a claim rested on it: `crates/store/src/query/log.rs` (`changes`,
`Changes::deserialize`), `crates/common/src/storage/transaction.rs:225-246` (the truncation
marker overwrites the first retained record), `crates/email/src/message/messagedata.rs:330-337`
and `crates/common/src/cache/email.rs:1887` (PL10), `crates/jmap/src/email/query.rs:319-335`
and `query/filter.rs:405-420` (PL9), `crates/common/src/cache/invalidate.rs:45-56`,
`crates/email/src/cache/mod.rs:55-70`, `crates/jmap/src/thread/get.rs:78-88`,
`crates/store/src/write/log.rs:299-305` (`build_id`). The unit fixtures of Tasks 3, 5, 8
and 15 that this review cites were evaluated by hand against the plan's own rules.

Not done: no build, test, benchmark or runtime probe; no subagents; the plan, spec and
every other file are unchanged. Upstream citations the plan gives by line and that no
finding depends on were not re-verified. The model-tier assignments were not assessed.
