# Zero-access calendar: JMAP calendars and browser sign-in (release 6) design spec

Date: 2026-11-13. Status: revision 2, for approval. Revision 2 answers
Jay's codex review of revision 1
(`2026-11-13-zero-access-jmap-calendars-design-codex-review.md`): the vault
assertion on every write of the token record (4.3 to 4.5, 4.7, 9, 10;
finding 1), how a refresh reads the record against the grant's progress
(4.5, 9; finding 2), and the event-level matcher of unexpanded text
queries (6.4, 9.2, 11; finding 3). Scope: release 6 of the
zero-access fork (jaylaney/stalwart), GitHub issue #22, milestone "Release 6:
JMAP calendars and web client", item 4 of release 1 spec section 1.2. Base:
the fork on upstream Stalwart v1.0.3 with release 7 merged (PR #84; code
references are to its head `9b1df344b`).

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
marked **D1** to **D16**, each with its reason; section 12 lists the ones
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
- **Pending invitations.** A keyed JMAP request that holds a calendar
  method runs the drain (5.5).
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
its token, after the token expires or its grant is destroyed through the
registry, by at most one maintenance interval (4.7).

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

The work of one JMAP event write for a key account, in order, with the
owner of each step. Line references are to
`crates/jmap/src/calendar_event/set.rs` at the base, for orientation; the
plan's preflight fixes them.

| Step | Owner |
|---|---|
| Parsing, the method's access check, `ifInState` | upstream, unchanged |
| Admission (6.1). For an update, reading the stored event and building the unsealed view the patch applies to | fork |
| Taking the properties 6.5 rules on out of the incoming object | fork. Upstream's patcher stores `useDefaultAlerts` with `preferences_mut`, so it must not see that property. |
| Patch application, synthetic recurrence ids, JSCalendar validation, calendar-count checks | upstream, unchanged |
| Conversion to iCalendar (`export_icalendar`), and a generated UID when a create has none | upstream, unchanged |
| The per-user split (update `:820-887`, create `:1761-1779`) | skipped (D10) |
| Organizer assignment, the SEQUENCE increment and the `updated` stamps, the participants limit, the size cap | upstream's functions, called by the key branch on the converted tree. They are JMAP's shaping of the tree and have no CalDAV counterpart. |
| The scheduling status: `ItipSendStatus::resolve` with the owner as a key account when `sendSchedulingMessages` is true, otherwise `NotRequested`. A denied status answers `forbidden`, as upstream's JMAP does. | key branch |
| Everything else: the calendars' access rules through the contexts; the checks that compare the new tree with the stored one (UID unchanged, the changes a grantee may make, supported components); UID uniqueness under the lease; quota; the scheduling builders and the second run; the recipient cap; statuses for unreachable and withheld recipients; the SEQUENCE floor; the fan-out; sealing; the commit with its fences and its one retry; the mail submission | the pipeline |
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
| Whether to schedule | always, except a `DELETE` with `Schedule-Reply: F` | `sendSchedulingMessages`, which defaults to false (D4) |
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
- **An unexpanded text query is matched in memory**, with the tokenizer
  the search index uses (D12).
- **UID uniqueness is CalDAV's rule**: one UID per calendar, checked under
  the UID lease against current archives.
- **Destroying a calendar sends cancellations** for its events, as a
  CalDAV collection `DELETE` does (D16).
- **No drafts, no per-user event data, no participant-visibility flags**
  (6.5).
- **Default alerts are applied by value** (D11).

### 3.4 Inherited limitations, accepted

- A calendar colour set by a CalDAV client is a dead property; JMAP's
  `color` is a calendar preference. Upstream keeps the two apart for every
  account, so a colour chosen in one kind of client does not show in the
  other.
- An event written with `sendSchedulingMessages: false` and later edited
  through CalDAV is scheduled from that edit on, as upstream does for an
  ordinary account.
- Upstream's JSCalendar conversion decides what survives a round trip
  between the two protocols.

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

From a secret the fork derives, with HKDF and fixed labels under
`za/v1/`: a wrapping key, an 8-byte tag that identifies the secret
without revealing it, and a key that seals the next refresh secret. The
wrap is the primitive the app-password wrap uses (`wrap_key`,
`crates/vault`). Its associated data binds the account id, the grant id,
the kind (access or refresh) and, for a refresh wrap, the generation.

