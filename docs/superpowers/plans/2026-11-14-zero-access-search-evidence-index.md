# Zero-access search: decisions and evidence index

This index accompanies the [qualification outcome](2026-11-07-zero-access-plan16-outcome.md)
and [retention identity ledger](2026-11-14-search-ci-retention-ledger.md).
The [revision 38 design](../specs/2026-11-04-zero-access-search-design.md),
[Plan 16](2026-11-07-zero-access-16-search.md) and follow-up implementation plans remain
authoritative. This consolidation changes documentation, not their contracts.

All twenty detailed reports are preserved below at merged PR84 revision
`67ca91226429945f6f1dd3a24feaaa354e889ed2`. Their statuses describe their own cutoffs.
The qualification outcome records the final successful CI at `82b8984aac77a2fe718eb37f8f59110bdbbf0305`;
earlier pending CI, Docker and fresh-store statements are historical. Historical
benchmarks and fault campaigns do not become measurements of that later source.

## Accepted decisions

The current spec contains the detailed construction and accepted residuals. These
entries preserve the reasons to consult it and identify decisions later reports changed.

| Area | Accepted decision and consequence | Authority / record |
|---|---|---|
| Format and memory | Keep positions for phrase matching and upstream Term compression. Retain provisional 512 MiB account, 1,536 MiB Content and 256 MiB Thread limits, one shared cluster account cap and the one-third boot constraint. Measured payloads, model charges and RSS are different quantities. | Design 4.2–4.4; archived Plan 16 outcome and cold-start report. |
| Padding and aliases | Publishing pads to the exact configured minimum, including non-powers of two. Compaction uses public-count allowance; retained records witness kept aliases, establishing aliases ≤ records. Bridge retention and one-hop folding preserve resolution. Time/admission-cost leakage remains accepted. | Design 2, 4.2, 5.5; archived Codex design review's R35 closure. |
| Identity and history | Use one validated cache/map/content view with caller and checkpoint floors. Thread state comes from its admitted Email-group cache counter. queryChanges projects historical identity/existence but uses current readable row/content values, not exact historical ranks. Initial continuations retain endpoint/epoch evidence requirements. | Design 6.2–6.3; archived plan reviews and Plan 16 rulings. |
| Lifecycle and recovery | Bind loss recovery to its exact asserted base; preserve an empty-account boundary. Reset asserts live Account and captured markers. Destroy scans exact Principal/document-zero links with the applicable vault/manifest fences; double absence is not a permanent account-lifetime fence. Stale setup-token issuance remains a scoped residual. | Design 5.4, 5.7; Plan 16 lifecycle amendments. |
| Compaction/admission | Full merge tier saturates at 255; tiered overflow refuses atomically. Charges follow last owners and include staging/scratch. Optional Content must not determine Map availability. | Design 4.3, 5.5; WB2 and warm-admission records. |
| Retry, revisions 34–37 | Transients never consume a message; the old skip count is diagnostic and `ZA_SEARCH_SKIP_AFTER` is ignored. Keep atomic prefix discard and a memory-only per-node account cooldown (1, 2, 4, 8, 16, up to 30 seconds), plus shared one-second authenticated progress observation. Restart/eviction can permit an immediate probe; repair can wait until the next deadline. | Current design 5.1; archived PR84 fixes/retry/account-retry/CI-settlement outcomes. |
| Cancellation and diagnostics | Caller deadline expiry is not an observed store fault. An acknowledged own-abort write preserves the failed probe; independently observed meaningful progress uses normal settlement. Safe fixed backend/operation labels exclude raw stored data. Two diagnostic-detail gaps remain below. | Design 5.1 and 8; archived CI/retry settlement and final follow-up. |
| Lock and response lifetimes | Warm manifest I/O occurs outside the shared index guard, followed by authenticated resident-head comparison/fresh exclusive fallback. FETCH releases the view before client output. Trusted creation references survive within their request, while literal/result-reference stale IDs remain strict; returned IDs can stale after their method's paired state. | Design revisions 34–38 and 6.2; PR84 follow-up plans. |
| Parity and cleanup | Differential threading assumes settled paired delivery, ordinary merge work and deletion cleanup. Keep all 3,258 paired messages, three transport exceptions and one pinned K deviation. Periodic idle sweep uses the existing 30-second scheduler. Ordinary destroy's accepted full link-scan cost remains. | Design 5.1, 7.5 and 9.5; archived fixes/corpus reports. |
| Measurement policy | The 600-second wire ceiling belongs only to the system benchmark; it is not a product timeout change. CI's curve25519-dalek optimization belongs only to the test package. Longer job budgets, faster fixture setup and the measured request correction solve distinct problems. | Archived benchmark, corpus and curve-profile reports. |
| Retention | Exact moves and composite equivalents preserve old step obligations; supported-content refusals retire while valid metadata and unsupported-operation checks remain. No baseline/gate/threshold reduction. | [Complete identity mapping](2026-11-14-search-ci-retention-ledger.md), unchanged machine dispositions, final successful strict gates. |

## Findings and evidence limits

