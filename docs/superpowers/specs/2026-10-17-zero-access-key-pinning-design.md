# Zero-access calendar: key pinning for invitations design spec

Date: 2026-10-17. Status: draft for review. Scope: the follow-up to release
2 of the zero-access fork (jaylaney/stalwart) that release 2 spec section 2
records as a limit ("Key substitution"); GitHub issue #32.

The terms key account, master key (MK), event wrapping key (EWK), data key
(DEK), session keys, key cache, generation and visible by structure are
those of the release 1 spec, `2026-10-06-zero-access-calendar-design.md`
(revision 7), cited below as "release 1". The terms `pk` wrap, fan-out,
drain, pending item, internal recipient, token pair, UID lease and fence
are those of the release 2 spec,
`2026-10-11-zero-access-invitations-design.md`, cited below as "release 2".
Every release 1 invariant (its section 12) and every release 2 invariant
(its section 12, numbers 11 to 15) continues to hold; this spec adds to them
and relaxes none. The account page's endpoint contract is
`docs/api.md` in `jaylaney/circulo-account`, cited below as "api.md".

## 1. Purpose

Release 2 seals every invitation, update, cancellation and reply between
key accounts to the recipient's X25519 public key as the server serves it
from the store (release 2 section 3.1). A write-capable operator who
substitutes bob's stored public key reads every new invitation alice sends
bob, and alice's unmodified CalDAV client cannot notice. This spec closes
that gap for the contacts a sender has pinned: the sender's own session
remembers, under its MK, the key it sealed to for each address, and refuses
to seal to a different one.

### 1.1 Why the comparison happens at seal time

The attack is silent. The writer replaces bob's `VaultRecord.public_key`
with a key of its own; alice's fan-out seals the DEK to it; the writer opens the
`pk` wrap, rewraps the DEK to bob's real key and restores bob's public key.
bob's drain opens the item normally (`pk_unwrap` in
`crates/vault/src/keys.rs` derives the recipient key from bob's private
key), and nothing stored differs from an honest delivery. A check that
needs someone looking at the right moment catches almost nothing; the
comparison has to happen where the DEK is wrapped, in the sender's request
inside `za_fan_out` (`crates/groupware/src/calendar/fanout.rs`), before
any item is written.

### 1.2 In scope

- A per-account pin record, sealed under MK, mapping each contact address to
  the public key the sender has sealed to (section 4).
- The pin check in `za_fan_out`, for every sealed delivery: refusal with a
  `3.8` schedule status on a mismatch, trust on first use otherwise
  (section 5).
- A self-check that derives an account's own public key from its private
  key and repairs a substituted stored key (section 3.3).
- Four password-verified endpoints under `/api/vault/contacts` for the
  account page: list, look up, pin (with redelivery of refused messages)
  and forget (section 6).

### 1.3 Out of scope

- iMIP and external attendees (release 3). Pins apply only to internal
  recipients in release 2's sense.
- The page's contacts screen, in `jaylaney/circulo-account`: its own plan
  (section 12). This spec defines the endpoints; the server plan updates
  api.md there, and the page work follows against the updated contract.
- Key rotation. None exists: recovery and password changes keep the keypair
  (release 2 section 3.2).
- Signing invitations. A different trust root (section 11).

### 1.4 Assumptions

- Key change is rare and already destructive. The public key never changes
  for the life of a vault (release 2 section 3.2); the only way an address
  gets a new key today is a fresh setup, which abandons the old vault and
  everything sealed under it. "The key served for this address changed"
  therefore means the contact started over or the store was tampered with,
  and refusing until the sender confirms is reasonable.
- Release 1's and release 2's assumptions hold. No setting, permission,
  event type or environment knob is added (the registry schema and the
  event enums are generated, `CLAUDE.md`).

## 2. Threat model additions

**Claim.** Invitations, updates, cancellations and replies between key
accounts are confidential against a store writer who substitutes public
keys, for every contact the sender has pinned, including the first message
to a contact the sender pinned on the page before that message. A message
to a pinned address whose served key differs from the pin is not sealed to
the served key and no item is written. Everything stays inside release 1's
and release 2's trust model: sessions hold plaintext, native clients are
unmodified, and background code needs no key (release 1 invariant 6). Pins
are the sender's own data, sealed under the sender's MK.

**Limits, stated alongside the claim.**

1. **Trust on first use.** The first message to an unpinned address is
   sealed to the served key, as in release 2, and that key is pinned. If
   the store was substituted at that moment, the first message is exposed
   and the substituted key is pinned. The substitution becomes visible
   afterwards: once the writer restores bob's real key, or bob's own
   self-check repairs it (section 3.3), alice's next message to bob is
   refused with `3.8`, and alice's page shows the mismatch.
2. **Rollback.** Deleting the sender's pin record does not reopen first
   use: the account's first pin write rewraps its private key under
   associated data that marks it as pinned, and an absent or unusable pin
   record under a pinned wrap fails closed (section 4.4). Undoing the mark
   needs MK. What remains is a consistent rollback: the vault record
   restored to its pre-pin state (the legacy wrap) together with the
   deletion of the pin record, or the pin record alone rolled back to an
   older sealed value. Both are possible; integrity was never claimed
   against a store writer (release 1 section 2). The first also reverts
   every password, recovery-key, TOTP and app-password change made since
   the first pin, which the user notices.
