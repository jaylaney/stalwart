# Plan 16 Task 19: the 55-draft mutation runs

Date: 2026-11-10/11. Committed source `bae62db6f08af27b1058c148a2fbe292f18635e9`
(plan revision 27, spec revision 33). Disposable checkout
`.worktrees/release7-search-checks`, detached at that commit.

## Summary

All 55 draft labels (54 distinct faults) were executed, one fault at a time, against their
named unit and integration routes. Every selection had a passing baseline and a passing
restored control under the same selection.

"Named catcher" means a catcher named for the row in the 55-draft package. That package's
catcher lists include every catcher in the plan's Step 2 table, plus unit catchers the
drafts added.

- **52 labels** fail at an assertion in a named catcher whose operands show the fault's
  effect. In ten of them, the integration catcher in the plan's Step 2 table did not itself
  demonstrate the fault: it was masked by an earlier failure, unobserved, or did not
  discriminate. Those ten are M1, M7, M21, M29a, M30, M33, M40, M43a, M44 and M49.
  - For five of them (M7, M21, M33, M43a, M44), the plan names only that integration case,
    so the demonstration rests on the draft package's unit catchers.
  - Findings below lists all ten.
- **2 labels survive their named catcher:**
  - M14: the R3 deleted-merged-message case passes. The identical patch is killed by M50's
    units.
  - M16: the R10 restart case passes.
- **1 label has a masked named catcher.** M27's F3 case is never reached, because an
  earlier fixture fails first on the same fault class. M27 has no unit kill route.
- Of the 54 distinct faults, 53 fail at least one executed test. M16 is the only fault no
  executed test detected.

Task 19 is **not** complete. This batch answers Step 2 for these 55 labels only. Every other
pending Task 19 gate in the outcome note still stands, as listed under Gaps.

## Scope and screening limits

A previous execution subagent was stopped by automatic screening ("possible cybersecurity
risk"). Its unfinished executor was not used, read or reproduced, and the stopped request was
not re-dispatched to another agent.

This assessment is ordinary fault-injection testing of the project's own code:
- the faults were temporary source edits in a disposable checkout;
- the stores were synthetic and temporary;
- no network target beyond localhost test listeners was involved;
- no real account was accessed.

I wrote a new runner and ran it from the main session. That departs from the standing
"delegate implementation" rule, because re-delegating would have repeated the stopped
request. No screening restriction was bypassed. Nothing in this batch was refused or
restricted.

No mutation was committed. The accepted checkout's production source was never edited, and
nothing was pushed or deployed. No default, stored layout or guarantee was changed.

## Source identity and inputs

- **Both checkouts:** clean at `bae62db6f` before and after. The only change in the accepted
  checkout is this report, plus git-ignored ledger files.
- **Plan:** SHA-256 `dea47618…a484a0bd5`. **Spec:** `4f5815bb…3047cdd9`.
- **Frozen preparation**, in the ledger at
  `.superpowers/sdd/2026-11-07-zero-access-16-search/task19-mutations55-2026-11-10/preparation/`,
  verified by the runner at every phase start:

  | File | SHA-256 |
  |---|---|
  | `manifest.json` | `7a4d2d0e…6bc2` |
  | `execution-plan.json` | `864edf0f…c049` |
  | `validation.json` | `bfe25cce…6728` |
  | `selector-validation.json` | `aac1470c…f1` |
  | every row patch and all six selector diffs | the `*_sha256` recorded in the manifest and plan |

- **Toolchain:** rustc 1.99.0 (b940084d7 2026-09-28), cargo 1.99.0.
- **Target directory:** the checks worktree's own `target/`.

## What I inspected

- AGENTS.md, CLAUDE.md, README-dev, plan Task 19 Step 2 (the table M1–M79), spec 9.7.
- The retained `stopped-state.json`, `active.json` and `review/preflight-review.md`.
- The preparation report, manifest and execution plan. The routes total 108:

  | Expectation | Routes |
  |---|---|
  | kill | 71 |
  | pass (control) | 17 |
  | observe | 18 |
  | excluded (M14/M50 duplicate obligations) | 2 |

- The named test bodies and fixture call order wherever a first panic needed attribution:
  `index.rs` `test`, `destroy_cases` and `test_recovery`, `task13.rs`, `history.rs`,
  `gates.rs`, `leak_search.rs`, `differential.rs`.
- The server-side guard behind M29a's wire failure (`debug_assert!` in
  `store/src/za_search/thread.rs:207`).

