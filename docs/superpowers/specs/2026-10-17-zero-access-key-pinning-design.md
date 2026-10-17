# Zero-access calendar: key pinning for invitations design spec

Date: 2026-10-17. Status: revision 5, draft for review. Scope: the follow-up
to release 2 of the zero-access fork (jaylaney/stalwart) that release 2 spec
section 2 records as a limit ("Key substitution"); GitHub issue #32.
The revision history is in git.

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

Release 2 seals every scheduling message between key accounts to the
recipient's X25519 public key as the store serves it (release 2 section
3.1). A store writer who substitutes bob's public key reads every new
invitation alice sends bob, unnoticed by alice's client. This spec closes
that gap for pinned contacts: the sender's session remembers, under its MK,
the key it sealed to for each address, and refuses to seal to another.

### 1.1 Why the comparison happens at seal time

The attack is silent: the writer replaces bob's `VaultRecord.public_key`,
opens the `pk` wrap alice's fan-out made, rewraps the DEK to bob's real key
and restores bob's key; bob's drain opens the item normally (`pk_unwrap` in
`crates/vault/src/keys.rs` derives the recipient key from the private
key). A check that needs someone looking at the right moment catches almost
nothing, so the comparison happens where the DEK is wrapped, inside
`za_fan_out` (`crates/groupware/src/calendar/fanout.rs`), before any item
is written.

### 1.2 In scope

- A pin record sealed under MK, mapping contact addresses to keys (section
  4), and the pin check in `za_fan_out`: `3.8` on a mismatch, trust on
  first use otherwise (section 5).
- A self-check that repairs a substituted stored key of one's own (3.3).
- Four password-verified endpoints under `/api/vault/contacts`: list, look
  up, pin (redelivering refused changes) and forget (section 6).

### 1.3 Out of scope

- iMIP and external attendees (release 3); pins cover internal recipients.
- The page's contacts screen (`jaylaney/circulo-account`, section 12); the
  server plan updates api.md there.
- Key rotation (none exists, release 2 section 3.2) and signing (section
  11).

### 1.4 Assumptions

- Key change is rare and destructive: only a fresh setup, abandoning the old
  vault, gives an address a new key, so a changed key means the contact
  started over or the store was tampered with; refusing until the sender
  confirms is reasonable.
- Release 1's and 2's assumptions hold. No setting, permission, event type
  or environment knob is added (registry schema and event enums are
  generated, `CLAUDE.md`).

## 2. Threat model additions

**Claim.** Scheduling messages between key accounts are confidential
against a store writer who substitutes public keys, for every contact the
sender has pinned, against a writer without earlier copies of the sender's
pin record or private-key wrap (limit 2), including the first message to a
contact pinned on the page beforehand: a message to a pinned address whose
served key differs is not sealed and no item is written. The trust model is
release 1's and 2's (sessions hold plaintext, clients unmodified,
background code keyless, release 1 invariant 6); pins are the sender's own
data under its MK.

**Limits, stated alongside the claim.**

1. **Trust on first use.** The first message to an unpinned address is
   sealed to the served key, as in release 2, and that key is pinned. If
   the store was substituted at that moment, the first message is exposed
   and the substituted key is pinned. The substitution becomes visible
   afterwards: once the writer restores bob's real key (the fan-out reads
   the served key fresh from bob's vault record, section 5.1), or bob's own
   self-check repairs it (section 3.3), alice's next message to bob is
   refused with `3.8`, and alice's page shows the mismatch.
2. **Replay.** The wrap mark (section 4.4) defends against deletion of the
   pin record, not against replay. A writer holding any earlier copy of
   field 151, the empty record `forget(null)` writes included, reverts the
   pins to that state: contacts pinned later return to first use, and a
   `Confirmed` pin reverts to whatever it was, with no generation bump and
   no signal beyond the page's `source` and `pinned_at`. The empty record
   is the universal replay target. A writer holding the pre-pin
   `private_key_wrap` bytes can paste that one field into the current vault
   record (its associated data names only the purpose and the account, not
   the revision or the other fields) and delete the pin record: first use
   reopens while the current password, recovery key, TOTP and app passwords
   keep working, and nothing signals the user but the page showing no pins.
   The property holds against a writer who holds no earlier copy of either
   field; integrity was never claimed against a store writer (release 1
   section 2).
3. **The sender's own record.** The self-check repairs a substituted key on
   the sender's own vault record; messages sealed to it before the repair
   are exposed unless their senders had pinned the sender.
4. **Lookup reveals key accounts** to any key account holder (section 6.3),
   at a full password verification per lookup. Accepted.
5. **The running server** holds plaintext and pins during a session and can
   seal to any key; pins defend against the store, not the process.
6. **Redelivery sends current state** (sections 5.1, 6.4), not the refused
   messages. Lost: a whole-event CANCEL for a deleted event (the deletion
   retires the document's entries, section 5.2); a removal CANCEL, so the
   contact keeps a stale copy; occurrence declines made by deleting an
   occurrence or adding an EXDATE (the attendee declines again); a refused
   decline whose attendee deletes the copy while the organizer's key still
   mismatches (with a match the DELETE delivers DECLINED); and the
   organizer's VALARMs on a refused first invitation that `contacts/pin`
   redelivers (built with `is_first_request` false).
7. **Out-of-order arrival.** A redelivery and a concurrent ordinary
   delivery can reach the recipient out of order, as can an item arriving
   between `contacts/pin`'s drain pass and its build. A SEQUENCE bump is
   protected by the drain's stale check (release 2 section 6.4); an
   organizer change without one (SUMMARY, DESCRIPTION) can be reverted
   until the organizer's next edit, narrowed by section 6.4 step 4's
   not-retired rule; a REPLY is unprotected until the attendee's next
   change. Upstream shares the gap (`handle_reply` has no timestamp check).
8. **Generation bumps.** Pinning costs one bump; each repair and each heal
   (section 4.4) another. A writer who keeps substituting the account's own
   key forces one per new session, logging its other devices out each time:
   availability, not confidentiality, and simpler denials exist; repairs
   are not bounded.

**Visible metadata added** (beyond release 2 section 2):

- The existence of an account's pin record, its size class (a multiple of
  256 bytes, section 4.1) and when it changes (its stored bytes change on
  every pin write).
- That the account's `private_key_wrap` changed (its bytes and a revision
  bump) at the first pin and at each heal, and the revision bump of each
  repair, but not whether the wrap is pinned or legacy: the associated data
  is not stored.
- That a delivery did not happen: a writer who substituted a key and waits
  for an item learns that the sender had pinned the address.

