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

- A pin record sealed under MK, mapping contact addresses to keys (section
  4), and the pin check in `za_fan_out`: `3.8` on a mismatch, trust on
  first use otherwise (section 5).
- A self-check that repairs a substituted stored key of one's own (3.3).
- Four password-verified endpoints under `/api/vault/contacts`: list, look
  up, pin (redelivering refused changes) and forget (section 6).

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

- Key change is rare and destructive: the public key never changes for a
  vault's life (release 2 section 3.2), and only a fresh setup, abandoning
  the old vault, gives an address a new key. A changed key means the
  contact started over or the store was tampered with, so refusing until
  the sender confirms is reasonable.
- Release 1's and release 2's assumptions hold. No setting, permission,
  event type or environment knob is added (the registry schema and the
  event enums are generated, `CLAUDE.md`).

## 2. Threat model additions

**Claim.** Scheduling messages between key accounts are confidential
against a store writer who substitutes public keys, for every contact the
sender has pinned, including the first message to a contact pinned on the
page beforehand: a message to a pinned address whose served key differs is
not sealed and no item is written. The trust model is release 1's and 2's
(sessions hold plaintext, clients unmodified, background code keyless,
release 1 invariant 6); pins are the sender's own data under its MK.

**Limits, stated alongside the claim.**

1. **Trust on first use.** The first message to an unpinned address is
   sealed to the served key, as in release 2, and that key is pinned. If
   the store was substituted at that moment, the first message is exposed
   and the substituted key is pinned. The substitution becomes visible
   afterwards: once the writer restores bob's real key, or bob's own
   self-check repairs it (section 3.3), alice's next message to bob is
   refused with `3.8`, and alice's page shows the mismatch.
2. **Replay of the legacy wrap.** Deleting the sender's pin record alone
   does not reopen first use: the account's first pin write rewraps its
   private key under associated data that marks it as pinned, and an
   absent or unusable pin record under a pinned wrap fails closed (section
   4.4). But the wrap's associated data names only the purpose and the
   account, not the revision or the record's other fields. A writer who
   saved the pre-pin `private_key_wrap` bytes can paste that one field into
   the current vault record and delete the pin record: first use reopens,
   the current password, recovery key, TOTP and app passwords keep working,
   and nothing signals the user except the page showing no pins. Rolling
   the pin record back to an older sealed value is possible too. The
   fail-closed property therefore holds against a writer without a pre-pin
   snapshot of the vault record; integrity was never claimed against a
   store writer (release 1 section 2).
3. **The sender's own record.** A substituted key on the sender's own vault
   record is caught by the sender's self-check, which repairs it; messages
   others sealed to it before the repair are exposed unless their senders
   had pinned the sender.
4. **Lookup reveals key accounts** to any key account holder (section 6.3),
   at a full password verification per lookup. Accepted.
5. **The running server** holds plaintext and pins during a session and can
   seal to any key; pins defend against the store, not the process.
6. **Redelivery sends current state** (section 6.4), not the refused
   messages, so three things are lost: a whole-event CANCEL for a deleted
   event is dropped; a removal CANCEL (the event no longer names the
   address) is dropped and the contact keeps a stale copy; and occurrence
   declines the attendee expressed by deleting an occurrence or adding an
   EXDATE are not reconstructed, so the attendee declines again.
7. **Out-of-order arrival.** A redelivery and a concurrent ordinary delivery
   can reach the recipient out of order. A change that bumped the SEQUENCE
   is protected by the drain's stale check (release 2 section 6.4); a
   REPLY is not, and the attendee's next change corrects it. Upstream has
   the same gap (`handle_reply` applies PARTSTAT without a timestamp
   check).

**Visible metadata added** (beyond release 2 section 2):

- The existence of an account's pin record, its size class (a multiple of
  256 bytes, section 4.1) and when it changes (its stored bytes change on
  every pin write).
- That the account's `private_key_wrap` changed once (its bytes and the
  revision bump), but not whether the wrap is pinned or legacy: the
  associated data is not stored.
- That a delivery did not happen: a writer who substituted a key and waits
  for an item learns that the sender had pinned the address.

**Not visible:** whether the wrap is pinned or legacy (though a pin record's
existence suggests the former), who is pinned, the pinned keys and their
fingerprints, the sources and times of pins, the withheld list,
`key_repaired_at`, and the `3.8` status itself, which lives inside
the sender's sealed event (release 1 section 6 seals ATTENDEE and ORGANIZER
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
2. If the wrap is pinned and the pin record exists and opens, set
   `key_repaired_at` in it (conditional write, section 4.2). Otherwise
   write nothing to it: the self-check never creates a pin record, so
   under a pinned wrap with an absent or unusable record the fail-closed
   state survives a repair (invariant 22).
