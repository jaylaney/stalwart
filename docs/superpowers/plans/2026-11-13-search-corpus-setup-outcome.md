# Differential corpus setup outcome

Follow-up to PR #84, based on `9b1df344b96c0ec808ef3243ed18cb2a209a9089`.
Implementation commits: `f5b2885c9`, `30c3f527a`.
[PR #85](https://github.com/jaylaney/stalwart/pull/85) targets `release7-search`
while #84 remains open. The original PR's source checkout was not edited.

Status: both corrected local suites passed and the A/B measurement is complete.
CI is not green: zero-access and retention jobs are still running, and the fresh-store
restart job failed its 60-second bootstrap readiness check.

## What changed

The fixture retains all 3,258 paired inputs, serial delivery order and the complete
persisted task/search → SMTP → task/search barrier. Only its polling cadence is
10 ms; existing callers retain 200 ms task and 100 ms SMTP polling. Task waits
remain unbounded and the SMTP-only deadline remains ten seconds. Pending-work
messages use elapsed time so faster polling does not multiply their frequency.

Each keyed identification batch unlocks once, then independently authenticates
and opens every stored metadata stub and complete MIME blob. Keys drop before
the next delivery batch. Other callers, including headerless readback and
lifecycle cases, retain fresh unlocks. There is no plaintext cache or changed
account authentication path.

Setup now reports phases, completed pairs, side counts, fallback counts, batch
and verification/barrier durations, and ten-second pending heartbeats. A heartbeat
observes one pinned work future without restarting, detaching or timing it out.
Labels contain fixed phases and numeric progress, not message or account content.

Corpus resources/generation, all three paired refusals/APPEND fallbacks, arrival
guards, stored-MIME identity and hash assertions, page size 400, all 183 comparison
forms, the existing K deviation, G5, canonical inclusion and teardown are retained.
No production source, CI selector, request deadline or build profile changed.

## Measurement

Both runs use the same Mac, unoptimized Cargo test profile and complete
`za::za_tests --exact --nocapture`, with `STORE=RocksDb`, `RUST_MIN_STACK=16777216`
and `RUST_BACKTRACE=1`. The comparison has `BLOB_STORE` unset. Builds and local
suites run serially. The final filesystem run additionally sets
`BLOB_STORE=FileSystem` and uses the same candidate executable.

The comparable setup interval runs from `differential: paired corpus` to
`differential: begin phase=currentness mode=key`. It includes account creation,
corpus construction, delivery, authenticated stored-message identification and
bulk indexing; it excludes the later currentness request and query matrix. An
external line-timestamp wrapper records both boundaries. Candidate subphase
measurements are inclusive where nested and are not summed as disjoint costs.

| Run | Setup interval | Full libtest runtime | Result |
| --- | ---: | ---: | --- |
| Unchanged baseline, default blobs | 582.54 s | 1,657.27 s | Passed |
| Corrected candidate, default blobs | 379.65 s | 1,451.18 s | Passed |
| Corrected candidate, filesystem blobs | 381.36 s | 1,418.55 s | Passed |

Setup decreased by 202.89 seconds (34.83%); full-suite runtime decreased by
206.09 seconds (12.44%). The longest output gap within the setup interval fell
from 453.51 seconds to 10.001 seconds. The final candidate emitted all 68 batch
receipts and three pending heartbeats during bulk indexing.

This is one sequential local A/B observation, not a stable benchmark or a hosted
runner prediction. Generated Message-IDs vary between runs; within each run the
paired stored-MIME equality checks remain binding. There is no unchanged
filesystem baseline and no filesystem speedup claim.

## Verification and review

- Corrected affected test target compiled successfully. Baseline and final
  candidate libtest listings are identical: 118 tests, zero benchmarks. New
  observer controls execute inside the canonical suite rather than a new selector.
- Formatting and diff hygiene pass. The workflow table has zero differences;
  43 gate self-tests pass with two existing skips. This is not a new execution of
  the complete retention/coverage net.
- The fresh baseline full suite passed all 183 forms with zero differences beyond
  the pinned K deviation, G5 and final cleanup. Its delivery receipt contains
  3,237 base plus 18 generated LMTP pairs and three paired IMAP APPENDs.
- The corrected default-blob candidate passed the full suite, all 183 forms, G5
  and cleanup. The filesystem candidate also passed those checks, using the same
  frozen executable. Both retain the same delivery counts and boundary 3259.
- Two independent source reviews covered the four changed Rust files and their
  unchanged contracts. The first found an administrator binding hidden by the new
  setup wrapper; returning it preserved cleanup. The fresh whole-follow-up review
  found cooperative-budget interference in the observer control's manual polls.
  Four probe-local `tokio::task::unconstrained` wrappers isolate those assertions
  from both `select!` and channel scheduling. Actual setup/heartbeat awaiting
  remains normally cooperative. Scoped rereviews closed both findings; no source
  findings remain. Independent receipt review confirmed both complete runs,
  their source/binary/log bindings, counts and measurements.
- Agents deliberately inherited the parent model/effort because the originating
  capability ceiling was not exposed. This round does not claim named Sol/Astra
  routing. Source changes were delegated; root owned integration and execution.

The first candidate compiled, then its full run was deliberately interrupted
before corpus setup at 299.89 seconds for the source-established scheduling
correction. It had no assertion failure and provides no passing qualification.
The corrected build took 63.14 seconds; final runs use its frozen executable.
No runtime reproduction of the earlier scheduling scenario is claimed.

## Evidence and CI

Local evidence is retained, untracked, under
`.superpowers/sdd/2026-11-13-search-corpus-setup/` in the managed follow-up checkout.
It includes exact commands/environment, source hashes, compiler receipts, frozen
executables, timed/raw logs, exit statuses and independent review reports.

- Baseline binary: `3d82685d6c06f4c4a6c6a7f4f13a27b7c881e664fdfdf5909886a70aecd7fc99`.
  Its retained executable and every intervening changed Rust source were verified
  against the base before execution; it was also preserved as a separate clone.
- Corrected candidate binary:
  `0eec0d69ac44e67f9753ebe62c245624cfacc7d645cdada50cfaef7e6f87f094`.
  `candidate-final-binary.json` and `candidate-final-sources.json` bind the build
  to the final Rust sources. Final run directories are `candidate-final/` and
  `candidate-final-filesystem/`; `candidate/` is the interrupted earlier attempt.

A draft PR and manual CI were started after source review/build acceptance while
local runs continued. The workflow only triggers PRs against main, so this
stacked follow-up requires manual dispatch. [Run 43488078110](https://github.com/jaylaney/stalwart/actions/runs/43488078110)
is on code commit `30c3f527a`. Style/gates, upstream suites (including Automation),
ordinary protocol suites, unit tests, the shipping product build and the independent
CalDAV client passed. Zero-access and retention jobs remain in progress.
[Fresh OSS Store Restart](https://github.com/jaylaney/stalwart/actions/runs/43488078110/job/132121499137)
failed because its disposable server did not become ready within 60 seconds; that
job does not use the changed test helpers. Its underlying cause is not established.
These statuses are a snapshot, not a final CI result. The outcome/plan completion
commit changes documentation only; this CI run remains bound to the code commit
above. PR #84's hosted post-bulk JMAP request-timeout diagnosis is separate and is
not claimed fixed by faster setup.
