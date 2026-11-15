# Approved D29 amendment: preserve the original sync start

Date: 2026-11-14. Status: approved by Jay; approval recorded 2026-11-15.
This amendment supersedes D29's start-inference rule in
[the release 6 spec, section 6.8](2026-11-13-zero-access-jmap-calendars-design.md#68-changes-and-querychanges)
and governs plan 17 Task 22. Incorporated into spec revision 7 and the
implementation plan on 2026-11-15. Jay also authorized implementation;
Task 22 follows the existing task order and review gates. The rest of
the approved plan remains in force.

## Why the existing candidate cannot work

Source review at `899671c8c` found a reachable collision. Let a completed
sharing boundary be `b`, and let `b < m < f < g < c` be actual Calendar
change IDs. A client has a valid exact state `m`. Normal retention leaves
an empty marker at `m` and event records at `f` and `g`.

| First request, with `maxChanges: 1` | Why it passes | Continuation |
|---|---|---|
| Initial | Initial sync is exempt from the boundary and truncation checks | `{ from: f, to: g, sent: 1 }` |
| Exact `m` | `m > b`; the exclusive log query skips the marker without flagging truncation | `{ from: f, to: g, sent: 1 }` |

A completed rename then advances the boundary to `c`. D15 requires the
initial continuation to remain usable and the incremental continuation to
answer `cannotCalculateChanges`. Their tokens and current server state
are identical. Reading the first retained record cannot recover the lost
origin. Choosing the physical marker or the first data record merely
changes which request gets the wrong answer.

Evidence: marker creation in `crates/common/src/storage/transaction.rs:228`;
marker exclusion in `crates/store/src/query/log.rs:160`; range construction
in `crates/jmap/src/changes/page.rs:270`; validity checks in
`crates/jmap/src/changes/get.rs:433`. `za_log_first_retained` at
`crates/store/src/query/log.rs:285` returns a truncation marker only, not
the first data record. An independent Astra xhigh review established this
from source; no runtime reproduction has yet been run.

## Approaches

**Approved: carry the original start explicitly.** Add a versioned,
opaque continuation form for key-account calendar changes. Preserve D15's
initial-sync exemption and compare incremental pages against the exact
state from which the sync began. This requires a narrow change to state
encoding/decoding and calendar continuation handling.

**Alternative: allow initial paged syncs to restart.** Refuse ambiguous
continuations after a boundary change and have clients start again. This
reduces the state-format change but weakens the approved initial-sync
guarantee. It is not the approved approach.

Inferring the origin from current log contents is rejected: the collision
above proves that even a complete reread cannot distinguish the histories.

## Approved contract

1. A key-account `Calendar/changes` or `CalendarEvent/changes` response
   that needs another page carries both the existing range/count and the
   original start: `Initial` or `Exact(s)`. Preserve that original start
   through every continuation until the sync ends. Never reconstruct it
   from `from_id`, retained records, or the current boundary. The legacy
   owner continuation in point 5 is the sole emission exception.
2. For a grantee, an initial-origin continuation stays exempt from the
   sharing-boundary check. An exact-origin continuation calls the shared
   `ZaSyncBoundary::refuses(s)` predicate. Pending boundaries retain D15's
   existing behavior. Owner and ordinary-account exemptions remain.
3. Upstream's log reader, retention, object filtering, pagination and
   ordinary state strings remain unchanged. Origin metadata selects the
   boundary check; it changes no access right. All existing account,
   object and calendar authorization still runs.
4. `/queryChanges` uses the same checked changes path. It must preserve
   origin whenever it produces or consumes a continuation.
5. Legacy intermediate states without an origin cannot establish a
   grantee's key-calendar start: return `cannotCalculateChanges` so the
   client restarts from Initial. Owner and ordinary-account handling of
   their existing states stays unchanged: a key owner's legacy `r` chain
   continues to emit legacy `r` states until completion, without inventing
   an origin. Its next Initial/Exact start uses the new form when paged.
   No released key-calendar JMAP
   continuation is being migrated: these methods are not served before
   this release.

## Approved representation and scope

Use a new `State` variant for key-calendar intermediate
states, containing the existing `JMAPIntermediateState` and an origin
enum (`Initial` or `Exact(u64)`). This is transient protocol state, not a
stored archive. No stored struct layout, database field, secret, setting
or client code is added.

Use prefix `z` and the existing STALWART base32/LEB128 conventions. Encode,
in order: version `1`, origin tag (`0` Initial, `1` Exact), the original
nonzero exact ID when the tag is `1`, then `from_id`, `to_id - from_id`
and positive `items_sent`. Reject unknown versions/tags, missing or
trailing fields, zero exact IDs, zero counts and arithmetic overflow.
Budget the encoding buffer for the additional fields; ordinary `n`, `s`
and `r` encodings remain byte-for-byte unchanged.

Only key-calendar changes emit the new form. The calendar handler unwraps
its range for upstream pagination and reattaches its unchanged origin to
each new intermediate response. Other object/account routes must not
interpret this form as authorization or silently accept it as a calendar
continuation. In a normal `State` request slot such as `sinceState` or
`sinceQueryState`, an out-of-scope `z` state gives `invalidArguments`. In
an `ExpectedState` precondition slot (`ifInState`, `ifFromInState` or
`destroyFromIfInState`), it gives `stateMismatch`, matching the existing
invalid-string behavior. Preserve the established validation and
authorization order; add no global account lookup or parser permission
path. The implementation plan must enumerate and update all exhaustive
`State` consumers and the context checks that reject this form.

The standard browser client continues to echo an opaque JMAP state string.
The existing reference-client qualification gate remains pending; this
design is not evidence of browser compatibility.

## Incorporated plan changes and required acceptance evidence

D29's inference is replaced in spec revision 7 and Task 22. I sections
7.2 and 7.6, the merge-hotspot/cost notes, dependent Tasks 23, 26 and 37,
and documentation/final-review Tasks 43 and 44 now carry this contract.
Sol implementers and Astra reviewers remain; Astra xhigh is required for
this state/ordering change and for the final whole-branch review.

- Add codec round trips and malformed-input cases for both origins, plus
  unchanged ordinary-state fixtures.
- Reproduce the retention-marker collision through the actual changes
  handler. The two first pages now produce distinct tokens. After the
  rename, the initial continuation is served and the exact continuation
  answers `cannotCalculateChanges`.
- Cover pending boundaries, multiple pages, log trimming between pages,
  and insert/delete folding that changes the returned range's first ID.
- Cover owner and ordinary-account behavior, the legacy grantee-state
  restart, the legacy owner `r` chain, the out-of-scope state-slot errors,
  and the `/queryChanges` path.
- Retain the plan's real suites and independent review. Do not replace
  this integration evidence with a unit test of a guessed start ID.

Task 22's design-approval and spec/plan-update prerequisites are satisfied.
Implementation follows Task 21 and still requires the acceptance evidence
and independent reviews above. This document records design approval
and plan integration, not implementation completion or runtime qualification.