**Not visible:** whether the wrap is pinned or legacy (though a pin record's
existence suggests the former), who is pinned, the pinned keys and their
fingerprints, the sources and times of pins, the withheld list,
`key_repaired_at`, and the `3.8` status itself, which lives inside the
sender's sealed event (release 1 section 6 seals ATTENDEE and ORGANIZER
properties with their parameters).

## 3. Fingerprints and the self-check

### 3.1 Fingerprint format

The display fingerprint of a 32-byte X25519 public key is the first 16
bytes of SHA-256 over it, lowercase hex in 8 space-separated groups of 4,
on both sides and in every API response, such as
`3f0a 9c41 07be 22d8 e1f0 5a6c 0b93 77e2`. It is a new function,
`display_fingerprint` in `crates/vault/src/keys.rs`, not `keys::fingerprint`
(the per-process keyed BLAKE3 hash for cache keys, release 1 section 5).

### 3.2 An account's own fingerprint

An account's own fingerprint is always derived from its private key: unwrap
`private_key_wrap` under MK (pinned, then legacy associated data, section
4.4; `za_open_private_key` in `crates/common/src/auth/vault.rs`), compute
`x25519_dalek::PublicKey::from(&StaticSecret)`, and fingerprint that; never
`VaultRecord.public_key`, which a store writer controls: read from the
store on both sides, alice's and bob's pages would agree on the writer's
key.

### 3.3 The self-check

Where a session first unwraps its private key (`za_open_private_key`, which
`Server::za_private_key` wraps), the server derives the public key from the
private key and compares it with the `public_key` of the record it just
read (the same read). On a mismatch:

1. Trace under an existing event: the candidate is
   `SecurityEvent::Unauthorized` with a `zero-access:` detail (release 2
   section 6.5 uses it for a forged reply token), such as `zero-access:
   stored public key does not match the private key`, with the account id
   and no key bytes or fingerprints (section 13, item 1).
2. Commit one batch on Principal document 0 through the common helper
   (section 4.5), as the first pin is (section 4.4): the vault record,
   asserted at the bytes of the read, with `public_key` rewritten from the
   derived key; when a pin record exists and opens, that record, asserted
   at its bytes, with `key_repaired_at` set; and when that record sits
   under a legacy wrap, the heal's rewrap in the same batch. With no usable
   record nothing is written to field 151: the self-check never creates a
   pin record, so under a pinned wrap with an absent or unusable record the
   fail-closed state survives a repair (invariant 22). One pass is one
   generation bump and leaves no half-state.
3. The bump ends the account's cached logins and resident keys, the right
   consequence of a tampered record (section 2, limit 8), and the helper's
   invalidation follows. A lost assertion re-reads and re-checks under
   section 4.2's attempt limit; if the stored key now matches, nothing is
   written; after the last attempt the next session checks again.

The check runs only in a request holding the account's session keys. The
drain today unwraps the private key only when a pass finds pending items
(`drain_items` in `crates/groupware/src/calendar/drain.rs`), but the target
of a refused substitution receives none; so every session's first drain
pass runs the check, items pending or not (one vault record read and one
X25519 scalar multiplication per session; release 2's suspension when the
wrap does not open stands). The contacts endpoints run it under the same
rule, and the page's "your fingerprint" comes from it.

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
| | `revision` | `u64` | Incremented by every write. Informational; the conditional write asserts the stored bytes (section 4.2), and the revision is no defence against replay (section 2, limit 2). |
| | `pins` | `Vec<ZaPin>` | One per address, unique by `address`. |
| | `withheld` | `Vec<ZaWithheld>` | Refused deliveries awaiting redelivery, unique by (`document_id`, `uid`, `address`). No cap: only the owner's sessions add entries (its fan-outs, `contacts/pin`'s inner fan-out, the drain's delegating merge). |
| | `key_repaired_at` | `Option<i64>` | Unix seconds of the last self-check repair (section 3.3). |
| `ZaPin` | `address` | `String` | The contact's address, normalized (section 4.3). |
| | `public_key` | `[u8; 32]` | The pinned X25519 public key. |
| | `pinned_at` | `i64` | Unix seconds. |
| | `source` | `PinSource` | `FirstUse` (the fan-out pinned the served key) or `Confirmed` (the user pinned it on the page). |
| `ZaWithheld` | `document_id` | `u32` | The source event's document id: the sender's event whose fan-out refused (the PUT's document, or the organizer event of the drain's delegating merge; a deletion records no entry, section 5.1). |
| | `uid` | `String` | The source event's UID. |
| | `address` | `String` | The refused recipient's address, normalized as pins are (section 4.3). |
| | `since` | `i64` | Unix seconds of the first refusal. |
| | `refusal_id` | `u64` | 64 random bits, regenerated on every refusal of the entry (a new entry gets one too), so a delivery retires exactly the refusal it consumed (section 5.2, phase 4). |

An entry's identity is (`document_id`, `uid`, `address`): document ids are
a monotonic per-account counter never reused, a MOVE keeps the id and a
COPY gives one document two names, so no calendar id is part of it. An
entry holds no message (delivery rebuilds current state, sections 5.1 and
6.4) and is small. The structs have a silent `Debug` (they hold addresses).

### 4.2 Reads and writes

- **Read fresh** on every fan-out and endpoint call, never cached in
  `SessionKeys` (a session cache goes stale across sessions and nodes).
- **Writes are conditional** on the previous stored value. The pin-record
  assertion is `AssertValue::Hash` of the raw stored bytes whenever any
  value exists, usable or not, and `AssertValue::None` only when the read
  returned nothing, as the UID lease asserts its key
  (`crates/common/src/za/lease.rs`). Under a pinned wrap a write asserts
  only the pin record; under a legacy wrap every pin-record write is the
  two-assertion batch of section 4.4.
- **Attempts.** A pin-record write, single assertion or batch, that loses
  an assertion re-reads, re-runs its decision against the fresh state (for
  the fan-out, every comparison of section 5.1) and tries again, up to
  eight attempts in total. The eighth loss is the failure: `5.1` for that
  fan-out's key-account recipients, 409 `{"error": "conflict"}` from an
  endpoint. The conflicts are between the account's own sessions over a
  small record, and two attempts made a new source of `5.1`.
- **Only with the owner's MK**, in a request holding the owner's session
  keys (a DAV request's, or an endpoint's freshly verified ones).
- **Removed with the account** by `danger_destroy_account`
  (`crates/store/src/dispatch/store.rs`), field 151 included.

