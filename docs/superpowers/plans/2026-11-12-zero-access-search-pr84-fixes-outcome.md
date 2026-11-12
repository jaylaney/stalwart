# PR 84 search review fixes — outcome

Date: 2026-11-12. Status: local verification and source reviews passed; new-head CI pending. PR remains draft.

This follows [the fix plan](2026-11-12-zero-access-search-pr84-fixes.md) on
`release7-search`, starting at `239d2ec23141c47b03aee9572d712c46b4bb2ea5`.
The user authorized the eight review fixes, Automation fixture correction,
spec amendments, commit/push and affected CI reruns. This record does not
declare Task 19 or release qualification complete.

## Changes

| Review item | Result |
|---|---|
| Transient failures consume a document after eight attempts | Every transient leaves the item pending, including after diagnostic-counter saturation. Recovery indexes it without reindexing. The stored counter stays compatible; `ZA_SEARCH_SKIP_AFTER` is retired and ignored. Persistent faults can hold Content freshness until repaired. |
| Idle entries require another request to expire | The existing 30-second scheduler event sweeps search entries. Entry acquisition no longer scans the resident table; active reader/pass/load safeguards remain. |
| FETCH holds the index guard during client output | FETCH retains a projection for the selected document IDs and their validated cache, then releases the view before any output, including VANISHED and empty responses. |
| Metadata SEARCH/SORT requires an available map | Metadata-only commands bypass the map; identity criteria, grouping, content and keyless refusals retain their requirements. SAVE uses its captured previous result for composite `$` sequences even without a Map, avoiding a wait on its own result channel for both keyed and ordinary accounts. |
| Same-request creation references become stale | Successful keyed Email creation/import references carry account/document provenance per original occurrence. Consumers map them inside their own validated view. Generated on-success targets carry the same identity. Stale literals, incoming `createdIds`, result references and other accounts receive no exception. |
| Each pass copies the thread part twice | The planner mutates one detached copy. Update-only passes avoid copying the thread part while retaining cursor, truncation and checkpoint semantics. |
| Ordinary-account destroy scan is undocumented | CLAUDE.md now records the accepted full blob-link scan, including its cost. Cleanup behavior is preserved. |
| Shipping code includes unused loader helpers | Helpers are gated for their actual unit or cross-crate test consumers. The email test dependency enables the required test helper without enabling it in the product dependency path. |
| Automation rejects a retrying task after its alert threshold | The DKIM assertion accepts both `Retry` and `Stalled`, retaining the domain, minimum-attempt and failure-reason checks. |

[Spec revision 34](../specs/2026-11-04-zero-access-search-design.md) records the
changed retry contract, trusted references, output lifetime and periodic sweep.
Stored layouts are unchanged. Existing bounded JMAP blob-store waits still hold
their views; the FETCH correction removes the unbounded client-output hold.

## Evidence and review

Local command receipts and logs live in
[`../../../.superpowers/sdd/2026-11-12-zero-access-search-pr84-fixes/`](../../../.superpowers/sdd/2026-11-12-zero-access-search-pr84-fixes/).
They are git-ignored and local to this checkout. Receipts record command,
environment, starting HEAD, changed-source hashes, status and log hash.

- Negative controls reproduced transient consumption at the old threshold,
  global eviction from entry lookup, metadata-only map refusal, FETCH blocking
  publication during paused output, and a same-request Email/get returning
  `notFound` after its creation ID changed mapping.
- Common/email search unit suites passed: 62 and 61 tests respectively.
- JMAP protocol unit suite passed: 21 tests, including new provenance cases.
- The expanded focused PR 84 suite passed in 33.85 seconds, including the
  existing protocol module, both account modes' composite SAVE cases, the
  trusted-reference races, blocked FETCH output and actual periodic eviction.
- The complete canonical RocksDB ZA suite passed in 1749.36 seconds, with
  183 paired query forms, zero differences beyond the documented K deviation,
  and successful final cleanup. It ran from the retained focused-test artifact
  with SHA-256 `f1b95cf91bfe6f62bfad1092584a2d43bd5e513928dd8891f14d04f89a619b53`.
- The whole ordinary-account JMAP and IMAP suites passed in 134.08 and
  9.47 seconds respectively, using the same retained artifact and running
  sequentially. The final source hashes match the compiled artifact.
- The shipping configuration check passed after the SAVE follow-up:
  `cargo check --locked --offline -p stalwart --no-default-features --features rocks`.
  Workspace formatting and `git diff --check` passed.
- Cold-harness Python units passed: 14 tests. CI step-table comparison found
  zero differences; gate-tool tests passed 41, with two existing skips.
- Independent source reviews approved the pass/retry, IMAP and cache/Automation
  scopes. The cache review found a test-only import still visible in shipping
  builds; that import was gated and re-reviewed. Focused JMAP review approved.
  The whole-branch review identified overbroad spec wording around
  `EmailSubmission/query`'s existing document-only `emailIds` filter. The spec
  now explicitly preserves that metadata behavior, including queries after the
  source Email is gone; no new prefix check was added. The fresh whole-branch
  source review approved the production candidate. The focused reviewer also
  approved the corrected race fixture after its queue-drain and timeout issues
  were resolved.

Initial fixture compilation failures are preserved separately: the IMAP test
needed a mutable server reference, and the new JMAP fixtures needed the
existing response constructor and typed ID constants. One integration startup
attempt was denied localhost listener access by the sandbox before assertions;
the permission-enabled run used the retained compiled binary and reproduced
the intended failure. A later integration attempt exposed a fixture waiting
for an intentionally held SMTP queue. It now observes the bridge's real LMTP
insertion directly, and its parked HTTP request has a finite timeout longer
than the bounded paused phase. These attempts are not counted as behavioral
negative controls or passing tests.

The first complete ZA attempt reached the existing composite SAVE regression
and timed out at `tests/src/za/mail/threads/protocol.rs:98` after 698.78 seconds.
Metadata routing exposed an implicit dependency on a Map: a composite `$`
sequence ignored the previous saved result and waited for its own replacement
channel. The correction uses the captured result independently of Map presence.
The existing regression is unchanged, and additional wire cases cover both
account modes and missing-prior refusal. This failure is retained separately
from the subsequent rerun.

Agents implemented disjoint scopes; the primary coordinated Git and serialized
Cargo/server execution. Model routing deliberately inherited the parent route
because its capability ceiling could not be resolved. No stronger route is
claimed. Reviewers were independent of their implementers.

## CI

The original Test run `43114499660` at `239d2ec23` failed Automation because its
DKIM fixture recognized `Retry` only, while the task had become `Stalled` with
attempt 17. The fixture deliberately directs DNS to `127.0.0.1:1`; the later
PowerDNS suite had not started. This was not evidence of a PowerDNS outage.

The original ZA job was cancelled after about 94 minutes with its suite still
running. Its check annotation said `repository invocation blocked`; the job's
configured timeout was 120 minutes. It supplied no ZA suite result and does not
establish a test timeout or a passing result.

Local full-suite verification, shipping configuration and source reviews
passed. This is the pre-publication checkpoint committed with the fixes; the PR
records the resulting commit and new-head CI run. Push must trigger fresh CI
for the final head rather than retrying the superseded `239d2ec23` jobs.
Docker is unavailable locally; full Automation validation belongs to CI.

The previous optimized build, cold-start measurements and mutation campaigns
remain historical evidence at their recorded heads. They were not repeated
for this fix wave, and these results do not close the remaining native-client,
physical multi-node or broader release-qualification gaps.