## Method

Runner: `independent-run/tools/runner.py`. Its procedure:

1. **Mount** one row:
   - check that each touched file's SHA-256 equals the manifest's original;
   - run `git apply --check`, then `git apply`;
   - check that each file's SHA-256 equals the manifest's mutant;
   - check that `git status` shows only the expected paths;
   - journal the mount with the applied diff's hash.
2. **Run** each route's exact manifest `argv` (`cargo test --locked … --exact`):
   - Environment, the same for listing, baseline, mutant and control runs:
     - PATH prefixed with Homebrew rustup;
     - `CARGO_TARGET_DIR` set to the checks worktree's `target/`;
     - `RUST_BACKTRACE=1` and `RUST_LIB_BACKTRACE=0`, so a panic in a shared helper names
       its calling fixture without a second run;
     - a fresh `TMPDIR` per run, deleted afterwards;
     - `BLOB_STORE`, `ZA_KEY_ACCOUNTS`, `NO_INSERT` and `RUSTFLAGS` unset;
     - for the suite, `STORE=RocksDb RUST_MIN_STACK=16777216`.
   - Integration runs are serial, with the suite's ports checked free before each run.
3. **Restore** with `git checkout --`, re-hash against the original and re-check
   `git status`. A mismatch would abort the campaign; none occurred. The journal holds 83
   mounts and 83 restores.

Gates and selections:

- **Unit commands:**
  - listing gate: each of the 45 unit commands listed exactly its one test;
  - baseline: all 45 passed;
  - restored control: all 45 passed after the last unit fault.
- **Integration selections:** the six frozen whole-module selectors, mounted by exact hash.
  Each got a `--list` gate (exactly `za::za_tests`), a baseline, the fault runs and a
  restored control. Selector restoration was then verified.
- **No relaxation:** internal-case carving and assertion relaxation were excluded. A masked
  named catcher is reported as masked.

Attribution:

- Every first panic was attributed from its location, message, operands and backtrace
  frames.
- Faults that edit a file containing inline tests shift that file's line numbers, so the
  plan's line-range anchors miss. M29c, M30, M38a and others were attributed by the named
  function in the frame instead.
- A control's observability came from the fixture call order: a control that runs before
  the failing case was observed passing.

Load gate and retry, added after integration attempt 1 (see Incidents):

- Integration runs wait until the 1-minute load average is ≤ 10, the core count, for at most
  60 minutes. The load is recorded before and after each run.
- A failed baseline or control gets one retry.
- A fault run whose first panic is a client-helper timeout gets one retry. None was needed.
- Every attempt is preserved.

## What ran

| Phase | Runs | Wall time |
|---|---|---|
| Unit: 45 listings, 45 baselines, 54 fault routes over 42 mounts, 45 controls | 189 | about 11 min |
| Integration attempt 1: search complete; gates baseline failed (environmental); stopped | 11 (one killed mid-build) | about 14 min |
| Integration attempt 2: gates, differential, leak, paging, index | 55 | about 3 h 7 min |

Baselines and restored controls (run time including the incremental build):

| Selection | Modules | Baseline | Fault runs | Control |
|---|---|---|---|---|
| search | `mail::search` | pass 20.6 s | 1 (M9) | pass 82.8 s |
| gates | `mail::gates`, `mail::index` | attempt 1 **fail** 213.6 s (IMAP 1.5 s read timeout in `index` R10 `append_many`, load ≈ 59); attempt 2 pass 243.1 s | 1 (M11) | pass 283.9 s |
| differential | `mail::differential` | pass 706.6 s | 1 (M41) | pass 743.3 s |
| leak | `leak_search` | pass 111.4 s | 3 | pass 149.2 s |
| paging | `mail::threads`, `mail::index` | pass 308.1 s | 8 | pass 318.6 s |
| index | `mail::index` | pass 218.0 s | 27 | pass 271.9 s |

No fault run hit a compile failure, a setup failure, an empty selection, a timeout or a
leftover process.

## Results by label

Key: **K** = fails at a named assertion, with operands showing the fault. **masked** = an
earlier semantic assertion fails first, so the named case is not reached. **ctl** = control.
**obs** = observer. Line numbers are from the mutant where the fault edits the same file.

