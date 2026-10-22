# Zero-access sharing design review

Re-reviewed 2026-10-22 in `release3-mail-spec`, on branch `release4-sharing-spec-r1`.

- Spec: revision 6, draft for review.
- Source HEAD: `52ebb3f7588d24a925cfdf664839893516811573`.
- Spec SHA-256: `037059d97224616d057642e6c2b50993b71c0716d432f16f1b7bae53433acf00`.
- Result: one P1 finding and three P2 findings below.
- Previous review: revision 5 at `2400fb2f7abc1483bb3177563e69b6b5f9e509bb`.
  This report supersedes that finding list; each earlier finding's disposition follows.
- Method: static review of the revision 5-to-6 spec changes against current Rust source.
  The spec is the only tracked file changed between those heads. Three independent reviewers
  covered conversion/keys, DAV access/sync, and scheduling; the primary reviewer checked the
  evidence and adjudicated findings. Reviewers continued on their inherited parent route at
  `xhigh`; the exact parent model identity and capability ceiling were not exposed.
- Validation boundary: no builds, tests, server runs, client probes, or implementation edits.
  “Addressed” means addressed in the design, not implemented or verified at runtime.
  Proposed regression cases have not been executed.

## Disposition of the revision 5 findings

| Previous finding | Revision 6 disposition |
|---|---|
| 1. Ordinary writes during incomplete conversion | The original linked-event write is now refused through every path, and unlink-only recovery remains available ([§5.5](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1327)). The new rollback procedure introduces a different key-cache problem; see current finding 1. |
| 2. Assisted discovery bypasses foreign eligibility | Addressed by eligibility checks before assisted-discovery insertion, `propname` output, resolver access, and tombstone emission, including own-home requests and group-only grants ([§5.2](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1152)). |
| 3. Reusable slugs expose private tombstones | Partially addressed: the grant boundary rejects history preceding a new grant and the filter now respects the requested collection. Renaming an existing granted calendar can still expose another calendar's history; see current finding 2. |
| 4. A declined writer receives no self-delivery | The original sole-attendee decline is explicitly covered independently of `NothingToSend` ([§6.2](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1577)). The replacement mechanism has narrower coverage and an address/token mismatch; see current findings 3 and 4. |
| 5. Alternate-alias revoke contradicts its test | Addressed: an unambiguous alternate alias resolves to the grant, while conflicting ledger and directory identities produce 409 ([§4.4](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:881)). |
| 6. REQUEST can add a component's SENT-BY | Addressed: the rule now distinguishes existing components, new components, and CANCEL replacement, with a matching recurrence test ([§6.2](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1676); [case 8](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:2145)). |

The revision 4 fixes for linked MOVE, stable revoke IDs, the account-deletion assertion,
same-level re-key invalidation, and the event-DELETE calendar fence remain in the design.
The issues below concern distinct interactions introduced or left incomplete by revision 6.

## 1. [P1] Bind cached collection keys to the conversion that created them

Location: [§3.5, rollback removes the ring](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:611),
[§3.5, first grant creates generation 1](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:561), and
[§3.6, collection-key cache identity](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:655).

Rollback restores the unshared form and removes the owner ring. A later first grant of the
same calendar generates a different random CK, again numbered generation 1. The session cache
is keyed only by owner account, calendar document ID, and generation, so those two different
keys have the same cache identity. [§4.6](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:990) explicitly leaves the owner's resident
tokens and keys untouched.

A concrete sequence is:

1. While phase 1 is active, an owner DAV session reads the calendar and caches its generation-1 CK.
2. The grant discovers the raced-in link and finishes rollback, removing that ring.
3. The owner unlinks the event and grants again. Reconversion persists a different generation-1 CK.
4. The same resident DAV session creates a new event. Its cache returns the old CK for the
   current generation. The new calendar assertion succeeds, but the event's DEK is wrapped
   under a key absent from the current ring.

Reads with freshly opened keys fail authentication. More seriously, newly created event
contents can become permanently unreadable when the old cached CK expires. Creating an event
does not need to decrypt an existing event first, so an earlier decryption failure does not
prevent this write.

Current HTTP authentication reuses resident `SessionKeys` when the account's authentication
generation matches
([authenticate.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/http/src/auth/authenticate.rs:62)),
and the key cache returns the same keys through a cloned `Arc`
([cache.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/vault/src/cache.rs:88)).
Conversion changes the calendar, not that authentication generation. The create path seals
a fresh event and then inserts it
([update.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/dav/src/calendar/update.rs:602)).
Sealing under a wrong cached key still succeeds; opening under the replacement key fails
([keys.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/vault/src/keys.rs:201)).

Give each conversion a unique key-domain identity included in cache lookup, or bind cached
keys to a stable identity derived from the freshly read ring. Reading only the current
generation number is insufficient. A best-effort owner-cache invalidation alone also leaves
missed broadcasts and in-flight references.

Add a rollback/reconversion case retaining the same owner DAV session across both conversions.
Warm its CK cache before rollback, then verify reads and a new-event PUT after reconversion
using another freshly authenticated session.

## 2. [P2] Reset the sync boundary when a granted calendar takes another calendar's slug