### 4.3 The token record

`PrincipalField::ZeroAccessTokens = 153`, on the account's principal
document, an rkyv record the fork owns, absent when the account holds no
keyed grant. It holds:

- a format version and the vault revision its entries were made under;
- per grant, the grant id and:
  - at most two **refresh entries**: generation, the MK wrap, the next
    generation's secret sealed under this generation's secret (once
    issued), and the expiry of the refresh token;
  - at most four **access entries**: the secret's tag, the MK wrap and
    the access token's expiry.

Every write is conditional on two things in one batch: the record's
previous bytes, and the vault record's stored bytes as the writer read
them, at the revision under which it obtained MK. `vault_commit_store`
makes the same kind of assertion for its own companion records
(`crates/common/src/auth/vault.rs`). A writer whose vault assertion fails
stops. It never stores its wraps under a newer revision. A vault write
and a token-record write therefore order strictly: either the vault
write's batch deletes what the other wrote, or the other's assertion
fails.

Every write also drops entries that have expired, grants that hold no
live entry and grants that no longer exist.

The record is not an authority for login. Upstream decides whether a
token is valid, bound to the vault revision as today (release 1; Jay kept
this binding on 2026-11-02). The record only yields keys to a token
upstream already accepted.

### 4.4 Sign-in: from the password to the grant

The authorization-code flow has two requests: the password step
(`POST /api/auth`, `authorize_code` in
`crates/http/src/auth/oauth/auth.rs`, which the server's own sign-in page
and the reference client both call) and the code exchange
(`POST /auth/token`). MK is available only in the first. The grant exists
only after the second.

**D6. The hand-off.** When the password step succeeds for a key account,
`authenticate_login` takes MK from `authenticate_with_keys` and writes a
hand-off entry: MK wrapped under a key derived from a fresh 32-byte
secret, with the vault revision, stored in the in-memory store under a
fork prefix and the 40-character code, with the code's expiry. The secret
is appended to the code the client receives, so the first 40 characters
are upstream's code and the rest is the secret. The code exchange splits
them, runs upstream's checks (client, redirect URI, PKCE, single use), takes
and deletes the hand-off entry, opens MK, and after `issue_new_grant` has
created the grant writes the grant's first refresh entry (generation 0)
and access entry, each under a fresh token secret. That write carries
the vault assertion of 4.3: the exchange reads the vault record,
requires its revision to equal the hand-off's, and asserts those bytes.

The entry is stored rather than kept in process memory so that the two
requests may land on different nodes of a cluster.

Three paths reach the code:

- **Consent given or not needed.** The code is issued in the password
  request. The reference client sends `consent: true`.
- **Consent pending.** `request_consent` returns a consent token and the
  password is gone when `confirm_consent` runs. The hand-off is written
  under the consent token, its secret appended to that token;
  `confirm_consent` strips it, takes the entry and writes a new one under
  the code.
- **A grant already exists for the client and scopes.** The code is
  issued in the password request.

Rules:

- For a key account, the authorization-code flow yields a keyed grant or
  none. A code exchange that finds no hand-off entry, or one that does
  not open, or one made under a vault revision that is no longer
  current, answers `invalid_grant`.
- If the record write fails after the grant was created, the grant is
  deleted. A failed vault assertion answers `invalid_grant`: the
  password or another vault fact changed while the sign-in was in
  flight. Any other failure answers a server error. Either way no wrap
  is stored and the client signs in again.
- A consent that is refused deletes the entry.

### 4.5 Refresh

Upstream accepts two refresh generations at any time: the last one a
client proved it received (the stored generation) and the one issued
after it. Presenting the newer one rotates. Presenting the older one
while a rotation is pending is a retry and returns the same next token.
Presenting anything older revokes the grant (`rotate_oauth_grant`,
`crates/common/src/auth/oauth/grant.rs`).

The fork follows that state machine without changing it:

1. The refresh token is validated as upstream does, and
   `rotate_oauth_grant` runs before the fork decides anything. A
   superseded token therefore revokes the grant exactly as upstream
   revokes it, and the grant's entries are removed with it.