| Item | Disposition |
|---|---|
| Historical design/plan reviews | Scoped approvals used a no-P1-construction stopping rule. Their older P2s and counterexamples are not current open implementation findings. PL23/PL24 and the later FX content-load cases were incorporated before implementation. |
| WB1–WB3 | Corrected at `53062c367`, with two independent scoped review passes and fresh verification. The later never-skip-transient policy supersedes WB1's original bounded non-absent retry rule. |
| T13 Content caller floor | Test-only correction integrated exactly; focused fault detection plus complete default/FileSystem correct-code runs. Closed integration gap. |
| Warm admission / actual map order | Warm admission corrected before final benchmarks. The actual-map-order companion detected its fault; former gap closed. |
| Ordinary/mixed SAVE self-wait | Initially deferred; later reproduced during PR84 and corrected using captured prior results independently of Map presence, with both account modes covered. Closed, not a current open item. |
| M16 / M23 / M48 | Historical survivors; strengthened tests passed correct code, without new faulty-variant execution. Do not mark killed. M16 establishes the state directly; M23 is a planner contract test. |
| M29b and named catchers | Hidden-section tombstone variant unexecuted. Alternate-catcher and unobserved branch limits are listed in the qualification outcome; archive preserves each route. |
| Trace detail, comment 4566621670 | Still unresolved: manifest-read errors flattened before typed enrichment, secondary diagnostic-write failure not separately classified. Preserve one primary abort and safe logging. |
| Final local SELECT timeout | First canonical attempt failed; the unchanged executable passed its permitted retry. Four diagnostic processes/48 SELECTs did not reproduce it. Cause unestablished; retry is not a fix. |
| Historical local SMTP | Full attempt exited 101 (75 passed, one failed, three ignored); the permitted exact isolated `dkim2_dsn_is_signed` retry passed. No cause established. |
| Optimized Rust teardown / shipping smoke | Measurement preceded an unresolved worker teardown panic. Original shipping smoke's extra state revision remains unattributed; mailbox-initialized fixture subsequently passed unchanged state/census checks. |
| Other earlier anomalies | Task 9's first Sales failure, original parity interleavings/timestamps and PR85's initial 60-second fresh-store readiness failure were not causally established. Their later non-recurrence is not a diagnosis. Exact attempts remain archived. |
| Remaining qualification | Native clients, physical multi-node behavior, networked-store timing, cold-backlog IMAP wait-budget evidence, broader attachment formats, final sizing and noted mutation sensitivity remain outside completed evidence. Final CI closes the previously pending Docker/fresh-store/retention gates only within CI's contract. |

## Archived reports

Each link names the exact pre-consolidation file in the immutable revision above.
The two active reports (qualification outcome and retention ledger) are also linked
in their original forms so no previous result table or mapping history is lost.