Location: [§5.2, grant-bound sync position](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1235)
and [§5.6, calendar rename preserves the grant](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1439).

The new lower bound closes the reviewed sequence where a new calendar is granted after old
private history was written. It does not close reuse by renaming a calendar that already has
an older grant:

1. Alice grants Q at `/other/`, recording boundary G. Bob later receives sync position S,
   with G < S.
2. Alice deletes private P's `/family/medical-visit.ics` at D, with S < D, then removes P
   or renames it away.
3. Alice renames the already granted Q to `/family/`.
4. Bob syncs Q from S. S is newer than G, so the new check accepts it; the current-prefix
   filter admits P's private tombstone.

The spec says rename keeps the document ID and needs no further change. Current
[rename_container](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/dav/src/calendar/copy_move.rs:1099)
changes only the calendar name, retaining its preferences, ring, and grant boundary.
[DavQuery::changes](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/dav/src/common/mod.rs:253)
accepts S without binding it to a path or calendar, and
[the vanished fetch](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/dav/src/common/propfind.rs:1369)
reads the account's retained paths since S. A valid saved token is sufficient; no forged token
or hostile store is needed.

Bind historical deletion authorization to immutable calendar identity and its current path
lifetime, or advance a safely fenced sync boundary when a shared calendar changes slug and
refuse older incremental state. Extend case 1 with an existing-grant rename into a private
calendar's former slug, not just creation followed by a new grant.

## 3. [P2] Preserve self-delivery for ordinary updates and cancellations

Location: [§6.2, self-delivery trigger and REQUEST method](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1577)
and [planning handoff](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:2577).

The replacement rule triggers only when the writer's ATTENDEE line or the event's SEQUENCE
changes, and always constructs a REQUEST. Two ordinary scheduling operations fall outside it.

**An update without a sequence increment.** Alice organizes an event attended by Bob and
Carol. Bob changes SUMMARY through Alice's shared calendar, preserving SEQUENCE. Upstream
recognizes the change and sends Carol an update, but excludes Bob as local under the merged
addresses. SUMMARY and DESCRIPTION are absent from the fields that increment SEQUENCE
([organizer.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/organizer.rs:35)).
The fork's sequence floor only prevents a decrease; it does not increment an unchanged value
([sequence.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/calendar/sequence.rs:103)).
Neither self-delivery trigger fires, so Bob's personal copy retains the old summary after sync.

**Deletion of the shared source.** If Bob deletes that event, the merged-address builder again
excludes him from CANCEL recipients. `itip_cancel` reads the source tree immutably and
increments only the outgoing CANCEL's SEQUENCE
([event_cancel.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/event_cancel.rs:22);
[recipient selection and CANCEL sequence](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/event_cancel.rs:47)).
There is no committed event tree after deletion from which the specified REQUEST can be built.
Sending the old active tree as a REQUEST would not cancel Bob's personal copy.

The local-recipient filter applies to both paths
([send_update_messages](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/mod.rs:221)).
The original decline case is fixed, but the new mechanism does not preserve delivery of the
other updates and cancellations to the writer's own attendee addresses.

Define self-delivery for scheduling changes that do not increment SEQUENCE and require a
self-CANCEL for deletion/removal where appropriate. Preserve the explicit decline REQUEST
when ordinary scheduling produces nothing. Add SUMMARY-only and writer-deletes-shared-event
cases, checking the writer's personal copy after drain and refresh.

## 4. [P2] Deliver every address-specific token when the writer attends under aliases

Location: [§6.2, one self-delivery item with the writer's token](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:1580).

The new mechanism mandates one REQUEST item for the writer's inbox with “the writer's token.”
Tokens belong to attendee addresses, not accounts. Both sealing and opening a pending item
require exactly one address/token pair
([seal/item.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/calendar/seal/item.rs:152);
[reader validation](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/calendar/seal/item.rs:210)).

For example, Bob creates an Alice-organized recurring event through Alice's shared calendar,
attending the master as `bob@` and an override as `bob-work@`. Both addresses belong to
Bob and are excluded as local by the merged-list builder, so invitation delivery depends on
the new self path. The owner-address token union creates distinct pairs for the two addresses
([scheduled_attendees and token_union](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/calendar/fanout.rs:948)).
The single self item can carry only one of them, and Bob's imported copy takes only that pair
([drain import](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/calendar/drain.rs:890)).

A later RSVP that changes only the other instance is sent from that instance's attendee
address
([attendee.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/scheduling/attendee.rs:110)).
Fan-out cannot find the corresponding token and marks the REPLY not attempted
([fanout.rs](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/calendar/fanout.rs:915)).
This uses one local attendee per component and does not rely on the separately accepted
multiple-local-attendee ambiguity.

Send one self item per distinct attending writer address, preserving the current pending
format, or explicitly version and design a multi-pair item and its reader. The existing
REQUEST merge can retain pairs arriving for different addresses
([drain merge](/Users/jay/Development/stalwart/.worktrees/release3-mail-spec/crates/groupware/src/calendar/drain.rs:1128)).
Test replies from both aliases after self-import and after token replacement.
