# Zero-access calendar: JMAP calendars and browser sign-in (release 6) design spec

Date: 2026-11-13. Status: revision 7, merged as pull request #90 on
2026-11-21 (merge commit `04145d586`; the review record was condensed
after the merge), with one wording correction of
2026-11-18 on Jay's decision (3.3: the UID lease covers creates; a change
of calendars checks without it; follow-up issue #91), and with the
changes of 2026-11-18 that follow the pull request's review. Decided by
the project owner on 2026-11-18: 3.3 (an attendee's create sends no
reply), 5.3 and 6.7 (a key calendar's delete in one transaction on both
protocols, its refusal above 5,000,000 bytes before any cancellation, and
its fence), 5.5 (the drain only within the token's scope), 6.5 (a refused
attachment link fails only its own occurrence) and section 10 (the `code`
masking confirmed; the amendments of the drain's and the search pass's
triggers and of release 5's chunked collection `DELETE`). The
controller's rulings on confirmed findings of that review: 3.3 and 6.5 (a
non-member held to an event's effective privacy), 6.8 (the event state of
a caller who is not the owner) and section 10 (invariant 72's sentence on
a key lookup that outlasted its token, and the costs). The controller's
rulings on findings of the reviews of the two fix waves, each confirmed
by the project owner on 2026-11-19: 5.3 and two section 10 amendment
sentences (FoundationDB, a read replica and the ephemeral store keep the
chunked delete; the fence's refusal after the cancellations accepted as a
residual; a `Blob` call still triggers the search pass for a token
without the mail scope). The follow-up of 6.8 was filed on his word of
2026-11-19 as issues #93 and #94. The project owner's disposition of
2026-11-19 on what a token's scope admits on DAV (no change) is stated
in 5.5. The changes of 2026-11-19 that follow the pull request's second
review are all the controller's rulings on confirmed findings of that
review, reported to the owner (none was put to him as a decision):
4.6 and invariant 72 (a keyed
cache entry ends before its token's signed expiry), 5.2 (a keyed CalDAV
`COPY` inside one calendar answers `no-uid-conflict`), 5.3 and 6.7 (the
calendar delete's members, every name removed, the unwritten-child
refusal and the size check's reserve), section 8 and section 10 (the
costs). The changes of 2026-11-20, which follow the pull request's third
review and a second whole-branch review, are these: decided by the
project owner on 2026-11-20, 6.8 (a non-owner's `destroyed` ids,
inherited and accepted; D15 a sync-correctness rule, not an access
control); the controller's rulings, reported to the owner, 4.3 (the
access entry a write adds is never the one removed), 5.3, 6.7 and
section 10 (the owner's destroy check made only without
`onDestroyRemoveEvents`, stopping at the first member; the costs); and,
as documentation, a pointer in 3.4 to the `destroyed` ids and 3.3's
statement that an alert-only or keyword-only JMAP update of a key
account's event takes the converter's round trip, an item then open
for the owner. The changes of 2026-11-20 that follow the pull request's
fourth review: decided by the project owner, 3.3 (no change in this pull
request for alert-only and keyword-only updates; issue #96); and the
controller's rulings, reported to the owner, 4.3, 4.4 and 4.5 (an
access entry already expired at its write is not stored and removes
none; two residuals of that rule).
Revision 7 incorporates Jay's
approved D29 amendment (`2026-11-14-zero-access-jmap-paged-sync-amendment.md`,
in the repository's history at `04145d586`), recorded 2026-11-15: key-calendar continuations carry their original sync
start explicitly (6.8, 9.1, 9.2, 9.8 and 10 to 12). Revision 6 applies Jay's decision
of 2026-11-14 on when a key account's browser sessions end (2, 4.3 to
4.7, 7, 9.1, 9.2, 9.7 and 10 to 12): only a credential change ends
them, and D19 is rewritten for it. Revision 5 applied what the plan's
drafting found on 2026-11-14 (4.2 to 4.7, 5.2, 5.3, 6.1, 6.3 to 6.9, 8,
9.1, 9.2, 9.4, 9.7 and 10 to 12) and added four decisions: which Bearer
values are split (D26), token refusals that are not counted for banning
(D27), a store error on one object of a `/set` (D28) and a paged sync at
the sharing boundary (D29). Revision 4 corrected the spec
against the plan's preflight of 2026-11-14 (2, 3.1 to 3.4, 4.2 to 4.7,
5.1 to 5.4, 6.1, 6.4 to 6.10 and 7 to 13) and added nine decisions:
who owns each step of a write (D17), where the scheduling status is
resolved (D18), what a token binds to (D19), the cap on keyed grants
(D20), what a removal asserts (D21), the refresh without a secret (D22),
the Bearer request at a stale generation (D23), untraced token answers
(D24) and the calendar refusal's own text (D25). Jay gave the go-ahead
to plan from revision 3 on 2026-11-14, after the codex pass on it left
no open finding. Revision 3 answered
the re-review of revision 2 (findings 4 to 6 of the same file): how a
superseded refresh entry is retired when the grant advances and the
record's write does not follow (2, 4.3, 4.5, 4.7, 9, 10; finding 4), the
paused-refresh test (9.2; finding 5), and the expanded form of negated
queries (9.2; finding 6). Revision 2 answered
Jay's codex review of revision 1
(the design's review, in the repository's history at `04145d586`): the vault
assertion on every write of the token record (4.3 to 4.5, 4.7, 9, 10;
finding 1), how a refresh reads the record against the grant's progress
(4.5, 9; finding 2), and the event-level matcher of unexpanded text
queries (6.4, 9.2, 11; finding 3). Scope: release 6 of the
zero-access fork (jaylaney/stalwart), GitHub issue #22, milestone "Release 6:
JMAP calendars and web client", item 4 of release 1 spec section 1.2. Base:
the fork on upstream Stalwart v1.0.3 with release 7 merged (PR #84; code
references are to its head `82b8984aa`, where the plan's preflight read them).

The terms key account, master key (MK), session keys, key cache,
authentication generation, visible by structure, pending item, drain,
fan-out, token pair, UID lease, pin, grantee, grant ring, collection key,
key domain, `SENT-BY` placement and the second run are those of the release
1 spec (`2026-10-06-zero-access-calendar-design.md`), the release 2 spec
(`2026-10-11-zero-access-invitations-design.md`), the key pinning spec
(`2026-10-17-zero-access-key-pinning-design.md`), the mail spec
(`2026-10-20-zero-access-mail-design.md`), the sharing spec
(`2026-10-22-zero-access-sharing-design.md`), the iMIP spec
(`2026-11-02-zero-access-imip-design.md`) and the search spec
(`2026-11-04-zero-access-search-design.md`), cited by release. Every
invariant of those specs (1 to 56) holds except where section 10 amends one
by name. Jay's decisions are stated as decided. Decisions of this spec are
marked **D1** to **D29**, each with its reason; section 12 lists the ones
Jay should check.

This spec adds four terms:

- **Token secret.** Thirty-two random bytes that the server appends to an
  OAuth token it issues for a key account and never stores (4.2).
- **Token record.** The per-account record that holds the master key
  wrapped under keys derived from token secrets (4.3).
- **The pipeline.** The key-account calendar operations that CalDAV runs
  today, moved into the `groupware` crate so that CalDAV and JMAP both call
  them (section 5).
- **The reference client.** Bulwark Webmail 1.21.2, static edition
  (1.3).

## Implementation and review routing

Jay set these assignments on 2026-11-14. They apply to every task in
[plan 17](../plans/2026-11-14-zero-access-17-jmap-calendars.md), including
tests, documentation and fixes after review. The controller briefs,
adjudicates and integrates; implementation and review use separate agents.

| Role | Model | Reasoning effort |
|---|---|---|
| Implementer | Sol (`gpt-6.1-sol`) | `high` |
| Focused task reviewer | Astra (`gpt-6-astra`) | `high` |
| Reviewer of identity, authorization, persistence, ordering or concurrency contracts | Astra (`gpt-6-astra`) | `xhigh` |
| Final whole-branch reviewer (Task 44) | Astra (`gpt-6-astra`) | `xhigh` |

Each dispatch sets the model and effort explicitly. Reviewers are
independent of the implementer and read only. The final whole-branch
review uses a fresh Astra agent with a clean context. Fixes go back to a
Sol implementer and then to a separate Astra reviewer under the same
effort rules. If a required route is unavailable, report the limitation;
do not silently substitute another model or lower the effort.

## 1. Purpose

A key account's calendar works from a standard JMAP calendar client in a
browser, with the client unmodified.

The release ships the server side only. It builds, bundles, hosts and
configures no client (Jay, 2026-11-13). The earlier specs called this
release "JMAP calendars and a web client"; the second half now means "what
a web client needs from the server".

### 1.1 In scope

- **Sign-in that carries keys.** An OAuth token issued to a key account
  through the authorization-code flow opens the account's keys on every
  HTTP request that presents it (section 4). The mail spec deferred this to
  release 6 (its section 1.2, "Bearer and OAuth wraps").
- **JMAP calendar methods for key accounts.** `Calendar/get`, `/set`,
  `/changes` and `/query`; `CalendarEvent/get`, `/query`, `/set`,
  `/changes`, `/queryChanges` and `/parse`; `ParticipantIdentity/get`,
  `/changes` and the default-identity form of `/set` (section 6).
- **Shared calendars through JMAP.** A grantee reads and writes a shared
  key calendar through JMAP under the rules CalDAV applies today (6.1).
- **Scheduling from JMAP.** Invitations, replies and cancellations written
  through JMAP go through the fan-out and the iMIP submitter, never the
  task queue (5.4).
- **Pending invitations.** A keyed JMAP request whose token's scope
  admits calendars, and that holds a calendar call that parsed with its
  capability in the scope-filtered `using`, runs the drain (5.5).
- **The session and principal objects** advertise the calendar capability
  for key accounts (6.10).

### 1.2 Out of scope

- Any web client, its hosting, its Application registration and the
  server's CORS settings. They belong to whoever sets a client up.
- **Granting a share through JMAP** (`shareWith`). Sharing stays on the
  account page; a client's share dialog gets an error (Jay, 2026-11-13;
  6.11).
- `CalendarEventNotification` objects, `Principal/getAvailability` for a
  key account, `CalendarEvent/copy`, per-user event data, JMAP object
  metadata and downloadable inline attachments (6.11).
- Keys for a token of the device-code flow, and for a Bearer token on IMAP,
  POP3, SMTP or ManageSieve (4.8).
- A client's password, two-factor and app-password screens. Registry
  credential edits stay refused for key accounts (release 1); the account
  page does them.
- File content for key accounts (release 8).

### 1.3 Assumptions

The reference client was read at its tag `1.21.2`
(`github.com/bulwarkmail/webmail`). The design relies on these facts about
it; section 13 lists them again for the plan's preflight to re-check
against a running instance.

- Every JMAP method call is an HTTP POST to the session's `apiUrl`. It
  opens no WebSocket. Its push connection is a `GET` of `eventSourceUrl`
  with the `Authorization` header, and it reads state strings only. This
  matches search spec decision 1: keys live on HTTP requests, and the push
  connection never holds them.
- The static edition signs in by posting the account name, password and
  second-factor code to the server's `POST /api/auth`, then exchanges the
  code at `POST /auth/token`. It treats every token as an opaque string,
  puts it in `Authorization: Bearer`, refreshes on a 401, and serialises
  refreshes across tabs because upstream rotates refresh tokens.
- It shows the calendar only when the primary account's
  `accountCapabilities` holds `urn:ietf:params:jmap:calendars`, and reads a
  sharer's calendars by sending the same calls with the sharer's account
  id.
- It acknowledges a `CalendarEventNotification` by destroying it.
- Its event editor sends `useDefaultAlerts: true` on every new event.

The edition with its own server component signs in with HTTP Basic when the
account has no second factor, which carries keys today, and otherwise with
the same token flow. That edition keeps the credential in its own server's
cookies; the release makes no claim about it.

## 2. Threat model delta

Release 1's guarantee and limits stand. This release changes four things.

**New at rest: the token record.** It holds MK wrapped under keys derived
from token secrets (4.3). A token secret exists only in the token string
the client holds. The record alone opens nothing, and so does the server's
own token-encryption key: upstream's tokens are sealed under a key the
server holds, and their payload names the account, grant and generation,
so a wrap keyed by anything in the payload would be open to the operator.
That is why the fork adds a secret of its own (4.1). An entry can outlive
its token until the next maintenance run, in three cases: the token
expired, its grant was destroyed through the registry, or the grant
advanced past it and the record's own write did not follow (4.5, 4.7).
The maintenance purge runs once a day by default. The record holds at
most 32 grants of an account (D20).

**New for a short time: the hand-off.** Between the password step and the
code exchange, MK is stored wrapped under a secret that travels inside the
authorization code (4.4). The entry lives for the code's lifetime, ten
minutes by upstream's default, and is deleted when the code is redeemed.

**New at the client.** A browser now holds a credential that opens the
account's keys for as long as its grant lives: upstream's default is
thirty days without use, an operator setting. Whoever takes a live token
and the stored record has the account's keys, exactly as whoever takes the
password and the stored vault record does. A native CalDAV client stores
the password itself, so this is not a new class of exposure, but it is a
new place. A token's OAuth scope limits what the server will do for it; it
does not limit what its secret opens.

**A vault write that changes no credential does not end a browser
session.** Only a password change, a recovery and a second-factor change
end the account's browser sessions (D19). Revoking an app password does
not. Nor does a new recovery key, an app password's creation,
publication or withdrawal, the self-check's repair or the first pin's
rewrap. A person who wants every browser signed out changes the password
or the second factor, or revokes the grants.

**The operator additionally learns** that an account signs in from a
browser client, how many grants it holds, and when each was refreshed.
Upstream's grant objects already record this.

**Whoever serves the client's code can read what the user reads.** No
client is part of this release, but the limit belongs beside the
guarantee: a web client is code delivered at each visit, and its server can
change it.