2. If the rotation issues, the fork reads the record and classifies the
   presented generation against the grant's refresh entries. The grant's
   rotation and the record's write are separate operations, so the
   record can have moved on since upstream accepted this request.
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
   returns the same secret. Otherwise it draws a new one, writes its
   refresh entry and seals it into the presented entry.
4. It draws a new access secret and writes its entry. Upstream's access
   token is a new string on every response, so a retry adds an entry.
5. The record keeps the presented generation and the next one and drops
   lower ones. The write carries the two assertions of 4.3.
   - If the vault assertion fails, the fork stops and answers
     `invalid_grant`.
   - If the record assertion fails, the fork rereads the record and
     classifies again from step 2, with the record as it now is. It
     writes only if the presented generation's entry is still in that
     record. MK held in memory from the earlier read is never used to
     put back an entry the record no longer holds.
6. If the write cannot be made for another reason, the answer is a
   server error. The grant has rotated, so the client's retry is
   upstream's retry case and step 3 finds or creates the same entry.

A superseded refresh token therefore opens nothing: its entry and the
link that led from it are gone once its successor is used, and no later
write restores them.

The second case of step 2 is more lenient than upstream is to the same
request arriving a moment later, which upstream calls a replay and
answers by revoking the grant.

A token of a key account that carries no secret belongs to a keyless
grant (4.8) and refreshes as upstream does, with no record access.

### 4.6 Use

Keys are still attached in one place, `authenticate_uncached`
(`crates/http/src/auth/authenticate.rs`). The key cache is keyed by a
fingerprint of the `Authorization` header, so a Bearer header is cached
as a Basic one is, under the same idle and age limits and the same
generation fence.

On a cache miss with a Bearer token of a key account that carries a
secret, the HTTP layer reads the record, finds the access entry by the
secret's tag, and opens MK. The session keys' generation is the record's
vault revision, which must equal the account's current one. A miss costs
one record read, one HKDF and one AEAD open; no Argon2 runs.

The lookup lives in the HTTP layer and not in `authenticate_with_keys`.
The IMAP and POP3 logins call that function too
(`crates/imap/src/op/authenticate.rs`, `crates/pop3/src/op/authenticate.rs`)
and keep whatever keys it returns for the life of the connection (4.8).

**D7.** A token that carries a secret whose entry is missing, expired or
does not open is refused with 401, as an invalid token is. It is never
served as a keyless token. A client that refreshes on 401 then gets a
new access token with a new entry, where a keyless answer would leave it
holding a token that reads nothing.

A token without a secret is a keyless token, as today: an API key, a
device-flow token, a master-user token. Search spec 6.2's table applies
to it unchanged.

The keys work on every HTTP path. A CalDAV request with a keyed Bearer
token is a keyed CalDAV request.

### 4.7 Ends

- **A vault write.** `vault_commit_store` deletes the token record in the
  batch that writes the vault record. Every password change, recovery,
  app-password creation or revocation, second-factor change, and the
  self-check's repair therefore ends every browser session of the account
  and destroys its wraps in one commit. Upstream refuses the tokens by
  credential version, as before. A sign-in or a refresh that obtained MK
  before the vault write cannot store a wrap after it, because its vault
  assertion fails (4.3).
- **A grant deletion.** `delete_oauth_grant` removes the grant's entries.
  Upstream calls it on revocation (`/auth/revoke`), a replayed refresh
  token, a client mismatch and when it prunes idle or excess grants.
  Upstream's end-session endpoint deletes no grant. A grant can also be
  destroyed through the registry (`x:OAuthGrant/set`), which does not
  pass through that function. Upstream refuses such a grant's tokens at
  once. Its entries are removed by the next write of the record or the
  next sweep, whichever comes first: both drop every grant that no
  longer exists.
- **Expiry.** An expired entry is refused at use, removed by the next
  write of the record, and removed by a sweep in the maintenance task's
  purge step, beside the lease sweep (D8). The sweep needs no key.
- **Account destruction.** The record goes with the principal's
  properties.

