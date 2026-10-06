# Zero-access calendar: design spec

Date: 2026-10-06. Status: draft for review. Scope: release 1 of the zero-access
fork of Stalwart (`jaylaney/stalwart`, upstream `stalwartlabs/stalwart`,
base commit 3f657330, Stalwart 0.16.25).

## 1. Purpose

A hosted calendar service where event content is encrypted at rest with keys
only the user holds, while unmodified CalDAV clients (Apple Calendar,
Thunderbird, DAVx5) keep working. Mail, invitations, search and a web client
come in later releases; this spec lays the key hierarchy and the sealed storage
format they will build on.

### 1.1 In scope (release 1)

- Per-user key hierarchy with password, recovery key and app-password wraps.
- Account API (six JSON endpoints) in the fork. The web page that calls it
  lives in a separate repository and is not part of this spec.
- Master key cached per HTTP credential for CalDAV requests.
- Field-level encryption of calendar events, tasks and journals, and of
  calendar collection names, descriptions and colours.
- Decryption on every CalDAV read path.
- Feature gating so that no server-side code path ever needs a key it does
  not have.
- Tests, including a zero-access proof that scans the stores for plaintext.

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
  no migration of existing plaintext calendar data to sealed form.
- Accounts use Stalwart's internal directory. LDAP, SQL and OpenID
  directories are not supported for key accounts.
- Deployment starts from an empty data store. No upstream data layout is
  inherited.

## 2. Threat model and claims

**Claim.** The operator cannot read event content at rest or while the user
is logged out. Stolen disks, backups, database dumps, a rogue database
administrator, the cloud provider, and legal demands against stored data are
all defeated.

**Not claimed.** A compromised or compelled *running* server can capture the
password at login or the master key from memory during a session. Native
CalDAV clients send the password and expect plaintext back, so this is
inherent to the product, and the privacy policy must say so.

**What the operator can see** (the "visible" set, section 6): when events
happen and for how long, timezones, recurrence rules and exceptions, status
and transparency, alarm trigger times and whether an alarm is an email alarm,
UIDs, filenames chosen by clients (often UID-derived), how many calendars
and events a user has, their sizes, ETags and sync history, and account
metadata such as login times.

**What the operator cannot see.** Titles, descriptions, locations, geo,
URLs, attendees, organizers, categories, comments, attachments, conference
links, class, priority, colours, calendar names and descriptions, and any
extension property.

## 3. Key hierarchy

All material is generated when the user first sets their own password
(section 4). Nothing is generated from an admin-supplied password.

| Item | Derivation | Stored |
|---|---|---|
| salt | 16 random bytes | clear |
| password root | Argon2id(password, salt), 32 bytes; parameters stored with the salt, defaults 64 MiB, 3 passes, 1 lane | never |
| auth verifier | HKDF-SHA256(root, "za/v1/verifier") | SHA-256 of it, inside the registry password hash (section 4.3) |
| key-encryption key (KEK) | HKDF-SHA256(root, "za/v1/kek") | never |
| master key (MK) | 32 random bytes | wrapped under KEK |
| recovery key | 16 random bytes, shown once as 26 base32 characters in groups of 4 plus a check character | MK wrapped under HKDF(recovery key, "za/v1/rkek"); no salt, so a password change (which rotates the salt) leaves this wrap valid |
| app-password wrap | per app password: HKDF(secret, "za/v1/akek/" + credential id) | MK wrapped under it, keyed by credential id |
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
tool outside this repository and would be overwritten. The record has its own
explicit version byte and holds: state, salt and Argon2 parameters, the MK
wraps (password, recovery, app passwords by credential id), public key,
wrapped private key, and the setup-token hash while pending.

States: `PendingSetup` (setup token issued, no password, logins refused),
`Active`. Accounts with no record are *non-key accounts* and take every
existing Stalwart code path unchanged.

The `AccountCache` entry for an account carries "is key account" and the
public key, so gating checks (section 9) cost nothing.

## 4. Account API and authentication

### 4.1 Endpoints

All under `/api/vault/`, JSON request and response, served by the existing
HTTP server. CORS is allowed for the configured origin of the account page.

| Method and path | Auth | Request | Response |
|---|---|---|---|
| `POST setup-token` | bearer with a new admin-only permission (name chosen in planning) | account | token, expiry (default 7 days) |
| `POST setup` | none | username, token, password | recovery key |
| `POST password` | Basic, primary password | new password | ok |
| `POST recover` | none | username, recovery key, new password | new recovery key |
| `POST recovery-key` | Basic, primary password | none | new recovery key |
| `POST app-password` | Basic, primary password | description | app password, credential id |