3. **The sender's own record.** Pins protect what the sender seals. A
   substituted key on the sender's own vault record (so that messages
   others send to the sender are exposed) is caught by the sender's
   self-check, which detects and repairs; it does not prevent the messages
   sealed between the substitution and the repair, unless their senders
   had pinned the sender.
4. **Lookup reveals key accounts.** Any key account holder can look up any
   local key account's fingerprint by address (section 6.3), which confirms
   that the address is a key account; each lookup costs a full password
   verification under the anonymous-request rate limit. Accepted.
5. **The running server** holds plaintext and pins during a session, as in
   release 1, and can seal to any key it likes; pins defend against the
   store, not the process.

**Visible metadata added** (beyond release 2 section 2):

- The existence of an account's pin record, its size class (a multiple of
  256 bytes, section 4.1) and when it changes (its stored bytes change on
  every pin write).
- That the account's `private_key_wrap` changed once (its bytes and the
  revision bump), but not whether the wrap is pinned or legacy: the
  associated data is not stored.
- That a delivery did not happen: a writer who substituted a key and waits
  for an item learns that the sender had pinned the address.

**Not visible:** whether the wrap is pinned or legacy (though a pin
record's existence suggests the former), who is pinned, the pinned keys
and their fingerprints, the sources and times of pins, the withheld list,
`key_repaired_at`, and the `3.8` status itself, which lives inside the
sender's sealed event (release 1 section 6 seals ATTENDEE and ORGANIZER
properties with their parameters).

## 3. Fingerprints and the self-check

### 3.1 Fingerprint format

The display fingerprint of a 32-byte X25519 public key is the first 16
bytes of SHA-256 over the key, written as lowercase hex in 8 groups of 4
characters separated by single spaces, the same on both sides and in
every API response: `3f0a 9c41 07be 22d8 e1f0 5a6c 0b93 77e2`.

It is a new function in `crates/vault/src/keys.rs` (proposed
`display_fingerprint`), not the existing `keys::fingerprint`, a keyed
BLAKE3 hash under a per-process secret for authentication-cache keys
(release 1 section 5) that is neither stable nor meant for people.

### 3.2 An account's own fingerprint

An account's own fingerprint is always derived from its private key: unwrap
`private_key_wrap` under MK (pinned, then legacy associated data, section 4.4;
`za_open_private_key` in `crates/common/src/auth/vault.rs`), compute
`x25519_dalek::PublicKey::from(&StaticSecret)`, and fingerprint that; never
`VaultRecord.public_key`, which a store writer controls: read from the store on
both sides, alice's and bob's pages would agree on the writer's key.

### 3.3 The self-check

Where a session first unwraps its private key (`za_open_private_key`, which
`Server::za_private_key` wraps), the server derives the public key from the
private key and compares it with the `public_key` of the record it just read
(the same read). On a mismatch:

1. Trace under an existing event: the candidate is
   `SecurityEvent::Unauthorized` with a `zero-access:` detail (release 2
   section 6.5 uses it for a forged reply token), such as `zero-access:
   stored public key does not match the private key`, with the account id
   and no key bytes or fingerprints (section 13, item 1).
2. Set `key_repaired_at` in the pin record, creating the record if absent.
3. Rewrite the vault record's `public_key` from the derived key through the
   common commit helper (section 4.5), conditional on the revision read.
   When the wrap opened under the legacy associated data, the same write
   rewraps the private key as pinned (section 4.4): the record now exists,
   and one write keeps this to one generation bump. The authentication
   generation moves once, ending the account's cached logins and resident
   keys, the right consequence of a tampered record; the helper's
   `CacheInvalidation::Account` and `AccessToken` let other senders'
   fan-outs see the repaired key at once. A lost race is not retried; the
   next session checks again.

The check needs the private key, so it runs only in a request holding the
account's session keys. Today the drain unwraps the private key only when
a pass finds pending items (`drain_items` in
`crates/groupware/src/calendar/drain.rs`), but the target of a refused
substitution receives none (section 5). So every session's first drain
pass unwraps the private key and runs the check, items pending or not; the
cost is one vault record read and one X25519 scalar multiplication per
session, and release 2's suspension of the drain when the wrap does not
open is unchanged. The contacts endpoints (section 6) run the same check;
the page's "your fingerprint" comes from it.

## 4. The pin record

### 4.1 Location and format

The pin record is a new principal field beside the vault record,
`PrincipalField::ZeroAccessPins = 151` (`crates/types/src/field.rs`), keyed
like it (account id, `Collection::Principal`, document 0). As the comment on
`ZeroAccessVault = 150` requires of 150, 151 must never have been assigned
to any field (leftover bytes must not read as a record; section 13, item
2).

