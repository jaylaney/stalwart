# PR 84 review-summary follow-up outcome

Starting head: `fbcdba82346f4b831310a27a63c4b674b680e1b3`.
Status: implementation, independent source review and local checks complete;
first canonical attempt failed and the unchanged retry passed. Hosted checks remain required.
Plan: [review-summary follow-ups](2026-11-14-pr84-review-summary-followups.md).
Ignored evidence: `.superpowers/sdd/2026-11-14-pr84-review-summary-followups/`.

## Changes

The warm pass loader releases its initial shared index guard before manifest I/O.
After authenticating the read it reacquires shared access and compares the bytes'
digest with the current resident head. Only a match accepts the observation and
settles retry state. A mismatching or missing resident head takes the existing
fresh manifest read under exclusive reload serialization. Cold/exclusive reload
ordering is unchanged. Spec revision 38 records this boundary.

The regression parks a captured manifest read and requires an index writer to
proceed. Further cases publish a newer head through the real service reload,
require a fresh fallback read, preserve an unknown retry episode if that fallback
fails, and verify cancellation leaves retry/durable state intact and releases locks.

The retry tests now cover actual observer `timeout_at` expiration in an isolated
paused runtime. The shared server runtime and retry bookkeeping's standard clock
remain real. Tokio's `test-util` feature is added only to the integration crate's
dev-dependency; existing explicit-expiry seam tests remain. Abort-bookkeeping cases
now authenticate the committed manifest after settlement, require exactly one
manifest read and no repeated expensive work, and check retained delay/class/streak.

IMAP coverage adds keyless metadata success and identity/grouping/content refusals
with an unavailable map. Paused FETCH covers saved `$` selection, message-limit
truncation, and nonempty CHANGEDSINCE, with independent exact identity expectations
and a real publishing pass while output stays blocked. The CHANGEDSINCE case uses
the filtering fallback; it is not coverage of the optimized changed-row scan.
CLAUDE's upstream dependency list now names the generated backend-event classifier.

## Review and validation

Separate independent source reviews approved the IMAP and retry-test changes. A
fresh reviewer approved the complete follow-up, including loader concurrency and
its interaction with retries. These reviewers inherited the parent route because
the originating capability ceiling was not explicit; no new named Astra review
or whole-feature-branch review is claimed.

The first build failed with E0277 because a notification future was not pinned.
The implementer corrected that polling expression and an independent reviewer
approved the correction. This compile failure is not the behavioral regression.

The next build passed in 126.965 seconds. Its focused run failed at the intended
writer-acquisition assertion while the old warm guard scope remained in place:
`warm manifest I/O must not hold the index read guard: Elapsed(())`.
Libtest reported 3.22 seconds; the wrapper recorded 11.465 seconds and exit 101.

After the production fix, the build passed in 97.065 seconds. All 2,190 Rust/config
source hashes matched before and after compilation. The retained executable is
`green-tests-bin`, SHA-256
`ebc89d50d41a42d3817b52f97c2804debd1af0c695cb66757fc3dd2ba220195a`.
Builds retain existing dependency/linker warnings; warning-free builds are not claimed.

| Check | Result |
|---|---|
| Focused PR84 whole suite, RocksDB | Passed, 60.96 s libtest / 67.192 s wrapper; first runtime attempt after fix |
| Search unit suites | Passed, 77 common + 63 email; 170.722 s including compilation |
| Shipping configuration check | Passed, 220.958 s; `--locked --offline -p stalwart --no-default-features --features rocks` |
| Focused PR84 suite, filesystem blobs | Passed, 61.09 s libtest / 61.216 s wrapper; first attempt |
| Complete canonical ZA suite, first attempt | Failed, 832.75 s libtest / 832.922 s wrapper; repeated backlog-draining `SELECT` hit the client's 1.5 s read timeout in `case_time_cap_bounds_the_command` |
| Complete canonical ZA suite, unchanged retry | Passed, 1,438.74 s libtest / 1,438.870 s wrapper; same frozen executable, including the differential corpus and final cleanup |
| `cargo fmt --all --check` and `git diff --check` | Passed |
| `python3 scripts/tests/ci_steps.py` | Passed, zero differences |
| Final-head hosted checks and retention | Pending; live status is tracked on PR84 |

The first canonical run reached and passed the new follow-up cases before failing
at `tests/src/za/mail/index.rs:2383`. The first time-cap SELECT and its partial-progress
assertion succeeded; a later SELECT failed. The cause remains under investigation;
this is not counted as a pass or dismissed as an unrelated/flaky failure. Its retained
log hash is `1bbc6e0e66f22a350d134d2c6ea54dfbd39e4d0beb365e6130b0599103dd1eb2`.
The first run overlapped the shipping `cargo check` for approximately 156 seconds;
the check finished roughly eleven minutes before the failure. Diagnostic runs and
the retry had no concurrent build or server suite. This records execution conditions,
not an established cause.

A temporary diagnostic runner replayed the same account's preceding tests and the
unchanged time-cap fixture in four fresh processes, unpaused/paused/paused/unpaused.
Tokio's first clock pause changes a process-wide `Instant::now` fast path, even
though it does not advance other runtimes' clocks. All 48 SELECTs passed in
147–236 ms, with the same checkpoint/chain progression and attempted-work counts.
This did not reproduce the failure or establish a pause-related slowdown; it does
not exclude a cause requiring the full-suite context. The harness was removed and
all 2,190 original GREEN source hashes verified before a full-suite retry using the
same retained executable. Neither client deadlines nor production code changed
between the first canonical attempt and its retry. The retry passed, including the
previously failing fixture (8.581 seconds including setup). The original timeout's
cause remains unexplained; the successful retry is not presented as a fix for it.
Retry log hash: `2a4a6245d844532320c441c4827456248bdb31060330fbb59a4109429398e6f6`.

The focused suite executes the new cases through the same helper as the canonical
ZA suite. Existing suite selection, deadlines, retries, retention baseline, gates,
crypto optimization and stored formats are unchanged. No new mutation run,
physical multi-node result or cold-start benchmark is claimed.

## Review-comment disposition

Current-source adjudication supported resolving 19 original inline threads. The
remaining thread, comment `4566621670`, stays unresolved: manifest-read failures
are still flattened before typed trace enrichment, and a secondary diagnostic-write
failure after the primary abort is not separately classified. This follow-up
preserves the one-primary-abort contract and does not claim to close those gaps.

All four review summaries will receive explicit item-by-item responses. Accepted
informational observations remain distinguishable from corrected code/test gaps:
returned-ID lifetime, unknown retry basis, request-driven map recovery, internal
unavailable-reason differences, deadline after a settled transaction, the checked
abort call-graph invariant, real scheduler integration wait, and the documented
backtrace-only measurement exception require no new mechanism here.

PR84 remains unmerged until the required checks pass on the newly published head.
