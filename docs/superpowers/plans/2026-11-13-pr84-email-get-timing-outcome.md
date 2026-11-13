# PR84 keyed Email/get diagnostic outcome

This isolated diagnostic is based on PR84 head `9b1df344b`. It does not change
PR84's branch or PR85's corpus optimization. Source review, formatting, the affected integration-test build and both focused
observer controls passed. The shipping check is running; the canonical local suite
and hosted measurements remain pending.

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
exercise unwind cleanup and are not failed checks.

The shipping `cargo check -p stalwart --no-default-features --features rocks`
is still running. The complete canonical ZA suite remains pending. Builds and
suites serialize after PR85's timed validation. The isolated diagnostic branch
is published to start CI while remaining local validation proceeds; publication
is not a claim of complete qualification or a timeout fix.

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