**D8.** The sweep removes expired entries and the entries of grants that
no longer exist. With it, an expired token, or the token of a grant
destroyed through the registry, opens nothing after the next maintenance
run. Without it an idle account's entries would stay until its next
sign-in or refresh. The fork adds no hook to the registry's writer for
the destroy case: the sweep bounds it, and the hook would be a new
insertion in upstream's registry code.

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
(`jmap` does not depend on `dav`). What moves:

- the admission and role helpers (`za_calendar_gate`, `za_writer_keys`,
  `za_roles` and `ZaRoles`);
- the context wrappers (`za_write_contexts`, `za_mutation_context`, the
  read contexts and views, `za_refresh_event`);
- the create flow under the UID lease (`za_take_uid_lease`,
  `za_assert_is_unique_uid`, `za_create_event`, `za_commit_create`,
  `za_lost_create_fence`);
- the scheduling assembly (`za_schedule` and the second run);
- the key branches of event update, event and collection delete, copy,
  move and rename, calendar creation and calendar property change, with
  their bounded retries.

The moved functions return a protocol-neutral error. `dav` maps it to the
`DavError` values it produces today; `jmap` maps it as section 8 says.
The CalDAV handlers keep URI parsing, header preconditions and response
bodies.

The move changes no behaviour. Its proof is that the existing CalDAV
tests pass without edits to their assertions: the za suite's CalDAV
modules, key-mode `webdav_tests` and the independent CalDAV client job.
The move is the plan's first task and is reviewed on its own, before any
JMAP code calls the result.

The groupware primitives the orchestration calls are already public
(sealing, `ZaFanOut`, `ZaDrain`, `ZaImipPending`, the sharing resolvers,
`delete_za_at`, `delete_with_events_za`) and do not move.

### 5.2 Operations

| Operation | CalDAV caller | JMAP caller |
|---|---|---|
| Create an event | `PUT` to a new path | `CalendarEvent/set` create |
| Update an event | `PUT` to an existing path | `CalendarEvent/set` update |
| Change an event's placement | `COPY`, `MOVE`, `DELETE` of one name | `CalendarEvent/set` update of `calendarIds` |
| Delete an event | `DELETE` | `CalendarEvent/set` destroy |
| Create a calendar | `MKCOL` | `Calendar/set` create |
| Change a calendar | `PROPPATCH`, rename | `Calendar/set` update |
| Delete a calendar | collection `DELETE` | `Calendar/set` destroy |
| Read an event or a calendar | `GET`, `PROPFIND`, `REPORT` | the `/get` and `/query` methods |

Placement follows the CalDAV rules. An event may be in several calendars
only when all are unshared calendars of its owner. A shared calendar is
an event's only calendar. Moving an event between an unshared and a
shared calendar reseals it in the target domain and reseals its
dead-property container, in one commit. What `COPY` and `MOVE` refuse,
the JMAP update refuses.

A JMAP update may change placement and content together. The pipeline
runs it as one update: it resolves the contexts of the old and new
calendars, seals the new tree in the target domain, and commits once. A
JMAP create may name several calendars under the same rule.

The commit asserts the context of every old and every new calendar. A
create checks UID uniqueness under the lease in every calendar it names.
A placement change checks each destination as CalDAV's `COPY` and `MOVE`
check it. The CalDAV helpers serve one calendar per request, so the
pipeline's entry points take the full set.

### 5.3 One commit per object

**D3.** For a key account, a `/set` call runs the pipeline once per
object, in upstream's order (creates, updates, destroys), each with its
own commit. The state check (`ifInState`) runs first; `newState` is read
after the last object.

Upstream builds one batch for the call. The pipeline cannot: a create
commits its document id before its fan-out (key pinning), holds a UID
lease, delivers items to recipients before its own commit and retries
once after rewrapping. One object's failure must not strand another's
delivered invitations. JMAP defines each object of a `/set` as its own
atomic unit, so per-object commits conform.

### 5.4 Scheduling

**D4.** `sendSchedulingMessages` is honoured as JMAP defines it. When it
is false or absent the pipeline runs with scheduling off: no fan-out, no
mail, statuses untouched. When it is true the status is
`ItipSendStatus::resolve` with the owner as a key account, which gives
`SendSealed` or one of upstream's reasons not to send.

