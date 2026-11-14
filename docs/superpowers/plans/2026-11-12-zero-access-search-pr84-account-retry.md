# PR 84 account-pass retry follow-up

**Goal:** Address comments 4555633206, 4555633217 and 4555633233 at
`b02015ceefb8c789bb1f35490f5e9144264a3232`, plus their attached documentation,
trace-accounting and deterministic-test notes.

**Spec:** ../specs/2026-11-04-zero-access-search-design.md, revision 36,
section 5.3 Retry pacing. That paragraph is the binding contract.

## Global constraints

- Work only in `/Users/jay/Development/stalwart/.worktrees/release7-search`, branch
  `release7-search`. No stored-layout, padding, alias, chronology, reservation,
  credential, ordinary-account or ID-validation changes. No prefix publication.
- No raw errors/content/credentials in new traces. Preserve atomic aborts,
  unknown-error transience, in-window absence, equality elision and content coverage.
- Primary owns docs, Git, serialized Cargo/server runs and external publication.
  No nested delegation. Preserve ignored evidence. No merge/deploy/upstream writes.
- Parent capability ceiling is unresolved; agents deliberately inherit the supported
  parent route. No named stronger-model review is claimed.
- User already authorized fixes, spec updates, commits, push, CI and inline replies.
  PR 84 remains ready for review.

## Rulings

Account-pass ownership replaces the message-open-only policy. Retain one controller
and one primary failure per probe; secondary diagnostic failures do not double count.
Use a shared one-second manifest-observation cadence, not per-waiter polling. This
bounds reads during manifest outages and notices progress without opening messages.
Keep original request deadlines and final current-view validation; unconditional
fail-fast would prevent noticing remote progress. Retain atomic abort and per-node
memory-only scheduling. Unknown observations never invent a generation. Tests use
explicit clocks/parked seams rather than real one- or two-second scheduling windows.

### Task 1: Implement and regress the account-pass boundary

**Owned files:** `crates/common/src/za/search/{cache,load,trace}.rs`, existing common test
hooks; `crates/email/src/message/za/search/{pass,write,ensure,compact}.rs`; test helpers and
`tests/src/za/mail/index.rs` and reuse of the existing pinned-view fixture in
`tests/src/za/mail/search.rs` (focused runner only if necessary). No docs/Git/builds.

- [x] Write failing regression coverage first; pause for primary's red run before
  production changes. At minimum, show account streak survives changing documents
  with current APIs. Add non-open and waiter regressions with the implementation.
- [x] Implement all revision 36 Retry pacing rules. Prefer factoring one admission/
  outcome boundary over per-return exceptions; retain authenticated targets on load
  failures, and admission before expensive cold loads. Every account-pass transient,
  including raw infrastructure errors, is paced; independently required service
  loads stay outside this controller.
- [x] Include synchronous pre-await failure recording and post-commit settlement,
  exact meaningful-basis comparisons, unknown-state handling and actual-winner
  assertion-loss handling. Do not restore a proposed loser's generation.
- [x] Add synthetic-clock unit coverage for alternating docs/classes, None/Some
  rules, meaningful changes, unknown observations, cadence and cap.
- [x] Add deterministic integration coverage for non-open staging/commit and cold
  load failure suppression, remote publication during a parked deferred waiter,
  original deadline, failed diagnostic write/loser reload trace accounting, and
  repair/atomicity. Use existing hooks where possible; scoped one-shot test-only
  seams are allowed. Extend five existing Deferred checks with an explicit long
  test deadline; retain exact delay arithmetic in synthetic-clock unit tests.
- [x] Return a concise report listing tests, scope/coverage limits and actual diff.
  No builds/tests without primary coordination; no commits or nested agents.

### Task 2: Review, verify and publish

- [x] Update developer retry docs, returned-ID consequences, archive literal silent
  dependency and fixture allocation; write an outcome with evidence limits.
- [x] Independent task review for spec and correctness, then fresh neutral whole-
  branch review against main `92a41596a571e164d3e88b3fa5f0c9eb6d5e0660`.
- [x] Primary runs search units, focused PR84 suite, canonical ZA suite, shipping
  check, formatting and diff hygiene sequentially; record initial failures and
  retries separately. No unrelated performance/mutation reruns.
- [x] Preserve CI suite/deadline/retry semantics while raising only the Zero-access
  job budget from 120 to 240 minutes after its observed timeout; keep the existing
  gate-test anchor synchronized and verify gate checks.

**Publication:** Commit/push to `jaylaney/stalwart`, reply inline to new threads with
evidence, and verify jobs on the resulting head. Keep PR ready and report pending
CI honestly. Final commit, reply and CI pointers are recorded on PR 84 after this
verification record is committed.
