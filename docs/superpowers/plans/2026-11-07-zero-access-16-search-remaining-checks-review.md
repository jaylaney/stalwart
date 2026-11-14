# Zero-access search: remaining Task 19 checks

Date: 2026-11-10. Contract: plan 16 revision 27, search design revision 33.

## Summary

- **Part 1 (the 19 screened regressions).** A narrowly scoped fault was injected for every target and its regression run.
  - 18 targets fail at their named catcher, so the regression is **demonstrated**.
  - **T13 Caller floor is partly demonstrated.** Faults on the standalone `Email/changes` route and the `Email/queryChanges` Map route are caught. The same fault on the caller-only Content route of `Email/queryChanges` **survives** the existing suite.
  - A proposed test-only case (Appendix A) passes on the correct code and fails on that fault. It ran only in the disposable worktree and is not applied to the branch.
- **Part 2 (the 55 rebased drafts).** All 55 bind to the accepted source. Every patch applies forward and reverses exactly on disposable copies, with no offset or fuzz. They were verified, not executed.
- **Restoration.** Every temporary fault and selection edit was restored byte for byte. The isolated worktree ended clean at the accepted HEAD after each group, and the accepted checkout was never edited.
- **Task 19 is not complete.** Unresolved gaps lists what remains.

## Source identity

| Item | Value |
|---|---|
| Accepted checkout | `/Users/jay/Development/stalwart/.worktrees/release7-search` |
| HEAD | `bcc81d1773139bd99c04acb95518ba83f0c298bc`, clean before and after this work (`git status --porcelain` empty) |
| Implementation commit | `53062c367ac0b85596017fed6ebb50edd8a66567`. `git diff 53062c367 bcc81d17 -- crates tests` is empty, so the later commits change docs only. |
| Isolated worktree | `/Users/jay/Development/stalwart/.worktrees/release7-search-checks`, detached at `bcc81d17`, with its own `target/` |
| Build | `cargo test -p tests --no-run` from scratch: exit 0 in 301 s. The only warning was the known macOS `ld` `__eh_frame` compact-unwind notice. |

Each file below has the same SHA-256 prefix in the accepted checkout, at `53062c367` and in the drafts' recorded originals. Each was restored to this value after every run that edited it.

| File | SHA-256 prefix |
|---|---|
| `crates/email/src/message/za/search/view.rs` | `a4dfd384b8c97cfe` |
| `crates/email/src/message/za/search/write.rs` | `0b4e8754eac605d9` |
| `crates/common/src/za/search/destroy.rs` | `cdf48661fbf336d8` |
| `crates/jmap/src/changes/za.rs` | `cc61cc9d1e35a02d` |
| `crates/jmap/src/changes/get.rs` | `5c20e9329a78de13` |
| `crates/jmap/src/email/query.rs` | `98ea3b1a993c6eb7` |
| `crates/jmap/src/email/set.rs` | `75eedfbf3a1a7d96` |
| `crates/jmap/src/email/import.rs` | `0fbeea24457fe0fe` |
| `crates/email/src/message/za/search/rebuild.rs` | `248508f415818470` |
| `crates/store/src/write/batch.rs` | `ed09660ccb645e4f` |
| `tests/src/za/mod.rs` | `ad733bd21e25e94d` |
| `tests/src/za/mail/index.rs` | `bb55b88ce424576a` |
| `tests/src/za/mail/threads/task13/minimum.rs` | `9cc1194b9425d5f1` |

## Method

The procedure is plan 16 Task 19 Step 2 ("Procedure, plan 15's"), with the deviations recorded under Deviations.

- **One fault per run.** Each fault is a draft diff against `bcc81d17` that realizes the plan row's mutant and nothing else.
  - Most are one exact replacement whose original text occurs once.
  - Four drafts need two hunks to express one fault: M26, M71, M73 and M74a/b/c. The reasons are under Deviations.