The stored value is one blob sealed with `seal` (`crates/vault/src/keys.rs`,
XChaCha20-Poly1305, `nonce || ciphertext`) under MK, associated data
`aad("pins", account_id)` (release 1's shape, new purpose `AAD_PINS`). The
plaintext is a length prefix and the rkyv `ZaPinRecord`, padded to a
multiple of 256 bytes as release 1 pads bundles (its section 7.1), decoded
with a checked rkyv read as `Extra` is (release 2 section 9). The version
is inside; nothing precedes the nonce.

| Struct | Field | Type | Meaning |
|---|---|---|---|
| `ZaPinRecord` | `version` | `u8` | `1`. Any other value makes the record unusable (section 4.4). |
| | `revision` | `u64` | Incremented by every write. Informational; the conditional write asserts the stored bytes (section 4.2), and the revision is no defence against rollback. |
| | `pins` | `Vec<ZaPin>` | One per address, unique by `address`. |
| | `withheld` | `Vec<ZaWithheld>` | Refused messages awaiting redelivery, unique by (`uid`, `address`). |
| | `key_repaired_at` | `Option<i64>` | Unix seconds of the last self-check repair (section 3.3). |
| `ZaPin` | `address` | `String` | The contact's address, lowercased, as the message names it (section 4.3). |
| | `public_key` | `[u8; 32]` | The pinned X25519 public key. |
| | `pinned_at` | `i64` | Unix seconds. |
| | `source` | `PinSource` | `FirstUse` (the fan-out pinned the served key) or `Confirmed` (the user pinned it on the page). |
| `ZaWithheld` | `uid` | `String` | The event's UID. The list has no cap: only the owner's fan-outs add entries, one per (`uid`, `address`). |
| | `address` | `String` | The refused recipient's address, lowercased. |
| | `since` | `i64` | Unix seconds of the first refusal. |

The structs have a silent `Debug` (they hold addresses).

### 4.2 Reads and writes

- **Read fresh** on every fan-out and endpoint call, never cached in
  `SessionKeys` (fan-outs are rare, the value is small, and a session cache
  would be stale across sessions and cluster nodes).
- **Writes are conditional** on the previous stored value, as
  `vault_write_store` writes the vault record (`AssertValue::Hash` of the
  bytes read, `AssertValue::None` when none was read). A lost assertion
  re-reads and retries once, re-running the decision against the fresh
  record (for the fan-out, every comparison of section 5.1); a second loss
  is that operation's pin write failure (section 5.4).
- **Only with the owner's MK**, in a request holding the owner's session
  keys (a DAV request's, or an endpoint's freshly verified ones).
- **Removed with the account.** `danger_destroy_account`
  (`crates/store/src/dispatch/store.rs`), called by the destroy-account task
  (`crates/services/src/task_manager/destroy_account.rs`), deletes the
  account's whole principal-property range, field 151 included, as it does
  the vault record's 150.

### 4.3 Keyed by address, not account id

Pins are keyed by the lowercased address as the message names it, never by
account id. The store writer also controls the address-to-account mapping
(`za_account_id_from_email` in `crates/common/src/cache/principals.rs`
resolves through the directory). Pointing bob's address at a fresh account
with a legitimate keypair of the writer's would pass a pin keyed by bob's
old account id as a first use of the new id. Keyed by address, the new
account's key differs from bob's pin and the message is refused.

The pin key and the lookup response use one normalization,
`to_canonical_address` (`crates/utils/src/lib.rs`, which lowercases), on
the `mailto:`-decoded address the fan-out sees. Two addresses that resolve
to one account (an alias, a sub-address) are pinned separately.

### 4.4 The pinned wrap and failing closed

Whether an account has pinned is carried by the associated data of its
private-key wrap, not by a stored field. Release 2 wraps the private key
under MK with `aad("private-key", account_id)` (`AAD_PRIVATE_KEY`); this
spec calls that the legacy wrap. The account's first pin write is followed,
in the same request (which holds MK and has or can unwrap the private key),
by a rewrap under `aad("private-key/pins", account_id)`
(`AAD_PRIVATE_KEY_PINNED`), the pinned wrap, written to
`VaultRecord.private_key_wrap` conditional on the revision through the
common commit helper (section 4.5). That is the one authentication
generation bump the feature costs an account, ever: it ends the account's
other sessions once, and later pin writes leave the vault record alone
(section 11). The vault record's layout and the wrap's length do not
change, so no stored struct changes (release 1 invariant 1).

**Reading the mark.** Every reader of the private key (the drain's
`za_open_private_key`, the fan-out, the contacts endpoints) tries the
pinned associated data first, then the legacy:

| Wrap opens under | Meaning | An absent or unusable pin record |
|---|---|---|
| pinned | the account has pinned | fails closed |
| legacy | the account never pinned | is legitimate: first use; an unusable record is treated as absent and overwritten |
| neither | tampered or corrupt | the drain keeps release 2's suspension (its section 10); the fan-out fails closed; the endpoints answer 409 `{"error": "vault record missing"}` |

A record is unusable when it does not open, does not decode, or decodes
with a version other than `1`. The fan-out needs the unwrap only when the
pin record is absent or unusable: a present record that opens is proof
enough, since a writer cannot forge a record sealed under MK. The common
path therefore costs no unwrap; when the fan-out does need the mark, it
reads the vault record fresh for it rather than trust a session's view.

**Fail closed.** Under a pinned wrap (or one that opens under neither), an
absent or unusable pin record makes the fan-out refuse every key-account
recipient with `3.8` (section 5.1) and write nothing to the pin record: no
first-use pins and no withheld entries, because any write would create a
fresh record and end the state. Messages refused in this state are not
redelivered; the sender's next edit after recovery sends them. The state
ends when the user, on the page, starts over (`contacts/forget` with a null
address writes an empty record) or pins a contact (`contacts/pin` writes a
fresh record holding that pin). Recipients that are not internal are
unaffected (not attempted, `5.3`, as today).

**Order of the first pin write.** The pin record first, then the rewrap. A
crash between the two leaves a record under a legacy wrap, never a pinned wrap
without a record (which would fail closed for no reason). It is healed by the
next reader that finds an opening record while the legacy wrap still opens and
rewraps: a fan-out that writes the pin record (it reads the mark whenever
it is about to write the record, not only when the record is absent), the
session's first drain pass (which, finding the legacy wrap, reads the pin
record), and the contacts endpoints. Until then a writer who deletes the record
reopens first use; the window lasts to the account's next session.

