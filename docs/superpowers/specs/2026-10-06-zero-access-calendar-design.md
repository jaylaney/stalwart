# Zero-access calendar: design spec

Date: 2026-10-06. Status: revision 6, approved. Scope: release 1 of the
zero-access fork of Stalwart (`jaylaney/stalwart`, upstream
`stalwartlabs/stalwart`, base commit 3f657330, Stalwart 0.16.25).

Revision 2 incorporated the ten findings of the 2026-10-06 design review;
revision 3 its seven follow-up findings and four clarifications; revision 4
its two revision-3 findings and two clarifications
(`2026-10-06-zero-access-calendar-design-review.md`). Revision 5
(2026-10-07) records the plan 1 outcome: the account API as built, in
sections 4.1, 4.3, 5, 10 and 13, following the decisions in
`../plans/2026-10-06-zero-access-plan1-outcome.md`. Revision 6
(2026-10-07) records the plan 2 outcome, following the decisions in
`../plans/2026-10-06-zero-access-plan2-outcome.md`: the field-policy
amendments in section 6, what is visible by structure in section 2, the
carrier layout, quota rule and tolerance of plaintext in sections 7.1 and
7.3, the refusal event in section 10, and the one all-account routing change
in section 8.2. The 4.1 item scheduled for plan 2 (refusing `setup` and
`recover` for a disabled account or tenant) was implemented in plan 2.

## 1. Purpose

A hosted calendar service where event content is encrypted at rest with keys
derived from credentials only the user knows, while unmodified CalDAV clients
(Apple Calendar, Thunderbird, DAVx5) keep working. Mail, invitations, search
and a web client come in later releases; this spec lays the key hierarchy and
the sealed storage format they will build on.

### 1.1 In scope (release 1)

- Per-user key hierarchy with password, recovery key and app-password wraps.
- Account API (eight JSON endpoints) in the fork. The web page that calls it
  lives in a separate repository and is not part of this spec.
- A key cache with bounded residency for CalDAV requests, and an
  authentication cache that retains no credential.
- Field-level encryption of calendar events, tasks and journals, and of
  calendar collection names, descriptions, colours and custom timezones.
- Decryption on every CalDAV read path.
- Feature gating so that no server-side code path ever needs a key it does
  not have.
- Tests, including a leak regression test that decodes and scans the stores.

### 1.2 Out of scope (later releases, in order)

1. **Invitations between users of the service.** Attendee addresses stay
   encrypted; the server stores one random token per attendee next to the
   RSVP status, wraps the event key to each internal attendee's public key at
   invite time, and matches replies by token. Free-busy between users.
2. **Invitations by email (iMIP).** Outbound mail and reply processing.
3. **Mail.** Encrypt-at-rest for IMAP/JMAP mail, bearer and OAuth wraps.
4. **JMAP calendars and a web client.** JMAP calendar methods are gated off
   for key accounts in release 1.
5. **Search.** Per-user keyed index built while the user is online.

### 1.3 Assumptions

- Every product account is a key account from creation. Admin and test
  accounts without key material behave exactly like stock Stalwart. There is
  no migration of existing plaintext calendar data to sealed form, and the
  setup flow refuses accounts that already hold data (section 4.1).
- Accounts use Stalwart's internal directory. LDAP, SQL and OpenID
  directories are not supported for key accounts.
- Deployment starts from an empty data store. No upstream data layout is
  inherited.

## 2. Threat model and claims

**Guarantee.** The sealed fields listed in section 6 are unreadable from the
stored data without either the user's credentials (password, recovery key or
an app password) or a key that is resident in a server's memory. Nothing from
which a credential or key can be recovered is retained in memory after a
request ends, except the key-cache entry. That entry becomes unusable
fifteen minutes after the last request that used it, or sixty minutes after
its creation, whichever is sooner, and is physically removed and zeroed by
the next sweep, at most one minute later (section 5). A request already
running when its entry expires keeps its own copy until it finishes. This protects against stolen
disks and backups, database dumps, a database administrator, and demands for
stored data.

**Limits, stated alongside the guarantee.** The running server receives the
password on every CalDAV request and holds plaintext while it serves them. A
compromised or compelled running server can capture passwords or resident
keys. Native CalDAV clients make this inherent to the product. Three things
are distinct and must not be conflated in product copy: how long a
credential's authentication result is cached, how long a key is resident, and
whether the user's client is connected.

**Integrity.** The guarantee is confidentiality, not integrity, against an
operator who can write to the store. Such an operator can drop or swap
sealed fields and bundles silently (the server reads an absent carrier as
nothing sealed) but cannot read them. Tampering with a bundle that is
present is detected and fails the request (section 10).

**Visible metadata** (section 6): when events happen and for how long,
timezones (including the calculation rules of custom timezones), recurrence
rules and exceptions, status and transparency, alarm trigger times and
whether an alarm is an email alarm, UIDs, filenames chosen by clients (often
UID-derived), how many calendars and events a user has, the exact plaintext
size of each event, the type name of every component including `X-`
components, which components carry a sealed bundle and the size class of
each bundle (a multiple of 256 bytes), free text that sits in visible slots
(TZID values, `X-` values of STATUS and TRANSP, and `X-` parts of RRULE),
ETags and sync history, and account metadata such as login times.