3. Rewrite the vault record's `public_key` from the derived key through the
   common commit helper (section 4.5), conditional on the revision read.
   The write does not rewrap: a legacy-wrap account gets a record only from
   its first pin write, which rewraps (section 4.4). The generation bump
   ends the account's cached logins and resident keys, the right
   consequence of a tampered record, and the helper's invalidation lets
   other senders' fan-outs see the repaired key at once. A lost race is not
   retried; the next session checks again.

The check runs only in a request holding the account's session keys. The drain
today unwraps the private key only when a pass finds pending items
(`drain_items` in `crates/groupware/src/calendar/drain.rs`), but the target of
a refused substitution receives none; so every session's first drain pass
unwraps it and runs the check, items pending or not, at one vault record read
and one X25519 scalar multiplication per session. Release 2's suspension when
the wrap does not open is unchanged. The contacts endpoints (section 6) run the
same check under the same rule (they create no pin record); the page's "your
fingerprint" comes from it.

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
| | `withheld` | `Vec<ZaWithheld>` | Refused deliveries awaiting redelivery, unique by (`document_id`, `address`). No cap: only the owner's fan-outs add entries. |
| | `key_repaired_at` | `Option<i64>` | Unix seconds of the last self-check repair (section 3.3). |
| `ZaPin` | `address` | `String` | The contact's address, lowercased, as the message names it (section 4.3). |
| | `public_key` | `[u8; 32]` | The pinned X25519 public key. |
| | `pinned_at` | `i64` | Unix seconds. |
| | `source` | `PinSource` | `FirstUse` (the fan-out pinned the served key) or `Confirmed` (the user pinned it on the page). |
| `ZaWithheld` | `calendar_id` | `u32` | A calendar of the source event, the sender's event whose fan-out refused (the PUT's or DELETE's document, or the organizer event of the drain's delegating merge). |
| | `document_id` | `u32` | The source event's document id; with `address`, the entry's identity. |
| | `uid` | `String` | The source event's UID, checked at redelivery so a reused document id under another UID is not taken for the source. |
| | `address` | `String` | The refused recipient's address, normalized as pins are (section 4.3). |
| | `since` | `i64` | Unix seconds of the first refusal. |
| | `refusal_id` | `u64` | 64 random bits, regenerated on every refusal of the entry (a new entry gets one too), so a redelivery removes exactly the refusal it consumed (section 6.4). |

The entry stores no message; delivery rebuilds current state (sections 5.1
and 6.4). Entries are small.
The structs have a silent `Debug` (they hold addresses).

### 4.2 Reads and writes

- **Read fresh** on every fan-out and endpoint call, never cached in
  `SessionKeys` (a session cache goes stale across sessions and nodes).
- **Writes are conditional** on the previous stored value, as
  `vault_write_store` writes the vault record (`AssertValue::Hash` of the
  bytes read, `AssertValue::None` when none was read). Under a pinned wrap
  a write asserts only the pin record; a lost assertion re-reads and
  retries once, re-running the decision against the fresh record (for the
  fan-out, every comparison of section 5.1), and a second loss is that
  operation's pin write failure (section 5.4). Under a legacy wrap every
  pin-record write is the atomic first-pin batch of section 4.4.
- **Only with the owner's MK**, in a request holding the owner's session
  keys (a DAV request's, or an endpoint's freshly verified ones).
- **Removed with the account:** the destroy-account task's
  `danger_destroy_account` (`crates/store/src/dispatch/store.rs`) deletes
  the account's whole principal-property range, field 151 included.

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
private-key wrap, not by a stored field. Release 2 wraps the private key under
MK with `aad("private-key", account_id)` (`AAD_PRIVATE_KEY`); this spec calls
that the legacy wrap. The account's first pin write rewraps the private key
under `aad("private-key/pins", account_id)` (`AAD_PRIVATE_KEY_PINNED`), the
pinned wrap, in the same atomic batch as the pin record (below), through the
common commit helper (section 4.5), in a request that holds MK. That is the one
authentication generation bump the feature costs an account, ever: it ends the
account's other sessions once, and later pin writes leave the vault record
alone (section 11). The vault record's layout and the wrap's length do not
change, so no stored struct changes (release 1 invariant 1).

**Reading the mark.** Every reader of the private key (the drain's
`za_open_private_key`, the fan-out, the contacts endpoints) tries the
pinned associated data first, then the legacy:

| Wrap opens under | Meaning | An absent or unusable pin record |
|---|---|---|
| pinned | the account has pinned | fails closed |
| legacy | the account never pinned | is legitimate: first use; an unusable record is treated as absent and overwritten |
| neither | tampered or corrupt | the drain keeps release 2's suspension (its section 10); the fan-out fails closed; the endpoints answer 409 `{"error": "vault record missing"}` |

