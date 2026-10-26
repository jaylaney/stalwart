# Zero-access sharing design review

Re-reviewed 2026-10-23 in `release3-mail-spec`, on branch `release4-sharing-spec-r1`.

- Spec: revision 11, draft for review.
- Source HEAD: `0c47b7f0d136c74535660f4e3ca57e86ef0489db`.
- Spec SHA-256: `0e8ea8313895d0f8a4d34256bc5b3289406d0f441c61e360a394fc2d2dd197e8`.
- Result: no actionable findings in this re-review. Both revision 10 P2 findings are
  addressed in the design.
- Previous review: revision 10, committed as `c06e95bd9a068aff832a124711dfd4d72bf0a3fd`.
  This report supersedes it.
- Scope: the revision 10-to-11 changes, both reported failure lifecycles, the narrowed
  exception, finalization and reconciliation, and the corresponding tests and handoff.
  Only the spec changed between the reviewed commits; Rust source is unchanged.
- Method: two fresh read-only reviewers independently covered the exception/recipient path
  and finalization/regression path. The primary checked their evidence and the combined diff.
  Reviewers deliberately inherited the parent model and effort because the exact originating
  model identity and capability ceiling were not exposed.
- Validation boundary: static source/spec review. No builds, tests, server runs, client
  probes, or implementation edits. The proposed tests and mutation checks were not executed.

## Revision 10 findings

| Finding | Revision 11 disposition |
|---|---|
| [P2] CLIENT/NONE self-CANCELs fail recipient snapshot validation | Addressed by narrowing the contract. Only `force_send` and `rsvp` are overridden; `is_server_scheduling` is preserved ([mechanism](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1783)). CLIENT/NONE is explicitly withdrawn from the exception ([revised exception](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1901)). |
| [P2] Second-run-only success skips finalization cleanup | Addressed. When the first run returns `NothingToSend` and the second succeeds, the committed tree is finalized with the second update's increments or an empty list for create. First-run success is not finalized twice, and cancellation fallback remains unfinalized ([finalization rule](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1807)). |

## Closure evidence

### Scheduling-agent exception

The two-field override bypasses RSVP/PARTSTAT suppression while leaving the scheduling-agent
gate intact ([sender predicates](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/mod.rs:210)).
The receiver's local-attendee check depends on locality and scheduling agent, not RSVP or
PARTSTAT ([snapshot condition](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/snapshot.rs:217);
[drain parsing](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/calendar/drain.rs:584)).
The remaining exception therefore does not require relaxing incoming validation.

Case 25 now runs CLIENT and NONE from creation through deletion and requires no delivery or
personal copy in either run ([new fixture](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:2787)). Restoring the withdrawn override
would create an unwanted REQUEST copy, which the specified mutation check detects
([mutation](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:2950)). The author's removal of the follow-up parseability regression
is appropriate for this fixture: the revised contract never creates that forced copy.

The exception remains confined to snapshot fields. Its revised definition, invariant 38,
planning handoff and merge guidance consistently preserve scheduling-agent behavior.

### Finalization and later CANCEL payloads

Upstream successful create finalizes with no increments
([create](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/event_create.rs:23)); successful normal update uses its
increment vector, while cancellation fallback bypasses finalization
([update and fallback](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/event_update.rs:30)).
The revised mechanism matches these branches and performs both operations of
[itip_finalize](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/itip.rs:354): removing `SCHEDULE-FORCE-SEND`
and applying the listed increments once.

Combined-message reconciliation remains explicitly required
([SEQUENCE rule](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1848)). It still covers the partial-update branch's missing
increment-vector propagation ([partial builder](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/organizer.rs:234));
finalization does not replace that reconciliation.

Case 10 checks cleanup on Alice's stored event and the later CANCEL
([stored-state assertion](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:2647)). Case 25 removes Carol from the sole-attendee
fixture so the first run actually returns `NothingToSend`, then compares every ATTENDEE
parameter through create and DELETE ([differential fixture](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:2792)).
This detects the original failure: REQUEST export hides the parameter, but CANCEL copies it
from the stored event ([CANCEL builder](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/event_cancel.rs:147)),
and the recipient merge copies the attendee parameters into the personal event
([CANCEL merge](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/inbound.rs:209)).
The corresponding mutation retains this observable assertion
([missing-finalize mutation](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:2952)).

## Review outcome

No further spec revision is requested by this pass. Both reported failures are addressed
without reopening the [accepted inherited behaviors](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:2023); those remain
accepted, not fixed.

The [bounded stopping criterion](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:2815) is satisfied for this review: no new
demonstrated divergence was established in the changed paths. Proceed to implementation
planning and the prescribed lifecycle tests under that contract. This is a design-review
conclusion, not evidence that the implementation or runtime gates have passed.