**Sealed:** titles, descriptions, locations, geo, URLs, attendees,
organizers, categories, comments, attachments, conference links, class,
priority, colours, calendar names and descriptions, every extension property,
every extension parameter, and comments, names and extensions inside
timezone definitions.

## 3. Key hierarchy

All material is generated when the user first sets their own password
(section 4). Nothing is generated from an admin-supplied password.

| Item | Derivation | Stored |
|---|---|---|
| salt | 16 random bytes | vault record |
| password root | Argon2id(password, salt), 32 bytes; parameters stored with the salt, defaults 64 MiB, 3 passes, 1 lane | never |
| auth verifier | HKDF-SHA256(root, "za/v1/verifier") | SHA-256 of it, in the vault record |
| key-encryption key (KEK) | HKDF-SHA256(root, "za/v1/kek") | never |
| master key (MK) | 32 random bytes | wrapped under KEK |
| recovery key | 16 random bytes, shown once as 26 base32 characters in groups of 4 plus a check character | MK wrapped under HKDF(recovery key, "za/v1/rkek"); no salt, so a password change (which rotates the salt) leaves this wrap valid |
| app-password wrap | per app password: HKDF(secret, "za/v1/akek/" + credential id) | MK wrapped under it, keyed by credential id, with a publication state (section 4.1) |
| TOTP secret | as enrolled through the account API | vault record, in the clear like upstream's; the registry credential's own TOTP field stays empty for key accounts |
| keypair | X25519 | public key clear; private key wrapped under MK |
| event wrapping key (EWK) | HKDF(MK, "za/v1/events") | never; derived per request |
| data key (DEK) | 32 random bytes per event and per calendar collection | wrapped under EWK, stored with the object |

Wrapping and sealing use XChaCha20-Poly1305 with a random 24-byte nonce and
associated data that names the purpose and the account, so a wrap cannot be
replayed as a different wrap. The verifier and the KEK come from independent
HKDF labels, so the stored verifier reveals nothing about the KEK. All
primitives are crates already in the dependency tree: `argon2`, `hkdf`,
`sha2`, `chacha20poly1305`, `x25519-dalek`, `zeroize`, `base32`.

Key material in memory is held in `Zeroizing` buffers, has a custom `Debug`
that prints nothing, and never appears in logs, traces or error messages.

### 3.1 The vault record

Each key account has one vault record in the data store, keyed by account id
as a new principal field (next to the existing default-calendar field). It is
**not** a registry object, because the registry's Rust code is generated by a
tool outside this repository and would be overwritten.

The record is the **sole authority for every fact that decides whether a
login of a key account succeeds**: state, salt and Argon2 parameters,
verifier hash, TOTP secret, the MK wraps (password, recovery, app passwords
by credential id with publication state), public key, wrapped private key,
and the setup-token hash while pending. The registry holds only the marker
(section 4.3) and the hashes of app-password secrets, and no registry edit
can change the outcome of a login without a matching vault write. The record
has its own explicit version byte and a revision; every write is conditional
on the revision read (`assert_value`), so concurrent writers fail and retry
rather than overwrite. The revision is the account's **authentication
generation** (section 5), and because all login-relevant state is in the one
record, a verification that read the record at generation G verified exactly
the state of generation G.

States: `PendingSetup` (setup token issued, no password, logins refused),
`Active`.

### 3.2 Account classification

An account is a *key account* when its registry password credential holds
the marker `$za$` (section 4.3). Registry credential changes invalidate only
the access-token cache upstream, not the account cache, so the vault
endpoints emit `CacheInvalidation::Account` themselves after every write
that changes classification, the public key, or the authentication
generation (section 4.1). The `AccountCache` entry carries the flag, the
public key and the generation, all read from the vault record when the entry
is built.

A marker whose vault record is missing is treated as corruption: logins are
refused, the account is still classified as a key account, and an operator
has to intervene. This state cannot arise from the ordering in section 4.1;
it can only come from data loss.

Accounts with no marker are *non-key accounts* and take every existing
Stalwart code path unchanged.

## 4. Account API and authentication

### 4.1 Endpoints

All under `/api/vault/`, JSON request and response, served by the existing
HTTP server. The six user endpoints carry credentials in the request body
so that a TOTP code can accompany the password (Stalwart's Basic decoder
never carries one, and verification refuses a correct password without the
code when TOTP is enabled). The one admin endpoint, `setup-token`, is the
exception: it is authenticated by the standard `Authorization` header path
(Basic or Bearer) and requires the existing `SysAccountUpdate` permission;
no new permission is introduced. A tenant-bound caller can only target
accounts of its own tenant, and any other account is reported as not found.

CORS is driven by the environment variable `ZA_ACCOUNT_PAGE_ORIGIN`. When it
is set, every `/api/vault/*` response and the `OPTIONS` preflight carry that
value as the allowed origin, with `Vary: Origin`, regardless of the
request's `Origin` header; the browser performs the comparison. The
preflight allows `POST` and `OPTIONS` with the `Content-Type` and
`Authorization` headers, and credentials are never allowed. When the
variable is unset no CORS headers are sent. On vault responses the
configured origin wins over a permissive `*` origin; other routes are
unaffected.