- **Byte checks.** Every draft was checked forward and in reverse on disposable copies before execution.
- **Selection.** The za suite is the single test `za::za_tests`. Named fixtures are internal `case_*` functions, so they cannot be selected through Cargo.
  - Every run used a tests-only focused selection in `tests/src/za/mod.rs`. It keeps the suite's setup, the needed mail module(s), and the trailing inventory, destroy, purge and `assert_is_empty` block unchanged.
  - Each group first ran a passing baseline with the identical selection, and ended with a passing control.
  - Every run shows non-empty selection: `running 1 test`, `test za::za_tests ...`, `1 passed` or `1 failed`, `111 filtered out`.
- **Command.** Run from the isolated worktree:

  ```
  export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
  export CARGO_TARGET_DIR=/Users/jay/Development/stalwart/.worktrees/release7-search-checks/target
  unset BLOB_STORE ZA_KEY_ACCOUNTS NO_INSERT
  export STORE=RocksDb RUST_MIN_STACK=16777216 TMPDIR="$(mktemp -d /private/tmp/t19-checks.XXXXXX)"
  timeout 3600 cargo test -p tests za::za_tests -- --exact --nocapture
  ```

  The backtrace diagnostics added `RUST_BACKTRACE=1`. Integration runs were strictly serial, and port 8899 was checked free before each run.
- **Restoration.** After every fault run:
  - `git checkout -- <file>`;
  - SHA-256 compared with the original;
  - `git status --porcelain` showing only the declared selection;
  - at group end, the selection restored, with empty `git status` and `git diff --stat`.
- **What was recorded.** For every failure: the first `panicked at` (file:line, message, left/right), exit status, and the enclosing case function. Where a panic sits in a shared helper, the case was confirmed by a separate `RUST_BACKTRACE=1` diagnostic run, and the original result was kept.
- **Masked catchers.** These followed the plan: keep the original result, then relax only the masking assertion with a tests-only edit, never the named catcher. Only M26 needed one follow-up.
- **Classification.** An exit of 101 with a `left/right` or message assertion inside the named case is an intended behavioural failure. No run ended in a compile error, setup failure or timeout.
- **Roles.**
  - Opus subagents drafted the faults.
  - Sonnet subagents executed the T10 and race groups and the first paging batch.
  - The orchestrator (Opus 5.5) reviewed every draft against the source, checked each executor's evidence, and ran the rest of the paging group and the epoch group with checked scripts.

## Part 1: the 19 regression checks

Selections:
- **search:** `mail::search` only.
- **races** and **epoch:** `mail::index` only, with identical selection bytes.
- **paging:** `mail::threads` then `mail::index`.

All baselines and controls passed:

| Group | Baseline | Control |
|---|---|---|
| search | 67 s (libtest 18.75 s) | 68 s |
| races | 261 s (220.87 s) | 275 s |
| paging | 319 s (276.25 s) | 347 s |
| epoch | 267 s (221.29 s) | 288 s |

In every row, the fault's run exited 101 and its file returned to the hash in Source identity.

### Results

