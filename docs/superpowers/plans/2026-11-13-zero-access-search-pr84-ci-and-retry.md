# PR 84 CI timeout diagnosis and retry review

Starting head: `eaff7efcd75bb747fb3d10cd3c151186763eebb2`.
Checkout: `/Users/jay/Development/stalwart/.worktrees/release7-search`.

The CI suite has six completed failures across three heads, all at the five-second
JMAP test client after differential bulk indexing. The exact request is unproven.
The enclosing job budget is a separate limit. Three new inline comments also
question caller-deadline classification, backend-error observability, and cooldown
settlement after an abort write changes other manifest fields.

## Constraints and ownership

- Preserve five-second generic JMAP and 45-second currentness clients, corpus,
  request sizes, assertions, suite order and retry rules during diagnosis.
- Diagnostics use fixed labels, account mode, page/count numbers and timings.
  Never print request bodies, message content, credentials or cryptographic keys.
- Keep the historical qualification baseline intact. Backtrace configuration must
  remain explicitly checked without weakening other environment checks.
- Kernel changes follow independently adjudicated findings and the updated spec.
  A caller timeout alone must not be confused with a backend failure already
  observed; atomic-abort and cancellation settlement remain required.
- Primary owns docs, Git, integration and serialized builds/server tests. Writers
  own disjoint files; no nested agents. Reviewers remain independent.
- Agents inherit the supported parent route because its capability ceiling is
  unresolved. No named Astra review is claimed.
- Existing user authorization covers fixes, verification, commit/push, affected
  CI and inline replies. Keep PR 84 ready. No merge or deployment.

## Work

1. Verify CI history and trace post-index requests. Preserve logs and distinguish
   completed failures from enclosing-job cancellations.
2. Add backtraces and scoped differential phase/request timings. Verify locally
   and use the next CI result to identify the failing call; do not declare a
   performance cause from source inspection alone.
3. Independently triage comments 4565479001, 4565479007 and 4565479018 and the
   fragile timing assertions. Record accepted corrections before implementation.
4. Implement confirmed retry corrections with direct regression coverage and
   update the spec where the contract needs clarification.
5. Independently review the candidate, run relevant checks, publish the fixes and
   diagnostic evidence, and inspect current-head CI. Keep any unproved CI cause
   explicitly open until the diagnostic result supports a correction.

Evidence and per-agent reports:
`.superpowers/sdd/2026-11-13-pr84-ci-timeout/`.
