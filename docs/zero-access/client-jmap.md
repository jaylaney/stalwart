# Reference JMAP client against key accounts (manual checklist)

## What this is

The manual gate of release 6 spec 9.8
([design](../superpowers/specs/2026-11-13-zero-access-jmap-calendars-design.md), sections 1.3, 7
and 9.8). It checks that an unmodified browser JMAP client works against key accounts. The
reference client is Bulwark Webmail **1.21.2**, static edition (`github.com/bulwarkmail/webmail`,
tag `1.21.2`). The checklist qualifies the server only: the release ships, hosts and configures no
client, and nothing here becomes part of a deployment.

The gate has not been run. The automated tests of plan 17
([outcome note](../superpowers/plans/2026-11-14-zero-access-plan17-outcome.md)) exercise the
server's handlers, codecs and pipeline on local RocksDB; they do not establish what this client
does. Record each run here or in the outcome note's successor, with the details under
"Recording".

## Setup

On one local machine:

1. Build and start the server from the commit under test, on a fresh store, with the scripts in
   `scripts/local-server/` (their `README.md` has the first-run steps: the TLS certificate, the
   recovery boot, `provision.sh`, then `start.sh`). Use the shipping feature set
   (`--no-default-features --features rocks`), never `enterprise`.
2. Fetch the client at tag `1.21.2` and build its static edition as its own documentation for that
   tag describes. Serve the built files from a local static HTTP server on an origin of its own
   (for example `http://localhost:8081`). Record the origin.
3. Register the client with the server, as an administrator, through the registry's JMAP methods
   (`x:<Type>/set`, as `tests/src/utils/registry.rs` and `tests/src/utils/oauth.rs` do):
   - an `OAuthClient` whose `clientId` is the one the client's configuration names, whose
     `redirectUris` hold the client's redirect address on the origin of step 2, with the
     `calendars` and `mail` scopes and the `authorizationCode` and `refreshToken` grant types;
   - optionally, an `Application` whose `oauthClientId` is that client id. The server's rule
     (`crates/common/src/manager/application.rs`, `is_application_client_id`;
     `crates/common/src/auth/oauth/client.rs`, `registered_client` and `kind`): a registered
     client with a matching `Application` is a first-party client and is granted the full scope
     set; without one it is a registered client granted the scopes it asks for. Either way the
     password step issues the code at once only when the request carries `consent: true` (the
     reference client sends it, spec 4.4), or when a grant for the client and scopes already
     exists and the request does not force the consent prompt
     (`crates/http/src/auth/oauth/auth.rs:832`); otherwise it answers that consent is required, with a consent token. Record which kind of
     client was registered and which path the sign-in took.
4. Make the server's HTTP CORS answers allow the client's origin. The only CORS switch in the
   registry schema is upstream's `usePermissiveCors` on the `Http` settings
   (`crates/registry/src/schema/structs.rs`); the release adds none. `ZA_ACCOUNT_PAGE_ORIGIN`
   governs only `/api/vault/*`, which the client does not call. Record the setting used.
