# Plan 16 Task 19: the remaining mutation rows

This report covers the last part of the Task 19 Step 2 mutation sample:

- the 16 plan rows that neither earlier campaign had run;
- follow-ups for the 55-draft rows whose named test an earlier test masked;
- a probe for M16.

It is independent of the earlier
[19-regression report](2026-11-07-zero-access-16-search-remaining-checks-review.md) and the
[55-draft report](2026-11-07-zero-access-16-search-mutations55-review.md). Together the three
cover every row of the plan's Step 2 table (M1–M79). Task 19 as a whole is not complete; see
[Gaps](#gaps-and-work-not-done).

## Summary

- **Rows in this campaign:**
  - **16 never-run rows:** M4, M5, M8, M13, M15, M19, M22, M23, M24, M25, M28, M31, M45,
    M46, M48, M54. They are 18 labels, since M5 and M28 have two variants each. No patches
    existed, so they were written fresh from the plan's row text, the source at `bae62db6f`
    and the earlier source-only locators. M31 was then written a second time (M31b).
  - **Follow-ups for masked rows:** M7, M21, M27, M33, M43a, M44, and M66's second branch.
    Each reuses its 55-package patch byte for byte, in a selection that removes the masking
    test's call.
  - **M16 probe.**
- **Selections:** whole `za::za_tests` runs on narrowed selections, as in the 55-draft run.
  Every executed selection had a passing correct-code baseline and a passing restored control.
  Two corrected selections first failed their baselines on correct code, so their rows were
  rerun on a third correction (see [Incidents](#incidents-and-deviations)).
- **Result for the 16 rows:**

  | Outcome | Rows |
  |---|---|
  | Caught at the named test | M4, M5a, M5b, M8, M13, M15, M24, M25, M28a, M28b, M45, M46, M54 (13 labels) |
  | Caught, but not by the named test, which passes | M22: detected twice, but its named F10 case passes |
  | Caught, but the named test is not reached | M19: two earlier tests of the same rule catch it before R7. M31: its unit and R22's held-set check catch it before R16/R20 |
  | Survived | M23, M48 |

- **Follow-ups:**
  - **Now demonstrated at the named test:** M7 (backlog case), M21 (R14), M27 (F3) and M33
    (R26). M27 was the 55-draft report's open "named catcher not demonstrated" finding.
  - **Named wire case doesn't detect the fault:** M44. R24/G2 passes when reached; the row stays
    caught by its two unit tests.
  - **Named case not reached:** M43a. An earlier composition case caught it in round one, and
    R3 in round two. The row stays caught by its unit test.
  - **M66's second branch** passed under the fault, as expected. It tests field 150, not 152.
- **M16 is not equivalent.** The probe shows no existing test reaches the state where M16's
  fault would change behaviour. But a narrow race can reach it; see
  [Finding 2](#findings).
- **M14** is recorded as caught by M50's unit tests, which apply the identical patch. R3
  still can't see it.
- **M29b** (tombstones in the padded section) was never drafted in any campaign and was not
  run here. It needs a type change to the hidden-section codec, not a bounded fault.
- **Restoration:** every mounted fault and selection edit was restored and hash-verified.
  Both checkouts ended clean at `bae62db6f`. The accepted checkout changed only in documents.

## Scope and screening limits

- **Brief:** Jay's instruction of 2026-11-11, "work on finishing the mutation sample".
  It took up section 1 of my plan from the same day: the never-run rows, the follow-ups,
  M14, M16 and M27, and the outcome note's table.
- **Follow-ups:** the 55-draft brief had excluded relaxation. These follow-ups remove whole
  dispatch calls only, never an assertion, in the disposable checkout. They are the plan's
  "masked catcher follow-ups". Each selection's correct-code baseline shows the removal leaves
  the remaining tests passing.
- **No production changes:** no fault, selection edit or probe was committed.
- **No test proposals:** none were written. M16, M23, M44, M48 and M14 are findings for Jay.
- **Screening:** none of this work was refused or interrupted. Patches were written and run in
  the main session, not by a delegated executor, as in the 55-draft run, because the earlier
  preparation subagents for some of these rows were stopped by screening.
- **Nothing pushed:** no mutation commit, push, pull request, deployment, real-account access
  or production-default change.

## Source identity and inputs

- **Source:** `bae62db6f08af27b1058c148a2fbe292f18635e9` in both checkouts, before and after.
- **Plan:** SHA-256 `dea47618…a484a0bd5`. **Spec:** `4f5815bb…3047cdd9`. Both are unchanged
  since the 55-draft run.
- **Toolchain:** rustc and cargo 1.99.0. The target directory is the checks worktree's own.
- **Packages** live in the ledger at
  `.superpowers/sdd/2026-11-07-zero-access-16-search/task19-mutations-final-2026-11-11/`.

  | Round | Preparation | `manifest.json` | `execution-plan.json` | Runner |
  |---|---|---|---|---|
  | 1 | `preparation/` | `11728382…aef2` | `cccdb1a3…cfc3f` | `runner2.py` `e5ef3f61…` |
  | 2 | `round2/preparation/` | `5ee82f96…20b1` | `d1fd1df6…9feb` | `runner3.py` `6a0085da…` |
  | 2b | `round2b/preparation/` | `23988a7d…aef2` | `e8589368…a2bf5` | `runner4.py` `fa8d83f6…` |
  | 2c | `round2c/preparation/` | `334d24f6…c1c9` | `e3538f79…aad0` | `runner5.py` `ee1778a1…` |

  Each runner checks its package hashes at every phase start. Each row's patch and selection
  diff carries its own hash in the manifest.

## What I inspected

- **The plan:** Task 19 Step 2's table and procedure (`…-16-search.md:4004-4090`). Also the
  Step 0 catchers for M45 and M47.
- **The earlier records:**
  - the two earlier reports;
  - the source-only locators (`task19-mutation-locators.md`, recorded at `070c1d3a`);
  - the catcher assessment (`task19-catcher-assessment.md`; its M24 construction is the one
    used here);
  - the stopped-preparation notes.
- **Production source** at each row's site:
  - `pass.rs` (Planner, `read_records`, `read_sources`, `finish`, the pass driver and its
    shortcut);
  - `write.rs` (`next_manifest`, `seal_thread_delta`, `stage_blobs`, `build_batch`,
    `commit_and_swap`);
  - `view.rs`, `rebuild.rs`, `compact.rs`, `changes.rs`;
  - the JMAP changes page and `Email/set`;
  - `load.rs`'s refresh of `scanned_to`, and `ManifestWrite::serialize_with_ids`.
- **Every named fixture** and the dispatch order around it, in `index.rs`, `threads.rs`,
  `task13*.rs`, `races.rs`, `history.rs` and `leak_search.rs`. Two dispatch details matter
  below:
  - the threads module's R16/R20 and related recovery cases run from the index module's
    `test_recovery`;
  - R17 runs in the threads module's compaction group.

## Method

The method is the 55-draft run's, with the following changes.

1. **Patches.**
   - Each row is an exact replacement list (`specs.py`). Every old string must occur once.
   - `build.py` applies the list in the disposable checkout, captures `git diff` and records
     before and after hashes. It restores with `git checkout --`, then proves the stored
     patch applies forward and reverses to the original bytes.
   - All 18 new patches, the probe and M31b passed `cargo check --locked` before any run:
     the email lib and its tests, or the jmap lib.
   - Two patches compile with one harmless warning each: M4 (an unneeded `mut`) and M45 (an
     unused import).
2. **Realizations that needed choices.** These are the rows where the plan's one-line wrong
   change maps to more than one edit.

   | Row | Realization |
   |---|---|
   | M4 | The pass's step-0 Email state is passed to `write_pass` and written as the row's change id, with the commit-time fill turned off |
   | M5a | The manifest is still read, but its hash is not compared |
   | M13 | A singleton-only pass keeps its records and revision but writes no witness |
   | M22 | The `(S, E]` range is read with `Store::changes` and fed in as one folded record |
   | M24 | The catcher assessment's construction: retained deletions applied before pending cursor items, checkpoint untouched |
   | M25 | The message cache read at step 0 is passed to the planner, which skips a deletion the cache still lists |
   | M28a | An update-only delta is a manifest-only pass and seals a thread blob |
   | M28b | Cursor movement with nothing staged gives no manifest-only write |
   | M31 | First version: newest-first order. M31b: newest first, with the lowest admitted id minus one becoming the cursor's upper bound |
   | M45 | Compaction uses the Email log's first retained change alone; admission is unchanged |
   | M54 | Created ids are mapped through the input view before it is released |

3. **Selections.**
   - **Frozen selectors from the 55 package**, reused byte for byte: leak, index and paging.
   - **Derived selectors**, built from those, remove whole dispatch calls in the test files
     and nothing else. Their removal lists are in each manifest's `removed_lines`.
   - **Attribution anchors** are function line ranges computed from each selection's own file
     content, because removing lines shifts later ones.
4. **Unit commands:** four email unit tests, run as observers. Each had a listing, a
   baseline, a fault run and a control.
5. **M16 probe.** A server-side panic fires if every other condition of the in-memory
   shortcut holds while an identity cursor is pending. That is exactly the state in which
   M16's fault would change behaviour. The probe expects a pass.
6. **Load gate.** Integration runs wait until the 1-minute load average is at or below 10.
   Round 2b onward use 20, with Jay's agreement (see [Incidents](#incidents-and-deviations)).
7. **Retries.** Baselines and controls get one recorded retry. A fault run whose first failure
   is a helper timeout gets one recorded retry. No retry was needed for a timeout.

## What ran

| Round | Phases | Runs | Faults mounted / restored | Wall time | Load waits |
|---|---|---|---|---|---|
| 1 | units; leak, index, paging, three masked-row selections | 61 | 31 / 31 | 156 min | 12, 1,830 s in total |
| 2 | four focused selections | 17 | 5 / 5 | 33 min | 4, 780 s |
| 2b | two corrected index selections | 7 | 1 / 1 | 9.5 min | none |
| 2c | one corrected index selection | 6 | 3 / 3 | 18.6 min | none |

Every baseline and control that ran on an executed selection passed: 4 unit commands, 6
round-one selections, and 3, 1 and 1 selections in rounds 2, 2b and 2c. Two
corrected selections failed their own baselines on correct code, focus-index and
index-narrow. Their rows are recorded `NOT_EXECUTED` there and ran in round 2c.

## Results by label

Key:
- **K:** fails at an assertion in the named test, with values showing the fault.
- **early:** an earlier test catches the same fault first, and the named test isn't reached.
- **passes:** the named test runs and passes under the fault.

Line numbers refer to the selection's own file.

| Label | Plan site | Round 1 | Later rounds | Conclusion |
|---|---|---|---|---|
| M4 | revision row change id | early: `case_import_response_state_includes_publication`, `published > insertion` fails | K backlog case: 2 of 4 destroyed singleton ids (`history.rs:176`) | Demonstrated |
| M5a | validation read | K `case_r2_cross_node`: merged id missing from `Email/get` (`races.rs:227`, via `assert_email_get_publication`) | — | Demonstrated |
| M5b | state check | K `case_r2_cross_node(true)`: served `Email/get` where an error is required (`races.rs:178`) | — | Demonstrated |
| M8 | changes range end | early: `case_query_composition`, H removed twice | K R12 paging: `Intermediate` after the last page instead of `Exact(14)` (`history.rs:206`) | Demonstrated |
| M13 | publishing rule | K `membership`: "expected one Email publication after 9, got []" (via `assert_search_publication`). Observer unit `the_witness…`: `None` vs `Some(7)` | — | Demonstrated |
| M15 | fresh epoch | early: `case_zero_row_epoch`, `rebuilt.epoch > previous` fails | round 2c: K R3, `Email/changes` from the latest pre-loss state served instead of `cannotCalculateChanges` (`index.rs:3784`) | Demonstrated |
| M19 | links in the manifest's batch | early: `case_lost_assertion`, an extra permanent 65,707-byte link survives `Lost` | round 2c: early again at `case_expired_hold_does_not_publish`, permanent links changed after an aborted pass (`index.rs:2152`) | Caught by two earlier tests of the same rule; named R7 not reached |
| M22 | unfolded overlay | early: `case_retention_advances_between_bound_checks`, `updated` [] vs [U] | named F10 `case_deletions` **passes**; then `case_thread_page_pins_members`: created [] vs [X] (`history.rs:404`) | Detected twice; named F10 case doesn't detect this realization |
| M23 | strict order within a record | **passed**; observer unit passed | — | **Survived** (Finding 3) |
| M24 | cursor evidence | K R16/R20 non-residual form: `[[270],[272,273]]` vs joined (`threads.rs:707`) | — | Demonstrated |
| M25 | unconditional tombstone | K R17: "persisted tombstone" missing (`threads.rs:1094`); observer unit passed, since it has no cache | — | Demonstrated |
| M28a | empty-staging rule (blob) | early: `case_content_wait_budget`, 6 vs 5 committed passes | K F4 block: pass after publication `Advanced{published: false}` vs `Current` (`index.rs:139`, the check in `index::test` right before F4) | Demonstrated |
| M28b | empty-staging rule (cursor) | K R22: `after.cursor.is_none()` fails (`index.rs:3864`) | — | Demonstrated |
| M31 | cursor order | first version: observer unit K ([9, 5] vs [5, 9]). R10 failed first on a progress side effect (48 vs 200 queried), which the locator warned against | M31b, round 2c: R10 passed, so progress is kept. Then R22's first `assert_held(…, 48)` failed: the newest 48 documents were held, not the oldest (document 204, via `assert_held`) | Caught by its unit and by R22's held-set check; named R16/R20 not reached |
| M45 | shorter log's bound | K Step 0 case: "compaction retained the expired singleton" after cold reload (`index.rs:4885`) | — | Demonstrated |
| M46 | counter comparison | K G3: `serverUnavailable` instead of `Email/get` on an empty account after an epoch (`races.rs:98`) | — | Demonstrated |
| M48 | skip of a held insert | **passed** | — | **Survived** (Finding 4) |
| M54 | mixed Set response view | K: created id `beaaaaal` vs `yaaaaal` (`races.rs:471`) | — | Demonstrated |
| M16 probe | shortcut reachability | passed in index and paging; no probe panic in either log | — | No existing test reaches M16's state |
| M7 | history direction | parity removed: early again at `case_query_composition` (`composition.rs:274`) | K backlog case (`history.rs:176`) | Named test now demonstrated |
| M21 | existence rule | parity and backlog removed: K R14 `case_deletions`, a transient listed in `created` (`history.rs:282`) | — | Named test now demonstrated |
| M27 | timeout as loss | warm-content case removed: K F3 "the timeout changed the index manifest" | — | Named test now demonstrated; closes the 55-draft finding |
| M33 | skipped checkpoint | PL4 removed: K R26 via `advanced`, `Current` vs `Advanced(published=false)` | — | Named test now demonstrated |
| M43a | member rewritten at merge | early again at `case_query_composition` | early at compaction R3: 1 vs 2 (`threads.rs:947`) | Named test not reached; caught by its unit test and two wire tests |
| M44 | reconciliation attribution | PL6 removed: R24/G2 ran and **passed**; whole selection passed | — | Named wire case doesn't detect it; caught by its two unit tests (55-draft run) |
| M66 branch 2 | field-152 absence in unlink | branch 1 removed: branch 2 (vault) passed | — | As expected; M66 is caught by branch 1 |

The route-by-route views with calling functions are `run/adjudication.txt` and the same file
under each round directory.

## Findings

1. **Rows caught first by an earlier test.** M4, M7, M8, M15, M19, M22, M28a and M43a each
   failed first at an earlier test that checks the same rule. The focused rounds then reached
   the named test for M4, M7, M8, M28a and M15. M19, M31 and M43a still met an earlier test of the same rule each time, and I stopped there. Some tests repeatedly
   mask others:
   - `case_query_composition` masks the history cases;
   - pass-counting tests mask F4;
   - `case_zero_row_epoch` masks R3;
   - `case_lost_assertion` masks R7.

   For a future suite reorder, those named tests could move earlier.
2. **M16 is reachable through a race.** The shortcut's cursor check stops being redundant
   only if the resident `scanned_to` reaches the rebuild's epoch while a cursor is pending.
   That can happen:
   1. A pass reads the pre-rebuild manifest.
   2. The rebuild commits.
   3. The same pass reads the rebuild's own witness record, takes the no-change branch, and
      raises the stale index's `scanned_to` without a compare-and-swap (`pass.rs:884-891`).
   4. A later reload keeps that value (`load.rs:1216`).

   From then on an idle node returns `Current` with the rebuild pending if the cursor check
   is missing. The probe confirms no fixture reaches this state. Covering it needs a test-mode
   pause between the manifest read (`pass.rs:692`) and the record read (`pass.rs:818`),
   which no hook provides today. That is a decision for Jay.
3. **M23 survives.** Its named case asserts `insert < deletion`: N's insert and D's deletion
   are deliberately in separate records. The fault reorders only within one record. No test
   builds a record that both inserts a reply and deletes its reference, for example a single
   `Email/set` create and destroy. The observer unit `a_partial_record…` also passes.
4. **M48 survives.** The "repeated delta" case redelivers an identical message, which
   creates a new document id. So the pass never meets an insert for a document the index
   already holds. In normal flow that state is also prevented upstream:
   - rebuild cursors snapshot documents and state together;
   - reconciliation's gap inserts exclude anything the retained log shows.

   The guard is defence in depth, with no unit or wire test.
5. **M44's named wire case doesn't detect it.** With PL6 removed, R24/G2 runs and passes
   under the fault. PL6 shares R24's account, so the removal changes R24's history slightly;
   the baseline passed with the same removal. The row stays demonstrated by its units.
6. **M22's named case doesn't detect this realization.** F10 `case_deletions` passes when
   the stream is fed one folded record. Two other tests catch it. A different folding
   adapter might trip F10; that is not established.
7. **M14** is recorded as caught by M50's unit tests, which apply the identical patch. R3
   still needs a nonzero retained bound to carry the plan's obligation, as the 55-draft report
   found.
8. **M31's first patch broke progress, not just order.** Newest-first order with forward
   `next` progression skips older items past one pass. R10 caught that (48 vs 200) before
   R16/R20 ran. M31b keeps progress exact; see its row.
9. **Test fragility seen in a corrected selection.** With the index module's early calls
   removed, `case_deletion_only_pass_writes_no_entry` failed on correct code. The account
   reached a different thread-segment count, so its deletion-only pass hit a compaction (16
   blobs to 1). The case assumes no compaction at that point without pinning the segment
   count. It's harmless in the canonical order, but worth a guard if the order changes.

## Incidents and deviations

- **Package rebuilt before any run.** I added the M16 probe after the first build. Nothing
  had run against that build, so I rebuilt it. The 18 patches are byte-identical to the
  compile-checked set.
- **focus-index (round 2) failed its correct-code baseline twice:** Finding 9, at
  `index.rs:2131`. Its four rows were not executed there.
- **index-narrow (round 2b) failed its correct-code baseline twice.** Removing both
  zero-row calls left the account without mailboxes after the `destroy_all_mailboxes` between
  them, so R3's `SELECT INBOX` failed. The second zero-row call had been recreating them.
  index-narrow-b (round 2c) also removes that cleanup call and keeps `mailbox_get`.
- **Load limit raised to 20** from round 2b, with Jay's agreement.
  - The only load-induced failure in any campaign happened at about 59 (55-draft run).
  - Loads of up to about 22 delayed runs, and one burst reached 99. Runs waited rather than
    started at those levels.
  - Runs started at or below 10 in rounds 1 and 2, and at or below 20 from round 2b.
- **No runner was stopped by signal.** No abort or environmental retry occurred.

## Gaps and work not done

- **Not executed:** M29b. Faithfully storing tombstones in the padded hidden section needs a
  hidden-section codec change: a new record kind, a `HiddenSection` field, every constructor
  and the loader. That is not a bounded fault.
- **Not reached:**
  - M43a's named folded-alias case;
  - M64's injected-error branch (reaching it means editing inside the case);
  - M19's named R7 case and M31's named R16/R20 case.
- **Not run:** full-suite or FileSystem-blob fault runs; clippy and rustfmt; non-RocksDB
  stores.
- **Findings for Jay:** M14, M16, M23, M44, M48, and M22's F10.
- **Still-pending Task 19 gates**, unchanged by this work:
  - Docker automation and directory rows in CI;
  - native clients;
  - the separate fresh-store gate;
  - networked-store timing;
  - physical multi-node verification;
  - `CLAUDE.md` Step 3. Its release 7 status still says the 55 drafts' behavioural runs are
    pending.

## Evidence

Root: `.superpowers/sdd/2026-11-07-zero-access-16-search/task19-mutations-final-2026-11-11/`
(git-ignored).

| Item | Contents |
|---|---|
| `preparation/`, `round2*/preparation/` | `manifest.json`, `execution-plan.json`, `patches/`, `selections/`, `mutants/` (mutant file copies), `specs.py` and `build*.py` |
| `run/`, `round2*/run/` | `journal.jsonl` (every mount, restore, run, load wait, with hashes), `summary.tsv`, `results.json`, `runs/<label>/` (`command.json`, `output.log`, `result.json`, `applied.diff`), `adjudication.txt` |
| `tools/` | `runner2.py`–`runner5.py`, `build.py`–`build4.py`, `specs.py`, `compilecheck*.py`, the compile-check logs |
| `evidence.sha256` | SHA-256 of every file above |
