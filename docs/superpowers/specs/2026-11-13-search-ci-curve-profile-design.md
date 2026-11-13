# Search CI curve-profile experiment and integration

The owner authorized a diagnostic-backed timeout fix, subagent implementation,
PR85 integration into PR84, and holding PR84 out of main until combined CI is green.
The original diagnostic is still running; its hosted stage log is unavailable.
Locally, the unchanged 400-row Get took 537.977 ms, including 515.672 ms opening
metadata. Dependency source places scalar arithmetic in curve25519-dalek.

## Controlled candidate

Test one variable: set only the test package profile of curve25519-dalek to
opt-level 3. Keep first-party test code at opt-level 0, debug assertions and
overflow checks enabled, dependency versions/features unchanged, and every
production profile unchanged. This follows the existing test-only Argon2 override.
No authentication, key caching, wire/storage layout or cryptographic algorithm changes.

Preserve the same canonical suite, 3,258-message corpus, 400-message first Get,
seven requested properties, five-second client deadline, all 183 comparisons,
G5 and cleanup. Preserve the diagnostic stages and validate all 400 row opens.
Retain the current frozen default-profile binary and its receipts as the control.
A local improvement demonstrates test-build cost only; it does not prove the
hosted timeout cause or shipping performance. Do not promote this candidate as
the timeout fix until hosted diagnostic or controlled hosted comparison supports it.
If hosted evidence identifies another stage, reject/reconsider this candidate.

## Integration and acceptance

Root owns builds, tests, Git and publication; code/config implementation and
independent reviews use separate subagents. Serialize local builds and suites.
Record actual compiler optimization/assertion flags, source and binary identities,
initial failures, subsequent runs and the fixed aggregate timings.

After the fix is supported and reviewed, integrate it and PR85 on release7-search.
Resolve any shared differential-fixture edits while retaining both observation
and setup contracts. Review the complete combined follow-up diff from 9b1df344b,
run relevant local checks, and get CI for the exact combined source head. Both
ZA storage modes, retention, fresh-store and the remaining jobs must pass before
PR84 may merge into main. Unrelated initial CI failures remain in the record.
