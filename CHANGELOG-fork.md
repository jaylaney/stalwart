# Fork changelog

This file records releases of the independent zero-access fork. Versioning, provenance,
compatibility, and release qualification follow [the release policy](docs/releases.md).
The inherited [CHANGELOG.md](CHANGELOG.md) records upstream Stalwart history.

## Unreleased

## 0.4.0-beta.1 — 2026-11-21

Immutable tag: `fork-v0.4.0-beta.1`; GitHub prerelease title `release-0.4.0-beta-1`. A limited
prerelease that moves the fork to upstream Stalwart `v1.0.3` and adds release 7 (mail search and
threads for key accounts) and release 6 (JMAP calendars and browser sign-in for key accounts).
Published as a GitHub prerelease, it claims no general client or production readiness until its
pending gates (below) pass.

### Provenance

- Fork source: the commit the tag `fork-v0.4.0-beta.1` names, resolved by
  `git rev-parse 'fork-v0.4.0-beta.1^{commit}'`. The tag goes on the merge commit of this
  release's preparation pull request, so this entry names the tag, not a commit. The published
  release notes and the shipped `stalwart.provenance.json` record the exact full commit.
- Fully integrated upstream base: Stalwart `v1.0.3`, commit
  `8b32686532d34623e9ce8e1237cf2bed9554af38`
  ([release notes](https://github.com/stalwartlabs/stalwart/releases/tag/v1.0.3)), which
  includes `v1.0.2` (`1d66d65daf4828d7a6b38214873cab02274fd55b`). Merged through PR #82 and
  pinned in `fork-release.toml` and in the verifier's `PINNED_UPSTREAM_TAGS`. Internal crates
  and numeric compatibility checks follow `1.0.3`; the fork-owned `vault` crate keeps `0.16.25`.
- No additional upstream backports.
- Since `fork-v0.3.0-beta.1`: PR #82 (upstream `v1.0.3`), PR #84 (release 7, issue #23) and
  PR #90 (release 6, issue #22); PR #83 (CI token permissions), PR #85 (the differential test
  corpus's setup, merged into release 7's branch), PR #87 (dnspython 2.9.2 in the CalDAV test
  runner) and PR #88 (search documentation); and direct commits on `main` to the README, the
  developer guide, the manual checklist's status and CI's skip of documentation-only changes.
- The published `fork-v0.1.0-beta.1`, `fork-v0.1.1-beta.1`, `fork-v0.2.0-beta.1` and
  `fork-v0.3.0-beta.1` tags and artifacts retain their original identity.

### Changes

#### Upstream `v1.0.2` and `v1.0.3` (PR #82)

The [merge outcome](docs/superpowers/plans/2026-11-05-upstream-1.0.3-merge-outcome.md) records
the conflicts, hooks, decisions and residuals. Ordinary accounts take upstream's changes, among
them OpenPGP and S/MIME encryption at rest with the algorithm chosen from the user's key, the
encryption gateway, external public keys, key harvesting, server-side blob encryption and zstd
compression, the `BlobRecode` task and new OAuth scopes; see the
[upstream changelog](CHANGELOG.md). For the fork:

- Only a committed ordinary delivery contributes to key harvest, decided after each recipient's
  outcome. Key accounts, refused or failed deliveries, Sieve outcomes that write nothing and
  duplicates never contribute. An ordinary co-recipient with a committed delivery still
  contributes its own correspondent information.
- Per-account encryption-at-rest settings are inert for key-account ingest. Ordinary ingest that
  fails during provisioning can leave upstream's `sample_blob` staged. Sealed staging uses no
  compression and the Email profile's encryption. The operator-held blob key remains outside the
  guarantee: backups carry decoded payloads, and swap snapshots use plain codec settings.
- A key account's `EmailPush` becomes a content-free `StateChange`. For configured `EmailPush`
  subscribers, that converted notification is delivered regardless of the subscription's
  `types`, an empty list included. Generic state changes still use type filtering.
- iTIP gates cover the trait entry points. A full REQUEST removes omitted overrides only up to
  the master SEQUENCE, for ordinary accounts and the key-account drain alike; an override at a
  higher SEQUENCE can remain. Explicit CANCEL keeps its sequence rules.
- Uploads check membership before the keyless refusal, so a nonmember cannot tell whether an
  account is a key account. The configured outbound gateway recipient-key fingerprint is traced
  beside queue-event addresses.
- Default-role edits bump the permission epoch, which narrows stale reuse of cached tokens. The
  inherited narrow race in which the spill cache publishes an old-permission token remains.
- The local leak scanner reports physical matching files and current decoded matching blob
  values separately. Stop the local server first. A compatible `ldb` and Python 3.14 or later
  are required, and an encrypted or undecodable logical value fails the check.

#### Release 7: mail search and threads for key accounts (PR #84)

Issue #23; [design, revision 38](docs/superpowers/specs/2026-11-04-zero-access-search-design.md),
[qualification report](docs/superpowers/plans/2026-11-07-zero-access-plan16-outcome.md),
[evidence index](docs/superpowers/plans/2026-11-14-zero-access-search-evidence-index.md) and the
developer guide's [Searching key accounts](docs/README-dev.md#searching-key-accounts). Before
it, a key account's mail could not be searched, sorted or threaded on the server (release 3's
refusals), and every message was a thread of its own.

- **Search, sort and snippets.** In a session that holds the account's keys, IMAP `SEARCH` and
  `SORT`, JMAP `Email/query` and `Email/queryChanges` (text filters and the `from`, `to`,
  `subject` and `sentAt` sorts) and `SearchSnippet/get` are served from a per-account index
  sealed under a search key derived from the account's master key. The index covers what
  upstream's covers (sender, recipients, subject, body, headers and extracted attachment text)
  with upstream's tokenizer and rules, so the results are an ordinary account's for the same
  mail, under the condition and with the differences under "Known limits". The `threadId` sort
  stays unsupported, as upstream's is for every account. The sorts a key account's JMAP session
  advertises in `emailQuerySortOptions` are now served, which closes #54.
- **Threads.** Messages are grouped as upstream threads them (Message-ID, References, subject),
  within the limits under "Known limits": JMAP `threadId`, `Thread/*`, `collapseThreads`, the
  thread filters and sorts and the thread counts of `Mailbox/get`; IMAP `THREAD`, `THREADID` and
  `EMAILID`. The thread id is sealed.
- **Indexed during sessions.** The index is built, read and written only in a keyed mail
  request: in an IMAP session holding the keys, before each command in the authenticated or
  selected state except `IDLE`, `NOOP` and `LOGOUT`, and after an `APPEND` that committed; or in
  a JMAP request over HTTP whose token holds the account's own keys (a password, an app password
  or an OAuth token that carries a secret) and that has an Email, Thread, Mailbox,
  SearchSnippet, EmailSubmission or Blob call that parsed with its capability in the
  scope-filtered `using`, with one more pass after an owner's `Email/set` create or
  `Email/import` in it. POP3, CalDAV and CardDAV, other JMAP requests, JMAP over WebSocket or
  EventSource, SMTP, ManageSieve and `/api/*` never index. The keyless reindex task and account
  destruction delete the index. Mail that arrives while no keyed mail request runs is indexed by
  the next ones; until then a server search does not find it, and it threads alone. A content
  search tries to bring the index current within a wait budget of `ZA_SEARCH_WAIT_SECS` (30 s by
  default). When it cannot obtain a current index, it is refused with "Search for this account
  is not available right now; try again shortly." (`unsupportedFilter` or `unsupportedSort` on
  JMAP, `NO [CANNOT]` on IMAP), never answered in part. That happens when the budget runs out,
  or earlier: the account's content is over `ZA_SEARCH_ACCOUNT_MAX_MB`, a load is refused by
  `ZA_SEARCH_MEM_MB`, the index is of a newer format, a store or transient error occurs, the
  index is being rebuilt, or the content part cannot resolve the condition. A session without keys still cannot search the account's mail.
- **More JMAP methods need the keys.** For a key account, every method that returns or takes an
  Email or thread id, or whose answer depends on the threads, needs the account's keys:
  `Email/query`, `/queryChanges` and `/changes`, `Thread/*` and `Mailbox/get`, which release 3
  served without keys, are among them, as are `EmailSubmission/get`, `/query` and `/changes`
  and `Blob/lookup`. A session without keys (WebSocket, EventSource, a token without a secret)
  keeps push, mailbox listing and management, flag and mailbox updates, identities, vacation
  responses and Sieve scripts.
- **Rebuilds.** A lost or unreadable index is rebuilt from the mail at the next keyed mail
  request and starts a new identity epoch: JMAP Email sync states issued before it are refused
  (`cannotCalculateChanges`) and the client resyncs, and IMAP `EMAILID` and `THREADID` values
  can change with no `CHANGEDSINCE` signal. The loss of the content part alone rebuilds the
  content without changing thread identity. A key account set up before this release has no
  index and is rebuilt once ("Upgrade and rollback").

#### Release 6: JMAP calendars and browser sign-in for key accounts (PR #90)

Issue #22; [design, revision 7](docs/superpowers/specs/2026-11-13-zero-access-jmap-calendars-design.md),
[outcome](docs/superpowers/plans/2026-11-14-zero-access-plan17-outcome.md) and the developer
guide's [Browser sign-in and JMAP calendars](docs/README-dev.md#browser-sign-in-and-jmap-calendars).
The release ships no web client, and the reference client's checklist
([`client-jmap.md`](docs/zero-access/client-jmap.md)) has not been run. Before it, a key
account's JMAP calendar methods were refused and its OAuth tokens carried no keys.