**Nothing new in the clear from JMAP.** A JMAP write stores what the same
write through CalDAV stores (section 3). The JMAP-only values that upstream
keeps outside the iCalendar tree are either visible by design already
(sort order, visibility flags, default-alert offsets, an IANA time zone
id), refused (6.5, 6.11), or applied by value into the sealed tree (D11).

## 3. The contract, the oracle and the stopping rule

### 3.1 Parity by construction

**D1.** A JMAP write to a key account's calendar data is the CalDAV write
of the iCalendar tree that upstream's JSCalendar converter produces from
it. A JMAP read is upstream's JSCalendar projection of the tree a CalDAV
read serves.

The reason is the 27 places where upstream's JMAP write path differs from
the fork's CalDAV path for key accounts: left alone it would queue
plaintext invitations to the task queue, skip sealing, the UID lease and
the SEQUENCE floor, drop attendee token pairs and write per-user data in
the clear. Each earlier release settled those points for CalDAV through
rulings and mutation runs. A second, JMAP-shaped copy of that pipeline
would need the same work again and would then drift. So the fork does not
insert key-account branches along upstream's JMAP path. It moves the
CalDAV orchestration into `groupware` (section 5) and calls it from both
protocols.

**D17. Who owns each step.** Upstream's validation steps stay in each
protocol's own handler. CalDAV's stay in `dav` as they are. The JMAP key
branch calls upstream JMAP's own functions for the same checks, in
upstream's order. The pipeline in `groupware` holds only the fork's
key-account steps: the contexts, the UID lease and its uniqueness check,
the scheduling assembly, the fan-out, sealing, the commit with its fences
and retry, and the mail guard.

The reason is that CalDAV's key branches are threaded through upstream's
handler code. Giving JMAP the whole CalDAV write would mean copying
upstream's checks into `groupware`, where they would drift from
upstream, or moving them out of `dav`, which widens every merge. Each
protocol already has its own checks, and upstream keeps the two in step
for ordinary accounts. D1 therefore holds by construction for the fork's
steps, and for upstream's checks as far as upstream's two handlers agree
(3.4).

The work of one JMAP event write for a key account, in order, with the
owner of each step. Line references are to
`crates/jmap/src/calendar_event/set.rs`; the plan's preflight confirmed
them.

| Step | Owner |
|---|---|
| Parsing, the method's access check, `ifInState` | upstream, unchanged |
| Admission (6.1). For an update, reading the stored event and building the unsealed view the patch applies to, through a read context | fork |
| Taking the properties 6.5 rules on out of the incoming object | fork. Upstream's patcher stores `useDefaultAlerts` with `preferences_mut`, so it must not see that property. |
| Patch application, synthetic recurrence ids, JSCalendar validation, calendar-count checks | upstream, unchanged |
| Conversion to iCalendar (`export_icalendar`), and a generated UID when a create has none | upstream, unchanged |
| The per-user split (update `:820-887`, create `:1761-1779`) | skipped (D10) |
| Organizer assignment, the SEQUENCE increment and the `updated` stamps, the participants limit, the size cap | upstream's functions, called by the key branch on the converted tree. They are JMAP's shaping of the tree and have no CalDAV counterpart. |
| Upstream JMAP's own checks: the calendar ACL checks of the cache, UID unchanged on an update, the changes a grantee may make (`EventChanges::assert_allowed`), user-data limits (`validate_user_data`, `validate_user_data_changes`), the object-count and disk quota pre-checks, the previous and next alarms, and the build of the event data (`CalendarEventData::new_for` with the key-account flag) | upstream's functions, called by the key branch in upstream's order. CalDAV runs its own equivalents in its own handler (D17). |
| The SEQUENCE floor (`za_sequence_floor`) | fork, called by the key branch right after upstream's increment, as CalDAV's handler calls it |
| The scheduling status (D18): the key branch passes whether `sendSchedulingMessages` is true and that a denied status is an error. The pipeline resolves the status and answers `forbidden` for a denied one before any write. | the pipeline |
| The fork's key-account steps: the calendars' fresh access rules through the contexts; UID uniqueness under the lease; the scheduling builders and the second run; the recipient cap; statuses for unreachable and withheld recipients; the fan-out; sealing; the commit with its fences and its one retry; the mail submission | the pipeline |
| The response | upstream's assembly |

Upstream's JMAP refuses an object with `noSupportedScheduleMethods` when
a participant cannot be reached by any method. For a key account the
pipeline records that participant's status and saves the object, as
CalDAV does (3.3).

### 3.2 What each protocol decides

The pipeline takes these as inputs, because the two protocols differ in
them by design:

| Input | CalDAV | JMAP |
|---|---|---|
| Whether to schedule, and whether a denied status fails the write (D18) | always, except a `DELETE` with `Schedule-Reply: F`; a denied status is traced and the write saved | `sendSchedulingMessages`, which defaults to false (D4); a denied status answers `forbidden`. A calendar destroy always schedules and never fails on a denied status (D16) |
| Preconditions | `If-Match`, `If-None-Match`, `If-Schedule-Tag-Match`, checked by the handler | `ifInState`, checked by the handler |
| Resource name | from the request path | random, as upstream's JMAP assigns |
| Placement | one calendar per `PUT`; `COPY` and `MOVE` change it | `calendarIds`, on create and update (5.2) |
| Error form | `DavError` | `SetError` or a method error (section 8) |

### 3.3 Where key accounts differ from upstream's JMAP

Accepted, each because the CalDAV behaviour is the qualified one:

- **Each object of a `/set` is its own commit** (D3).
- **A recipient that cannot be reached, or that the fan-out withholds,
  does not fail the object.** A participant no method reaches, a pinned
  key that changed, an unroutable local address and the other withheld
  classes record their status on the attendee and the object is saved,
  as on CalDAV. Upstream's JMAP refuses the first of these with
  `noSupportedScheduleMethods`.
- **An unexpanded text query is matched in memory**, over the document
  the search index would hold for the event, with the index's own code
  for terms and text conditions (D12).