A record is unusable when it does not open, does not decode, or has a
version other than `1`. A record that opens is proof enough (a writer
cannot forge one under MK), so the fan-out reads the mark, fresh from the
vault record, only when the record is absent or unusable or it is about to
write the record.

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

**The first pin write is one batch.** Both fields live on the account's
Principal document 0, so one `BatchBuilder` batch carries two
`assert_value`/`set` pairs: the pin record asserted absent (or, to heal a
record found under a legacy wrap, asserted at the bytes read), and the
vault record asserted at the bytes read (`AssertValue::Hash`, as
`vault_write_store` does), creating or rewriting the pin record and
rewrapping `private_key_wrap` together. On any assertion failure the writer
re-reads both records and decides again, at most twice; a third conflict
is the delivery failure (`5.1`) of that fan-out's key-account recipients
(an endpoint answers 409 `conflict`). A stale "legacy, no record"
observation therefore never creates a record after the account became
pinned: the vault assertion fails, and the re-read finds the pinned wrap
(and, if the record was deleted meanwhile, fails closed). A record under a
legacy wrap arises only from a replayed wrap (limit 2); the next reader
that writes the record (a fan-out, the session's first drain pass, an
endpoint) heals it with the same batch.

**What a store writer can and cannot do.** Producing a legacy wrap needs
MK. Deleting the pin record alone fails closed, and so does replacing it
with another account's (its associated data names the account) or with
garbage. A self-check repair does not end the state either (section 3.3).
What the writer can do is replay: the associated data does not bind the
revision or the other fields, so a saved pre-pin `private_key_wrap`
pasted into the current record, with the pin record deleted, reopens
first use while every current credential keeps working (section 2, limit
2). The property holds against a writer without that snapshot.

### 4.5 Vault writes outside the HTTP module

The rewrap and the repair are vault writes made by the fan-out
(`crates/groupware`), the drain and self-check (`crates/common`) and the
endpoints, but `za_commit` lives in `crates/http/src/api/vault.rs`. The
revision bump, conditional write and invalidation move into a common
helper in `crates/common/src/auth/vault.rs`, which `za_commit` wraps and
extends with orphan pruning, and which also builds section 4.4's first-pin
batch. `CLAUDE.md`'s rule becomes "every vault write goes through the
common helper or `za_commit`"; the plan updates `CLAUDE.md`.

## 5. The fan-out

### 5.1 The per-recipient decision

All sealed deliveries go through `za_fan_out`: the organizer's PUT create
and update and DELETE (`crates/dav/src/calendar/update.rs`, `delete.rs`),
the attendee's reply by PUT or DELETE, and the drain's delegating merge
(`crates/groupware/src/calendar/drain.rs`). The check lives there, once,
for all of them, per key-account recipient resolved to an internal account
and its served key (`resolve_inner`, from the `AccountCache` entry's
`za_public_key`):

| Pin record | Private-key wrap | Pin for the address | Served key | Outcome |
|---|---|---|---|---|
| opens | not read | present, no entry for this document | equal to the pin | deliver as today (`1.2`, or `5.1` on a write failure) |
| opens | not read | present, an entry for this document | equal to the pin | deliver upstream's message, then the full current state if the relationship remains (below); retire the entry after both (section 5.2) |
| opens | not read | present | different | refuse: no item, `3.8`, a withheld entry |
| opens | not read | absent | any | pin the served key as `FirstUse`, then deliver |
| absent or unusable | legacy | (none) | any | pin as `FirstUse` in a fresh record with the rewrap (one batch, section 4.4), then deliver |
| absent or unusable | pinned, or opens under neither | (unknown) | any | refuse: no item, `3.8`, nothing recorded (section 4.4) |

A refusal writes no item for the recipient, sets its property (the
ATTENDEE, or the ORGANIZER when `on_organizer`) to `3.8` through
`za_set_schedule_status`, and records a withheld entry for (source document
id, address) with a fresh `refusal_id`, or gives the existing entry a fresh
one (section 4.1). The sender's request succeeds and the other recipients
are delivered to, as release 2 section 5.3 handles a per-recipient
failure. The comparison is a byte equality of public values. Recipients
that are not internal (release 2 section 5.2) are not attempted and never
pinned; non-key senders never reach `za_fan_out` (release 1 invariant 9).