**Why a store writer cannot defeat it.** Turning a pinned wrap back into a
legacy one needs MK. Deleting the pin record alone fails closed, and so
does replacing it with another account's (its associated data names the
account) or with garbage. The remaining attack is the consistent rollback
of section 2, limit 2.

### 4.5 Vault writes outside the HTTP module

The rewrap is written by the fan-out (`crates/groupware`), the drain and
the self-check (`crates/common`) and the endpoints (`crates/http`), but
`za_commit` lives in `crates/http/src/api/vault.rs`. The revision bump, the
conditional write and the invalidation therefore move into a common helper
in `crates/common/src/auth/vault.rs`, which `za_commit` wraps and extends
with orphan pruning (which reads the registry). `CLAUDE.md`'s rule becomes
"every vault write goes through the common helper or `za_commit`"; the plan
updates `CLAUDE.md`.

## 5. The fan-out

### 5.1 The per-recipient decision

All sealed deliveries go through `za_fan_out`: the organizer's PUT create
and update and DELETE (`crates/dav/src/calendar/update.rs`, `delete.rs`),
the attendee's reply by PUT or DELETE, and the drain's delegating merge
(`crates/groupware/src/calendar/drain.rs`). The check lives there, once,
for all of them. For each key-account recipient whose address resolved to an
internal account and its served public key (`resolve_inner`, from the
recipient's `AccountCache` entry, `za_public_key`):

| Pin record | Private-key wrap | Pin for the address | Served key | Outcome |
|---|---|---|---|---|
| opens | not read | present | equal to the pin | deliver as today (`1.2`, or `5.1` on a write failure) |
| opens | not read | present | different | refuse: no item, `3.8`, a withheld entry |
| opens | not read | absent | any | pin the served key as `FirstUse`, then deliver |
| absent or unusable | legacy | (none) | any | pin as `FirstUse` in a fresh record, rewrap, then deliver |
| absent or unusable | pinned, or opens under neither | (unknown) | any | refuse: no item, `3.8`, nothing recorded (section 4.4) |

A refusal writes no item for that recipient, sets the recipient's property
(the ATTENDEE, or the ORGANIZER when `on_organizer`) to `3.8` through
`za_set_schedule_status`, and adds a withheld entry {uid, address, since}
to the sender's pin record unless one with that (uid, address) exists. The
sender's request succeeds and the other recipients are delivered to, as
release 2 section 5.3 handles a per-recipient failure. The comparison is a
byte equality of public values. Recipients that are not internal (release
2 section 5.2) are not attempted and never pinned, and non-key senders
never reach `za_fan_out` (release 1 invariant 9).

### 5.2 Order

`za_fan_out` today resolves and writes recipient by recipient. With pins it
runs in three phases:

1. Resolve every recipient of every message (the per-call recipient map
   already deduplicates addresses) and read the pin record; when it is
   absent or unusable, or when phase 2 will write it, read the mark from
   the vault record (section 4.4). A fan-out that changes nothing reads
   neither the mark nor the private key.
2. Decide every key-account recipient against the record (section 5.1), and
   when anything changed (new first-use pins, new withheld entries), write
   the pin record once, conditionally (section 4.2), before any item. When
   the wrap is legacy (the account's first pin, or a crash to heal), the
   rewrap follows (section 4.4); a rewrap that loses its race is traced and
   left to the next reader, and delivery proceeds.
3. Write the items of the recipients decided "deliver".

Pinning first, a crash between the writes leaves a pin without a delivery
(the next edit delivers), never a delivery without a pin. A fan-out whose
decisions change nothing writes no pin record.

### 5.3 The status code

A new constant `ITIP_STATUS_KEY_REFUSED = "3.8"`, the RFC 5546 "no
authority" code that RFC 6638 lists for SCHEDULE-STATUS, defined in the
fork's `fanout.rs` next to `status_rank`, not in upstream's
`crates/groupware/src/scheduling/itip.rs` (which plan 9 already touched for
the fork's three statuses).

`status_rank` gives it rank 3, above `5.1` (2) and `5.3` (1), so that
`record_status` keeps it over any other outcome when a recipient gets
several messages in one fan-out.

The value is the bare code. A status text (`3.8;Key changed, confirm on
your account page`) cannot ride in it: RFC 6638 section 3.2.9 defines the
SCHEDULE-STATUS parameter as a list of status codes only, and
`za_set_schedule_status` writes codes only. The page explains the code.

A later delivery replaces `3.8` (`za_set_schedule_status` keeps only
upstream's `3.7`); a PUT that builds no message for the address keeps it.

### 5.4 Failures

- A pin record read failure (a store error), or a write that loses its
  assertion twice or fails, is the delivery failure (`5.1`) of every
  key-account recipient of that fan-out, as today's store errors are;
  nothing is written for them. It is traced with the sender's account id
  and a `zero-access:` description, without addresses. The sender's request
  succeeds.
- A refusal is traced once per fan-out under the same event as the
  self-check (section 3.3), with the sender's account id and the count of
  refused recipients only: no address, no recipient account id (it would
  record who invited whom where no item shows it), no fingerprint.

### 5.5 The UID lease fence

The pin write is not under the UID lease (release 2 section 5.4) and
carries no fence. The pin record is the sender's own, per account,
conditional on its own stored value and independent of any UID; the fence
keeps recipients from adopting a losing token, and a pin write hands a
recipient nothing. A fan-out that loses its fence after its pin write
leaves first-use pins of keys the server served, which later fan-outs would
pin too, and withheld entries whose redelivery finds the committed event or
drops the entry (section 6.4).

### 5.6 Replies and delegates

- **Replies.** bob's reply seals to alice's key, so bob pins alice's
  address on his first reply. A refused reply puts `3.8` on the ORGANIZER
  of bob's copy and {uid, alice's address} in bob's withheld list; its
  redelivery sends a REPLY with bob's current participation status.
- **Delegates.** The delegating merge fans out in the organizer's session
  (release 2 section 6.4), so the organizer's pins apply: a refused
  delegate gets `3.8` on its ATTENDEE in the organizer's event, where that
  fan-out's statuses go, and an entry in the organizer's withheld list.

## 6. Account API additions

### 6.1 Common to the four endpoints

Four endpoints join the eight under `/api/vault/` (release 1 section 4.1).
`handle_vault_request` in `crates/http/src/api/vault.rs` routes (`contacts`,
none), (`contacts`, `lookup`), (`contacts`, `pin`) and (`contacts`, `forget`),
the shape `app-password/revoke` uses, to a new module
`crates/http/src/api/contacts.rs`, which shares `vault.rs`'s verification and
response helpers (`za_verify_primary`, `za_reread_verified`, `conflict`,
`json`). All are `POST` with a JSON body, follow api.md's conventions exactly
(CORS from `ZA_ACCOUNT_PAGE_ORIGIN` as for the existing eight, `Cache-Control:
no-store`, the anonymous-request rate limit, the two error body shapes), and
are password-verified: api.md's shared `username`, `password` and `totp`
fields, verified by `za_verify_primary`, with api.md's shared status table
(whose heading changes from five endpoints to nine).

The verification builds the request's own `SessionKeys` from the password, as
the existing endpoints do (not a key-cache entry; dropped with the request).
Each endpoint unwraps the private key (pinned, then legacy) and runs the
self-check of section 3.3, repair included; a wrap that opens under neither
answers the `vault record` 409 family. A write after a repair or a rewrap in
the same request (the pin endpoint's rewrap, for instance) re-reads the vault
record first, so it is not refused as a stale generation. Bodies and responses
stay out of traces (release 1 section 10). Everything, redelivery included,
runs in this HTTP request: the drain trigger in `handle_dav_request`
(`crates/dav/src/request.rs`) is not involved, and the request holds plaintext
while it redelivers, as any session request does.

### 6.2 POST /api/vault/contacts

Purpose: the page's contacts view. Request: the shared fields only.

Success (200):

| Field | Type | Meaning |
|---|---|---|
| `fingerprint` | string | The account's own fingerprint, derived from its private key (section 3.2). |
| `stored_key_matches` | boolean | Whether the stored public key equalled the derived one when this request read it, before any repair it made. |
| `key_repaired_at` | integer or null | Unix seconds of the last self-check repair. |
| `record_missing` | boolean | The wrap is pinned and the pin record is absent or unusable (the fail-closed state, section 4.4). |
| `pins` | array | One object per pin, below; empty when `record_missing`. |

Each pin object:

| Field | Type | Meaning |
|---|---|---|
| `address` | string | The pinned address. |
| `fingerprint` | string | The pinned key's fingerprint. |
| `pinned_at` | integer | Unix seconds. |
| `source` | string | `"first-use"` or `"confirmed"`. |
| `served_fingerprint` | string or null | The fingerprint of the key the server would seal to now; null when the address no longer resolves to an internal key account. |
| `withheld` | integer | The number of withheld entries for this address. |

| Code | When | Body |
|---|---|---|
| 200 | Listed | the object above |
| 400, 401, 402, 409, 429 | As in the shared table | |

### 6.3 POST /api/vault/contacts/lookup

Purpose: show a contact's fingerprint before pinning it, for comparison out
of band.

Request: the shared fields and `address` (string, required).

Success (200): `{"address": "<normalized address>", "fingerprint":
"<fingerprint>"}`, the address in the form a pin is keyed by (section 4.3).

The address resolves as the fan-out resolves a recipient
(`za_account_id_from_email`, a key account whose account cache entry holds
a public key; the receive permission is not checked, since a pin where
nothing is delivered is harmless). Looking up oneself is allowed and
returns the served key, though `contacts` already reports
`stored_key_matches`.

| Code | When | Body |
|---|---|---|
| 200 | Found | the object above |
| 400 | Shared causes; `address` missing or malformed | problem body |
| 401, 402, 429 | As in the shared table | |
| 409 | The address is not a local key account (unknown addresses, plain accounts and accounts pending setup get the same answer) | `{"error": "not a key account"}` |
| 409 | Other shared 409s | as in the shared table |

`not a key account` is not counted as a failed authentication.

### 6.4 POST /api/vault/contacts/pin

Purpose: confirm a contact's key, replacing any earlier pin, and redeliver
what was withheld for the address. It is also the recovery path from the
fail-closed state.

Request: the shared fields, `address` (string, required) and `fingerprint`
(string, required, as `lookup` returned it; any case and spacing are
accepted, and the 16 decoded bytes are compared, as `recover` tolerates the
recovery key's form).

The endpoint resolves the address as `lookup` does and pins the served key
only if its fingerprint equals the given one, with source `Confirmed`,
replacing any pin for the address. The served fingerprint is not in the
refusal body; the page looks the address up again. The pin is written first
(one conditional write, creating a fresh record when it is absent or
unusable), then the wrap is rewrapped if it is legacy (section 4.4). Then
up to 32 withheld entries for the address are redelivered, the rest left
for the next `contacts/pin`:

1. Find the event by UID (`find_event` in `drain.rs`) and unseal it. If it
   is gone, no longer names the address (as an ATTENDEE when the user
   organizes it, as the ORGANIZER otherwise), or fails the `SendSealed`
   preconditions (release 2 section 5.1: scheduling enabled,
   `CalendarSchedulingSend`, an event not in the past), drop the entry.
2. Build the message from the event's current state: for an organizer, the
   REQUEST `itip_create` would build (`organizer_request_full`, as the
   delegating merge calls it) restricted to this attendee; for an
   attendee, the REPLY the attendee builder would build for the user's
   current entry, from a fork helper over upstream's pieces
   (`itip_build_envelope` and the builder's component export), since
   `attendee_handle_update` emits a REPLY only for a change between two
   snapshots. A unit test holds the helper to `attendee_handle_update`'s
   output for the same change.
3. Run `za_fan_out` with the request's `SessionKeys` and the event's token
   pairs (`event_token_pairs`); it now finds the pin and delivers. A
   REQUEST redelivery takes the organizer's UID lease and passes it as the
   fence, as the delegating merge does (release 2 section 6.3, step 5), in
   case a token is minted for an attendee without a stored pair.
4. Write the status with `za_set_schedule_status` and reseal the event as
   the drain's merge does (`seal_event` with the returned pairs, under the
   assertion on the stored archive), with no schedule-tag change. A lost
   assertion re-reads and retries once; the entry counts as delivered once
   its item is written, whether or not its status was.
5. Remove delivered and dropped entries in one conditional pin-record write
   at the end; failed entries stay withheld.

Dropping an entry whose event is gone or no longer names the address
means a withheld CANCEL is never sent. That is accepted: a key change
means the contact did a fresh setup, so the copy the CANCEL would have
reached no longer exists; the page says so. (When the refusal was a
substitution instead, the contact's copy keeps its last delivered state,
which the page says too.)

Redelivery failures never fail the pin. Success (200): `{"ok": true,
"delivered": <int>, "failed": <int>, "dropped": <int>}`.

| Code | When | Body |
|---|---|---|
| 200 | Pinned; redelivery counts as above | the object above |
| 400 | Shared causes; `address` missing or malformed; `fingerprint` missing or not 32 hex digits once spaces are removed | problem body |
| 401, 402, 429 | As in the shared table | |
| 409 | The address is not a local key account | `{"error": "not a key account"}` |
| 409 | The served key's fingerprint differs from `fingerprint` | `{"error": "fingerprint mismatch"}` |
| 409 | The pin write lost its assertion twice | `{"error": "conflict"}` |
| 409 | Other shared 409s | as in the shared table |

### 6.5 POST /api/vault/contacts/forget

Purpose: remove a pin, or start over.

Request: the shared fields and `address` (string or null, required; the
key must be present, as `otp_auth` must on `totp`).

- With an address: removes its pin and withheld entries; the next message
  pins the served key as `FirstUse`. An address without a pin is a no-op,
  still 200, as removing TOTP when none is enrolled is.
- With null: replaces the whole record with an empty one (no pins, no
  withheld entries, `key_repaired_at` kept), which ends the fail-closed
  state. The wrap stays pinned.

Success (200): `{"ok": true}`.

| Code | When | Body |
|---|---|---|
| 200 | Forgotten, or nothing to forget | `{"ok": true}` |
| 400 | Shared causes; `address` absent, or present and malformed | problem body |
| 401, 402, 429 | As in the shared table | |
| 409 | The write lost its assertion twice | `{"error": "conflict"}` |
| 409 | Other shared 409s | as in the shared table |

### 6.6 Flows (for api.md)

- **Check or pre-pin a contact.** `contacts/lookup`; compare the
  fingerprint out of band with the one on the contact's own page; on a
  match, `contacts/pin`. Done before the first invitation, it protects
  that one too.
- **A key changed.** `contacts` shows a `served_fingerprint` that differs
  and a `withheld` count; after checking with the contact, `contacts/pin`
  pins the new key and redelivers.
- **Recovery from `record_missing`.** `contacts/forget` with null, or pin
  each contact again.

api.md's "What the page never sees" is unchanged: no key material is
returned, and fingerprints and addresses go only to the record's owner.

## 7. Stored format additions

| Field | Stored value | Sealed under, associated data |
|---|---|---|
| `ZeroAccessPins` (151, new) | nonce (24) then XChaCha20-Poly1305 of: length (`u32`, big endian), rkyv `ZaPinRecord` (section 4.1), zero padding to a multiple of 256 bytes; tag (16) | MK, `aad("pins", account_id)` |
| `ZeroAccessVault` (150), `private_key_wrap` | unchanged layout and length (`nonce || ciphertext`, 72 bytes) | MK; `aad("private-key", account_id)` (legacy, release 2) until the first pin write, `aad("private-key/pins", account_id)` (pinned) from then on |

`3.8` (`ITIP_STATUS_KEY_REFUSED`) joins `1.2`, `5.1` and `5.3` as a value
the fan-out writes inside the sender's sealed ORGANIZER and ATTENDEE
properties. The sealed item format, the `X-ZA-*` carriers, the token pairs
and the policy version do not change.

## 8. Error handling

- Endpoints: api.md's two body shapes, its shared status table and the
  409s of section 6; everything else is the generic problem body.
- Fan-out: section 5.4 (`3.8` for a refusal, `5.1` for a pin record
  failure); the sender's request succeeds.
- Self-check: a lost race is not retried in the session; a store error is
  traced and the request proceeds; release 2 section 10's private-key
  handling is unchanged.
- Redelivery: a failed entry stays withheld and counts as `failed`.
- No new settings, event types, permissions or environment knobs.
  Existing events carry `zero-access:` details, and no trace carries key
  bytes, fingerprints or addresses.

## 9. Testing

**Unit.** In `crates/vault`: `display_fingerprint` matches a fixed vector
and the display form; a pin record round-trips through seal and open, fails
under another account's associated data and under another MK, pads to 256
bytes, and an unknown `version` reads as unusable; a pinned wrap does not
open under the legacy associated data nor a legacy wrap under the pinned,
and the reader's pinned-then-legacy order reports which one opened. In
`fanout.rs`: `status_rank` ranks `3.8` above `5.1`, and
`za_set_schedule_status` replaces a `3.8` with a later status. In
`groupware`: the REPLY helper matches `attendee_handle_update`'s REPLY for
the same participation change (section 6.4).

**Integration.** A new za sub-module, `tests/src/za/pinning.rs`, run in
`tests/src/za/mod.rs` after the drain modules and before the leak tests,
with the drain on (`DrainOn`). The attack is a store-level helper added to
`tests/src/utils/za.rs`, `substitute_public_key`: it rewrites the victim's
`public_key` through `za_vault_record` and `za_vault_write` at the next
revision, invalidates the victim's `Account` and `AccessToken` caches, and
returns the original key for restoring. A second helper reads which
associated data an account's `private_key_wrap` opens under (with the
account's MK from `za_unlock`) and can rewrap it as legacy, to plant the
state a crash between the two first-pin writes leaves. Cases, with alice,
bob and carol as key accounts:

1. First use under a legacy wrap: alice, who never pinned and has no pin
   record, invites bob; the item is delivered, alice's record holds a
   `FirstUse` pin for bob, and alice's wrap now opens only as pinned (the
   rewrap on the first pin, one revision bump).
2. Substitute bob's key; alice edits the event: bob's ATTENDEE in alice's
   event shows `3.8`, bob's inbox holds no new item, and alice's
   `contacts` shows one withheld entry and a `served_fingerprint` that
   differs from the pin. A second edit leaves the count at one.
3. bob's next DAV request, with no item pending, repairs bob's record
   through the self-check: the stored key matches again and bob's
   `key_repaired_at` is set.
4. `contacts/pin` with the substituted key's fingerprint answers 409
   `fingerprint mismatch`; `contacts/lookup` then `contacts/pin` with the
   returned one, in a different case and spacing, redelivers: `delivered`
   is 1, bob's drain opens the item, and alice's event shows `1.2` for bob.
5. `contacts/forget` for bob, then a new invitation: bob is pinned again as
   `FirstUse`.
6. Fail closed: under alice's pinned wrap, delete her pin record at store
   level; alice's next invitation gives `3.8` to every key attendee and
   writes no pin record; `contacts` reports `record_missing`;
   `contacts/forget` with null recovers and the next invitation delivers.
7. Healing: rewrap carol's private key as legacy while her pin record
   (holding a pin) exists; carol's next fan-out that writes the record, or
   her next session's first drain pass, rewraps it as pinned, after which
   deleting the record fails closed.
8. Reply direction: bob replies to alice (bob pins alice); substitute
   alice's key; bob's next reply is refused with `3.8` on the ORGANIZER of
   bob's copy; bob pins alice's real key on the page; the redelivered REPLY
   applies bob's current participation status to alice's event.
9. Delegate direction: bob delegates to carol, with carol pinned by alice
   and carol's key substituted; alice's drain (the delegating merge) refuses
   carol with `3.8` on carol's ATTENDEE in alice's event.
10. Pre-pin: alice looks carol up and pins her before any invitation;
    substitute carol's key; the first invitation to carol is refused.
11. Pin before deliver: a one-shot pin-write failure (`PIN_WRITE_FAIL_ONCE`
    in `crates/common/src/za/test_hooks.rs`) on an invitation to a new
    contact leaves `5.1` and no item in the contact's inbox.

Cases restore keys, forget pins and delete events, since the suite calls
`assert_is_empty` once at its end (`tests/src/za/mod.rs`);
`tests/src/utils/cleanup.rs` exempts field 151 while its account exists,
as it exempts field 150.

**Leak tests** (`tests/src/za/leak.rs`). The whole-store scan already covers
every principal property; a pin record holding canary addresses must show
no hit.

**CalDAV client CI** (`tests/caldav_client/`): server suite only. The
substitution is a store write the client harness cannot make, and a client
sees only a `3.8` parameter on its own event, which the server suite
asserts; a client flow would only show that a client tolerates `3.8`.

**Mutation checks** in the style of plan 7: replacing the key equality with
`true` fails case 2; moving the pin write after the item writes fails case
11; removing the (uid, address) dedupe fails case 2's second edit; trying
only the legacy associated data, or treating an absent record as first use
whatever the wrap, fails case 6; dropping the healing rewrap fails case 7;
removing the fingerprint check in `contacts/pin` fails case 4's stale
fingerprint.

## 10. Invariants for implementers

Release 1's ten and release 2's five (11 to 15) hold. This spec adds:

16. The pin record is sealed under MK. It is read and written only in a
    request that holds the owner's session keys; background code never
    reads it.
17. Pins are compared before any item is written, and a mismatch writes no
    item. The pin record write of a fan-out precedes its item writes.
18. Pins are keyed by lowercased address, never by account id.
19. The page's own fingerprint and the self-check derive the public key
    from the private key, never from the stored public key.
20. The pinned wrap is set once and never reverts to legacy. The vault
    record's layout does not change.
21. No key bytes, fingerprints or addresses in traces.

## 11. Alternatives considered

- **Pins inside the vault record**, as issue #32 first put it. Rejected:
  its revision is the authentication generation (release 1 section 3.1),
  so every pin write would end every open session of the account.
- **Pins keyed by account id.** Rejected: the writer controls the
  address-to-account mapping, and an id-keyed pin accepts a fresh account
  behind bob's address as first use (section 4.3).
- **A `has_pins` bool in the vault record.** Rejected: the vault record is
  not authenticated, so a writer clears the bool with a plain edit and
  reopens first use. The wrap's associated data cannot be changed without
  MK.
- **A version 2 vault record layout** to carry such a marker. Rejected as
  unnecessary once the associated data carries it, and it would have made
  pinned accounts unreadable to an older binary.
- **Fingerprints read from the stored key on both sides.** Rejected: the
  writer controls both and can make them agree (section 3.2).
- **Failing the PUT on a mismatch.** Rejected: a generic client error with
  no reason, an event that cannot be saved, every other attendee held back.
  SCHEDULE-STATUS signals per recipient and the request succeeds, as
  release 2 section 5.3 handles a per-recipient failure.
- **Strict mode, no trust on first use.** Rejected: every new contact
  blocked until a page visit; pre-pinning gives strictness per contact.
- **Caching pins in `SessionKeys`.** Rejected: stale across sessions and
  nodes, for reads that happen only when messages are sent.
- **Asking the user to resend from the client.** Rejected: an unchanged
  re-save builds no message, so nothing would be sent.
- **Signing invitations.** Out of scope: a signing key per account and a
  different trust root.

## 12. Release pointers

- The page's contacts screen (`#contacts` in `jaylaney/circulo-account`):
  its own plan after this one, against api.md as the server plan updates
  it (the four endpoints, the nine-endpoint shared table and the flows of
  section 6.6).

## 13. Open items for planning

Each item is for planning to look up in code; the recommendation stands
unless the code says otherwise.

1. **The trace event.** Confirm `SecurityEvent::Unauthorized` with
   `zero-access:` details for the self-check mismatch (section 3.3) and the
   refusal trace (section 5.4). Recommendation: that event for both, as
   release 2 section 6.5 uses it for a forged token.
2. **Field 151.** Confirm that 151 was never assigned to a principal field
   in upstream's history (section 4.1), as was done for 150.
   Recommendation: 151; the next free unassigned value otherwise.
3. **Other sessions after the rewrap.** The fan-out reads the mark fresh
   when it needs it (section 4.4), but the drain's `SessionKeys` keep the
   unwrapped private key in a `OnceLock`. Confirm that the rewrap's
   `CacheInvalidation::AccessToken` drops every key-cache entry of the
   account on every node, so no session outlives the rewrap with a
   legacy-era view. Recommendation: rely on it; the fan-out's fresh read
   already decides failing closed, so a session's cached private key is
   only ever used to unwrap.
4. **The REPLY helper's pieces.** Confirm which parts of upstream's attendee
   builder (`crates/groupware/src/scheduling/attendee.rs`, `pub(crate)`)
   the helper can call from the fork's module without widening upstream's
   visibility. Recommendation: the helper lives in `groupware` beside the
   fan-out, and any visibility change is a one-word `pub(crate)` edit.
5. **The address the fan-out sees.** Confirm that `message.to` holds the
   `mailto:`-decoded address `Email::new` sanitizes, so that
   `to_canonical_address` gives the same key on the fan-out side and in
   the lookup response (section 4.3). Recommendation: normalize both from
   the sanitized form in one helper.
6. **Client display of `3.8`.** How Apple Calendar, Thunderbird and DAVx5
   show a `3.8` SCHEDULE-STATUS. Recommendation: a manual-checklist item
   (`docs/zero-access/manual-checklist.md`), no code.