| Target | Guarantee | Inspected source and fault | Observed first failure (fault run) | Conclusion |
|---|---|---|---|---|
| T10 Content floor | A content view resamples the message cache after a deletion-only publication. | `view.rs:147-148`, `za_view_with_minimum`. Delete only the Content checkpoint clause `\|\| (deadline.is_some() && cache.last_change_id < guard.manifest.indexed_to.change_id)`. Map's floor and the caller-minimum clause stay. | `search.rs:1866`, `content_view_resamples_after_nonpublishing_deletion`: parked NOT-alpha reply, left `["buaaaaan","byaaaaao"]`, right `["byaaaaao"]`. The deleted D was returned. | **Demonstrated** |
| T10 Final cap | The account cap applies to the exact final content view being served. | `view.rs:157-158`. Delete only the final-guard `check_content_cap(...)?`. The load-time check (`load.rs:781`) and the ensure-current checks stay. | `search.rs:1319`, `assert_search_unavailable`, called from `final_content_view_checks_cap` (`:1665`, cap + 1 byte): left `String("Email/query")`, right `"error"`. The over-cap view was served. | **Demonstrated** |
| M18 | Uncommitted thread state stays invisible, including after a failed publication. | `write.rs:693`, `commit_and_swap`. The pass's staged thread records, aliases and tombstones go into the resident index before the commit point; the real commit, CAS and post-commit swap stay. | `index.rs:2063`, `case_lost_assertion`: "the staged thread record became resident before commit". | **Demonstrated** |
| M26 | Destruction leaves no later publication or surviving permanent link from an already prepared writer. | `destroy.rs:196-198` and `:244-246`, `za_search_destroy`. For a readable Index manifest, unlink the inventory first (each chunk asserting the old manifest), then write the marker. | Original run: `index.rs:2832`, `case_r19_paused_pass_loses_to_the_marker` (the retained marker race): left `[]`, right the two committed hashes. Follow-up `M26-f1` (tests-only relaxation of `:2832`/`:2836` to prints): `index.rs:2538`, `assert_destroyed_index` from `case_r19_no_publication_after_unlink` (`:2883`), "orphaned index links for account 24". | **Demonstrated** (both handoff fixtures) |
| M35 | Initial paging remains usable when its pinned end is retained despite an older truncation. | `changes/za.rs:150`, `za_changes_page`. The from-zero retained endpoint becomes `since` (0) instead of the pinned E. | `task13.rs:36` (helper `response`) via `rest_at` `:292`, from `case_initial_after_truncation` `:245` (backtrace): left `"error"` (`cannotCalculateChanges`, "Change log is truncated"), right `"Email/changes"`. | **Demonstrated** |
| M55 | A response view includes the calling operation's acknowledged write. | `email/set.rs:1765`. The keyed final response view gets no write floor (`last_change_id` becomes `None`). This is the write-wrapper site the ledger's locator ruling prefers. | `threads.rs:77` (helper `method`) from `races.rs:540`, `case_write_response_requires_own_change_id` (backtrace): left `String("Email/set")`, right `"error"`. Supplementary `M55-import` (`email/import.rs:316`): `races.rs:692`, `case_import_requires_own_change_id`, left `String("Email/import")`. | **Demonstrated** |
| M57 | Initial continuation refuses when the lifecycle evidence needed at its pinned end has expired. | `changes/za.rs:150`. The from-zero retained endpoint becomes `None`, bypassing the pre-read bound, suffix truncation and post-read bound. The epoch refusal stays. | `task13.rs:36` from `case_initial_pinned_end_lifecycle` `:375` (backtrace): left `"Email/changes"`, right `"error"`. A page was served where a refusal is required. | **Demonstrated** |
| M58 | An initial continuation refuses after loss of the identity epoch needed for that page. | `changes/za.rs:192`. Replace `(since == 0 && range.pinned.is_some() && end < view.epoch)` with `false`. `za_epoch_refuses` stays. | `threads.rs:883`, `case_initial_continuation_epoch_loss: Thread`, reached through `mail::index`'s `test_recovery`: left `None`, right `Some("cannotCalculateChanges")`. | **Demonstrated** |
| M59 | Retention advancing during a page read cannot silently remove a required survivor. | `changes/za.rs:186`. Disable only the final retained-bound recheck term; the truncation predicates stay. This is one site at `bcc81d17`. | `task13.rs:36`, from-zero iteration of `case_retention_advances_between_bound_checks` (preceded by "retention advances past the admitted cache, from zero=true"): left `"Email/changes"`, right `"error"`. Supplementary `M59-positive` (recheck kept only for from-zero) failed in the S>0 iteration ("from zero=false"). | **Demonstrated** (both modes) |
| T13 Caller floor (a) | `Email/changes` respects the caller's state minimum. | `changes/get.rs:262`. `since.minimum()` becomes `None`. | `minimum.rs:134` "standalone changes floor": left `Jmap(CannotCalculateChanges)`, right `Store(UnexpectedError)`. | **Demonstrated** |
| T13 Caller floor (b) | `Email/queryChanges` (Map route) respects the caller's minimum. | `email/query.rs:159`, Map arm of `email_query_view`. `minimum` becomes `None`. | `minimum.rs:138` "queryChanges floor": left `Jmap(CannotCalculateChanges)`, right `Store(UnexpectedError)`. | **Demonstrated** |
| T13 Caller floor (c) | The caller-only Content route of `Email/queryChanges` respects the caller's minimum. | `email/query.rs:154`, Content arm (`za_content_query(..., minimum)` becomes `None`). The shared `view.rs:146` check stays. | Existing suite: **passed** (exit 0, libtest 274.43 s). With the proposed test (Appendix A): its baseline passed, then the fault failed at `minimum.rs:266` "Content queryChanges floor", left `Jmap(CannotCalculateChanges)`, right `Jmap(UnsupportedFilter)`. | **Survived** the accepted suite; caught by the proposed test |
| M68 | An empty-account rebuild still persists its identity boundary. | `rebuild.rs:452`. Delete only `batch.za_log_empty_email();` in the zero-witness branch. | `index.rs:3509`, `case_zero_row_epoch: fresh`: "fresh: keyed upload left the manifest absent". `case_reindex_without_email_rows`, which runs earlier, passed under the fault. | **Demonstrated** |
| M69 | That empty boundary has a real durable Email-log record. | `store/src/write/batch.rs:618`, `serialize_changes`. After `register_change_group`, `continue` when the Email change set is empty (group allocated, record omitted). | `index.rs:3519`, `case_zero_row_epoch: fresh`, `rebuilt.epoch == email_boundary(..)`: left `1`, right `0`. | **Demonstrated** |
| M70 | The empty boundary is encoded as a valid record with its kind preserved. | `batch.rs:620`. The empty Email change set is written as a zero-length value instead of `[0]`. | `index.rs:3543`, `case_zero_row_epoch: fresh`: left `[]`, right `[0]`. | **Demonstrated** |
| M71 | A stale thread-loss diagnosis cannot replace a healthy successor. | `rebuild.rs:351` and `:384`. For thread-loss evidence, a moved base passes the moved-base check and the CAS asserts the current successor instead of the observed bytes. | `index.rs:4619`, `case_stale_loss_keeps_successor` (Thread, first iteration): "B changed the exact successor manifest for Thread". | **Demonstrated** |
| M72 | A stale content-loss diagnosis cannot replace a healthy successor. | `rebuild.rs:525`, `za_search_content_rebuild_start`. `!loss.matches(..)` becomes `... && false`. | `index.rs:4619`, Content (second iteration; Thread passed first): "... for Content". | **Demonstrated** |
| M73 | A prior missing-manifest observation cannot reset a newly present successor. | `rebuild.rs:351`/`:384`, as M71, for a missing observation (`raw` is `None`). | `index.rs:4619`, Missing (third): "... for Missing". | **Demonstrated** |
| M74a | A stale older-format diagnosis cannot reset a newer healthy successor. | As M71, for `LossClass::OlderFormat` only. | `index.rs:4619`, Older (fourth): "... for Older". | **Demonstrated** |
| M74b | A stale manifest-tag diagnosis cannot reset a healthy successor. | As M71, for `LossClass::Tag` only. | `index.rs:4619`, Tag (fifth): "... for Tag". | **Demonstrated** |
| M74c | A stale manifest-parse diagnosis cannot reset a healthy successor. | As M71, for manifest-only `LossClass::Parse`. | `index.rs:4619`, Parse (sixth): "... for Parse". | **Demonstrated** |