**Delivery to an address with an entry.** A fan-out delivering to a
pinned, same-key address with a withheld entry for this document writes
two items for that recipient, in order: (1) upstream's own message for this
change, exactly as without pins (a partial or full REQUEST, a partial or
whole-event CANCEL, or the change's REPLY, a DELETE's DECLINED reply and
its occurrence declines included); then (2) the full current state, only
while the relationship remains after the change: for an organizer, the
committed event still names the address (not after a whole-event CANCEL or
a removal); for an attendee, the event still exists (not after a DELETE).
(2) is the organizer's full REQUEST for that attendee
(`organizer_request_full`) or the attendee's current-participation REPLY
(section 6.4's helper), built from the state being committed, never from a
superseded tree: the DELETE path tells `za_fan_out` its tree is the
pre-deletion one. The entry is retired after every item of that recipient
is written (section 5.2, phase 4). Upstream's message carries the intended
change (an occurrence cancel, a DELETE's decline), the full state carries
everything withheld, and nothing is derived from a tree that contradicts
the action; the cost is a second item, only while an entry exists. An item
write failure (`5.1`) leaves the entry, so entries drain once deliveries
flow again and `contacts/pin` redelivers only documents nobody edited.

### 5.2 Order

`za_fan_out` today resolves and writes recipient by recipient; with pins it
runs in four phases:

1. Resolve every recipient of every message (the per-call recipient map
   already deduplicates addresses) and read the pin record; when it is
   absent or unusable, or when phase 2 will write it, read the mark from
   the vault record (section 4.4). A fan-out that changes nothing reads
   neither the mark nor the private key.
2. Decide every key-account recipient against the record (section 5.1), and
   when anything changed (new first-use pins, any refusal), write the pin
   record once, conditionally (section 4.2), before any item. Under a
   legacy wrap that write is the atomic batch with the rewrap (section
   4.4), re-decided on a conflict.
3. Write the items of the recipients decided "deliver", two for a
   recipient with an entry (section 5.1).
4. Retire the entries of recipients whose items were all written, in one
   more conditional pin-record write. The one retirement rule, here and in
   `contacts/pin` step 5: an entry is removed only when its stored
   `refusal_id` still equals the id the delivery consumed (read in phase
   1), checked on every write attempt, the retry after a conflict
   included. Otherwise a newer refusal re-armed the entry, and it stays.
   A failed or skipped retirement leaves the entry and never undoes a
   written item. `za_fan_out` takes a `redelivery` flag from its caller;
   `contacts/pin` sets it, and the fan-out then sends only the message it
   was given (already the full state), adds no second item and retires
   nothing: the endpoint's guarded step 5 is its only cleanup.

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

The value is the bare code: RFC 6638 section 3.2.9 allows status codes
only in SCHEDULE-STATUS, so the page explains it.

A later delivery replaces `3.8` (`za_set_schedule_status` keeps only
upstream's `3.7`); a PUT that builds no message for the address keeps it.

### 5.4 Failures

- A pin record read failure, or a phase 2 write that loses its assertion
  twice or fails, is the delivery failure (`5.1`) of every key-account
  recipient of that fan-out, and nothing is written for them; it is traced
  with the sender's account id and no address. A failed phase 4 retirement
  is not a failure: the items stand and the entry stays (section 5.2).
- A refusal is traced once per fan-out under the same event as the
  self-check (section 3.3), with the sender's account id and the count of
  refused recipients only: no address, no recipient account id (it would
  record who invited whom where no item shows it), no fingerprint.

### 5.5 The UID lease fence

The pin write carries no UID lease fence (release 2 section 5.4): the
record is the sender's own, conditional on its own value, and hands a
recipient nothing. A fan-out that loses its fence after the pin write
leaves first-use pins of served keys, entries for a never-committed
source (which redelivery drops, section 6.4), and entries it retired for
a delivery whose source commit then failed; the next edit sends it again.

### 5.6 Replies and delegates

- **Replies.** bob's reply seals to alice's key, so bob pins alice's
  address on his first reply. A refused reply puts `3.8` on the ORGANIZER
  of bob's copy and a withheld entry for (his copy's document id, alice's
  address). bob's next ordinary reply sends that change's REPLY, then his
  current participation (unless he deleted the event); `contacts/pin`
  sends his current participation alone.
- **Delegates.** The delegating merge fans out in the organizer's session
  (release 2 section 6.4), so the organizer's pins apply: a refused
  delegate gets `3.8` on its ATTENDEE in the organizer's event, where that
  fan-out's statuses go, and a withheld entry for (the organizer event's
  document id, the delegate).

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

Verification builds the request's own `SessionKeys` from the password, as the
existing endpoints do (not a key-cache entry; dropped with the request). Each
endpoint unwraps the private key (pinned, then legacy) and runs the self-check
of section 3.3, repair included; a wrap that opens under neither answers the
`vault record` 409 family. A write after a repair or a rewrap in the same
request (the pin endpoint's rewrap, for instance) re-reads the vault record
first, so it is not refused as a stale generation. Bodies and responses stay
out of traces (release 1 section 10). Everything, redelivery included, runs in
this HTTP request: the drain trigger in `handle_dav_request`
(`crates/dav/src/request.rs`) is not involved, and the request holds plaintext
while it redelivers, as any session request does.

### 6.2 POST /api/vault/contacts

Purpose: the page's contacts view. Request: the shared fields only.

Success (200):

| Field | Type | Meaning |
|---|---|---|
| `fingerprint` | string | The account's own fingerprint, derived from its private key (section 3.2). |
| `stored_key_matches` | boolean | Whether the stored public key equalled the derived one when this request read it, before any repair it made. |
| `key_repaired_at` | integer or null | Unix seconds of the last self-check repair recorded in an existing pin record (section 3.3). |
| `record_missing` | boolean | The wrap is pinned and the pin record is absent or unusable (the fail-closed state, section 4.4). |
| `pins` | array | One object per pin, below; empty when `record_missing`. The listing creates no record. |

Each pin object:

| Field | Type | Meaning |
|---|---|---|
| `address` | string | The pinned address. |
| `fingerprint` | string | The pinned key's fingerprint. |
| `pinned_at` | integer | Unix seconds. |
| `source` | string | `"first-use"` or `"confirmed"`. |
| `served_fingerprint` | string or null | The fingerprint of the key the server would seal to now; null when the address no longer resolves to an internal key account. |
| `withheld` | integer | The number of this address's withheld entries: source events refused and not delivered since. |

| Code | When | Body |
|---|---|---|
| 200 | Listed | the object above |
| 400, 401, 402, 409, 429 | As in the shared table | |

### 6.3 POST /api/vault/contacts/lookup

Purpose: a contact's fingerprint, for comparison out of band. Request: the
shared fields and `address` (string, required).

Success (200): `{"address": "<normalized address>", "fingerprint":
"<fingerprint>"}`, the address in the form a pin is keyed by (section 4.3).

The address resolves as the fan-out resolves a recipient
(`za_account_id_from_email`, a key account whose account cache entry holds
a public key; the receive permission is not checked, since a pin where
nothing is delivered is harmless). Looking up oneself returns the served
key. `not a key account` is not counted as a failed authentication.

| Code | When | Body |
|---|---|---|
| 200 | Found | the object above |
| 400 | Shared causes; `address` missing or malformed | problem body |
| 401, 402, 429 | As in the shared table | |
| 409 | The address is not a local key account (unknown addresses, plain accounts and accounts pending setup get the same answer) | `{"error": "not a key account"}` |
| 409 | Other shared 409s | as in the shared table |

### 6.4 POST /api/vault/contacts/pin

Purpose: confirm a contact's key and redeliver what was withheld for the
address; also the recovery path from the fail-closed state. Request: the
shared fields, `address` and `fingerprint` (both strings, required; the
fingerprint as `lookup` returned it, any case and spacing, compared as the
16 decoded bytes, as `recover` tolerates the recovery key's form).

The endpoint resolves the address as `lookup` does and pins the served key
only if its fingerprint equals the given one, with source `Confirmed`,
replacing any pin for the address. The served fingerprint is not in the
refusal body; the page looks the address up again. The pin is written first
(a conditional write creating a fresh record when it is absent or
unusable; under a legacy wrap, the atomic batch with the rewrap, section
4.4). Then up to 32 withheld entries for the address are redelivered, the
rest left for the next `contacts/pin`. For each entry, with its
`refusal_id` read at the start:

1. Load the source document by (`calendar_id`, `document_id`) and unseal
   it. Drop the entry if it is gone, no longer belongs to the account or
   to that calendar, carries a UID other than the entry's, no longer names
   the address (as an ATTENDEE when the user organizes it, as the
   ORGANIZER otherwise), or fails the `SendSealed` preconditions (release 2
   section 5.1: scheduling enabled, `CalendarSchedulingSend`, an event not
   in the past). No account-wide UID lookup is made: two events with one
   UID in two calendars are separate entries.
2. Build the current state for that one recipient. When the user organizes
   the event: the full REQUEST for that attendee, through the path the
   drain's delegating merge uses for a new delegate
   (`organizer_request_full`, release 2 section 6.4), under the
   organizer's UID lease passed as the fence (release 2 section 6.3, step
   5). When the user is an attendee: a REPLY of the attendee's current
   participation, the master's and each existing override's, built by a
   small fork helper over upstream's builder pieces (release 2's attendee
   export, `itip_build_envelope`; section 13, item 3).
3. Run `za_fan_out` with that message, the loaded event as the sender's
   tree, its current token pairs (`event_token_pairs`) and `redelivery`
   set (section 5.2); it finds the pin and delivers.
4. Write the status with `za_set_schedule_status` and reseal the event as
   the drain's merge does (`seal_event` with the returned pairs, under the
   assertion on the stored archive), with no schedule-tag change. A lost
   assertion re-reads and retries once; the entry counts as delivered once
   its item is written, whether or not its status was.