With scheduling on or off, an update carries the stored token pairs and
the stored display name into the resealed event, and applies the SEQUENCE
floor after upstream's own increment.

`ItipMessages::queue` is never called for a key account. The three
`resolve(.., false)` call sites in `crates/jmap/src/calendar_event/set.rs`
stay as they are: the key branch leaves upstream's path before them.

### 5.5 The drain

A JMAP request runs `za_drain` once, before dispatch, when its token
holds its own account's keys and the request holds a method of
`Calendar`, `CalendarEvent` or `ParticipantIdentity`. It never fails the
request. The hook sits beside release 7's search pass hook in
`handle_jmap_request` (`crates/jmap/src/api/request.rs`).

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
- **Anyone else**, and any keyless token, gets the method error
  `forbidden` with the fixed keyless description release 7 uses for mail
  (D13).

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

### 6.4 `CalendarEvent/query`

- `inCalendar`, `uid`, `after`, `before`, every sort and
  `expandRecurrences` read the cache and the stored tree's visible
  fields, as upstream does. Start, end, recurrence and UID are visible by
  structure.
- **D12.** `text`, `title`, `description`, `location`, `owner` and
  `participant` are matched in memory over the unsealed view of each
  candidate. The matcher is the one upstream already uses in memory for
  expanded queries (`InstanceTextFilter`,
  `crates/groupware/src/calendar/instance_filter.rs`), which tokenizes
  as the search index does.
  - **Expanded form.** Upstream's code, given the view. Every condition
    must match in the instance's own component, and the filter is a
    single condition or a flat AND, as upstream requires.
  - **Unexpanded form.** The event is matched as the one document the
    index would hold for it. For each field, the matcher takes the
    values of every scheduling component of the event, in the order
    `build_calendar_document` indexes them
    (`crates/groupware/src/calendar/index.rs`), joined as the index
    joins successive values of a field, and tokenizes the result. A
    condition is evaluated against those event-level tokens. A
    condition of several words therefore matches when one word is in
    the master and another in an override, and a phrase matches
    wherever the index would find it. The conditions' results are
    combined by the filter's AND, OR and NOT, as upstream combines the
    index's results.

  A key account has no calendar search documents, so nothing is indexed.
  A text query without a time range opens every event of the account.
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

Destroying an event runs the pipeline's delete with the event's domain,
so cancellations and declines go through the fan-out. Destroying one
occurrence through its synthetic id is upstream's conversion to an
update.

### 6.6 `CalendarEvent/parse`

Served when the token holds the keys of the account named in the call. A
blob of a key account is a sealed upload or a part of a sealed message;
the handler reads it through the unsealing reader with those keys and
never raw, by the mail spec's rule for sealed sources. The method writes
nothing.

### 6.7 `Calendar/get` and `Calendar/set`

`Calendar/get` reads each calendar through `calendar_view_in_domain` for
the caller, so the name and description are the caller's sealed personal
values. `shareWith` and `myRights` come from the stored grants, which are
visible by design.

`Calendar/set`:

- **Create** seals the name, description, colour and a custom time zone
  under the owner's keys (`seal_calendar`). Sort order, the visibility
  and subscription flags, default-alert offsets and an IANA time zone id
  stay in the clear, as release 1 decided.
- **Update** edits the caller's own entry through the calendar's context
  and reseals it (`seal_calendar_changes`). A grantee changes only its
  own entry. The handler never adds a preference entry: a grantee's
  entry exists from the grant.
- **Destroy** runs the pipeline's collection delete (D16). Upstream's
  refusal of a non-empty calendar without `onDestroyRemoveEvents` stays.
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
reads as no colour. Without this the owner of a shared calendar loses
its colour when sharing and gets an error on setting one from a JMAP
client.

**D16.** Destroying a calendar sends cancellations and declines for its
events, as the CalDAV collection `DELETE` does. Upstream's JMAP sends
none; that is recorded in 3.3.

### 6.8 `/changes` and `/queryChanges`

The change log is keyless and is served as upstream serves it, with
upstream's filtering for a grantee.