| Method and path | Request body | Response |
|---|---|---|
| `POST setup-token` (admin) | account | token, expiry (default 7 days) |
| `POST setup` | username, token, password | recovery key |
| `POST password` | username, password, totp (optional), new password | ok |
| `POST recover` | username, recovery key, new password | new recovery key, whether TOTP was removed |
| `POST recovery-key` | username, password, totp (optional) | new recovery key |
| `POST app-password` | username, password, totp (optional), description | app password, credential id |
| `POST totp` | username, password, totp (current code, required when enrolled), otp_auth (URL to enrol or replace, null to remove), confirm (a current code from the new secret, required with a URL) | ok |
| `POST app-password/revoke` | username, password, totp (optional), credential id | ok |

Verification for `password`, `recovery-key`, `app-password`,
`app-password/revoke` and `totp` is a fresh, full verification of the
primary password and TOTP; cached CalDAV authentication is never consulted.
App passwords are not accepted on these endpoints, and a new password that
parses as an app password is refused on `setup`, `password` and `recover`.
Each write is additionally conditional on the generation that was verified
for the request: if the vault record changed between verification and
commit, the write is refused and the client retries.

**State transitions.**

| From | Operation | To |
|---|---|---|
| no marker, no vault record | `setup-token` | `PendingSetup` |
| `PendingSetup` | `setup-token` | `PendingSetup` with a new token (old token invalid) |
| `PendingSetup` | `setup` | `Active` |
| `Active` | `setup-token` | refused, 409 |
| `Active` | `setup` | refused, 409 |

`setup` and `recover` do not pass through `Server::authenticate`, so they
check the account's and the tenant's enabled state themselves and refuse a
disabled account or tenant before touching the vault.

Initial issuance (first row) is further restricted to **eligible**
accounts: internal directory, no password credential of any kind, no
calendar collections or events, and (from the mail release) no mail. This
keeps the flow from converting an account that holds plaintext data.
Reissuance (second row) applies to an account that already carries the
marker credential and a `PendingSetup` record; the marker is expected there,
and the data checks are repeated.

**Write ordering for `setup-token`.** (1) Write the `PendingSetup` vault
record, conditional on no record existing. (2) Write the registry marker.
(3) Invalidate `Account` and `AccessToken` for the account. A crash after
(1) leaves a record without a marker; re-issuing the token finds the record,
rotates the token and writes the marker. A crash after (2) is recovered by
the next registry read, which invalidates nothing stale because the account
had no cache entries worth keeping; re-issuing is harmless.

- `setup` is single use: it verifies the token hash, requires state
  `PendingSetup`, generates everything in section 3, and commits the
  `Active` record in one conditional write. A used or expired token is
  refused; a concurrent duplicate fails its revision check and is refused.
- `password` derives the old root, unwraps MK, then generates a new salt,
  re-derives, rewraps, and commits the new salt, parameters, verifier hash
  and password wrap in one conditional write. Recovery and app-password wraps
  are untouched.
- `recover` unwraps MK with the recovery key, sets the new password as above,
  removes the TOTP secret, and commits a fresh recovery wrap in the same
  write. The old recovery key stops working. The response says whether TOTP
  was removed. The recovery key is treated as the stronger factor: a lost
  authenticator must not lock the user out of account management, and the
  page offers re-enrolment afterwards.
- `recovery-key` commits a fresh recovery wrap.
- `totp` verifies the password and the current code when enrolled, then
  commits the new TOTP secret (or its removal) to the vault record in one
  conditional write. Enrolling or replacing a secret also requires a
  confirmation code computed from the new secret, so that a mistyped or
  unscanned secret cannot be committed; a wrong confirmation code counts as
  a failed authentication. Nothing is written to the registry. The endpoint
  exists because the registry's own TOTP editing verifies the password
  against the stored hash, which for key accounts is the marker.
- `app-password` generates the secret exactly as the registry does today and
  runs a three-step publication: (1) commit the wrap to the vault record in
  state `Pending` with a creation time and a random publication id; (2)
  create the registry credential; (3) commit the wrap as `Published`,
  conditional on it still being the same `Pending` entry. If (2) fails, the
  wrap is removed, conditional on it still being that `Pending` entry. If
  (3) finds the entry gone, the registry credential is deleted and the
  request fails. **Orphan pruning**, which runs on any vault write, removes
  only `Published` wraps whose registry credential no longer exists and
  `Pending` wraps older than one hour. It never touches a fresh `Pending`
  wrap, which closes the race between publication and a concurrent password
  change. Credential ids are the registry's next id; creation is refused
  while a wrap that survived pruning still exists under that id, which can
  block creation for up to an hour after a failed publication. Ids are
  reused once the highest credential is deleted.
- `app-password/revoke` removes the wrap from the vault record in one
  conditional write (bumping the generation), then deletes the registry
  credential. If the registry delete fails, the credential is dead anyway:
  app-password logins require a `Published` wrap (section 4.3), so the
  stale registry entry can no longer authenticate, and orphan pruning on the
  next vault write removes nothing because there is no wrap left to prune.
  Revoking an id that has no wrap but still has a registry credential
  deletes that dangling credential and succeeds; revoking an id that has
  neither is refused. Revocation through the registry is refused for key
  accounts (section 4.2).

**Atomicity and crash behaviour.** Every state change is one conditional
write of the vault record, except the two-record sequences above, both of
which are ordered so that the vault record alone is never harmful. After any
successful vault write, the endpoint emits `CacheInvalidation::AccessToken`
(drops cached authentication and resident keys, locally and by the existing
cluster broadcast) and `CacheInvalidation::Account` (rebuilds the
classification, public key and generation).

