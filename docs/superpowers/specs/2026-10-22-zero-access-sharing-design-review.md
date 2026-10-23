# Zero-access sharing design review

Re-reviewed 2026-10-23 in `release3-mail-spec`, on branch `release4-sharing-spec-r1`.

- Spec: revision 10, draft for review.
- Source HEAD: `ca2ed1ff6d2432dd96912213317d19ae7057d74d`.
- Spec SHA-256: `447eb90527963d931417448b48a59d62cf26f508a552c1c739325c8f37bc47c2`.
- Result: two P2 findings in the new second-run mechanism. The revision 9 recurrence
  finding is explicitly accepted under the revised contract, not fixed.
- Previous review: revision 9 at `50dc7ad8cbc09dc719ebe517ffd84d5d4a329c57`,
  recorded in this file but not committed. This report supersedes it.
- Scope: bounded owner/delegated lifecycle comparison, the routing exception, and case 25's
  oracle. Three independent reviewers covered builder parity, routing through recipient
  processing, and the test contract; the primary independently checked the source evidence.
  Reviewers deliberately inherited the parent model and effort because the exact originating
  model identity and capability ceiling were not exposed.
- Validation boundary: source inspection only. No builds, tests, server runs, client probes,
  or implementation edits. Proposed regressions have not been executed.

## Disposition of the previous review

| Item | Revision 10 disposition |
|---|---|
| Revision 9 P2: a later full update reactivates a cancelled override | Explicitly inherited and accepted in [table row 1](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1994). It is no longer a defect against the revised parity contract; it has not been fixed. |
| Override-only attendee cannot consume a master-only deletion CANCEL | Explicitly inherited and accepted in [table row 3](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1996). The prior payload clarification is withdrawn; adding a component solely for the writer would violate the new contract. |
| Revision 8/9 account-level alias and hand-derived CANCEL rules | Withdrawn by [the new contract](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1741). The alias outcome is accepted in [table row 2](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1995) and compared differentially rather than required to preserve the series. |

These accepted behaviors are not reopened below. Both findings concern the fork's proposed
mechanism or its deliberate exception.

## 1. [P2] Carry the CLIENT/NONE exception through recipient processing

Location: [snapshot-only routing exception](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1879) and
[self-item merge promise](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1914).

Setting `is_server_scheduling = true` in the builder's snapshots makes a self-CANCEL
addressable, but does not make its payload acceptable to the writer's drain. Unlike REQUEST
export, the CANCEL builder preserves the original ATTENDEE's `SCHEDULE-AGENT` parameter.

A concrete lifecycle:

1. Bob creates an event through Alice's shared calendar, with Alice as ORGANIZER and Bob
   as the sole attendee on the **master**, using `SCHEDULE-AGENT=CLIENT` or `NONE`.
2. The second run sends Bob the REQUEST. REQUEST export strips `SCHEDULE-AGENT`, so his
   drain imports the active personal copy
   ([REQUEST export](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/itip.rs:252)).
3. Bob deletes that shared event. The overridden snapshot selects Bob as a recipient, but
   the master CANCEL clones his original ATTENDEE line, including `CLIENT`/`NONE`
   ([CANCEL construction](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/event_cancel.rs:60);
   [verbatim property copying](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/event_cancel.rs:147)).
4. The drain parses the incoming message with `itip_snapshot(..., false)` before its
   delegated sender check ([incoming snapshot](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/calendar/drain.rs:584)).
   Alice's ORGANIZER is external to Bob; Bob's ATTENDEE is local but not server scheduled,
   so it does not satisfy `has_local_emails`
   ([local attendee condition](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/snapshot.rs:217)).
   The snapshot returns `NotOrganizerNorAttendee`
   ([snapshot result](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/snapshot.rs:353)).
   The CANCEL is rejected and Bob's personal event remains active.

This is not inherited table row 3: Bob's master ATTENDEE line is present. It is a failure to
deliver the behavior promised by the deliberate filter exception. An ordinary owner write
would suppress this recipient, while this design intentionally creates and maintains his copy.

