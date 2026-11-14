# Search CI curve-profile candidate outcome

Status: accepted as the test-build timeout correction after the hosted comparison
and integrated with PR85 into PR84's branch. PR84 remains out of main until combined
CI is green. The previous candidate's separate Inventory gate failure led to the
retention repair and fresh verification described in the 2026-11-14 outcome.

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
curve25519-dalek dependency. The completed hosted comparison below now isolates
the slow stage and confirms the candidate against the unchanged request deadline.

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

## Hosted comparison and acceptance

The [instrumented hosted control](https://github.com/jaylaney/stalwart/actions/runs/43497837906)
at `5dfc21fc5abbe34ca930e27f6cbffdf90a5ad4c0` and the
[hosted candidate](https://github.com/jaylaney/stalwart/actions/runs/43509717996)
at `ef6e0470512693b9a4930cd610e77e38ea05c0dc` have finished. Their source differs
only in the test-package override and documentation.

The control's two default-store attempts and first filesystem attempt timed out
at the selected request. At unwind, they had completed 251, 251 and 253 of the
400 metadata opens, with another open in flight; metadata opening accounted for
5.617, 5.628 and 5.643 seconds inclusive. Snapshot work took about 0.2 ms and the
completed metadata batch read about 1.4-1.6 ms. These sampled unwind durations
include work after client expiry; they are not final request-completion timings.
The control job was cancelled during its filesystem retry, so that fourth
invocation supplies no result.

The candidate passed both complete ZA storage-mode steps on their first attempts:

| Hosted mode | First keyed 400-message Get | 400 completed metadata opens | Full suite |
| --- | ---: | ---: | ---: |
| Default | 557.602 ms | 514.318 ms | 3,166.02 s |
| Filesystem blob store | 553.350 ms | 509.592 ms | 3,028.26 s |

Both observations were claimed, with all 400 opens and required stages completed,
no partial/in-flight required work, all 183 comparison forms, the documented K
deviation and cleanup. The same five-second client remains in use. This supports
the scoped curve test-profile correction; it is not a shipping performance claim.

The candidate run is still **not green overall**: its Inventory gate reports one
deleted libtest and 24 absent function-step records. The affected coverage-net
steps and Coverage gate passed. The net's SMTP queue retry case failed its first
attempt and passed the existing retry. Unit/upstream/protocol suites, product,
fresh-store restart and independent CalDAV jobs passed. The inventory failures
include exact moves, approved changed expectations and metadata checks that need
restoration; see `2026-11-14-search-ci-retention.md`. No baseline/gate/coverage
threshold reduction is accepted.

## Combined preparation and remaining integration

The local combined tree includes reviewed PR85 `cc5de6a55`, the correction and
diagnostics, and the cadence assertion that checks production reservation before
the manual test gate. A fresh independent review approved the entire follow-up
diff against `9b1df344b` without findings. Its complete canonical ZA suite passed
in 1,428.06 s (wrapper 1,433.69 s): 3,258 pairs, all 68 batches, all 183 forms,
expected K deviation, G5 and cleanup. Setup took 373.166 s and the selected Get
283.610 ms. The build passed in 208.64 s; formatting/diff checks passed.

All nine recorded source hashes and the frozen binary were reverified against the
run. Binary SHA-256: `7c198fa52de45be4a7d31938a2dc1f260326ed5fe31b9ea2606ddc4881d327ff`.
Log SHA-256: `a67c63f53e2ffb39b8c7d03b21f803a74c0af30ce4be6671f3917e675ee47d67`.
These results precede the new protocol metadata-retention restorations and do not
qualify those pending edits. Combined evidence is retained in the sibling ignored
ledger `2026-11-13-pr84-search-ci-combined`.

The retention repair subsequently passed independent review and both complete
local key-mode protocol suites. PR85's actual merge was incorporated without
further file changes, and the combined correction was published to release7-search.
The final CI and strict retention counts remain pending. PR84 stays open until
that CI is green; current run links are recorded on the PR.

Evidence is retained under `.superpowers/sdd/2026-11-13-search-ci-curve-profile/`
in the diagnostic checkout. Agents deliberately inherited the parent route because
its capability ceiling was not exposed; no named-model review claim is made.