All endpoints pass through the existing authentication failure delay and
fail2ban accounting. Setup, recover and the password-bearing endpoints count
against the IP and username; so does a `setup` attempt on an account that is
already active. Request bodies and responses of these endpoints are kept out
of the request trace events, so passwords, tokens, recovery keys and app
passwords never reach traces.

### 4.2 What is refused for key accounts

- Admin password set or reset through the registry (`Account/set` with
  credentials). Without this, a reset would orphan the master key.
- The `AccountPassword` self-service singleton entirely: password changes
  (same reason) and TOTP edits (it verifies against the marker and would
  always fail). Error text points at the account API.
- Every other registry edit of a key account's credentials: app-password
  creation (it could not wrap the master key), app-password or API-key
  deletion (it would bypass the vault generation, section 5), and TOTP
  edits. The vault endpoints are the only writers of credential state for
  key accounts, and they write the registry only for the marker and for
  app-password hashes.
- Bearer, OAuth and API-key logins on calendar paths (no key available).
  Non-calendar admin operations by an admin bearer token are unaffected.
- Master-user and impersonation access to key accounts' calendars.
- Moving the account to an external directory.

### 4.3 Password verification

The registry password credential of a key account holds the fixed marker
`$za$` instead of a hash. In `route_auth_request`'s internal-directory path,
a marker credential diverts to vault verification: load the record once,
derive the root with the stored salt and parameters, compare the verifier
hash in constant time, and apply the record's TOTP secret with the same
semantics as the existing MFA check (`MissingMfaToken` when TOTP is
enrolled and no code was presented). Every fact used by the decision comes
from that single read, and the generation returned is that read's revision.
On success the KEK and the generation are returned to the caller, so one
Argon2 run both verifies the password and unlocks the master key. Because this sits in the common routing
function, IMAP, POP3, SMTP and ManageSieve inherit it in later releases.
A key account is never authenticated against an external directory: the
request is refused before the directory is contacted, and the directory
synchronisation never replaces a password credential that holds the marker.

App-password logins keep the existing registry hash verification and then
read the vault record once and open the wrap stored under their credential
id, which must be `Published`; the generation returned is that read's
revision. A credential whose hash still exists in the registry but whose
wrap is gone cannot log in.

## 5. Caches, residency and generations

Two caches are involved on the HTTP path, and both change.

**Authentication cache.** Stalwart's `HttpAuthCache` is keyed by the raw
`Authorization` header value, which for Basic is the reversible base64 of
the password, and entries are inserted unconditionally after verification
and expire only when looked up again. For key accounts (and, since the
change is uniform, for all accounts) the key becomes a keyed hash (BLAKE3
or HMAC-SHA256 under a secret generated at process start) of the header
value. The entry additionally records the account's authentication
generation. The cache then retains nothing from which a credential can be
recovered; what remains is an opaque fingerprint usable only by the same
process.

**Key cache.** A separate `KeyCache` keyed by the same keyed hash. Entries
hold MK in a `Zeroizing` buffer, the generation, the insertion time and the
last-use time.

- Idle timeout 15 minutes (sliding), hard cap 60 minutes from insertion,
  both configurable. A sweep in the existing periodic housekeeping task
  removes expired entries at least every 60 seconds; lookups also refuse
  expired entries. Eviction zeroes the buffer.
- Bounded by entry count; least recently used is evicted under pressure.
- Keys are never written to the shared lookup store, Redis or the data
  store. Every node derives on its own misses, which it can always do for
  Basic credentials.

**Generations, and why invalidation alone is not enough.** A request can
read the vault record, spend a hundred milliseconds in Argon2, and meanwhile
a password change, a TOTP enrolment or an app-password revocation commits
and invalidates both caches. If the slow request then inserted its results,
the superseded credential state would keep working until those entries
expired. The fence rests on section 3.1: every fact that decides a key
account's login lives in the vault record, every change to such a fact is a
conditional write that advances the revision, and a verification reads the
record exactly once. A result carrying generation G therefore verified
precisely the state of generation G, and no registry-only edit can change
login outcome. On that basis:

- The generation verified by `route_auth_request` travels with the result.
- Before inserting into either cache, the HTTP layer compares that
  generation with the one in the (just rebuilt) `AccountCache`. On mismatch
  the request still succeeds, because it was correctly verified at the time,
  but nothing is cached.
- On a hit in either cache, the entry's generation is compared with the
  `AccountCache` generation; a mismatch discards the entry and forces full
  verification.
- Locally, the endpoint's invalidation runs before it responds, so the
  comparison is immediate. On other nodes, the window is the latency of the
  existing cluster broadcast, the same window upstream already has for any
  credential change; once the broadcast arrives, both the account cache and
  the entries are dropped.
- The shared, account-keyed access-token cache (`AccessTokenInner`) can
  still be rebuilt from a stale account snapshot by a slow request after an
  invalidation. That is upstream behaviour and affects permissions and
  aliases, not authentication: for key accounts no login decision depends on
  it, because the registry hash check for an app password is always
  followed by the vault read, and the primary credential is only a marker.