**D15.** A `sinceState` older than the sync boundary of a shared calendar
the caller can see (the later of its grant and its last rename), or any
state while such a boundary is pending, answers
`cannotCalculateChanges`. This is the JMAP form of the 403
`valid-sync-token` that CalDAV's `sync-collection` gives (sharing spec
5.2). One predicate decides both, for the same callers and the same
calendars. A JMAP state and a CalDAV sync token are the same change id,
so the comparison is the same.

### 6.9 `ParticipantIdentity`

`/get` and `/changes` are served. The identities are the account's own
addresses, which the registry already holds in the clear.

`/set` serves `onSuccessSetIsDefault`. A create, update or destroy is
refused with `forbidden`: an identity's name is free text that would be
stored in the clear, and the reference client only switches the default.

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
not served.

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
| Missing access, a context that refuses, a placement CalDAV refuses | set error `forbidden` |
| The object or its calendar is gone | set error `notFound` |
| UID already used in the calendar | set error `invalidProperties` on `uid` |
| A property ruling of 6.5 or 6.7 | set error `invalidProperties` naming the property |
| `sendSchedulingMessages` is true and the account may not send | set error `forbidden` with upstream's reason, before the pipeline runs |
| More recipients than the cap, or a scheduling error CalDAV refuses the write for | set error `invalidProperties` on `participants`, with upstream's description |
| The UID lease is busy, a create lost its fence, or a commit failed its assertion after the retry | set error `forbidden` with upstream's "modified by another process, try again" description, as upstream's `reject_uncommitted` gives |
| Quota | upstream's `overQuota` |
| A keyless token, a non-grantee | method error `forbidden` (6.1) |
| A store error | the method fails, as upstream |

An object that fails does not undo the objects before it (D3).

Traces of the new paths carry account, grant and document ids, counts
and fixed reasons. They never carry a token, a token secret, a code, a
tag, event content or an address. The JMAP request body of a key account
is already read untraced. A refused or failed token open is traced as an
authentication failure with a fixed reason. The mail guard's traces
(`ZA_IMIP_DROPPED`, `ZA_IMIP_ABANDONED`) apply to JMAP writes as they do
to CalDAV ones.

## 9. Testing

### 9.1 Unit

- The token split, the derivations and the wrap's associated data: an
  entry does not open under another account, grant, kind or generation.
- The record codec, its pruning and its caps.
- The refresh steps of 4.5 against upstream's rotation outcomes: rotate,
  retry, replay, the three classifications of step 2, a lost race on the
  record that finds the presented generation superseded, a failed vault
  assertion, a failed write then a retry.
- The unexpanded matcher against the index's document for the same
  event: several words split across components, phrases, each field.
- The hand-off: taken once, expired, wrong secret, stale vault revision.
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
  every vault write ending the session and leaving no record;
  revocation removing the grant's entries; a grant destroyed through
  the registry refused at once and its entries gone after the sweep; a
  refresh whose entry is missing deleting the grant; expiry and the
  sweep; a tampered or foreign secret answering 401; a device-flow
  token staying keyless; a Bearer login to IMAP and POP3 with a keyed
  token still refused; the code exchange with no hand-off answering
  `invalid_grant`.

  Test hooks (test mode only) pause the code exchange after its last
  version check, and a refresh before its record read and before its
  write. With them:
  - a password change, and separately a recovery, committed during a
    paused first issuance makes the issuance fail and leaves no record
    and no grant;
  - the same for a paused refresh, and for a refresh that retries a
    lost record write across the vault change;
  - a retry of generation `g`, paused while its successor `g+1`
    completes a refresh, answers `invalid_grant`, leaves the grant and
    the successor's tokens usable, and leaves generation `g` absent
    from the record. This runs twice: with the retry paused before it
    reads the record, and with it paused after it opened MK;
  - a refresh whose current entry was removed from the record deletes
    the grant.
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
  words, several words in one condition, quoted phrases, and the
  negation of each. Both accounts must return the same events. The
  expanded form of each query must still match on the selected
  occurrence's own component.
- **`jmap_calendar::scheduling`**: invitations, updates, cancellations
  and replies to and from a key account, an outside address and a plain
  local account; `sendSchedulingMessages` false and absent; pins and
  withheld recipients; the mail guard on a failed commit; the drain run
  from a JMAP request; an invitation by email found by UID after the
  drain; no `CalendarItipMessage` task for a key account, ever.