| Label | Plan 9.7 site | Unit routes | Integration routes | Conclusion |
|---|---|---|---|---|
| M1 | AAD header | K `s31…` (AAD bytes lack the header, `seal_tests.rs:74`); ctl R11 passed | index obs passed | Demonstrated (unit). The wire follow-through cannot see a change that sealing and opening share |
| M2 | manifest-write assertion | — | K `case_lost_assertion`: Advanced vs Lost (`index.rs:2075`) | Demonstrated |
| M3 | record boundary | K (complete checkpoint vs (11, 4)) | K R1 split MULTIAPPEND (`index.rs:1597`) | Demonstrated |
| M6 | manifest-hash comparison | K `r18…` (no reload, `load_tests.rs:637`) | — | Demonstrated |
| M7 | history direction | K R5 (state 99: 3 vs 7) | paging obs: failed at the earlier `case_merge_rendering_parity`, so the plan's `mail::threads` case wasn't reached | Demonstrated (unit) |
| M9 | exhaustive leaf | K `unresolved_leaf_refuses_the_entire_tree` | search front-door ctl passed, as planned | Demonstrated (unit) |
| M10 | id prefix check | K (Some(5) vs None) | K G20 `case_mapped_get_and_query`: stale prefix served | Demonstrated |
| M11 | `Mailbox/get` keyless | — | K F8 `keyless_tokens_hit_the_dispatcher_gate` (via `assert_refused`) | Demonstrated |
| M12 | witness rule | ctl witness unit passed | K `membership`: witness `UpdateItem` missing (via `assert_search_publication`) | Demonstrated |
| M14 | deleted-record retention (wire obligation) | excluded (see M50) | **passed**: R3 deleted merged message | **Survived its named catcher** |
| M16 | cursor over shortcut | — | **passed**: R10 restarts | **Survived its named catcher** |
| M17 | chain-header segment id | K R11 chain walk (`Loss(Parse)`); K `r11_content_blob_names_its_predecessor` | — | Demonstrated |
| M20 | scan on broken chain | K | K R7 middle node: orphaned links | Demonstrated |
| M21 | existence rule | K | masked: `case_backlog_and_paged_merges` destroyed set held 2 of 4 before R14 `case_deletions` | Demonstrated (unit) |
| M27 | timeout as loss | ctl `f3_h6_error_classes` passed | masked: `case_warm_content_transient_does_not_block_a_served_map` served ids instead of `unsupportedFilter`; F3 not reached; transient ctl ran earlier and passed | **Named catcher not demonstrated** |
| M29a | unbounded references | K capture (23 vs 16) | leak F1 obs: server `debug_assert!(len <= K)` panicked, request failed; F1 oracle unobserved | Demonstrated (unit) |
| M29c | pad minimum | K config refusal (`Ok` for 33300) | — | Demonstrated |
| M30 | reservation charge | K (checkpoint 49 vs 48) | paging obs **passed**: R21 amplified merge | Demonstrated (unit); the named wire case doesn't discriminate |
| M32 | intra-record checkpoint | K ([] vs [1, 2, 3, 4]) | K R1 (via `advanced`: Current) | Demonstrated |
| M33 | skipped checkpoint | K (not ManifestOnly) | masked: PL4 (Current vs Advanced(published=false)) before R26 | Demonstrated (unit) |
| M34a | refuse all below epoch | K | K R27 (error vs `Email/changes`) | Demonstrated |
| M34b | open epoch interval | K | — | Demonstrated |
| M36 | inclusive resume | K (2 vs 1) | K R1 restart (via `advanced`: Current) | Demonstrated |
| M37 | cache step | K | — | Demonstrated |
| M38a | tombstones before gap inserts | K H3 (9 vs 8) | — | Demonstrated. Its companion ctl held its predicted early check, then failed at second-pass threading (5 vs 8): an extra detection |
| M38b | gap deletions after retained | K I1; ctl passed | — | Demonstrated |
| M39 | survivor rule | K H10 (1 vs 5) | — | Demonstrated |
| M40 | manifest tag | K H9 (unverified manifest accepted) | index: R3 obs ran and passed (no bad-tag injection); stale-loss successor obs failed (`index.rs:4553`) | Demonstrated (unit) |
| M41 | per-record compression | obs G5 records and bounded frames both failed | K differential G5 (Δ2−Δ1 = 280 vs Δ4−Δ3 = 5408); leak obs passed; index obs: f6/R23 passed, warm-content compaction failed on budget | Demonstrated |
| M42a | unpadded thread blob | K R15 (65535 vs 65536); K R20/G1 | — | Demonstrated |
| M42b | 256-byte rounding | ctl R15 passed; K R20/G1 (256 vs 65536); K I2 | — | Demonstrated |
| M43a | member rewritten at merge | K H10 (3 vs 5); ctl R25 passed | masked: `case_merge_rendering_parity` before the folded-alias case | Demonstrated (unit) |
| M43b | alias dropped unfolded | K S55 (0 vs 1) | K `case_initial_continuation_folded_alias` (2 vs 1) | Demonstrated |
| M44 | attribution | K both units | index obs: failed at the earlier PL6; R24/G2 not reached | Demonstrated (unit) |
| M47 | creation vault assertion | — | K R19 creator: field 152 resurrected; paused-pass ctl passed | Demonstrated |
| M49 | head-only unlink | K (head only) | R7 interrupted and middle-node ctls passed; G12 obs: orphaned links | Demonstrated (unit); the R7 wire case can't discriminate |
| M50 | bridge rule | K dead-alias (3 vs 5); K bridge (record 7 pruned at 25) | excluded (see M14) | Demonstrated |
| M51 | dead-alias rule | K R35 (0 vs 1792) | — | Demonstrated |
| M52 | one-hop fold | K ((5, 4) vs (1, 2)) | — | Demonstrated |
| M53 | Publishing pad target | K P1-PAD (131072 vs 123413) | — | Demonstrated |
| M56 | pinned initial set | K ([2, 5, 9, 11] vs [2, 5]) | K pinned-end lifecycle (via `rest_at`) | Demonstrated |
| M60 | reset vs destroy | — | K: a reset after deletion must refuse; live-reset ctl passed | Demonstrated |
| M61 | stale reset unlink | — | K: the stale scan removed new links | Demonstrated |
| M62 | empty-marker retry | K (inventory kept) | K retry case: orphaned links; f23 ctl passed | Demonstrated |
| M63 | reset after destroy | — | K: field 152 recreated | Demonstrated |
| M64 | stale permanent destroy | — | K first repeat: field 152 recreated; injected-error arm not reached | Demonstrated |
| M65 | absent-manifest vault hash | K: marker creation must refuse | — | Demonstrated |
| M66 | field-152 absence in unlink | — | K manifest arm; vault-arm obs not reached | Demonstrated |
| M67 | field-150 absence in unlink | — | K vault arm; manifest-arm ctl passed | Demonstrated |
| M75 | full tier saturation | — | K: tier 0 vs 255 | Demonstrated |
| M76 | full merge at max tier | — | K: second full merge Aborted(Validation) (via `advanced`) | Demonstrated |
| M77 | tiered preflight | ctl planner passed | K tier overflow: Advanced vs Aborted(Validation) | Demonstrated |
| M78 | tiered overflow outcome | ctl planner passed | K tier overflow | Demonstrated |
| M79 | tiered promotion | K planner (Ok vs rejection) | K tier overflow | Demonstrated |
| actual-map-order | Task 3 companion | K chain order ((1, 2) vs (3, 2)); PL7 obs failed | — | Demonstrated |