5. Cleanup, one conditional pin-record write under section 5.2's
   retirement rule (the `refusal_id` read at the start, every attempt): an
   entry re-armed during the call stays and counts as `remaining`, not as
   delivered. Failed entries stay withheld.

Why current state: a full REQUEST covers every refused partial occurrence
change and can never revert a newer successful delivery, and so does a
current-participation REPLY. `contacts/pin` sends only the full state, since
it makes no change of its own; when the relationship no longer exists the
entry is dropped (step 1). Its losses are section 2, limit 6 (a deleted
event's CANCEL is dropped because the token pairs and the lease need the
event); after a real key change the contact did a fresh setup, so the copy
those messages would have reached no longer exists, which the page says.
The residual ordering race is limit 7.

Redelivery failures never fail the pin. Success (200): `{"ok": true,
"delivered": <int>, "failed": <int>, "dropped": <int>, "remaining":
<int>}`, where `remaining` counts the address's entries still withheld
other than failed ones: those beyond the 32 of this call and those refused
again during it.

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

Purpose: remove a pin, or start over. Request: the shared fields and
`address` (string or null; the key must be present, as `otp_auth` on
`totp`).

- With an address: removes its pin and withheld entries; the next message
  pins the served key as `FirstUse`. An address without a pin is a no-op,
  still 200, as removing TOTP when none is enrolled is.