- `setup` is single use: it verifies the token hash, requires state
  `PendingSetup`, generates everything in section 3, writes the registry
  password hash (4.3), and moves the record to `Active`. A used or expired
  token is refused.
- `password` derives the old root from the presented password, unwraps MK,
  then generates a new salt, re-derives, rewraps, and writes the new registry
  hash. Recovery and app-password wraps are untouched.
- `recover` unwraps MK with the recovery key, sets the new password as above,
  and returns a fresh recovery key. The old recovery key stops working.
- `recovery-key` returns a fresh key and invalidates the old one.
- `app-password` generates the secret exactly as the registry does today,
  wraps MK under it, stores the wrap by credential id, and creates the
  registry credential. Revocation uses the existing registry path; a wrap
  whose credential no longer exists is dead and is pruned on the next record
  write.
- Every state change triggers the existing `CacheInvalidation::AccessToken`
  for the account, which drops cached keys cluster-wide (section 5).
- All endpoints pass through the existing authentication failure delay and
  fail2ban accounting. Setup and recover count against the IP and username.

### 4.2 What is refused for key accounts

- Admin password set or reset through the registry (`Account/set` with
  credentials, and the `AccountPassword` singleton). Error text points at the
  account API. Without this, a reset would orphan the master key.
- App-password creation through the registry. Same reason.
- Bearer, OAuth and API-key logins on calendar paths (no key available).
  Non-calendar admin operations by an admin bearer token are unaffected.
- Master-user and impersonation access to key accounts' calendars.
- Moving the account to an external directory.

### 4.3 Password verification

The registry password credential for a key account holds a hash with a new
prefix, `$za$`, carrying the Argon2 parameters, salt and SHA-256 of the
verifier. The existing `verify_secret_hash` learns this prefix, so every
login path in Stalwart (HTTP today, IMAP, POP3, SMTP and ManageSieve later)
verifies a key account's password with no further change. A sibling function
returns the derived KEK on success, and the HTTP auth path uses it so that
one Argon2 run both verifies the password and unlocks the master key.

Two-factor authentication stays as is and runs before the vault code.

## 5. Request-time key cache

Stalwart's HTTP layer already caches successful authentications
(`HttpAuthCache`) keyed by the raw `Authorization` header, trusts a hit
without re-verifying, expires entries a fixed time after insertion (the
access-token expiry setting, default one hour), bounds the cache by weight,
and drops an account's entries on `CacheInvalidation::AccessToken`, which is
broadcast to every node. Release 1 extends that cache instead of adding one:

- The entry gains an optional master key in a `Zeroizing` buffer, filled only
  for key accounts authenticated with Basic credentials. The weight function
  counts it.
- The cache key becomes a keyed hash (BLAKE3 or HMAC-SHA256) of the
  `Authorization` string under a secret generated at process start, so neither
  the password nor a reusable fingerprint of it sits in memory.
- Keys live only in process memory. They are never written to the shared
  lookup store, Redis or the data store, and every node derives on its own
  misses.
- On a miss for a key account, the single Argon2 run from 4.3 verifies the
  password and yields the KEK, MK is unwrapped, and both the auth result and
  the key are cached.

The per-request `AccessToken` wrapper (not the shared, account-keyed
`AccessTokenInner`) gains an optional `SessionKeys` value holding MK and the
derived EWK. The HTTP auth layer sets it; DAV handlers pass it explicitly into
every calendar read and write. Tokens built for background tasks have none.

## 6. Field policy

Policy version 1. Visibility is an **allowlist**; anything not listed is
sealed, including extension properties.

**Visible in event, task and journal components:** UID, DTSTART, DTEND,
DURATION, DUE, RRULE, RDATE, EXDATE, RECURRENCE-ID, SEQUENCE, STATUS, TRANSP,
DTSTAMP, CREATED, LAST-MODIFIED.

**Visible in alarm components:** TRIGGER, ACTION, REPEAT, DURATION. The
precomputed "is email alarm" flag is visible.

**Visible elsewhere:** timezone components in full; the VCALENDAR root's
PRODID, VERSION, CALSCALE and METHOD; the precomputed time ranges, alarm
triggers and base offsets; the resource filename(s), ETag, size, created and
modified times, and sync state.