- **`jmap_calendar::sharing`**: a grantee's reads and writes at both
  levels, the delegated reply with `SENT-BY`, `shareWith` refused,
  revoke, the sync boundary (D15), a non-grantee refused.
- **`jmap_calendar::refusals`**: every row of 6.11, a keyless token on
  each served method, impersonation.

### 9.3 The differential oracle

`jmap_calendar::oracle` runs the script of 3.5 through both protocols on
fresh key accounts and compares as 3.5 says. Its precondition is that
both runs start from equal accounts with equal pins. It reports the
first step that differs with both projections.

### 9.4 Upstream's JMAP calendar tests in key mode

Key-mode `jmap_tests` skips the calendar modules today. From release 6
it runs upstream's `calendar::calendars`, `event`, `instance`, `alarm`
and `identity` bodies with key accounts wherever the rulings leave their
assertions true, and a variant in `tests/src/jmap/za_variants.rs` where
they do not. Expected variants: `notification` (refused), `acl` (the
grant made through the share endpoints), `availability` (empty), and
the cases that use drafts, per-user data, participant-visibility flags,
default alerts by reference or `CalendarEvent/copy`. The plan lists each
changed assertion with the ruling that changes it.

### 9.5 Leak coverage

The scan gains, for every scanned account:

- after JMAP writes, the rules it applies after CalDAV writes: sealed
  archives only, no calendar search document, no event preferences;
- no `CalendarItipMessage` task;
- the token record holds only what 4.3 lists, and no token secret, code
  or MK appears anywhere in the store, while sessions are live and after
  they end;
- no hand-off entry outlives its code.

### 9.6 Tests that change

The refusal assertions for the methods section 6 opens:
`tests/src/za/gating.rs`, `tests/src/za/websocket.rs` (the socket still
refuses, now with the keyless answer), `za_variants::calendars` and the
session pins in `za_variants::principal_get`. The plan inventories them.

### 9.7 Mutation sample

At least these faults, each caught by a named test:

- the JMAP create skips the lease; skips sealing; calls
  `ItipMessages::queue`; drops the token pairs on update; drops the
  display name; skips the SEQUENCE floor;
- a text filter reads the stored tree instead of the view; the
  unexpanded matcher reads one component at a time;
- a token-record write omits the vault assertion; a superseded refresh
  generation deletes the grant; a lost record race puts back a pruned
  entry;
- the drain hook is removed;
- a notification destroy is let through;
- the wrap's associated data omits the grant id; the vault commit leaves
  the token record; a superseded refresh entry is kept; a token with an
  unopenable secret is served keyless; the hand-off is not deleted on
  redemption.

### 9.8 Manual checklist: the reference client

`docs/zero-access/client-jmap.md` records how to serve Bulwark's static
edition from a local server and the checks of section 7, run against a
key account with and without a second factor, with a second key account
as grantee and invitee and an outside address at the sink. It is a
manual gate, as the native-client checklists are.

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
    holds one beyond the key cache's fingerprint of the header.
62. MK leaves process memory only wrapped under a key derived from a
    secret the server does not store: the password, the recovery key,
    an app password, a token secret or a hand-off secret.
63. The token record yields keys only to a token upstream accepted, for
    the grant, kind and generation the entry was made for, under the
    current vault revision.
64. Every vault write deletes the token record in its own batch, and
    every write of the token record asserts, in its own batch, the vault
    record it was made under. No wrap made under one vault revision is
    stored under a later one.
65. A token that carries a secret is served with keys or refused.
66. A scheduling inbox item is never listed, opened or destroyed through
    JMAP.

Amendments:

- **Release 1 section 9**, the row "JMAP calendars capability and
  methods": superseded by section 6. The row on Bearer and OAuth logins
  on calendar paths: a token with a secret carries keys (4.6).
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

## 11. Merge hotspots and silent dependencies

New fork insertions in upstream files:

- `crates/http/src/auth/oauth/auth.rs`: `authenticate_login` takes keys;
  the hand-off at `issue_auth_code`, `request_consent` and
  `confirm_consent`.