- **Browser sign-in carries keys.** An OAuth token of a key account issued through the
  authorization-code flow carries a 32-byte secret after a `~`, which makes it about 45
  characters longer than upstream's, and the HTTP layer opens the account's keys from it on
  every request. A per-account token record (principal field 153) holds the master key wrapped
  under keys derived from those secrets. The device-code flow and the Bearer logins of IMAP,
  POP3, SMTP and ManageSieve stay keyless. At a cache miss, a token that carries a secret whose
  entry is gone, expired or does not open is refused with 401, as an invalid token is, and the
  refusal is not counted for banning. On a cache hit the token record is not read. A grant holds
  at most four access entries: each access-token issue (the code exchange, then each refresh)
  first removes the grant's expired entries, then, if four remain, drops the earliest-expiring
  of those stored entries before adding the new one (the new entry is never the one dropped,
  and a new entry already expired at the write is not stored and drops nothing). The record
  write invalidates no cache, so every node that holds a live cache entry for a dropped token
  keeps serving it until that node's entry ends, and each node without one refuses it with 401
  at its next request. A keyed cache entry lasts at most the
  token's remaining signed lifetime less one second (an access token's is an hour by default)
  and the key cache's bounds (`ZA_KEY_IDLE_SECS`, 900 s; `ZA_KEY_MAX_AGE_SECS`, 3600 s), so a
  client should refresh before `expires_in`. A credential change and a grant's deletion (a
  sign-out, a revocation, or a 33rd sign-in's eviction when the evicted grant's deletion
  succeeds) end cached access at once on that node and on the others by the cluster broadcast. A client that refreshes on 401 gets a new entry while
  its refresh entry exists; a refresh whose entry is gone deletes the grant and answers
  `invalid_grant`, and the user signs in again.
- **When browser sessions end.** Only a password change, a recovery or a second-factor change
  ends a key account's browser sessions. A grant's deletion (a sign-out that revokes it) ends
  that grant's sessions, at once on the node that deletes it and on the others by the cluster
  broadcast. A new recovery key, an app password's revocation, the self-check's repair and the
  first pin's rewrap keep them. A second-factor change draws a fresh salt, so removing the
  factor again does not revive the sessions it ended. At most 32 browser sign-ins of an account
  hold keys at once; the 33rd ends the one used least recently. In `0.3.0-beta.1` every vault
  write ended a key account's OAuth sessions.