**Sealed:** every other property of every component, including SUMMARY,
DESCRIPTION, LOCATION, GEO, URL, ATTENDEE, ORGANIZER, CATEGORIES, COMMENT,
CONTACT, RESOURCES, ATTACH, RELATED-TO, CLASS, PRIORITY, COLOR, CONFERENCE,
IMAGE, STRUCTURED-DATA, all `X-` properties, and alarm SUMMARY, DESCRIPTION
and ATTENDEE. Also sealed: the event's WebDAV display name and dead
properties, and a calendar collection's display name, description, colour
and dead properties.

The policy version is recorded with each sealed object. Moving a property
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
recurrence keys index components by position. Within each component, the
sealed properties are removed from `entries` and replaced by one property:

- `X-ZA-SEALED` on each component that had any sealed property. Its text
  value is base64 of: format byte, nonce, ciphertext. The plaintext is the
  rkyv serialization of the removed `ICalendarEntry` values (names,
  parameters and values intact, no text round trip), prefixed by its length
  and padded to a multiple of 256 bytes. Associated data: account id, UID,
  component index, policy version.
- `X-ZA-KEY` on the VCALENDAR root: policy version, wrap type (`mk` now,
  `pk` later for events written to the public key), nonce and the DEK
  wrapped under EWK. Associated data: account id, UID.

The event's `display_name` and `dead_properties` fields, when non-empty, are
sealed with the same DEK into a `X-ZA-EXTRA` root property and the fields
themselves are emptied.

The stored `size` is the size of the sealed serialization.

### 7.2 Calendar collections

A collection has no tree. Its `display_name` field holds a marker prefix
plus base64 of: policy version, wrapped DEK, nonce, ciphertext of the rkyv
serialization of {display name, description, colour, dead properties}.
Associated data: account id, collection document id. The `description`,
`color` and `dead_properties` fields are emptied. The collection's URL slug,
timezone, ACL list and per-user preference flags stay visible, since the
shared resource cache and routing need them. When all four sealed values are
empty, the field stays empty and no bundle is written.

### 7.3 Unsealing

`unseal_event(archived, keys) -> Archive<CalendarEvent>` unwraps the DEK,
opens each bundle, splices the entries back into their components, restores
the extra fields, and **re-serializes the owned struct with rkyv into a fresh
archive buffer**. Callers that take `&ArchivedCalendarEvent` keep their
signatures. This costs one serialization per unsealed event, which is small
against the cost of the request, and keeps the fork's diff at each read site
to one line. A collection has the matching `unseal_calendar`.

Any failure to open a bundle, an unknown policy version, a wrap type the
server cannot open, or an `X-ZA-*` property on a non-key account is an error
(section 10).

## 8. Write and read paths

### 8.1 Writes