**Residency bound.** Expiry is deadline-driven at lookup and removal is
sweep-driven: an entry is refused by every lookup from the moment it passes
the idle timeout (15 minutes after last use) or the hard cap (60 minutes
after creation), and it is removed and zeroed by the next sweep, at most 60
seconds later. The authentication cache holds no credential at any time.

**Fingerprint residency.** An authentication-cache entry for a key account
lives exactly as long as its key-cache entry: the sweep, LRU eviction,
expiry at lookup, explicit removal and clearing all drop the matching
fingerprint. A fingerprint therefore never outlives the key it was verified
with.

**Cache loaders.** The account and access-token cache loaders are fenced by
a global account epoch, because the cache's `remove` is a no-op on a
pending placeholder: without the fence an in-flight load could republish a
stale classification after an invalidation. A residual window remains in
which a waiter on a placeholder is handed an account entry loaded just
before the invalidation; it is the size of a thread pre-emption and is
accepted for release 1.

**In-flight copies.** The per-request `AccessToken` wrapper (not the shared,
account-keyed `AccessTokenInner`) gains an optional `Arc<SessionKeys>`
holding MK and the derived EWK. It is set by the HTTP auth layer, passed
explicitly into every calendar read and write, dropped with the request, and
zeroed when the last reference drops. Eviction from the cache does not
truncate a request already in flight; the request's copy lives at most for
the request timeout. Tokens built for background tasks have none.

## 6. Field policy

Policy version 1. Visibility is an **allowlist at three levels**:
components, properties, and parameters of visible properties. Anything not
listed is sealed, including extension properties and extension parameters.
The policy applies to every iCalendar tree the server stores: events, tasks,
journals, and the custom timezone trees stored in collection preferences.

**Visible properties in event, task and journal components:** UID, DTSTART,
DTEND, DURATION, DUE, RRULE, RDATE, EXDATE, RECURRENCE-ID, SEQUENCE, STATUS,
TRANSP, DTSTAMP, CREATED, LAST-MODIFIED. UID is visible on every
component, not only these: also on VALARM, VTIMEZONE, STANDARD, DAYLIGHT,
VFREEBUSY and the VCALENDAR root, because events already expose their UIDs
and components are indexed by UID.

**Visible properties in alarm components:** TRIGGER, ACTION, REPEAT,
DURATION. The precomputed "is email alarm" flag is visible.

**Visible properties in timezone components, wherever they occur:** TZID,
LAST-MODIFIED, and on the VTIMEZONE component X-LIC-LOCATION and
X-MICROSOFT-CDO-TZID, because calcard resolves timezones by name and
clients send those names there; in their STANDARD and DAYLIGHT
subcomponents: DTSTART, TZOFFSETFROM, TZOFFSETTO, RRULE, RDATE. TZNAME,
TZURL, COMMENT and other extension properties inside timezones are sealed
like any other property.

**Visible properties on the VCALENDAR root:** PRODID, VERSION, CALSCALE,
METHOD.

**Visible parameters** on a visible property: VALUE, TZID, RANGE, RELATED,
and for RDATE and EXDATE also the period and date forms. Every other
parameter on a visible property, including any `X-` parameter, is removed
and sealed with its property index and parameter index so it is restored
losslessly.

**Also visible:** the precomputed time ranges, alarm triggers and base
offsets; the resource filename(s), ETag, plaintext size, created and modified
times, and sync state; a collection's URL slug, IANA timezone id or custom
timezone calculation rules, sort order, subscription flags, default-alert
offsets and ACL list.

**Sealed:** every other property of every component (SUMMARY, DESCRIPTION,
LOCATION, GEO, URL, ATTENDEE, ORGANIZER, CATEGORIES, COMMENT, CONTACT,
RESOURCES, ATTACH, RELATED-TO, CLASS, PRIORITY, COLOR, CONFERENCE, IMAGE,
STRUCTURED-DATA, all `X-` properties except the two timezone names above,
alarm SUMMARY, DESCRIPTION and ATTENDEE, timezone TZNAME and TZURL), every
non-listed parameter, the event's WebDAV display name and dead properties,
and a collection's display name, description, colour and dead properties.

The policy version is recorded with each sealed object. Moving an item
between sets is a new version applied to objects written from then on; old
objects are never rewritten without the user's key.

## 7. Sealed storage format

**Invariant: no stored struct changes layout.** Stalwart reads stored
archives with unchecked zero-copy access and has no schema-evolution
mechanism; a layout change would make records unreadable and would also
break every future upstream migration, which deserializes by upstream's
layout. All ciphertext therefore rides inside existing fields.

### 7.1 Events

The stored `ICalendar` tree keeps its component list, order and
`component_ids` exactly as parsed, because time ranges, alarms and JMAP
recurrence keys index components by position. Within each component, sealed
properties are removed from `entries`, sealed parameters are removed from
their properties, and one property is appended:

- `X-ZA-SEALED` on each component that had any sealed property or
  parameter, as the last entry of that component. Its text value is base64
  of: format byte, nonce, ciphertext. The plaintext is the rkyv
  serialization of a list of removals, each either
  (original entry index, `ICalendarEntry`) or (entry index, parameter index,
  `ICalendarParameter`), with names, parameters and values intact and no
  text round trip, prefixed by its length and padded to a multiple of 256
  bytes. Unsealing re-inserts each removal at its original index, so the
  restored component is identical to the parsed one, entry for entry and in
  order. Associated data: account id, UID, component index, policy version.
