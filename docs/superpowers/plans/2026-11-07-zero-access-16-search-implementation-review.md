# Plan 16 implementation review

The consolidated correction in `53062c367ac0b85596017fed6ebb50edd8a66567`
passes both independent scoped reviews. Requirements and correctness verdicts
are PASS, all three original findings are addressed, and neither reviewer found
new breakage. Task 19 qualification remains incomplete.

## Review scope

Two independent Astra reviewers examined the whole implementation from
`3649daf00bc5d63f662938eace4dd19d82ac88f2` through
`f4eb2ecccb073fea34fefa116743644732f9dedc`. Their lenses were state, ordering,
concurrency and persistence; and privacy, resource ownership and coverage.
Both found the same three defects. One consolidated Sol implementation fixed
them, followed by one independent scoped re-review from each reviewer.

The scoped reviews covered the exact eight-file correction whose binary Git
diff has SHA-256
`34a3594aabb133a096a2631858ba081f22ce1b16449c98eba14b3912e1227d66`.
The primary verified that the committed correction matches those reviewed bytes.
Reviewers inspected source and reported evidence without rerunning tests.

## Findings and disposition

| Finding | Original severity | Disposition |
| --- | --- | --- |
| WB1: rapid absent reads could permanently consume an insert before its retry window expired | P1 in the state review; P2 in the privacy review | Addressed. In-window absence preserves the matching attempt count and aborts without advancing input or Content coverage. Only a current non-absent transient can exhaust the persistent-fault count. |
| WB2: optional Content refresh could block or refuse an otherwise readable Map | P2 | Addressed. Required Thread and optional Content loads have separate per-account flights. Deferred Content remains owned and charged, while detached preparation and exact-generation publication preserve currentness. |
| WB3: JMAP unit tests used the obsolete visibility API | P2 | Addressed. Tests supply the required visibility callback and retain their original assertions. |

The H6 absence-window rule governs the former plan C1/C8 conflict. Existing
persisted counts have no historical fault class and remain usable by a later
non-absent failure. No stored layout, production default or ordinary-account
behavior was changed.

The Content error path checks its target and restores an untouched suffix in
one writer section after off-lock charge adjustment. This closes the former
second-wait gap by source reasoning. The new moved-head test covers stale-fetch
retry; it does not reproduce that narrower old interval.

## Verification of the committed source bytes

| Check | Result |
| --- | --- |
| Common search units, test mode | 59 passed |
| Email pass units, test mode and required common feature | 17 passed |
| JMAP ZA units, test mode | 15 passed |
| Exact RocksDB `za::za_tests` suite | Passed: 1 suite, 1628.27 seconds test runtime; 1794 seconds including compilation |
| Workspace formatting | Passed |
| Shipping `cargo check -p stalwart --no-default-features --features rocks` | Passed in 232.49 seconds, with no warning or error lines |

The full suite passed both new served regressions, the existing recovery and
compaction controls, and 183 paired comparisons across 3,258 messages with the
explicit K deviation. The integration linker emitted its pre-existing Apple
compact-unwind warning; the shipping compile check was warning-free.

Initial behavioral/compiler failures, the intentional SIGINT interruption,
and a later SMTP 452 setup failure are retained separately. The 452 occurred
after WB1 passed and before WB2 assertions. Its log does not identify whether
the root or reply delivery reached the shared limit. The final run gives the
two new cases a dedicated synthetic account, preserves all deliveries and
assertions, and leaves production and suite-wide rates unchanged.

## Remaining qualification

The Task 19 full verification matrix, fresh mutation execution, final-head
benchmarks and tracked outcome integration are still pending. Nineteen checks
stopped by automatic screening remain unqualified. A separate independent
verification request for the updated offline drafts was stopped before dispatch
without a reason; it is not an observed test failure or a mutation result.

The completed full benchmark belongs to `f4eb2ecc`, before this correction.
Its full-size cold and post-rebuild Content serving remain unqualified.
Native clients, networked-store timing and physical multi-node behavior also
remain outside this approval. No merge, push, deployment or release is implied.

The complete original and scoped reports, raw logs and source snapshots are
retained locally under `.superpowers/sdd/2026-11-07-zero-access-16-search/`.
The scoped reports are `whole-branch-fix-scoped-state-review.md` and
`whole-branch-fix-scoped-privacy-review.md`; execution evidence is in
`whole-branch-fix-report.md` and `whole-branch-fix-primary-shipping.json`.