Specify how that exception remains effective through incoming and stored-event snapshotting,
without relaxing ordinary-message checks. Merely changing the first parse to `true` is
insufficient: that option also bypasses the ORGANIZER's scheduling-agent check
([organizer check](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/snapshot.rs:107)), and later merges parse both
stored and incoming trees with `false`
([merge snapshots](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/calendar/drain.rs:1057)). A successful CANCEL merge would
itself store the copied ATTENDEE parameters
([CANCEL merge](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/inbound.rs:209)).
Keep any exception scoped to the authorized self-delivery path and preserve the specified
sender, organizer and token validation.

Extend [case 25's exception run](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:2745) through create, drain, DELETE, drain
for each of CLIENT and NONE, asserting the personal copy is cancelled. Include a subsequent
update/reinvitation where applicable so accepting the cancellation cannot leave the stored
copy unparseable.

## 2. [P2] Preserve finalize cleanup when only the second run builds

Location: [first-run NothingToSend behavior](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1800) and
[reconciliation substituted for finalization](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1831).

The combined SEQUENCE reconciliation repairs the missing increments, but it does not perform
`itip_finalize`'s other operation: stripping `SCHEDULE-FORCE-SEND` from the committed
ORGANIZER and ATTENDEE lines. The spec explicitly leaves that tree unstripped when the first
run returns `NothingToSend`.

A concrete comparison outside the routing exception:

1. Alice is ORGANIZER; Bob is the sole attendee, with ordinary NEEDS-ACTION participation
   and `SCHEDULE-FORCE-SEND=REQUEST` on his ATTENDEE line.
2. In run A, Alice creates the event. Upstream builds Bob's REQUEST and finalizes the
   committed tree, removing that parameter
   ([create finalization](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/event_create.rs:23);
   [parameter cleanup](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/itip.rs:354)).
3. In run B, Bob submits the same create through his grant. Bob is local under the merged
   list, so the first run has no recipients and returns `NothingToSend`
   ([recipient selection](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/organizer.rs:366);
   [empty result](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/organizer.rs:389)).
   The second run builds his REQUEST, but neither run cleans the committed tree.
4. After the initial drain, the corresponding owner/delegated DELETE copies the stored
   attendee parameters into its CANCEL. Run B includes `SCHEDULE-FORCE-SEND`; run A
   does not. The CANCEL merge copies that difference into Bob's personal event
   ([CANCEL payload](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/event_cancel.rs:147);
   [copied merge entries](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/inbound.rs:209)).

The initial REQUESTs hide the difference because export strips the parameter.
[SEQUENCE reconciliation](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/calendar/sequence.rs:161) changes sequence values
only, so it cannot prevent the later payload and personal-copy divergence. NEEDS-ACTION
was already deliverable in run A; none of the deliberate filter exceptions is needed here.

Require owner-equivalent cleanup when only the second create/update run succeeds, while
retaining exactly one sequence increment. Preserve upstream's distinction between normal
successful update finalization and the cancellation fallback, which does not finalize the
new tree ([update branches](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/event_update.rs:30)).

Add this sole-attendee create-to-delete comparison to
[the existing sole-attendee case](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:2605) / case 25, retaining all ATTENDEE
parameters in the comparison. Case 25 already correctly requires those parameters to be
compared ([comparison fields](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:2726)); its main fixture's additional Carol
recipient can make the first run succeed and therefore conceal this particular path.

## Bounded review conclusions

The move to upstream parity supplies a usable stopping criterion. The second run's
pre-finalize placement, combined SEQUENCE reconciliation, either-run schedule tag, and
DELETE-loop placement are source-consistent apart from the cleanup omission above.
No additional actionable finding was established in the inspected create, SUMMARY, DTSTART,
override addition/removal, reinvitation, alias and deletion paths.

Case 25 has an implementable comparison oracle: it retains SEQUENCE, component identity,
attendee parameters and token-address presence while normalizing the stated incidental
differences. No independent defect was established in its harness or normalization contract.
Keeping its number after the concurrency cases is harmless. Recording the attendee-side
mutation as equivalent when unobservable is appropriate; it should not count as a killed
mutation without an observable assertion.

The review does not require a new self-message architecture or fixes to the inherited table.
The remaining work is the two concrete gaps above, followed by verification of their scoped
changes against the same contract. Implementation and runtime validation remain outstanding.
