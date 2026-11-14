# PR 84 retry follow-up — outcome

Date: 2026-11-12. Status: independently reviewed and locally verified; pre-publication checkpoint.
Starting head: `5df77aff0411c271b30a7865ea5a56ef672bab4e` on `release7-search`.
PR: https://github.com/jaylaney/stalwart/pull/84, ready for review at the user's request.

This follows [the plan](2026-11-12-zero-access-search-pr84-retry-followup.md)
and spec revision 35. The second external review accepted the previous eight
fixes and raised four follow-ups. Source triage found:

| Comment | Disposition and correction contract |
|---|---|
| 4554121977 — unpaced retry/write amplification | Confirmed. Keep atomic abort and add monotonic resident-entry cooldown 1, 2, 4, 8, 16, then at most 30 seconds. Elide fully unchanged abort manifests. Deferred probes do no message open, replay, copy, commit or repeated abort trace. |
| 4554121981 — stuck message cannot be located | Confirmed. Actual document-open abort traces need document ID and diagnostic attempt count, plus fixed reason. No archive/error payload is logged. |
| 4554121987 — permanent archive errors treated as transient | Confirmed narrowly. Checked access to malformed unversioned outer metadata can return DeserializeError with the fixed Details `Archive access failed`. That is parse loss. Generic DeserializeError, unknown/non-Store faults and missing codec keys stay transient. |
| 4554121996 — returned creation IDs can already be stale | Wording/qualification gap under the accepted contract. Each method's created IDs remain paired with its final state; later methods do not rewrite earlier responses or top-level createdIds. Trusted same-request references remain usable, while stale literals/result-reference values retain strict checking. Add a live-document cross-request regression. |

## Decisions and costs

Atomic abort is retained. Publishing its healthy prefix would require a new
checkpoint/cursor and publication contract. The selected cooldown bounds replay,
but the healthy prefix and later chronological work still wait for repair.

Cooldown uses a local probe streak, including in-window absence whose persisted
diagnostic count stays zero. Its bound is per node/resident entry. Restart or
eviction can permit a fresh immediate probe; repair can wait until the next
capped deadline. Diagnostic-count-only remote commits must preserve the streak
and deadline, while meaningful progress/reset/replacement clears them. This
avoids two nodes invalidating each other's cooldown with failure-count writes.
Stored formats, padding and admission reservations remain unchanged.

Per-method created IDs are preserved with their associated state. A later merge
can stale that ID before the enclosing request finishes. The trusted-reference
exception lasts only for that request; clients use normal changes/query flows
to discover a replacement. This is explicit in spec revision 35.

## Lower-priority review notes

- The focused PR84 runner's fixtures also run in canonical ZA. The inventory
  gate checks baseline tests; this newly added convenience entrypoint does not
  create a baseline-selection obligation. No CI selection change is needed.
- Existing stale-keyless coverage in `mail/gates.rs` already combines nested
  OR/NOT identity terms and SORT/THREAD refusals. Add the distinct nested
  identity case with an unavailable Map to the metadata fixture.
- FETCH guard coverage already has empty CHANGEDSINCE and VANISHED cases;
  the existing protocol fixture covers saved `$` FETCH. All filtering and
  truncation precedes projection, and output borrows the unchanged ID vector.
  State that invariant in a comment; no fallback identity behavior is added.
- The periodic eviction fixture's 45-second timeout observes the actual
  30-second scheduler; it is not a mandatory sleep. Retain that evidence.

## Verification and publication

Command receipts, source hashes, the red-source
patch and retained red test binary are recorded in the local git-ignored
`.superpowers/sdd/2026-11-12-zero-access-search-pr84-retry-followup/` directory.

The first classifier attempt failed compilation because the new non-Store error
control used an unsupported constructor. That fixture-only error was corrected;
it is not behavioral negative evidence. The rerun failed at the intended
classification assertion: Transient(Store) versus Loss(Parse), in 2.27 seconds.

