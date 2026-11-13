# Search CI curve-profile candidate outcome

Status: controlled candidate, not yet accepted as the hosted timeout fix or
integrated into PR84. The owner authorized the evidence-backed correction and
PR85 integration, with PR84 held out of main until combined CI is green.

## Evidence motivating the experiment

PR84 run 43469202844, job 132036569137, finished with four failures at the first
keyed inventory Email/get of 400 messages: two default-storage attempts and two
filesystem attempts, all on the unchanged five-second client. Currentness and
all preceding query pages completed. The matching failures do not distinguish
server stages or prove a blob-backend cause. Full logs and exact excerpts are
retained in the ignored diagnostic ledger's hosted directory.

The separate instrumented local control passed its complete ZA suite and
recorded 537.977 ms for the request, including 515.672 ms for 400 metadata opens.
Source inspection places the scalar-multiplication implementation in the pinned
curve25519-dalek dependency. Hosted control run 43497837906 remains in progress;
its server-stage log is not yet available to the API.

## Candidate and local validation

Only `[profile.test.package.curve25519-dalek] opt-level = 3` was added. No product
code or profile, dependency version/feature, first-party optimization level,
assertion, request deadline, corpus, property, test selector or retry was changed.
The existing Argon2 test-package optimization remains intact.

Independent source/configuration review and a subsequent compiler/unit receipt
audit found no blocking finding. Formatting and diff checks passed. The affected
integration target compiled in 493.68 s; all 47 vault unit tests passed. Existing
dependency and linker warnings remain; this is not a warning-free build claim.

The complete canonical local ZA suite passed in 1,710.95 s (wrapper wall time
1,716.96 s), with all 3,258 paired inputs, the three expected IMAP fallbacks,
bulk boundary 3259 after 68 keyed requests, all 183 forms with zero differences
beyond the pinned K deviation, G5 and cleanup. It used the frozen candidate
binary below, with `STORE=RocksDb`, `RUST_MIN_STACK=16777216`, `RUST_BACKTRACE=1`,
no `ZA_*` selector and no filesystem blob override. The binary was invoked with
`za::za_tests --exact --nocapture`. No local filesystem candidate run was made.

| Same selected 400-message request | Default-profile control | Candidate |
| --- | ---: | ---: |
| Client elapsed | 537.977 ms | 296.601 ms |
| Method, inclusive | 528.053 ms | 287.037 ms |
| 400 metadata opens, inclusive | 515.672 ms | 273.834 ms |
| Metadata batch reads | 1.247 ms | 1.280 ms |
| Snapshot | 0.104 ms | 0.129 ms |

Both observations were claimed and recorded 400 completed opens, one completed
snapshot and no partial or in-flight required stages. The local request reduction
supports the candidate; it is one sequential A/B observation, not hosted
attribution, a product performance result or a general whole-suite speedup claim.
Control libtest duration was 1,726.94 s. The candidate run's log SHA-256 is
`e44baf4a17497d479ac19e61821a1904268ee25d49cdf5de723e2fcdd1f2aefb`.

The compiler comparison contains 672 matching artifact identities (692 artifact
events) and unchanged
features. Only curve25519-dalek's library and build-script optimization changed
from 0 to 3. Every other recorded profile property is identical, including enabled
debug assertions and overflow checks. First-party tests and the X25519 wrapper
remain at optimization level 0.

Control frozen integration binary:
`8c1357bcad4cee5a70aa6507ae2f73f7f55268f104eac0acb4292afd7df88798`.
Candidate frozen integration binary:
`6bb6f1b653071e087b9388489a686113a33df85a0e468eefab9c02bf3370b2f7`.

## Remaining decision and integration

The [instrumented hosted control](https://github.com/jaylaney/stalwart/actions/runs/43497837906)
at `5dfc21fc5abbe34ca930e27f6cbffdf90a5ad4c0` and the
[hosted candidate](https://github.com/jaylaney/stalwart/actions/runs/43509717996)
at `ef6e0470512693b9a4930cd610e77e38ea05c0dc` remain pending. Their source differs
only in the test-package override and documentation. The candidate was published
to run this controlled comparison; no hosted success or fix is claimed yet.

Compare the unchanged selected request's stage counters and timings on the hosted
runner. Local acceleration alone is not hosted attribution, and this test-profile
setting is not a shipping performance change. If the hosted
comparison does not support this correction, reconsider it without changing the
deadline or shrinking the workload.

After acceptance, integrate the correction/diagnostic and reviewed PR85 onto
release7-search, review their combined fixture edits and run the required checks
on the exact combined source. PR84 remains open until that CI is green.

Evidence is retained under `.superpowers/sdd/2026-11-13-search-ci-curve-profile/`
in the diagnostic checkout. Agents deliberately inherited the parent route because
its capability ceiling was not exposed; no named-model review claim is made.