- **UID uniqueness is CalDAV's rule**: one UID per calendar. A create
  checks it under the UID lease against current archives. A change of an
  event's calendars checks each calendar the event enters against current
  archives without the lease, as CalDAV's `MOVE` does (5.2), so a create
  of the same UID that crosses such a change is not excluded (issue #91).
- **Destroying a calendar sends cancellations** for its events, as a
  CalDAV collection `DELETE` does (D16).
- **No drafts, no per-user event data, no participant-visibility flags**
  (6.5). Because a key account's event holds no per-user data (D10),
  every JMAP update of it, an alert-only or keyword-only one included,
  stores the tree the converter produces, where upstream keeps the
  stored tree for a per-user-only change, so 3.4's round-trip limitation
  applies to those updates too. The project owner decided on 2026-11-20
  to make no change in this pull request (issue #96).
- **Default alerts are applied by value** (D11).
- **An attendee's create sends no reply.** An attendee who creates, as
  its own copy, the event of an invitation that has not reached the
  calendar yet sends no reply from that create, and the copy does not
  schedule later edits. Upstream's JMAP create falls back to the
  attendee's reply; CalDAV's create does not, and the pipeline's create
  is CalDAV's (D1). Decided by the project owner on 2026-11-18.

A difference in which JMAP is the stricter side, by the controller's
ruling on the pull request's review:

- **A non-member is held to an event's effective privacy.** For a
  caller who is not a member of the account, JMAP applies the higher of
  the stored flags' privacy and the unsealed event's to reads, the text
  query, destroy and update, and answers `notFound` for an event whose
  view does not open for the caller (6.3, 6.4, 6.5). CalDAV applies the
  stored flags, which a non-standard `CLASS` leaves public under field
  policy 2.

### 3.4 Inherited limitations, accepted

- A calendar colour set by a CalDAV client is a dead property; JMAP's
  `color` is a calendar preference. Upstream keeps the two apart for every
  account, so a colour chosen in one kind of client does not show in the
  other.
- An event written with `sendSchedulingMessages: false` and later edited
  through CalDAV is scheduled from that edit on, as upstream does for an
  ordinary account.
- Upstream's JSCalendar conversion decides what survives a round trip
  between the two protocols (for a key account's alert-only and
  keyword-only updates too, 3.3).
- A caller who is not the owner receives in `destroyed` the ids of the
  owner's events it may not see (6.8); accepted by the project owner on
  2026-11-20.
- JMAP does not check a calendar's supported component set
  (`supported-calendar-component-set`), for any account. CalDAV does, on
  a `PUT` and on the target of a `COPY` or `MOVE`. A JMAP write can
  therefore put an event into a calendar that a CalDAV client restricted
  to tasks. The check is upstream's and exists in CalDAV's handler only,
  so by D17 the pipeline adds none.

### 3.5 The oracle

A differential test drives one edit script through both protocols and
compares the results (9.3). The script covers create, update, placement,
recurrence edits, invitations to a key account and to an outside address,
replies, and deletion. For each step it compares:

- the JSCalendar projection of the unsealed stored event, less ids, the
  resource name and timestamps;
- the items delivered to key recipients and the mail handed to the sink,
  by recipient, method, UID, SEQUENCE and recurrence id;
- each attendee's schedule status and the stored token pairs' recipients.

The script's JSCalendar input is produced from its iCalendar input by
upstream's own converter, so a difference is the pipeline's, not the
script's.

### 3.6 The stopping rule

A review finding that JMAP and CalDAV differ for a key account, in
anything section 3.2 does not list as an input, is fixed by routing the
JMAP path through the pipeline, not by a new rule. Review of a revision
stops when a pass returns no finding of that kind and no finding that a
JMAP write stores plaintext CalDAV would have sealed. What upstream's JMAP
does differently from CalDAV for every account is recorded in 3.3 or 3.4
and is not a finding.

## 4. Sign-in: tokens that carry keys

### 4.1 Why the token needs a secret of its own

Upstream's access and refresh tokens are AEAD ciphertexts under the
server's token key (`seal_token`, `crates/common/src/auth/oauth/token.rs`).
The payload holds the account id, the grant type, issue and expiry times,
the credential version and the grant's id and generation. A refresh
token's nonce is derived from the grant id and generation, so a retried
refresh returns the same string. Nothing in a token is unknown to someone
who holds the server's token key and the grant objects. A master-key wrap
keyed by the token would therefore be a wrap the operator can open.

### 4.2 The token secret

**D5.** For a grant of a key account, every token string the token
endpoint returns is upstream's token, a `~`, and the URL-safe base64 of 32
random bytes, without padding. `~` is in RFC 6750's token alphabet and
outside upstream's (`A-Z a-z 0-9 - _ .`), so the split is unambiguous.
Each token string has its own secret. The server never stores one.

`Server::decode_access_token` strips the suffix before it opens the token,
so every consumer of a token (HTTP, the token, revocation and
introspection endpoints, and the mail protocols' bearer mechanisms)
accepts the longer string. Two callers read the secret: the HTTP layer's
Bearer arm (4.6) and the refresh grant (4.5).

**D26.** The strip applies to every token that begins with the server's
own token header, for every account, since the account is not known
before the token is opened (section 10 records the exception). Any other
Bearer value, such as an external directory's token, is passed through
unchanged, whatever characters it holds. The reason is that RFC 6750
lets another issuer's token hold `~`, and such a token belongs to an
ordinary account.

A suffix on an ordinary account's token of this server is refused by the
HTTP layer (4.6) and ignored by the other consumers. Introspection reports upstream's
state only: a keyed token whose entry is gone still introspects as
active.

From a secret the fork derives, with HKDF and fixed labels under
`za/v1/`: a wrapping key, an 8-byte tag that identifies the secret
without revealing it, and a key that seals the next refresh secret. The
wrap is the primitive the app-password wrap uses (`wrap_key`,
`crates/vault`). Its associated data binds the account id, the grant id,
the kind (access or refresh) and, for a refresh wrap, the generation.
The hand-off's wrap (4.4) binds the account id, the code or consent
token it is stored under, and the binding (D19).

### 4.3 The token record

`PrincipalField::ZeroAccessTokens = 153`, on the account's principal
document, an rkyv record the fork owns, absent when the account holds no
keyed grant. It holds:

- a format version and the binding its entries were made under (D19,
  below);
- at most 32 grants (D20); per grant, the grant id and:
  - at most two **refresh entries**: generation, the MK wrap, the next
    generation's secret sealed under this generation's secret (once
    issued), and the expiry of the refresh token;
  - at most four **access entries**: the secret's tag, the MK wrap and
    the access token's expiry. Past four, a write removes the
    earliest-expiring of the entries stored before it (the first on a
    tie), so the entry it adds is never the one removed, also when it
    expires before every stored one (a lowered access-token lifetime, or
    node clocks that differ). An access entry that has already expired
    at the time of its write is not stored and removes none (the same
    boundary as reconciliation: an entry lives while its expiry is after
    the write's time).

An entry's expiry is computed in the request that writes the entry,
before its store reads. On a retried
refresh the next generation's entry can therefore carry an expiry later
than its token's, by the retry's delay. The token's own expiry and the
sweep bound it. Two residuals of the rule that an expired access entry
is not stored: an access entry's expiry is fixed from a clock read made
just after its token was sealed, so it can be one second later than the
token's signed expiry, and an entry written in that second is stored
though its token is already refused; and on a cluster whose nodes'
clocks differ by more than the write's overrun, a node with the lagging
clock refuses a token whose entry the writing node did not store, and
the client refreshes.

**D19. The binding.** One function, `za_session_binding`, computes from
the vault record the value that a key account's OAuth tokens and its
token record bind to. This spec calls that value the binding. It is a
64-bit fingerprint of the account's credentials as the vault record
holds them: the salt, the Argon2 parameters, the password verifier and
the second-factor secret, present or absent. It is SHA-256 over a fixed
label under `za/v1/` and those fields in that order, each field of
variable length preceded by its length. The value is the digest's first
eight bytes, and 0 is replaced by 1, because upstream reads a credential
version of 0 as unversioned. A key account's credential version, which
upstream seals into every token and checks at every use, is this value.
The hand-off entry (4.4) and the token record store it, and every reader
compares it with the vault record's current one. A vault write deletes
the token record exactly when it changes the binding (4.7).

The binding changes with a password change, a recovery and a
second-factor change, and with nothing else. A key account's browser
sessions therefore end only when a credential changes. They survive
every other vault write: a new recovery key, an app password's creation,
publication, withdrawal or revocation, the self-check's repair, and the
first pin's rewrap of the private key. Jay decided this on 2026-11-14,
replacing his rule of 2026-11-02 that every vault write ends them. The
reason is that the first pin's rewrap is automatic: under the old rule a
person's first invitation to another key account signed their browser
out.

The binding is not the authentication generation. The key cache's
generation stays the vault revision, so a vault write that leaves the
binding unchanged still evicts resident keys, and the next request opens
them again from the token record (4.6).

A write that stores a wrap is conditional on two things in one batch:
the record's previous bytes, and the vault record's stored bytes as the
writer read them, after it checked that their binding is the one under
which it obtained MK. `vault_commit_store` makes the same kind of
assertion for its own companion records
(`crates/common/src/auth/vault.rs`). A writer whose assertions fail
reads both records again. If the vault record's binding has changed, it
stops. Otherwise it decides again from what it now reads and may retry,
three attempts in all. It never stores its wraps under another binding.
A vault write that changes the binding and a token-record write
therefore order strictly: either the vault write's batch deletes what
the other wrote, or the other's assertion fails and it stops. A vault
write that leaves the binding unchanged only makes the other writer read
again and retry.

**D21.** A write that only removes entries (a grant's deletion, the
sweep) asserts the record's previous bytes and nothing else. Such a
writer holds no MK and has read no vault record. A removal can never
store a wrap, so a vault assertion would protect nothing.

Every write also reconciles the record with the grants it names, at the
cost of one registry read per grant. It drops:

- entries that have expired;
- grants that hold no live entry, and grants that no longer exist;
- every refresh entry below its grant's stored generation. Upstream
  treats such a generation as a replay, so nothing may open under it.

**D20.** The record holds at most 32 grants. A sign-in that would add a
33rd evicts the least recently used one: the grant whose `lastUsedAt` in
the registry is oldest, which upstream sets at sign-in and at every
rotation. The write drops that grant's entries, and the grant is deleted
after the write. If the deletion fails, the grant has no entries and
ends at its next refresh (4.5). The reason for a cap is that upstream
allows 256 grants per account. A record of that many would pass
FoundationDB's limit on a value, and every write reads each grant it
names from the registry.

The record is not an authority for login. Upstream decides whether a
token is valid, bound to the vault record through D19's binding.
The record only yields keys to a token upstream already accepted.

### 4.4 Sign-in: from the password to the grant

The authorization-code flow has two requests: the password step
(`POST /api/auth`, `authorize_code` in
`crates/http/src/auth/oauth/auth.rs`, which the server's own sign-in page
and the reference client both call) and the code exchange
(`POST /auth/token`). MK is available only in the first. The grant exists
only after the second.

**D6. The hand-off.** When the password step succeeds for a key account,
`authenticate_login` takes the session keys from `authenticate_with_keys`
and returns them. The hand-off entry is written where the code or the
consent token is created (`issue_auth_code`, `request_consent`), before
that code or token is stored, so neither ever exists without its entry.
The entry is MK wrapped under a key derived from a fresh 32-byte secret.
The binding (D19) is stored with the wrap and is part of its associated
data, so a writer of the in-memory store cannot change it. The entry is
stored in the in-memory store under
upstream's OAuth prefix, a `~` and the 40-character code, with the code's
expiry; the task that purges upstream's codes purges it too. The secret
is appended to the code the client receives, with no separator, so the
first 40 characters are upstream's code and the rest is the secret. The
code exchange splits them, runs upstream's checks (client, redirect URI,
PKCE, single use), takes and deletes the hand-off entry, opens MK, and
after `issue_new_grant` has created the grant writes the grant's first
refresh entry (generation 0) and access entry, each under a fresh token
secret (the access entry only if it has not already expired at the
write, 4.3). That write carries the vault assertion of 4.3: the exchange
reads the vault record, requires its binding to equal the hand-off's,
and asserts those bytes. The device flow calls the same login function
and drops the keys (4.8).

The entry is stored rather than kept in process memory so that the two
requests may land on different nodes of a cluster.

Three paths reach the code:

- **Consent given or not needed.** The code is issued in the password
  request. The reference client sends `consent: true`.
- **Consent pending.** `request_consent` returns a consent token and the
  password is gone when `confirm_consent` runs. The hand-off is written
  under the consent token, its secret appended to that token.
  `confirm_consent` strips it before upstream's check of the token, which
  accepts exactly 40 characters, takes the entry and writes a new one
  under the code.
- **A grant already exists for the client and scopes.** The code is
  issued in the password request.

Rules:

- For a key account, the authorization-code flow yields a keyed grant or
  none. A code exchange that finds no hand-off entry, or one that does
  not open, or one made under a binding that is no longer current,
  answers `invalid_grant`.
- If the record write fails after the grant was created, the grant is
  deleted. A changed binding answers `invalid_grant`: the
  password or the second factor changed while the sign-in was in
  flight. A vault write that left the binding unchanged only makes the
  write read again and retry (4.3). Any other failure answers a server error. Either way no wrap
  is stored and the client signs in again.
- A consent that is refused deletes the entry. So does a confirmation
  that fails its checks after the consent token was taken.
- A sign-in past the cap of D20 evicts the least recently used grant.
- A code exchange that finds a secret on the code of an account that is
  not a key account answers `invalid_grant`. Upstream would not have
  found a code of that length. The code is spent.
- If a vault write of any kind crosses the password step, between the password's
  check and the hand-off's write, the step answers an authentication
  failure and issues no code. It is not counted against the client's
  address (D27).

### 4.5 Refresh

Upstream accepts two refresh generations at any time: the last one a
client proved it received (the stored generation) and the one issued
after it. Presenting the newer one rotates. Presenting the older one
while a rotation is pending is a retry and returns the same next token.
Presenting anything older revokes the grant (`rotate_oauth_grant`,
`crates/common/src/auth/oauth/grant.rs`).

The fork follows that state machine without changing it:

1. The refresh token is validated as upstream does. For a key account
   the fork then reads the record and applies D22 (below). After that,
   `rotate_oauth_grant` runs before the fork decides anything else. A
   superseded token therefore revokes the grant exactly as upstream
   revokes it, and the grant's entries are removed with it.
2. If the rotation issues, the fork reads the record and classifies the
   presented generation against the grant's refresh entries. The grant's
   rotation and the record's write are separate operations, so the
   record can have moved on since upstream accepted this request.
   In every attempt, after reading a usable record and its named grants
   from the registry, but before reconciling or classifying that record,
   a stored generation of the presenting grant above the presented
   generation answers `Superseded`: `invalid_grant`, no entry written
   and no grant deleted. This request's rotation left the stored
   generation at the presented generation, and it never falls; a higher
   value therefore belongs to a later rotation, even when this attempt
   read an older record snapshot. A record that does not name the grant
   still answers `Lost`. Otherwise reconcile and classify as follows.
   - **Its entry is there and opens** with the presented secret. The
     fork has MK and goes on.
   - **Its entry is gone and the record holds a higher generation of the
     grant.** A newer refresh finished while this one was in flight.
     Upstream accepted this request before that happened, so it is not a
     replay, but its generation is now superseded. The fork answers
     `invalid_grant` and changes nothing: it does not delete the grant
     and it writes no entry.
   - **Anything else**: the grant has no entry, or the presented entry
     does not open. The keys are lost. The fork deletes the grant and
     answers `invalid_grant`. A keyed grant never becomes a keyless one.
3. For the generation it issues, the fork needs that generation's refresh
   secret. If the presented entry already holds a sealed next secret,
   this is a retry or a concurrent duplicate: the fork opens it and
   returns the same secret, and writes the next generation's entry again
   from MK and that secret if it is missing. Otherwise it draws a new
   one, writes its refresh entry and seals it into the presented entry.
4. It draws a new access secret and writes its entry, unless that entry
   has already expired at the write (4.3); the refresh entry is written
   either way, so the client can refresh again. Upstream's access
   token is a new string on every response, so a retry adds an entry.
5. The record keeps the presented generation and the next one and drops
   lower ones. The write carries the two assertions of 4.3. If they
   fail, the fork reads the vault record and the token record again.
   - If the vault record's binding has changed, the fork stops and
     answers `invalid_grant`. Only a credential change does that (D19).
     A refresh that lost its vault assertion to any other vault write
     goes on.
   - Otherwise it classifies again from step 2, with the record as it
     now is, three attempts in all. It writes only if the presented
     generation's entry is still in that record. MK held in memory from
     the earlier read is never used to put back an entry the record no
     longer holds.
6. If the write cannot be made for another reason, the answer is a
   server error. The grant has rotated, so the client's retry is
   upstream's retry case and step 3 finds or creates the same entry.

A superseded refresh token opens nothing once the refresh that
superseded it has written the record: its entry and the link that led
from it are gone, and no later write restores them.

The grant's advance and the record's write are two writes, and the
second can fail or never run. If the grant advances to a generation and
the process stops, or the write fails and the client never retries, the
entry below that generation is still in the record. It is removed by
whichever comes first: the client's retry (step 6), any later write of
the record, or the sweep. Each drops every refresh entry below its
grant's stored generation (4.3, 4.7). Until then the superseded secret
and the stored record together still open MK, for at most one
maintenance interval. Upstream refuses the superseded token itself at
once, and revokes the grant if it is presented.

The second case of step 2 is more lenient than upstream is to the same
request arriving a moment later, which upstream calls a replay and
answers by revoking the grant.

**D22.** A key account's refresh always reads the record, before the
rotation. A refresh token that carries no secret, presented for a grant
that has entries, answers `invalid_grant` and changes nothing: the grant
does not rotate. So does a suffix that is not a secret. A token without
a secret for a grant without entries belongs to a keyless grant (4.8)
and refreshes as upstream does. The reason is that revision 3 let a
keyed refresh token, presented without its secret, rotate into a keyless
pair, against step 2's rule that a keyed grant never becomes a keyless
one. The refusal has to come before the rotation, or it would not leave
the grant unchanged.

One residual is accepted. A keyed grant can lose its entries while
upstream still accepts its refresh token: the eviction of D20 or the
first write of 4.4 succeeded and the grant's own deletion failed. Such a
grant refreshes as a keyless one if its client presents the token
without the secret. It opens nothing.

### 4.6 Use

Keys are still attached in one place, `authenticate_uncached`
(`crates/http/src/auth/authenticate.rs`). The key cache is keyed by a
fingerprint of the token as presented, suffix included, so a Bearer
token is cached as a Basic one is, under the same idle and age limits
and the same generation fence.

On a cache miss with a Bearer token of a key account that carries a
secret, the HTTP layer reads the record, finds the access entry by the
secret's tag, and opens MK. It also reads the vault record and requires
its binding to equal the record's (D19). The session keys' generation is
the vault revision of that read. A miss costs two point reads, one HKDF
and one AEAD open, and the token is opened a second time when its cache
entry is published (the keyed entry's deadline, below); no Argon2 runs.

**D23.** If that generation is not the account's current one, the
request is refused with 401. It is never served uncached, as a password
login in that state is today. The reason is that a password login has
just proved its password against the vault record, while a token proves
only that it was once issued. If the vault write that crossed the
request changed a credential, it ended the token's session (4.7). If it
left the binding unchanged, the client's refresh succeeds, and its next
request opens the keys again at the new generation.

The lookup lives in the HTTP layer and not in `authenticate_with_keys`.
The IMAP and POP3 logins call that function too
(`crates/imap/src/op/authenticate.rs`, `crates/pop3/src/op/authenticate.rs`)
and keep whatever keys it returns for the life of the connection (4.8).

**D7.** A token that carries a secret whose entry is missing, expired or
does not open is refused with 401, as an invalid token is. It is never
served as a keyless token. A client that refreshes on 401 then gets a
new access token with a new entry, where a keyless answer would leave it
holding a token that reads nothing.

**D27.** A refusal by the fork is traced as an authentication failure
with a fixed reason and is not counted against the client's address for
banning. That covers an entry that is missing or does not open, a
malformed secret, a secret on a token that cannot carry one, and a stale
generation (D23). The reason is that such a client holds a token
upstream accepted: one whose entry was dropped by the cap of four gets
one 401 and refreshes. A wrong password is counted as before.

D7 is decided at a cache miss. A node that has a token's keys cached
serves it until that cache entry ends, at most the token's remaining
life, even if the record's entry was dropped by the cap of four. A keyed
entry ends before its token's signed expiry: at publication the HTTP
layer opens the token again and lowers the entry's deadline to the fresh
remaining lifetime less one second, never raising it; a token that has
expired by then publishes nothing, and the request is served with its
keys. The reason is that the lookups after the token's first decode
would otherwise extend the entry past the signed expiry. An ordinary
account's entry keeps upstream's deadline (inherited). This is the
controller's ruling on the pull request's second review. A
grant's deletion evicts the cached entry at once. A suffix on an
ordinary account's token is refused with 401. That is a token of this
server; a Bearer value that is not one is not split (D26).

A token without a secret is a keyless token, as today: an API key, a
device-flow token, a master-user token. Search spec 6.2's table applies
to it unchanged. A keyed access token presented without its secret is
one too, by the client's own act.

The keys work on every HTTP path. A CalDAV request with a keyed Bearer
token is a keyed CalDAV request.

### 4.7 Ends

- **A credential change.** `vault_batch_write`, which every vault write
  passes through, deletes the token record in the batch that writes the
  vault record, when the write changes the binding (D19): a password
  change, a recovery and a second-factor change. Each ends every browser
  session of the account and destroys its wraps in one commit. Upstream
  refuses the tokens by credential version. A sign-in or a refresh that
  obtained MK before the change cannot store a wrap after it: its vault
  assertion fails, and it finds the binding changed (4.3).

  Every other vault write leaves the record and the sessions in place: a
  new recovery key, an app password's creation, publication, withdrawal
  or revocation, the self-check's repair (also when the drain runs it),
  and the first pin's rewrap of the private key. Such a write still
  raises the authentication generation, so resident keys are evicted and
  a request that crossed it gets one 401 (D23). The first invitation a
  key account sends to another key account pins a contact; its browser
  stays signed in.
- **A grant deletion.** `delete_oauth_grant` removes the grant's entries
  (D21). It reads the token record only when the grant's account is a
  key account. Upstream calls it on revocation (`/auth/revoke`), a replayed
  refresh token, a client mismatch, when it prunes idle or excess grants
  and when an account's registry password changes. The fork calls it for
  the eviction of D20. A failed removal of entries is logged and never
  fails the grant's deletion; the sweep bounds it. Upstream's
  end-session endpoint deletes no grant. A grant can also be destroyed
  through the registry (`x:OAuthGrant/set`), which does not pass through
  that function, and account destruction deletes grants the same way.
  Upstream refuses such a grant's tokens at once. Its entries are
  removed by the next write of the record or the next sweep, whichever
  comes first: both drop every grant that no longer exists.
- **Expiry.** An expired entry is refused at use, removed by the next
  write of the record, and removed by a sweep in the maintenance task's
  purge step, beside the lease sweep (D8). The sweep needs no key.
- **Account destruction.** The record goes with the principal's
  properties.

- **An interrupted refresh.** A refresh entry below its grant's stored
  generation, left behind when the record's write did not follow the
  grant's advance (4.5), is removed by the next write of the record or
  the next sweep.

**D8.** The sweep reconciles every token record as a write does (4.3):
it removes expired entries, the entries of grants that no longer exist,
and refresh entries below their grant's stored generation. With it, an
expired token, a superseded refresh token whose successor's write was
interrupted, or the token of a grant destroyed through the registry,
opens nothing after the next maintenance run. Without it an idle
account's entries would stay until its next sign-in or refresh. The fork adds no hook to the registry's writer for
the destroy case: the sweep bounds it, and the hook would be a new
insertion in upstream's registry code.

The sweep finds the records through the registry's account list, with
one point read per account. Property keys are ordered by account, so no
range scan finds one field across accounts. A failure on one account is
logged and the sweep goes on. Its writes follow D21. A record that does
not decode is left as it is: it may be a record of a later format. A
sign-in or a refresh writes over it.

### 4.8 What stays keyless

**D9.** Three kinds of token of a key account carry no keys:

- **The device-code flow.** The device holds its code before the password
  is typed, and the code is the store's lookup key, so there is nothing
  secret to wrap under. No client the product targets uses the flow.
- **Bearer mechanisms on IMAP, POP3, SMTP and ManageSieve.** Only the
  HTTP layer reads the token record (4.6), so a bearer login on a mail
  protocol gets no keys and a key account's mail session is refused as
  today. An IMAP or POP3 connection keeps its keys for its whole life,
  which would outlast the access token that opened it. A mail client
  still signs in with a password or an app password.
- **API keys and impersonation**, as before.

### 4.9 What a client must tolerate

Nothing new: a token is a longer opaque string of the same alphabet as
RFC 6750 allows, with no whitespace. A keyed access token is about 45
characters longer than upstream's.

## 5. The pipeline

### 5.1 The move

**D2.** The key-account orchestration that lives in the `dav` crate today
moves to `groupware`, which both `dav` and `jmap` already depend on
(`jmap` does not depend on `dav`). By D17, what moves is the fork's
steps, and of those only what JMAP calls:

- the admission and role helpers (the body of `za_calendar_gate`,
  `za_writer_keys`, `za_roles` and `ZaRoles`);
- the context helpers (`za_write_contexts`, `za_mutation_context`, the
  read contexts and views, `za_refresh_event`, and the listing of the
  calendars a grantee may see);
- the UID lease functions of a create (`za_take_uid_lease`,
  `za_assert_is_unique_uid`, `za_lost_create_fence`, `za_commit_create`);
- the scheduling assembly (`za_schedule`, the second run, and the status
  step of D18);
- the commits of an event create, an event update, an event delete and a
  collection delete, each with its fan-out, sealing, fences, bounded
  retry and mail guard;
- the contexts of a placement change. `za_move_contexts`, which serves
  CalDAV's `COPY` and `MOVE` of an event today, becomes the one function
  that both protocols call for a change of an event's calendars. The two
  placement refusals and the reseal of an event's dead-property
  container move with it.

The key branches are not separate functions today. Each CalDAV handler
therefore keeps its upstream steps and calls the pipeline at the points
where its key branches stand, in the same order. A CalDAV create already
works this way. The JMAP key branch calls the same functions.

What stays in `dav`, because JMAP has no caller for it: collection copy,
move and rename; event rename; the bodies of the event `COPY` and
`MOVE`, which call the moved functions; the event `PROPPATCH` branch;
and calendar creation and calendar property change, whose key steps are
already `groupware` functions (`seal_calendar`,
`calendar_view_in_domain`, `seal_calendar_changes`). The code that reads
and writes dead-property containers stays too: it is upstream's.

The moved functions return a protocol-neutral error. `dav` maps it to the
`DavError` values it produces today; `jmap` maps it as section 8 says.
The CalDAV handlers keep URI parsing, header preconditions and response
bodies.

The move changes no behaviour. Its proof is that the existing CalDAV
tests pass without edits to their assertions: the za suite's CalDAV
modules, key-mode `webdav_tests` and the independent CalDAV client job.
Three unit tests in `dav` name moved functions; their call expressions
change and their assertions do not. The move is the plan's first group
of tasks, each reviewed on its own, before any JMAP code calls the
result.

The groupware primitives the orchestration calls are already public
(sealing, `ZaFanOut`, `ZaDrain`, the sharing resolvers, `delete_za_at`,
`delete_with_events_za`) and do not move. `ZaImipPending` is in
`common`.

### 5.2 Operations

| Operation | CalDAV caller | JMAP caller |
|---|---|---|
| Create an event | `PUT` to a new path | `CalendarEvent/set` create |
| Update an event | `PUT` to an existing path | `CalendarEvent/set` update |
| Change an event's placement | `COPY`, `MOVE`, `DELETE` of one name | `CalendarEvent/set` update of `calendarIds`, as one update |
| Delete an event | `DELETE` | `CalendarEvent/set` destroy |
| Create a calendar | `MKCOL` | `Calendar/set` create |
| Change a calendar | `PROPPATCH` | `Calendar/set` update |
| Delete a calendar | collection `DELETE` | `Calendar/set` destroy |
| Read an event or a calendar | `GET`, `PROPFIND`, `REPORT` | the `/get` and `/query` methods |

Placement follows the CalDAV rules. An event may be in several calendars
only when all are unshared calendars of its owner. A shared calendar is
an event's only calendar. Moving an event between an unshared and a
shared calendar reseals it in the target domain and reseals its
dead-property container, in one commit. What `COPY` and `MOVE` refuse,
the JMAP update refuses. A keyed CalDAV `COPY` inside one calendar, with
or without an overwrite, is answered 403 `no-uid-conflict`, as upstream
answers an ordinary account; release 4's keyed handler skipped the check
for a `COPY` with an overwrite. One function resolves the contexts of a
placement change for both protocols (5.1), so the rights it asks and the
refusals it gives are the same by construction. A calendar's path name
changes only by a CalDAV rename; JMAP's `name` is the display name.

A JMAP update may change placement and content together. The pipeline
runs it as one update: it resolves the contexts of the old and new
calendars, seals the new tree in the target domain, and commits once. A
JMAP create may name several calendars under the same rule.

The commit asserts the context of every old and every new calendar. A
create checks UID uniqueness under the lease in every calendar it names.
A placement change asks for the rights CalDAV's `COPY` and `MOVE` ask
for. On every calendar the event is in: `RemoveItems` when the event
leaves a calendar, otherwise `ReadItems`. On each calendar it enters:
`AddItems`. When the content changes too: `ModifyItems` on every
calendar the event is in. It checks UID uniqueness in each calendar the
event enters, against current archives. The limit on the number of
calendars is upstream JMAP's own check (D17). The event's next alarms
are computed over its new calendars. CalDAV's requests name one source
and one target; the function takes the full sets, of which CalDAV's are
the smallest case.

For JMAP the content counts as changed when the tree that upstream's
shaping produces differs from the stored one. Upstream stamps `updated`
on an owner's update, so most JMAP moves ask `ModifyItems` as well. The
two share levels cannot tell the difference: `edit` holds every right a
placement can ask and `view` none.

### 5.3 One commit per object

**D3.** For a key account, a `/set` call runs the pipeline once per
object, in upstream's order (creates, updates, destroys), each with its
own commit. The state check (`ifInState`) runs first; `newState` is the
change id of the last object's commit, or the old state when no object
commits. A state read after the last object would also cover another
writer's later change, which the client would then never fetch.

Upstream builds one batch for the call. The pipeline cannot: a create
commits its document id before its fan-out (key pinning), holds a UID
lease, delivers items to recipients before its own commit and retries
once after rewrapping. One object's failure must not strand another's
delivered invitations. JMAP defines each object of a `/set` as its own
atomic unit, so per-object commits conform.

**D28.** A store error on one object does not fail the method. That
object and every later object of the call report the set error
`forbidden` with a fixed description (section 8) and are not attempted.
The objects already committed keep their answers, and `newState`
reflects them. An error before the first object fails the method, as
upstream. The reason is D3: with one commit per object, a failed method
would hide from the client what the call had already committed.

**A calendar's delete is one object.** On a store that can establish it
(RocksDB, SQLite, PostgreSQL, MySQL), a key account's calendar is deleted
with its events in one transaction, on both protocols (`Calendar/set`
destroy and a CalDAV collection `DELETE`). A delete whose batch would
exceed the store's chunk size (5,000,000 bytes) by the builder's own
count is refused before any cancellation is sent: `forbidden` with a
fixed text over JMAP (section 8), 403 over CalDAV. The remedy is to
delete events first. A write that crosses the delete makes it answer "try again"
(`forbidden` with the "modified" text over JMAP, 503 over CalDAV): the
child list is compared with a snapshot no older than the account's
Calendar change counter before the first fan-out and again immediately
before the commit, and the commit carries an operation that fails unless
the commit is assigned the change id that follows the one read. The
reason: split at commit points, a delete whose later chunk failed left
the calendar with part of its events and an answer that contradicted the
store, and a create that crossed it could leave an event whose calendar
was gone. Upstream's JMAP destroy of an ordinary account's calendar is
one transaction too. Decided by the project owner on 2026-11-18. The
store behaviour the fence relies on (a batch's change id allocated in the
transaction before its merge functions run, and an error from one
aborting it) is pinned by a test on RocksDB and SQLite; for PostgreSQL
and MySQL it was read, not run.

The calendar's members are the events whose stored record has a name
under it, read from the snapshot's records, not from the path index,
which shows one document per href; on every store, and the owner's
`onDestroyRemoveEvents` check uses the same definition of membership.
That check is made only when the flag is not set, and it stops at the
first stored record that names the calendar; with the flag set nothing
is read for it. Every name an event has
under the deleted calendar is removed, and the event is deleted when no
other calendar holds it. On a fenced store the steps run in this order:
the build; the first comparison of the members; the refusal, with "try
again", of a listed child the build wrote nothing for (one that left the
calendar and came back while the delete was built would otherwise pass
both comparisons unasserted); the size check, with the fence's operation
counted in advance; then the fan-out, the second comparison and the
commit. The reason: the path index can hide a second document stored at
one href, and an event can hold two names in one calendar, and either
would otherwise be left naming the deleted calendar. These are the
controller's rulings on the pull request's second review.

The fence's refusal can come after the cancellations went out: key
attendees then hold a CANCEL for events the organizer still has, and the
outside attendees' mail is dropped. That is the failure class every
failed commit of this delete already had; it now has more triggers (a
create into the calendar during the fan-out, any calendar commit of the
account in the last window), and a retry converges. It is accepted as a
residual by the controller's ruling on the pull request's review,
confirmed by the project owner on 2026-11-19.

By the controller's ruling on the pull request's review, confirmed by
the project owner on 2026-11-19: on FoundationDB, a read replica and the
ephemeral store the delete is chunked at the store's commit points and
not fenced, without the unwritten-child refusal and without the size
check, and a later chunk that fails leaves the earlier chunks' deletions
committed; it takes the members from the stored records and removes
every name under the calendar, as on the other stores. FoundationDB's limits on a transaction's time and
size would make the delete of a calendar of a few thousand events fail
on every attempt, each after its cancellations went out. A read
replica's reads can lag the primary, so the comparison could pass on a
stale child list. The ephemeral store applies each operation as it goes,
with no rollback, and is the data store only in web bootstrap mode,
where no key account exists. Issue #92 carries a note of 2026-11-18
that this pull request makes the delete one transaction; a note added
on 2026-11-19, after the push, of what it keeps (a calendar too large
for one transaction, the stores that keep the chunked form, the fence's
refusal after the cancellations, a large delete retried whole on a
conflicting commit, and PostgreSQL and MySQL read, not run); and a
comment of 2026-11-20 on a default-calendar id written across a
concurrent delete. Its title and body were not edited.

### 5.4 Scheduling

**D4.** `sendSchedulingMessages` is honoured as JMAP defines it. When it
is false or absent the pipeline runs with scheduling off: no fan-out, no
mail, statuses untouched. When it is true the status is
`ItipSendStatus::resolve` with the owner as a key account, which gives
`SendSealed` or one of upstream's reasons not to send.

**D18.** The status is resolved inside the pipeline, with the sender's
roles: the writer's account, with the owner's addresses added when the
writer is a grantee. The caller passes two things: whether scheduling
was requested, and whether a denied status is an error. For JMAP it is.
A status denied because scheduling is disabled, the account has no
calendar address or it lacks the permission fails the object with
`forbidden` before any write, as upstream's JMAP does. For CalDAV it is
not: the reason is traced and the write saved, as today. An event in the
past is saved and nothing is sent, on both. An event destroy uses the
condition CalDAV's `DELETE` uses, with the same two answers. The reason
is that upstream's JMAP resolves the status with the target account's
addresses, while CalDAV's key path resolves it with the sender's roles,
so that a grantee's write carries `SENT-BY`. Resolving it in one place
gives both protocols the same sender.

With scheduling on or off, an update carries the stored token pairs and
the stored display name into the resealed event, and applies the SEQUENCE
floor after upstream's own increment.

`ItipMessages::queue` is never called for a key account. The three
`resolve(.., false)` call sites in `crates/jmap/src/calendar_event/set.rs`
stay as they are: the key branch leaves upstream's path before them.

### 5.5 The drain

A JMAP request runs `za_drain` once, before dispatch, when its token
holds its own account's keys, the token's scope admits calendars, and
the request holds a call of `Calendar`, `CalendarEvent` or
`ParticipantIdentity` that parsed and whose capability is among those
the request declared and the token's scope left in place (the
dispatcher's own check before a method runs). It never fails the
request. The hook sits beside release 7's search pass hook in
`handle_jmap_request` (`crates/jmap/src/api/request.rs`).

A CalDAV request drains under the same scope rule (`za_drain_keys`,
`crates/common/src/za/calendar.rs`). A Basic login, an app password and
a full-scope token have no restricted scope and are unaffected; a token
whose scope excludes calendars drains on neither protocol, so a key
account used only through such tokens applies no invitation until a
calendar-scoped or password client connects. The method's JMAP
permission is not part of the rule: an account that lacks it still
drains on a calendar call the dispatcher then refuses, and a call whose
back-reference fails has drained, since references are resolved after
the drain. Decided by the project owner on 2026-11-18. What a token's
scope admits on DAV is upstream's scope model, unchanged; the drain's
scope rule limits the drain, not what a token can read. The project
owner's disposition of 2026-11-19: no change for it in this pull request.

Without it an account used only from a browser would never apply an
invitation.

## 6. JMAP methods for key accounts

### 6.1 Admission

`za_assert_calendar_allowed` is replaced. For a target account that is
not a key account, upstream's outcome stands. For a key account, after
the method's own access check:

- **The owner** needs a token that holds the account's keys and is not an
  impersonation.
- **A grantee** needs its own keyed, non-impersonated session and shared
  access to the account. Each calendar is then resolved through
  `za_calendar_context` with the grantee's keys and the grant ring, as on
  CalDAV. No owner session is manufactured.
- **A caller with no access to the account** gets upstream's own answer
  from the method's access check, `accountNotFound`. The fork's refusal
  applies only after that check passes.
- **Anyone else the check admits**, and any keyless token or
  impersonation, gets the method error `forbidden` with a fixed keyless
  description (D25).

**D25.** The keyless description for calendars is a text of its own,
parallel to mail's: "Calendars in this account are encrypted at rest and
are not available to this session." The reason is that the text release
7 uses names mail, and a client shows the description to its user.

`CalendarEvent/parse` admits the owner only (6.6). Admission for
`/changes` and `/queryChanges` runs in the changes handler
(`crates/jmap/src/changes/get.rs`), which the dispatcher does not gate.

The methods of 6.11 keep `accountNotSupportedByMethod`.

### 6.2 The method table

| Method | For a key account |
|---|---|
| `Calendar/get`, `/changes`, `/query` | served (6.7, 6.8) |
| `Calendar/set` | served, less `shareWith` (6.7) |
| `CalendarEvent/get` | served (6.3) |
| `CalendarEvent/query`, `/queryChanges` | served (6.4, 6.8) |
| `CalendarEvent/set` | served (6.5) |
| `CalendarEvent/changes` | served (6.8) |
| `CalendarEvent/parse` | served (6.6) |
| `ParticipantIdentity/get`, `/changes` | served (6.9) |
| `ParticipantIdentity/set` | the default-identity form only (6.9) |
| `CalendarEvent/copy` | refused (6.11) |
| `CalendarEventNotification/*` | refused (6.11) |
| `Principal/getAvailability` | an empty list for a key principal, as today |
| `ShareNotification/*` | unchanged; never gated |

### 6.3 `CalendarEvent/get`

For each event the handler resolves the read context of one of its
calendars (memoised per request), builds the unsealed view
(`event_view_in_domain`) and hands the view's content to upstream's
projector. The owner's read completes a pending sharing boundary, as a
CalDAV read does.

- `isDraft`, `mayInviteSelf`, `mayInviteOthers`, `hideAttendees` and
  `useDefaultAlerts` are false.
- Privacy, hidden participants and the private view for a grantee are
  upstream's, applied to the view.
- A legacy unsealed event of a key account is served to its owner, as on
  CalDAV.
- An inline attachment's link is served as upstream forms it; its blob is
  not downloadable (6.11).
- A read that asks for no property needing the stored content is
  answered from the cache without a read context, as `/query` and
  `/changes` are (6.4, 6.8).

### 6.4 `CalendarEvent/query`

- `inCalendar`, `uid`, `after`, `before`, every sort and
  `expandRecurrences` read the cache and the stored tree's visible
  fields, as upstream does. Start, end, recurrence and UID are visible by
  structure.
- **D12.** `text`, `title`, `description`, `location`, `owner` and
  `participant` are matched in memory over the unsealed view of each
  candidate.
  - **Expanded form.** Upstream's in-memory matcher for expanded queries
    (`InstanceTextFilter`,
    `crates/groupware/src/calendar/instance_filter.rs`), given the view.
    Every condition must match in the instance's own component, and the
    filter is a single condition or a flat AND, as upstream requires.
    Its phrase and prefix matching differ from the index's. That is
    upstream's behaviour for every account.
  - **Unexpanded form.** The event is matched as the one document the
    index would hold for it. The handler builds that document from the
    unsealed view with the function the index builder itself calls
    (`ArchivedCalendarEventContent::index_document`,
    `crates/groupware/src/calendar/index.rs`; the builder,
    `build_calendar_document`, is in
    `crates/services/src/index_queue/document.rs`). It derives the
    document's terms with the internal store's indexer and resolves
    each text condition with the store's own planner of text
    conditions. Both run through the in-memory content part that
    release 7 added for mail search
    (`crates/store/src/za_search/content.rs`). Nothing is copied by
    hand: the values, their order, their joining, the terms and the
    planning are the index's code. A condition of several words
    therefore matches when one word is in the master and another in an
    override, and a phrase matches wherever the index would find it.
    How a phrase matches on a field the index holds without positions
    is whatever the stored index answers; the two-account case of 9.2
    decides it.
    The conditions' results are combined by the filter's AND, OR and
    NOT, with the function upstream combines the index's results with.

  Release 6 therefore depends on release 7's search code: the content
  part, and the two functions of the store it calls. Revision 3 planned
  a matcher of its own that would copy the index's rules. The preflight
  found that the upstream matcher it named does not match phrases and
  prefixes as the index does, and that release 7 already holds the
  index's rules in a form that runs in memory.

  `CalendarEvent/queryChanges` calls the query function directly, so the
  key branch sits inside that function, at two points: before the search
  store is asked, and where the expanded loop reads the stored content.

  A key account has no calendar search documents, so nothing is indexed.
  A text query without a time range opens every event of the account.
  The query holds the terms of its candidate events in memory for the
  request and charges no memory budget for them. Both are accepted
  costs (Jay, 2026-11-14).

  Three differences from an ordinary account remain. An ordinary
  account's index lags its writes; the matcher is always current. An
  external search store has its own text rules; the matcher follows the
  internal store. The index drops terms of a document past one mebibyte
  of term cost, in an order that is not fixed.
- A `metadata` condition is refused, as object metadata is.

### 6.5 `CalendarEvent/set`

The object is validated and converted by upstream's code and written by
the pipeline (3.1). Property rulings:

| Property | Ruling for a key account |
|---|---|
| `isDraft: true` | refused, `invalidProperties`. CalDAV has no drafts, and the flag would sit in the clear. |
| `mayInviteSelf`, `mayInviteOthers`, `hideAttendees` set true | refused, `invalidProperties`. They are row flags with no CalDAV form. |
| `useDefaultAlerts: true` | applied by value (D11). |
| `alerts`, `keywords`, `color` written by a grantee | written to the event itself if the grantee may edit it, otherwise refused. There is no per-user copy (D10). |
| `links` with a `blobId` | refused, `invalidProperties` (6.11). |
| `calendarIds` | the placement rules of 5.2. |
| `sendSchedulingMessages` | D4. |
| everything else | upstream's conversion; the result is sealed by the field policy. |

The rulings on `isDraft`, `mayInviteSelf`, `mayInviteOthers`,
`hideAttendees` and `useDefaultAlerts` apply to the event's own patch. A
patch of one occurrence is left to upstream, which refuses these
properties there. The ruling on `blobId` links applies to every patch.
A refused link fails only the occurrence operation that carries it: the
call's other occurrence edits and destroys of the same event are applied
and committed together, as upstream applies its other per-occurrence
errors.

**D10. No per-user event data.** Sealing refuses an event that holds
per-user preferences (`seal_event_in_domain`), and that stays. CalDAV
skips upstream's per-user split for a key account, so JMAP does too: an
alert, keyword or colour belongs to the event and every reader sees it.

**D11. Default alerts by value.** A create or update with
`useDefaultAlerts: true` replaces the event's alerts with the current
default alerts of the calendar, taken from the writer's own preference
entry and chosen by whether the event shows without a time. Nothing
records that the defaults were used; a read returns
`useDefaultAlerts: false` and the alerts. Changing a calendar's defaults
later does not change existing events. Upstream stores the flag as
per-user data, which D10 excludes, and the reference client sends it on
every new event, so refusing it is not an option. A CalDAV client writes
its alarms into the event in the same way.

The alarms are written as upstream writes a default alert into an
iCalendar view: the description is the event's title, or "Reminder", and
an email alert's recipient is the writer's account name. The event's
former alarm components are removed from the tree, not only unlinked, so
none is stored as an orphan.

A grantee's entry of a shared calendar is a copy of the owner's, made at
the grant, default alerts included. A grantee that set none therefore
writes the defaults the owner had at the grant, and the owner's later
changes do not reach it.

The `/set` answer is upstream's, computed from the object as the client
sent it. It does not carry the alerts the server wrote, and it reports
`useDefaultAlerts: false` only when the client omitted the flag. A client
learns the alerts from its next `CalendarEvent/get`. Returning them would need an insertion in upstream's
server-set code, which the release does not make (Jay, 2026-11-14).

Destroying an event runs the pipeline's delete with the event's domain,
so cancellations and declines go through the fan-out. Destroying one
occurrence through its synthetic id is upstream's conversion to an
update.

For a caller who is not a member of the account, an update and a
destroy are checked against the event's effective privacy, the higher of
the stored flags' privacy and the unsealed event's, as `/get` and the
text query apply it (3.3). An event whose view does not open for the
caller answers `notFound`. A grantee at `edit` therefore cannot destroy,
declassify or rewrite an event whose `CLASS` is a sealed non-standard
value through JMAP. Over CalDAV it can still delete it and read it
whole, under field policy 2's recorded limit.

### 6.6 `CalendarEvent/parse`

Served when the token holds the keys of the account named in the call. A
blob of a key account is a sealed upload or a part of a sealed message.
Upstream's blob reader, which the handler calls, already reads it
through the unsealing reader with those keys and never raw, by the mail
spec's rule for sealed sources, so only admission changes. A grantee
cannot parse a blob in the sharer's account. The method writes nothing.

### 6.7 `Calendar/get` and `Calendar/set`

`Calendar/get` reads each calendar through `calendar_view_in_domain` for
the caller, so the name and description are the caller's sealed personal
values. `shareWith` and `myRights` come from the stored grants, which are
visible by design. A grantee's list holds the calendars its fresh
context admits, as CalDAV's listing does. `Calendar/query` lists ids
from the cache, as upstream does, and opens nothing.

`Calendar/set`:

- **Create** seals the name, description, colour and a custom time zone
  under the owner's keys (`seal_calendar`). Sort order, the visibility
  and subscription flags, default-alert offsets and an IANA time zone id
  stay in the clear, as release 1 decided.
- **Update** edits the caller's own entry through the calendar's context
  and reseals it (`seal_calendar_changes`). A grantee changes only its
  own entry, and only at the `edit` level: a grantee at the `view` level
  is refused with `forbidden`, as CalDAV's PROPPATCH refuses it. The
  handler never adds a preference entry: a grantee's
  entry exists from the grant, and a caller with no stored entry is
  refused.
- **Destroy** runs the pipeline's collection delete (D16): one
  transaction with its events where the store can establish it, refused
  above the size cap before any cancellation, and answered "try again"
  when a write crosses it or an event moved while the delete was built
  (5.3). Upstream's refusal of a non-empty calendar without
  `onDestroyRemoveEvents` stays; for the owner it is decided from the
  calendar's members as 5.3 defines them, so an event the path index
  hides also counts. The check is made only when the flag is not set,
  and it stops at the first stored record that names the calendar; with
  the flag set nothing is read for it.
  A grantee cannot destroy a shared calendar, as on CalDAV.
- **`onSuccessSetIsDefault`** writes the default calendar id, an id only.
- **`shareWith`** in a create or an update is refused with the set error
  `forbidden` and a description that names the account page.

**D14. Colour on a shared calendar.** Today a shared calendar's personal
entry cannot hold a colour: `seal_calendar_changes` refuses one and the
grant's conversion drops the owner's. The sealed personal bundle already
has a colour field that is always written empty
(`seal_personal_entry`). Release 6 fills it: each principal's colour of
a shared calendar is sealed in its own bundle, the conversion carries
the owner's colour across, and a bundle written before this release
reads as no colour. A new grantee's entry starts with the owner's
colour, because a grant copies the owner's entry. Without this the owner of a shared calendar loses
its colour when sharing and gets an error on setting one from a JMAP
client.

**D16.** Destroying a calendar sends cancellations and declines for its
events, as the CalDAV collection `DELETE` does. Upstream's JMAP sends
none; that is recorded in 3.3.

### 6.8 `/changes` and `/queryChanges`

The change log is keyless and is served as upstream serves it, with
upstream's filtering for a grantee. The event state given to a caller
who is not the owner of a key account is the exception (below). A
caller who is not the owner receives, as upstream gives them, the ids
of the owner's events it may not see (those outside its shared
calendars and confidential ones inside them) in `destroyed` when those
events change or are deleted: ids only. This is inherited
behaviour, accepted by the project owner on 2026-11-20.

**D15.** For a grantee, a `sinceState` at or before the sync boundary of
a shared calendar it can see (the later of its grant and its last
rename), or any `sinceState` while such a boundary is pending, answers
`cannotCalculateChanges`. An initial sync is never refused, and the
owner is never checked: CalDAV refuses incremental tokens only, and only
for a sharer's calendars. This is the JMAP form of the 403
`valid-sync-token` that CalDAV's `sync-collection` gives (sharing spec
5.2). One predicate decides both, for the same callers and the same
calendars. A JMAP state and a CalDAV sync token are the same change id,
so the comparison is the same. The project owner confirmed on
2026-11-20 that D15 is a sync-correctness rule and not an access
control: a state's origin is taken as the client presents it, and a
client that presents another origin obtains item ids of the log's
range, never content.

**D29. A paged sync.** The predicate is applied to the state the sync
started from, with the same comparison on every page. A key-account
`Calendar/changes` or `CalendarEvent/changes` continuation carries the
existing range/count plus its original start, `Initial` or `Exact(s)`.
Every later page preserves that start unchanged. For a grantee, an
initial-origin page stays exempt from the sharing-boundary check; an
exact-origin page calls `ZaSyncBoundary::refuses(s)`, including D15's
pending-boundary rule. The owner remains exempt. `/queryChanges` uses
the same checked changes path and preserves origin in every continuation
it consumes or produces.

The origin is never inferred from `from_id`, retained records or the
current boundary. Source review found that an initial sync and a valid
incremental sync can yield identical legacy continuations after normal
log retention, then require different answers after a rename. The
approved amendment (in the repository's history at `04145d586`)
records that collision; it replaces revision 6's inference rule.

Use a new transient `State` variant, not a stored archive. Its opaque
string has prefix `z` and STALWART base32/LEB128 fields in this order:
version `1`; origin tag (`0` Initial, `1` Exact); the original nonzero
exact ID only for tag `1`; `from_id`; `to_id - from_id`; and positive
`items_sent`. Reject unknown versions/tags, missing or trailing fields,
zero exact IDs, zero counts and arithmetic overflow. Budget the encoding
buffer for the extra fields. Ordinary `n`, `s` and `r` encodings stay
byte-for-byte unchanged.

Only key-calendar changes emit `z`. A legacy `r` continuation without
origin is `cannotCalculateChanges` for a grantee of a key calendar, so
the client restarts from Initial. A key owner's existing `r` chain keeps
emitting `r` until completion, without inventing an origin; its next
Initial/Exact start emits `z` if paged. Ordinary accounts retain their
existing state handling. No released key-calendar JMAP continuation is
being migrated: these methods were refused before this release.

An out-of-scope `z` in a normal state request slot (`sinceState` or
`sinceQueryState`) gives `invalidArguments`. In an `ExpectedState`
precondition slot (`ifInState`, `ifFromInState`, `destroyFromIfInState`)
it gives `stateMismatch`, as an invalid string already does. Keep the
existing validation and authorization order; add no global account
lookup or parser permission path. Origin changes only the boundary
check, never access rights. Upstream's log reader, retention, filtering
and pagination remain unchanged, including their independent validity
checks. No stored layout, database field, secret, setting or client code
is added. Reference-client qualification remains a separate gate.

**The event state of a caller who is not the owner.** Upstream's event
state follows the log's item changes: the last one, or, after a snapshot
rebuild, the log's tip. A grant and a rename are
container changes, so that state can lie at or below a sharing boundary,
and a grantee given it would be refused at its next call until an event
changed. For a caller who is not the owner of a key account:

- `CalendarEvent/get`, `/query` and `/set` answer the snapshot's highest
  change id. Every boundary the snapshot holds lies at or below it, and
  its log record exists.
- `/changes` and `/queryChanges` end at the highest change id of a
  snapshot read before the page when the request's state was initial or
  exact: the page read the log to its end.
- A continuation of an exact-origin paged sync ends there only when the
  log holds no event change between its last item change and that tip.
  Otherwise it ends at its last item change, and the next call sends
  what follows.
- A continuation of an initial-origin paged sync ends where upstream ends
  it, at the range's last event change. D29 exempts its pages from the
  boundary check, and a `z` state carries no tip of its first page, so a
  boundary that completed while the sync paged cannot be told from one
  that completed before it began. Raising over the first kind would hide
  a newly shared calendar's events, which the next call's refusal and
  restart deliver, so neither kind is raised.

The state therefore also moves on a container-only change, and a poll
then returns empty lists. An owner's and an ordinary account's states
are unchanged. Two cases stay open:

- (a) A paged sync from the initial state ends below any sharing
  boundary above its last event change, whether the boundary completed
  while it paged or before it began; its next call is refused, and a
  restart that pages from the initial state again ends at the same state,
  until an event is written, the client syncs in one page, or it takes
  its state from `/get` or `/query`. A client that follows the protocol
  holds the state `/get` gave it and does not enter this flow. This is
  the cost the controller's ruling accepted.
- (b) A grant that commits between a call's boundary check and its read
  of the tip, in the same call, leaves a raised state above the grant:
  the state of an exact request, or of the last page of an exact-origin
  continuation, which the raise serves under its guard. This is new with
  the raise.

The follow-up, filed on the project owner's word of 2026-11-19: issue #94
(read the calendar log's tip before the sharing-boundary check, which
closes (b)) and issue #93 (carry the first page's tip in the `z` state,
which would let a paged initial sync end above a boundary that predates
it and so remove (a); it needs a new version of the state's codec).

### 6.9 `ParticipantIdentity`

`/get` and `/changes` are served. The identities are the account's own
addresses, which the registry already holds in the clear. The first read
writes the identities record, which holds those addresses and the
account's registry name, as upstream does.

`/set` serves `onSuccessSetIsDefault`. A create, update or destroy is
refused with `forbidden`: an identity's name is free text that would be
stored in the clear, and the reference client only switches the default.
The refusal's description is fixed (section 8).

### 6.10 The session and principal objects

**D13.** A key account's account object and `primaryAccounts` advertise
`urn:ietf:params:jmap:calendars` and `:calendars:parse` for every token,
keyed or not. A keyless token meets the refusal per method. This is
release 7's rule for mail, and one session shape per account avoids a
client seeing capabilities change between requests. A grantee's session
lists the sharer's account with the calendar capability. The `Metadata`
capability stays hidden.

`Principal/get` shows a key principal's calendar capability block. Its
`mayGetAvailability` is false, because the principal's availability is
not served. `mayShareWith` and the account's calendar capability object
are upstream's values.

### 6.11 What stays refused

| Feature | Answer | Why |
|---|---|---|
| `Calendar/set` `shareWith` | set error `forbidden` | A grant needs the grantee's pinned key and converts the calendar; the account page does both (Jay, 2026-11-13). |
| `CalendarEventNotification/*` | `accountNotSupportedByMethod` | These objects are the scheduling inbox. A client that acknowledges by destroying would delete an invitation the drain has not applied. The fork writes no change notifications for key accounts. |
| `CalendarEvent/copy` | `accountNotSupportedByMethod` | Cross-account copies are refused on CalDAV too. |
| Per-user event data | D10 | |
| Object metadata on calendars and events | as upgrade ruling 66 | |
| Inline attachment blobs | the download is denied, as today; a write with a `blobId` link is refused | The embedded-blob reader reads the stored archive raw. No targeted client writes event attachments. |
| `Principal/getAvailability` of a key principal | an empty list | Unchanged. |
| `ParticipantIdentity/set` create, update, destroy | set error `forbidden` | 6.9. |

## 7. The reference client against a key account

What a user of Bulwark's static edition sees after release 6. This table
is the basis of the manual checklist (9.8).

| Feature | Result |
|---|---|
| Sign in with password, and with password and code | works (section 4) |
| Month, week, day and agenda views | work |
| Create, edit, move and delete an event | work |
| Recurring events: this one, this and future, all | work |
| Tasks | work |
| Invite a key account or an outside address; see replies | works |
| After the first invitation to a key account | the browser stays signed in (D19) |
| Accept or decline, from the calendar and from an invitation email | works |
| Calendars shared with the user, read and edit | work |
| Answer for the owner of a shared calendar | works |
| Create, rename, colour, reorder and delete a calendar | work |
| Default reminders | applied when the event is saved (D11) |
| Import an `.ics` file | works |
| Search | works, matched on the server per request |
| Live updates | work, as state changes |
| The share dialog | an error naming the account page |
| Free/busy and suggested times | hidden |
| Change-notification pop-ups | none |
| Password, two-factor and app-password screens | errors; the account page does these |
| File uploads | refused until release 8 |
| Its own shared tags and templates | absent (object metadata) |

## 8. Error handling and traces

The pipeline's neutral errors map to JMAP as follows.

| Condition | JMAP answer |
|---|---|
| Missing access, a context that refuses, a placement CalDAV refuses (among them the linked event CalDAV answers 409) | set error `forbidden`, with upstream's description where upstream's own check answers first |
| The object is gone, or its calendar is gone when the pipeline resolves it | set error `notFound`. A `calendarIds` entry that does not exist gets upstream's answer. |
| UID already used in the calendar | set error `invalidProperties` on `uid`, with upstream's description |
| A property ruling of 6.5 or 6.7 | set error `invalidProperties` naming the property |
| `sendSchedulingMessages` is true and the account may not send | set error `forbidden` with upstream's reason, from the pipeline, before any write (D18) |
| More recipients than the cap, or a scheduling error CalDAV refuses the write for | set error `invalidProperties` on `participants`, with upstream's description |
| The UID lease is busy, a create lost its fence, or a commit failed its assertion after the retry | set error `forbidden` with upstream's "modified by another process, try again" description, as upstream's `reject_uncommitted` gives |
| A calendar delete whose one transaction would exceed the store's chunk size (5.3) | set error `forbidden` with the fixed description below, before any cancellation; CalDAV answers 403 |
| A calendar delete crossed by a write to the account's calendars, or holding a listed child the build wrote nothing for (5.3) | set error `forbidden` with "Another process modified this calendar, please try again."; CalDAV answers 503 |
| Quota | upstream's `overQuota`, from upstream's pre-check or from the commit |
| A keyless token, an impersonation, a caller the access check admits without keys of its own | method error `forbidden` (6.1) |
| A caller with no access to the account | upstream's `accountNotFound`, from the method's access check (6.1) |
| A store error on an object of a `/set` | set error `forbidden` with the fixed description below, for that object and every later one (D28) |
| A store error before the first object of a `/set`, or in a read | the method fails, as upstream |

An object that fails does not undo the objects before it (D3).

Fixed descriptions, where upstream's own check does not answer first:

- a store error on an object (D28): "The server could not complete this
  change, try again."
- a calendar too large to delete at once (5.3): "This calendar holds
  too many events to delete at once. Delete some of its events first."
- `shareWith`: "Calendar sharing is managed on the account page."
- `isDraft: true`: "Drafts are not available for zero-access accounts."
- a participant-visibility flag set true: "This property is not
  available for zero-access accounts."
- a link with a `blobId`, on `links`: "Event attachments are not
  available for zero-access accounts."
- an identity create, update or destroy: "Participant identities in
  this account cannot be created, changed or destroyed; only the default
  identity can be switched."

Traces of the new paths carry account, grant and document ids, counts
and fixed reasons. They never carry a token, a token secret, a code, a
tag, event content or an address. The JMAP request body of a key account
is already read untraced. A refused or failed token open is traced as an
authentication failure with a fixed reason. The mail guard's traces
(`ZA_IMIP_DROPPED`, `ZA_IMIP_ABANDONED`) apply to JMAP writes as they do
to CalDAV ones.

**D24.** A successful answer of the token endpoint is untraced, for
every account. The reason is that upstream traces every response body
that is not marked untraced, and this one holds the token strings, for a
key account with their secrets. Marking it for key accounts only would
leave an ordinary account's tokens in the trace, which is no better.

## 9. Testing

### 9.1 Unit

- The token split, the derivations and the wrap's associated data: an
  entry does not open under another account, grant, kind or generation.
  A value without the server's token header is not split (D26). A
  hand-off does not open under another binding.
- The record codec, its caps and its reconciliation: expired entries,
  a grant that is gone, a refresh entry below the grant's stored
  generation. A write that only removes entries succeeds whatever the
  vault record holds and cannot add an entry (D21).
- The refresh steps of 4.5 against upstream's rotation outcomes: rotate,
  retry, replay, the three classifications of step 2, a lost race on the
  record that finds the presented generation superseded, a failed vault
  assertion, a failed write then a retry.
- The unexpanded matcher of 6.4: several words split across components,
  phrases, prefixes, stems, each field.
- The binding function (D19): it changes with each of its four inputs
  and with nothing else, and it is never 0.
- The sync-boundary predicate (D15), the
  rights and refusals of a placement change (5.2), and the eviction
  choice of the cap (D20).
- D29's versioned state codec: both origins round-trip; malformed,
  overflowing and trailing fields are refused; ordinary `n`/`s`/`r`
  fixtures stay byte-identical. Out-of-scope state slots and expected
  state preconditions keep their distinct errors from 6.8.
- The hand-off: taken once, expired, wrong secret, stale binding.
- D11's alert replacement and the property rulings of 6.5.
- The error mapping of section 8.

### 9.2 The za suite

New modules, placed after the sharing modules and before the sweep and
trace modules. They reserve the fixture families `token-*` and
`jmapcal-*`, all at example.com.

- **`tokens`**: sign-in through `/api/auth` and `/auth/token` gives a
  token that reads sealed data over JMAP and CalDAV; the consent path;
  refresh, a retried refresh returning the same refresh token, and a
  replayed one revoking the grant; a superseded token opening nothing;
  a credential change ending the session and leaving no record, and
  every other vault write leaving both;
  revocation removing the grant's entries; a grant destroyed through
  the registry refused at once and its entries gone after the sweep; a
  refresh whose entry is missing deleting the grant; expiry and the
  sweep; a tampered or foreign secret answering 401; a device-flow
  token staying keyless; a Bearer login to IMAP and POP3 with a keyed
  token still refused; the code exchange with no hand-off answering
  `invalid_grant`. Also: a 33rd sign-in ending the least recently used
  grant (D20); a refresh without the secret of a keyed grant answering
  `invalid_grant` and leaving the grant unrotated (D22); a Bearer
  request crossed by a vault write refused and not served (D23); a
  successful token answer absent from the traces, for a key and an
  ordinary account (D24); the hand-off absent, checked by its exact key,
  after redemption, expiry and a refused consent; the first pin's rewrap
  leaving the session in place (D19). Also: account destruction leaving no record;
  an ordinary account's code with a secret answering `invalid_grant`
  (4.4); a refused token not counted against its address (D27).

  Test hooks (test mode only) pause the code exchange after its last
  version check, and a refresh before its record read and before its
  write. Two more fail once: a refresh's record write, and the removal
  of a grant's entries. With them:
  - a password change, and separately a recovery, committed during a
    paused first issuance makes the issuance fail and leaves no record
    and no grant;
  - a vault write that leaves the binding unchanged, committed during a
    paused first issuance and, separately, during a paused refresh, is
    retried: the issuance and the refresh succeed;
  - a paused refresh crossed by the same vault changes, and a refresh
    that retries a lost record write across one, answers
    `invalid_grant` and leaves no record. Its old tokens are refused.
    The grant object may remain, as any grant of the account does after
    a credential change;
  - a refresh made to fail right after its rotation and before its
    record write, with no client retry, leaves the superseded entry in
    the record; the maintenance sweep removes it, and in a second run a
    write of the record for another grant removes it;
  - a retry of generation `g`, paused while its successor `g+1`
    completes a refresh, answers `invalid_grant`, leaves the grant and
    the successor's tokens usable, and leaves generation `g` absent
    from the record. This runs twice: with the retry paused before it
    reads the record, and with it paused after it opened MK;
  - a refresh whose current entry was removed from the record deletes
    the grant.
- **`jmap_calendar::pipeline`**: the pipeline's paths that no CalDAV
  request reaches, called in process: a create named in several
  calendars under one lease, a placement change with a content change in
  one commit, a placement that reseals an event's dead-property
  container, and the destroy of an event linked into several calendars.
- **`jmap_calendar::events`**: create, update, destroy, placement and
  linking, recurrence edits through synthetic ids and overrides, tasks,
  the property rulings, per-object commits with one failing object, and
  each read path. Every case reads the stored event back through CalDAV
  and the reverse.
- **`jmap_calendar::calendars`**: create, update, destroy with events,
  default, colour before and after sharing (D14), a grantee's own entry.
- **`jmap_calendar::query`**: the filters and sorts, expansion, text
  matching in both forms with AND, OR and NOT, privacy for a grantee.
  One case puts the same events into a key account and an ordinary
  account, among them recurring events whose master and override hold
  different words in the same field. Once the ordinary account's index
  is current it runs the same unexpanded queries against both: single
  words, several words in one condition, quoted phrases (among them a
  phrase in both word orders on a field the index holds without
  positions), and the negation of each. Both accounts must return the same events. For the
  single conditions and the flat ANDs among them, the expanded form
  must still match on the selected occurrence's own component. For the
  negated ones the expanded form must answer `invalidArguments`:
  upstream refuses `NOT` and `OR` when expanding.
- **`jmap_calendar::scheduling`**: invitations, updates, cancellations
  and replies to and from a key account, an outside address and a plain
  local account; `sendSchedulingMessages` false and absent; pins and
  withheld recipients; the mail guard on a failed commit; the drain run
  from a JMAP request; an invitation by email found by UID after the
  drain; no `CalendarItipMessage` task for a key account, ever.
- **`jmap_calendar::sharing`**: a grantee's reads and writes at both
  levels, the delegated reply with `SENT-BY`, `shareWith` refused,
  revoke, the sync boundary (D15), a non-grantee refused. D29's actual
  changes-handler tests reproduce the retention-marker collision: the
  first pages carry distinct origins; after rename the initial-origin
  page is served and the exact-origin page is `cannotCalculateChanges`.
  Cover pending boundaries, more than two pages, trimming between pages,
  insert/delete folding, both calendar object types, unchanged ordinary
  accounts, legacy grantee restart and the legacy owner `r` chain.
  `jmap_calendar::query` repeats the origin/boundary cases through
  `/queryChanges` once that method is served.
- **`jmap_calendar::refusals`**: every row of 6.11, a keyless token on
  each served method, impersonation, and D29's out-of-scope `z` state
  and expected-state precondition errors without bypassing admission.

### 9.3 The differential oracle

`jmap_calendar::oracle` runs the script of 3.5 through both protocols on
fresh key accounts and compares as 3.5 says. Its precondition is that
both runs start from equal accounts with equal pins. It reports the
first step that differs with both projections.

### 9.4 Upstream's JMAP calendar tests in key mode

Key-mode `jmap_tests` skips the calendar modules today. From release 6
it runs upstream's `calendar::calendars`, `instance` and `alarm` bodies
with key accounts wherever the rulings leave their
assertions true, and a variant in `tests/src/jmap/za_variants.rs` where
they do not. Expected variants: `notification` (refused), `acl` (the
grant made through the share endpoints), `availability` (empty), and
the cases that use drafts, per-user data, participant-visibility flags,
default alerts by reference or `CalendarEvent/copy`. Also `identity`
(creates and destroys refused, 6.9), the `event` case that expects
upstream's UID rule (3.3), and in `acl` the grantee's destroy of a
shared calendar (6.7). `instance` runs upstream's body, since 6.5 leaves
occurrence patches to upstream. The plan lists each changed assertion
with the ruling that changes it. The variants take the prefix
`calendar_`.

### 9.5 Leak coverage

The scan gains, for every scanned account:

- after JMAP writes, the rules it applies after CalDAV writes: sealed
  archives only, no calendar search document, no event preferences;
- no `CalendarItipMessage` task;
- the token record holds only what 4.3 lists, and no token secret, code
  or MK appears anywhere in the store, while sessions are live and after
  they end;
- no hand-off entry outlives its code. This is checked by the entry's
  exact key: the scan cannot read the in-memory store.

### 9.6 Tests that change

The refusal assertions for the methods section 6 opens:
`tests/src/za/gating.rs`, `tests/src/za/websocket.rs` (the socket still
refuses, now with the keyless answer), `za_variants::calendars` and the
session pins in `za_variants::principal_get`. Also
`tests/src/za/app_password.rs`, whose sign-in now yields a keyed grant
and leaves a record, and the store-emptiness check of
`tests/src/utils/cleanup.rs`, which exempts the record's field while its
account exists. The plan inventories them.

### 9.7 Mutation sample

At least these faults, each caught by a named test:

- the JMAP create skips the lease; skips sealing; calls
  `ItipMessages::queue`; drops the token pairs on update; drops the
  display name; skips the SEQUENCE floor;
- a text filter reads the stored tree instead of the view; the
  unexpanded matcher reads one component at a time;
- a token-record write omits the vault assertion; a superseded refresh
  generation deletes the grant; a lost record race puts back a pruned
  entry; the sweep keeps a refresh entry below its grant's stored
  generation;
- the drain hook is removed;
- a notification destroy is let through;
- the wrap's associated data omits the grant id; a credential
  change leaves the token record; a superseded refresh entry is kept; a token with an
  unopenable secret is served keyless; the hand-off is not deleted on
  redemption;
- the cap's eviction leaves the evicted grant; a refresh without the
  secret of a keyed grant rotates it; a Bearer request at a stale
  generation is served; a successful token answer is traced;
- the unexpanded matcher's document is built from the stored tree; a
  placement change asks for a right CalDAV's `MOVE` does not.

D21 has no fault in the sample. A removal reads no vault record, so the
fault could only be made by adding a read, and nothing would make that
read stale. The unit tests of 9.1 cover it.

### 9.8 Manual checklist: the reference client

`docs/zero-access/client-jmap.md` records how to serve Bulwark's static
edition from a local server and the checks of section 7, run against a
key account with and without a second factor, with a second key account
as grantee and invitee and an outside address at the sink. It is a
manual gate, as the native-client checklists are.

D29's part of that gate verifies the unmodified client's opaque state
round trip across multiple pages, initial-sync continuation across a
sharing-boundary change, and restart after an incremental
`cannotCalculateChanges`. Record these observations on the tested client
and server versions; codec and handler tests alone do not qualify them.

## 10. Invariants for implementers

57. A key account's calendar data is written only by the pipeline,
    whatever the protocol. No JMAP path calls upstream's event or
    calendar writers, `ItipMessages::queue` or `notify_direct_change`
    for a key account.
58. A JMAP write stores what the CalDAV write of the same tree stores.
    Nothing JMAP-only is stored for a key account except values release
    1 lists as visible by design.
59. A key account's event holds no per-user data. Sealing refuses one
    that does.
60. Text of a key account's event is matched only in process memory,
    over the unsealed view, in a request that holds the keys.
61. A token secret and a hand-off secret exist only in the string the
    client holds. Neither is stored, logged or traced, and no cache
    holds one beyond the key cache's fingerprint of the token.
62. MK leaves process memory only wrapped under a key derived from a
    secret the server does not store: the password, the recovery key,
    an app password, a token secret or a hand-off secret.
63. The token record yields keys only to a token upstream accepted, for
    the grant, kind and generation the entry was made for, under the
    vault record's current binding (D19).
64. A vault write that changes the binding deletes the token record in
    its own batch. Only a credential change does: a password change, a
    recovery or a second-factor change. Every write of the
    token record that stores a wrap asserts, in its own batch, the vault
    record it was made under. No wrap made under one binding is stored
    under another.
65. A token that carries a secret is served with keys or refused.
66. A scheduling inbox item is never listed, opened or destroyed through
    JMAP.
67. A refresh entry below its grant's stored generation is removed by
    the next write of the record or the next sweep. No write adds one.
68. An upstream validation step is neither copied into `groupware` nor
    moved out of its handler. A fork step of a key account's calendar
    write exists once, in the pipeline, and both protocols call it
    (D17).
69. A write of the token record that only removes entries asserts the
    record alone and stores no wrap (D21).
70. The token record never holds more than 32 grants (D20).
71. A key account's refresh reads the token record before the grant
    rotates. A refresh token without a secret never rotates a grant that
    has entries (D22).
72. Keys opened from a token are served only at the account's current
    generation. On a mismatch the request is refused, never served
    uncached (D23). A request whose token expires before its cache
    entry is published is not such a mismatch: it is served with its
    keys and nothing is cached. A keyed cache entry ends before its
    token's signed expiry (4.6).
73. A successful answer of the token endpoint is never traced, for any
    account (D24).
74. Only a Bearer value that begins with the server's token header is
    split at `~`. Any other value reaches upstream unchanged (D26).
75. For a key account a store error on an object of a `/set` is
    answered on that object and every later one. It never fails the
    method once the first object was reached (D28).
76. A page of a sync that started from an initial state is never
    refused at a sharing boundary. Every new key-calendar continuation
    preserves its original start explicitly; incremental pages check
    that original exact state (D29).

Amendments:

- **Release 1 section 9**, the row "JMAP calendars capability and
  methods": superseded by section 6. The row on Bearer and OAuth logins
  on calendar paths: a token with a secret carries keys (4.6).
- **Release 1, a key account's credential version**: it was the vault
  revision, so every vault write ended the account's OAuth sessions
  (kept on 2026-11-02). Superseded by D19: the credential version is the
  binding, and only a credential change ends them (a grant's deletion
  ends that grant's sessions, 4.7).
- **Release 1 invariant 5** stands: `Server::authenticate` returns
  keyless tokens, and keys are attached by the HTTP layer for one
  request. The Bearer arm is a second source beside Basic.
- **Release 1 invariant 8** (the vault record is the sole authority for
  every fact that decides a login) stands; 4.3 says why the token record
  is not an authority.
- **Release 2 section 5.1**, "JMAP callers pass `owner_is_key = false`":
  true of upstream's path, which a key account no longer reaches.
- **Mail spec 1.2 and 3.3, search spec 6.2**: "a Bearer or OAuth token
  is keyless" becomes "a token without a secret is keyless". The table
  of search spec 6.2 is unchanged.
- **Sharing spec 5.7**: superseded by 6.1 and 6.7 for reads and writes;
  `shareWith` stays refused.
- **Upgrade design, "Per-user event data"**: stands, now enforced
  against a reachable path (D10).
- **Search spec decision 1** stands.
- **Invitations spec 6.1 and 6.2**: the drain's trigger gains the scope
  clause of 5.5. A DAV request drains only when its token's scope admits
  calendars, and a JMAP request as 5.5 says.
- **Search spec 5.2**, the JMAP trigger: the pass runs before dispatch
  only when a call that triggers it parsed and its capability is in
  `using` as the token's scope filtered it. A `Blob` call is such a call
  for every scope that carries the blob capability, so a token without
  the mail scope still triggers the pass through it (accepted by the
  controller's ruling on the pull request's review, confirmed by the
  project owner on 2026-11-19).
  The pass after a committed message write is unchanged.
- **Release 5, iMIP spec 5.4**: the recorded residual of a collection
  `DELETE` whose commit fails in a later chunk no longer occurs for a key
  account on RocksDB, SQLite, PostgreSQL or MySQL, where the delete is
  one transaction (5.3). It stays on FoundationDB, a read replica and
  the ephemeral store, by the controller's ruling on the pull request's
  review, confirmed by the project owner on 2026-11-19.
- **The rule that ordinary accounts take upstream's paths unchanged**
  (release 1 section 12) gains three exceptions. All apply to every
  account, ordinary ones included. `decode_access_token` strips a `~`
  suffix from every token that begins with the server's token header
  before it opens it (D26), because the account is not known until the
  token is open. The token endpoint's successful answers are untraced
  (D24). The value of every `code` query parameter is masked in a traced
  request URL and in a traced `Referer` header (`traced_url`), because no
  account is known when the URL is traced; the project owner confirmed
  this amendment on 2026-11-18. Three costs also fall on ordinary
  accounts: one cached account read at a code exchange, at a refresh and
  at a grant's deletion; one
  in-memory delete at a consent confirmation that carries no secret; one
  cached classification read in each calendar method.
  The pull request's review adds these: one boolean test per
  `commit_point()` call of every batch (the one-transaction flag of
  5.3); a deleted grant's sessions are evicted twice on the deleting
  node, in place and then through the queued invalidation, and on a
  cluster broadcast twice, so the global revocation epoch advances at two
  instants and an authentication of any account in flight at either one
  is served but not cached, and a revocation of N grants makes N + 1
  cache scans and broadcasts where upstream made one of each, with each
  deletion on a cluster awaiting its broadcast before its cleanup; one
  more cached account read where a deletion's re-read finds the grant
  already gone; and, per `OAuthGrant` invalidation, the account's keys
  removed after the epoch advances (the same work, later). The drain's
  scope rule and the search pass's condition are reached only with keys,
  and the grantee's event state, the effective privacy, the delete's
  fence and size refusal and the guarded raise of 6.8 are on key-account
  paths. A key account's grantee (not an ordinary account) pays one
  content read and one unseal on a destroy; one more log read per event
  poll when a container change is the last record of the calendar log;
  and one log read on the last page of an exact-origin paged event sync.
  The pull request's second review adds, for an ordinary account, a few
  boolean tests per listed child of a calendar collection delete, with
  no read and no write; the member list, the unwritten-child refusal, the
  every-name removal, the size check's reserve and the keyed entry's
  lowering are on key-account paths, and the keyed `COPY`'s condition is
  in the fork's keyed handler. For a key account a calendar delete reads
  the members with in-memory passes over the account's event records;
  the passes read no store, though each of the two comparisons first
  reads the calendar change counter and takes a snapshot at that tip.
  There are three passes on a fenced store (the build and the two
  comparisons), for a CalDAV collection `DELETE` and for the owner's
  JMAP destroy with `onDestroyRemoveEvents`; the owner's JMAP destroy
  without the flag first checks for a member, stopping at the first, so
  it is refused after at most one pass when the calendar holds an event
  and makes four passes when it holds none; on the stores that keep the
  chunked form one (the build), or two for that destroy without the
  flag. The second review's keyed-entry bound adds one more token open
  per keyed cache miss; a keyed entry
  ends up to two seconds before its token, and with one whole second
  left nothing is
  published, so a keyed request in that window is verified in full and
  charged to its address's anonymous rate limit where one is configured
  (a client that refreshes before `expires_in` never reaches it).
  D29 adds state-codec fields and context checks, with no global account
  lookup or additional ordinary-account store I/O. Key-calendar pages
  carry the extra origin fields and classify the account even for an
  owner's or initial request when choosing the continuation format;
  reuse the calendar admission classification where available. There is
  no per-page log read to infer the origin.

## 11. Merge hotspots and silent dependencies

New fork insertions in upstream files:

- `crates/http/src/auth/oauth/auth.rs`: `authenticate_login` takes keys;
  the hand-off at `issue_auth_code`, `request_consent` and
  `confirm_consent`; `approve_device` drops the keys.
- `crates/http/src/auth/oauth/token.rs`: the code exchange, the refresh
  grant, and `token_response`, which marks a granted answer untraced.
- `crates/common/src/auth/oauth/token.rs`: the suffix strip in
  `decode_access_token`. `crates/common/src/auth/oauth/grant.rs`: the
  entry removal in `delete_oauth_grant`.
- `crates/http/src/auth/authenticate.rs`: the Bearer arm's keys and its
  refusal at a stale generation (D23). An existing hotspot.
  `crates/common/src/auth/authentication.rs` is not touched: the lookup
  stays out of `authenticate_with_keys` (4.6).
- `crates/common/src/auth/vault.rs`: the record's deletion in
  `vault_batch_write`. `crates/common/src/auth/access_token.rs`: a key
  account's credential version through `za_session_binding` (D19).
  `crates/types/src/field.rs`: field 153.
- `crates/services/src/task_manager/tasks/maintenance.rs`: the sweep.
- `crates/jmap/src/api/request.rs` (admission, the drain hook),
  `crates/jmap/src/api/session.rs`, `crates/jmap/src/principal/get.rs`,
  `crates/jmap/src/changes/get.rs` and `changes/query.rs`; one branch each at the head of the
  two `/set` handlers in `crates/jmap/src/calendar/` and
  `crates/jmap/src/calendar_event/`, whose key writers are child modules
  that reuse upstream's private helpers; replacement points in the two
  `/get` handlers; two points in `calendar_event/query.rs`; and a
  refusal in `crates/jmap/src/participant_identity/set.rs`.
- `crates/dav/src/calendar/` and `crates/dav/src/common/za.rs` shrink:
  their key branches become calls into `groupware`.
  `crates/dav/Cargo.toml` and `crates/jmap/Cargo.toml` enable
  `groupware`'s test hooks from their own `test_mode` feature.
- D29: `crates/jmap-proto/src/types/state.rs` (the `z` codec and
  `ExpectedState` handling), `request/parser.rs` (method context),
  `crates/jmap/src/changes/{get,query,za}.rs` and
  `participant_identity/changes.rs` (state consumers and rejection).
  The small insertion rule permits these state-enum arms and the
  calendar continuation adapter. `changes/page.rs` remains upstream's
  range/count algorithm; its output is wrapped only for key calendars.

Silent dependencies:

- The refresh steps follow `StoredOAuthGrant::rotation`: two accepted
  generations, a deterministic refresh token on retry, and deletion on
  replay. A change to that state machine needs a re-read of 4.5.
- The suffix relies on upstream's token alphabet excluding `~` and on
  the code and consent token being 40 alphanumeric characters used as
  the store key.
- The hand-off relies on the password step issuing the code or the
  consent token in the same request that verified the password.
- The JMAP adapter relies on upstream's `/set` handlers producing the
  final tree before any write, and on `export_icalendar` being the only
  conversion. A change that moves a write ahead of the conversion, or
  adds a second writer, needs a re-read of 3.1.
- For a key account no `preferences_mut` call on an event may be
  reached. Upstream's patcher makes one for `useDefaultAlerts`, which is
  why 3.1 takes that property out first; a property upstream later
  stores per user must be classified the same way. The key branch skips
  the per-user split and never reaches the `resolve(.., false)` sites.
- 3.1's table follows the order of upstream's `/set` handlers at the
  base. A step upstream adds, moves or merges must be given an owner.
- D12's expanded form relies on `InstanceTextFilter` covering every text
  condition of `CalendarEventFilter`. A condition upstream adds must be
  classified. Its unexpanded form relies on `index_document`
  (`crates/groupware/src/calendar/index.rs`) being the only builder of a
  calendar search document, and on release 7's content part
  (`crates/store/src/za_search/content.rs`) deriving terms with the
  store's indexer and resolving text conditions with the store's planner
  (`crates/store/src/search/leaf.rs`). A change to release 7's part, or
  an upstream change that builds the document elsewhere, needs a
  re-read; the two-account query case of 9.2 is the guard.
- 4.5 relies on upstream's retry case returning without a registry
  write and on no lock being held from the rotation to the fork's
  record write. The classification in step 2 exists for that gap.
- The reconciliation of 4.3 reads `OAuthGrant.generation` as upstream's
  rotation means it: the last generation a client proved it received,
  below which every presentation is a replay. A change to that meaning
  needs a re-read of 4.3, 4.5 and the sweep.
- 4.6 relies on the HTTP layer being the only caller that reads the
  token record. `authenticate_with_keys` is shared with the IMAP and
  POP3 logins.
- D14 relies on the personal bundle's rkyv layout keeping its colour
  field.
- 1.3's facts about the reference client are facts about one release of
  it.
- The key writer of `CalendarEvent/set` repeats the head and the update
  grouping of upstream's handler and calls upstream's checks in
  upstream's order (D17). A check upstream adds must be given a place.
- The pipeline decides whether a scheduling error fails a write with a
  rule that follows upstream's `failed_precondition`
  (`crates/dav/src/calendar/mod.rs`). It names every error, so a new one
  stops the build.
- D19 relies on every vault write passing through `vault_batch_write`,
  and on a key account's credential version being computed in one
  place. It also relies on the four inputs of the binding being the only
  facts of the vault record that decide a login: a fact added to that
  set must join the fingerprint.
- D26 relies on upstream's token header (`TOKEN_HEADER`,
  `crates/common/src/auth/oauth/token.rs`) opening every token the
  server issues and no token of another issuer.
- D29 relies on preserving the original origin while upstream's range
  and count advance (`crates/jmap/src/changes/page.rs`), on `/queryChanges`
  forwarding the state unchanged, and on every `State` consumer keeping
  `z` within its key-calendar scope. Changes to the codec, request-slot
  parsing, expected-state checks, early returns or pagination require
  the collision, legacy-state and context-error cases of 9.1 and 9.2.
- The query branch of 6.4 relies on no key account's event carrying
  the hide-attendees flag, which 6.5 refuses. Upstream's check for such
  events sends an attendee condition to the search store.
- D11's email recipient repeats upstream's private rule for a default
  alert's recipient (`crates/groupware/src/calendar/alerts.rs`).
- The key writer of `Calendar/set` repeats the head of upstream's
  handler, as the writer of `CalendarEvent/set` does.

## 12. Decisions, and what Jay should check

Jay's decisions:

1. Release 6 delivers server support for a web client and sets none up
   (2026-11-13).
2. Bulwark is the reference client (2026-11-13).
3. Sharing stays on the account page; a client's share dialog gets an
   error (2026-11-13).
4. A key account's OAuth sessions end only when a credential changes: a
   password change, a recovery or a second-factor change (2026-11-14;
   D19, 4.7). This replaces the decision of 2026-11-02 that every vault
   write ends them. The reason is that the first pin's rewrap is
   automatic, so under the old rule a person's first invitation to
   another key account signed their browser out.
5. The push connection never holds keys (search spec decision 1).

Calls of this spec that change what a user or an operator sees:

- **D7**: a keyed token whose entry is gone is refused, not served
  keyless.
- **D9**: the device-code flow and the mail protocols' bearer logins stay
  keyless.
- **D11**: default alerts are copied into the event when it is saved.
- **D14**: a shared calendar's colour is carried in the sealed personal
  bundle. This is the one change to release 4's sharing code.
- **D16**: destroying a calendar through JMAP sends cancellations.
- **D8**: the maintenance sweep of expired token entries. A grant
  destroyed from an "authorized apps" screen is refused at once, but its
  wrap stays at rest until the next sweep. The alternative is a hook in
  upstream's registry writer. The same sweep bounds a superseded entry
  left behind when a refresh is interrupted between its two writes.
- **4.5**: a refresh whose key entry is lost ends the grant, so the
  person signs in again. A refresh that only lost a race to a newer one
  is refused without ending the grant.
- **D12**: text search of a key account's events matches whole words
  and their stems, as an ordinary account's does.
- **D18**: a JMAP write that asks for scheduling the account may not do
  is refused, as upstream's JMAP refuses it; the same write through
  CalDAV is saved unsent.
- **D20**: at most 32 browser sign-ins of an account hold keys at once.
  The 33rd ends the one used least recently.
- **D22**: a refresh token of a keyed grant is refused without its
  secret.
- **D23**: a request whose token was crossed by a vault write gets 401,
  where a password login would be served.
- **D24**: the token endpoint's successful answers leave the traces, for
  every account.
- **D25**: a keyless calendar method names calendars in its error.
- **D26**: a Bearer value of another issuer is never split, whatever it
  holds.
- **D27**: a token the fork refuses is not counted against the client's
  address.
- **D28**: a store error during a `/set` is reported on the objects it
  stopped, not as a failed method.
- **D29**: a paged first sync of a shared calendar is never refused
  at a sharing boundary. The explicit-origin `z` continuation, legacy
  handling and context errors of 6.8 were approved by Jay, recorded
  2026-11-15; the earlier origin-inference candidate is superseded.
- **6.7**: a grantee at the `view` level cannot rename or recolour its
  own entry of a shared calendar from a JMAP client.
- **D11**: the `/set` answer does not carry the alerts the server wrote.
- **6.4**: a text search without a time range holds the terms of every
  event of the account in memory for the request.
- **3.4**: JMAP does not check a calendar's supported component set.
- **4.7**: under decision 4, changing the password or the second factor
  signs every browser out. Revoking an app password, issuing a new
  recovery key or pinning a contact for the first time does not.

D17 and D21 change nothing a user or an operator sees.

Operator note: how long a browser stays signed in is upstream's
refresh-token lifetime, thirty days without use by default. The release
adds no separate setting for key accounts.

Follow-up outside this repository: the account page's text about how
long keys stay in memory describes password clients only and needs a
sentence about browser sign-in.

## 13. Premises for the plan's preflight

The plan's preflight of 2026-11-14 checked these against the code:

- The seam between upstream's JMAP calendar handlers and the CalDAV key
  branches. The key branches are threaded through the CalDAV handlers
  and are not separate functions (5.1, D17).
- The order of upstream's `/set` steps that 3.1's table assigns, for
  create, update and destroy. It holds.
- `InstanceTextFilter` does not match phrases and prefixes as the index
  does, so the unexpanded form uses the index's own code (D12).
- `blob_download` already reads through the unsealing reader (6.6).
- Upstream's in-memory store entries of the code and the consent token
  share one expiry, on every supported store (4.4).
- The callers of `delete_oauth_grant` are those 4.7 lists. Grants are
  also deleted outside it by the registry's destroy and by account
  destruction.
- The maintenance task finds token records through the registry's
  account list (D8).

Still to check, against a running instance: the reference client's
behaviour in 1.3.