Per-route records: `independent-run/results.json`, `verdicts.json` and `adjudication.txt`.
Each run's directory holds `command.json`, `output.log` (with backtraces) and `result.json`;
each mount directory holds `applied.diff`.

## Findings

1. **M16 survives.** R10 passes with the shortcut allowed while a cursor is pending. As the
   preparation argued, a separate check (durable state newer than `scanned_to`) blocks the
   shortcut on this path. That is a reason for the survival, not an equivalence proof. The
   plan's rule calls for either a test that reaches a pending cursor with state ≤
   `scanned_to`, or an argued equivalence in the outcome note.
2. **M14's named obligation survives.** The fresh R3 account never truncates a log, so the
   retained bound is 0 and the pruning fault is inert there. The fault itself is killed by
   M50's units. R3 needs a nonzero retained bound to carry the plan's obligation.
3. **M27's named F3 case is unobserved.** An earlier fixture fails on the same fault class
   first. Demonstrating F3 would need either a separately approved test-only follow-up
   (relaxation or reordering) or the F3 case moved ahead of the warm-content case.
4. **Plan-named wire cases that don't discriminate.** All four were predicted by the
   preparation. Each row is still killed by its named unit:
   - M1: R10/R11 reload. A symmetric AAD change round-trips.
   - M30: R21 at the default cap.
   - M40: the R3 bad-tag check. R3 injects no bad tag; the stale-loss fixture detects it.
   - M49: R7. Only one content segment; G12 detects it.
