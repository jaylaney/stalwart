# Zero-access plan 17: JMAP calendars and browser sign-in (release 6)

This file is the index of plan 17. The plan was executed in full and merged as pull request #90
on 2026-11-21 (merge commit `04145d586`). What was done, the decisions, the rulings that bind
later work and what stays open are in the
[outcome note](2026-11-14-zero-access-plan17-outcome.md).

**Goal:** a key account's calendar works from an unmodified JMAP calendar client in a browser: an
OAuth token issued through the authorization-code flow opens the account's keys on every HTTP
request that presents it, and the JMAP calendar methods read and write through the same
key-account pipeline as CalDAV.

**Architecture:** the fork's key-account calendar steps move from the `dav` crate into
`groupware::calendar::pipeline` with no change of behaviour (group A, proved by the existing
CalDAV tests), gain the paths only JMAP uses (group B), and are called by JMAP key readers and
writers that sit as child modules of upstream's handlers and call upstream's own validation in
upstream's order (group D). Sign-in (group C): every token string of a key account's grant
carries a 32-byte secret after a `~`, a per-account token record (principal field 153) holds the
master key wrapped under keys derived from those secrets, and the HTTP layer's Bearer arm opens
it. Group E closes the release: the differential oracle, upstream's JMAP calendar tests in key
mode, leak coverage, the gates, the mutation sample, the documentation, the final test matrix
and one whole-branch review.

**Spec:** [`docs/superpowers/specs/2026-11-13-zero-access-jmap-calendars-design.md`](../specs/2026-11-13-zero-access-jmap-calendars-design.md),
revision 7 (decisions D1 to D29, invariants 57 to 76). Where the plan and the spec disagreed,
the spec won.

## Tasks

Group A, the move (the fork's key-account calendar steps leave `dav` for `groupware`):

1. The pipeline module and its neutral error
2. Admission, roles, contexts, read helpers; `ZaSyncBoundary::refuses`
3. The UID lease functions
4. The scheduling assembly
5. The create commit
6. Placement contexts, refusals, the event container's reseal
7. The update target and commit
8. The deletes
9. The move's proof run, with the fresh-store script

Group B, the pipeline paths only JMAP uses:

10. A create in several calendars; the `jmap_calendar` test helpers
11. Placement in an update
12. The destroy of a whole event; `za_delete_send_itip`

Group C, tokens:

13. Vault primitives: secrets, the record, the hand-off value
14. Field 153, storage, `za_token_read`, the vault write's deletion
15. Issue, open, forget; the grant hook for key accounts
16. The suffix strip, untraced answers, the Bearer arm
17. The hand-off, the code exchange, the refresh precheck and its interim refusal
18. Refresh
19. The sweep; account destruction

Group D1, the JMAP reads, queries and the admission table:

20. The calendar refusal text and the admission table
21. `CalendarEvent/get`, `Calendar/get`, `Calendar/query`
22. `/changes`, explicit sync origin and the sharing boundary
23. `ParticipantIdentity`
24. `CalendarEvent/parse`
25. The event text index
26. `CalendarEvent/query` and `/queryChanges`
27. Colour on a shared calendar

Group D2, the JMAP writes, scheduling, sharing and refusals:

28. `Calendar/set`
29. Default alerts by value
30. `CalendarEvent/set` create
31. `CalendarEvent/set` update and scheduling
32. `CalendarEvent/set` placement
33. `CalendarEvent/set` destroy
34. The drain hook
35. The session and principal objects
36. The `jmap_calendar::sharing` module
37. The `jmap_calendar::refusals` module

Group E, closing:

38. The differential oracle
39. Upstream's JMAP calendar tests in key mode
40. Leak coverage
41. The other harnesses and the gates
42. The mutation sample
43. Documentation
44. The final test matrix and the whole-branch review

## Where the full plan is

The task texts, the interface sheet and the execution details (commands, code rules, suite
rules, rebasing, review focus) were removed after the merge. They are in the repository's history
at the merge commit `04145d586`:

- `docs/superpowers/plans/2026-11-14-zero-access-17-jmap-calendars.md` (this file as it was
  executed);
- `docs/superpowers/plans/2026-11-14-zero-access-17-jmap-calendars-interfaces.md` (the interface
  sheet, cited in code comments and older records as "I §n");
- `docs/superpowers/plans/2026-11-14-zero-access-17-jmap-calendars-tasks-a.md` (Tasks 1 to 9),
  `-tasks-b.md` (10 to 12), `-tasks-c.md` (13 to 19), `-tasks-d1.md` (20 to 27), `-tasks-d2.md`
  (28 to 37) and `-tasks-e.md` (38 to 44).

Read one with `git show 04145d586:<path>`. The
[outcome note](2026-11-14-zero-access-plan17-outcome.md) records what was done.