- With null: replaces the whole record with an empty one (no pins, no
  withheld entries, `key_repaired_at` kept), which ends the fail-closed
  state; the wrap stays pinned. Under a legacy wrap with no record it
  writes nothing.

Success (200): `{"ok": true}`.

| Code | When | Body |
|---|---|---|
| 200 | Forgotten, or nothing to forget | `{"ok": true}` |
| 400 | Shared causes; `address` absent, or present and malformed | problem body |
| 401, 402, 429 | As in the shared table | |
| 409 | The write lost its assertion twice | `{"error": "conflict"}` |
| 409 | Other shared 409s | as in the shared table |

### 6.6 Flows (for api.md)

Check or pre-pin: `contacts/lookup`, compare out of band, `contacts/pin`.
Key changed: `contacts` shows a differing `served_fingerprint` and a
`withheld` count; `contacts/pin` pins and redelivers. `record_missing`:
`contacts/forget` with null, or pin each contact.

api.md's "What the page never sees" is unchanged: no key material is
returned; fingerprints and addresses go only to the record's owner.

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

Endpoints use api.md's body shapes and shared table plus section 6's 409s;
the fan-out follows section 5.4; a self-check race is not retried and a
store error is traced; a failed redelivery entry stays withheld. No new
settings, event types, permissions or knobs; traces carry `zero-access:`
details and no key bytes, fingerprints or addresses.

## 9. Testing

**Unit.** In `crates/vault`: `display_fingerprint` matches a fixed vector
and the display form; a pin record round-trips through seal and open, fails
under another account's associated data and under another MK, pads to 256
bytes, and an unknown `version` reads as unusable; a pinned wrap does not
open under the legacy associated data nor a legacy wrap under the pinned,
and the reader's pinned-then-legacy order reports which one opened; every
refusal changes an entry's `refusal_id`. In `fanout.rs`: `status_rank`
ranks `3.8` above `5.1`, and `za_set_schedule_status` replaces a `3.8`
with a later status. In `groupware`: the REPLY helper's REPLY carries the
master's and each override's PARTSTAT.

**Integration.** A new za sub-module, `tests/src/za/pinning.rs`, run in
`tests/src/za/mod.rs` after the drain modules and before the leak tests,
with the drain on (`DrainOn`). Helpers in `tests/src/utils/za.rs`:
`substitute_public_key` rewrites an account's `public_key` (`za_vault_write`,
next revision, caches invalidated) and returns the original; a wrap helper
reports which associated data the wrap opens under, saves and restores its
bytes, and can rewrap it as legacy. New test hooks in
`crates/common/src/za/test_hooks.rs`: `PIN_DECIDE_PAUSE` parks a fan-out after
its pin-record and vault reads, `RETIRE_PAUSE` parks `za_fan_out` after a
recipient's item writes and before phase 4 (reached by ordinary fan-outs and by
`contacts/pin`'s inner call), `REDELIVER_CLEANUP_PAUSE` parks `contacts/pin`
after its event commits and before its cleanup write, and `PIN_WRITE_FAIL_ONCE`
fails one pin write. Cases, with alice, bob and carol as key accounts:

1. First use under a legacy wrap: alice, with no record, invites bob; the
   item is delivered, alice holds a `FirstUse` pin for bob, and her wrap
   opens only as pinned (one revision bump, one batch).