### Notes by target

- **T10 floor.** The fixture asserts every precondition: changed manifest hash, equal advanced checkpoints, and an unchanged newest row, epoch and content head. The fault paired a stale cache with post-deletion content.
  - The kill proves the floor on routes that pass no caller minimum: `Email/query` and IMAP.
  - `Email/queryChanges` also passes the caller minimum, so on that route the minimum can still refresh the cache even with this floor removed.
- **T10 cap.** The case needs the parked query's first acquisition after the snapshot pause to accept without retry; a retry would let ensure-current's own cap checks refuse. That held: the over-cap descriptor was served.
  - The fault leaves an unused import of `check_content_cap` (`view.rs:9`), a warning only.
- **M18.** "Swap before the batch" was realized for the thread map only. The manifest, hash, segments, revision row and content cannot move before commit, because they need the change id assigned at commit.
  - The fault exposes exactly the uncommitted thread state that the plan's catcher observes.
  - The later "remains absent after failed CAS" check (`index.rs:2078`) went unobserved, because the suite stops at the first panic.
- **M26.** Under the fault, the retained marker-race fixture fails first: when the marker is visible, the links are already gone.
  - The relaxation made only those two assertions print their values: `links [] committed [two hashes]`.
  - The fault also moves the destroy pause and fail hook on the reordered path, so they keep their documented meaning of "after fence selection". Shipping behaviour of the hooks is unchanged.