### 4.3 Keyed by address, not account id

Pins are keyed by the normalized address, never by account id: the writer
also controls the address-to-account mapping (`za_account_id_from_email`,
`crates/common/src/cache/principals.rs`), and pointing bob's address at a
fresh account with a keypair of its own would pass an id-keyed pin as a
first use. Keyed by address, the new key differs and is refused.

One helper normalizes addresses for the fan-out key, the pins and the
endpoints: `Email::new`'s sanitization (`sanitize_email` in
`crates/utils/src/lib.rs`: whitespace stripped, lowercased, IDNA), then
`to_canonical_address`. The fan-out reaches it through `Email::new`, which
first decodes `mailto:` and extracts an addr-spec; the endpoints apply it
to the string as given and answer 400 when sanitization fails, so
`Bob <bob@x>` and `mailto:bob@x` are not accepted. Two addresses that
resolve to one account (an alias, a sub-address) are pinned separately.

### 4.4 The pinned wrap and failing closed

Whether an account has pinned is carried by the associated data of its
private-key wrap, not by a stored field. Release 2 wraps the private key
under MK with `aad("private-key", account_id)` (`AAD_PRIVATE_KEY`); this
spec calls that the legacy wrap. The account's first pin write rewraps the
private key under `aad("private-key/pins", account_id)`
(`AAD_PRIVATE_KEY_PINNED`), the pinned wrap, in the same atomic batch as
the pin record (below), through the common commit helper (section 4.5), in
a request that holds MK. That is the one generation bump pinning itself
costs (repairs and heals cost their own, section 2, limit 8); later pin
writes leave the vault record alone (section 11). The vault record's
layout and the wrap's length do not change, so no stored struct changes
(release 1 invariant 1).

**Reading the mark.** Every reader of the private key (the drain's
`za_open_private_key`, the fan-out, the contacts endpoints) tries the
pinned associated data first, then the legacy:

| Wrap opens under | Meaning | An absent or unusable pin record |
|---|---|---|
| pinned | the account has pinned | fails closed |
| legacy | the account never pinned | is legitimate: first use; an unusable record is overwritten by the first-pin batch, asserted at its bytes |
| neither | tampered or corrupt | the drain keeps release 2's suspension (its section 10); the fan-out fails closed; the endpoints answer 409 `{"error": "vault record missing"}` |

A record is unusable when it does not open, does not decode, or has a
version other than `1`. A record that opens is proof enough (a writer
cannot forge one under MK), so the fan-out reads the mark, fresh from the
vault record, only when the record is absent or unusable or it is about to
write the record. The mark is a property of one vault read: the mark, the
asserted hash and the wrap bytes a batch rewraps all come from the same
`VaultRead`, and `SessionKeys` caches the private key only, never the mark.

**Fail closed.** Under a pinned wrap (or one that opens under neither), an
absent or unusable pin record makes the fan-out refuse every key-account
recipient with `3.8` (section 5.1) and write nothing to the pin record: no
first-use pins and no withheld entries, because any write would create a
fresh record and end the state. Messages refused in this state are not
redelivered; the sender's next edit after recovery sends them. Recipients
that are not internal are unaffected (not attempted, `5.3`, as today).

The first-pin batch is atomic, so a missing or unusable record under a
pinned wrap cannot arise from the server's own writes: it is evidence of
tampering or data loss. The page orders the recovery accordingly (section
6.6): look each contact up and re-pin it after an out-of-band comparison
(`contacts/pin` writes a fresh record holding that pin); `contacts/forget`
with a null address, which writes an empty record and so returns every
contact to first use, is the last resort, and the page says why.