The focused integration negative control failed at the intended combined
assertion in 9.02 seconds: one unchanged abort commit, two immediate probes,
zero malformed-row skips, and two aborted passes on that malformed row, versus
the required zero commits, one probe, one skip and subsequent current outcome.
It first verified the actual malformed outer row's checked archive error and
then exercised the production pass. Production edits began only after these
results. The first candidate then passed 66 common and 63 email search unit tests,
and the focused integration suite passed in 34.80 seconds. These results
precede the independent review correction described below; final-source
verification is recorded below.

Independent focused review found one P2: a local abort commit, or an
assertion winner matching its proposed result, could preserve cooldown across
suspension/reconciliation changes that remote validation treats as meaningful.
Four negative controls reproduced the retained 30-second deadline. The fix
requires all non-diagnostic manifest fields to remain equal when rebinding a
local commit or proposed assertion result. Its independent re-review passed,
and final search units passed: 72 common and 63 email. Positive controls cover
first diagnostic creation, changed failing document and same-document count
updates. The final focused integration suite passed in 36.36 seconds. Both independent
focused reviews and a fresh whole-branch source review approved the frozen
candidate. The shipping configuration check, workspace formatting and diff
hygiene passed. The full canonical RocksDB ZA suite passed from that same retained binary in
1671.62 seconds (1677.31 seconds including wrapper setup/teardown), with 183
paired query forms, zero differences beyond the documented K deviation, and
successful final cleanup. The paired corpus held 3,258 messages per account. The initial
green unit build also exposed an unsupported `u8` trace scalar; conversion to
`u32` fixed compilation before the passing run.

The earlier-head CI run 43270913937 on `5df77aff0` has passed Automation,
Upstream Suites, Unit Tests, the main Test job, shipping product build,
Independent CalDAV Client and Fresh OSS Store Restart. ZA and retention gates
were still running at this checkpoint. Those results do not qualify later
follow-up edits. No new cold-start performance or mutation campaign is claimed.


## Final verification record

The primary ran these checks with stable Rust, serializing builds and server
suites in the existing isolated Cargo target, `STORE=RocksDb` and `RUST_MIN_STACK=16777216` where applicable:

- `cargo test --locked --offline -p common -p email --lib --features test_mode,enterprise,common/test_mode za::search:: -- --nocapture`: 72 common and 63 email tests passed.
- `cargo test --locked --offline -p tests za::mail::threads::pr84::pr84_regression_tests -- --exact --nocapture`: passed in 36.36 seconds.
- The retained binary with `za::za_tests --exact --nocapture`: passed in 1671.62 seconds.
- `CARGO_BUILD_JOBS=2 cargo check --locked --offline -p stalwart --no-default-features --features rocks`: passed, without enterprise or test_mode.
- `cargo fmt --all --check` and `git diff --check`: passed.

Retained test binary SHA-256:
`5a82eff37099205ba9f1062f15e78a3c003a3e1233cfbfa8e6d54581fe32ab66`.
The binary receipt records the final changed Rust-source hashes. Those hashes
match the candidate; the shipping and unit-check inputs also match their
relevant final production sources. The development test link emitted its
existing large-unwind-table warning; the executable and suites completed.

Task 2's final tests cover both Email/set create and Email/import, with live
messages across requests and a positive current-ID control, plus nested IMAP
identity filtering against an unavailable Map. Existing same-request result
reference, keyless, saved FETCH, CHANGEDSINCE and VANISHED cases remain in place.

Independent focused reviews and the fresh whole-branch review against base
`92a41596a571e164d3e88b3fa5f0c9eb6d5e0660` approved the final frozen candidate.
The reviewers performed source inspection; execution evidence came from the
primary. Agents deliberately inherited the parent model/effort because the
originating capability ceiling was unresolved. No additional Astra review is
claimed. Their reports are retained beside the command receipts in the ignored
workspace. No unresolved source-review finding remains in this fix wave.

This note is committed at the pre-publication checkpoint. The PR records the
resulting commit, inline replies and CI run for the pushed head. Earlier-head CI
results and historical cold-start/mutation evidence do not qualify this head.
Native-client, physical multi-node and broader release qualification remain
outside these checks; Task 19 is not declared complete.