- **M35.** The suite stops at the first panic, so the handoff's second fixture (`retention.rs` `case_admission_equality`) was not separately observed.
- **M55.** The literal shared acceptance clause (`view.rs:146`) also carries the Task 13 caller floor. So, following the ledger's locator ruling ("Prefer removing the floor at write wrapper ... separate from Task13 caller-token floor"), the fault was placed at the write caller. The shared clause was not separately mutated.
- **M57.** The fault also skips the test-mode page pause on the from-zero path. The catcher failed before any later parked case could be affected.
- **M58.** Only the first recovery variant (Thread) was reached.
- **T13c.** `minimum.rs:154` uses `wait_secs: 0`, so the existing stale Content `queryChanges` request is refused at the deadline before any floor is read. No existing fixture sends a stale state with a content filter on `queryChanges` with a non-zero wait. This matches the outcome note's deferred item "Task 13 caller-only Content floor is source-only". Appendix A gives the proposed fix.
- **Epoch group.** The drafter predicted possible masking of M68 by `case_reindex_without_email_rows` (`index.rs:240`, which runs before `test_recovery` at `:243`). It did not occur. M69 and M70 edit the store crate; their runs rebuilt it and stayed within about 3.5 minutes each.

## Part 2: the 55 rebased drafts

The package is `.superpowers/sdd/2026-11-07-zero-access-16-search/task19-rebased-mutation-prep/`; its `manifest.json` SHA-256 is `d54e30aa7f32e60d2580a33c894cb08ab11828b722fc7e8ca48b3a9fd49b7e5a`. A subagent verified it on disposable copies only, with no Cargo, tests or Git writes. I re-checked three rows independently (M6, M41 multi-file, M29c pure deletion), and those results matched.

| Check | Result (of 55) |
|---|---|
| Package integrity | All 8 accepted package manifests match their recorded SHA-256. All 47 unchanged patches equal the accepted patch bytes. The 8 changed patches (M6, M3, M12, M32, M33, M38a, M38b, M16) have `+`/`-` lines equal to the accepted patch, in order. |
| Source identity | Every row's `current_file_sha256` equals the blob at `bcc81d17` and at `53062c367`. Originals equal those blobs (SHA-256 and `cmp`); mutant copies and patch files match their hashes. |
| Exact replacements | 55/55. Each original occurs once in the `bcc81d17` blob and each replacement once in the mutant, and the match and `fn` anchors are correct. Four rows are pure deletions (M29c, M42a, M44, M63). Replaying the replacements reproduces every mutant byte for byte. |
| Catcher anchors | 108 catchers. Every named test function is at its `current_source_anchor`. Whether each filter selects a non-empty set of tests was **not** verified (no Cargo was run). |
| Forward application | `git apply --check --verbose` and `patch -p1 --dry-run`: no offset or fuzz. `patch -F0` output equals the mutant copy and its SHA-256. |
| Reverse application | `patch -R -F0` restores the `bcc81d17` blob exactly (`cmp` and SHA-256), with no stray files. A second reverse is refused by `patch -R -F0 -N`, `patch -R -F0 -f` and `git apply -R --check`. |
| Candidate binding | `candidate_tracked_diff_sha256` (`34a3594a…`) reproduces exactly as `git diff f4eb2ecc 53062c367` over the eight candidate paths. All eight candidate files equal their `53062c367` blobs. |

