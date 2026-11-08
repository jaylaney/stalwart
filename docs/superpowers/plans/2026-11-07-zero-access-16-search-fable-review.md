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