5. Point the client at the server's base URL, as the client's own documentation describes.
6. Accounts, all on the fresh store:
   - **A**: a key account without a second factor, provisioned through
     `POST /api/vault/setup-token` and `POST /api/vault/setup` (README-dev, "Operating zero-access
     accounts");
   - **B**: a key account with a second factor (TOTP enrolled through `/api/vault/totp`);
   - **C**: a second key account, used as the grantee of a calendar of A shared through the
     account API (`docs/zero-access/calendar-sharing-api.md`) and as an invitee;
   - **O**: an outside address in a domain that is not local, relayed to an SMTP sink on the same
     machine (any local SMTP capture tool), through a relay route as the za suite's
     `imip::route_outside_mail` sets one up. The sink receives the server's mail to O. Mail from
     O into the server (O's replies, an invitation O sends) becomes a pending item only when
     upstream's `is_authenticated()` accepts the message (release 5 invariant 43); a capture tool
     that only receives cannot send such mail. If the setup has no sender the server accepts as
     authenticated, record the inbound legs of rows 6 and 8 as not run, with that reason.

   Sign in to the client as A in one browser profile and as C in another. B is used for the
   second-factor sign-in rows.

## Recording

For each run, record:

- the date;
- the client's version (`1.21.2`, static edition) and the origin it was served from;
- the server's commit and its `stalwart --version` line;
- the browser and its version;
- one result per row of "Checks": as expected, different (with what was seen), or not run (with
  why).

Record a row that cannot be exercised (a feature the client lacks, a missing account) as not run,
never as passed.

## Checks

The rows of spec section 7, in its order, then D29's rows. "A" is the signed-in user unless a row
says otherwise.

| # | Feature | Action | Expected result |
|---|---|---|---|
| 1 | Sign in with password, and with password and code | Sign in as A with the password; sign out; sign in as B with the password and the TOTP code | Both sign in; the calendar shows. A wrong password is refused. |
| 2 | Month, week, day and agenda views | Open each view with events present | Each shows the events with their titles and times. |
| 3 | Create, edit, move and delete an event | Create an event, change its title and time, move it to another of A's calendars, delete it | Each step is shown and persists after a reload; a CalDAV client of A sees the same event. |
| 4 | Recurring events: this one, this and future, all | Create a weekly event; edit one occurrence; edit this and future; edit all; delete one occurrence | Each edit applies to the occurrences the choice names. |
| 5 | Tasks | Create, complete and delete a task | Each step is shown and persists. |
| 6 | Invite a key account or an outside address; see replies | As A, invite C and O to an event with the invitation sent; C accepts in its calendar; O answers by mail the server accepts as authenticated (setup, account O) | C sees the invitation; O's mail reaches the sink; A sees both replies. Without an authenticated sender for O, record O's reply as not run. |
| 7 | After the first invitation to a key account | Row 6's invitation is A's first to C (A had not pinned C before). Keep using the same browser session; reload; wait past the access token's lifetime | The browser stays signed in (D19): the same session reads the event, and the client's refresh is granted. Only a password change, a recovery or a second-factor change ends a key account's browser sessions; a grant's deletion (a sign-out) ends that grant's. |
| 8 | Accept or decline, from the calendar and from an invitation email | As C, accept one invitation from the calendar and decline another; for an invitation mailed from O (by a sender the server accepts as authenticated), answer from the email and from the calendar | Each answer reaches the organizer; the event shows the answer. Without an authenticated sender for O, record the mailed-invitation leg as not run. |
| 9 | Calendars shared with the user, read and edit | As C, open A's calendar shared at `view`, then at `edit`: read, create and edit an event | At `view` C reads and cannot write; at `edit` C reads and writes, and A sees C's changes. |
| 10 | Answer for the owner of a shared calendar | As C with `edit`, answer an invitation in A's shared calendar | The organizer receives the answer sent by C on A's behalf (`SENT-BY`). |
| 11 | Create, rename, colour, reorder and delete a calendar | As A, do each | Each step persists. A colour set on a shared calendar is A's own; C's starts as A's at the grant (D14). |
| 12 | Default reminders | Set a calendar's default reminder; create an event in it | The reminder is applied when the event is saved (D11) and shows after the next read, not in the save's answer. |
| 13 | Import an `.ics` file | Import a file with one event and one recurring event | Both appear. |
| 14 | Search | Search a word of an event's title, of its description, and a word that matches nothing | Matches are found; the search is matched on the server per request. Record the time on a calendar with a few hundred events. |
| 15 | Live updates | With A open in two tabs, change an event in one | The other shows the change (push carries state changes only). |
| 16 | The share dialog | Open the client's share dialog for a calendar and share it | An error that names the account page ("Calendar sharing is managed on the account page."). |
| 17 | Free/busy and suggested times | Open the client's free/busy or suggested-times view | Hidden or empty. |
| 18 | Change-notification pop-ups | Change a shared event as C while A is signed in | No change-notification pop-up. |
| 19 | Password, two-factor and app-password screens | Open each of the client's screens | Errors; the account page does these. |
| 20 | File uploads | Upload a file in the client's files feature | Refused: a key account cannot store file content until release 8 (spec 1.2). |
| 21 | Its own shared tags and templates | Open the client's tags or templates feature | Absent (object metadata is refused for key accounts). |
| 22 | D29: an opaque state across several pages | As C, with A's shared calendar holding more changes than one page (add events in A's CalDAV client), let the client sync incrementally | The client sends back the `z` continuation strings unchanged across the pages and ends at an exact state; no page is refused. Record the states the browser's network log shows. |
| 23 | D29: an initial sync across a sharing-boundary change | As C, start an initial sync of a large shared calendar, and while it pages rename the calendar through CalDAV as A (or grant again) | The initial sync continues to its end; it is never refused at the boundary. |
| 24 | D29: restart after an invalidated incremental sync | As C, hold an incremental state from before the rename of row 23, then sync | The server answers `cannotCalculateChanges`; the client restarts from an initial sync and shows the calendar. |

Rows 22 to 24 are the reference-client part of D29's qualification. The handler and codec tests of
Tasks 22, 26 and 37 do not establish them.

What the client may meet, from the plan's execution (each is server behaviour that the checks above
can show, recorded so it is not mistaken for a client fault):