- `X-ZA-KEY` on the VCALENDAR root: policy version byte, wrap type byte
  (`0x01` is the master-key wrap now; a public-key wrap type follows later
  for events written to the public key) and the wrapped DEK. Associated
  data: account id, UID.

On the VCALENDAR root the carriers are `X-ZA-EXTRA` (when present) then
`X-ZA-KEY`, as the root's last entries. A carrier anywhere else in a tree (a
stray carrier) makes the object unreadable (section 10) rather than being
partly unsealed.

The event's `display_name` and `dead_properties` fields, when non-empty, are
sealed with the same DEK into an `X-ZA-EXTRA` root property and the fields
themselves are emptied.

The stored `size` of an event is the plaintext iCalendar length exactly as
upstream computes it, so HEAD, PROPFIND content-length and quota accounting
are unchanged. The sealing overhead inside the record (about 400 bytes per
component with sealed content, plus the `X-ZA-EXTRA` content) is not charged
to quota. Collections are different: upstream's size function counts the
stored preferences name, so a sealed collection charges its real sealed
bytes (about 500 bytes each). This asymmetry is accepted: event overhead is
bounded per component and collection counts are small. The exact event size
is visible metadata (section 2).

### 7.2 Calendar collections

A collection's user-facing name, description and colour live in the owner's
`CalendarPreferences` entry inside the `Calendar` record; the record's own
`name` is the URL slug and stays visible. The owner's preferences `name`
field carries a marker prefix plus base64 of: policy version, wrapped DEK,
nonce, ciphertext of the rkyv serialization of {name, description, colour,
the collection's dead properties}. The preferences `description` and
`color` fields and the collection's `dead_properties` are emptied.
Associated data: account id and purpose only, so copying or moving the
collection, which assigns a new document id, needs no resealing. (Swapping
two collections' sealed names requires database write access, which is
outside the threat model.) Sort order, flags and default alerts in the
preferences entry stay visible. Sharing is refused for key accounts, so only
the owner's entry exists. The bundle, which carries the collection's wrapped
DEK, is written whenever **any** ciphertext in the collection depends on
that key, including sealed removals in a custom timezone; its content part
may then be an empty set of values. It is omitted only when nothing in the
collection is sealed. Clearing the last ordinary value on a collection that
still has a sealed timezone therefore keeps the bundle.

**Custom timezones.** The preferences `time_zone` field may hold a full
iCalendar tree submitted as `calendar-timezone`. That tree is sealed under
the section 6 policy exactly like an event tree, with the collection's DEK:
an `X-ZA-SEALED` property per affected component (associated data: account
id, "calendar-tz", component index, policy version) and the removals
recorded with their indices. The calculation properties stay visible so
timezone resolution works without a key; the full tree is restored by
`unseal_calendar` before any `calendar-timezone` response.

### 7.3 Unsealing and the two views

`unseal_event(archived, keys) -> Archive<CalendarEvent>` unwraps the DEK,
opens each bundle, restores entries and parameters at their original
indices, restores the extra fields, and **re-serializes the owned struct
with rkyv into a fresh archive buffer**. Callers that take
`&ArchivedCalendarEvent` for reading keep their signatures, at the cost of
one serialization per unsealed event.

The unsealed archive is a **read view of the content only**. Its archive
version is meaningless and is never used. Response identity and conditional
metadata (ETag, `If-Match`, `If-None-Match`, sync tokens, schedule tag,
modified time) are taken exclusively from the stored sealed archive, whose
version hash is what upstream's ETag is built from. The index builder
asserts optimistic concurrency on the stored archive bytes and diffs old
index values from them, so every write path keeps the stored sealed archive
as `current` and uses the unsealed view solely for comparison, editing and
response bodies. `unseal_calendar` is the collection equivalent and covers
the custom timezone tree.

A bundle that is present but cannot be opened, an unknown policy version, a
wrap type the server cannot open, or a stray carrier is an error (section
10). An object in a key account with no carriers at all is plaintext: it
passes through unseal unchanged and is sealed by its next write, so accounts
converted with existing data keep working and are sealed incrementally.
(Plan 3 closes every writer that could store plaintext into a key account
without a key.) `X-ZA-*` properties on a non-key account are ordinary
properties and are neither checked nor removed; non-key accounts take
unchanged paths (invariant 9).

## 8. Write and read paths

### 8.1 Writes

- `PUT` of an event: parse, validate, compute time ranges and alarms on the
  plaintext tree as today (this also computes the email-alarm flag from the
  alarm's text, before sealing), then `seal_event` immediately before the
  store write. On update, the stored sealed archive is `current` for the
  index builder; the no-change shortcut compares the incoming tree against
  the **unsealed** view. A new DEK is generated for every write.
- `MKCALENDAR` and `PROPPATCH` on a collection: seal name, description,
  colour and dead properties into the owner's preferences entry, and seal a
  submitted custom timezone tree.
- `PROPPATCH` on an event: seal display name and dead properties.
- `COPY` and `MOVE` within the account: the record is copied as stored. If
  the copy changes the UID, the event is unsealed and resealed (the session
  has the key). Collections copy as stored (section 7.2). Across accounts:
  refused with 403 when either side is a key account.
- `DELETE`: unchanged. The cancel path that reads the event only runs when a
  schedule tag is set, which never happens for key accounts in release 1.

### 8.2 Reads

Every path that serializes an event or collection to a client calls the
unseal step after loading and before any use of the tree:

- `GET` and `HEAD`.
- `PROPFIND` and `REPORT` responses carrying `calendar-data`, with or
  without a property list, `expand` or `limit-recurrence-set`.
- `REPORT calendar-query`: the time-range prefilter runs on the visible
  resource cache; candidates are unsealed before the component, property,
  parameter and text filters run.
- `REPORT calendar-multiget`, `sync-collection`, and the owner's own
  `free-busy-query`.
- `PROPFIND` of a collection's display name, description, colour and
  `calendar-timezone`.

The in-memory resource cache (`DavResources`) copies only filenames, start
and duration from events and only slug, ACLs, timezone calculation data and
preference flags from collections. It is built without a key and needs none.

Calendar REPORTs (`calendar-query`, `calendar-multiget`, `free-busy-query`)
are accepted only under the `/dav/cal/` and `/dav/itip/` prefixes and answer
405 elsewhere, and a multiget href outside the requested account's calendars
answers 404 for that href. This applies to all accounts and is the one
deliberate change to non-key behaviour: the old routing let a REPORT reach a
key account's calendar through an ungated prefix.

## 9. Feature gating for key accounts

Every path that would need a key it does not have is closed, never left to
return ciphertext:

| Feature | Release 1 behaviour for key accounts |
|---|---|
| Calendar sharing (`ACL` method, JMAP `shareWith`) | refused on calendars owned by key accounts; a key account may still read calendars shared *to* it by non-key accounts or groups |
| Group-owned calendars, impersonation, master user | a key account's calendars are never readable through `is_member` shortcuts; impersonation and master-user sessions get 403 on calendar paths |
| Free-busy by another user, scheduling outbox, `Principal/getAvailability` | 403 for key accounts' calendars |
| CalDAV scheduling (RFC 6638) | scheduling permissions off; auto-schedule not advertised; a `PUT` with attendees stores them sealed and sends nothing; no schedule tag is ever set; the notification collection is never written |
| Inbound iMIP ingest from mail | skipped for key accounts (events would need the public key; release 2) |
| HTTP RSVP page | not routed for key accounts |
| JMAP calendars capability and methods | not advertised; methods return `accountNotSupportedByMethod` |
| Full-text indexing | `build_calendar_document` returns `NotIndexed` for key accounts; the stored search-hash index value is computed from the sealed tree and therefore from visible fields only |
| Alarm email | generic: subject and body carry the start time, timezone and a link; no title, description, location, organizer, guests or conference link; recipient is the account address |
| Display alarm push | unchanged (already carries only ids) |
| Trace events that log a whole iCalendar (`dates.rs`, `query.rs`) | removed in the fork for all accounts |
| Per-account event preferences (`CalendarEvent.preferences`, JMAP-only) | not sealed in plan 2; plan 3 traces which paths populate it for key accounts and seals or gates them |
| Backup and restore | copy sealed records as-is; no plaintext calendar content exists in any subspace, including the task queue |

## 10. Error handling

- Unseal failure (tampering, corruption, unknown version, missing key) or
  a stray carrier: the request fails with 500 for DAV and a logged error
  naming account, collection and document id but never any content. Nothing
  is deleted or rewritten. A multi-item report fails the single item with a
  500 status element and continues.
- A calendar request for a key account that arrives without `SessionKeys`
  (admin, impersonation, master user, a background path) is refused with
  403, logged as `SecurityEvent::Unauthorized` with details prefixed
  `zero-access:`, so tests can assert it never happens on supported paths.
  No new event type is added; event enums are generated upstream.
- A marker credential with no vault record refuses login with a distinct
  logged event (section 3.2).
- Setup, recover and the password-bearing endpoints return 401 for a wrong
  token, recovery key, password, TOTP code or TOTP confirmation code, and
  never say which input was wrong; 402 when TOTP is enrolled and no code
  was presented, so the page can ask for one and retry; 403 when the
  account or its tenant is disabled; 409 for a wrong state, an ineligible
  account, a failed revision check or a lost race (the client retries),
  with a short fixed reason in an `error` field. Every other failure uses
  the server's generic problem body, so clients branch on the status code,
  not on the body.
- Argon2 runs on the blocking pool as the existing hash verification does.

## 11. Testing

**Unit.** The key module: deterministic derivation for a given salt;
verifier and KEK independence (different labels, no shared bytes); every wrap
round-trips; wrong password, wrong recovery key, wrong app password and
tampered ciphertext fail cleanly; associated-data binding refuses a bundle
moved to another account, UID or component index. The sealing module:
against a corpus of real iCalendar files from Apple Calendar, Thunderbird,
Google export and DAVx5, plus synthetic cases with interleaved visible and
sealed properties, repeated properties, extension parameters on DTSTART,
comments and `X-` properties inside VTIMEZONE, and custom collection
timezones with comments: seal then unseal reproduces every entry at its
original index; visible properties and parameters match the allowlists
exactly; bundle sizes are on 256-byte boundaries; component order and
`component_ids` are untouched. The caches: a key-cache entry inserted and
never looked up again is gone after the idle timeout plus one sweep, with no
credential presented in between; the hard cap evicts an entry that is used
continuously; eviction zeroes the buffer; after eviction neither cache holds
anything from which the password, an app password or MK can be derived
(asserted by inspecting the cache contents); an entry with a stale
generation is discarded on hit; a result with a stale generation is not
inserted.

**Integration.** The `webdav_tests` suite runs in two configurations. The
**baseline** runs every sub-module unchanged against non-key accounts, as
upstream does, so that gated and intentionally changed behaviour is still
covered for ordinary accounts. The **key-account** run executes `basic`,
`put_get`, `mkcol`, `prop`, `multiget`, `sync`, `lock`, `principals`,
`card_query` and `cal_query` unchanged against key accounts, and runs
key-account variants of `copy_move` (cross-account copy and move refused
with 403, in-account copy and move succeed), `cal_alarm` (the email carries
start time and link and none of the summary, description or conference
canaries), `acl` (grants refused), `cal_itip` and `cal_scheduling` (not
offered). The harness gains a mode that provisions test accounts through the
setup-token flow. `put_get` is the byte-exact fidelity check. New tests
cover: each endpoint; each endpoint with TOTP enabled, including enrolment,
replacement and removal through `totp`, with the registry credential's TOTP
field asserted empty throughout; cache hit and miss; password change
then read; recovery then read; app-password login; bearer refused; admin
reset and the self-service singleton refused; sharing refused; collection
COPY to a new destination and over an existing one; repeated PUT and
PROPPATCH followed by DELETE with quota back at baseline; the setup-token
transition table including refusal for `Active` and for ineligible
accounts; a crash after the `PendingSetup` record and before the marker;
concurrent setup, password change, recovery and app-password creation (one
succeeds, the others get 409); the app-password interleaving in which a
password change runs between the pending wrap and the registry credential
(the returned app password must still decrypt); a simulated crash between
the pending wrap and the registry write; a verification paused across a
password change, a recovery, a TOTP enrolment and an app-password
revocation, asserting in each case that the superseded credential state is
not cached afterwards and that a later Basic request without the newly
required factor is refused; app-password revocation through the registry
refused; a timezone-only sealed collection round-trips, and clearing the
last ordinary value of a collection with a sealed timezone keeps the
timezone readable; provisioning observed from a node with a warmed account
cache; and the generic alarm email.

**Leak regression test.** A test writes events and collections through
CalDAV with distinctive titles, descriptions, locations, attendees, calendar
names, extension properties, extension parameters on visible properties,
timezone comments inside events and inside a custom `calendar-timezone`,
then copies a collection, then reads back every record in every data-store
subspace, decompresses and unarchives each one, and asserts the schema of
what it finds: only allowlisted properties and parameters in every tree,
only `X-ZA-*` carriers besides them, empty sealed fields, and no canary
string in any decoded record, index value, search-store document, blob,
task, or generated alarm email. A negative control plants a deliberately
unsealed event and must fail. The test also asserts that no `X-ZA-` property
ever reaches a client response. It runs in CI on every change. It is a
regression test for the sealing boundary, not a proof of the full
confidentiality claim.

**Manual checklist.** Apple Calendar on macOS and iOS, Thunderbird, DAVx5:
create, edit, move between calendars, recurring with exceptions, alarms,
delete, rename a calendar, change its colour, set a custom timezone, copy a
calendar, sync after offline edits, change password and reconnect, log in
with an app password, enrol and remove TOTP, all of it with TOTP enabled on
one account.

## 12. Invariants for implementers

1. No stored struct changes layout. Ciphertext rides in existing fields.
2. Sealing never adds, removes or reorders components or `component_ids`,
   and unsealing restores entries and parameters at their original indices.
3. Visibility is an allowlist at component, property and parameter level,
   applied to every stored iCalendar tree. Unknown items are sealed.
4. A sealed property never reaches a client. Every read site unseals or
   fails.
5. The index builder only ever sees the stored sealed archive as `current`,
   and all response identity comes from the stored archive. The unsealed
   archive is a content view.
6. Background code never needs a key. If a path would, it is gated, not
   worked around.
7. Keys live in `Zeroizing` buffers with a silent `Debug`, only in process
   memory, with active eviction, never in logs. No cache retains a
   credential or a reversible form of one.
8. The vault record is the sole authority for every fact that decides a
   key account's login (verifier, TOTP, app-password wraps); every write to
   it is conditional on its revision; verification reads it exactly once;
   and that revision is the authentication generation carried by every
   cached result. No registry-only edit may change login outcome.
9. Non-key accounts take unchanged upstream code paths, with one recorded
   exception: the calendar REPORT prefix check in section 8.2.
10. Fork diff stays narrow: new modules for keys, sealing, the key cache and
    the account API; one-line call insertions at read and write sites;
    gating checks at existing permission points.

## 13. Prerequisites

- The toolchain is Homebrew's keg-only rustup (`/opt/homebrew/opt/rustup/bin`
  must be on `PATH`); RocksDB builds from source.
- Tests require `STORE=RocksDb` (as CI uses) and `RUST_MIN_STACK=16777216`.
  The CalDAV suite is not in upstream CI and runs with
  `cargo test -p tests webdav_tests`.
- Still unverified at spec time and to be settled in planning: how calcard
  serializes an `X-` property with a text value (affects only the
  never-expected case of a sealed property leaking to a client); which
  periodic housekeeping task hosts the key-cache sweep; and what the
  resource cache stores for a custom collection timezone.
