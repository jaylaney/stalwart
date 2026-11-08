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

## Scoped re-review of plan revision 3 (b536b87aa)

Date: 2026-11-07. Plan commit: `b536b87aa` (branch `za-search-design`), revision 3, answering
FR1 to FR14 above. Spec: revision 19, unchanged. Scope: the fourteen corrections, the
interfaces they renamed, and what the revision-3 diff touched; the Codex findings and the
whole-plan read above stand.

**Verdict: REQUEST CHANGES.** The three P1s of revision 2 are closed: FR1's cursor and marker
completion is now a pass decision that writes the manifest (traced against
`case_r22_vanished_cursor_checkpoints`, `case_h23_content_loss_is_content_only` and the
idle-account marker), and FR3's Thread state is the view's cache counter at every site. FR2 is
closed as stated, but its correction should be reverted: the state it fixes cannot arise and the
narrowed attribution re-opens a lost retained insert (FS3). One P1 remains, FS1, found while
tracing FR2's step 4: the reconciliation's resume rule at the truncation marker's own record
restarts the reconciliation on every pass, so it never finishes. It predates revision 3 (it is
revision 2's PL6 text, which revision 3 did not touch) and sits in the same paragraph FR2
changed; it is reported here because the brief asked for every finding, and the stopping rule
counts a remaining P1 whatever round introduced it.

Counts: 1 P1, 5 P2, 4 P3 (10 findings). Of these, FS1 and FS6 predate revision 3; the rest are
in, or follow from, the revision-3 text.

### Per-finding assessment of revision 3 (FR1 to FR14)

| Finding | Closed as stated? | New defect introduced? |
|---|---|---|
| FR1 (cursor and marker completion) | Yes in design. `CursorStep::{None, Advanced, Exhausted}` makes completion a fact of the loop; `bookkeeping` forces `ManifestOnly`; `write_pass` maps `Exhausted` to `cursor = None` without comparing `next` with `end`. Traced `case_r22`: after the twelve expunges the second keyed request's pass has a pending cursor and an empty list, answers `Exhausted` + `bookkeeping`, writes the manifest alone (`indexed_to` unchanged, no blob, no log entry), and the unconsumed delete records wait for the next pass, which consumes them as `ManifestOnly` (nothing held). Traced `case_h23`'s completion: the content cursor's last pass, having run off the list, sets `bookkeeping` and clears `content_to.cursor` with `content_to.checkpoint = indexed_to`, whether it staged records (`ContentOnly`) or none (`ManifestOnly`). Traced the idle-account marker: the pass after the last gap pass reads the update-only witness and mailbox records after `indexed_to`, its checkpoint reaches `cache.last_change_id`, step 5 sets `bookkeeping`, and the marker clears in a manifest-only write; a run of more than `ZA_SEARCH_CHECKPOINT` update-only records is caught by the `update_records` checkpoint. | FS2: `bookkeeping` is one bit for three independent completions and `Staging` carries no step for the content cursor or the marker. FS5: `CursorStep::None` has no production rule, and step 3's cursor block is not conditioned on a pending cursor. FS7: step 5's `indexed_to` must be named as the pass's resulting checkpoint. |
| FR2 (attribution over the consumed records) | Yes as stated: step 1 counts only the records step 4 will consume, and the unit fixture encodes it. But the state FR2 describes cannot occur, and the change regresses PL6: FS3. The plan's own `case_f9_truncation_during_a_pending_rebuild` fails on correct code for reasons independent of FR2: FS4. FS1 (pre-existing) is in the same paragraph. | FS3, FS4; FS1 adjacent. |
| FR3 (Thread state from the cache) | Yes. `ZaSnapshot::state()` is `cache.last_change_id`, which is the Email log's last synced record id (`crates/email/src/cache/mod.rs:300-307` sets it from `changes.to_change_id`, `:483` on apply), moved by container-only records too (G3), and proved at or past the newest row and the epoch by `za_view`'s step 6. `Thread/get` (`thread/get.rs:80-85`) and `Thread/changes`' `E` (Task 13 step 1) both take it; the refusal bound (step 3) and the epoch check read the logs as before, unaffected by `E`; G3's empty account answers `Thread/get` with `Exact(last_change_id)` and `Thread/changes` from `Initial` with the initial stream; `case_r27`'s `Thread/changes` since `s` sees only the boundary's container record in `(s, E]`, which is no candidate, so it is empty under `Exact(E)`. | None in construction. FS8 (P3): the spec sentence that needs D13's wording is 6.3's (and summary line 63), not 6.2 alone; `State::from(u64)` does not exist; two stale lines in Task 11; a parity note on flag-only records. |
| FR4 (PL11 test) | Yes. With both generations held nothing is evictable and `600 + 600 > 1024` → `false`; after the first reader drops, the second generation is in use and `300 + 600 <= 1024` → `true`; the counter assertions carry the claim. | None. |
| FR5 (`document_ids`) | Yes: `&RoaringBitmap` in Task 4 (line 1187), Task 10 (line 2299) and D10; Task 6's reload test compares it. | None. |
| FR6 (absence window) | Yes. `za_open_for_index(.., entry: &IndexEntry, window_secs, now)` applies `absence_class(entry.absent_since(&hash, now), now, window_secs)` on the stub's blob hash and calls `entry.seen` on success; `IndexEntry` and `absence_class` are `common`'s, which `email` imports; Task 8's `ServerOpener` passes `config.absent_retry_secs`. `plan_pass`'s `loss_of(AbsentWithinWindow) = BlobAbsent` for the attempts rule is still consistent. | None. |
| FR7 (`members`) | Yes. Task 3's second table keeps tombstoned records; `thread_members` intersects it with the cache and adds the record-less singleton; the second half of `thread_members_intersect_the_present_set` is consistent with `map_id(8) == 2|8` since `current_thread` follows a tombstoned record's assignment. | FS10 (P3): `fold_sub_bound` and compaction step 7 still say "the thread table", singular. |
| FR8 (one view per task) | Yes. `Email/set`'s `flush` at `set.rs:1117` runs unconditionally after the creates and `recheck.finish` at `:1131`, so the hook, then the one snapshot, run on every `Email/set`; updates (`:1147`), destroys and the create response follow. `Email/import` the same; IMAP `SEARCH` reuses the content view; the Task 10 paragraph names the write-preferring `RwLock` correctly. | None. |
| FR9 (compaction rows) | Yes: every retained row the input part holds, the base newest row included, the pass's own excluded. | None. |
| FR10 to FR14 | Yes: the refusal note removed; `Checkpoint` cited as Task 3's at lines 1801 and 2851; step 0's guard scoped; the clone charged; `term.rs` gone from Task 1's commit. | FS9 (P3): the charge is described inside `plan_pass`, which has no cache. |

### New findings

#### FS1 — P1 — The reconciliation's resume at the marker's own record restarts for ever (Task 14; pre-existing, adjacent to FR2)

Confidence: high on the text, medium on how often the input occurs. Plan lines 2872 (step 4's
resume rule and its last sentence), 2877 (`difference_resumes_retained_records_from_indexed_to`:
`(30, 0, true)` and `(44, 2, false)` only), 2880 (step 4's wiring). Revision 2 carried the same
text; revision 3 changed step 1 beside it.

Step 4 reads every retained record while `indexed_to.change_id < first_retained`, else from
`indexed_to`, and its last sentence restarts the reconciliation whenever a read from `indexed_to`
reports `truncated`. Counterexample: `indexed_to = (c, p, false)` (a record left part-way: a
MULTIAPPEND or import larger than the remaining cap, or a time-cap stop, G18), then no keyed
request while deliveries accumulate; the purge trims the log to `maxChangesHistory` and the marker
walks up one record per purge (`transaction.rs:196-246`: the empty value lands on the
`max_entries + 1`-th newest key), landing on `c`. The next keyed request's pass reads
inclusively from `c`, meets the empty value, reports `truncated && resumed_record_lost`, and
`za_search_reconcile_start` sets the marker with `bound = c`, `indexed_to` unchanged. The next
pass: `c < first_retained` is `c < c`, false, so `retained_from = Some((c, p, false))`; the
inclusive read from `c` meets the marker again and reports `truncated`; rule 4's last sentence
calls it a second truncation and restarts (`gap_next = 0`) and returns. Every later pass repeats
the same write; the marker never clears, `is_current` is false for ever (`reconciliation.is_some()`),
every content request answers `NotCurrent` after `ZA_SEARCH_WAIT_SECS`, and a gap insert
admitted before the restart in the same pass is either discarded or redone from `gap_next = 0`
each time. The same loop follows a genuine second truncation whose marker lands exactly on the
partial `indexed_to`'s record.

Correction: `retained_from = None` while `indexed_to.change_id <= first_retained` (the marker's
record is empty, so reading from the first retained key loses nothing, and the records above it
are all unconsumed); and detect a second truncation by the bound, not the flag: when the read
from `indexed_to` reports `truncated` with a `first_retained` above the marker's `bound`, restart
with `gap_next = 0` and the bound re-read, otherwise consume on. Add `(40, 2, false) → None` to
`difference_resumes_retained_records_from_indexed_to`, and a case (unit, over `difference` and
the resume, or a `case_pl6` variant whose `lower_changes_history` lands the marker on a partial
record) that the marker clears.

#### FS2 — P2 — `bookkeeping` is one bit for three completions, and `Staging` cannot carry the content cursor's or the marker's step (Tasks 8 and 14)

Confidence: high. Plan lines 1991 (`Staging`), 2130 (step 3: `bookkeeping` set for an exhausted
rebuild cursor), 2144 (`write_pass`: "Task 14 adds the content cursor's and the marker's
clearing, both through `bookkeeping`"), 2847 (Task 14's Files: `write.rs` absent), 2867 (the
content cursor's `next_below` and clearing), 2871 and 2873 (`gap_next` written in the pass's
manifest; the marker cleared).

`write_pass` must, on a `bookkeeping` pass, clear the rebuild cursor (it has `staging.cursor`
for that), or the content cursor (setting `content_to.checkpoint = indexed_to`), or the marker,
and on other passes write `content_to.cursor.next_below` or the marker's `gap_next`; `Staging`
has no field for any of the last four, and `write.rs` is not among Task 14's files. Two of the
three can be pending at once: a content loss during a rebuild (Task 15's merge reads an input
node with `fetch_verified`, a `Loss(Hash)` there "is a content loss, Task 14") leaves the rebuild
cursor and the content cursor set; a truncation while a content cursor is pending leaves the
marker and the content cursor set (`za_search_reconcile_start` clears only the rebuild cursor).
Counterexample: both cursors pending; the pass that exhausts the rebuild cursor's list sets
`bookkeeping`; `write_pass`, told the content cursor clears "through `bookkeeping`", sets
`content_to.cursor = None` and `content_to.checkpoint = indexed_to` with the content cursor's
items unprocessed; `za_search_content_needs_rebuild` answers `None` from then on (checkpoint equals
`indexed_to`, no cursor), so the messages indexed before the loss never regain content records and
text search silently misses them. With the marker and the content cursor pending, the content
cursor's exhaustion pass clears the marker with gap items unprocessed, and those messages are
absent from the index for good (singletons in the map, missing from every search).

Correction: give each completion its own step: `cursor: CursorStep` (rebuild), `content_cursor:
CursorStep` (`Advanced(next_below)` or `Exhausted`), `marker: MarkerStep { Untouched,
GapNext(u32), Finished }`; make `bookkeeping` the derived "any `Exhausted` or `Finished`" (or keep
the flag and set it from them); have `write_pass` apply each arm independently; add `write.rs`
to Task 14's Files and name the three arms in Task 8's step 6, where revision 3 wrote "both
through `bookkeeping`".

#### FS3 — P2 — FR2's correction fixes a state that cannot arise and re-opens a lost retained insert (Task 14)

Confidence: high. Plan lines 2869 (step 1's narrowing), 2877
(`difference_counts_only_the_records_it_will_consume`), 2892 (`case_pl6_unreadable_gap_and_retained_inserts_finish`:
"exactly two `ZA_SEARCH_SKIPPED` events"), 470 (the case-map row), spec 5.4 ("a message of the
difference whose raw insert or delete lies among them takes that record's change id": attribution
over the retained log, not a part of it).

The premise. A reconciliation starts only when the pass's read from `indexed_to` (or from
`scanned_to`, which is higher) reports `truncated` or `resumed_record_lost`, which means the
marker key is at or above the read's start; the marker's own record is empty, so every retained
record with content is above `indexed_to` at that moment. FR2's fixture (`indexed_to = 101`,
`first_retained = 98`) has the marker below `indexed_to`: no read from `indexed_to` meets it, no
reconciliation starts, and the rebuild cursor indexes the remaining messages from the cache. The
hole FR2 described is closed by construction in revision 2.

The regression. Trace `case_pl6` (cap 4): 30 indexed, 20 more delivered (ids 31..50, one log
record each), id 35 and id 48 corrupted, `lower_changes_history(Some(5))` and purge: the marker
empties the sixth newest record (id 45's insert) and the five newest, the inserts of 46..50, are
retained with content. Passes A to D index the gap inserts 31..45 (35 skipped, `SKIPPED` 1),
`gap_next = 46`. Pass E: `retained_from = None`, step 4 consumes the retained records under the
cap: 46 and 47 indexed, 48 skipped as loss (`SKIPPED` 2), 49 indexed, `indexed_to` complete at
49's record. Pass F: `retained_from = Some(indexed_to)`; under revision 3 `inserted_in_log`
counts only the records after `indexed_to` (50's insert and the passes' own update-only witness
records), so it is `{50}`; `gap_inserts = cache − index − {50} = {35, 48}` at or above
`gap_next = 46` = `{48}`; 48 is opened again and skipped (`SKIPPED` 3) before step 4 consumes
50. The case asserts exactly two, so it fails on the prescribed construction. Under revision
2's attribution (every retained record, `{46..50}` on every pass) 48 stays excluded and the case
passes. The extra open is bounded (`gap_next` then passes 48), so this is a test failure and a
spec deviation without a plan call, not a loss.

Correction: revert step 1 to attribution over every retained record (spec 5.4's text), drop
`difference_counts_only_the_records_it_will_consume` or invert it (`indexed_to = (101, 0, true)`,
`first_retained = 98`, inserts 98..100 → `gap_inserts == []`, with a comment that the state is
unreachable), rewrite the case-map FR2 row, and record in the header that FR2's premise fails
because a reconciliation starts only from a read that meets the marker.

#### FS4 — P2 — `case_f9_truncation_during_a_pending_rebuild` fails on correct code (Task 14)

Confidence: high. Plan lines 2893 (the case), 2139 (step 1 reads no log while `cursor_items` is
non-empty), 2880 (truncation is noticed only from the log read), spec 5.4 ("if the log truncates
while a rebuild cursor is pending, the difference cursor subsumes the rebuild's").

With 100 messages (change ids to `L`), the loss and one keyed request: the first batch sets
`indexed_to = L` (the cache's counter before the boundary) and the boundary takes `L + 1`; the pass
indexes 48 cursor items and writes its witness at `L + 2`. `lower_changes_history(Some(5))` and
the purge put the marker on the sixth newest key, `L − 3`, below `indexed_to`, so no read from
`indexed_to` can report `truncated`: no reconciliation ever starts in this fixture. And whatever
the retention, the next keyed request's pass has 52 cursor items, reads no log, and leaves
`cursor = Some(Advanced(..))`, so the assertion `cursor == None && reconciliation.is_some()` fails
on the prescribed construction; the truncation is noticed only after the cursor is exhausted, when
"the rebuild's remaining items" are already indexed. The outcome (`Email/query` lists all 100) is
reached either way; the intermediate assertion and the spec's "subsumes" sentence are not.

Correction, either: (i) on a pass with a pending cursor, probe `za_log_first_retained(Email)` (one
point read) and treat `first_retained > indexed_to.change_id`, or `first_retained ==
indexed_to.change_id` with a partial `indexed_to`, as the truncation that starts the
reconciliation and folds the cursor (spec 5.4's sentence as written), and set the case's retention
to `Some(1)` so the marker lands on the boundary record, above `indexed_to`; or (ii) keep the
construction and rewrite the case to run keyed requests until the cursor is exhausted before
asserting `reconciliation.is_some()`, with the retention set so the marker is above `indexed_to`
(`Some(1)`), and note the deferral against 5.4 as a plan call. (i) honours the spec; (ii) is one
read cheaper.

#### FS5 — P2 — `CursorStep::None` has no production rule, and step 3's cursor block is unconditioned (Task 8)

Confidence: medium (the unit tests would expose a literal implementation, so an executor
recovers; the text still prescribes the wrong thing). Plan lines 2130 (step 3: "the loop ends one
of two ways ... or the list was empty → `Exhausted` and `bookkeeping = true` ...; return without
reading records"), 2084-2094 (`the_decision_follows_what_changed`: the shared `base` has no
cursor, yet `cursor_only` passes `cursor_items [4, 5]`), 2114-2115 (`s_empty`).

Read literally, every pass without a pending cursor has an empty `cursor_items`, answers
`Exhausted` with `bookkeeping = true`, returns without reading records and decides
`ManifestOnly`: `update_only.decision == Nothing` fails, `case_flag_changes_write_nothing` (F4)
sees the manifest bytes change on every request, and no log record is ever consumed. The variant
`None` exists for the no-cursor case but nothing says when it is produced, and `plan_pass` cannot
tell "no cursor" from "a pending cursor whose list is empty" by `cursor_items` alone. Also: an
`Abort` returns from inside `admit` before the loop's verdict, so `cursor` must stay `None` (the
cursor unchanged in the manifest) for an aborted cursor pass; say so, since advancing it past
items whose staging was discarded would lose them for good (they are not in the log after
`indexed_to`).

Correction: "the cursor block runs only when `base.manifest.cursor.is_some()`; otherwise `cursor =
CursorStep::None`, `bookkeeping` stays false and the records are read"; "an `Abort` leaves
`cursor = CursorStep::None`"; give `cursor_only` and `s_empty` a base whose manifest carries the
cursor (`Some({next: 4, end: 5})`, `Some({next: 49, end: 60})`).

#### FS6 — P2 — `case_h23`'s first text query cannot "wait and succeed" (Tasks 9 and 14; pre-existing)

Confidence: medium (it depends on how Task 14 wires the loader's content-loss arm, which the plan
leaves to "calls `za_search_content_rebuild_start`"). Plan lines 2888 (the case: "the first
`Email/query` with a text filter waits for the content cursor and succeeds"), 2251
(`za_search_ensure_current`: `self.za_search_load_content(&entry, keys).await.map_err(map_load)?`),
1616-1618 (`absence_class`), 206 (`ZA_SEARCH_ABSENT_RETRY_SECS` 10).

After the head blob is deleted and the restart, `Email/get` loads the thread part only. The text
query's `ensure_current` finds the index current (no cursor yet) and calls
`za_search_load_content`; `load_content` fetches the head, finds it absent, and answers
`Transient(AbsentWithinWindow)` for ten seconds, then `Loss(BlobAbsent)`; `map_load` turns the
first into `Unavailable::Transient` and the second into `Rebuilding`, and the `?` returns at once.
The first query is refused in both windows; the content rebuild starts on the `Loss` path and the
following query runs the content cursor. The case as written fails on correct code.

Correction: `search_config_guard!(absent_retry_secs: 0)` in the case (as `case_pl5` does), and
either have `ensure_current` re-enter its loop once after a content-loss `Loss` (the content
rebuild started, the cursor's passes then run within the deadline, which matches "waits for the
content cursor and succeeds"), or change the expectation to "the first text query answers
`ZA_MAIL_SEARCH_UNAVAILABLE`; the second succeeds".

#### FS7 — P3 — Step 5's `indexed_to` is the pass's resulting checkpoint (Task 14)

Plan line 2873. "`indexed_to.change_id >= cache.last_change_id` as read at this pass" works only
when `indexed_to` means `staging.checkpoint` after step 4 (the update-only records after the last
publishing pass raise it to the cache's counter). An executor who compares
`base.manifest.indexed_to` never clears the marker on an idle account: the pass that reads the
witness records decides `Nothing` (the condition is false against the base), only `scanned_to`
moves, and the next pass reads nothing. Say "the pass's checkpoint after step 4
(`staging.checkpoint`)".

#### FS8 — P3 — FR3's leftovers: the spec sentence is 6.3's, `State::from(u64)`, two stale lines, a parity note (Tasks 11 and 13, D13)

Plan lines 132 (D13: "One line of 6.2 for Jay"), 2455 (Task 11's Files: "`thread/get.rs:35-106`
(state after the manifest, I5 ...)"), 2466 (Consumes: `StateManager::get_state(account_id,
SyncCollection::Thread)`, now unused by the fork), 2521 (`State::from(snapshot.state())`).

The sentence D13 contradicts is spec 6.3's ("`E` for `Thread/changes`, and `Thread/get`'s
`state`, is the Thread log's change id, which ... lags the Email state by flag-only records") and
the summary at spec line 63, beside 6.2's I5; name all three for Jay. `State` implements
`From<Option<ChangeId>>` only (`crates/jmap-proto/src/types/state.rs:33-40`), and `cache_state` is
private to `changes/state.rs`; write `State::from((s != 0).then_some(s))` or expose the helper.
Lines 2455 and 2466 still describe the store read. Parity: with `E = cache.last_change_id`, a
flag-only record lies inside `(S, E]` for `Thread/changes` at once, and the Thread stream's rule
("some at both → `Updated(T)`" over every candidate with a raw item record) renders
`Updated(T)` for it, where upstream's `Thread/changes` (the Thread log) reports nothing for a flag
change; the rule is revision 1's, D13 only makes it prompt. Harmless to a client (`Thread/get`
returns the same members) but a parity deviation to record in the outcome note, or to narrow by
counting `UpdateItem`-only candidates for the Email stream alone.

#### FS9 — P3 — FR13's charge is placed in a function without a cache (Task 8)

Plan line 2130. `plan_pass(base, cursor_items, records, limits, started, opener)` has no server
or cache; "charged ... through `cache.charge(PartKind::Thread, base.thread.loaded_bytes())`"
belongs in step 5 around the call (`let _charge = self.inner.cache.za_search.charge(PartKind::Thread,
base.thread.loaded_bytes() as u64);` before `plan_pass`, dropped after `write_pass`), or
`plan_pass` gains a `BudgetCharge` argument. H8 ("never refused") holds either way.

#### FS10 — P3 — Two thread tables now, one named (Tasks 3 and 15)

Plan lines 975 (`fold_sub_bound`: "rebuild the thread table"), 1117-1118 (step 4: "rebuilds
`threads`"), 2921 (compaction step 7: "Rebuild the derived thread table from the retained present
records"). With FR7's `members` table beside `present_members`, each rebuild must redo both, the
second from every retained record, present or tombstoned; a `compact` that rebuilds only the
present table leaves `members` naming pruned records (and, after a fold, stale thread ids), so
`thread_members` on a compacted part could list a document under its pre-fold thread. Say "both
tables" at the three sites.

### Method and limits

A reading review of the revision-3 diff (`git diff 584096fe5 b536b87aa` on the plan, every
hunk), of each task the hunks touch in full (Tasks 3, 4, 6, 7, 8, 9, 10, 11, 13, 14, 15 and the
plan calls and case map), and of the spec passages the corrections rest on (5.1's partial record
and marker, 5.4, 6.2 I5, 6.3's Thread state). The three traces the brief asked for were walked
by hand against the plan's own rules (`case_r22`, `case_h23`, the idle-account marker for FR1;
`case_f9_truncation_during_a_pending_rebuild` for FR2, plus `case_pl6` once the narrowing's effect
on a skipped retained insert showed; the Thread stream's existence rules and the view's counter
check for FR3, including `Thread/changes`' refusals and G3's empty account). Source checked where
a closure rested on it: `crates/email/src/cache/mod.rs:85-100, 255-310, 440-485` (`last_change_id`
is the Email log's synced `to_change_id`), `crates/jmap/src/changes/state.rs:58-66, 128-141`
(`get_state` reads the named log; `cache_state` is private), `crates/jmap/src/thread/get.rs:35-110`,
`crates/jmap/src/changes/get.rs:200-430`, `crates/jmap-proto/src/types/state.rs:15-95`,
`crates/types/src/collection.rs:228-233`, `crates/common/src/storage/transaction.rs:196-246` (the
marker lands on the `max_entries + 1`-th newest key), `crates/store/src/query/log.rs:308-333`,
`crates/jmap/src/email/set.rs:1100-1135` (the unconditional `flush` and `recheck.finish` before
the updates), `crates/common/src/cache/invalidate.rs:40-60`, `crates/common/src/lib.rs:286-296`.

Not done: no build, test or runtime probe; no subagents; the plan, the spec and every other file
are unchanged (this section is the only edit). Tasks the diff did not touch (1, 2, 5, 12, 16 to
19) were not re-read beyond the lines the findings cite; the Codex findings' closures were not
re-verified. FS1's likelihood (the marker landing on a partially consumed record) was reasoned
from `truncate_change_log`'s placement and the purge cadence, not measured.

## Scoped re-review of plan revision 4 (794915249)

Date: 2026-11-07. Plan commit: `794915249` (branch `za-search-design`), revision 4, answering
FS1 to FS10 above. Spec: revision 19, unchanged. Scope: the ten corrections, the interfaces
they renamed or added, and what the revision-4 diff touched (`git diff b536b87aa 794915249` on
the plan, every hunk); the Codex findings and the two earlier Fable sections stand.

**Verdict: APPROVE.** FS1 is closed: the `<=` resume rule and the bound-based second-truncation
test terminate on a marker that lands on a partial `indexed_to` and restart once on a genuine
second truncation (traced below). FS2 to FS10 are closed as stated. No P1 remains. Two P2s:
FS8's narrowed `Thread/changes` rule contradicts a Task 13 unit test the diff did not update,
which now fails on correct code (FT1); and the `Staging` steps FS2 added have no production path,
since `plan_pass`'s signature carries no content-cursor items, gap items or stamp, and `admit`
has no content-only mode (FT2, which predates revision 4 and is exposed by it). The rest is
wording and interface bookkeeping.

Counts: 0 P1, 2 P2, 7 P3 (9 findings). FT2, FT4 and part of FT6 predate revision 4.

### Per-finding assessment of revision 4 (FS1 to FS10)

| Finding | Closed as stated? | New defect introduced? |
|---|---|---|
| FS1 (resume at the marker's record; second truncation by the bound) | Yes. Traced the P1 input: `indexed_to = (c, p, false)`, the purge puts the marker on `c` (`transaction.rs:196-248`: the empty value lands on the `max_entries + 1`-th newest key). The pass's inclusive read from `c` meets it, `truncated && resumed_record_lost`, `za_search_reconcile_start(bound = c)`. Next pass: step 1 reads from `None`, meets the marker (`truncated`, `first_retained = c`), `bound_now = c == marker.bound` → not a second truncation; `retained_from = None` since `c <= c`; the record loop sees only records above `c` (`partial` is false for each, since none has change id `c`), consumes them, `indexed_to` moves past `c`; the following pass has `retained_from = Some(indexed_to)`, its exclusive read never meets the marker, and the marker clears by step 5 once the checkpoint reaches the cache's counter. Record `c`'s unfinished inserts are gap items (in the cache, not in the index, in no retained record), stamped `bound − 1`. A genuine second truncation whose marker lands on a partial `indexed_to` during the reconciliation: the step-1 read reports `bound_now > marker.bound` → one restart with the new bound and `gap_next = 0`; the next pass has `bound_now == marker.bound`, `retained_from = None` by `<=`, and consumes on. The complete case `(40, 0, true) → None` is unreachable while a marker is set (the probe and the exclusive read both pass a complete `indexed_to` at the marker's key) and harmless when hit (reading from `None` and from `40` exclusive yield the same records). | None in construction. FT5 (P3): a bound move that lost nothing (the marker rose but stayed at or below `indexed_to`) also restarts, re-opening skipped gap inserts below the old `gap_next` once. FT6 (P3): `za_retained_bound` and `second_truncation` are not in Task 14's interface lists, and the attribution read's `max_records` is unstated. |
| FS2 (one step per completion; three arms) | Yes. `Staging.{cursor, content_cursor: CursorStep, marker: MarkerStep}`, `bookkeeping` derived, `write_pass` applies each arm from its own step only. Traced both cursors pending: a pass with the rebuild cursor `Advanced` leaves `content_cursor = None` (the content block is gated on the rebuild cursor being `None` or exhausted this pass), so the content arm does nothing; the pass that exhausts the rebuild cursor clears it alone; the content cursor's `Exhausted` sets `content_to.cursor = None` and `content_to.checkpoint = new.indexed_to` (already `staging.checkpoint` by the first line) and the Behind rule stays quiet afterwards. Traced the marker and a content cursor pending: the content block runs first (rebuild cursor `None`), the marker stays `Untouched`, and gap items follow once the content cursor is `None`; a `GapNext` is produced only while the base manifest carries a marker, so the `expect` cannot fire, and a stale base loses its assertion before the write. | FT2 (P2, pre-existing): nothing in `plan_pass`'s signature delivers the content-cursor items, the gap inserts, the gap deletes or the stamp that produce `content_cursor` and `marker`. FT8 (P3): Task 8's "return without reading records" after the cursor block against Task 14's content block running in the pass that exhausted the rebuild cursor. |
| FS3 (attribution over every retained record) | Yes. Step 1 reads every retained record; `difference_attributes_over_every_retained_record` (`indexed_to = (49, 0, true)`, `first_retained = 45`, 48 skipped) gives `gap_inserts == []` and `retained_from == Some((49, 0, true))`; `case_pl6` traced to exactly two `ZA_SEARCH_SKIPPED` (35 in the gap passes, 48 in the first retained pass; pass F's `inserted_in_log = {46..50}` keeps 48 out). The header's FR2 line and the case-map row are rewritten. | None. |
| FS4 (probe on a cursor pass) | Yes. Traced `case_f9_truncation_during_a_pending_rebuild` with `Some(1)`: after the loss, the first keyed request's hook starts the rebuild (`indexed_to = L`, the cache's counter; the boundary batch takes `L + 1`) and re-enters the pass once, which indexes 48 cursor items and writes its witness at `L + 2`; the Thread log has the same two keys. `truncate_change_log(.., 1)` on each log: the descending iteration stops at the second-newest key, `L + 1`, writes the marker there and deletes below; `za_retained_bound = max(L + 1, L + 1) = L + 1 > L` → the next keyed request's probe calls `za_search_reconcile_start` (cursor `None`, marker `{L + 1, 0}`) and returns `Recovery(Truncated)`; the assertion `cursor == None && reconciliation.is_some()` holds with the epoch unchanged. The re-entered pass then has 52 gap inserts (the cache minus the 48 indexed minus `inserted_in_log = {}`, since `L + 2` is update-only), admits 48 under the cap with `created = L`, `GapNext(97)`; the pass after admits the last four and consumes `L + 2 ..` from `None` (`L <= L + 1`), and the marker clears when the checkpoint reaches the counter. With `Some(5)` the marker lands below `L` and the probe is silent, as the case says. The probe's `bound == indexed_to.change_id && !complete` arm matches the spec's "a record `c` found replaced by the truncation marker" and the `> ` arm the lost records; a stale cache (up to 5 s) that puts `indexed_to` below a fresh marker starts a reconciliation that is needed (the lost records' messages are in neither the cursor's snapshot nor the retained log). | FT3 (P3): the pass and Task 9's `Recovery(Truncated)` arm both call `za_search_reconcile_start`. FT9 (P3): the case rests on the re-entered cursor pass publishing its witness, so that two keys sit above `indexed_to`; say so. |
| FS5 (`CursorStep::None`; the block gated on a pending cursor; an abort moves nothing) | Yes. The cursor block runs only when `base.manifest.cursor.is_some()`; `update_only` asserts `None && !bookkeeping` on the shared base; `cursor_only` and `s_empty` carry a base with the cursor; an `Abort` leaves all three steps and `bookkeeping` at their defaults, and `write_pass`'s `None` arm keeps the manifest's cursor, so the discarded items are re-admitted from `next` and the failing one is skipped at `skip_after`. | None. |
| FS6 (content loss inside the wait) | Yes. Traced `case_h23` with `absent_retry_secs: 0`: the keyed `Email/get` loads the thread part, its pass consumes the last witness record (update-only, `Nothing`, `scanned_to` moves); the text query's `ensure_current` finds the index current, `za_search_load_content` meets the absent head, `absence_class(.., 0)` is `Loss(BlobAbsent)` at once, `za_search_content_rebuild_start(ContentLoss(BlobAbsent))` writes `content = None`, `content_to = {(0, 0, true), false, Some(u32::MAX)}` (the chain walk breaks at the missing head and takes the scan minus the thread blobs), `continue`; the next pass sees the moved hash, `plan_reload` is an `Append` with no thread entry (content not loaded, nothing to walk), the content block admits every present document (none has a content record in a `None` chain), `ContentOnly` passes until `Exhausted` sets `content_to.cursor = None` and `checkpoint = indexed_to`; `is_current` then holds, `load_content` walks the new chain, `Ok(entry)`. `content_recovered` makes a second `Loss` return `Rebuilding`. No guard is held across the rebuild start or the load (`is_current`'s and the OverCap read's guards are statement temporaries). `continue` skips the bottom deadline check once; the next iteration's check bounds it. | None. FT4 (P3, pre-existing): the Produces sentence says `ensure_current` calls `za_search_content_needs_rebuild` before loading content, and the rewritten sketch does not. |
| FS7 (step 5 against `staging.checkpoint`) | Yes; the FS1 and FS4 traces above clear the marker through it on an idle account. | None. |
| FS8 (D13's citations; `State::from(Option)`; the member-set rule) | Yes. D13 names 6.2's I5, 6.3's sentence and the revision-16 summary line (spec lines 41-64); `State::from((s != 0).then_some(s))` matches `crates/jmap-proto/src/types/state.rs:33-40`; Task 11's Files and Consumes lines describe the view's counter. The member-set rule traced against `case_merge_rendering_parity`'s new step: a flag-only `UpdateItem` of `U` at `s + 1` makes `U` a candidate with `T0 == T1` and `members_of(T, s) == members_of(T, s + 1)` → nothing, while the plain control's Thread log holds no record for the flag change → both empty; `Email/changes` keeps `Updated(T|U)` on both. Against R25's pinned page: a deleted last member (some at `S`, none at `E`) and a merge (sets differ) still classify as before, and a pinned `E` still counts a member deleted in `later`. Residual, same class as R14: a member created and deleted within the range leaves the sets equal, so the fork renders nothing where upstream's folded Thread container records may say `Updated`; no client-visible difference in members. | FT1 (P2): the Task 13 unit test `thread_members_at_a_pinned_end_count_a_member_deleted_after_it` still asserts `[Updated(3)]` for a range whose only record is a flag-only `UpdateItem` with equal member sets. |
| FS9 (the clone's charge in step 5) | Yes: `let _clone_charge = self.inner.cache.za_search.charge(PartKind::Thread, base.thread.loaded_bytes() as u64)` before `plan_pass`, a named binding that lives past `write_pass`; `charge` is Task 6's and H8 holds. | None. |
| FS10 (both thread tables) | Yes at the three sites (Task 3's interface and step 4, compaction step 7). | FT7 (P3): Task 3 step 4's storage list still declares `threads` and `buckets` only, so "`threads` and `members`" names a field the sketch lacks. |

### New findings

#### FT1 — P2 — `thread_members_at_a_pinned_end_count_a_member_deleted_after_it` fails under FS8's rule (Task 13)

Confidence: high. Plan lines 2742 (the rule: "some at both and the sets equal → nothing") and
2790 (the test: "T=3 members {3, 7} at both S=19 and E=20 (created before 19); 7 deleted at 25
(in `later`); za_thread_stream S=19 E=20 with an UpdateItem of 7 in range: [Updated(3)]").

The fixture's only record in `(19, 20]` is a flag-only `UpdateItem` of 7. Existence of 7 at
`S = 19`: no insert or delete in range, `later` holds a delete, the index holds it with
`created < 19` → present; at `E = 20`: no delete in range, `E` pinned, `later` holds the delete at
25 → present. `members_of(3, 19) == members_of(3, 20) == [3, 7]`: the sets are equal, so revision
4's rule yields `[]`, and the assertion `[Updated(3)]` fails on the prescribed construction. The
revision-4 diff changed the rule and left the test as revision 1 wrote it. The second assertion,
`members_of(3, 20) == [3, 7]`, is R25's claim and still holds.

Correction: give the range a membership change and keep the pinned-end claim: 7 inserted at 20
(`InsertItem(7)` in range, its record created at 20 with a revision whose row is at 20), so
`members_of(3, 19) == [3]`, `members_of(3, 20) == [3, 7]`, `za_thread_stream == [Updated(3)]`,
and `members_of(3, 20) == [3, 7]` though 7 is deleted at 25 in `later`; or keep the fixture and
assert `[]` with a comment citing FS8. Add the FS8 variant to the case-map row.

#### FT2 — P2 — `Staging.content_cursor` and `Staging.marker` have no production path through `plan_pass` (Tasks 8 and 14; predates revision 4)

Confidence: medium (an executor can invent the missing parameters, but the plan is silent on
them and `admit`'s one mode is wrong for content items). Plan lines 2004-2005 (`Staging`,
`plan_pass(base, cursor_items, records, limits, started, opener)`), 2146 (step 3: `admit` always
calls `staged.assign` and pushes a record; `content_cursor` and `marker` "are Task 14's, `None`
and `Untouched` until it sets them"), 2866 (Task 14 modifies `pass.rs` for "the content cursor's
and the marker's steps"), 2886 ("the pass admits them under the same cap, opening each and
staging a content record only (no thread assignment, no alias)"), 2890 (gap inserts "opened and
indexed as an insert with `created = bound − 1`", gap deletes "tombstoned with `deleted = bound −
1`"), 2899 ("the item source is `cursor_items`, then `content_cursor_items`, then the
difference's gap items, then the log").

`plan_pass` receives one `Vec<u32>` and the records. Nothing in its signature carries the
content-cursor list (whose items must be opened and yield `content.push` only, no `assign`, no
record, no alias, no `witness`), the gap inserts (admitted with `created = bound − 1`, a value
`plan_pass` cannot derive from `base`), the gap deletes (tombstoned with `deleted = bound − 1`,
outside any record) or the marker's `gap_next` to produce `GapNext(id + 1)`. Revision 3 had the
same gap; FS2's new fields make it concrete, since the fields now exist and no text says how
they are set. Counterexample of the wrong-mode risk: an executor who feeds content-cursor items
through `cursor_items` has `admit` assign each a record at `revision + 1` for a document the
part already holds, the decision becomes `Publishing`, and the pass writes a thread blob and a
witness entry for a content-only step, which 5.4 forbids ("no thread blob, no revision, no log
entry").

Correction: in Task 8's Produces, replace `cursor_items: Vec<u32>` with a sources struct that
Task 14 fills, for example `pub(crate) struct PassSources { pub cursor: Vec<u32>, pub content:
Vec<u32>, pub gap_inserts: Vec<u32>, pub gap_deletes: Vec<u32>, pub gap_stamp: Option<u64> /*
bound − 1 while a marker is set */ }` (Task 8 passes empty content and gap lists), add an
`admit_content(d)` beside `admit` (open, `content.push((d, c))`, `items += 1`, `reserved`,
`bytes`; a `Loss` counts as skipped and still advances the cursor; a `Transient` aborts as
`admit` does), and state in Task 14 step 4 which source sets which step: the content loop sets
`content_cursor`, the gap-insert loop sets `marker = GapNext(id + 1)` after each item, the gap
deletes call `staged.tombstone(d, stamp)`, and step 5 sets `marker = Finished`.

#### FT3 — P3 — Two starters for one reconciliation (Tasks 9 and 14)

Confidence: medium-high. Plan lines 2887 ("the pass writes a manifest-only checkpoint with
`reconciliation = Some(..)`"), 2899 ("`truncated || resumed_record_lost` →
`za_search_reconcile_start` and return `Recovery(Truncated)`"; the probe: "→
`za_search_reconcile_start` ... and return `Recovery(Truncated)`"; and "In Task 9's
`za_search_pass_hook` and `za_search_ensure_current`, the `Ok(Recovery(cause))` arm calls ...
`za_search_reconcile_start` for `Truncated`, then re-enters the pass once"), 2265 (Task 9's arm).

Read together, the pass writes the marker and returns `Recovery(Truncated)`, and the hook writes
it again before re-entering: two manifest writes, the second re-reading the bound and resetting
`gap_next` to 0, harmless on a fresh start but a redundant write and hash move on every start
and on every FS1 restart. Revision 4 made the pass's return value explicit, which makes the
duplication explicit. Correction: one starter. The pass starts it (it has the bound and, for the
probe, the cursor in hand) and `Recovery(Truncated)` means "started, re-enter"; Task 9's
`Truncated` arm only re-enters. Or the pass returns without starting and the hook starts it with
`za_retained_bound`; the former keeps the probe's clearing of the cursor in one write.

#### FT4 — P3 — `za_search_content_needs_rebuild` has a named caller that does not call it (Tasks 9 and 14; predates revision 4)

Confidence: medium. Plan lines 2871 ("Task 9's `za_search_ensure_current` calls it before
loading content"), 2267 (the rewritten sketch: OverCap check, then `za_search_load_content`,
no call), 2886 (`CapRaised` and `Behind`), 2909 (`case_f6_r23`: "clear the override,
`simulate_restart`, one keyed request starts the content-only rebuild").

`is_current` is false while `content_to.suspended` or `content_to.checkpoint != indexed_to`, so
without the call the wait spins to `NotCurrent` and nothing starts the `CapRaised` or `Behind`
rebuild; `case_f6_r23` expects "one keyed request" (not a content request) to start it, which
points at the pass hook rather than the wait. Revision 4 rewrote the `ensure_current` line in
full and kept the Produces sentence. Correction: name the site in Task 14 step 4. The pass
(`za_search_pass` step 0, after the shortcut is refused and the manifest read): `if let Some(r)
= za_search_content_needs_rebuild(&base.manifest, &config) { self.za_search_content_rebuild_start
(account_id, keys, r).await?; return Ok(Advanced { published: false, indexed_to }) }`, which
serves both the case and the wait (the next loop iteration runs the content cursor); and drop
or correct the Produces sentence.

#### FT5 — P3 — A bound move that lost nothing restarts the reconciliation (Task 14)

Confidence: medium. Plan line 2891 ("when a read reports `truncated` and the log's bound now
read ... is above `marker.bound`, the pass restarts").

The step-1 read from `None` always reports `truncated`, so every pass compares bounds. A purge
during the reconciliation that moves the marker up but not past `indexed_to` (the records
between were consumed) restarts with `gap_next = 0`, and every gap insert skipped as loss below
the old `gap_next` is re-opened once (PL6's cost, bounded, no suite case counts skips across a
second truncation). Not wrong; one pass and one open per skipped item more than needed.
Correction, optional: restart only when the move lost records, `bound_now > indexed_to.change_id
|| (bound_now == indexed_to.change_id && !indexed_to.complete)` (the probe's test), and otherwise
rewrite `marker.bound = bound_now` in the pass's manifest so the comparison is quiet next pass
(later stamps then use the new bound, which is still below every servable state).

#### FT6 — P3 — Interface bookkeeping around FS1 and FS4 (Task 14)

Plan lines 2870-2871 (Consumes and Produces), 2888 (step 1), 2896 (`second_truncation`), 2899.
`Server::za_retained_bound` is Task 13's and is consumed by the probe and by rule 4, but is
absent from Task 14's Consumes (`Store::za_log_first_retained` is listed instead); `pub(crate)
fn second_truncation(marker_bound: u64, bound_now: u64) -> bool` appears only in the unit test's
description, not in Produces; and step 1's attribution read ("every retained record, read once
per pass") names no `max_records`, while Task 8's read passes `config.checkpoint.max(pass_cap +
1)`. A capped attribution read would turn a retained insert above the cap into a gap insert
stamped `bound − 1`, which a client at a state between the record and `E` would never be shown
as created. Say `max_records: usize::MAX` (the retained log is at most `maxChangesHistory`
records), add the two names to the lists.

#### FT7 — P3 — Task 3 step 4's storage sketch lacks the `members` table (Task 3)

Plan lines 1113-1117 (storage: `records`, `aliases`, `rows`, `threads`, `buckets`) and 1131
("rebuilds both thread tables (`threads` and `members`, FS10)"). The interface at line 988 and
`loaded_bytes` already count two tables; the sketch declares one. Add `members: AHashMap<u32,
Vec<u32>>` (every record, present or tombstoned, by `current_thread`, ascending) to the storage
list, and say that `assign` inserts into both and `tombstone` removes from `threads` only.

#### FT8 — P3 — The cursor block's `return` against the content block's "or was exhausted this pass" (Tasks 8 and 14)

Plan lines 2146 (Task 8 step 3: after the cursor loop, "return without reading records") and
2886 (Task 14: the content block "runs only while `base.manifest.content_to.cursor.is_some()` and
the rebuild cursor is `None` or was exhausted this pass"). Read literally, Task 8's `return`
precedes any content block Task 14 adds after it, so "was exhausted this pass" cannot happen;
Task 8's own rule ("no log record in a pass that worked cursor items") is about records, not
content items. Say in Task 8 that the cursor block returns "before the record loop, after the
sources Task 14 adds", or drop "or was exhausted this pass" and let the content cursor wait one
pass.

#### FT9 — P3 — `case_f9_truncation_during_a_pending_rebuild` rests on an unstated key count (Task 14)

Plan line 2912. The `Some(1)` fixture works because exactly two Email keys sit above
`indexed_to = L` after the first keyed request: the boundary at `L + 1` and the re-entered
cursor pass's witness at `L + 2`, so the second-newest key is `L + 1`. If the hook did not
re-enter, or the re-entered pass were not publishing, the marker would land on `L` itself
(complete) and the probe would be silent. Say so in the case and assert it before truncating
(`read_manifest(..).thread_blobs.len() == 1` and `cursor == Some(..)` after the first keyed
request, or `log_records_since(.., L).len() == 2`).

### Method and limits

A reading review of the revision-4 diff (`git diff b536b87aa 794915249` on the plan, all 253
lines, every hunk), of the tasks the hunks touch in full where the hunk's rule depends on them
(Tasks 3, 6, 7, 8, 9, 11, 13, 14, 15, the header, the plan calls and the case map), and of the
spec passages the corrections rest on (4.2's manifest fields, 5.1's checkpoints and error
classes, 5.4, 6.3, 7.5). The five traces the brief asked for were walked by hand against the
plan's own rules: FS1 on a marker landing on a partial `indexed_to` and on a genuine second
truncation; FS2's three arms with a rebuild cursor and a content cursor pending and with a
marker and a content cursor pending; FS4's probe on `case_f9_truncation_during_a_pending_rebuild`
with `Some(1)` and `Some(5)`; FS6's re-entry on `case_h23`; FS8's rule on
`case_merge_rendering_parity`'s flag step, on R25's pinned page and on Task 13's unit tests.
Source checked where a closure rested on it: `crates/common/src/storage/transaction.rs:116-248`
(`delete_changes` truncates each sync collection to `max_entries`; `truncate_change_log` places
the empty value on the `max_entries + 1`-th newest key and deletes below), `crates/store/src/
query/log.rs:117-184, 308-333` (the exclusive skip of the from-key before the empty-value check,
so a complete `indexed_to` at the marker's key never reports `truncated`),
`crates/jmap-proto/src/types/state.rs:15-95` (`From<Option<ChangeId>>` only),
`crates/services/src/task_manager/tasks/maintenance.rs:425-435` and
`crates/email/src/message/delete.rs:155-180` (the purge task's `purge_account` calls
`delete_changes`), `tests/src/system/purge.rs:35-50, 140-155` (`max_changes_history: Some(1)`
through `DataRetention`). The Thread-log bound was reasoned, not measured: every Thread log key
of a key account is also an Email log key (the stub ingest's container insert, the emptied-thread
delete and the witness's property change share their batch's change id with an Email item
record), so after a purge at one `max_entries` the Thread marker is at or below the Email marker
and `za_retained_bound` is the Email marker; the FS4 trace depends on that.

Not done: no build, test or runtime probe; no subagents; the plan, the spec and every other file
are unchanged (this section is the only edit). Tasks the diff did not touch (1, 2, 4, 5, 10, 12,
16 to 19) were not re-read beyond the lines the findings cite; the Codex findings' and FR1 to
FR14's closures were not re-verified beyond what the FS traces exercised. Task 7's
`first_retained` sketch (set on every key read, against its doc comment and test, which say the
marker's key only) was noted as a pre-existing ambiguity outside this scope; FS1's rule reads it
from a from-`None` read, whose first key is the marker whenever one exists.

## Scoped re-review of plan revisions 5 to 7 (aa594dc09)

Date: 2026-11-08. Plan commit: `aa594dc09` (branch `za-search-design`), revision 7. Spec:
revision 20 (the D13 wording). Scope: the text `git diff 794915249 aa594dc09` adds or changes
in the plan (every hunk), the names those hunks introduced or renamed and every consumer of
each, and the spec's revision-20 diff. Revision 4's approval, the Codex rounds and the three
earlier Fable sections stand and were not reopened.

**Verdict: REQUEST CHANGES.** One P1 remains, introduced by revision 5's FT4 correction: the
`CapRaised` and `Behind` content-only rebuilds are started by the pass "after the shortcut is
refused", and the shortcut answers `Current` on an idle account whose checkpoint sits at the
cache's counter (the ordinary state after a deletion-only or an update-only checkpoint pass),
so after a restart with a raised cap nothing starts the rebuild until unrelated mail activity,
and every text search of that account spins to its wait deadline and refuses (FU1; the fix is
one clause in the shortcut). The other construction changes hold: `PassSources` and
`admit_content` trace correctly through a content pass, a gap pass, an `Abort` and the
two-cursors case; `SecondTruncation` gives the right answer on every input tried; the swap's
growth admission leaves the loaded index consistent and cannot race a reader; the D13 regression
terminates in both forms. Five P2s (a gap-delete ordering the sketch gets wrong across the cap
and the absent `plan_pass` test for the new sources, a bound move with no write path, two Task 6
tests that exercise Task 8's code, a unit fixture FT8's rule now fails, and a content cursor
that completes over the cap without suspending) and seven P3s.

Counts: 1 P1, 5 P2, 7 P3 (13 findings).

### Assessment of the revision 5 to 7 changes

| Change | Assessment | Findings |
|---|---|---|
| FT1 (Task 13, the pinned-end unit test; plan line 2823) | Closed. Traced: 7 inserted at 20 into T=3, record created at 20, row at 20; `members_of(3, 19) == [3]`, `members_of(3, 20) == [3, 7]` (E pinned, the delete at 25 is in `later`), sets differ → `[Updated(3)]`; the FT1 variant (flag-only `UpdateItem`, 7 created before 19) gives equal sets → `[]`. | — |
| FT2 (Tasks 8 and 14, `PassSources` and `admit_content`; 2037-2038, 2179, 2188, 2914, 2930, 2934) | The production path now exists. Traced a content-cursor pass: `content_to.cursor = Some(u32::MAX)`, list `[9, 7, 4]` (present, no content record), cap 2: `admit_content(9)`, `admit_content(7)` push two content records, cap → `content_cursor = Advanced(7)`, no record, alias or witness, decision `ContentOnly` (records empty, content non-empty), the arm writes `next_below = 7`, a content blob and the manifest, no thread blob, no row, no log entry; the next pass lists `[4]`, runs off → `Exhausted`, the arm sets `content_to.cursor = None`, `checkpoint = indexed_to`. All three items `Loss`: `skipped` three, `consumed` unset, `Exhausted` → `bookkeeping` → `ManifestOnly`. An `Abort` at item 7 (`Transient`, attempts below `skip_after`): `content` discarded, `content_cursor = None`, the arm keeps `next_below = u32::MAX`, `failing = (7, 1)`; the next pass re-admits 9 and 7. A gap pass with marker `{bound: 40, gap_next: 0}`, `gap_stamp = Some(39)`, inserts `[2, 5]`, deletes `[8]`: `admit(2, created = 39)`, `GapNext(3)`, `admit(5, 39)`, `GapNext(6)`, 8 tombstoned at 39, `Publishing`, the marker arm writes `gap_next = 6`. Both cursors pending: the cursor block runs first; when the cap stops it, `content_cursor` stays `None` and the content arm does nothing (FS2's trace stands); when it runs off its list, the content block runs under the remaining cap in the same pass and both arms apply. | FU2 (the gap deletes after a cap-stopped insert loop; no `plan_pass` test with content or gap sources), FU5 (`s_empty`), FU6 (`!content_allowed` in the content block) |
| FT3 (Tasks 9 and 14, one starter; 2943) | Closed. The pass starts the reconciliation and returns `Recovery(Truncated)`; Task 14 step 4 has Task 9's `Truncated` arm "only re-enter". Task 9's own step 3 keeps its placeholder comment, which Task 14 replaces, so the two tasks agree. | — |
| FT4 (Tasks 9 and 14, the `CapRaised`/`Behind` start in the pass; 2915, 2300) | Not closed: the named site is behind the shortcut. Traced `case_f6_r23`: after `simulate_restart` the entry reloads with `scanned_to = indexed_to.change_id`; the last pass before the restart published (the deliveries while suspended), so its witness sits above `indexed_to`, the shortcut is refused, the manifest is read, `za_search_content_needs_rebuild` answers `CapRaised`, the start writes `content = None` and the cursor, the pass returns `Advanced`, and the next keyed request's pass runs the content block: the case passes as written. The same flow on an account whose last pass was deletion-only (`NonPublishing`, `indexed_to` at the delete record, no log entry) answers `Current` at the shortcut on every request (FU1). `case_h23` traced as FS6 did; the content-loss arm in `za_search_ensure_current` is untouched by FT4 and still re-enters once; `is_current` is false while the cursor is pending, so the shortcut's `content_to.cursor.is_none()` term keeps the pass running during that rebuild. | FU1 |
| FT5 (Task 14, `SecondTruncation`; 2915, 2935, 2940) | The decision is right on every input tried. Marker on a partial `indexed_to` (FS1's case): `bound_now == marker.bound` → `No`, consume on. A genuine second truncation, `indexed_to = (44, 0, true)`, marker moved to 45 → `Restart` (record 45 unconsumed and lost); moved to 44 → `BoundMoved` (44 consumed whole); `(44, 2, false)` with the marker on 44 → `Restart` (its remaining inserts lost). A move to 41 with `indexed_to = (44, 0, true)` → `BoundMoved`, stamps from `bound_now − 1` stay below every servable state. But the "rewrite `marker.bound`" outcome has no carrier to the manifest. | FU3 |
| FT6 to FT9 (lists, `usize::MAX`, `members`, the source order, `case_f9`'s assertion, Task 7's `first_retained`) | Closed as stated. `log_records_since(.., L).len() == 2` matches the FS4 trace (the boundary at `L + 1`, the witness at `L + 2`). FT8's rule ("a pass that worked any item of the four reads no records") is consistent with Task 14's content-block gate, and lets an exhausted-empty cursor pass consume the records after `indexed_to`, which is safe (an insert logged after `indexed_to` has an id above `end`, H15) but changes what one Task 8 unit fixture asserts. | FU5 |
| Revision 6, PL3 (the D13 regression, Task 13; 2875-2885) and revision 7, PL3 (`SEARCH_VIEW_PAUSE`, Tasks 2 and 10; 232, 645, 907, 2377, 3285) | The pause sits at step 8, after the guard is held and before rendering; Task 2's lists, Global Constraints and the CLAUDE.md paragraph carry it. The same-node form terminates: the Thread/get's own pre-request pass consumed the witness (`Nothing`, `scanned_to` moved under `index.write()` before the view existed), so the second connection's pre-request pass takes the shortcut without a write lock; the keyed write commits; its post-commit pass clones its base under `index.read()` beside the parked guard (no writer is queued), commits, and parks at `SEARCH_SWAP_PAUSE` before `index.write()`; the released Thread/get renders from its guard and returns `cache.last_change_id`, read before the merge; the released pass then takes the write lock and swaps. `take_for` is one-shot per account (`test_hooks.rs:418-424`), so the keyed write's own response view passes the pause point. The remote form (`SEARCH_SWAP_SKIP`) is as claimed: the next `Thread/changes` reads the moved manifest, reloads and renders the merge. | FU8 (the write that runs the post-commit pass is named "deliver") |
| Revision 6, PL11 (the charges resized at the swaps and compactions; the test's owners; 2219, 2986, 1772) | Closed. `thread_charge.resize(thread_bytes)` after `extend`; the compaction swaps resize both; the lifetime test drops the map's and the local handles before expecting the counter to move. | — |
| Revision 7, PL11 (growth admitted against the process budget; 2219, 1649, 1772, 500-501) | The construction holds. The arm runs under `entry.index.write()`, so no reader holds the view it mutates; `make_room` is a synchronous `fn` over `try_read` (Task 6 step 3), so nothing awaits under the lock and no two swaps can wait on each other's entries; the account's own entry is `keep` and reads as in use. Refused: `index.content = None` drops `LoadedContent` and its `BudgetCharge` at the old size (the charge was not yet resized), the counter stays consistent, `manifest`, `scanned_to`, `thread_segments` and `thread_bytes` are still set, the `segments` insert is skipped by its `if let`, and the next content request finds `guard.content.is_none()` at the view's step 7 and loads through `za_search_load_content`'s own `make_room`, as after an eviction; `is_current` reads the manifest only and is unaffected. Admitted: `bytes` and the charge move to `loaded_bytes()`, the one estimate the loads use. | FU4 (the two tests' home), FU7 (`cache` names the message cache in `write_pass`; the assignment inside the `as_mut()` borrow), FU11 |
| Revision 6, PL19 (`Writer::into_raw_bytes`; 534-542, 565) and revision 7, PL19 (`codec.rs` in Files and the commit; 511, 618) | Closed. `into_document` compresses at or above `COMPRESS_WATERMARK_DICTIONARY` (64 bytes, `codec.rs:89-102`, `compress.rs:20`) and always appends a marker, so the raw buffer is the right measure. The above-watermark assertion holds for the fixture: `w0..w3999` are at most 5 bytes, which `CheekyHash` stores inline (`cheeky_hash.rs:33-43`), so the term list and the ascending position lists compress and `stored` carries `DOCUMENT_ZSTD`; `MAX_DOCUMENT_SIZE` is 1 MiB, no truncation. `index_text`, `with_account_id`, `with_document_id` exist with the signatures the test uses (`document.rs:14-76`); `AccountIndexer` is `pub(crate)`, reachable from `index_za_tests.rs`. `IndexDocument` derives no `Clone` and `SearchValue` is `Clone`, so `clone_for_test` is a field-wise clone. | FU9 (`clone_for_test` in no Files or Produces line) |
| Revision 6, PL1 (`r11`'s two forms; 1723-1728) | Closed. `fetch_verified` checks the descriptor's hash before `parse_content_blob`, so the stale-hash form is `Loss(Hash)`; the header form rebuilds node 2 to name the replacement's real hash and node 1's segment, and `parse_content_blob`'s predecessor check (Task 5: the header's segment against the expected one, in the clear, `Err(Parse)`, as `r11_content_blob_names_its_predecessor` asserts) answers `Loss(Parse)`. The fixture builder must re-derive the manifest's head hash too, since node 2's bytes changed; implied, not stated. | — |
| Revision 6, PL16 (`case_content_wait_budget`; 2288) | Closed; the equality is gone, `is_current` and the progress assertions remain. | — |
| D13 (plan call 151; spec revision 20, header lines 4-16, 6.2 at 1763-1796, 6.3 at 2103-2110) | Agree. The spec now says the state is the validated view's cache counter, the Email group's counter the cache was built at, never a store read after the cache; 6.3 adds that a flag-only record inside `(S, E]` renders nothing for the Thread stream, which is FS8's rule. `ZaSnapshot::state()` (2520) is that counter; `Thread/get` (2570) and the page (2779) consume it; no `thread_state` remains outside the case name. | FU10 (stale "for Jay") |

### New findings

#### FU1 — P1 — The `CapRaised` and `Behind` rebuilds start only when the shortcut is refused (Tasks 8 and 14)

Confidence: medium-high. Plan lines 2915 (Produces: "called by the pass, step 0 of
`za_search_pass`, after the shortcut is refused and the manifest read"), 2187 (the shortcut:
`cursor.is_none() && reconciliation.is_none() && content_to.cursor.is_none() &&
indexed_to.complete && cache.last_change_id <= scanned_to` → `Current`), 2219 (a
`NonPublishing` pass writes `indexed_to = staging.checkpoint` and no log entry), 2300
(`za_search_ensure_current`: `Current` → `is_current` false while `content_to.suspended` →
sleep → loop → `NotCurrent` at the deadline), 2953 (`case_f6_r23`). Spec 5.4 (lines 1361-1366:
"a content-only rebuild, taken when the account cap is raised over a suspended chain") and 9.2
(2627-2630: "a raised cap, or a second node with a higher cap, takes a content-only rebuild").

Counterexample. A key account over the cap (`content_to.suspended`); the user deletes a message
on a keyed session; the next pass is `NonPublishing`: `indexed_to = complete(d)` where `d` is the
delete record's change id, the account's newest, and no log entry is written. The account is
then idle. The operator raises `ZA_SEARCH_ACCOUNT_MAX_MB` and restarts. The user's first keyed
request: `za_search_entry` loads the thread part with `scanned_to = scanned_seed(indexed_to) =
d`; the revalidated cache has `last_change_id = d`; no cursor, no marker, no content cursor,
`indexed_to.complete` → the shortcut answers `Current`, and `za_search_content_needs_rebuild`
is never evaluated. Every later keyed request takes the same shortcut. A text search:
`za_search_ensure_current` runs the pass (`Current`), finds `is_current` false (`suspended`),
sleeps 20 ms, and repeats until the deadline → `Unavailable::NotCurrent` →
`ZA_MAIL_SEARCH_UNAVAILABLE` after the whole wait budget, on every search, until a delivery or a
flag change moves the counter above `d`. The spec's "takes a content-only rebuild" is not met in
that state, and the symptom is a hang of `ZA_SEARCH_WAIT_SECS` per search. The same holds for a
`ManifestOnly` checkpoint written by the update-only rule (`indexed_to = complete(scanned_to)`),
and for the "second node with a higher cap" of spec 9.2. `case_f6_r23` does not reach it because
its last pass before the restart published, leaving a witness above `indexed_to`.

Correction. Make the shortcut's "nothing to do" include the content rule: in step 0's read
block compute `needs = za_search_content_needs_rebuild(&manifest, &config)` from the loaded
manifest and add `&& needs.is_none()` to the shortcut's condition (or equivalently run the start
when `needs` is `Some`, before the shortcut), then keep the start where revision 5 put it, after
the manifest read and the reload-if-moved, so a stale loaded manifest is refreshed first. State
the site in Task 8 step 5's step 0 and Task 14 step 4 ("at step 0, ... disables the shortcut"),
not only in Task 14's Produces parenthetical. Add to `case_f6_r23` a variant that ends the
suspended phase with a keyed deletion (a `NonPublishing` pass, `indexed_to` at the counter)
before the restart, and asserts that the first keyed request still starts the rebuild.

#### FU2 — P2 — The gap deletes run after a cap-stopped gap-insert loop, and no unit test drives `plan_pass` with content or gap sources (Tasks 8 and 14)

Confidence: high on the text; the H3 violation follows from the sketch's literal reading. Plan
lines 2179 (the sketch: "`for d in gap_inserts { if cap → break; admit(d, created = stamp);
marker = GapNext(d + 1) }` ...; then `for d in gap_deletes { ... staged.tombstone(d, stamp)
... }`"), the same line's general rule ("each later block running only when every earlier one is
not pending, is empty, or ran off its list this pass"), 2934 (rule 3: "Gap inserts first, under
the pass's cap ...; then gap deletes"), 496 (the FT2 case-map row), 2940 (the pure tests). Spec
5.4, lines 1409-1416: "the gap items first ..., their inserts oldest document id first and then
their deletions, so that a gap insert finds every message it references that the gap deleted".

The general sentence gates the gap-delete block on the gap-insert block running off its list;
the sketch's "then `for d in gap_deletes`" after a loop that `break`s at the cap does not. An
executor who follows the sketch applies every gap delete in the pass whose insert loop the cap
stopped. Counterexample: gap inserts `[.. 48 items .., Y]` with `Y` replying to `X`, gap deletes
`[X]`, cap 48. Pass 1 admits 48 inserts, breaks, tombstones `X` (`staged.tombstone`, dropped
from `buckets`). Pass 2 admits `Y`: `assign`'s candidates are the present records of its bucket,
`X` is tombstoned, `Y` is a singleton where upstream, and spec H3, thread it with `X`. The
mutant `reconciliation_h3_gap_insert_threads_with_gap_deleted` guards is this order within one
pass; nothing guards it across the cap, and no suite case has gap deletes beside more gap
inserts than the cap (`case_f9_truncation_during_a_pending_rebuild` has 52 gap inserts and no
deletion). The FT2 case-map row names two pure tests of `rebuild.rs` and a suite case, none of
which calls `plan_pass` with a non-empty `content`, `gap_inserts` or `gap_deletes`, so the new
blocks (`admit_content`'s three arms, the content cursor's two steps, `GapNext`, the stamp, the
gating) have no unit test.

Correction: in the sketch, `if gap_inserts ran off its list this pass { for d in gap_deletes {
... } }` (the deletes stay in the difference until applied, so deferring them loses nothing),
and say in rule 3 that the deletions wait for the last gap insert. Add to Task 8 step 1 two
tests through `plan_pass`: `content_sources_stage_content_only` (a base with
`content_to.cursor = Some(u32::MAX)`, `sources.content = [9, 7, 4]`, cap 2 → two content
records, no record, no alias, `witness == None`, `ContentOnly`, `content_cursor ==
Advanced(7)`; the resumed pass → `Exhausted`; one `Loss` item → `skipped` and the cursor
advances; a `Transient` → `Abort` with `content_cursor == None`) and
`gap_deletes_wait_for_the_last_gap_insert` (marker set, `gap_stamp = Some(39)`, `gap_inserts =
[2, 5]` with 5 replying to 8, `gap_deletes = [8]`, cap 1: pass 1 admits 2 with `created == 39`,
`marker == GapNext(3)`, `tombstones.is_empty()`; pass 2 admits 5, `thread_of(5) ==
thread_of(8)`, then `tombstones == [Tombstone { 8, 39 }]`). Name them in the FT2 row.

#### FU3 — P2 — `SecondTruncation::BoundMoved` has no write path, and step 4's wiring sentence still restarts (Tasks 8 and 14)

Confidence: high. Plan lines 2935 (rule 4: "else it writes `marker.bound = bound_now` in its
manifest and consumes on"), 2036 (`Staging.marker: MarkerStep`), 2179 (`pub enum MarkerStep {
Untouched, GapNext(u32), Finished }`), 2193 (the marker arm: `Untouched => {}`, `GapNext(g) =>
.. gap_next = g`, `Finished => .. = None`; "Task 14 adds no code to `write.rs`"), 2910 (Files:
"`write.rs`'s three arms are Task 8's and need no change, FS2"), 2943 (step 4: "a `truncated`
read whose bound is above `marker.bound` restarts the reconciliation (rule 4, PL6, FS1)").

Nothing in `Staging` or `MarkerStep` carries a bound, and the arm writes `gap_next` or clears
the marker, so "writes `marker.bound = bound_now`" has no path to the manifest; an executor
either leaves the bound unwritten (harmless: the next pass compares again and answers
`BoundMoved` again, so FT5's quietness is not achieved and the stamps of that pass come from a
bound the manifest does not record) or invents a field. Step 4's sentence, which the FT5 hunk
did not touch, still describes the revision-4 rule. Correction: give the step a carrier,
`MarkerStep::GapNext { gap_next: u32, bound: Option<u64> }` or a separate `Staging.marker_bound:
Option<u64>` the arm applies as `new.reconciliation.as_mut().expect(..).bound = b` (the arm is
Task 8's, so Task 8 writes it and Task 14 produces the value); say that a `BoundMoved` pass with
nothing else staged is a `ManifestOnly` write (add `marker_bound.is_some()` to the decision's
`ManifestOnly` disjunction); say that `gap_stamp` is `bound_now − 1` in that pass; and rewrite
step 4's sentence to name `second_truncation`'s three outcomes.

#### FU4 — P2 — Two Task 6 unit tests exercise Task 8's swap arm (Tasks 6 and 8)

Confidence: high. Plan lines 1772 (`pl11_growth_through_local_passes_is_charged`: "five calls
of the swap's content arm ...; `set_content_bytes_for_test` is not used; the swap path is";
`pl11_growth_across_accounts_is_admitted_against_the_process_budget`: "account 1's swap arm
grows it by 300 KiB", "its `indexed_to` and `scanned_to` moved as the swap sets them"), 1595
(Task 6's test file is `crates/common/src/za/search/load_tests.rs`), 2015 and 2219 (the swap is
`write_pass` in `crates/email/src/message/za/search/write.rs`, Task 8). Task 6 runs before Task
8 and `common` cannot depend on `email`, so neither test can call the code it describes when
Task 6 is executed; the case-map rows (500-501) cite Task 6 for both. Correction: factor the
growth admission into `common` as a method Task 6 produces and tests, for example
`ZaSearchCache::admit_content_growth(&self, index: &mut LoadedIndex, account_id: u32) -> bool`
(compute `grown = part.loaded_bytes()`, `make_room` for the delta, drop `content` and return
`false` when refused, else resize `bytes` and the charge), called by Task 8's swap after the
inserts; the two tests then insert records into a `LoadedIndex` built in the test and call it,
and the `indexed_to`/`scanned_to` clause moves to a Task 8 suite case or is dropped.

#### FU5 — P2 — `s_empty` asserts `ManifestOnly` on a pass that now reads the records (Task 8)

Confidence: medium (it depends on `s_empty` sharing the fixture's records, which the test's
first line suggests). Plan lines 2154-2162 (`cursor_items_come_first_and_take_the_epoch_as_
creation`: "records c11.. present"; `s_empty`: "an empty list is exhausted (FR1)", asserting
`Exhausted && bookkeeping && ManifestOnly`), 2179 ("a pass that worked any item of the four
reads no records ... and returns after them"). Under revision 4 a pending cursor returned before
the record loop whatever its list held; under FT8's rule a pending cursor whose list is empty
worked no item, so the pass reads `records` and admits c11's inserts: `Publishing`, not
`ManifestOnly`. Correction: give `s_empty` an empty `records` (and keep the assertion), or keep
the records and assert `Publishing` with `cursor == Exhausted` ("the exhausted cursor and the
records clear in one write"); either way, say which.

#### FU6 — P2 — A content cursor finishes over the cap without suspending (Tasks 8 and 14)

Confidence: medium. Plan lines 2188 (`content = limits.content_allowed.then(|| ContentRecord::
new(..))` in the opener, `content_allowed = !suspended && content.size < account_max_mb << 20`),
2179 (`admit_content`: `if let Some(c) = indexed.content { content.push(..) }`, the cursor
advances either way), 2194 ("`new.content_to.checkpoint = new.indexed_to` whenever ... every
record this pass indexed has a content record (`staging.content.len() == staging.records.len()`,
vacuously true for a pass that indexed none)"; "When `Publishing` and `!limits.content_allowed`:
`new.content_to.suspended = true`"), 2193 (the `Exhausted` arm: `content_to.cursor = None;
content_to.checkpoint = new.indexed_to`). Spec 4.2 (766-770: `content_to` is "the change id and
cursor bound up to which every indexed message also has its content record").

If the chain reaches the cap during a content-only rebuild (a cap raised to a value the
account's content still exceeds, or the equality case of FU12), `content_allowed` turns false,
the opener returns `content: None`, `admit_content` pushes nothing and advances, the loop runs
off its list, `Exhausted` sets `content_to.checkpoint = indexed_to` and `cursor = None`, and
`suspended` stays false (it is set only on a `Publishing` pass). `is_current` is then true,
content requests are served, and the documents the cursor passed over the cap have no content
record: a silent hole where 4.2 promises coverage and 7.4 promises a refusal. Correction: when
`!limits.content_allowed`, the content block does not run (the cursor waits) and the pass sets
`content_to.suspended = true` through a step of its own (or the `Publishing` rule's condition
becomes "`Publishing` or a content cursor pending"), so the next `za_search_content_needs_
rebuild` decides; a unit test through `plan_pass` with `content_allowed: false` and a pending
content cursor asserting `content_cursor == None` and the suspension step.

#### FU7 — P3 — Two slips in the swap sketch (Task 8)

Plan line 2219. `cache.make_room(..)` names the `cache` bound at the top of `write_pass` (2192:
`let cache = server.get_cached_messages(account_id).await?`, the message cache); write
`server.inner.cache.za_search.make_room(..)`. And `index.content = None` is assigned inside the
`if let Some(content) = index.content.as_mut()` block that borrows it; compute the verdict in
the block and assign after it (FU4's helper makes both moot).

#### FU8 — P3 — `case_d13`'s "deliver bridge C ... its post-commit pass" (Task 13)

Plan lines 2880-2881. An LMTP delivery runs no post-commit pass (Task 9: APPEND, `Email/set`
create and `Email/import` do; a delivery is indexed by the next keyed request's pass). Name the
keyed write on the second connection (`Email/import` or APPEND of the bridge), or deliver and
run `run_search_pass` on a task of its own, parked at `SEARCH_SWAP_PAUSE`.

#### FU9 — P3 — Interface bookkeeping of revisions 5 to 7 (Tasks 1, 2, 4)

Plan line 907: "the six pause statics" lists seven. Lines 511 and 517: `clone_for_test`
(`#[cfg(feature = "test_mode")]`, a field-wise clone of `IndexDocument`, which derives no
`Clone`) is named only in step 3's parenthetical; add it to Task 1's Files line for
`search/mod.rs` and its Produces. Line 1215: `ContentRecord::encoded_len`'s purpose, "for the
loaded part's size and its charge", is revision 6's; revision 7 made `loaded_bytes()` the one
estimate (1649), so say "for Task 4's size tests" (`s42_document_cap_is_upstreams`, 1301, still
uses it) or drop the clause.

#### FU10 — P3 — Stale revision references after the D13 ruling (header, Execution, Task 19)

Plan line 151: D13 still ends "Three sentences of the spec for Jay (6.2, 6.3, the header)" after
"ruled by Jay on 2026-11-08"; say "ruled; the three sentences are revision 20's". Lines 180 ("the
spec at revision 18") and 3282 (the CLAUDE.md text: "revision 18") against the header's
revision 20 (74).

#### FU11 — P3 — `pl11_growth_across_accounts` counts three other parts after evicting one (Task 6)

Plan line 1772. After the first growth evicted one of the three other parts, two remain; "with
every other part held by a reader, the same growth is refused ... `content_used()` holds the
three other parts only" is right only on a fresh fixture (four parts at 200 KiB, the three others
held, 800 + 300 > 1024 → refused, 600 KiB left). Say the second half starts from a fresh cache,
or reload the evicted part first.

#### FU12 — P3 — The content-only rebuild's shape for `CapRaised` (Task 14)

Plan lines 2930 (the start: `content = None` with the chain's nodes unlinked, for every reason;
"runs on a node whose content part is loaded or loadable under its cap"; `CapRaised` when
`suspended && content.size <= account_max_mb · 1 MiB`), 2188 (`content_allowed` when `size <
cap`), 2187 (`plan_pass`'s `base` has `content: None`), 2915 (`za_search_content_rebuild_start
(..).await?`). Spec 5.4 (1361-1366) and 9.2 (2628-2630) describe the raised-cap rebuild as a
backfill of "the messages indexed while suspended", skipping on content presence; the plan drops
the whole chain and re-indexes every document's content, which meets the letter (nothing has a
content record after the drop) at the cost of re-indexing the pre-suspension content, and makes
the "loaded or loadable" sentence and the content-presence skip moot (`content_cursor_items`
reads `base`, whose `content` is `None`, so the skip is vacuous and the descending bound alone
excludes the items earlier passes staged). Name the choice as a plan call, or keep the chain for
`CapRaised` and say which loaded content part the skip reads. Two edges beside it: `CapRaised`'s
`<=` and `content_allowed`'s `<` overlap at `size == cap`, where a suspended account starts a
rebuild under the same cap (make them complementary); and the `?` on the start propagates
another node's assertion failure as `Err` from `za_search_pass`, which the hook traces as an
error, where `Lost` plus a reload is the pass's convention.

#### FU13 — P3 — Wording in the reconciliation rules (Tasks 8 and 14)

Plan line 2935: "`za_retained_bound`, the same max as step 1 of this rule": step 1 reads the
Email log; the max is the rule's intro (`bound = max(za_log_first_retained(Email), ..(Thread))`).
Line 2179: the gap blocks' pending condition (`base.manifest.reconciliation.is_some()`) is
stated for the content block and not for them, and `gap_stamp.expect(..)` must sit inside that
gate, or Task 8's passes (`PassSources::default()`) panic. Line 2934: say that `gap_stamp` is
`bound_now − 1` on a `BoundMoved` pass (FU3).

### Method and limits

A reading review of `git diff 794915249 aa594dc09` on the plan (378 lines, every hunk) and on the
spec (revision 20), of the Fable section the revision-5 edits answer and the two Codex sections
the revision-6 and -7 edits answer, and of the tasks the hunks touch where the new rule depends
on their text (Tasks 1, 2, 6, 8, 9, 10, 11, 13, 14, 15, the header, the plan calls, the case map
and Task 19's CLAUDE.md text). The traces the brief asked for were walked by hand against the
plan's own rules: a content-cursor pass, a gap pass, an `Abort` inside each, and both cursors
pending through `plan_pass` as revision 7 writes it; `case_f6_r23` and `case_h23` end to end,
with the shortcut, the FT4 start and `za_search_ensure_current`'s content-loss arm;
`second_truncation` on a marker over a partial `indexed_to`, a genuine second truncation and a
lossless bound move; the swap's growth admission in both outcomes against the next content
request, `is_current` and a concurrent reader; `case_d13` in both forms against tokio's
write-preferring `RwLock` and the one-shot pause slot; `r11`'s two forms against `fetch_verified`
and `parse_content_blob`'s contract; the above-watermark assertion of Task 1's test against the
codec. Source checked where a closure rested on it: `crates/store/src/search/codec.rs:60-130`
(`into_document`, the watermark and the marker), `crates/store/src/write/compress.rs:20`,
`crates/store/src/search/term.rs:95-115, 629-647` (`index_document`, `Document::encode`),
`crates/store/src/search/mod.rs:39, 176-184` and `document.rs:14-76` (`MAX_DOCUMENT_SIZE`,
`IndexDocument`'s derives and setters), `crates/utils/src/cheeky_hash.rs:19-80` (inline keys up
to 16 bytes), `crates/jmap/src/thread/get.rs:30-100` (`Thread/get` without ids lists every
thread), `crates/common/src/za/test_hooks.rs:418-460` (`take_for` is one-shot per account). Spec
passages read: the revision-20 diff, 4.2 (766-775), 4.3 (858-872), 5.4 (1300-1312, 1356-1380,
1405-1420), 9.2 (2622-2634).

Not done: no build, test or runtime probe; no subagents; the plan, the spec and every other file
are unchanged (this section is the only edit). The closures of FS1 to FS10, FR1 to FR14 and the
Codex findings were not re-verified beyond what these traces exercised; tasks the diff did not
touch were not re-read beyond the lines the findings cite. FU1's reachability rests on the
`NonPublishing` write rule at 2219 and the shortcut at 2187 as written; a `NonPublishing` pass
that also wrote a log entry would change the trace, and the plan says it writes none (R13).