2. Substitute bob's key; alice edits: bob's ATTENDEE shows `3.8`, bob's
   inbox gets no item, alice's `contacts` shows one withheld entry and a
   differing `served_fingerprint`. A second edit keeps one entry with a new
   `refusal_id`.
3. bob's next DAV request, no item pending, repairs his stored key; his
   record exists and his wrap is pinned, so `key_repaired_at` is set.
4. `contacts/pin` with the substituted fingerprint answers 409 `fingerprint
   mismatch`; after `contacts/lookup`, `contacts/pin` with the returned one
   in another case and spacing delivers 1, bob's drain opens it, and
   alice's event shows `1.2` for bob.
5. `contacts/forget` for bob, then a new invitation pins bob as `FirstUse`.
6. Fail closed: delete alice's record under her pinned wrap; her next
   invitation gives `3.8` to every key attendee and writes no record;
   `contacts` reports `record_missing`; `contacts/forget` with null
   recovers.
7. Repair keeps the fail-closed state: delete alice's record under her
   pinned wrap and substitute her own key; the self-check runs once through
   her first DAV drain and once through her `contacts` listing; each time
   the key is repaired, no record is created, and a later invitation still
   gives `3.8`, writes no item and leaves no record.
8. Stale first-use decision: with `PIN_DECIDE_PAUSE`, park carol's
   first-use fan-out after its reads (legacy, no record); complete a
   `contacts/pin` pre-pin (record and rewrap), delete the record, release:
   the stale fan-out fails closed and publishes no record.
9. Healing: rewrap carol's key as legacy while her record holds a pin; her
   next fan-out that writes the record, or her next session's first drain,
   rewraps it in one batch, and deleting the record then fails closed.
10. Replay of the legacy wrap (asserts section 2, limit 2, not a
    protection): save carol's legacy wrap, pin, change her password,
    restore only the wrap and delete the record: first use reopens while
    the new password keeps logging in.
11. Ordinary delivery retires an entry. Reply: refuse bob's DECLINED reply;
    restore alice's key; bob's ordinary ACCEPTED reply delivers the REPLY
    and his current participation; `contacts/pin` then finds no entry and
    alice still shows bob ACCEPTED. Organizer: refuse alice's update to
    occurrence A; restore bob's key; alice cancels occurrence B by an
    ordinary edit; bob receives B's cancellation and A's update (the full
    REQUEST), and the entry is retired only after both items. DELETE:
    refuse bob's ACCEPTED; restore alice's key; bob deletes his event;
    alice receives DECLINED, no full state follows, bob's source is
    deleted, and the entry is retired after the decline.
12. Partial changes: with bob's key substituted, alice changes occurrence A
    and then occurrence B (two refused partial updates); after
    `contacts/pin`, bob's event holds both changes.
13. Stated losses, one test each: a refused CANCEL for a deleted event is
    dropped; a refused removal CANCEL is dropped and bob keeps his copy; an
    occurrence bob declined by deleting it is not reconstructed by the
    redelivered REPLY.
14. Delegate direction: bob delegates to carol, pinned by alice with
    carol's key substituted; alice's delegating merge gives carol `3.8`
    and a withheld entry.
15. Pre-pin: alice pins carol before any invitation; substitute carol's
    key; the first invitation to carol is refused.
16. Pin before deliver: `PIN_WRITE_FAIL_ONCE` on an invitation to a new
    contact leaves `5.1` and no item.
17. Cleanup races, with `REDELIVER_CLEANUP_PAUSE`: park a `contacts/pin`
    after its event commit, substitute bob's key, commit a newer refused
    PUT, release: the newer refusal stays and counts as `remaining`. Then
    two calls A and B both read entry W; B delivers and removes W; a newer
    refusal recreates W; A's cleanup retry leaves the new W in place. With
    `RETIRE_PAUSE`: park an ordinary fan-out after bob's item write, commit
    a newer refusal, release: the newer `refusal_id` survives phase 4; the
    same with `contacts/pin`'s inner fan-out parked, whose step 5 then
    keeps the newer entry.
18. Event identity: alice holds UID U in calendars A and B, both naming
    bob; only B's refused update is redelivered; refusals of both give two
    entries, each redelivered from its own document; a refused source
    deleted and recreated under U is dropped.