Discrepancies, neither affecting binding:

1. The protected path `docs/superpowers/specs/2026-11-04-zero-access-search-design.md` matches `53062c367` but not `bcc81d17`, because the later docs commits changed it. No draft targets it.
2. Apple `patch -R --dry-run` prompts on an already-restored file and exits 0 on its default answer. The three non-interactive checks above replaced it.

The 55 drafts remain **unexecuted**. This work verified their preparation only.

## Deviations

1. **Focused selections.** Runs used module-level tests-only selections instead of the full za suite. The selections were baselined and controlled identically, and the full suite at the source-identical `4dde13ade` passed in Phase B (carried context, not re-run here). A removed module could only have failed earlier; it cannot make a named catcher pass or fail. For the epoch group, the drafter's sub-entry selection (`test_recovery` only) was rejected in favour of the whole `mail::index` module, so the plan's masking procedure applied.
2. **Two-hunk faults.**
   - M26 is one reorder that touches two places.
   - M71, M73 and M74a/b/c each need the moved-base check and the CAS target changed together, because the plan keeps "the CAS against N".
   - Each hunk restores exactly.
3. **Paging execution.** The paging executor started a background batch for the nine rows, then handed back before the batch finished. It was told to stand down, and the orchestrator completed the group.
   - The batch checked the hash before and after each row and the selection-only status, and reported no mismatch. I read every row's log directly.
   - The diagnostics, T13c, the strengthening validation, the control and the final restore ran under an orchestrator script with the same checks: `finish-paging.out`.
4. **Diagnostics.** M35, M57, M55 and M55-import panic in shared helpers with no case marker printed before the panic. Separate `RUST_BACKTRACE=1` runs (`t19-diag-*-backtrace`) confirmed the calling cases; the original results are kept unchanged.
5. **Supplementary rows.** M59-positive and M55-import are labelled supplementary. M59-zero was drafted but not run, since it is equivalent in effect to M59 at this site.
6. **Plan names.** `case_r28_r31_initial` and `case_initial_continuation_retention_race` do not exist at `bcc81d17`. They correspond to `case_initial_after_truncation` and `case_retention_advances_between_bound_checks`.

## Unresolved gaps

- **T13c coverage.** The accepted suite does not detect removal of the caller minimum on the Content route of `Email/queryChanges`.
  - The proposed test (Appendix A) is validated only in the disposable worktree under the focused selection: baseline pass, fault fail, rustfmt clean.
  - It is not applied to the branch, and has not run in the full za suite or with `BLOB_STORE=FileSystem`.
- **Unobserved secondary assertions.** Because the suite stops at the first panic:
  - M35's `case_admission_equality`;
  - M18's after-failed-CAS assertion (`index.rs:2078`);
  - M58's recovery variants after Thread.
- **The rest of the Step 2 campaign is unexecuted.**
  - The 55 rebased drafts, including M30's wrong-reservation outcome, and the other Step 2 rows outside these 19.
  - Nonempty test selection for the 55 drafts' catchers is unverified.
  - The plan's expected outcome, "every row killed, or recorded as a finding", is therefore not established for the campaign as a whole.
