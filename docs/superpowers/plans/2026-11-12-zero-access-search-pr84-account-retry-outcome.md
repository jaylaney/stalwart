# PR 84 account-pass retry follow-up outcome

Starting head: `b02015ceefb8c789bb1f35490f5e9144264a3232`.
Spec: revision 36 of the search design. Implementation, independent reviews and
local verification are complete. Publication and current CI links are recorded on
[PR 84](https://github.com/jaylaney/stalwart/pull/84).

The three new review comments identify one incomplete retry boundary: required load
and write failures were unpaced, changing failing documents reset the local streak,
and deferred Content waits could miss another node's committed progress. The fix
keeps atomic abort and persistent formats, makes the account pass own retry admission,
and shares cheap authenticated manifest observation across deferred callers.

## Decisions

- One-second observation cadence per resident entry bounds manifest-read amplification;
  remote progress may take up to that interval to be noticed before normal loading.
- Keep the original Content request deadline and final view validation. A local probe
  deadline beyond it does not justify unconditional fail-fast: another node may progress.
- Preserve atomic prefix discard, the memory-only per-node controller, and existing
  service-load availability. No persistent lease or partial-prefix publication was added.
- Agent routing deliberately inherits the parent route because its capability ceiling
  is not exposed; this is not evidence of a named Astra review.
- Preserve ignored evidence in the existing isolated worktree. No merge or deployment.

## Verification

Evidence: `.superpowers/sdd/2026-11-12-zero-access-search-pr84-account-retry/`.

- RED: the synthetic-clock alternating-document/class regression failed against the
  original implementation: its second failure used 1 second rather than 2 seconds.
  `red-account-streak/result.json` records exit 101 and the source hashes.
- Search units: `green-search-units-3/result.json`, common 76 and email 63 passed.
  This includes the final loader locking and production trace corrections.
- Final focused PR84 integration: `focused-pr84-4/result.json`, passed in 36.45 seconds
  (154.44 seconds including compilation). Includes pinned-view publication, pure
  assertion contention and deferred required-Thread recovery as well as the new
  account-pass, secondary-error and remote-publication regressions.
- Shipping configuration: `shipping-check-r3/result.json`, passed in 68.28 seconds,
  `cargo check --locked --offline -p stalwart --no-default-features --features rocks`.
  This is a compile check, not a new optimized executable or performance qualification.
- Workflow checks: `ci_steps.py` reports zero differences; `test_gates.py` ran 43
  tests successfully with two existing skips. Suite selection and rerun semantics
  are unchanged by the finite job-budget increase.
- Independent focused review requested two corrections: explicitly expire the
  canonical expired-hold test's deliberate cooldown before repair, and distinguish
  allowed compaction fallback from an actual abort trace. Both passed scoped review.
  Root found a second test-only seam accidentally applied to pure assertion loss;
  it now asserts no cooldown is invented and runs in the focused suite. Its scoped
  re-review passed too. Reports: `task-1-review.md`, `task-1-rereview.md`,
  `task-1-rereview-2.md`.
- Fresh whole-branch static review against main `92a41596a571e164d3e88b3fa5f0c9eb6d5e0660`
  approved with no confirmed blocking finding (`whole-branch-review.md`).
  Subsequent loader, test and CI-budget deltas received the scoped independent
  reviews described here, including `ci-budget-rereview.md`. Source review does
  not substitute for runtime evidence.
- The first canonical attempt was deliberately stopped after 238.95 seconds, before
  reaching the source-proved test-only seam defect. Its receipt records SIGTERM;
  it was neither a completed run nor an observed test assertion failure.
- `canonical-za-2` failed after 653.81 seconds in the existing acquired-view/queued-
  writer fixture. The pass loader's unconditional exclusive index guard prevented
  publication while a served view held its reader. This is a confirmed local
  regression; the earlier CI timeout occurred later in the differential test and
  is not established to have the same cause. The unchanged-head loader path is
  corrected to use shared access, with fresh authentication after any
  read-to-write transition. The original failing fixture is unchanged and now also
  runs in the focused suite. Requirements and serving/lock reviews approved the
  correction (`task-1-rereview-3.md`, `whole-branch-fix-review.md`); fresh units,
  focused execution and shipping compilation passed after it.
- The failed candidate used the retained exact binary
  from focused round 2: SHA-256
  `bc6a18186b03a27729ac6f6d70ceaf69cadac9d15c3fa25500e14171f33efd84`.
  `final-tests-bin.json` and the command receipts retain the binary/source identity.

- `canonical-za-3` failed after 889.20 seconds in the existing required-Thread
  timeout recovery test. Its immediate second metadata query expected another
  two blob reads; the new account cooldown correctly deferred them. A fresh
  implementer corrected the test to assert the deferred/no-fetch state, then
  explicitly expire its test deadline before requiring recovery. The original
  atomicity and eventual-load assertions remain. The same fixture now runs in the
  focused suite on a fresh account. A source audit of index, search and thread
  recovery tests found no further concrete immediate-retry gap; independent service
  loads and ordinary contention keep their distinct behavior. This run used the
  retained focused-round-3 binary: SHA-256
  `a8a9e7174cabcdfdbd5e91b51505193ce350a8b73ef09e66c51e29563b0ea0c9`
  (`final-tests-bin-r3.json`). It is failed-run evidence, not a completed suite.
- Final canonical RocksDB ZA run: `canonical-za-4/result.json`, exit 0,
  1,690.17 seconds execution / 1,695.34 seconds including wrapper setup. All 183
  paired query forms showed zero differences beyond the documented K deviation;
  final cleanup completed. It used the exact focused-round-4 binary retained in
  `final-tests-bin-r4.json`, SHA-256
  `7b740fb0ae43d0a38fce3ce0effed0a24a525c144af6c3528029e90a7e61c0f7`.
  Rust-source hashes match before/after both runs and the final tree. The final
  production sources also match the passing search-unit and shipping-check
  receipts (`final-source-proof.json`). Earlier failed candidates remain retained
  separately. Final formatting, diff hygiene and workflow consistency checks
  passed (`final-style-r4`).
- The round-4 independent review approved the recovery-test delta with no new
  concrete finding and verified that its source matches the focused passing receipt
  (`task-1-rereview-4.md`). The fresh implementer audit is recorded in
  `task-1-fix4-report.md`.

The remote-publication regression is a deterministic shared-store
simulation, not a qualification of two running nodes. No new performance benchmark,
cold-start qualification or broad mutation campaign is claimed by this correction.

## Existing-head CI observation

[Run 43286973675](https://github.com/jaylaney/stalwart/actions/runs/43286973675)
for the starting head passed Automation and the other completed jobs. Its first
canonical ZA attempt failed with a five-second raw JMAP client timeout after the
bulk-index completion milestone. The log had no backtrace identifying the specific
request. The automatic retry then exceeded the job's two-hour maximum during the
differential corpus. This is not evidence that the retry-boundary correction fixes
that timeout, and no global client-timeout increase is justified by that log alone.

The Zero-access job's finite timeout is raised from 120 to 240 minutes. The measured
18-minute setup plus the 57-minute attempt and its allowed retry already exceed the
old limit before later suites. This provides headroom for both blob modes and later
suites; it does not guarantee completion under every rerun or fix the unidentified
client timeout. The cost is up to two additional runner hours for a stuck job.
No test selection, client deadline, retry count or failure condition was relaxed.
The matching gate-test environment fixture follows the new timeout literal.

## Coverage boundaries

New direct runtime cases cover cold required-Thread failure, unknown manifest reads,
both upload positions, commit and late mailbox enumeration faults, unchanged manifest
and permanent links, bounded shared observations, eventual repair, secondary-write/
winner-read single-abort accounting, remote publication and original observer/Content
loader deadlines. Synthetic-clock tests establish the account streak across changing
documents/classes and its diagnostic/unknown/lifecycle rules.

Dedicated new parked tests were not added for cancellation before diagnostic
acknowledgement, cancellation during assertion-winner reload, progress commit before
swap, moved-head required-Thread failure, or compaction-read failure. Existing tests
and source ordering cover related paths; they are not claimed as direct new execution
of each listed scenario. No stored-format, padding, alias, chronological checkpoint,
ordinary-account or stale-ID policy change is part of this correction.