Cases restore keys, forget pins and delete events (`assert_is_empty` runs
once at the suite's end); `tests/src/utils/cleanup.rs` exempts field 151
while its account exists, as it does 150.

**Leak tests** (`tests/src/za/leak.rs`). The whole-store scan already covers
every principal property; a pin record holding canary addresses must show
no hit.

**CalDAV client CI** (`tests/caldav_client/`): server suite only; the
harness cannot write the store, and a client sees only a `3.8` parameter.

**Mutation checks** in the style of plan 7: replacing the key equality with
`true` fails case 2; moving the pin write after the item writes fails case
16; dropping the vault assertion from the first-pin batch fails case 8;
keying entries by UID fails case 18; comparing a counter instead of
`refusal_id` in the cleanup fails case 17; trying only the legacy
associated data, or treating an absent record as first use whatever the
wrap, fails case 6; letting the self-check create a record fails case 7;
dropping the healing rewrap fails case 9; retiring without the id check
in phase 4 fails case 17; sending only the full state, or only
upstream's message, on an ordinary delivery with an entry fails case 11;
redelivering upstream's partial message instead of the full REQUEST fails
case 12; removing the fingerprint check
in `contacts/pin` fails case 4.

## 10. Invariants for implementers

Release 1's ten and release 2's five (11 to 15) hold. This spec adds:

16. The pin record is sealed under MK. It is read and written only in a
    request that holds the owner's session keys; background code never
    reads it.
17. Pins are compared before any item is written, and a refusal writes no
    item. A fan-out's phase 2 pin-record write precedes its item writes;
    its retirement write follows them, and a failed retirement leaves the
    entry without undoing an item.
18. Pins are keyed by lowercased address, never by account id.
19. The page's own fingerprint and the self-check derive the public key
    from the private key, never from the stored public key.
20. The pinned wrap is set once and never reverts to legacy. The vault
    record's layout does not change.
21. No key bytes, fingerprints or addresses in traces.
22. Under a pinned wrap, only `contacts/pin` and `contacts/forget` with a
    null address create a pin record. Under a legacy wrap the fan-out (or
    `contacts/pin`) creates the account's first record, atomically with the
    rewrap, which is the moment the account becomes pinned. The self-check
    and the listing never create one.
23. Entries are bound to the source document and never hold a message.
    The next ordinary delivery sends upstream's message, then the full
    current state while the relationship remains; `contacts/pin` sends the
    full current state. An entry is retired only under its consumed
    `refusal_id`.

## 11. Alternatives considered

- **Pins inside the vault record**, as issue #32 first put it. Rejected:
  its revision is the authentication generation (release 1 section 3.1),
  so every pin write would end every open session of the account.
- **Pins keyed by account id.** Rejected: the writer controls the
  address-to-account mapping, and an id-keyed pin accepts a fresh account
  behind bob's address as first use (section 4.3).
- **A `has_pins` bool in the vault record**, in a version 2 layout.
  Rejected: the record is unauthenticated, so a writer clears the bool with
  a plain edit, and a new layout locks older binaries out; the wrap's
  associated data cannot change without MK.
- **Binding the pinned wrap to the vault revision**, so a replayed wrap
  would not open. Rejected: every vault write (a password change, an
  app-password publication) would then need MK and a rewrap. **Binding it
  to the pin record's content** was rejected too: every pin write would
  rewrite the wrap and bump the authentication generation.
- **Storing the refused messages and replaying them.** Rejected: a partial
  organizer message that replaces an older one loses occurrence changes,
  and one appended replays stale state over a newer successful delivery
  (a DECLINED reply arriving after an ACCEPTED one). Current state costs
  the losses of section 2, limit 6, instead.
- **Fingerprints read from the stored key on both sides.** Rejected: the
  writer controls both and can make them agree (section 3.2).
- **Failing the PUT on a mismatch.** Rejected: a generic client error, an
  unsaveable event, every attendee held back; SCHEDULE-STATUS signals per
  recipient (release 2 section 5.3).
- **Strict mode, no trust on first use.** Rejected: every new contact
  blocked until a page visit; pre-pinning gives strictness per contact.
- **Asking the user to resend from the client.** Rejected: an unchanged
  re-save builds no message, so nothing would be sent.
- **Signing invitations.** Out of scope: a signing key per account and a
  different trust root.

## 12. Release pointers

- The page's contacts screen (`#contacts` in `jaylaney/circulo-account`):
  its own plan, against api.md as the server plan updates it (section 6).

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
3. **The REPLY helper's pieces.** Confirm which `pub(crate)` parts of
   upstream's attendee builder (`crates/groupware/src/scheduling/attendee.rs`)
   and `itip_build_envelope` the fork helper of section 6.4 can call.
   Recommendation: the helper lives in `groupware`, with at most a one-word
   visibility edit upstream.
4. **The address the fan-out sees.** Confirm that `message.to` holds the
   `mailto:`-decoded address `Email::new` sanitizes, so that
   `to_canonical_address` gives the same key on the fan-out side and in
   the lookup response (section 4.3). Recommendation: normalize both from
   the sanitized form in one helper.
5. **Client display of `3.8`.** How Apple Calendar, Thunderbird and DAVx5
   show a `3.8` SCHEDULE-STATUS. Recommendation: a manual-checklist item
   (`docs/zero-access/manual-checklist.md`), no code.