- A `/set` with `ifInState` in a request whose own drain imported or merged an invitation answers
  `stateMismatch` once per arrival (R34-5). That is correct by JMAP's rule; the client is expected
  to refetch and retry.
- With default alerts, a windowed or property-subset read can show an `alerts/<id>/iCalendar` patch
  in each override whose title differed from the master's when the defaults were written (Task 39,
  F1). An ordinary account's events show the same patch when their alarms carry different
  descriptions.
- A grantee at the `view` level is refused every `Calendar/set` update of its own entry, its
  visibility and subscription flags included (Task 39, F3).
- An attendee that creates, as its own copy, an event it does not organize and has already answered
  sends no reply from that create, and that copy does not schedule later edits either (R36-10, the
  final review's FC1-1; decided by the project owner on 2026-11-18 with no code change, spec 3.3). Accepting an invitation whose event is
  not yet in the calendar, if the client does it by creating the event, therefore creates the copy
  and sends no reply: record whether the client accepts that way and whether the organizer sees
  the answer. Invitations that arrive through the drain carry a schedule tag and are unaffected.
- An empty `shareWith` (`null` or `{}`) on a `Calendar/set` create or rename is refused like any
  other `shareWith` (FC2-15): record whether the client sends one.
- An event created without a `uid`, whose create fails after its invitations went out, is retried
  by the client under a new UID the server generates, so the attendees can hold an event the
  organizer never stored (FB-2): record whether the client sends a `uid` on create.
- A reminder's push (`CalendarAlert`) names a positional alert id (`k1`, ...) where
  `CalendarEvent/get` shows the stored id, since the alarm's id is sealed (FD-3): record whether
  the client matches the alert by id.
- An event or invitation written through JMAP holds upstream's converter output (`DURATION` for
  `DTEND`, the export's property order), a `CREATED` stamp, and a SEQUENCE the server raises when
  the client omits `sequence` (Task 38). A CalDAV client of the same account sees that text.
- A calendar destroy can be refused as too large for one transaction (`forbidden`, "This calendar
  holds too many events to delete at once. Delete some of its events first."), before any
  cancellation is sent: record whether the client shows the text and lets the user delete events
  first (spec 5.3).
- A calendar destroy that meets a concurrent write to the account's calendars answers "try again"
  (`forbidden`, "Another process modified this calendar, please try again."), possibly after its
  cancellations went out: record whether the client retries or tells the user (spec 5.3).
- A grantee's destroy or update of an event whose details are private to its owner (a private or
  sealed non-standard `CLASS`) is refused (`forbidden`, "Private events can only be modified by
  the calendar owner."), and an event whose details the grantee cannot open answers `notFound`
  (spec 3.3, 6.5): record what the client shows.
- A grantee's event state from `CalendarEvent/get` lies at or above the sharing boundary and is
  the state to sync from (spec 6.8): record whether the client takes its event state from
  `CalendarEvent/get` (or from `/query` or `/changes`), and whether its next `/changes` is served
  after a grant or a rename.

## Assumptions to confirm on a running instance

The facts of spec 1.3, read from the client's source at its tag and not yet observed against this
server. Confirm each with the browser's developer tools (network log) during the checks.

1. **Every JMAP method call is an HTTP POST to the session's `apiUrl`; no WebSocket; push is a
   `GET` of `eventSourceUrl` with the `Authorization` header, reading state strings only.** Observe:
   the network log shows POSTs to `apiUrl`, no WebSocket upgrade, and one long-lived `GET` to
   `eventSourceUrl` whose events carry states only.
2. **Sign-in posts the account name, password and code to `POST /api/auth`, then exchanges the code
   at `POST /auth/token`; every token is an opaque string in `Authorization: Bearer`, refreshed on
   a 401, with refreshes serialised across tabs.** Observe: the two requests at sign-in; the access
   token about 45 characters longer than an ordinary account's; after a 401 one refresh and a
   retried request; with two tabs open across an expiry, one refresh, not two racing ones.
3. **The calendar shows only when the primary account's `accountCapabilities` holds
   `urn:ietf:params:jmap:calendars`; a sharer's calendars are read by the same calls with the
   sharer's account id.** Observe: the session object's capabilities for A and, for C, A's account
   listed with the calendar capability; C's calls on A's calendars carry A's account id.
4. **A `CalendarEventNotification` is acknowledged by destroying it.** Observe: whether the client
   calls `CalendarEventNotification/set` with `destroy`; for a key account the server answers
   `accountNotSupportedByMethod`, and the invitation stays until the drain applies it.
5. **The event editor sends `useDefaultAlerts: true` on every new event.** Observe: the
   `CalendarEvent/set` create body of a new event.

Record a fact that does not hold as a finding for the spec (section 13 lists these facts as still
to check), with what was seen.
