# PR84 keyed Email/get diagnostic outcome

This isolated diagnostic is based on PR84 head `9b1df344b`. It does not change
PR84's branch or PR85's corpus optimization. Source review, formatting, the affected
integration-test build, both focused observer controls, shipping check and complete
local ZA suite passed. The code is
committed as `5dfc21fc5` on `codex/pr84-email-get-timing`. Hosted stage measurements
remain pending; no timeout fix or hosted cause is claimed.

## Scope

Only the first keyed inventory Email/get is armed. Its 400 IDs, seven properties,
generic client and five-second deadline remain unchanged, as do the full corpus,
comparison assertions and subsequent requests. The opaque correlation call ID
is the sole wire-body difference. No production performance fix is included.

Test-mode spans report dispatcher indexing admission, the method, snapshot
acquisition, metadata batch reads and cumulative metadata row opening. A fixed
aggregate block appears on success or fixture unwind/cancellation. Counts
separate started, returned, unmarked dropped and still-active operations. A
returned operation may contain an error; an unmarked Drop does not prove why it
ended. Timings are inclusive and sampled per stage, including active elapsed
time. Authentication and response serialization remain outside these stages.
No account/call IDs, message content, keys or raw requests/responses are printed.

## Review and verification

Independent source review found no actionable findings. It checked request and
coverage preservation, claim isolation, concurrent sampling, unwind reporting,
legacy observers, privacy and feature boundaries. Agents inherited the parent
route because the originating capability ceiling was not exposed; no named-model
routing claim is made. Root inspected the actual diff and owns execution.

Formatting and diff hygiene pass. The affected integration target compiled in
448.36 seconds. The new common observer control and the existing cold-query
observer control each passed (one test each). Their intentional caught panics
exercise unwind cleanup and are not failed checks. The integration link emitted
a macOS compact-unwind-table size warning; compilation and the unwind controls
passed.

The shipping `cargo check -p stalwart --no-default-features --features rocks`
passed in 286.22 seconds. The complete `za::za_tests --exact --nocapture` passed
in 1,726.94 seconds with RocksDb, default blob storage, a 16 MiB Rust minimum
stack and backtraces enabled. It retained all 3,258 paired messages and three
fallbacks, boundary 3259 after 68 requests, all 183 forms with only the pinned K
deviation, G5 and cleanup. No filesystem diagnostic run was performed. Builds
and suites ran serially after PR85's timed validation.

The selected request claimed its observation and returned all 400 rows; every
required span completed with no partial or in-flight operation. The fixed report
contained its expected 24 lines. Independent receipt review verified source and
binary identity, log hashes, local results and these diagnostic counts.

| Local measurement | Elapsed | Completed operations |
| --- | ---: | ---: |
| Client request | 537.977 ms | 1 |
| Dispatcher pre-pass | 0.217 ms | 1 |
| Method | 528.053 ms | 1 |
| Snapshot | 0.104 ms | 1 |
| Metadata batch reads | 1.247 ms | 2 |
| Metadata row opening | 515.672 ms | 400 |

These are inclusive measurements from one local test-profile run, not disjoint
costs to add together or a performance comparison. Metadata row opening dominates
this local method; this does not establish the hosted timeout's cause, or isolate
which operation inside row opening is responsible.

[Diagnostic CI run 43497837906](https://github.com/jaylaney/stalwart/actions/runs/43497837906)
is bound to code commit `5dfc21fc5`. Style/gates, unit tests, upstream and ordinary
protocol suites, product build, fresh-store restart and independent CalDAV client
passed. ZA and retention remain running. This is a status snapshot, not a final
CI pass. The subsequent outcome/plan update changes documentation only. The
fresh-store success does not turn PR85's separate initial failure into a pass or
establish its cause.

## Evidence and interpretation

Ignored evidence is retained under
`.superpowers/sdd/2026-11-13-pr84-email-get-timing/` in the diagnostic worktree,
including source hashes, the frozen review diff and independent review report.
Build/run receipts bind commands, source and binary identities and logs. The
frozen integration binary SHA-256 is
`8c1357bcad4cee5a70aa6507ae2f73f7f55268f104eac0acb4292afd7df88798`.

The original hosted first keyed Get hit the client timeout; the unchanged local
baseline completed it in approximately 542 ms. Neither that difference nor this
instrumentation establishes a crypto, storage, scheduling or lock cause. A
performance change should follow actual hosted stage evidence.