| Original report | Scope |
|---|---|
| [2026-11-07-zero-access-16-search-codex-review.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-07-zero-access-16-search-codex-review.md) | Plan review rounds, PL1–PL24 and scoped approval. |
| [2026-11-07-zero-access-16-search-fable-review.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-07-zero-access-16-search-fable-review.md) | Plan construction traces and FR–FX review rounds. |
| [2026-11-07-zero-access-16-search-implementation-review.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-07-zero-access-16-search-implementation-review.md) | Whole-branch WB1–WB3 and correction verification. |
| [2026-11-07-zero-access-16-search-mutations-final-review.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-07-zero-access-16-search-mutations-final-review.md) | Remaining rows, masked-catcher follow-ups, M16 reachability. |
| [2026-11-07-zero-access-16-search-mutations55-review.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-07-zero-access-16-search-mutations55-review.md) | 55 labels, per-route results, interruptions/restoration. |
| [2026-11-07-zero-access-16-search-remaining-checks-review.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-07-zero-access-16-search-remaining-checks-review.md) | 19-target campaign, T13 proposal and draft binding. |
| [2026-11-07-zero-access-plan16-outcome.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-07-zero-access-plan16-outcome.md) | Construction rulings, all benchmark attempts, matrix and mutation tables. |
| [2026-11-10-zero-access-search-cold-start-outcome.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-10-zero-access-search-cold-start-outcome.md) | Optimized Rust and separate OSS restart qualification. |
| [2026-11-12-zero-access-search-pr84-account-retry-outcome.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-12-zero-access-search-pr84-account-retry-outcome.md) | Account retry ownership, remote observation and loader correction. |
| [2026-11-12-zero-access-search-pr84-fixes-outcome.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-12-zero-access-search-pr84-fixes-outcome.md) | Eight review fixes, SAVE correction and Automation fixture. |
| [2026-11-12-zero-access-search-pr84-retry-followup-outcome.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-12-zero-access-search-pr84-retry-followup-outcome.md) | Cooldown, loss classifier, abort settlement and created-ID lifetime. |
| [2026-11-13-pr84-email-get-timing-outcome.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-13-pr84-email-get-timing-outcome.md) | Selected 400-message request instrumentation and local measurement. |
| [2026-11-13-search-ci-curve-profile-outcome.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-13-search-ci-curve-profile-outcome.md) | Hosted control/candidate and combined local qualification. |
| [2026-11-13-search-corpus-setup-outcome.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-13-search-corpus-setup-outcome.md) | Fixture-preserving setup change and local A/B. |
| [2026-11-13-zero-access-search-pr84-ci-and-retry-outcome.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-13-zero-access-search-pr84-ci-and-retry-outcome.md) | Hosted failure history, safe diagnostics, cancellation/abort corrections. |
| [2026-11-14-pr84-review-summary-followups-outcome.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-14-pr84-review-summary-followups-outcome.md) | Warm loader lock, retry/IMAP tests and first-failure/unchanged-retry record. |
| [2026-11-14-search-ci-retention-ledger.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-14-search-ci-retention-ledger.md) | Original complete mapping and pre-hosted status. |
| [2026-11-14-search-ci-retention-outcome.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/plans/2026-11-14-search-ci-retention-outcome.md) | Metadata restorations, local suites, hashes and merge binding. |
| [2026-11-04-zero-access-search-design-codex-review.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/specs/2026-11-04-zero-access-search-design-codex-review.md) | Design counterexamples, R35 proof and scoped approval. |
| [2026-11-04-zero-access-search-design-fable-review.md](https://github.com/jaylaney/stalwart/blob/67ca91226429945f6f1dd3a24feaaa354e889ed2/docs/superpowers/specs/2026-11-04-zero-access-search-design-fable-review.md) | Design review rounds, parity/privacy/construction decisions. |

## Evidence roots

These are checkout-local, git-ignored roots, not files restored by the archive links.
They retain command/environment receipts, source and binary hashes, raw/sanitized
logs, independent review reports and failed/superseded attempts. Ordinary clones do
not contain them. This consolidation neither deletes nor relocates that evidence.
The archived reports record exact subpaths, artifact hashes and invocation details.

| Root under `.superpowers/sdd/` | Key contents |
|---|---|
| `2026-11-07-zero-access-16-search/` | Task/whole-branch reviews; `task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/`; final matrix and benchmark reports/primary receipts. |
| `2026-11-07-zero-access-16-search/cold-start-qualification/` | Controls, corpus manifests, Rust full run, shipping original/mailbox-initialized runs, teardown diagnostic, `retained-evidence.sha256`. Private MIME exports/stores remain in their original locations; seed-1 exporter regenerates the corpus. |
| `2026-11-07-zero-access-16-search/task19-remaining-checks/` | `all-runs.json`, drafts/diffs, per-group logs, T13 proposal and `evidence.sha256`. |
| `2026-11-07-zero-access-16-search/task19-mutations55-2026-11-10/` | Frozen preparation and `independent-run/` journals, route verdicts, mounted diffs, runner incidents and evidence hashes. |
| `2026-11-07-zero-access-16-search/task19-mutations-final-2026-11-11/` | Preparation and round2/2b/2c packages, journals, baselines/restored controls, adjudications and evidence hashes. |
| `2026-11-12-zero-access-search-pr84-fixes/` | Negative controls, scope reviews, final source-bound units/protocol/ZA checks. |
| `2026-11-12-zero-access-search-pr84-retry-followup/` | Classifier/cooldown controls, frozen test binary and final-source proof. |
| `2026-11-12-zero-access-search-pr84-account-retry/` | Account streak/load tests; separate stopped/failed canonical candidates and final round-4 result. |
| `2026-11-13-pr84-ci-timeout/` | Hosted history, diagnostic/retry reviews and frozen focused/canonical binary. |
| `2026-11-13-pr84-email-get-timing/` | Observer controls, diagnostic source/binary binding and local/hosted stages. |
| `2026-11-13-search-ci-curve-profile/` | Compiler profile comparison, hosted control/candidate and initial Inventory failure. |
| `2026-11-13-search-corpus-setup/` | Baseline/corrected/interrupted attempts and sequential A/B receipts. |
| `2026-11-13-pr84-search-ci-combined/` | Combined source/binary hashes and canonical run before metadata restoration. |
| `2026-11-14-search-ci-retention/` | Mapping/protocol reviews, frozen binary and complete local key-mode suites. |
| `2026-11-14-pr84-review-summary-followups/` | Red/green loader evidence, first canonical failure, diagnostics and unchanged retry. |

The [qualification outcome's reproduction section](2026-11-07-zero-access-plan16-outcome.md#reproduction-and-evidence)
provides the main benchmark/test commands. Use [cold-search.md](../../../tests/product/cold-search.md)
for the shipping fixture and [the gate README](../../../scripts/tests/README.md) for
retention commands/schema. Mutation reproduction also requires the archived selection,
patch and correct-code baseline/restoration records; a libtest exit alone is insufficient.

For an original report locally, use `git show 67ca91226429945f6f1dd3a24feaaa354e889ed2:<path>`.
The ignored consolidation inventory at
`.superpowers/sdd/2026-11-14-search-evidence-trim/inventory.json` records all twenty
original paths and SHA-256 values; the Git revision is the durable clone-accessible source.