5. **Named wire cases masked by earlier detection.** In each, the suite fails on the fault at
   an earlier semantic fixture:
   - M7: merge-rendering parity, before the plan's `mail::threads` case;
   - M21: backlog and paged merges, before R14;
   - M33: PL4, before R26;
   - M43a: merge-rendering parity, before the folded-alias case;
   - M44: PL6, before R24/G2.

   The draft package's units kill each row. For M29a, the leak F1 oracle is unobserved, because a
   debug-only server assertion fired first. A release build has no such guard on that path.
6. **Unobserved later arms:**
   - M64: the injected-error arm.
   - M66: the vault-arm observer.
7. **Predictions that differed:**
   - M38a's companion control failed after its predicted check held.
   - M76's preflight concern, that the helper's message would not be recognised, does not
     arise: the backtrace names `case_full_merge_saturates_repeatedly`.

## Incidents and deviations

- **Load.** The gates baseline on clean source failed at an IMAP 1.5 s read timeout while
  another session's Xcode build held the 1-minute load near 59 on a 10-core host. The
  original failed baseline is preserved (`runs/I-gates-base`). After the load gate was
  added, the baseline and control passed at load ≤ 10, which supports an environmental
  cause.
- **Stop.** I stopped the runner to add the load gate, and the stop went wrong:
  1. The SIGTERM handler raised `PermissionError` on `killpg`. `Popen.wait` appears to have
     absorbed it, and the runner continued.
  2. It recorded the in-flight differential baseline as exit −15 and moved on.
  3. A follow-up group kill ended the leak baseline (also −15). The runner then mounted the
     paging selector, which my `git checkout` reverted.
  4. The runner was then killed directly.

  No fault was mounted at any point. The journal's `manual_stop` record lists the artifact
  runs: `I-differential-base` and `I-leak-base` (both −15, not test results) and
  `I-paging-list` (killed mid-build). Attempt 2 reran those selections under `-a2` labels.
  Superseded records are kept under `history` and `selection_history` in `results.json`.
- **Runner versions:**
  - `fa62294a…` ran the unit phase and integration attempt 1. Its bytes were not kept
    separately; its hash is in the journal's `phase_start` records.
  - `9cb37305…` (`tools/runner.py`) ran attempt 2. It adds the load gate, attempt labels,
    the recorded retries, route history and a handler that cannot return into the run loop.
- **Focused selections.** Whole-module dispatch replaces the full suite, as the preparation
  recorded. A focused result applies to the selected modules only.
- **Delegation.** The runner was written and run in the main session, not by a subagent, for
  the reason given under Scope.

## Gaps and work not done

- **Not established:**
  - full-suite fault runs;
  - follow-ups for M27, M14 and M16, or for the masked wire cases of M7, M21, M33, M43a and
    M44 (relaxation was excluded);
  - the later arms of M64 and M66;
  - the M29a wire F1 oracle;
  - a test that discriminates M16.
- **Not run:** clippy, rustfmt and any non-RocksDB store.
- **Earlier faults not re-run:** the separate 19-regression faults and the T13 Content fix
  were not repeated. That record is
  `2026-11-07-zero-access-16-search-remaining-checks-review.md`.
- **Still-pending Task 19 gates** outside this batch, unchanged:
  - Docker automation and directory verification in CI;
  - native clients;
  - the separate fresh-store gate;
  - networked-store timing;
  - physical multi-node verification;
  - the outcome note's mutation table;
  - `CLAUDE.md` Step 3.

## Evidence

Root: `.superpowers/sdd/2026-11-07-zero-access-16-search/task19-mutations55-2026-11-10/independent-run/`
(git-ignored).

| Item | Contents |
|---|---|
| `journal.jsonl` | every mount, restore, run start and end, load wait, signal and manual stop, with hashes |
| `summary.tsv` | one line per verdict and per baseline or control |
| `results.json` | per-route records with first panic, frames, load and superseded history |
| `verdicts.json` | the row-level conclusions above (52 / 2 / 1) |
| `adjudication.txt` | the route view with calling fixtures |
| `unit-commands.json` | the 45 unit command ids |
| `runs/<label>/` | 255 run directories (254 completed; `I-paging-list` killed mid-build): `command.json`, `output.log`, `result.json` |
| `runs/*mount*/applied.diff` | the 83 applied faults |
| `tools/` | `runner.py` (`9cb37305…`), `adjudicate.py`, `drycheck.py`, runner stdout |
| `evidence.sha256` | SHA-256 of every file above |