**The first pin write is one batch.** Both fields live on the account's
Principal document 0, so one `BatchBuilder` batch carries two
`assert_value`/`set` pairs, the assertion before the set of each key (as
PostgreSQL and MySQL require): the pin record asserted as section 4.2 states
(`None` when the read returned nothing, else `Hash` of whatever it returned,
garbage included), and the vault record at the bytes read (`AssertValue::Hash`,
as `vault_write_store` does), writing the pin record and the rewrapped
`private_key_wrap` together. A lost assertion re-reads both and decides again
(section 4.2's attempts), so a stale "legacy, no record" observation never
creates a record once the account is pinned: the vault assertion fails and the
re-read finds the pinned wrap (failing closed if the record was deleted
meanwhile). A record under a legacy wrap arises only from a replayed wrap
(section 2, limit 2); the next reader that writes the record (a fan-out, the
session's first drain pass, the self-check, an endpoint) heals it with the same
batch.

A batch inside a drain pass or a PUT drops the sender's own cache entry
mid-request; benign: the request finishes on its `Arc<SessionKeys>`, the
next one re-authenticates, and its drain state starts afresh.

**What a store writer can and cannot do.** Producing a legacy wrap needs
MK. Deleting the pin record fails closed, as does replacing it with another
account's or with garbage, and a self-check repair does not end the state
(section 3.3). The writer can replay (section 2, limit 2): an earlier copy
of field 151 reverts the pins, and a saved pre-pin wrap with the record
deleted reopens first use. The property holds against a writer who holds
no earlier copy of either field.

### 4.5 Vault writes outside the HTTP module

The rewrap and the repair are vault writes made by the fan-out
(`crates/groupware`), the drain and self-check (`crates/common`) and the
endpoints, but `za_commit` lives in `crates/http/src/api/vault.rs`. The
revision bump, conditional write and invalidation move into a common
helper in `crates/common/src/auth/vault.rs`, which `za_commit` wraps and
extends with orphan pruning, and which also builds section 4.4's batches.
`CLAUDE.md`'s rule becomes "every vault write goes through the common
helper or `za_commit`"; the plan updates `CLAUDE.md`.

## 5. The fan-out

### 5.1 The per-recipient decision

All sealed deliveries go through `za_fan_out`, so the check lives there
once: the organizer's PUT and DELETE (`crates/dav/src/calendar/update.rs`,
`delete.rs`, a calendar DELETE per event), the attendee's reply by PUT or
DELETE, and the drain's delegating merge. `za_fan_out` gains a source
parameter (`document_id`, `uid`), a deletion flag and the `redelivery` flag
of section 5.2; the create path assigns the document id before the fan-out
(a monotonic counter; a failed create's id is never used, section 5.5),
and `ZaDeleteMessage` carries the source.

**The served key** is read fresh from the recipient's vault record
(`Server::za_vault_record(recipient).public_key`, one store read per
key-account recipient per fan-out); the fan-out compares and seals to it.
The account cache still resolves the address and its checks
(`resolve_inner`), but its `za_public_key` (size-bounded, no TTL, not
invalidated by a direct store write) is not used. A record with no usable
public key makes the recipient not internal.

| Pin record | Wrap | Pin for the address | Entry for this document | Served key | Outcome |
|---|---|---|---|---|---|
| opens | read only if the record is written | present | none | equal to the pin | deliver as today (`1.2`, or `5.1` on a write failure) |
| opens | read only if the record is written | present | present | equal to the pin | the two-item rule (below), then retire the entry (section 5.2) |
| opens | read only if the record is written | present | any | different | refuse: no item, `3.8`, a withheld entry |
| opens | read only if the record is written | absent | none | any | pin the served key as `FirstUse`, then deliver |
| opens | read only if the record is written | absent | present (kept by `forget`, section 6.5) | any | pin as `FirstUse`, then the two-item rule and retirement |
| absent or unusable | legacy | (none) | (none) | any | pin as `FirstUse` in a fresh record with the rewrap (one batch, asserted at whatever the read returned, section 4.4), then deliver |
| absent or unusable | pinned, or opens under neither | (unknown) | (unknown) | any | refuse: no item, `3.8`, nothing recorded (section 4.4) |

A refusal writes no item for the recipient, sets its property (the
ATTENDEE, or the ORGANIZER when `on_organizer`) to `3.8` through
`za_set_schedule_status`, and records a withheld entry for (source
`document_id`, `uid`, address) with a fresh `refusal_id`, or gives the
existing entry a fresh one (section 4.1). A deletion's fan-out records no
entry, since nothing can be redelivered from a deleted document (section
5.2, phase 4). The sender's request succeeds and the other recipients are
delivered to, as release 2 section 5.3 handles a per-recipient failure.
The comparison is a byte equality of public values. Recipients that are
not internal (release 2 section 5.2) are not attempted and never pinned;
non-key senders never reach `za_fan_out` (release 1 invariant 9). A
"deliver" decision records no entry.

**The two-item rule.** A fan-out delivering to an address with a withheld
entry for this document writes up to two items for that recipient, in
order:

1. Upstream's own message for this change, exactly as without pins: a
   partial or full REQUEST, a partial or whole-event CANCEL, or the
   change's REPLY, a DELETE's DECLINED reply and this change's occurrence
   declines included. When an attendee's DELETE yields nothing to send
   (the attendee is already DECLINED or DELEGATED, so `attendee_decline`
   skips it), the fan-out builds the DECLINED REPLY itself
   (`attendee_decline` with the skip disabled) because an entry exists.
2. The full current state, only while the relationship remains after the
   change: for an organizer, the committed event still names the address
   (not after a whole-event CANCEL or a removal); for an attendee, the
   event still exists (not after a DELETE). For an organizer it is
   `organizer_request_full(committed_tree, fresh_snapshot, None, false)`
   for that attendee, without a second `itip_finalize` (no second SEQUENCE
   bump); for an attendee, the current-participation REPLY of section
   6.4's helper. Both are built from the state being committed, never from
   a superseded tree: the deletion flag says the tree is the pre-deletion
   one. Item (2) is skipped when item (1) already is
   `organizer_request_full` output addressed to the recipient (a master
   change, an EXDATE occurrence cancel, the drain's delegating message),
   and when the helper finds nothing to send.

The entry is retired after all of that recipient's items are written
(section 5.2, phase 4); an item write failure (`5.1`) leaves it. Upstream's
message carries the intended change, the full state everything withheld,
and nothing comes from a tree that contradicts the action; the cost is a
second item while an entry exists. Entries drain once deliveries flow
again, and `contacts/pin` redelivers only documents nobody edited.

### 5.2 Order

`za_fan_out` today resolves and writes recipient by recipient; with pins it
runs in four phases:

1. Resolve every recipient of every message (the per-call recipient map already
   deduplicates addresses), read each key-account recipient's served key
   (section 5.1) and the pin record, keeping the `refusal_id` of each entry it
   will consume; when the record is absent or unusable, or when phase 2 or 4
   will write it, read the mark from the vault record (section 4.4; under a
   legacy wrap each such write is that section's batch). A fan-out that changes
   nothing reads neither the mark nor the private key.
2. Decide every key-account recipient against the record (section 5.1), and
   when anything changed (new first-use pins, any refusal), write the pin
   record once, conditionally (section 4.2), before any item. Under a
   legacy wrap that write is the batch with the rewrap (section 4.4).
3. Write the items of the recipients decided "deliver", up to two for a
   recipient with an entry (section 5.1).
4. Retire, in one more conditional pin-record write asserting the bytes
   phase 2 wrote (or those read in phase 1 when phase 2 wrote nothing):
   the entries of recipients whose items were all written, and for a
   deletion's fan-out every entry of the deleted document. The one
   retirement rule, here and in `contacts/pin` step 5: an entry is removed
   only when its stored `refusal_id` still equals the id the delivery
   consumed (read in phase 1), checked on every attempt of section 4.2,
   the retries after a conflict included. Otherwise a newer refusal
   re-armed the entry, and it stays. A failed or skipped retirement leaves
   the entry and never undoes a written item. When `contacts/pin` sets
   `redelivery`, the fan-out sends only the message it was given (already
   the full state), adds no second item and retires nothing: the
   endpoint's guarded step 5 is its only cleanup.

Pinning first, a crash between phases 2 and 3 leaves a pin without a
delivery, which the next edit that builds a message to the address
delivers, never a delivery without a pin. A fan-out whose decisions change
nothing writes no pin record in phase 2.

### 5.3 The status code

A new constant `ITIP_STATUS_KEY_REFUSED = "3.8"` (RFC 5546 "no
authority"), in the fork's `fanout.rs` next to `status_rank`, not in
upstream's `itip.rs`. `status_rank` gives it rank 3, above `5.1` (2) and
`5.3` (1), so `record_status` keeps it over any other outcome in one
fan-out. It is the bare code (RFC 6638 section 3.2.9 allows codes only);
the page explains it. A later delivery replaces it
(`za_set_schedule_status` keeps only upstream's `3.7`); a PUT that builds no
message for the address keeps it.

### 5.4 Failures

- A pin record read failure, or a phase 2 write that fails or loses its
  eighth attempt (section 4.2), is the delivery failure (`5.1`) of every
  key-account recipient of that fan-out, and nothing is written for them;
  it is traced with the sender's account id and no address. It is a `5.1`
  outcome, never an error returned from `za_fan_out`: the create path and
  the drain's merge read an assertion-failure `Err` as a lost UID lease
  fence. A failed phase 4 retirement is not a failure: the items stand and
  the entry stays.
- A refusal is traced once per fan-out under the same event as the
  self-check (section 3.3), with the sender's account id and the count of
  refused recipients only: no address, no recipient account id (it would
  record who invited whom where no item shows it), no fingerprint.

### 5.5 The UID lease fence

The pin write carries no UID lease fence (release 2 section 5.4): the
record is the sender's own and hands a recipient nothing. A fan-out that
loses its fence or its source commit after the pin write leaves first-use
pins, entries for a never-committed source (dropped at redelivery), and
entries it retired; the next edit sends the state again.

### 5.6 Replies and delegates

- **Replies.** bob's reply seals to alice's key, so bob pins alice's
  address on his first reply. A refused reply puts `3.8` on the ORGANIZER
  of bob's copy and a withheld entry for (his copy's document, alice's
  address). bob's next ordinary reply sends that change's REPLY, then his
  current participation (unless he deleted the event); a DELETE delivers
  DECLINED; `contacts/pin` sends his current participation alone.
- **Delegates.** The delegating merge fans out in the organizer's session
  (release 2 section 6.4), so the organizer's pins apply: a refused
  delegate gets `3.8` on its ATTENDEE in the organizer's event, where that
  fan-out's statuses go, and a withheld entry for (the organizer event's
  document, the delegate).

## 6. Account API additions

### 6.1 Common to the four endpoints

Four endpoints join the eight under `/api/vault/` (release 1 section 4.1).
`handle_vault_request` in `crates/http/src/api/vault.rs` routes (`contacts`,
none), (`contacts`, `lookup`), (`contacts`, `pin`) and (`contacts`, `forget`),
the shape `app-password/revoke` uses, to a new module
`crates/http/src/api/contacts.rs`, which shares `vault.rs`'s verification and
response helpers (`za_verify_primary`, `za_reread_verified`, `conflict`,
`json`). All are `POST` with JSON and follow api.md's conventions exactly
(CORS, `Cache-Control: no-store`, the anonymous-request rate limit, the two
error body shapes); all are password-verified with api.md's shared fields and
status table, whose heading changes from five endpoints to nine. Addresses in
requests are normalized by section 4.3's helper; one that fails sanitization is
a 400.

Verification builds the request's own `SessionKeys` (not a key-cache
entry). Each endpoint unwraps the private key (pinned, then legacy) and
runs section 3.3's self-check, repair included; a wrap that opens under
neither answers the `vault record` 409 family. The repair and the
rewrap are the only vault writes an endpoint may commit on top of a
revision other than the one it verified (release 1 section 4.1): they copy
every other field from the re-read and change nothing login-relevant.
Every other endpoint vault write keeps the verified-generation rule. Bodies
and responses stay out of traces (release 1 section 10). Everything runs
in this HTTP request, which holds plaintext while it redelivers, as any
session request does; `contacts/pin` calls the drain itself (`za_drain`
with the request's `SessionKeys`, section 6.4), and the DAV trigger in
`handle_dav_request` (`crates/dav/src/request.rs`) is not involved.

### 6.2 POST /api/vault/contacts

Purpose: the page's contacts view. Request: the shared fields only.

Success (200):

| Field | Type | Meaning |
|---|---|---|
| `fingerprint` | string | The account's own fingerprint, derived from its private key (section 3.2). |
| `stored_key_matches` | boolean | Whether the stored public key equalled the derived one when this request read it, before any repair it made. |
| `key_repaired_at` | integer or null | Unix seconds of the last self-check repair recorded in an existing pin record (section 3.3). |
| `record_missing` | boolean | The wrap is pinned and the pin record is absent or unusable: the fail-closed state, evidence of tampering or data loss; recovery order in section 6.6. |
| `pins` | array | One object per pin, below; empty when `record_missing`. The listing creates no record. |

Each pin object:

| Field | Type | Meaning |
|---|---|---|
| `address` | string | The pinned address. |
| `fingerprint` | string | The pinned key's fingerprint. |
| `pinned_at` | integer | Unix seconds. |
| `source` | string | `"first-use"` or `"confirmed"`. |
| `served_fingerprint` | string or null | The fingerprint of the key the server would seal to now, read fresh as the fan-out reads it (section 5.1); null when the address no longer resolves to an internal key account. |
| `withheld` | integer | The number of this address's withheld entries: source events refused and not delivered since, including entries a later `contacts/pin` drops (section 6.4, step 1). |

| Code | When | Body |
|---|---|---|
| 200 | Listed | the object above |
| 400, 401, 402, 409, 429 | As in the shared table | |

### 6.3 POST /api/vault/contacts/lookup

Purpose: a contact's fingerprint, for comparison out of band. Request: the
shared fields and `address` (string, required).

Success (200): `{"address": "<normalized address>", "fingerprint":
"<fingerprint>"}`, the address in the form a pin is keyed by (section 4.3).

The address resolves as the fan-out resolves a recipient (section 5.1:
`za_account_id_from_email`, a key account, its served key read fresh from
its vault record); the receive permission is not checked, since a pin
where nothing is delivered is harmless. Looking up oneself returns the
served key. `not a key account` is not counted as a failed authentication.

| Code | When | Body |
|---|---|---|
| 200 | Found | the object above |
| 400 | Shared causes; `address` missing, or not a sanitizable address (section 4.3) | problem body |
| 401, 402, 429 | As in the shared table | |
| 409 | The address is not a local key account (unknown addresses, plain accounts and accounts pending setup get the same answer) | `{"error": "not a key account"}` |
| 409 | Other shared 409s | as in the shared table |

### 6.4 POST /api/vault/contacts/pin

Purpose: confirm a contact's key and redeliver what was withheld for the
address; also the main recovery path from the fail-closed state. Request:
the shared fields, `address` and `fingerprint` (both strings, required;
the fingerprint as `lookup` returned it, any case and spacing, compared as
the 16 decoded bytes, as `recover` tolerates the recovery key's form).

The endpoint resolves the address as `lookup` does and pins the served key
only if its fingerprint equals the given one, with source `Confirmed`,
replacing any pin for the address. The served fingerprint is not in the
refusal body; the page looks the address up again. The pin is written
first, creating a fresh record when the read found none or an unusable one
(asserted as section 4.2 states; under a legacy wrap, the batch with the
rewrap). Then it runs the account's drain pass (`za_drain` with the
request's `SessionKeys`), as DAV paths drain before handling: the drain's
stale check rejects a whole REPLY when any instance of the attendee's copy
is behind the organizer, which an undrained partial REQUEST makes ordinary
in a page-only flow. Then up to 32 withheld entries for the address are
redelivered, the rest left for the next call, each with its `refusal_id`
read at the start:

1. Load the source document by `document_id` and unseal it. Drop the entry
   if the document is gone, belongs to another account, or carries another
   UID than the entry's; if it no longer names the address (as an ATTENDEE
   when the user organizes it, as the ORGANIZER otherwise); or if it fails
   the `SendSealed` preconditions (release 2 section 5.1: scheduling
   enabled, `CalendarSchedulingSend`, an event not in the past). No
   account-wide UID lookup is made: two events with one UID in two
   calendars are separate entries.
2. Build the full current state for that one recipient. When the user
   organizes the event: the full REQUEST for that attendee through the
   path the drain's delegating merge uses for a new delegate
   (`organizer_request_full`, release 2 section 6.4), under the
   organizer's UID lease passed as the fence (release 2 section 6.3, step
   5). The refusing fan-out already created the recipient's token, so no
   minting is expected; the lease is cheap and keeps redelivery consistent
   with the delegating merge. When the user is an attendee: the fork
   helper's REPLY, which mirrors `attendee_handle_update`'s instance
   selection minus its "changed" predicate: only instances where the local
   attendee is a server-scheduling participant (`is_server_scheduling`,
   RSVP not FALSE, ORGANIZER not SCHEDULE-AGENT=CLIENT) and whose STATUS is
   not CANCELLED, with the ATTENDEE line and its DELEGATED-FROM and
   DELEGATED-TO lines as upstream exports them (section 13, item 3). The
   filter is needed because `handle_reply` rejects a whole reply when any
   instance names a non-participant (`SenderIsNotParticipant`) and adds an
   override for an instance the organizer lacks
   (`MergeAction::AddComponent`), while an attendee's copy keeps a
   cancelled override naming the attendee after a partial CANCEL.
3. Run `za_fan_out` with that message, the loaded event as the sender's
   tree, its current token pairs (`event_token_pairs`) and `redelivery`
   set (section 5.2); it finds the pin and delivers.
4. Write the status with `za_set_schedule_status` and reseal the event as
   the drain's merge does (`seal_event` with the returned pairs, under the
   assertion on the stored archive), with no schedule-tag change. On a lost
   archive assertion the endpoint re-reads the archive and retries once,
   re-deriving everything from the fresh archive: the reseal uses the
   fresh archive's `event_token_pairs` (never the pairs read at step 1,
   which would drop a concurrently added attendee's token), and the `1.2`
   is applied only if the fresh event does not carry a `3.8` for the
   address (a newer refusal); a second loss skips the status write. In
   either lost-assertion case the entry is not retired and counts as
   `remaining`: a concurrent change happened, and the next ordinary
   delivery sends the full state again.
5. Cleanup, one conditional pin-record write under section 5.2's
   retirement rule (the `refusal_id` read at the start, on every attempt):
   an entry re-armed during the call stays and counts as `remaining`, not
   as delivered. Failed entries stay withheld.

Every outcome of an entry is one of these:

| Outcome | When | Entry |
|---|---|---|
| `delivered` | the item is written and step 4 committed | retired under the rule |
| `dropped` | step 1's conditions; the full REQUEST yields nothing for the address (it is DECLINED, RSVP=FALSE or SCHEDULE-AGENT=CLIENT); the REPLY helper yields nothing (the master is cancelled or no instance qualifies); the address no longer resolves to an internal key account (the next ordinary edit routes it as upstream does) | retired under the rule |
| `failed` | the UID lease is busy; an item write fails | stays |
| `remaining` | beyond the 32 of this call; re-armed during it; step 4 lost its archive assertion | stays |

Why current state: a full REQUEST covers every refused partial change and
never reverts a newer delivery, nor does a current-participation REPLY;
`contacts/pin` makes no change of its own, so it sends only the full state.
Its losses are section 2, limit 6 (after a real key change the contact's
old copy no longer exists, which the page says); its race is limit 7.

Redelivery failures never fail the pin. Success (200): `{"ok": true,
"delivered": <int>, "failed": <int>, "dropped": <int>, "remaining":
<int>}`.

| Code | When | Body |
|---|---|---|
| 200 | Pinned; redelivery counts as above | the object above |
| 400 | Shared causes; `address` missing or not sanitizable; `fingerprint` missing or not 32 hex digits once spaces are removed | problem body |
| 401, 402, 429 | As in the shared table | |
| 409 | The address is not a local key account | `{"error": "not a key account"}` |
| 409 | The served key's fingerprint differs from `fingerprint` | `{"error": "fingerprint mismatch"}` |
| 409 | The pin write lost its eighth attempt (section 4.2) | `{"error": "conflict"}` |
| 409 | Other shared 409s | as in the shared table |

### 6.5 POST /api/vault/contacts/forget

Purpose: remove a pin, or start over. Request: the shared fields and
`address` (string or null; the key must be present, as `otp_auth` on
`totp`).

- With an address: removes its pin and keeps its withheld entries,
  harmless without a pin, which the next delivery drains after pinning the
  served key as `FirstUse` (section 5.1). No pin: a no-op, still 200.
- With null: writes an empty record (`key_repaired_at` kept), ending the
  fail-closed state, the wrap still pinned; over an unusable record it
  asserts those bytes (section 4.2). It is the last resort of section
  4.4's recovery: every contact returns to first use, and the empty record
  is the universal replay target (section 2, limit 2).
- Under a legacy wrap: with no record, both forms write nothing (200);
  with a record, both run the heal batch, the rewrap with the record as
  modified (section 4.4).

Success (200): `{"ok": true}`.

| Code | When | Body |
|---|---|---|
| 200 | Forgotten, or nothing to forget | `{"ok": true}` |
| 400 | Shared causes; `address` absent, or present and not sanitizable | problem body |
| 401, 402, 429 | As in the shared table | |
| 409 | The write lost its eighth attempt (section 4.2) | `{"error": "conflict"}` |
| 409 | Other shared 409s | as in the shared table |

### 6.6 Flows (for api.md)

Check or pre-pin: `contacts/lookup`, compare out of band, `contacts/pin`.
Key changed: `contacts` shows a differing `served_fingerprint` and a
`withheld` count; `contacts/pin` pins and redelivers. `record_missing`:
explain that the record was deleted or damaged, re-pin each contact after
an out-of-band comparison, and offer `contacts/forget` with null last,
saying it returns every contact to first use (section 4.4).

api.md's "What the page never sees" is unchanged: no key material is
returned; fingerprints and addresses go only to the record's owner.

## 7. Stored format additions

| Field | Stored value | Sealed under, associated data |
|---|---|---|
| `ZeroAccessPins` (151, new) | nonce (24) then XChaCha20-Poly1305 of: length (`u32`, big endian), rkyv `ZaPinRecord` (section 4.1; entries are `document_id`, `uid`, `address`, `since`, `refusal_id`), zero padding to a multiple of 256 bytes; tag (16) | MK, `aad("pins", account_id)` |
| `ZeroAccessVault` (150), `private_key_wrap` | unchanged layout and length (`nonce || ciphertext`, 72 bytes) | MK; `aad("private-key", account_id)` (legacy, release 2) until the first pin write, `aad("private-key/pins", account_id)` (pinned) from then on |

`3.8` joins `1.2`, `5.1` and `5.3` inside the sender's sealed ORGANIZER
and ATTENDEE properties. The item format, the `X-ZA-*` carriers, the token
pairs and the policy version do not change.

## 8. Error handling

Endpoints use api.md's body shapes and shared table plus section 6's 409s;
the fan-out follows section 5.4; the self-check follows section 3.3; a
failed redelivery entry stays withheld. Only the repair and the rewrap may
commit on top of an unverified revision (section 6.1). No new settings,
event types, permissions or knobs; traces carry `zero-access:` details and
no key bytes, fingerprints or addresses.

## 9. Testing

**Unit.** In `crates/vault`: `display_fingerprint` matches a fixed vector
and the display form; a pin record round-trips through seal and open, fails
under another account's associated data and under another MK, pads to 256
bytes, and an unknown `version` reads as unusable; a pinned wrap does not
open under the legacy associated data nor a legacy wrap under the pinned,
and the reader's pinned-then-legacy order reports which one opened; every
refusal changes an entry's `refusal_id`. In `fanout.rs`: `status_rank`
ranks `3.8` above `5.1`, and `za_set_schedule_status` replaces a `3.8`
with a later status. In `groupware`: the REPLY helper carries the master's
and each qualifying override's PARTSTAT, omits cancelled instances and
those where the attendee is not server-scheduled, and yields nothing when
the master is cancelled.

**Integration.** A new za sub-module, `tests/src/za/pinning.rs`, after the
drain modules and before the leak tests, drain on (`DrainOn`). Helpers in
`tests/src/utils/za.rs`: `substitute_public_key` (rewrites an account's
`public_key` at the next revision, with or without cache invalidation, and
returns the original) and a wrap helper (reports which associated data
opens the wrap, saves and restores its bytes, rewraps it as legacy). Hooks
in `crates/common/src/za/test_hooks.rs`: `PIN_DECIDE_PAUSE` (a fan-out
after its reads), `RETIRE_PAUSE` (`za_fan_out` after a recipient's items,
before phase 4, also reached by `contacts/pin`'s inner call),
`REDELIVER_CLEANUP_PAUSE` (`contacts/pin` after its event commit, before
cleanup) and `PIN_WRITE_FAIL_ONCE`. Cases, with alice, bob and carol as key
accounts; "item order" is the recipient's inbox in document-id order:

1. First use under a legacy wrap: alice, with no record, invites bob; the
   item is delivered, alice holds a `FirstUse` pin for bob, and her wrap
   opens only as pinned (one revision bump, one batch).
2. Substitute bob's key; alice edits: bob's ATTENDEE shows `3.8`, bob's
   inbox gets no item, alice's `contacts` shows one withheld entry and a
   differing `served_fingerprint`. A second edit keeps one entry with a new
   `refusal_id`. The same holds when the substitution invalidates no cache
   (the served key is read fresh).
3. bob's next DAV request, no item pending, repairs his stored key in one
   batch; his record exists, so `key_repaired_at` is set.
4. `contacts/pin` with the substituted fingerprint answers 409 `fingerprint
   mismatch`; after `contacts/lookup`, `contacts/pin` with the returned one
   in another case and spacing delivers 1, bob's drain opens it, and
   alice's event shows `1.2` for bob.
5. `contacts/forget` for bob keeps his entry; the next invitation pins bob
   as `FirstUse` and drains the entry under the two-item rule.
6. Fail closed: delete alice's record under her pinned wrap; her next
   invitation gives `3.8` to every key attendee and writes no record;
   `contacts` reports `record_missing`; `contacts/pin` for each contact
   recovers, and so does `contacts/forget` with null.
7. Repair keeps the fail-closed state: delete alice's record under her
   pinned wrap and substitute her own key; her first DAV drain repairs it;
   substitute it again; her `contacts` listing repairs it. Each time no
   record is created, and a later invitation still gives `3.8`, writes no
   item and leaves no record.
8. Stale first-use decision: with `PIN_DECIDE_PAUSE`, park carol's
   first-use fan-out after its reads (legacy, no record); complete a
   `contacts/pin` pre-pin (record and rewrap), delete the record, release:
   the stale fan-out fails closed and publishes no record.
9. Healing: rewrap carol's key as legacy while her record holds a pin; her
   next fan-out that writes the record, or her next session's first drain,
   rewraps it in one batch, and deleting the record then fails closed.
10. Replay (asserts section 2, limit 2, not a protection): save carol's
    legacy wrap, pin, change her password, restore only the wrap and delete
    the record: first use reopens while the new password keeps logging in.
    Restoring an earlier pin record reverts a `Confirmed` pin silently.
11. Ordinary delivery with an entry, asserting item order. Reply: refuse
    bob's DECLINED, restore alice's key; bob's ACCEPTED delivers the REPLY
    then his current participation; `contacts/pin` finds no entry and alice
    shows bob ACCEPTED. Organizer: refuse an update to occurrence A,
    restore; alice adds an override of B with STATUS:CANCELLED; bob gets
    B's partial CANCEL, then the full REQUEST with A's update, and the
    entry is retired after both; a master change gives one item. DELETE:
    refuse bob's ACCEPTED, restore, bob deletes: alice gets DECLINED and no
    full state, the source is gone, the entry retired after the decline.
    Already DECLINED: the same, and the fan-out builds the DECLINED itself.
12. Partial changes: with bob's key substituted, alice changes occurrence A
    and then occurrence B (two refused partial updates); after
    `contacts/pin`, bob's event holds both changes.
13. Stated losses, one test each: an organizer DELETE retires the
    document's entries and bob keeps his copy; a refused removal CANCEL is
    dropped and bob keeps his copy; an occurrence bob declined by deleting
    it is not reconstructed by the redelivered REPLY.
14. Delegate direction: bob delegates to carol, pinned by alice with
    carol's key substituted; alice's delegating merge gives carol `3.8`
    and a withheld entry.
15. Pre-pin: alice pins carol before any invitation; substitute carol's
    key; the first invitation to carol is refused.
16. Pin before deliver: `PIN_WRITE_FAIL_ONCE` on an invitation to a new
    contact leaves `5.1` and no item.
17. Cleanup races. `REDELIVER_CLEANUP_PAUSE`: park `contacts/pin`, commit a
    newer refused PUT, release: the refusal stays as `remaining`; calls A
    and B read W, B removes W, a refusal recreates W, A's retry keeps it.
    `RETIRE_PAUSE`: park an ordinary fan-out after bob's item, commit a
    newer refusal, release: the new `refusal_id` survives phase 4; the same
    through `contacts/pin`'s inner call, whose step 5 keeps the entry.
18. Event identity: alice holds UID U in two documents in calendars A and
    B, both naming bob; only B's refused update is redelivered; refusals of
    both give two entries, each redelivered from its own document; a
    refused source deleted and recreated under U is dropped.
19. Participation filter: alice removes bob from one occurrence; alice's key
    is substituted; bob's master ACCEPTED is refused; restore the key;
    `contacts/pin`: alice applies ACCEPTED (not `SenderIsNotParticipant`
    history) and gains no override.
20. Drain before build: refuse bob's master reply; deliver a partial
    REQUEST for occurrence X to bob's inbox without draining;
    `contacts/pin`: alice shows bob ACCEPTED on the master.

Cases restore keys, forget pins and delete events (`assert_is_empty` runs
once at the suite's end); `tests/src/utils/cleanup.rs` exempts field 151
while its account exists, as it does 150. The exemption must also hold in
key-mode `webdav_tests`: `za_variants`' scheduling variant creates pin
records at first use, and upstream's sub-modules call `assert_is_empty`
while the accounts exist.

**Leak tests** (`tests/src/za/leak.rs`). The whole-store scan already covers
every principal property; a pin record holding canary addresses must show
no hit.

**CalDAV client CI** (`tests/caldav_client/`): server suite only.

**Mutation checks** (plan 7 style), each failing the named case: key
equality forced true (2); the cached key (2, uninvalidated); pin write after
the items (16); no vault assertion in the first-pin batch (8); entries keyed
by UID alone (18); a counter for `refusal_id` (17); legacy-only reads, or an
absent record always first use (6); the self-check creating a record (7);
no healing rewrap (9); only one of the two items (11); a partial message
redelivered (12); no participation filter (19); no drain pass (20); no
fingerprint check in `contacts/pin` (4).

## 10. Invariants for implementers

Release 1's ten and release 2's five (11 to 15) hold. This spec adds:

16. The pin record is sealed under MK. It is read and written only in a
    request that holds the owner's session keys; background code never
    reads it.
17. Pins are compared before any item is written, and a refusal writes no
    item. A fan-out's phase 2 pin-record write precedes its item writes;
    its retirement write follows them, and a failed retirement leaves the
    entry without undoing an item.
18. Pins are keyed by the normalized address (section 4.3), never by
    account id, and the key compared and sealed to is read fresh from the
    recipient's vault record.
19. The page's own fingerprint and the self-check derive the public key
    from the private key, never from the stored public key.
20. The pinned wrap is set once and never reverts to legacy (a replayed
    wrap is healed). The vault record's layout does not change.
21. No key bytes, fingerprints or addresses in traces.
22. Under a pinned wrap, only `contacts/pin` and `contacts/forget` with a
    null address create a pin record. Under a legacy wrap the fan-out (or
    `contacts/pin`) creates the account's first record, atomically with
    the rewrap, which is the moment the account becomes pinned. The
    self-check and the listing never create one.
23. Entries are bound to the source document and never hold a message.
    The next ordinary delivery sends upstream's message, then the full
    current state while the relationship remains; `contacts/pin` drains
    first and sends the full current state. An entry is retired only under
    its consumed `refusal_id`.

## 11. Alternatives considered

- **Pins inside the vault record** (issue #32's first idea). Rejected: its
  revision is the authentication generation, so every pin write would end
  every session of the account.
- **Pins keyed by account id.** Rejected: section 4.3.
- **Comparing against the cached key** (`za_public_key`). Rejected: no
  TTL and no invalidation on a direct store write, so a changed key goes
  unseen while the entry lives (section 5.1).
- **A `has_pins` bool in the vault record**, in a version 2 layout.
  Rejected: the record is unauthenticated, so a writer clears the bool with
  a plain edit, and a new layout locks older binaries out; the wrap's
  associated data cannot change without MK.
- **Binding the pinned wrap to the vault revision.** Rejected: every vault
  write would then need MK and a rewrap. **Binding it to the pin record's
  content**: every pin write would rewrite the wrap and bump the
  generation.
- **Storing the refused messages and replaying them.** Rejected: a partial
  organizer message that replaces an older one loses occurrence changes,
  and one appended replays stale state over a newer successful delivery
  (a DECLINED reply arriving after an ACCEPTED one). Current state costs
  the losses of section 2, limit 6, instead.
- **Fingerprints read from the stored key on both sides.** Rejected:
  section 3.2.
- **Failing the PUT on a mismatch.** Rejected: a generic client error and
  every attendee held back; SCHEDULE-STATUS signals per recipient.
- **Strict mode, no trust on first use.** Rejected: every new contact
  blocked until a page visit; pre-pinning gives strictness per contact.
- **Asking the user to resend.** Rejected: an unchanged re-save sends
  nothing.
- **Signing invitations.** Out of scope: another trust root.

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
   and `itip_build_envelope` the fork helper of section 6.4 can call to
   apply `attendee_handle_update`'s instance selection minus its "changed"
   predicate (server-scheduling participant, RSVP not FALSE, ORGANIZER not
   SCHEDULE-AGENT=CLIENT, STATUS not CANCELLED, the DELEGATED-FROM and
   DELEGATED-TO lines carried). Recommendation: the helper lives in
   `groupware`, with at most a one-word visibility edit upstream.
4. **Client display of `3.8`.** How Apple Calendar, Thunderbird and DAVx5
   show a `3.8` SCHEDULE-STATUS. Recommendation: a manual-checklist item
   (`docs/zero-access/manual-checklist.md`), no code.
