# Zero-access search and threads: qualification outcome

Current qualification record, consolidated on 2026-11-14 after
[PR 84](https://github.com/jaylaney/stalwart/pull/84) merged at
`67ca91226429945f6f1dd3a24feaaa354e889ed2`. The accepted contract is the
[search design, revision 38](../specs/2026-11-04-zero-access-search-design.md);
[Plan 16](2026-11-07-zero-access-16-search.md) and its follow-up implementation plans
remain the construction record. Historical measurements below apply to their named sources.

The [evidence and decision index](2026-11-14-zero-access-search-evidence-index.md)
links all twenty original reports at the immutable merged revision, including exact
commands, hashes, failed attempts and detailed review histories. Checkout-local
`.superpowers/sdd/` evidence is git-ignored and absent from ordinary clones.

## Where things stand

The final source head `82b8984aac77a2fe718eb37f8f59110bdbbf0305` passed both
[PR CI run 43621111083](https://github.com/jaylaney/stalwart/actions/runs/43621111083)
and [full manual run 43621127921](https://github.com/jaylaney/stalwart/actions/runs/43621127921).
The manual run's Test Retention Gates completed successfully, including Inventory
and Coverage. These results supersede the earlier reports' pending combined-head,
Docker-suite, broader fresh-store and retention statuses. PR 84 is merged.

This establishes the executed CI contract, including its configured retries and
exclusions. It does not establish native-client or physical multi-node behavior,
networked-store timing, final production sizing, or new mutation sensitivity.
Task 19 is not declared fully complete. The optimized cold-search measurements and
mutation campaigns remain historical evidence; they were not repeated at the final head.

## Validation by source

| Source or source-bound checkpoint | Evidence | Qualification |
|---|---|---|
| Original review `f4eb2ecc` → correction `53062c367`, review-only `4dde13ade` | Two independent whole-branch Astra reviews found WB1–WB3 in `f4eb2ecc`; both scoped correction reviews passed. Local matrix accounted for 39 of 41 main rows: four source-identical carries, 33 passes, one SMTP row qualified by its permitted isolated retry, and one completed system harness with serving limitations. Shipping release build was warning-free. | Historical implementation and local matrix; the two Docker rows were then deferred to CI. |
| `bcc81d177` plus exact T13 test addition | Focused correct-code pass and missing-Content-minimum fault failure; integrated test passed complete default and FileSystem ZA suites in 1,640.65 s and 1,739.49 s. | Coverage correction; production unchanged. |
| `9ea951df965af634611fb6311afa987736f61242` | Optimized Rust cache-eviction measurement and separate optimized OSS process-restart measurement. | Local 30,000-message cold-search qualification, with the limits below. |
| PR84 fixes and retry waves, spec revisions 34–37 | Source-bound search/JMAP units, focused regressions, canonical ZA, ordinary protocols where affected, shipping checks and independent source reviews. | Separate successive checkpoints, not evidence that earlier performance or mutation results were repeated. |
| Combined CI/profile/corpus checkpoint, before metadata restoration | Canonical ZA passed in 1,428.06 s with all 3,258 paired inputs, 183 forms, the pinned K deviation, G5 and cleanup. | Predates the retention repair. |
| Retention restoration | Complete key-mode IMAP/JMAP passed on one frozen binary in 12.02/208.60 s; all 43 active gate self-tests passed, two existing skips. | Local protocol checks; final strict hosted acceptance is the manual run above. |
| Final loader/test follow-up, starting at `fbcdba823` | Focused RocksDB/FileSystem suites passed; 77 common + 63 email search units passed; shipping configuration, formatting and workflow checks passed. First canonical attempt failed at a later time-cap SELECT; unchanged retry passed in 1,438.74 s. | Preserve the initial failure separately; its cause remains unexplained. |
| `82b8984aac77a2fe718eb37f8f59110bdbbf0305` | Both linked CI runs succeeded; Inventory and Coverage passed. | Final pre-merge CI acceptance. |

Whole-branch source review and runtime evidence are separate. The PR84 retry-follow-up
and account-retry checkpoints each recorded a fresh whole-branch source review against
base `92a41596a571e164d3e88b3fa5f0c9eb6d5e0660`; subsequent account-retry deltas received
scoped review. The later CI-settlement and final loader/test reviews covered their
bounded follow-up changes, not a new whole-feature-branch review. These PR84 reviews
used inherited parent routing because its capability ceiling was unresolved; no
additional named Astra review is claimed.
Development links retained existing macOS compact-unwind/dependency diagnostics;
warning-free claims apply only to the explicitly recorded shipping builds/checks.

### CI timeout, setup and retention corrections

Three separate changes were qualified:

- **Differential setup:** all 3,258 paired inputs, three paired APPEND fallbacks,
  serial delivery order, stored-MIME checks, task/search → SMTP → task/search barrier,
  page size 400, 183 forms, K deviation and G5 stayed. Fixture polling became 10 ms
  and each identification batch unlocked once while still authenticating/opening each
  metadata stub and MIME blob. On one sequential local A/B run, setup fell from
  582.54 to 379.65 s and full-suite time from 1,657.27 to 1,451.18 s. The filesystem
  candidate also passed; there was no unchanged filesystem baseline or speedup claim.
- **Five-second Email/get timeout:** instrumented hosted control `5dfc21fc5` completed
  251, 251 and 253 of 400 metadata opens before timeout/unwind; inclusive opening
  measurements were 5.617–5.643 s and include work after client expiry. Candidate
  `ef6e0470512693b9a4930cd610e77e38ea05c0dc` changed only the test-package
  `curve25519-dalek` optimization from 0 to 3, plus documentation. Both complete ZA
  storage-mode steps passed first attempt; selected Get requests took 557.602 and
  553.350 ms with all 400 opens complete. Client deadlines, first-party optimization,
  corpus and assertions stayed unchanged. This supports the scoped test-build
  correction, not shipping performance or a general blob-backend diagnosis.
- **Retention:** the profile candidate still failed Inventory on one libtest and
  24 function-step records. Approved exact moves/expectation dispositions and restored
  metadata checks fixed that separate gap. The [retention ledger](2026-11-14-search-ci-retention-ledger.md)
  preserves every identity, original step and required composite equivalent. No
  baseline, gate algorithm, selection rule or coverage threshold was reduced.

The 120→240-minute ZA job budget addressed enclosing-job cancellations, not the
request timeout. Diagnostics retained fixed labels and safe aggregate stage data;
returned operations can contain errors, and an unmarked drop does not identify its
cause. Authentication and response serialization are outside the measured server stages.

## Benchmarks

These measurements used fresh local RocksDB stores on an Apple M1 Max with 10 cores
and 64 GiB, stable Rust 1.99.0. The format/system rows use the unoptimized test
profile; the optimized Rust and OSS runs are separate artifacts and fixtures.
Detailed results and reproduction receipts remain in the archived reports.

### Format benchmark

The final repeat at `4dde13ade2be92411d76eb7906b14a0423359dc9`, with code/tests
identical to `53062c367ac0b85596017fed6ebb50edd8a66567`, measured all 15,000 seed-1
messages: 1,217,729,696 MIME bytes and 4,500 text attachments. The attachment-disabled
control checked the attachment canary's absence and the body canary's presence.

| Final format figure | With positions | Without positions |
|---|---:|---:|
| Raw term bytes | 436,627,716 | 265,473,368 |
| Per-record Term-compressed bytes | 224,811,344 | 79,755,932 |
| One frame over all records | 208,245,428 | 58,957,049 |
| All per-record Term decompression | 3.136853 s | 2.272307 s |

Positions remain necessary for phrase matching; upstream Term compression remains
selected. The positions-bearing frames project to 449,622,688 B at 30,000 messages;
this is an extrapolated format payload, not measured loaded memory or sealed storage.
Thread records averaged 88.160 B and reached the derived 304 B bound. Open/index
rates were 28.778 documents/s with attachment text and 59.343 without. Text-only
attachments and test-mode unknown-language selection limit broader interpretation.

### Final-head system benchmark

“Final-head” here means historical `4dde13ade`/`53062c367`, not merged PR84.
The full run completed in 10,201.56 s of libtest time. It delivered 30,000 seed-1
messages, 9,000 attachments and 2,436,029,084 MIME bytes with the then-current
fourteen search settings unchanged.

| Observation | Result |
|---|---|
| Attachment-free / mixed pass rate | 98.62 / 21.14 messages/s over separate 2,000-message cohorts. |
| Normal 30,000-message indexing | 655 commits; 4,558.263 s committed wire time. All publishing requests exceeded the 100 ms target cadence. |
| Sealed Content | 452,374,470 B (431.42 MiB), leaving 80.58 MiB below the 512 MiB cap. |
| Full-size cold query | `unsupportedFilter` / `not_current` after 188.974 s, 20 blob reads, zero commits. Positive coverage and linked warm-repeat assertions were not reached. |
| Canonical H7 | Cursor exhausted at 1,896.006 s; first successful Content query at 2,065.774 s. A limit-1 wire match plus separate exact-head checks covered all 30,000 IDs in Content and Thread. |
| H7 rewrite bounds | 653 commits, 47 refusals, maximum chain 31 against bound 46; tiered rewrite/original-input ratio 1.765×, within 3×. |
| Separate warmed growth | Approximately 550.56 MiB modeled/charged resident Content and 3.68 MiB Thread at 30,000. Direct milestone warming; no separate wire query or complete ID census on this fixture. |
| Compaction timing | Longest normal joint full-Content/Thread request 155.733 s; separate warmed final full merge 154.838 s. These are request spans, not isolated stage timings. |
| Small cap lifecycle | At 1 MiB with segment limit 255, warm/cold requests refused `over_cap`; Map/epoch stayed available and cap increase restored matches/full fixture coverage. |
| Local point validation | Keyed median/p99 0.381/0.672 ms, plain 0.275/0.438 ms; networked PostgreSQL unmeasured. |

The highest sampled Content charge was 1,295.17 MiB and Thread charge 63.23 MiB.
Whole-process RSS peaked at 9,701,801,984 B in the separate warmed phase. Ledgers,
model estimates and RSS measure different things; sampled peaks are lower bounds
and different columns can peak at different times. Physical crossing of the default
512 MiB account cap was not observed.

Earlier attempts remain distinct: `28f84bdc6` failed with an unclassified send error;
`f7c02e5bc` reached a 60-second client send deadline; `f4eb2ecc` completed after a
benchmark-only 600-second wire ceiling, but cold and post-H7 Content were unavailable.
The longer ceiling changed the observation window, not product deadlines or latency.
The 2-second planner interval bounds item admission, not completion of owned work.
Historical refusal spans do not establish a common cause or attribute it to WB2.

### Optimized cold-start qualification

Both experiments used clean source `9ea951df965af634611fb6311afa987736f61242`, the
verified 30,000-message seed-1 corpus and the original fourteen search settings.

| Experiment | Cold / immediate repeat | Evidence and boundary |
|---|---|---|
| Optimized Rust after search-cache eviction | 26.469407209 s / 507.458 µs; 20 / 0 verified sealed reads | Both queries matched; exact-head internal census covered every delivered ID; durable manifest/Email state unchanged and zero indexing-writer commits. Same process and warm host filesystem caches. Test artifact included test_mode and enterprise. |
| Separate optimized OSS after process restart | 23.053401084 s / 1.026083 ms | Both matched the baseline ID/state; matching 30,000-ID public censuses before/after restart. Clean shipping binary without test_mode or enterprise; clean shutdown and runner exit 0. Host filesystem caches retained. |

The Rust measurement completed before a Tokio worker teardown panic: a timer was
polled after its driver stopped. Libtest/raw exit was zero; the panicking task and
cause remain unknown. A five-message diagnostic did not reproduce it. The result is
accepted with this anomaly open, not as a panic-free run.

The first shipping smoke failed strict query-state equality after restart. A reviewed
fixture initializes the mailbox through public Mailbox/get before delivery; corrected
smoke/full runs passed without weakening the census/state checks. This supports the
setup-order explanation, but the original extra state revision remains unattributed.
The optimized and unoptimized fixtures/chains differ, so their timings do not isolate
optimization's effect. The shipping search gate is distinct from the broader fresh-store
CI gate, which also covers calendars, recurrences, pins, shares and ordinary accounts.

### Defaults

Retain provisional `ZA_SEARCH_ACCOUNT_MAX_MB=512`, `ZA_SEARCH_MEM_MB=1536` and
`ZA_SEARCH_THREAD_MEM_MB=256` MiB. Every cluster node must share the account cap,
which must be at most one-third of the Content budget. These measurements support
sealed headroom and resident growth but do not establish final production sizing.
The old measured `ZA_SEARCH_SKIP_AFTER=8` setting is historical: revision 34 retired
it, and current code ignores it. A recurring transient never consumes a message.

## Mutation runs (plan 16)

All M1–M79 rows are accounted for across three campaigns, not all killed. The first
19-target campaign used `bcc81d177` (production/tests identical to `53062c367`);
the 55-draft and remaining-row campaigns used `bae62db6f08af27b1058c148a2fbe292f18635e9`.
Each executed selection had a passing correct-code baseline and restored control.
Faults were temporary and restored by hash; none was committed.

| Disposition | Rows and limits |
|---|---|
| First 19 targets | Eighteen detected. T13 standalone/Map caller minima were detected; the original Content route survived. Its new fixture passed correct code and caught the fault, then integrated and passed both complete ZA blob modes. |
| Historical survivors | M16, M23, M48. Later tests added a real-pass pending-cursor state, stronger mixed-record planner ordering, and live/deleted held-insert checks. All 18 planner tests and canonical ZA passed, but no faulty variants ran against those strengthened tests. |
| Unexecuted | M29b: faithful hidden-section tombstones require a codec/type change, not a bounded fault. It is not an equivalent or passed mutant. |
| Alternate catcher / named route gap | M14: identical M50 unit patch; M19: earlier atomic-link tests; M22: retention/thread-page tests, named F10 passes; M31: unit and R22, named R16/R20 unreached; M43a: unit/two wire tests, named folded-alias route unreached; M44: units, named R24/G2 passes; M49: unit/G12, R7 nondiscriminating. |
| Other observation limits | M1's symmetric AAD wire control, M30's R21 and M40's bad-tag-free R3 do not discriminate, despite unit detection. M29a's F1 wire oracle was masked by a debug assertion; M64's injected-error branch was unreached. |

The M16 addition establishes the resident state directly rather than scheduling the
complete cross-node race. M23 tests the planner contract; a normal Email/set create
and destroy commits separately and does not establish a mixed log record. Correct-code
passes do not retroactively change historical survivor verdicts.

Fault execution used focused ZA/module or named unit selections, RocksDB only; it did
not qualify full-suite, FileSystem or other-store fault execution. M26's first-campaign
follow-up relaxed two masking assertions; later masked-catcher follow-ups removed whole
dispatch calls without altering assertions. Baseline failures and interrupted runs are
retained separately. M35's secondary admission check, M18's after-failed-CAS assertion
and M58's later recovery variants were unobserved in their first campaign. The final
campaign demonstrated M7/M21/M27/M33 at their named cases, closing those earlier gaps.

## Remaining limitations and finding status

- Comment [4566621670](https://github.com/jaylaney/stalwart/pull/84#discussion_r4566621670)
  remains unresolved: manifest-read errors are flattened before typed trace enrichment,
  and secondary diagnostic-write failures are not separately classified. The safe
  one-primary-abort contract remains; raw errors are not logged.
- The last local follow-up's first canonical run failed at a repeated backlog-draining
  SELECT's 1.5-second read deadline after the new cases passed. An unchanged retry
  passed. Four diagnostic processes/48 SELECTs did not reproduce the failure; neither
  clock pause nor overlapping compilation is established as its cause. The historical local
  SMTP full attempt exited 101 (75 passed, one failed, three ignored); its permitted
  exact isolated retry passed without establishing the first failure's cause.
- Native-client behavior, physical multi-node execution, networked-store timing,
  cold-backlog IMAP wait-budget measurement and broader attachment-format coverage
  remain outside the completed evidence. Shared-store race fixtures are simulations,
  not two-node qualification. Memory/default sizing and mutation exceptions remain open.
- The optimized Rust teardown panic and original shipping-smoke state transition retain
  the qualifications above. Historical fixture/setup failures stay in the archive;
  later passing runs do not prove their causes.
- Accepted semantic residuals remain in design sections 5.7 and 7.5: stale setup-token
  lifetime, request-driven freshness, K/missing-evidence threading differences,
  IMAP identity reassignment without CHANGEDSINCE, and processing-cost timing leakage.
  queryChanges uses historical identity/existence with current readable filter/sort
  values, not reconstructed historical ranks. These are contract boundaries, not
  newly discovered defects or waived runtime checks.

WB1–WB3, warm admission, the actual-map-order mutation gap and T13 integration are
closed at their recorded sources. The formerly deferred ordinary/mixed SAVE self-wait
was reproduced and fixed during PR84; it is no longer an open item. Revision 34's
never-skip-transient/periodic-eviction rules supersede the old plan outcome wording.
The [decision index](2026-11-14-zero-access-search-evidence-index.md) separates these
closures from current risks and links their complete records.

## Reproduction and evidence

Historical receipts bind the exact source, features, environment, binary and logs.
Use those receipts to reproduce a particular result; running the same command at a
new head is a new measurement. Cargo uses stable Rust with the Homebrew rustup path.
Integration suites share ports and run sequentially.

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::search_bench::format -- --exact --ignored --nocapture
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::search_bench::system -- --exact --ignored --nocapture
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --exact --nocapture
STORE=RocksDb BLOB_STORE=FileSystem RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --exact --nocapture
```

The optimized Rust experiment built with
`cargo test --release --locked --offline -p tests --lib --no-run -j2`, then invoked
`za::search_bench::cold_start --exact --ignored --nocapture` on that frozen executable.
The separate OSS build used
`cargo build --release --locked --offline -p stalwart --no-default-features --features rocks -j2`.
[Shipping cold-search procedure](../../../tests/product/cold-search.md),
[retention gate procedure](../../../scripts/tests/README.md), and the
[evidence roots/archive index](2026-11-14-zero-access-search-evidence-index.md#evidence-roots)
provide the remaining fixture, command and provenance details.