- **JMAP calendars.** Served to the owner, and to a grantee of a shared key calendar under the
  rules CalDAV applies: `Calendar/get`, `/changes`, `/query` and `/set`; `CalendarEvent/get`,
  `/query`, `/queryChanges`, `/set` and `/changes`, and `/parse` for the keyed owner;
  `ParticipantIdentity/get`, `/changes` and the default switch of `/set`. A JMAP write stores what
  the CalDAV write of the same tree stores. A `/set` processes creates, then updates, then
  destroys, and commits each stored event (or calendar) on its own; when the call holds an
  occurrence id, the updates and occurrence removals of one stored event share its commit. A store
  error answers every id of that commit and every object processed after it ("The server could not
  complete this change, try again."), and the objects committed before it stay. Still refused:
  granting a share through JMAP (`shareWith`; sharing stays on the account page),
  `CalendarEventNotification/*`, `CalendarEvent/copy`, per-user event data, drafts,
  participant-visibility flags, object metadata, inline attachment blobs, and identity creates,
  updates and destroys.
- **Invitations.** A keyed JMAP request with a calendar call imports the account's pending
  invitations first, as a CalDAV request does. On both protocols only password clients, app
  passwords and tokens whose scope admits calendars import them, so an account used only through
  a mail-only browser client applies no invitation until such a client connects.
- **Calendar text search.** JMAP text conditions on a key account's events are matched in memory
  per request, over the unsealed events; a key account has no calendar search documents. A
  query without a time range opens every event of the account (300 events in about 0.4 s in the
  outcome note's debug-build measurement on one machine; memory not measured). A grantee does not
  find an event whose details it cannot read.
- **Default alerts.** A create or update with `useDefaultAlerts: true` writes the calendar's
  current default alerts into the event by value; the `/set` answer does not carry them.
- **Calendar deletes.** A key account's calendar delete, through JMAP (a non-empty calendar needs
  `onDestroyRemoveEvents: true`, otherwise `calendarHasEvent`) or a CalDAV collection `DELETE`,
  removes every name an event has under the calendar. An event that also has a name in another
  calendar is kept, with its alarms rescheduled, and sends no scheduling message. Only the events
  the calendar held alone are deleted, and only they send messages: a CANCEL for an event the
  account organizes, a declining REPLY for one it attends, for scheduling objects whose range
  ends after now and while the account's scheduling is not denied. On RocksDB, SQLite,
  PostgreSQL and MySQL the delete, these changes included, is one transaction (pinned by a test
  on RocksDB and SQLite; for PostgreSQL and MySQL read, not run). A calendar too large for one
  transaction (more than 5,000,000 bytes of deletions by the server's count) is refused before
  any scheduling message is sent (JMAP `forbidden`, CalDAV 403): delete some of its events
  first. A delete that meets a concurrent write, or an event moved while it was prepared,
  answers "try again" (JMAP `forbidden`, CalDAV 503), possibly after its internal scheduling
  items reached local key accounts' inboxes. Its email to outside and plain attendees is sent
  only after the commit, so a failed attempt mails nothing (traced `ZA_IMIP_DROPPED`). A retry
  rebuilds the calendar's member list and delivers the internal items again for the events it
  still deletes; an event moved out of the calendar between the two attempts stays in its new
  calendar, and the cancellation (or decline) the first attempt delivered for it is not undone.
  FoundationDB, read replicas and the bootstrap store keep a chunked, unfenced
  delete.
- **Sharing and sync.** A shared calendar's JMAP `color` preference is carried in each principal's
  sealed personal bundle; a new grantee's starts as the owner's. CalDAV's `calendar-color` stays a
  separate property, one value for all principals. A grantee cannot delete or change through JMAP
  an event whose details are private to its owner. A grantee's event state from
  `CalendarEvent/get` or `/query`, or from `/changes` and `/queryChanges` in one page or from an
  exact state, lies at or above the latest sharing change, so its next sync is served; a sync
  paged from the initial state can still end below one (issue #93).
- **CalDAV.** A key account's `COPY` onto another name in the same calendar is refused (403
  `no-uid-conflict`), as an ordinary account's is.

#### Every account

These change for ordinary accounts too, since the account is not known at that point or the
cleanup must cover both kinds:

- Traced request URLs and traced `Referer` headers show the value of every `code` query parameter
  as `[redacted]`; a bare `code` without `=`, a callback URL percent-encoded inside another
  parameter and a `;`-separated query pass unmasked. The token endpoint's successful answers are
  not traced.
- A `~` suffix is stripped from every token that begins with the server's token header before it
  is opened. An ordinary account's consent token followed by a 43-character suffix of the secret
  alphabet is accepted, the suffix stripped. A device-code poll whose `device_code` is the
  pending user code answers `expired_token` where upstream answers `authorization_pending`.
- IMAP `SAVE` resolves every sequence containing `$` against the captured previous search result.
- Permanent destruction of an account that holds neither a vault record nor a search manifest
  scans all 256 blob-link shards for its search links: one full blob-link scan per such
  destruction.
- Small costs: one cached account read at a code exchange, a refresh and a grant's deletion; one
  read of field 153 per account in every maintenance purge run; and, on any account's grant
  deletion, a pass over the key cache and the grant's sessions evicted twice, so an
  authentication of any account in flight at either instant is served but not cached.

### Trust-model changes

The limits of the calendar, mail and email-invitation designs stand. Release 7
([design section 2](docs/superpowers/specs/2026-11-04-zero-access-search-design.md)):

- What the index holds (terms, positions, sort keys, threading fields, thread ids and thread
  history) leaves process memory only in blobs sealed under the account's search key and bound
  to the account and the blob's position in the index. Search queries are never traced; the
  `ZA_SEARCH_*` traces carry ids, counts, sizes, versions and reason classes.
- Beyond the mail design's visible list, the operator learns: the manifest in principal field
  152 (format version, revisions, how far the index has reached, the identity epoch, the thread
  blobs' sizes, the content chain's length and total sealed size, a rebuild's cursor), and from it
  and the blob store's timestamps when the account's sessions indexed and how much; each content
  blob's sealed size, a coarse property of how its messages' words compress; and that a session
  made a content request, and when. A publishing pass writes one Email update of a witness
  message, one Thread change and an update of every mailbox, which follow the arrivals, not what
  the index found.
- One residual channel: a pass's duration, and where `ZA_SEARCH_PASS_MS` stops it, show the
  processing cost of the messages it admitted (tokenization, attachment-text extraction), a
  coarse property of their content that an operator who delivers calibrating mail can read.
- The running server holds the index in plaintext for every account in use and drops it,
  unzeroized, on eviction. A store writer can delete the manifest or a blob, which forces a
  rebuild and every client's resync, or roll the manifest back, after which clients may not be
  told what a re-published revision renders differently. A blob cannot be forged or moved to
  another account or position.

Release 6 ([design section 2](docs/superpowers/specs/2026-11-13-zero-access-jmap-calendars-design.md)):

- New at rest: the token record. It holds the master key wrapped under keys derived from token
  secrets, which exist only in the token strings clients hold; the record alone opens nothing.
  An entry can outlive its token until the next maintenance run (daily by default). During
  sign-in the master key is stored wrapped under a secret that travels inside the authorization
  code, for the code's lifetime (ten minutes by upstream's default), and deleted when the code
  is redeemed.
- A browser holds a credential that opens the account's keys while its grant lives (upstream's
  default: thirty days without use, an operator setting). Whoever takes a live token and the
  stored record has the keys, as with the password and the vault record. A token's OAuth scope
  limits what the server does for it, not what its secret opens. The server receives the
  password at sign-in, not on every request of a browser session.
- The operator also learns that an account signs in from a browser, how many grants it holds and
  when each was refreshed, which upstream's grant objects record. Whoever serves a web client's
  code can read what the user reads.
- A JMAP write stores what the CalDAV write of the same tree stores. JMAP-only values are visible
  by design (sort order, visibility flags, default-alert offsets, an IANA time zone id), refused,
  or written into the sealed tree. A default alert's id, a string the client chooses, is stored
  in the clear.

### Operator actions

- **A store carried over from `0.3.0-beta.1`** (see "Upgrade and rollback"). Upstream 1.0.2
  removed the per-account cipher choice, and 1.0.3 reads each account's earlier encryption
  settings as `Disabled`, losing them whole, the account's `encryptOnAppend` and
  `allowSpamTraining` included (the setting is inert for key accounts). For each account that had
  encryption at rest, reselect the public key, which creates fresh settings with both flags off;
  set `encryptOnAppend` again where append encryption was in effect (the server's and the
  account's flags both on), since IMAP `APPEND` and non-draft JMAP `Email/set` creates and
  `Email/import` are encrypted only with it, and `allowSpamTraining` again where it was on, since
  encrypted mail feeds spam training only with it. Upstream 1.0.2 replaced the server's
  `Email.encryptAtRest` with `atRestEnable` in the new `Encryption` settings, enabled by default,
  so a server that had disabled it disables it again, and removed the server's
  `Email.encryptOnAppend`. Start once with `STALWART_UPDATE_ROLE_PERMISSIONS=true` to add the
  default-role permissions introduced after 1.0.0, then remove the variable. Third-party clients
  that manage app passwords, API keys or account settings need upstream's new OAuth scopes. See
  the [upstream changelog](CHANGELOG.md).
- **Search sizing.** Size the `ZA_SEARCH_*` environment variables (defaults and meanings in the
  developer guide) for the host: `ZA_SEARCH_MEM_MB` (1536 MiB) budgets loaded content per
  process, `ZA_SEARCH_THREAD_MEM_MB` (256 MiB, soft) loaded threads, and
  `ZA_SEARCH_ACCOUNT_MAX_MB` (512 MiB) caps one account's sealed content, above which its content
  indexing stops and its content searches are refused. The defaults are provisional: the
  qualification report's measurements, on test builds and one machine, do not establish
  production sizing. Set the same `ZA_SEARCH_ACCOUNT_MAX_MB` on every node of a cluster. Unset,
  malformed and zero values take the defaults. The server logs the effective values at startup,
  and refuses to start, printing the reason, when `ZA_SEARCH_PAD_MIN` is below its minimum,
  `ZA_SEARCH_PASS_CAP` above its maximum, `ZA_SEARCH_ACCOUNT_MAX_MB` above a third of
  `ZA_SEARCH_MEM_MB`, or `ZA_SEARCH_SEGMENTS` outside 2 to 255.
- **After a blob-store fault.** If the blob store wrongly answered "not found", repair it, then
  run the per-account reindex task (`Task::AccountMaintenance`, type `Reindex`); for a key
  account it resets the search index, the next keyed mail request rebuilds it, and clients
  resync.
- **Browser sign-in.** The release adds no setting. A browser client signs a key account in
  through upstream's OAuth: the reference client's checklist registers it as an `OAuthClient` with
  the `authorizationCode` and `refreshToken` grant types and scopes that cover calendars, and its
  local test turns on `Http.usePermissiveCors`, the wildcard option: it adds
  `Access-Control-Allow-Origin: *` to every HTTP response but `/api/vault/*`, allowing every
  origin, server-wide. An origin-specific header is possible only through upstream's
  `Http.responseHeaders`, also server-wide, and only with `usePermissiveCors` off: the wildcard
  headers are applied after `responseHeaders` and replace a custom `Access-Control-Allow-Origin`,
  `-Allow-Headers` and `-Allow-Methods`, so a fixed origin needs all three set there.
  `ZA_ACCOUNT_PAGE_ORIGIN` governs only `/api/vault/*`.
  That setup has not been run against a client. How long a browser stays signed in is upstream's
  refresh-token lifetime. The maintenance purge removes expired entries, superseded refresh
  entries and the entries of grants destroyed through the registry from token records.
- **Tell key-account users** that after the upgrade each existing OAuth token of a key account is
  refused once and they sign in again, and that ending every browser session takes a password
  change, a recovery or a second-factor change, or revoking the grants.

### Operator-facing changes

Against `0.3.0-beta.1`, in the order of issue #74:

- **Registry types, properties and defaults:** none from the fork. Upstream `v1.0.2` and `v1.0.3`
  change the generated schema and defaults: among others the `SymmetricKey`,
  `ExternalPublicKey` and `SmimeTrustAnchor` objects, the `Encryption` settings (`atRestEnable`,
  replacing the server's `Email.encryptAtRest`), the removal of the server's
  `Email.encryptOnAppend` (each account's `EncryptionSettings` keeps its own),
  `Application.oauthScopes`, an OIDC directory's `trustedProvider`, the removal of the
  per-account cipher choice, and new permissions in the default roles. See the upstream
  changelog. No machine-readable registry schema is published with this release.
- **Environment variables and flags:** the thirteen `ZA_SEARCH_*` variables (release 7) and
  upstream's `STALWART_UPDATE_ROLE_PERMISSIONS`. Release 6 adds none. No command-line flag
  changes.
- **Zero-access HTTP endpoints:** no `/api/vault/*` path, request or response changes. `setup`
  also writes the account's empty search index, and a TOTP enrolment, replacement or removal now
  ends the account's browser sessions for good. A key account's access and refresh tokens from
  the authorization-code flow carry `~` and a 43-character secret; the token endpoint's
  successful answers are untraced, and the device-code poll's answer changes, for every account
  (above).
- **Server-side guards:** the four `ZA_SEARCH_*` startup refusals; the refusal of a key
  account's calendar delete too large for one transaction; the JMAP methods that now need a key
  account's keys.
- **What the process needs:** none found in the fork's changes: no new listener, file path or
  outbound connection. Release 7's index is stored in the configured blob store (blobs linked to
  the principal) and principal field 152; release 6's token record in field 153. Upstream
  `v1.0.2` adds outbound lookups for external public keys (Web Key Directory, keys.openpgp.org,
  HKP keyservers, DNS) and S/MIME intermediate-certificate retrieval and daily revocation-list
  checks.

### Upgrade and rollback

From `0.3.0-beta.1` (upstream `v1.0.1`): qualification covers fresh stores only, and no
automated test carries a store over (CI's fresh-store job checks a fresh store). What the code
and the records say:

- The store schema version is unchanged (base 7 with the edition flag; `verify_schema_version`
  in `crates/common/src/manager/schema.rs`), so this build's schema check accepts a store that
  `0.3.0-beta.1` wrote, of the same edition, instead of refusing it. Upstream describes `v1.0.2`
  and `v1.0.3` as upgrades of existing servers, with the operator actions above.
- Releases 6 and 7 change no stored struct layout. They add principal fields 152 (the search
  manifest) and 153 (the token record) and principal-linked search blobs, written on demand.
- A key account set up before release 7 has no search manifest (setup writes an empty one from
  this release on). Its first keyed mail request treats the absence as loss and rebuilds the
  index from the account's mail, over as many keyed mail requests as the mail needs. Until the
  index is current, content searches answer "not available right now" and unindexed messages
  thread alone; JMAP Email sync states issued before the rebuild are refused
  (`cannotCalculateChanges`) and clients resync, and IMAP `EMAILID` and `THREADID` values can
  change. The za suite exercises this path by deleting the manifest of an account the same build
  set up (an empty one, and one holding mail); no test starts from a store `0.3.0-beta.1` wrote.
- Each key account's existing OAuth token is refused once (its credential version changes), and
  the user signs in again.

Back up the complete database, blob store and configuration, and verify the restoration, before
replacing the binary. A rollback to `0.3.0-beta.1` restores that backup and loses every write
made after the upgrade; this release does not claim that a `0.3.0-beta.1` binary reads data
`0.4.0-beta.1` has written. One case is known: a binary built before release 6 refuses a shared
calendar's personal bundle that holds a JMAP `color` preference (set through JMAP, or copied from
the owner's at a grant), so it cannot read that calendar for each principal whose own bundle
holds one, and neither can such a node in a cluster that mixes versions. A colour set only by a
CalDAV client (`calendar-color`) does not block a rollback.

From `0.2.0-beta.1`: as above, together with the `0.3.0-beta.1` entry's notes on release 5; not
tested either. From `0.1.x`: not supported; moving is the fresh-store rebuild the `0.2.0-beta.1`
entry describes. Fresh stores are supported.

The planned shipping configuration is unchanged: Linux `x86_64-unknown-linux-gnu` with RocksDB,
without the enterprise and `test_mode` features, requiring an `x86-64-v2` CPU. Check the
artifact's provenance for its highest referenced GLIBC symbol version and test its runtime
dependencies on the deployment host; it is not a static or broadly portable musl build.

### Known limits and follow-ups

Release 7:

- Plan 16's Task 19 is incomplete. The memory and account-size defaults are provisional; native
  clients, networked-store timing (PostgreSQL unmeasured) and physical multi-node execution are
  not verified. Of the mutation sample, M16, M23 and M48 survived and later gained stronger
  tests that no faulty variant was run against, M29b was not executed, and the qualification
  report lists the other exceptions and the anomalies whose causes were not established.
- Parity with an ordinary account holds when each delivery, deletion and thread merge settles
  before the next thread-affecting operation (design section 7.5), with these recorded
  differences: freshness, since mail is indexed only during sessions; the "not available"
  refusal; each message keeps at most 16 distinct identifiers (its Message-ID, In-Reply-To,
  References from the nearest back, then Resent-Message-ID), so a message with its own
  Message-ID keeps at most 15 ancestors (one without keeps up to 16), and a message threads
  alone when the only link to a present relative is an identifier beyond either message's 16,
  where an ordinary account keeps every identifier, so the difference shows only with long
  reference chains or extra identifiers; in the other direction, an ordinary account's SMTP
  delivery looks only at the 256 newest messages of its subject, so a reply whose only relative
  is older threads alone there, while a key account looks up the whole subject and joins it; a key account keeps one Message-ID value
  per message where upstream matches every value, so a reply naming a message's other
  Message-ID threads on an ordinary account but not on a key account (rare; found in this
  release's review, not in design 7.5; issue #101); a message deleted before any keyed mail request indexed
  it gives no threading evidence; a rebuild invalidates every JMAP Email sync state, and IMAP
  `CHANGEDSINCE` never surfaces a reassigned `EMAILID` or `THREADID`; every publishing pass adds
  an `Email/changes` update of a witness message, an unsolicited IMAP `FETCH (FLAGS ...)` with an
  unchanged MODSEQ and a HIGHESTMODSEQ that advances with no flag changed, and `Mailbox/changes`
  then reports every mailbox; a keyless `Email/set` update or destroy accepts a stale id prefix;
  `EmailSubmission/get` returns a submission's current `threadId`. Design section 7.5 lists the
  rest (the thread-window superset, the one-response bound between nodes, `Initial` rendering
  every message as created, the truncation gap case).
- Latency: a pass finishes its compaction and commit before the wait deadline is checked again,
  so a request can take longer than `ZA_SEARCH_WAIT_SECS`. The first content search after a
  restart or eviction reads the sealed content: on 30,000 messages, about 23 to 26 s in the
  report's optimized builds on one machine.

Release 6 (the [outcome](docs/superpowers/plans/2026-11-14-zero-access-plan17-outcome.md)'s
"Accepted residuals" holds the full list, and its "Decisions and remaining qualification" the
items still open for the project owner):

- **#91:** moving an event into a calendar does not take the UID lease, so a move that crosses a
  create of the same UID in that calendar leaves two events with one UID there.
- **#92:** a calendar too large for one transaction is refused until some of its events are
  deleted; FoundationDB, read replicas and the bootstrap store keep the chunked, unfenced
  delete, where a failed later chunk leaves the earlier chunks' events deleted, and the failure
  drops the whole delete's email, so outside and plain attendees of the deleted events get no
  cancellation or decline; and a delete refused after its internal delivery leaves local key
  accounts holding a cancellation (or a key organizer holding a decline) for an event that
  still exists: until a retry deletes it (and delivers another), or for good when the event was
  moved out of the calendar meanwhile, since the retry no longer deletes it.
- **#93:** a sync paged from the initial state can end below a sharing change, and its next call
  is refused until an event is written, the client syncs in one page, or it takes its state from
  `CalendarEvent/get` or `/query`.
- **#94:** a grant that commits between the boundary check and the read of the log's tip can
  leave a newly shared calendar's older events unsent until they change or the client resyncs.
- **#95:** hardening of the token record's issue; no product path reaches it.
- **#96:** an owner's alert-only or keyword-only JMAP edit rewrites the whole event through the
  converter: a property the converter does not carry would be dropped (none has been shown), and
  on a recurring event with attendees a change to one occurrence becomes an override, which reads
  as a scheduling change.
- One shared calendar that does not open fails a grantee's whole `Calendar/get` and its
  exact-origin `/changes`, so a browser client loses the sharer's whole account, not one
  calendar.
- An interrupted refresh leaves the superseded secret able to open the keys until the record's
  next write, the client's retry or the next sweep. A caller who is not the owner receives in
  `destroyed` the ids of the owner's events it may not see.
- An attendee's JMAP create of its own copy of an event sends no reply, and that copy does not
  schedule later edits. An event created without a `uid` whose commit fails after its
  invitations went out is retried by the client under a new UID. A reminder's push names a
  positional alert id. Acknowledged or snoozed alarm state is lost with by-value default alerts.
- On a cluster (unqualified): a node that missed the invalidation of a vault write that leaves
  the credentials unchanged refuses the account's uncached Bearer requests until another
  invalidation of the account reaches it; and where nodes' clocks differ, a node whose clock lags
  can refuse a token whose entry the writing node did not store, and the client refreshes.

Release 5, carried over (the `0.3.0-beta.1` entry has the detail):

- **#77:** mail is submitted inside the user's request after the commit; a client that
  disconnects before the last submission loses the mail not yet queued, and nothing is retried.
- **#78:** a mailing list the registry no longer finds while a cached entry still names it is
  mailed, and SMTP can deliver to a catch-all that is a key account.
- **#72:** mail-borne invitation items have no authenticity, so an operator who can write to the
  store can forge one (decision 11).
- **#70:** an invitation between key accounts sends no notification email.

Pending gates:

- native client verification: no results recorded for any release, this one included; the
  [manual checklist](docs/zero-access/manual-checklist.md) has sections for release 7's search and
  threads and for release 6, whose reference-client checklist has not been run;
- the release 5 real-provider round (Gmail, Outlook.com and iCloud): pending;
- a clean-context whole-branch review, carried over from `0.2.0-beta.1`: pending. The 1.0.3 merge
  had a clean-context review of the merge alone, and releases 7 and 6 had whole-branch reviews
  of their own branches.

Not qualified: SQL and FoundationDB stores, multi-node execution, platforms other than the one
named above, upstream's private test suites and the external protocol suites (Dovecot imaptest,
Fastmail's JMAP test suite, Apple's CalDAVTester).

### Verification and outstanding gates

Upstream `v1.0.3` merge (PR #82; the
[merge outcome](docs/superpowers/plans/2026-11-05-upstream-1.0.3-merge-outcome.md) records every
command):

- The local matrix spans several tested commits. Unit rows (598, 16, 6 and 238 tests), the
  zero-access suite in both blob modes with cache swap, and the CalDAV, IMAP, JMAP, store,
  telemetry and system suites passed. The final SMTP aggregate on `55bb9ca67` recorded 75
  passed, 1 failed and 3 ignored; one exact-name retry of `smtp::reporting::analyze::report_analyze`
  passed, and the failure's cause is undetermined. Ten mutations each failed at the intended
  assertion. Product build, identity, the independent CalDAV client (plain 68 passed, 1 skipped
  and 3 xfailed; key 74 passed, 1 skipped and 2 xfailed) and all nine fresh-store restart
  readbacks passed on `55bb9ca67`, on macOS.
- CI: the manual run on `e17dabff8`
  ([run 42157308403](https://github.com/jaylaney/stalwart/actions/runs/42157308403)) failed its
  inventory gate, which the follow-up in the merge outcome addressed. On `b95a3523f`, the
  pull-request run ([run 42238580646](https://github.com/jaylaney/stalwart/actions/runs/42238580646))
  and the manually dispatched run
  ([run 42238569390](https://github.com/jaylaney/stalwart/actions/runs/42238569390)) passed every
  job, the test retention gates included. The pull request's last commit, `7a7b1a344`, changed
  only `CLAUDE.md` and the merge outcome, and its runs were cancelled. The push run on the merge
  `f148b2973` ([run 42254168744](https://github.com/jaylaney/stalwart/actions/runs/42254168744))
  passed every job that runs on push.
- A clean-context review of the merge found no correctness or security issue and one minor
  finding, fixed in the pull request.

Release 7 (PR #84; the [qualification report](docs/superpowers/plans/2026-11-07-zero-access-plan16-outcome.md)):

- PR #84's final head `82b8984aa`: the pull-request run
  ([run 43621111083](https://github.com/jaylaney/stalwart/actions/runs/43621111083)) passed every
  pull-request job, and the full manual run
  ([run 43621127921](https://github.com/jaylaney/stalwart/actions/runs/43621127921)) passed every
  job, both zero-access blob modes and the test retention gates included: the inventory gate
  passed, 795 tests; coverage, full 23,989 / 24,843 covered lines and runtime-only 13,258 /
  13,973, against the 0.16.27 baseline's 17,821 / 18,490 and 10,646 / 11,246. The push run on
  the merge `67ca91226` ([run 43647276566](https://github.com/jaylaney/stalwart/actions/runs/43647276566))
  passed every job that runs on push. The weekly scheduled run on `main` at `ade47a9ae`
  ([run 43912394102](https://github.com/jaylaney/stalwart/actions/runs/43912394102)) passed
  every job except the test retention gates, whose coverage net stopped when upstream's
  `smtp::queue::retry::queue_retry`, a listed timing-flaky test, failed on its attempt and on
  its one rerun; the gates were not reached in that run. They passed later on release 6's
  branch, which contains that source (below).
- The report records the local benchmarks and three mutation campaigns, with the limits above.
  Two independent whole-branch reviews found three issues in an earlier head, which were fixed
  and re-reviewed; later checkpoints had fresh whole-branch or scoped source reviews.

Release 6 (PR #90; the [outcome](docs/superpowers/plans/2026-11-14-zero-access-plan17-outcome.md)):

- PR #90's final head `f559febdb`: the pull-request run
  ([run 44924682624](https://github.com/jaylaney/stalwart/actions/runs/44924682624)) passed every
  pull-request job: style and the gates' workflow check, unit tests, upstream's suites, the
  `Test` job, the zero-access suites, the product build with the release verifier, the
  independent CalDAV client and the fresh-store restart gate.
- The test retention gates passed on `8e05a6b44` in the manually dispatched
  [run 44804659603](https://github.com/jaylaney/stalwart/actions/runs/44804659603), which passed
  every job: the inventory gate passed; coverage, full 30,776 / 31,866 and runtime-only
  17,641 / 18,583 covered lines, against the same baseline. `8e05a6b44` precedes one product
  change with its tests (an access entry expired at its write is not stored), the key-mode JMAP
  step's one rerun and the gates job's Rust 1.99.1 pin.
- On `f559febdb` itself, the manually dispatched
  [run 44924688662](https://github.com/jaylaney/stalwart/actions/runs/44924688662), the gates
  job's first with Rust 1.99.1, concluded as failed in two jobs, and every other job passed (the
  `Test` job's SMTP step included). Upstream Suites failed in its Automation Tests step at
  upstream's DKIM selector assertion (`tests/src/automation/dkim.rs:89`: the selector's
  timestamp one second off, `…269` against `…268`), the timing flake recorded in `CLAUDE.md`.
  In the Test Retention Gates job the coverage net's SMTP step failed: upstream's
  `smtp::queue::retry::queue_retry` failed on its attempt and on its one rerun at
  `tests/src/smtp/queue/retry.rs:238` (retry delays measured in whole seconds as `[1, 2, 2]`
  against `[1, 2, 3]`, under the slower coverage-instrumented build). The job's later steps ran
  on the net's profiles and both gates passed: the inventory gate, 795 tests; coverage, full
  31,455 / 32,488 (96.82%) and runtime-only 18,353 / 19,232 (95.43%) covered lines, 142 and 147
  lines above the 0.16.27 baseline's fractions. The failed jobs were not rerun.
- Locally (macOS, RocksDB, one node, debug test builds): the final matrix's 32 rows on
  `a5e45dc2f`, 30 passing on their first attempt and 2 after reruns, then validations on later
  sources; the 28-row mutation sample, all killed, and four further rows, of which one survivor
  gained a stronger test that kills it and one, in an upstream function, is an accepted
  residual. The final whole-branch review (five lenses)
  found no Critical finding and six Important ones, five fixed and one decided with no code
  change; four pull-request review rounds and a second whole-branch pass followed, and no
  Critical or Important finding remains open.

On `main` and for this release:

- `04145d586`, PR #90's merge, has the same tree as `f559febdb` (`dd61984b6f`), so the runs
  above test its code. Its push run
  ([run 44933215636](https://github.com/jaylaney/stalwart/actions/runs/44933215636)) passed every
  job that runs on push.
- PR #97 then merged (`b7c83694a`). It changes only test code: the shared integration-test
  listeners bind to loopback (`tests/src/utils/server.rs`, `tests/src/system/mod.rs`,
  `tests/src/automation/mod.rs`); no product source changed after `04145d586`. Its pull-request
  run on `f25b6789c`, the same tree as `b7c83694a` (`d7226eff7b`),
  ([run 44936128452](https://github.com/jaylaney/stalwart/actions/runs/44936128452)) passed
  every pull-request job, upstream's `Test` and `Upstream Suites` included, and the push run
  on `b7c83694a` ([run 44946844692](https://github.com/jaylaney/stalwart/actions/runs/44946844692))
  passed every job that runs on push. The test retention gates last ran on `f559febdb`, before #97's test changes.
- This release's preparation changes only documentation (`CHANGELOG-fork.md`, `README.md`,
  `docs/releases.md`, `docs/zero-access/manual-checklist.md`,
  `docs/zero-access/client-jmap.md`), which the Test workflow (`test.yml`) ignores
  (`paths-ignore`), so neither the preparation pull request nor its merge runs it; GitHub's CodeQL
  default setup still runs on them. The tagged source's product code is
  `04145d586`'s and its test code is `b7c83694a`'s.
- Release-preparation checks on the preparation tree (`04145d586` with this release's
  documentation changes): 9 release-metadata tests
  (`python3 -I scripts/tests/test_release_metadata.py`) and 25 product-verifier tests
  (`python3 -B -I -m unittest discover -s scripts/release -p 'test_*.py'`) passed, and the gates'
  workflow check (`python3 -I scripts/tests/ci_steps.py`, 0 differences) and tests
  (`python3 -I scripts/tests/test_gates.py`, 45 passed, 2 skipped) stayed green. No product was
  built during preparation. The tag's product build installs the runner's stable Rust, while the
  retention gates measure with Rust 1.99.1, so the two compilers can differ.

Gates beyond the automated runs, recorded after publication:

- the exact-source Linux shipping build and its verification, from the tag through
  `product-build.yml`, recorded in the published release notes (its run, result, checksums,
  attestation and measured GLIBC requirement), since this entry is part of the tagged source;
- native client verification, the release 6 reference client's checklist, the release 5
  real-provider round and the clean-context whole-branch review: pending (above).

## 0.3.0-beta.1 — 2026-11-04

Immutable tag: `fork-v0.3.0-beta.1`. A limited prerelease on upstream Stalwart `v1.0.1`, and
the first with release 5: invitations by email (iMIP) for key accounts. Published as a
GitHub prerelease, it claims no general client or production readiness until its pending
gates (below) pass.

### Provenance

- Fork source: the commit the tag `fork-v0.3.0-beta.1` names, resolved by
  `git rev-parse 'fork-v0.3.0-beta.1^{commit}'`. The published release notes and shipped
  `stalwart.provenance.json` record the exact full commit.
- Fully integrated upstream base: Stalwart `v1.0.1`, commit
  `23b69755cb3cd8ecd952fe0443c5ac067a506184`
  ([release notes](https://github.com/stalwartlabs/stalwart/releases/tag/v1.0.1)), unchanged
  from `0.2.0-beta.1` and pinned in `fork-release.toml` and in the verifier's
  `PINNED_UPSTREAM_TAGS`.
- No additional upstream backports. Upstream `v1.0.2` and `v1.0.3` are released and not
  integrated: none of their changes is in this preview.
- The published `fork-v0.1.0-beta.1`, `fork-v0.1.1-beta.1` and `fork-v0.2.0-beta.1` tags and
  artifacts retain their original identity.

### Changes

Release 5 (issue #18; [spec](docs/superpowers/specs/2026-11-02-zero-access-imip-design.md),
[outcome](docs/superpowers/plans/2026-11-02-zero-access-plan15-outcome.md)), merged through
PR #76. Before it, a key account's outside and plain local attendees were not invited
(`5.3`), and iMIP mail delivered to a key account had no calendar effect.

- **Key accounts invite by email.** A key account's CalDAV PUT or DELETE, as organizer or as
  attendee, mails the recipients that are not key accounts on this server: outside
  addresses, plain local accounts, local mailing lists and groups. A key attendee's reply to
  an outside or plain local organizer is mailed the same way, and a grantee writing in a
  shared calendar mails from the owner's address with `SENT-BY` naming the grantee. The
  message is upstream's iMIP email, composed in the user's own request and handed to the
  SMTP queue only after the change commits, never through the task queue. It carries no
  RSVP link or token: recipients answer from their own mail or calendar client, and the HTTP
  RSVP page stays refused for key accounts. Invitations between key accounts are unchanged:
  sealed internal delivery, never mail.
- **Key accounts are invited by email.** An iMIP request, update, cancellation, reply, ADD
  or REFRESH delivered to a key account becomes a pending item sealed to its public key,
  with the sender's address only inside the seal; the mail itself is stored sealed whatever
  happens to the item. The account's next session (any DAV request holding its keys)
  drains the item and applies upstream's merge, so the change appears at the client's next
  sync, not on arrival. Accepting or declining in the client mails the reply back.
- **What is mailed, and `SCHEDULE-STATUS`.** Each mailed recipient records `1.1` (sent,
  delivery not confirmed), written before the commit; a later emailed reply replaces it, as
  upstream does, with the reply's REQUEST-STATUS (or `2.0` when the reply carries none),
  and a submission that fails after the commit is traced and leaves `1.1`
  in place. Never mailed:
  - an active local key account's address, reached only through sealed internal delivery
    (one that cannot receive scheduling is not attempted, `5.3`);
  - any other address in a local domain that the registry does not resolve to a local
    account that is not a key account, or to a list (directly, or through the catch-all when
    the address has no entry): a disabled address, no entry and no catch-all, a catch-all
    that resolves to neither, an account the registry no longer finds. It records `5.1` and
    is traced;
  - an address that holds a contact pin in the sender's pin record, which records `3.8`.
    When the sender's pin state cannot be read or decided, every address that would be
    mailed records `5.1`. Mail recipients are never pinned.

  A failed recipient lookup records `5.1`. The drain's delegating fan-out and key pinning's
  redelivery stay internal: a recipient they would mail is not attempted (`5.3`).
- **Inbound requirements.** An item is written only past upstream's gate (scheduling
  enabled, not spam, the recipient allowed to receive scheduling) for a sender that passes
  the authentication upstream accepts (DMARC, or an authenticated local submission) and is
  not a local key account; key accounts reach each other only through internal delivery. A
  request for an unknown UID is kept only when the sender is a local principal, a contact
  card of the account holds the sender's address, or `autoAddInvitations` is on (off by
  default); any other method needs a UID the account already knows (an event or a pending
  item). PUBLISH, COUNTER and DECLINECOUNTER are not applied. Delivery decides from the
  message and keyless indexes only; the drain repeats every check it can with the keys.
- **REFRESH.** A listed attendee's emailed REFRESH is answered by mail with the event's
  current state, after the item's deletion commits, so one session answers each REFRESH
  item. It is withheld (the item deleted, no answer) for an address the rules above would
  not mail, for a pinned address, and when the sender's pin record is unusable; a store
  error reading the pin state leaves the item pending for a retry.
- **Integrity limit (Jay's decision 11, issue #72).** A mail-borne item, sealed to the public
  key, has no authenticity, so an operator who can write to the store can forge one, sender
  included. At the user's next session a forged REFRESH naming a listed attendee makes the
  drain mail the event's current full state through the SMTP queue, and a forged delegating
  reply adds delegates, of whom a key account is invited at once. Both need a listed
  attendee that is not a local key account, unless the store writer also remaps or disables
  a key attendee's address.
- **Traces.** The fork's iMIP events (delivery, the drain, each submission, withheld and
  dropped mail) carry ids, counts, SMTP stages, reply codes and fixed reasons, never an
  address or content; delivery events show `From` as `[redacted]`, and every SMTP
  transaction the outbound path opens is redacted as a key account's. The SMTP layer still
  traces a mailed invitation's envelope, as it does for every message.
- **Rate limiters.** The release 5 submitter's own local SMTP session is exempt from the
  inbound rate limiters (ruling O-1), because a temporary refusal there is never retried:
  upstream's default throttle (25 messages an hour per sender domain and recipient) would
  otherwise lose invitations and cancellations permanently. The exemption covers every
  inbound limiter of that session, an operator's limiter keyed on the sender included; the
  outbound queue's quota and throttles still apply.
- Ordinary accounts keep upstream's behaviour: every new path is a key account's.

### Operator actions

- Invitations to outside addresses leave through the server's SMTP routing (the
  deployment's relay, or delivery by MX), from the organizer's or replier's address. The
  deployment needs working outbound mail with DKIM signing and DMARC alignment for the
  sender's domain; otherwise providers junk or refuse the invitations, and no
  `SCHEDULE-STATUS` reports it (a remote bounce returns to the sender's mailbox as a DSN).
- A mailed invitation, update, cancellation or reply exists in plaintext in the SMTP queue
  (until relay or local delivery, and its blob until the next blob purge) and at the
  recipient, and the relay sees it in transit: the recorded amendment of release 2's
  invariant 12, whose task-queue part still holds. A deployment that delivers by MX instead
  of a relay keeps an undeliverable invitation in plaintext for the queue's whole retry
  window. A local list or group is mailed as upstream mails it, so the invitation crosses
  the queue in plaintext to every member, key members included (who receive it as sealed
  mail, with no calendar effect). SMTP logs record each mailed invitation's envelope, and
  `ItipMessageSent`'s queue id links it to the event's account and document ids: the
  operator learns who meets whom outside the server, and when.
- Inbound trust is upstream's: mail that fails DMARC (forwarded mail, many mailing lists)
  has no calendar effect, and with `mustMatchSender` off any authenticated local account can
  send iMIP from any address. Turning `autoAddInvitations` on lets any authenticated
  sender's invitation for an unknown UID create a pending item.
- Keep the calendar notification limit (`maxEventNotifications`) at its default, unlimited,
  or above the longest plausible logged-out stream: once every stored notification of an
  account is pending, new items are refused, a cancellation for a pending invitation
  included, which then exists only as sealed mail.
- Local domains fail closed: an address in a local domain that the registry does not
  resolve to a local account or list gets no invitation (`5.1`). That includes colleagues
  of a relay-enabled local domain hosted elsewhere, and unknown addresses of a local domain
  whose catch-all forwards to an outside mailbox.

### Compatibility and upgrade instructions

From `0.2.0-beta.1`: the store schema version is unchanged and release 5 changes no stored
layout (a mail-borne item is a release 2 pending item with one more sealed carrier), so a
store written by `0.2.0-beta.1` is expected to open in place. No automated test covers this
upgrade: CI's fresh-store job checks a fresh store only. Back up the complete database, blob
store and configuration, and verify the restoration, before replacing the binary. A
rollback to `0.2.0-beta.1` restores that backup and loses every write made after the
upgrade; this release does not claim that a `0.2.0-beta.1` binary reads data that
`0.3.0-beta.1` has written. Events created before the upgrade keep `5.3` for their outside
and plain local attendees, and nothing re-invites them automatically: they are mailed on
the next change that builds a message for them, and an edit that builds none (an alarm, a
change without scheduling content) mails nobody.

From `0.1.x`: not supported. Upstream 1.0.1 cannot open a `0.1.x` store, so moving is the
fresh-store rebuild the `0.2.0-beta.1` entry below describes. Fresh stores are supported.

The planned shipping configuration is unchanged: Linux `x86_64-unknown-linux-gnu` with
RocksDB, without the enterprise and `test_mode` features, requiring an `x86-64-v2` CPU.
Check the artifact's provenance for its highest referenced GLIBC symbol version and test
its runtime dependencies on the deployment host; it is not a static or broadly portable
musl build.

The existing key-account privacy and functionality limits remain described in the
[README](README.md) and [sharing API](docs/zero-access/calendar-sharing-api.md), and release
5's in its spec. Key-account JMAP calendars, calendar full-text search, re-sharing,
free/busy-only grants, cross-server sharing and key-account mail sharing remain refused or
deferred, as do free/busy with outside parties and iSchedule.

### Known limits and follow-ups

- **#77, mail lost on disconnect.** Mail is submitted inside the user's request, after the
  commit. A client that disconnects before the last submission loses the mail not yet
  queued (traced `ZA_IMIP_ABANDONED`, outcome unknown), and a crash there loses it
  silently. Nothing is retried: a lost cancellation or declining reply from a DELETE is
  permanent, where upstream's task retries for three days. Issue #77 moves submission to a
  detached task.
- **#78, a vanished list.** A mailing list the registry no longer finds while a cached entry
  still names it (a registry inconsistency, or a cluster node that has not applied the
  invalidation) is mailed, and SMTP delivers to the domain's catch-all, which can be a key
  account: a plaintext invitation through the queue. The product runs one node.
- **#72, DKIM provenance** of mail-borne items (decision 11, above). A REFRESH that claims a
  listed attendee's address also needs no store write when it arrives as an authenticated
  local submission or as DMARC-passing mail from a domain the server signs for; the DKIM
  check #72 plans would not cover the submission route.
- **#70, no notification email between key accounts.** An invitation between key accounts
  sends no email; the invitee sees it at its client's next sync.
- Temporary refusals are not retried: after the commit, a 4xx from the local SMTP layer (an
  address-verification error, the outbound queue's quota, a transient queue write failure,
  or an operator's milter, hook or script) loses that message permanently. The recipient
  keeps `1.1`, as it does after a local 5xx refusal.
- Each mailed recipient is one local SMTP transaction before the PUT or DELETE answers, so a
  write that invites many outside addresses answers more slowly.
- A retried inbound delivery writes one item per attempt (key accounts skip upstream's
  duplicate check), so a retried request can drain as a second change notification and a
  retried REFRESH can be answered more than once. A sender who knows a UID can fill a
  logged-out account's pending slots, evicting drained history.
- An alias removed between delivery and the drain makes the drain delete the pending
  invitation; the sealed mail remains.
- A mail-borne item, and the event imported from it, keep the outside organizer's `PRODID`,
  time zone ids and timestamps visible, beside release 1's visible fields.
- Inherited from upstream 1.0.1: DMARC is the basis of trust for outside senders; a request,
  ADD or cancellation must come from the organizer's own address (no `SENT-BY` assistant,
  no forwarded request); a reply is applied whatever its SEQUENCE; a key attendee invited at
  a sub-address cannot reply from it (`mustMatchSender` compares MAIL FROM exactly); no
  `SCHEDULE-STATUS` follows a mail's fate.
- In a domain whose external directory looks up recipients and which has a catch-all to a
  list or a plain account, the registry's classification can differ from SMTP's, and a key
  account's unsynchronized alias can be mailed in plaintext. The product's shape (key
  accounts on the internal directory) never has this.

### Verification and outstanding gates

The [release 5 outcome](docs/superpowers/plans/2026-11-02-zero-access-plan15-outcome.md)
records the local runs, mutation checks, rulings and residuals. In summary:

- PR #76's final head `95543bfd1`:
  [run 41852181302](https://github.com/jaylaney/stalwart/actions/runs/41852181302) passed
  every pull-request job: style and the gates' workflow check, unit tests, upstream's
  suites, the `Test` job, the zero-access suites, the product build with the release
  verifier, the independent python-caldav client and the fresh-store restart gate (the test
  retention gates run only weekly and on dispatch). The merge `92680dd3b` has the same tree
  as `95543bfd1`, and `81c23b37b`, where this release's branch starts, changes `CLAUDE.md`
  only.
- The test retention gates on `466f83d4e`, gates dispatch
  [run 41749193954, attempt 2](https://github.com/jaylaney/stalwart/actions/runs/41749193954/attempts/2)
  (attempt 1's runner ran out of disk during the build): the inventory gate passed, 795
  tests; coverage, runtime-only 13,180 / 13,898 covered lines against the 0.16.27
  baseline's 10,646 / 11,246, 23 lines of margin; full 23,052 / 23,859 against
  17,821 / 18,490, 56 lines of margin. An earlier dispatch had failed the runtime-only
  figure by one line, and the PR added tests for it. `466f83d4e` precedes the drain fixes
  of rulings P2-R and P2-R2 and their tests.
- Still running at preparation, and required before the tag:
  - the test retention gates on `95543bfd1`
    ([run 41852258119](https://github.com/jaylaney/stalwart/actions/runs/41852258119));
  - CI on `main` after the merge:
    [run 41858534411](https://github.com/jaylaney/stalwart/actions/runs/41858534411) at
    `92680dd3b`, and [run 41860172138](https://github.com/jaylaney/stalwart/actions/runs/41860172138)
    at `81c23b37b`.

  The published release notes record their results.
- Reviews: the final whole-branch review (three Opus lenses and a docs review); the
  PR-stage review (four Fable lenses and two PR comments, a fix wave and a scoped Fable
  re-review); the Codex re-review of `466f83d4e`, whose fixes (rulings P2-R and P2-R2) each
  had a scoped Fable review. No Critical or Important finding remains.
- Release-preparation checks, run at the preparation commit: 9 release-metadata tests
  (`python3 -I scripts/tests/test_release_metadata.py`) and 25 product-verifier tests
  (`python3 -B -I -m unittest discover -s scripts/release -p 'test_*.py'`) passed, and the
  gates' workflow check (`python3 -I scripts/tests/ci_steps.py`, 0 differences) and tests
  (`python3 -I scripts/tests/test_gates.py`, 43 passed, 2 skipped) stayed green. No product
  was built during preparation.

Gates beyond the automated runs, recorded after publication:

- the exact-source Linux shipping build and its verification, from the tag through
  `product-build.yml`;
- the release 5 real-provider round, the Gmail, Outlook.com and iCloud section
  ("Invitations by email (release 5)") of the
  [manual checklist](docs/zero-access/manual-checklist.md): pending;
- native client verification of release 5's flows: pending, with no recorded results on
  this release or on `0.2.0-beta.1`;
- a clean-context whole-branch review, carried over from `0.2.0-beta.1`: pending.

Not qualified: SQL and FoundationDB stores, multi-node execution, platforms other than the
one named above, upstream's private test suites and the external protocol suites (Dovecot
imaptest, Fastmail's JMAP test suite, Apple's CalDAVTester).

## 0.2.0-beta.1 — 2026-11-02

Immutable tag: `fork-v0.2.0-beta.1`. A limited prerelease on upstream Stalwart `v1.0.1`,
published as a GitHub prerelease. It claims no general client or production readiness until
its pending gates (below) pass.

### Provenance

- Fork source: the commit the tag `fork-v0.2.0-beta.1` names, resolved by
  `git rev-parse 'fork-v0.2.0-beta.1^{commit}'`. The published release notes and shipped
  `stalwart.provenance.json` record the exact full commit.
- Fully integrated upstream base: Stalwart `v1.0.1`, commit
  `23b69755cb3cd8ecd952fe0443c5ac067a506184`
  ([release notes](https://github.com/stalwartlabs/stalwart/releases/tag/v1.0.1)), fetched
  by URL and pinned in `fork-release.toml` and in the verifier's `PINNED_UPSTREAM_TAGS`.
- No additional upstream backports are recorded separately for this preview.
- The published `fork-v0.1.0-beta.1` and `fork-v0.1.1-beta.1` tags and artifacts retain their
  original identity.

### Changes

- Moves the fork from upstream `v0.16.27` to upstream `v1.0.1`. Every zero-access feature is
  carried over: key accounts, sealed calendars, invitations between key accounts, key
  pinning, calendar sharing and key-account mail at rest. Ordinary accounts take 1.0.1's
  behaviour.
- Field policy 2: an event's, task's or journal's `CLASS` is visible to the operator when its
  value is PUBLIC, PRIVATE or CONFIDENTIAL, so that sharers can hide events; any other value
  stays sealed.
- 1.0's object metadata (DAV dead properties, IMAP annotations, JMAP metadata) is refused for
  key accounts, and setup refuses an account that already holds some with 409 "account
  already holds object metadata".
- 1.0 indexes in a queue of its own. Setup refuses an account with queued index work (409
  "account has unfinished tasks"); after a transient failure an administrator resets the
  partition with `x:IndexQueueStatus/set`, and after a permanent one deletes the offending
  document or the account. A key account's mail search document now holds only its ids.
- Every account: an undecodable Bearer value is no longer quoted in authentication traces,
  and HTTP request traces print `[redacted]` for `Authorization`, `Proxy-Authorization` and
  `Cookie` headers. Every ordinary calendar write asserts that the account was not
  provisioned in the meantime, as ordinary mail writes already did.
- Setting up a key account revokes OAuth grants minted before it, and a key account refuses
  a grant token minted while it had no credential. The scheduling inbox's expiry no longer
  removes invitations the drain has not yet opened.
- A key account's sealed events and calendars use about 485 and 491 bytes more quota each
  than their plaintext, which 1.0 enforces at commit.
- Inherited from upstream 1.0.1 for ordinary accounts: a delegate is not invited when the
  delegator forwards the REQUEST (1.0 accepts a REQUEST only from the organizer), and
  calendar text search sees an event only once the asynchronous index has caught up. Key
  accounts' delegates are still invited, by the drain.
- The shipping binary's `--version` and help name the fork version, the upstream tag and
  commit, and the source commit, for example `Stalwart Server fork v0.2.0-beta.1 (upstream
  v1.0.1 23b69755cb3cd8ecd952fe0443c5ac067a506184; source <commit>; clean)`. The release
  verifier also checks that the upstream pin names its tag's commit, is an ancestor of the
  source, is the latest known upstream release in the source's history, and matches the
  internal crates' upstream version.

### Compatibility and upgrade instructions

Fresh stores only. Upstream 1.0.1 cannot open a store written by upstream 0.16 or by the
fork's `0.1.x` releases, and the fork adds no conversion: the server stamps a new store with
its schema version and edition and refuses any other. Stores from `0.1.x` are not
supported. Moving an installation from `0.1.x` means a rebuild on a fresh store: create the
accounts again, issue new setup tokens and enrol each key account again (new keys), and reload data through the clients. Keep the pre-cutover store
snapshot and the `0.1.x` binary for rollback; a rollback restores the old store as it was,
without anything written after the cutover. Back up the complete database, blob store and
configuration and verify restoration before any test.

The planned shipping configuration is Linux `x86_64-unknown-linux-gnu` with RocksDB,
without the enterprise and `test_mode` features, and requires an `x86-64-v2` CPU. Check the
artifact's provenance for its highest referenced GLIBC symbol version and test its runtime
dependencies on the deployment host; it is not a static or broadly portable musl build.

The existing key-account privacy and functionality limits remain described in the
[README](README.md) and [sharing API](docs/zero-access/calendar-sharing-api.md). Key-account
JMAP calendars, calendar full-text search, re-sharing, free/busy-only grants, cross-server
sharing and key-account mail sharing remain refused or deferred.

### Verification and outstanding gates

The [upgrade outcome](docs/superpowers/plans/2026-10-27-stalwart-1-upgrade-outcome.md)
records every command and result; its "Final record" section indexes them. In summary:

- The test matrix, the coverage net and the test retention gates (the design's C10 inventory
  and coverage gates) ran at `f135a9881`, every row green
  (documented SMTP flakes passed rerun by name). CI's `test.yml` at `2867bc24f`, the
  last code change before PR #69's review,
  [run 41432100574](https://github.com/jaylaney/stalwart/actions/runs/41432100574), passed
  every job: style and the gates' workflow check, unit tests (593 passed), upstream's suites,
  the zero-access suites in both blob-store modes and in plain and key mode, the independent
  python-caldav client (plain 68 passed, 3 xfailed; key 74 passed, 2 xfailed), the fresh
  store restart gate, and the test retention gates (full 21,599 / 22,335 and
  runtime-only 12,537 / 13,197 covered lines, each 72 and 44 lines above the 0.16.27
  baseline's fraction).
- PR #69's review found two port regressions (the setup-token request's concurrency slot,
  and a delegated PUT that could exceed the recipient limit), fixed with tests in
  `4a3dcde7f` and `7b088e8dc`. CI's `test.yml` on the PR's final commit `b69bb841b`,
  [run 41472049105](https://github.com/jaylaney/stalwart/actions/runs/41472049105), passed
  every job that runs on pull requests (the test retention gates run only weekly and on
  dispatch).
- The test retention gates were run again on `main` at `32d264cba`
  ([run 41484056674](https://github.com/jaylaney/stalwart/actions/runs/41484056674))
  alongside the release build, and passed with every other job of that run: 795 tests;
  full 21,639 / 22,374 and runtime-only 12,539 / 13,198 covered lines, each 74 and 45 lines
  above the 0.16.27 baseline's fraction.
- Release-preparation checks, rerun at the preparation commit: 9 release-metadata tests
  (`python3 -I scripts/tests/test_release_metadata.py`) and 25 product-verifier tests
  (`python3 -B -I -m unittest discover -s scripts/release -p 'test_*.py'`) passed. A local
  shipping-configuration build at `2867bc24f` (macOS arm64, not a release artifact) printed
  the expected identity and carried no enterprise or test feature.
- The final whole-branch review ran as four Fable lenses and two scoped re-reviews; no
  Critical or Important finding remains.

Gates beyond the automated runs, recorded after publication:

- the exact-source Linux shipping build and its verification, run from the tag through
  `product-build.yml`: passed
  ([run 41485088188](https://github.com/jaylaney/stalwart/actions/runs/41485088188)). The
  published release notes record the checksums, the attestation and the measured GLIBC
  requirement of the real artifact;
- the native client [manual checklist](docs/zero-access/manual-checklist.md): no recorded
  results; pending. Results from the `0.1.x` base are not evidence for this release;
- a clean-context whole-branch review: pending. The final review's four lenses were given
  the execution ledger and its rulings, which is not the clean context the plan specifies.

Not qualified: SQL and FoundationDB stores, multi-node execution, platforms other than the
ones named above, upstream's private test suites and the external protocol suites (Dovecot
imaptest, Fastmail's JMAP test suite, Apple's CalDAVTester).

## 0.1.1-beta.1 — 2026-10-29

Immutable tag: `fork-v0.1.1-beta.1`. This preview remains a prerelease.

### Provenance

- Fork source: the commit resolved by `git rev-parse 'fork-v0.1.1-beta.1^{commit}'`.
  The published release notes and shipped `stalwart.provenance.json` must record the exact
  full commit after release preparation is committed and tagged.
- Fully integrated upstream base: Stalwart `v0.16.27`, commit
  `aa47d8ef7bfba7200d3293198ac2592747b20e73`.
- No additional upstream backports are recorded separately for this preview.
- The published `fork-v0.1.0-beta.1` tag and artifacts retain their original identity.

### Changes

- Integrates the [.27 fixes](https://github.com/stalwartlabs/stalwart/releases/tag/v0.16.27):
  iTIP updates retain incoming timezone definitions for moved occurrences; OAuth accepts
  registered loopback redirects without a path; WebDAV `allprop` omits
  `DAV:current-user-principal`; IMAP `APPEND` and cross-account `COPY`/`MOVE` update
  `EmailDelivery` state, and combined `SEARCH RETURN` options retain all matching results
  where requested. Key-account refusal and sealed-data boundaries remain in force.
- Pins WebUI downloads to `v1.0.11` and checks the version before installation, avoiding
  incompatible WebUI updates on the .27 server base.
- Adds fork regressions for the .27 calendar/discovery changes and IMAP saved-search result
  combinations. Corrects system-test timing and upload-limit restoration fixtures while
  retaining their behavioral assertions.
- Uses the public repository's fork regressions and upstream suites for qualification;
  no private upstream test suite is required.
- Restores clean-checkout retrieval of OpenTelemetry from the original project's Git URL,
  pinned to the same `ae66e97b140f70e477ab710686aafce665cc2f8b` source commit.
- Adds no stored-format migration or legacy reader. The upstream 1.0 port remains deferred;
  this .27 integration does not promise ongoing upstream .16 maintenance or support.

### Compatibility and upgrade instructions

This experimental preview is intended for disposable installations. Existing-data upgrades
from `0.1.0-beta.1`, upstream production installations or unpublished fork builds have not
been qualified. Before testing existing data, back up the complete database, blob store and
configuration and verify restoration. Keep the matching pre-upgrade backup: replacing only
the binary is not a rollback guarantee. Unchanged stored layouts and formats do not establish
cross-version archive compatibility or a supported upgrade/rollback contract.

WebUI downloads remain pinned to `v1.0.11`; use the server's checked installer rather than
substituting a newer WebUI. The planned shipping configuration is Linux
`x86_64-unknown-linux-gnu` with RocksDB and excludes enterprise and `test_mode` features.
It requires an `x86-64-v2` CPU. Check the future artifact's provenance for its highest
referenced GLIBC symbol version and test runtime dependencies on the deployment host;
this is not a static or broadly portable musl build.

The existing key-account privacy and functionality limits remain described in the
[README](README.md) and [sharing API](docs/zero-access/calendar-sharing-api.md). This update
establishes no new backend, cluster, platform or native-client qualification.

### Verification and outstanding gates

The [upgrade outcome](docs/superpowers/plans/2026-10-27-stalwart-01627-upgrade-outcome.md)
records historical public-suite commands, results, source/artifact identities, bounded retries
and retained skip/xfail policy. Those results retain their named source identities and do not
establish execution against the future release commit or its shipping artifact.

Fresh release-preparation checks passed: eight release-metadata tests
(`python3 -B -m unittest discover -s scripts/tests -p 'test_release_metadata.py' -v`)
and thirteen product-verifier tests
(`python3 -B -m unittest discover -s scripts/release -p 'test_*.py' -v`).

All four [PR62 CI jobs](https://github.com/jaylaney/stalwart/actions/runs/40904792249)
passed at `7ba688c3051fe258eea15a9fcbcd68e6e594a8c9`. PR64 changes only duplicate-test
selection in the CI workflow; runtime sources, dependency versions/features and release
metadata are unchanged from that qualified source. Its
[CI run](https://github.com/jaylaney/stalwart/actions/runs/40917113519) at
`cce8d8d8f1d60a46242a156045479978f10b17f6` passed Test (including Directory) and Check
Style; remaining jobs were still running during release preparation. The dated changelog
is the only subsequent preparation edit. These CI results do not establish verification
of the future tagged shipping binary.

**TODO before artifact publication:** verify the exact tagged Linux shipping binary,
`--version`/help identity, clean source provenance, ELF CPU/GLIBC/runtime requirements,
checksums and build attestations. No Linux shipping artifact for this release commit has
been qualified. The product tag workflow builds and verifies artifacts after tag push and
uploads them to Actions; it does not create or publish a GitHub release.

Native Apple Calendar (macOS/iOS), Thunderbird, DAVx5 and mail-client qualification remain
pending under the [manual checklist](docs/zero-access/manual-checklist.md). SQL/other stores,
multi-node execution, additional platforms and production deployment remain unqualified.
This preview must remain a prerelease and does not claim general native-client or production
readiness.

## 0.1.0-beta.1 — 2026-10-26

Preview title: `release-0.1.0-beta-1`. Immutable tag: `fork-v0.1.0-beta.1`.

### Provenance

- Fork source: the commit resolved by `git rev-parse 'fork-v0.1.0-beta.1^{commit}'`.
  The published release notes and shipped `stalwart.provenance.json` record the exact full
  commit after release preparation is committed.
- Fully integrated upstream base: Stalwart `v0.16.26`, commit
  `18f4e9505027d790fd7e9fdad7f922be6719101d`.
- No additional upstream backports are recorded separately for this preview.

### Included fork behavior

- Zero-access account setup with password, recovery key, TOTP and app-password support.
- Private calendar events, tasks, journals and calendar metadata encrypted at rest, with
  decryption during authenticated CalDAV requests for ordinary clients.
- Local invitations between key accounts, pinned recipient keys, and owner/grantee view/edit
  calendar sharing, including delegated scheduling with `SENT-BY`.
- Key-account mail encrypted at rest. Contacts and files retain upstream storage behavior.
- Independent product version and full fork/upstream/source identity in operator-facing
  version/help/startup output. Internal crate compatibility versions remain upstream-based.
- Shipping Linux `x86_64-unknown-linux-gnu` build with RocksDB, binary/provenance checksums and
  build attestations. It excludes enterprise and `test_mode` features.

### Compatibility and upgrade instructions

This is the first experimental preview, intended for disposable installations. Upgrades from
upstream production installations or earlier unpublished fork builds have not been qualified.
Before testing existing data, back up the complete database, blob store and configuration and
verify restoration. Keep the matching pre-upgrade backup: rollback by replacing only the
binary is not supported or promised. The release preparation changes no stored layout or
cryptographic format; that does not establish an upgrade/rollback contract for the fork.

The GNU/Linux binary requires an `x86-64-v2` CPU. Consult its provenance for the highest
referenced GLIBC symbol version and test runtime library compatibility on the target host;
it is not a static or broadly portable musl build.

Key-account JMAP calendars, calendar full-text search, re-sharing, free/busy-only grants,
cross-server sharing and key-account mail sharing remain refused or deferred. The running
server receives credentials and handles plaintext during authenticated use; this is
server-side encryption at rest, not end-to-end encryption against a compromised server.
Revocation rotates future-write keys and does not reseal already readable data. See the
[README](README.md) and [sharing API](docs/zero-access/calendar-sharing-api.md) for the
remaining privacy and functionality boundaries.

### Verification and outstanding gates

The [calendar-sharing outcome](docs/superpowers/plans/2026-10-23-zero-access-plan14-outcome.md)
records earlier automated qualification at its named source commits. Those historical
results are not executions against this beta's release commit. The published beta notes
must record fresh release-preparation checks and the exact-source Linux shipping build,
with their commands/results and artifact checksums before publication.

Native Apple Calendar (macOS/iOS), Thunderbird, DAVx5 and mail-client qualification remain
pending under the [manual checklist](docs/zero-access/manual-checklist.md). SQL backends,
multi-node execution and production deployment are not qualified. This preview does not
claim complete native-client or production readiness.
