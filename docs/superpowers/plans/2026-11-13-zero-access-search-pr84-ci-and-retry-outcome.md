# PR 84 CI diagnosis and retry settlement outcome

Starting head: `eaff7efcd75bb747fb3d10cd3c151186763eebb2`.
Status: local implementation, independent review and validation complete. The hosted
request-timeout cause remains unproved; the instrumented CI result is still required.
Evidence is retained in the checkout's ignored directory
`.superpowers/sdd/2026-11-13-pr84-ci-timeout/`.

## Verified CI history

Six completed canonical ZA invocations across three heads failed at the generic
JMAP client's five-second deadline, `tests/src/utils/jmap.rs:439` at those heads.
Three other invocations were cancelled by their enclosing job budgets. All six
failures follow the differential bulk-index boundary 3259 by 8.769–12.533 seconds.
That interval includes preceding work and is not the duration of the failing request.

| Head / run | Completed failures | Other invocations |
| --- | --- | --- |
| `5df77aff0`, [43270913937](https://github.com/jaylaney/stalwart/actions/runs/43270913937) | two default-store attempts | first filesystem attempt cancelled at the two-hour job limit |
| `b02015cee`, [43286973675](https://github.com/jaylaney/stalwart/actions/runs/43286973675) | first default-store attempt | default retry cancelled at the two-hour limit; filesystem skipped |
| `eaff7efcd`, [43326034213](https://github.com/jaylaney/stalwart/actions/runs/43326034213) | two default-store attempts and first filesystem attempt | filesystem retry cancelled at the four-hour limit |

These are shell-level retries inside workflow attempt 1. The older run was manual;
the later two were PR-triggered. Completed CI invocations took 50–57 minutes to
reach the boundary. The preceding local full run passed in 1,690.17 seconds at
`eaff7efcd`; that local success does not explain the repeatable hosted failure.

The corpus contains 3,258 messages per account; 3259 is its Email-log boundary.
`za_index_current` uses a separate 45-second client, so the five-second helper panic
must occur later. Candidates begin in keyed inventory: Mailbox/get, Email/query
pages, 400-message Email/get pages and Thread/get pages. Source inspection identifies
serial sealed-metadata decryption in Email/get as a plausible cost, not a measured
cause. The new labels and backtrace must establish the actual failing call first.

The 240-minute job budget addresses a later enclosing-job cancellation. It does not
fix the request-level failure. No further budget or client-timeout increase follows
from the present evidence.

## Accepted review corrections

- [4565479001](https://github.com/jaylaney/stalwart/pull/84#discussion_r4565479001):
  caller deadline expiry during a pass load must not become an observed store fault
  or increase the account's failure streak. A prior actual failure survives a later
  cancelled wait; normal authenticated progress settlement remains intact.
- [4565479018](https://github.com/jaylaney/stalwart/pull/84#discussion_r4565479018):
  an acknowledged own-abort write must rebind its current failed probe without
  changing the streak, class or deadlines. Its reconciliation/suspension bookkeeping
  does not publish its discarded prefix. Independent meaningful observations,
  assertion winners and genuine progress keep their normal clearing rules.
- [4565479007](https://github.com/jaylaney/stalwart/pull/84#discussion_r4565479007):
  restore bounded diagnostic detail using an allowlisted backend class and a fixed
  local operation label on the first primary abort. Raw logging is rejected:
  retained-log corruption errors can carry stored key and value bytes. No original
  error payload, arbitrary context or nested cause is copied into search traces.

Design revision 37 makes those distinctions explicit. The reviewed cold-unknown
first-authentication behavior remains intentional. The shared read guard's backend
wait is a performance observation, not an established CI cause. The known test-only
cadence and short deadline setup races are included in this correction's scope.

## Diagnostic changes

The zero-access CI job sets `RUST_BACKTRACE=1`. Gate validation requires that exact
job-scoped setting and rejects step overrides or extra environment variables. Panic
diagnostics are explicitly separate from the historical capture/net measurement
environment; the qualification baseline is unchanged.

Differential tests add fixed phase, mode, method, page/count and elapsed-time labels.
They distinguish a returned helper from unwinding or a dropped unfinished future.
Payloads, deadlines, page sizes, corpus, assertions, suite order and reruns are
unchanged. These labels do not identify CPU, storage, lock or transport cost within
the server; process termination can leave only a begin label.

In the local canonical run, currentness completed in 4.479 seconds on its separate
45-second client. All following generic five-second inventory requests completed.
Keyed 400-message Email/get pages took 503.949–538.014 ms; plain pages took
12.506–16.822 ms. The final 58-message pages are excluded from those ranges.
`local-diagnostics.md` links these observations to exact log lines. They confirm a
local cost difference, but do not identify the hosted failing request or establish
its CPU, storage or lock cause. This is one unoptimized functional run, not release
performance qualification. Client deadlines and page sizes remain unchanged.

## Validation record

- RED: `red-own-abort/result.json`, exit 101. Both local suspension and
  reconciliation-bound abort-commit regressions failed against old production:
  the retry slot was None instead of retaining the recorded schedule.
- Diagnostic gate checks: `diagnostic-gates/result.json`, exit 0. Zero workflow
  differences; 43 tests passed and two existing baseline-data tests skipped.
- Independent static reviews approved the backtrace configuration and differential
  labels (`backtrace-review.md`, `diagnostics-review.md`). Independent kernel triage
  is in `retry-comments-triage.md`. Separate requirements and correctness reviews
  approved the frozen retry candidate (`retry-conformance-review.md`,
  `retry-correctness-review.md`); these are bounded source reviews, not runtime
  qualification or a new whole-branch review.
- Common/email search units: `retry-units/result.json`, exit 0; 77 and 63 passed,
  with unchanged source hashes during execution.
- Initial focused attempt: `retry-focused/result.json`, exit 101 at compilation.
  Two new helper arguments borrowed `SessionKeys`, so cloning them retained a
  reference that could not enter a spawned task. They now borrow the existing
  `Arc<SessionKeys>` and clone its owned handle. No integration case ran in this
  attempt; the correction is test-only.
- Second focused attempt: `retry-focused-2/result.json`, exit 101 after 24.29
  seconds of runtime. The new own-abort fixture assumed a nonzero retention bound
  on a fresh untruncated log. The API reports only actual truncation markers, so
  this setup now checkpoints a flag update, truncates the Email log through the
  real API, verifies the marker, and then appends the pending prefix and tail.
  Existing discard, commit, cancellation and scheduling assertions are retained.
- Final focused attempt: `retry-focused-3/result.json`, exit 0; all focused PR84
  cases passed in 66.06 seconds (130.71 seconds including compilation). The
  independent correctness reviewer checked both test-only corrections and approved
  the final scoped diff, SHA-256
  `5e097c0bf87616a7936beeea96f9f9f3cdbe5b5594b328d944f002e49d8f5d09`.
- The focused binary was retained as `final-tests-bin`, SHA-256
  `3d82685d6c06f4c4a6c6a7f4f13a27b7c881e664fdfdf5909886a70aecd7fc99`.
  `final-binary.json` binds it to the final Rust sources and focused receipt. The
  canonical suite passed with that same binary (`canonical/result.json`, exit 0):
  1,692.89 seconds of test runtime, 1,698.26 seconds including harness startup and
  exit. All 183 paired differential forms had zero differences beyond the pinned
  K deviation, and final cleanup completed. Final Rust-source hashes match the
  focused and canonical receipts.
- Shipping configuration: `shipping-check/result.json`, exit 0 in 185.91 seconds,
  using `cargo check --locked --offline -p stalwart --no-default-features --features
  rocks` with two Cargo build jobs. Later corrections change integration fixtures
  only; production sources remain unchanged.
- Workspace formatting: `final-format-2/result.json`, exit 0. The test link emitted
  the existing macOS large `__eh_frame` compact-unwind warning, also present in the
  preceding follow-up's retained logs; compilation and the focused run completed
  successfully. Final diff hygiene passed.

The runner sets `STORE=RocksDb`, `RUST_MIN_STACK=16777216`, `RUST_BACKTRACE=1`,
prepends the stable rustup path, and uses the existing isolated Cargo target.
Relevant commands were:

```sh
cargo test --locked --offline -p common -p email --lib --features test_mode,enterprise,common/test_mode za::search:: -- --nocapture
cargo test --locked --offline -p tests za::mail::threads::pr84::pr84_regression_tests -- --exact --nocapture
.superpowers/sdd/2026-11-13-pr84-ci-timeout/final-tests-bin za::za_tests --exact --nocapture
cargo check --locked --offline -p stalwart --no-default-features --features rocks
cargo fmt --all --check
python3 -I scripts/tests/ci_steps.py
python3 -I scripts/tests/test_gates.py
```

The passing focused cases include actual timer expiry on blocked initial index and
load-flight locks. Required-loader, winner, observer and Content cases use an
acknowledged explicit-expiry seam to establish ordering and the unchanged caller
deadline; they do not measure backend latency or prove cancellation of backend I/O.
Own-abort cases cover reconciliation/suspension with saturated and in-window-absence
diagnostics, prefix discard, exact schedule preservation before swap cancellation,
and no repeated work on the next deferred call. Raw-error canaries exercise typed
and fallback classifications; other fixed backend labels have source coverage.

No new release-performance, physical multi-node, native-client or broad mutation
qualification is claimed. Agents deliberately inherited the supported parent model
route because the originating capability ceiling was unresolved; this is not an
additional named Astra review. PR 84 remains ready; no merge or deployment.
