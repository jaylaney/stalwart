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

## Candidate and validation so far

Only `[profile.test.package.curve25519-dalek] opt-level = 3` was added. No product
code or profile, dependency version/feature, first-party optimization level,
assertion, request deadline, corpus, property, test selector or retry was changed.
The existing Argon2 test-package optimization remains intact.

Independent source/configuration review found no blocking finding. Formatting
and diff checks passed. The affected integration target compiled in 493.68 s;
all 47 vault unit tests passed. The complete canonical local ZA suite is running.
The candidate is published solely to run the controlled hosted comparison while
local validation continues; these pending runs are not passing qualifications.

The compiler comparison contains 672 matching artifact identities and unchanged
features. Only curve25519-dalek's library and build-script optimization changed
from 0 to 3. Every other recorded profile property is identical, including enabled
debug assertions and overflow checks. First-party tests and the X25519 wrapper
remain at optimization level 0.

Control frozen integration binary:
`8c1357bcad4cee5a70aa6507ae2f73f7f55268f104eac0acb4292afd7df88798`.
Candidate frozen integration binary:
`6bb6f1b653071e087b9388489a686113a33df85a0e468eefab9c02bf3370b2f7`.

## Remaining decision and integration

Compare the unchanged selected request's stage counters and timings locally and
on the hosted runner. Local acceleration alone is not hosted attribution, and
this test-profile setting is not a shipping performance change. If the hosted
comparison does not support this correction, reconsider it without changing the
deadline or shrinking the workload.

After acceptance, integrate the correction/diagnostic and reviewed PR85 onto
release7-search, review their combined fixture edits and run the required checks
on the exact combined source. PR84 remains open until that CI is green.

Evidence is retained under `.superpowers/sdd/2026-11-13-search-ci-curve-profile/`
in the diagnostic checkout. Agents deliberately inherited the parent route because
its capability ceiling was not exposed; no named-model review claim is made.