- `crates/http/src/auth/oauth/token.rs`: the code exchange and the
  refresh grant.
- `crates/common/src/auth/oauth/token.rs`: the suffix strip in
  `decode_access_token`. `crates/common/src/auth/oauth/grant.rs`: the
  entry removal in `delete_oauth_grant`.
- `crates/common/src/auth/authentication.rs` and
  `crates/http/src/auth/authenticate.rs`: the Bearer arm's keys. Both
  are existing hotspots.
- `crates/common/src/auth/vault.rs`: the record's deletion in
  `vault_commit_store`. `crates/types/src/field.rs`: field 153.
- `crates/services/src/task_manager/tasks/maintenance.rs`: the sweep.
- `crates/jmap/src/api/request.rs` (admission, the drain hook),
  `crates/jmap/src/api/session.rs`, `crates/jmap/src/principal/get.rs`,
  `crates/jmap/src/changes/get.rs`, and one branch each at the head of
  the calendar handlers in `crates/jmap/src/calendar/`,
  `crates/jmap/src/calendar_event/` and
  `crates/jmap/src/participant_identity/`.
- `crates/dav/src/calendar/` and `crates/dav/src/common/za.rs` shrink:
  their key branches become calls into `groupware`.

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
- D12 relies on `InstanceTextFilter` covering every text condition of
  `CalendarEventFilter`, and on its fields being the ones
  `build_calendar_document` indexes. A condition or a field upstream
  adds must be classified. The unexpanded matcher mirrors three things:
  that function's choice and order of values, the joining of a field's
  successive values in `IndexDocument::index_text`
  (`crates/store/src/search/document.rs`), and the planning of a text
  condition in `crates/store/src/search/leaf.rs`. A change to any of
  them needs a re-read; the two-account query case of 9.2 is the guard.
- 4.5 relies on upstream's retry case returning without a registry
  write and on no lock being held from the rotation to the fork's
  record write. The classification in step 2 exists for that gap.
- 4.6 relies on the HTTP layer being the only caller that reads the
  token record. `authenticate_with_keys` is shared with the IMAP and
  POP3 logins.
- D14 relies on the personal bundle's rkyv layout keeping its colour
  field.
- 1.3's facts about the reference client are facts about one release of
  it.

## 12. Decisions, and what Jay should check

Jay's decisions:

1. Release 6 delivers server support for a web client and sets none up
   (2026-11-13).
2. Bulwark is the reference client (2026-11-13).
3. Sharing stays on the account page; a client's share dialog gets an
   error (2026-11-13).
4. A key account's OAuth sessions end on every vault write (2026-11-02,
   kept here: 4.7).
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
  upstream's registry writer.
- **4.5**: a refresh whose key entry is lost ends the grant, so the
  person signs in again. A refresh that only lost a race to a newer one
  is refused without ending the grant.
- **D12**: text search of a key account's events matches whole words
  and their stems, as an ordinary account's does.

Operator note: how long a browser stays signed in is upstream's
refresh-token lifetime, thirty days without use by default. The release
adds no separate setting for key accounts.

Follow-up outside this repository: the account page's text about how
long keys stay in memory describes password clients only and needs a
sentence about browser sign-in.

## 13. Premises for the plan's preflight

Read from the code or from the reference client's source during design,
and to be re-checked before the plan's tasks are fixed:

- The seam inventory between upstream's JMAP calendar handlers and the
  CalDAV key branches (27 points), and the visibility of each function
  section 5.1 moves.
- The order of upstream's `/set` steps that 3.1's table assigns, for
  create, update and destroy.
- `InstanceTextFilter`'s coverage and its field set against
  `build_calendar_document`'s (D12).
- That `blob_download` can be given the unsealing reader for `parse`
  (6.6).
- That upstream's in-memory store entries of the code and the consent
  token expire as section 4.4 assumes, on every supported store.
- Every caller of `delete_oauth_grant`, and that the registry's destroy
  is the only grant deletion outside it (4.7).
- The reference client's behaviour in 1.3, against a running instance.
- How the maintenance task can enumerate the accounts that hold a token
  record without a full property scan.