- `PUT` of an event: parse, validate, compute time ranges and alarms on the
  plaintext tree as today (this also computes the email-alarm flag from the
  alarm's text, before sealing), then `seal_event` immediately before the
  store write. The no-change shortcut compares the incoming tree against the
  **unsealed** stored tree. A new DEK is generated for every write.
- `MKCALENDAR` and `PROPPATCH` on a collection: seal display name,
  description, colour and dead properties.
- `PROPPATCH` on an event: seal display name and dead properties.
- `COPY` and `MOVE` within the account: the record is copied as stored. If
  the copy changes the UID, it is unsealed and resealed (the session has the
  key). Across accounts: refused with 403 when either side is a key account.
- `DELETE`: unchanged. The cancel path that reads the event only runs when a
  schedule tag is set, which never happens for key accounts in release 1.

### 8.2 Reads

Every path that serializes an event or collection to a client calls the
unseal step after loading and before any use of the tree:

- `GET` and `HEAD`.
- `PROPFIND` and `REPORT` responses carrying `calendar-data`, with or
  without a property list, `expand` or `limit-recurrence-set`.
- `REPORT calendar-query`: the time-range prefilter runs on the visible
  resource cache; candidates are unsealed before the component, property and
  text filters run.
- `REPORT calendar-multiget`, `sync-collection`, and the owner's own
  `free-busy-query`.
- `PROPFIND` of a collection's display name, description and colour.

The in-memory resource cache (`DavResources`) copies only filenames, start
and duration from events and only slug, ACLs and preference flags from
collections. It is built without a key and needs none.

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
| Alarm email | generic: subject and body carry the start time, timezone and a link; no title, description, location, organizer or guests; recipient is the account address |
| Display alarm push | unchanged (already carries only ids) |
| Trace events that log a whole iCalendar (`dates.rs`, `query.rs`) | removed in the fork for all accounts |
| Backup and restore | copy sealed records as-is; no plaintext calendar content exists in any subspace, including the task queue |

## 10. Error handling

- Unseal failure (tampering, corruption, unknown version, missing key):
  the request fails with 500 for DAV and a logged error naming account,
  collection and document id but never any content. Nothing is deleted or
  rewritten. A multi-item report fails the single item with a 500 status
  element and continues.
- A calendar operation reaching the groupware layer for a key account without
  `SessionKeys` is a programming error surfaced as 403 with a distinct event
  type, so the zero-access test can assert it never happens on supported
  paths.
- Setup, recover and password endpoints return 401 for a wrong token,
  recovery key or password, 409 for a wrong state, and never say which of the
  inputs was wrong beyond that.
- Argon2 runs on the blocking pool as the existing hash verification does.

## 11. Testing

**Unit.** The key module: deterministic derivation for a given salt;
verifier and KEK independence (different labels, no shared bytes); every wrap
round-trips; wrong password, wrong recovery key, wrong app password and
tampered ciphertext fail cleanly; associated-data binding refuses a bundle
moved to another account, UID or component index. The sealing module:
against a corpus of real iCalendar files from Apple Calendar, Thunderbird,
Google export and DAVx5, seal then unseal reproduces every entry; visible
properties match the allowlist exactly; bundle sizes are on 256-byte
boundaries; component order and `component_ids` are untouched.

**Integration.** The `webdav_tests` suite (one function, sub-modules
`basic`, `put_get`, `mkcol`, `copy_move`, `prop`, `multiget`, `sync`, `lock`,
`principals`, `acl`, `card_query`, `cal_query`, `cal_alarm`, `cal_itip`,
`cal_scheduling`) runs unchanged against key accounts, except that `acl`,
`cal_itip` and `cal_scheduling` run against non-key accounts because those
features are gated. The harness gains a mode that provisions test accounts
through the setup-token flow. `put_get` is the byte-exact fidelity check. New
tests cover each endpoint, cache hit and miss, password change then read,
recovery then read, app-password login, bearer refused, admin reset refused,
sharing refused, cross-account copy refused, and the generic alarm email.

**Zero-access proof.** A test writes events and collections through CalDAV
with distinctive titles, descriptions, locations, attendees, calendar names
and extension values, then scans every data-store subspace, the blob store,
the search store and the task queue for each string. Any hit fails. It also
asserts that no `X-ZA-` property ever reaches a client response. It runs in
CI on every change.

**Manual checklist.** Apple Calendar on macOS and iOS, Thunderbird, DAVx5:
create, edit, move between calendars, recurring with exceptions, alarms,
delete, rename a calendar, change its colour, sync after offline edits,
change password and reconnect, log in with an app password.

## 12. Invariants for implementers

1. No stored struct changes layout. Ciphertext rides in existing fields.
2. Sealing never adds, removes or reorders components or `component_ids`.
3. Visibility is an allowlist. Unknown properties are sealed.
4. A sealed property never reaches a client. Every read site unseals or
   fails.
5. Background code never needs a key. If a path would, it is gated, not
   worked around.
6. Keys live in `Zeroizing` buffers with a silent `Debug`, only in process
   memory, never in logs.
7. Non-key accounts take unchanged upstream code paths.
8. Fork diff stays narrow: new modules for keys, sealing and the account
   API; one-line call insertions at read and write sites; gating checks at
   existing permission points.

## 13. Prerequisites

- A Rust toolchain is not installed on the development machine (no `cargo`,
  `rustc` or `rustup`). Install rustup and the Xcode command line tools
  before implementation; RocksDB builds from source.
- Tests require `STORE=RocksDb` (as CI uses) and `RUST_MIN_STACK=16777216`.
  The CalDAV suite is not in upstream CI and runs with
  `cargo test -p tests webdav_tests`.
- Two things still unverified at spec time and to be settled in planning:
  how calcard serializes an `X-` property with a text value (affects only the
  never-expected case of a sealed property leaking to a client), and whether
  the registry write path accepts the `$za$` hash prefix without a validator
  change.