- **Not run:** clippy on the faults, the faults under the full za suite, the FileSystem blob-store variant, and the key-mode suites.
- **Task 19 remains incomplete.** Besides the items above:
  - Docker suites are left to CI.
  - Native clients, the separate fresh-store gate, networked-store timing and physical multi-node behaviour are unqualified.
  - Full-size cold serving and its warm repeat are unqualified.
  - The outcome note and `CLAUDE.md` do not yet carry these results.

## Evidence

All paths are under `/Users/jay/Development/stalwart/.worktrees/release7-search/.superpowers/sdd/2026-11-07-zero-access-16-search/task19-remaining-checks/`. The SHA-256 list of every evidence file is `evidence.sha256`, itself `e8dc723a808331de109e8c4d13f5fb586602b72e93036d626b77bf3d1bf418cf`.

| Evidence | Path |
|---|---|
| Briefs | `drafts19/COMMON-BRIEF.md`, `EXECUTOR-BRIEF.md` |
| Fault drafts and selections, with per-target JSON (site, exact strings, hashes, byte checks, expected failure, masking analysis) | `drafts19/{search,races,paging,epoch}/` |
| Run logs, each with `.start` and `.exit` | `logs/t19-{base,mut,control,diag}-*.log` |
| Applied diffs (faults, selections, relaxation, strengthening) | `diffs/` |
| Per-run table (start, exit, wall, selection proof, first panic, log and diff SHA-256) | `all-runs.json`, generated by `collect_runs.py` |
| Group ledgers | `runs-search.{md,json}`, `runs-races.{md,json}`, `finish-paging.out`, `run-epoch.out`. The first paging batch's summary is in the session scratchpad (`batch1.out`) and its per-row logs are under `logs/`. |
| 55-draft verification | `drafts55/verify_drafts55.py`, `drafts55-results.json`, `drafts55-summary.md`, `drafts55-sha256.txt` |
| Build log | `baseline/build-tests.log` |

## Appendix A: proposed test-only diff (T13c)

The diff is `drafts19/paging/T13c-strengthen.diff`, SHA-256 `3420c2b9842b509f1bc530e727c68513200f29f1ad8106082971a1f1949287fe`. It adds a case to `tests/src/za/mail/threads/task13/minimum.rs`, and the file it produces has SHA-256 `125438ccea0d0e32…`. It removes no lines, weakens no assertion, builds no hand-made survivor and fabricates no row.

The existing stale Content `queryChanges` request runs with `wait_secs: 0`, so the deadline refuses it before any floor is read. A non-zero wait alone would not help: the main server's deletion pass persists `indexed_to = end`, so the shared Content checkpoint floor would mask the fault.

The new case avoids both:
- **Setup.** A fresh account on a separate node. A real flag write moves the counter past the node's held cache, while asserting the manifest's `indexed_to`, newest row and epoch unchanged.
- **Positive controls.** Content `queryChanges` from the node's own state, both unheld and held.
- **The check.** A held stale `queryChanges` with the caller's minimum must be refused as unavailable (`UnsupportedFilter`, `ZA_MAIL_SEARCH_UNAVAILABLE`) within the 20 s wait. After release, the same request must be served.

Validation in the disposable worktree:
- `t19-base-T13c-strengthen`: PASS, and the new case's banner printed.
- `t19-mut-T13c-strengthen`: FAIL at `minimum.rs:266`, left `Jmap(CannotCalculateChanges)`, right `Jmap(UnsupportedFilter)`.

