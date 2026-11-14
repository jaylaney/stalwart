# PR84 keyed Email/get timing

The current hosted ZA failure is the first keyed inventory `Email/get` of 400
messages. The same request passed locally in 542 ms. Source inspection identifies
serial sealed-metadata opening and a dispatcher indexing check as plausible
costs; neither is a measured hosted cause. The owner approved one focused
diagnostic patch before selecting a fix.

## Contract

Preserve the complete canonical suite, 3,258 corpus messages, the same first
400 IDs and seven properties, five-second client deadline, all assertions,
method ordering and production behavior. Do not change crypto, cache policy,
authorization, lifecycle fences, scheduling, timeouts or coverage in this patch.

Reuse the existing test-mode request observation infrastructure. Permit an armed
singleton `Email/get` to claim an observation by its exact account and unique call
ID, preserving existing `Email/query` behavior. Arm only the first keyed inventory
Get. Existing unarmed requests must remain unobserved. Use the existing client
helper so its TLS setup, capabilities, timeout and failure behavior stay intact.
Only the opaque call ID changes for the selected request.

Collect fixed stages for dispatcher pre-pass, method total, snapshot acquisition,
metadata batch reads and cumulative metadata-row opening. Existing nested load
and lock stages may contribute too. Report count and duration per stage, explicitly
identifying parent timings as inclusive rather than adding nested totals.
Authentication and HTTP response serialization are outside this first diagnostic;
any unexplained client interval remains unassigned, not called crypto or storage.

A client timeout need not have stopped the server future. The diagnostic must
therefore distinguish started from completed spans and mark in-flight/partial
measurements instead of printing a zero elapsed total as proof that no work ran.
Preserve the old observation API and its existing consumers. Emit a bounded
aggregate on normal return or unwinding/cancellation, with fixed labels, counters
and elapsed durations only. Never print account IDs/names, call IDs, raw requests,
responses, keys, headers, message content or arbitrary error context. Do not add
per-message output, extra requests, waits, retries or detached reporter tasks.

All server instrumentation and APIs are guarded by `test_mode`. A shipping
`--no-default-features --features rocks` check must still compile. No stored
layout or production event enum changes are permitted.

## Verification and interpretation

Check claim isolation, started/completed accounting for pending and dropped spans,
normal completion, and unchanged legacy observation behavior. The canonical
successful first Get must claim its observation and record snapshot/batch/open
work with all 400 responses and the original assertions. Use deterministic
controls; do not assert readiness based on Tokio cooperative-budget assumptions.

Root serializes builds and suites after PR85's timed filesystem run. Review the
patch independently, run relevant observation controls and a complete canonical
local suite, and dispatch CI on the isolated diagnostic branch. A local pass does
not establish the hosted cause. Interpret hosted partial/full counters before
choosing any fix; preserve all raw evidence. PR84's branch and PR85's setup patch
remain separate while this diagnostic is evaluated.