```diff
--- a/tests/src/za/mail/threads/task13/minimum.rs
+++ b/tests/src/za/mail/threads/task13/minimum.rs
@@ -201,4 +201,92 @@
     assert_eq!(added(&delta), [(newer, 2)]);
     assert!(ids(&delta, "removed").is_empty());
     test.destroy_all_mailboxes(&key).await;
+
+    case_content_route_minimum(test).await;
 }
+
+// The keyed Content route of Email/queryChanges forwards the caller's
+// minimum. A flag write moves the counter past the separate node's held
+// cache without moving the manifest, so the publication counter and the
+// Content checkpoint floor both admit that cache. Only the caller's minimum
+// can refuse it, and a nonzero wait keeps the deadline refusal out of the way.
+async fn case_content_route_minimum(test: &mut TestServer) {
+    println!("token minimum on the keyed Content queryChanges route");
+    const WAIT_SECS: u64 = 20;
+    let key = key_mail_account(test, "keymail-task13-content-minimum@example.com", STRONG).await;
+    let account_id = key.id().document_id();
+    let live = three_singletons(&key).await;
+    flag_seen(&key, live[0]).await;
+    let (warm_state, _) = query(&key, false).await;
+    let token = keyed_token(test, account_id, key.secret()).await;
+    let node = super::super::races::separate_publisher(test).await;
+    let warm = node.get_cached_messages(account_id).await.unwrap();
+    assert_eq!(warm.last_change_id, exact(&warm_state));
+    let content = json!({"subject": "paging"});
+    let _wait = search_config_guard!(test, wait_secs: WAIT_SECS);
+    let served = node_query_changes(&node, &token, &warm_state, content.clone())
+        .await
+        .expect("the node serves Content at its own counter");
+    assert_eq!(state(&served, "newQueryState"), warm_state);
+
+    let before = composition::manifest(test, &key).await;
+    assert!(warm.last_change_id > before.newest_row.change_id.max(before.epoch));
+    assert!(before.indexed_to.change_id <= warm.last_change_id);
+    flag_seen(&key, live[1]).await;
+    let end = test
+        .server
+        .get_cached_messages(account_id)
+        .await
+        .unwrap()
+        .last_change_id;
+    assert!(warm.last_change_id < end);
+    let after = composition::manifest(test, &key).await;
+    assert_eq!(after.indexed_to, before.indexed_to);
+    assert_eq!(after.newest_row, before.newest_row);
+    assert_eq!(after.epoch, before.epoch);
+    let suffix = range(test, &key, SyncCollection::Email, warm.last_change_id, end).await;
+    assert!(!suffix.truncated && contains(&suffix, Change::UpdateItem(live[1].id())));
+
+    let stale = State::new_exact(end);
+    let hold = HoldCache::new(account_id);
+    let held = node.get_cached_messages(account_id).await.unwrap();
+    assert_eq!(held.last_change_id, warm.last_change_id);
+    let served = node_query_changes(&node, &token, &warm_state, content.clone())
+        .await
+        .expect("the held node serves Content at its own counter");
+    assert_eq!(state(&served, "newQueryState"), warm_state);
+    let started = std::time::Instant::now();
+    let error = node_query_changes(&node, &token, &stale, content.clone())
+        .await
+        .expect_err("Content queryChanges served a stale cache");
+    assert!(
+        started.elapsed() < std::time::Duration::from_secs(WAIT_SECS),
+        "the refusal came from the deadline, not the floor"
+    );
+    assert_eq!(
+        error.event_type(),
+        trc::EventType::Jmap(trc::JmapEvent::UnsupportedFilter),
+        "Content queryChanges floor"
+    );
+    assert_eq!(
+        error.value_as_str(trc::Key::Details),
+        Some(format!("subject: {ZA_MAIL_SEARCH_UNAVAILABLE}").as_str())
+    );
+    assert_eq!(
+        composition::manifest(test, &key).await.indexed_to,
+        before.indexed_to
+    );
+    assert!(Arc::ptr_eq(
+        &node.get_cached_messages(account_id).await.unwrap(),
+        &held
+    ));
+    drop(hold);
+
+    // Released, the same minimum refreshes the node's cache and serves.
+    let served = node_query_changes(&node, &token, &stale, content)
+        .await
+        .unwrap();
+    assert_eq!(state(&served, "oldQueryState"), stale);
+    assert_eq!(state(&served, "newQueryState"), stale);
+    test.destroy_all_mailboxes(&key).await;
+}
```
