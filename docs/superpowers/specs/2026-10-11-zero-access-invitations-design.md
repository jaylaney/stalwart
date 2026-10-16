# Zero-access calendar: invitations between users (release 2) design spec

Date: 2026-10-11. Status: draft for review. Scope: release 2 of the
zero-access fork (jaylaney/stalwart), item 1 of release 1 spec section 1.2;
GitHub issue #17.

The terms key account, master key (MK), event wrapping key (EWK), data key
(DEK), `X-ZA-KEY`, `X-ZA-SEALED`, wrap type, session keys, key cache,
generation and visible by structure are those of the release 1 spec,
`2026-10-06-zero-access-calendar-design.md` (revision 7), cited below as
"release 1". Every release 1 invariant (its section 12) continues to hold;
this spec adds to them and relaxes none. This spec supersedes sections 7 and
8 of `2026-10-08-zero-access-sharing-sketch.md`; the sketch's other sections
(collection keys, sharing) remain a sketch.

## 1. Purpose

Two key accounts on the same server invite each other to events with
unmodified CalDAV clients. The organizer's client saves an event with
attendees as it does today; the attendee's client finds the invitation in
its scheduling inbox and on its calendar at its next sync; the attendee's
answer returns to the organizer's copy. The server delivers every message
sealed to the recipient's public key, so it never stores a message it could
read without a session of the recipient, and it opens a delivery only inside
a request of the recipient that holds the recipient's keys.

### 1.1 In scope (release 2)

- The `pk` wrap to an account's X25519 public key, the private key in
  session keys, and one sealed random token per attendee per event that
  authenticates replies (section 3).
- Fan-out: the organizer's request delivers REQUEST and CANCEL messages, and
  the attendee's request delivers REPLY messages, as sealed items in the
  recipient's scheduling inbox (section 5).
- The drain: the recipient's own request opens its pending items, applies
  them to its calendar through upstream's merge, and reseals them under its
  master key (section 6).
- The gates release 1 closed for scheduling, reopened for key accounts;
  free-busy between users and iMIP stay closed (section 8).
- Tests, including leak coverage of the inbox and the task queue
  (section 11).

### 1.2 Out of scope

- iMIP. An attendee address that resolves to an external address, to a
  plain (non-key) local account, or to a key account whose vault record has
  no public key yet is not attempted (section 5.2). Release 3.
- Email of any kind for an invitation. Release 2 is calendar only: the
  invitation appears in the attendee's scheduling inbox and on their
  default calendar (section 6.4). A sealed notification email once mail is
  encrypted at rest is a release 3 follow-up; a generic email like the
  release 1 alarm email was considered and deferred.
- The HTTP RSVP page and its attendee-copy rewrite stay off for key accounts
  (external attendees only; release 3).
- Free-busy between users: deferred to the sharing release (section 7).
- Sharing, group calendars, impersonation, JMAP calendars, full-text
  indexing and the generic alarm email are unchanged from release 1
  (release 1 section 9).

### 1.3 Assumptions

- Release 1's assumptions hold (key accounts from creation, internal
  directory, empty data store).
- A calendar address resolves to an account through upstream's
  `account_id_from_email`, as upstream's ingest resolves an iTIP sender.
- An account has a public key from `setup` onwards (release 1 section 4.1).
  An account in `PendingSetup` has none and cannot receive invitations.
- Scheduling settings are upstream's (`itip_*` in
  `crates/common/src/config/groupware.rs`). No setting, permission or event
  type is added; the registry schema and the event enums are generated.

## 2. Threat model additions

**Claim.** Release 1's guarantee extends to scheduling messages between key
accounts. The contents of an invitation, an update, a cancellation and a
reply are unreadable from the stored data without the recipient's
credentials or a key resident in a server's memory. A delivered item is
sealed to the recipient's public key until a session of the recipient
reseals it under the recipient's master key. Nothing derived from a
message is ever written in plaintext to the scheduling inbox, the task
queue, mail or any other store location (invariant 12). Delivery needs no
key of the recipient: the fan-out runs in the sender's request and uses
only the recipient's public key.

**Limits, stated alongside the claim.**

- As in release 1, the running server holds plaintext: the sender's request
  while it fans out, the recipient's request while it drains.
- **Key substitution.** The server seals to the public key it serves from
  the store. A write-capable operator who substitutes bob's stored public
  key can read every new invitation alice sends bob, and alice's unmodified
  CalDAV client cannot notice. Invitations are confidential against a
  reader of the store, not against a writer who substitutes keys. The
  follow-ups, fingerprints on the Circulo page and pinning at first use in
  the organizer's vault record, are issue #32, not release 2.
- An invitation from a plain local organizer or an external address
  reaches a key account as upstream's plaintext mail in its mailbox, as in
  release 1 (mail is not sealed until the mail release); its calendar
  ingest stays skipped (section 5.2).
- Integrity is as in release 1: a store writer can drop, swap, replay or
  delete sealed items, pending ones included; tampering with an item that
  is present is detected (section 10). The sender in an item's
  `changed_by` is written by the server, not signed.

**Visible metadata added** (beyond release 1 section 2):

- That account A wrote an item into account B's scheduling inbox, and when
  (the item's created and modified times and B's change log).
- The item's UID, visible on the VCALENDAR root and on every component
  under the release 1 policy, and the other properties that policy leaves
  visible in each component (times, recurrence, SEQUENCE, DTSTAMP, STATUS,
  TRANSP) and on the root (METHOD, so whether the item is a REQUEST, a
  REPLY or a CANCEL).
- The item's size: its plaintext size field and the size class of each
  sealed bundle.
- The sender's principal id in `changed_by`.
- Whether an item is still pending (the flag bit, section 9).
- The random token on a pending reply item (section 3.3).
- The schedule tag, a counter, now set on key organizers' events and, as
  upstream sets it at import and on each organizer update, on attendees'
  copies.
- Who else is invited cannot be read from any one item, but because the UID
  and the sender are visible, a reader of the whole store can list the
  inboxes that received items with the same UID from the same sender, which
  is the set of key-account attendees of that event.

**Not visible:** the contents (title, description, location and every
other field release 1 seals), the other attendees' addresses and names
inside an item, participation status, comments, the attendee address list
of the organizer's event, and any attendee's token except on that
attendee's own pending reply.

## 3. Key material

### 3.1 The `pk` wrap

Release 1 reserved a public-key wrap type for `X-ZA-KEY` (release 1 section
7.1). Release 2 defines it as wrap type `0x02`; `0x01` remains the
master-key wrap (`WRAP_MK` in `crates/groupware/src/calendar/seal/tree.rs`).
To wrap a DEK to recipient account `r` with static public key `P`:

| Step | Operation |
|---|---|
| 1 | Generate an ephemeral X25519 keypair (`e`, `E`). |
| 2 | Shared secret `s = X25519(e, P)`. |
| 3 | Wrapping key: 32 bytes of HKDF-SHA256 over `s`, label `za/v1/pk`, with `E` and then `P` bound into the info input. |
| 4 | XChaCha20-Poly1305 of the DEK under the wrapping key, 24-byte random nonce, associated data `aad("pk-dek", r) || "|" || uid` (section 9 has the full shape): the existing `wrap_key`, which yields `nonce || ciphertext`. |
| 5 | Zeroize `e`. |

The `X-ZA-KEY` bytes are `policy version || 0x02 || E (32 bytes) || wrapped
DEK`, base64 as in release 1. Opening computes `s = X25519(private key, E)`
and the same derivation, and unwraps with `unwrap_key`; it happens only in
the drain (invariant 11).

The recipient's public key is `VaultRecord.public_key`
(`crates/vault/src/record.rs`), as carried in the recipient's
`AccountCache` entry (release 1 section 3.2).

There is no HPKE library: one more dependency, and a format the fork does
not control. The composition uses only `x25519-dalek`, `hkdf`, `sha2` and
`chacha20poly1305`, already in `crates/vault` (`generate_keypair`,
`wrap_key`, `seal`, `aad` in `crates/vault/src/keys.rs`). The associated
data names the recipient account, the purpose and the UID in release 1's
event-key shape (section 9), so a `pk` wrap cannot be replayed as a
master-key wrap, for another account or for another UID.

### 3.2 The private key in session keys

The private key is stored in the vault record wrapped under MK
(`private_key_wrap`, associated-data purpose `private-key`). `SessionKeys`
(`crates/vault/src/session.rs`, today `account_id`, `generation`, `mk`,
`ewk`) gains a lazily unwrapped private key. The first drain of a session
reads the vault record once, unwraps the private key with MK, and keeps it
in a `Zeroizing` buffer with a silent `Debug`. A session is one key-cache
entry: `KeyCache` (`crates/vault/src/cache.rs`) holds an `Arc<SessionKeys>`
per credential fingerprint, so the private key is dropped with the cache
entry and zeroed when the last request holding it finishes. `SessionKeys`
also remembers the inbox change id it last drained (section 6.2); both
additions live in memory only.

Recovery does not change MK: `recover` opens the existing MK with the
recovery key and rewraps it under the new password (release 1 section 4.1),
so `private_key_wrap` stays valid. The public key never changes. Pending
items therefore survive a recovery, a password change and every
app-password change.

### 3.3 The attendee token

- 16 random bytes per attendee per event, generated by the organizer's
  session.
- **Carrier.** The tokens live in the organizer's sealed `X-ZA-EXTRA` root
  bundle (release 1 section 7.1), which already carries the event's display
  name and dead properties and is never shown to a client, as a list of
  (attendee address, token) pairs. The token is not an ATTENDEE parameter:
  a client that drops unknown parameters would silently lose it on its
  next PUT. The bundle's plaintext (`Extra` in
  `crates/groupware/src/calendar/seal/event.rs`) is decoded with a checked
  rkyv read, so a bundle that carries pairs is a new bundle format
  (section 9); release 1 bundles read as before.
- **Every rewrite carries the pairs.** Every rewrite of a key account's
  event by its own session carries the stored bundle's pairs into the new
  bundle, whether or not messages are built: a PUT with or without
  messages, a PROPPATCH, the drain's handling of a CANCEL (the CANCEL
  item's pair is the one the copy already holds), the organizer's merge of
  a REPLY, and the delegation fan-out in the drain. The one exception: an
  accepted REQUEST, by import or by merge, replaces the attendee's pair
  with the incoming one (section 6.4), so a removed and re-added
  attendee's copy follows the organizer to the new token. This matters
  because `CalendarEvent` has no field the pairs
  can ride in (invariant 1), and `seal_event`
  (`crates/groupware/src/calendar/seal/event.rs`) rebuilds `X-ZA-EXTRA`
  from the event's display name and dead properties alone, which the
  unsealed view (`za_event_view`) restored from the old bundle. Rebuilt
  from those fields only, an organizer PUT that builds no message (adding
  an alarm) would drop every pair and turn every later reply into a
  forgery, and an attendee's alarm edit would drop the attendee's own
  pair. COPY and MOVE keep the stored bytes
  (`crates/dav/src/calendar/copy_move.rs` has no seal step) and need
  nothing.
- **Maintenance.** A rewrite that builds messages (a PUT or DELETE that
  yields messages, or a drain that merges a reply) additionally reads the
  stored pairs and adds a pair for every scheduled attendee that has none
  (new attendees, and delegates that upstream's merge adds during the
  drain, section 6.4). Messages are built from the union of the stored
  pairs and the current attendees, so the CANCEL to a removed attendee
  carries that attendee's old pair; the organizer's persisted bundle keeps
  only the pairs of current attendees. Handing a removed attendee's token
  back is harmless: they have nothing left to reply to, and a reply from
  them is rejected as not a participant. A token is therefore stable for
  the event's life, and a removed and re-added attendee gets a new one.
- **Delegates** receive their token from the organizer, never from the
  delegating attendee: an attendee-originated delegate invitation is not
  attempted (section 5.5), and the organizer's drain invites the delegate
  (section 6.4). A delegate's REPLY goes to the organizer only: a
  delegator's bundle holds no pair to check it against, so the delegator's
  copy learns the delegate's status from the organizer's next update
  REQUEST (section 5.5).
- **Attendee side.** An item sent to attendee X carries X's pair, and only
  X's, in the item's own `X-ZA-EXTRA` bundle, not in the tree. The drain's
  import moves the pair into the `X-ZA-EXTRA` bundle of X's copy of the
  event, and every later rewrite of that copy carries it until a later
  accepted REQUEST replaces it, so X's session finds it there when it
  replies. The rewritten inbox item carries no
  token.
- **Reply item.** A reply carries the token in the clear, as the visible
  root property `X-ZA-RSVP-TOKEN` (base64url), so the organizer's drain can
  check it without a second unseal. The fan-out takes it from the bundle of
  the attendee's copy.
- **What it proves.** The token authenticates a reply. It does not hide the
  sender of an inbox item, whose principal id is visible by structure in
  `changed_by` (section 2). The organizer's token list is sealed.
- **Verification** happens only inside the organizer's drain, where the
  event is open: match the attendee by address as upstream's merge does,
  then require the token of that address's pair to equal the item's token
  (constant time). A mismatch drops the item with a security trace and no
  change to the event. A reply from an account that was never invited
  holds no pair, like a removed attendee's, and is rejected as not a
  participant (history, section 6.5).

## 4. Message flow overview

alice and bob are key accounts on the same server; carol is an external
address.

### 4.1 Invite

alice's client PUTs an event with bob and carol as ATTENDEEs. In that
request the server gives both tokens, runs upstream's `itip_create`, and
splits the recipients: bob is internal, carol is not attempted. bob's
REQUEST is sealed to bob's public key and written as a pending item into
bob's scheduling inbox; then alice's event is sealed under her MK with both
tokens and both schedule statuses inside. bob's next DAV request drains the
item: no event of bob's has that UID, and a local sender always adds
(upstream's rule), so it is imported into bob's default calendar, sealed
under bob's MK; the item is resealed under bob's MK and stays in his inbox.

### 4.2 Update

alice moves the event. Upstream's `itip_update` produces a REQUEST for the
remaining attendees and a CANCEL for removed ones; the fan-out delivers them
as in 4.1. bob's drain finds his copy by UID and runs upstream's merge on
the unsealed copy, which keeps what bob added himself (alarms, for
instance), and commits it sealed.

### 4.3 Cancel

alice deletes the event. The DELETE unseals the stored event, runs
upstream's `itip_cancel`, and delivers a CANCEL to every internal attendee.
bob's drain applies upstream's cancel handling, which marks his copy (or the
cancelled instances) `STATUS:CANCELLED`, and commits it sealed.

### 4.4 Reply

bob accepts in his client, which PUTs his copy with `PARTSTAT=ACCEPTED`.
Upstream's attendee path produces a REPLY to alice. The fan-out seals it to
alice's public key, adds bob's token from his copy's bundle as the visible
`X-ZA-RSVP-TOKEN`, and writes it into alice's inbox. alice's next DAV
request drains it: it finds her event by UID, matches bob by address,
requires the token, and runs upstream's merge, which updates bob's
participation status and schedule status in alice's copy. A reply to an
earlier revision (lower SEQUENCE) is kept as history, not applied
(section 6.4).

| Step | Runs in | Writes | Where | Sealed under |
|---|---|---|---|---|
| invite, update, cancel | alice's request | one item per internal attendee | each attendee's scheduling inbox | attendee's public key (`0x02`) |
| invite, update, cancel | alice's request | her event, with tokens and schedule statuses | alice's calendar | alice's MK (`0x01`) |
| drain of a REQUEST or CANCEL | bob's request | his copy of the event; the item, rewritten | bob's calendar; bob's inbox | bob's MK |
| reply (PUT or DELETE of his copy) | bob's request | his copy; one reply item | bob's calendar; alice's inbox | bob's MK; alice's public key |
| drain of a REPLY | alice's request | her event; the item, rewritten | alice's calendar; alice's inbox | alice's MK |

## 5. Writes: fan-out in the organizer's request

### 5.1 The sending state

`ItipSendStatus::resolve` (`crates/groupware/src/calendar/itip.rs`) runs in
both PUT branches of `crates/dav/src/calendar/update.rs`, before the event
is sealed. Today a key account resolves to `KeyAccount` and nothing is
sent. In release 2 a key account resolves to a new sending state,
`SendSealed`, under the same preconditions as upstream's `Send`: scheduling
enabled (`itip_enabled`), a calendar address, the
`CalendarSchedulingSend` permission, and an event that does not lie in the
past. Every other state keeps its upstream meaning.

Under `SendSealed`, upstream's `itip_create`, `itip_update` and
`itip_cancel` run on the plaintext tree exactly as for a plain account and
produce their `ItipMessage` values (sender, recipients, tree). Before they
run, the fan-out takes the union of the stored token pairs and pairs for
new attendees (section 3.3). A PUT that yields no message still carries
the stored pairs into the new bundle (section 3.3, "Every rewrite carries
the pairs"). The
schedule tag is set exactly as upstream sets it, and
`itip_set_unreachable_status` runs as upstream runs it; release 1's "no
schedule tag is ever set" ends for key organizers.
`itip_outbound_max_recipients` applies as upstream applies it: a message
set over the limit fails the PUT with the `MaxAttendeesPerInstance`
precondition before any delivery.

What changes is where messages go. Upstream's `ItipMessages::queue`
schedules a `Task::CalendarItipMessage` in the event's batch, which
`crates/services/src/task_manager/imip.rs` delivers as mail, local
recipients included. Under `SendSealed` the messages go to the fan-out: the
task queue is never written for a key account's message and no mail is
built.

**Creating an event.** The create branch of a key account's PUT changes in
two ways, and nothing else in the PUT path's write sequence changes. It
takes the UID creation lease (section 6.3) before its uniqueness check and
holds it through the insert, whose commit is fenced on the lease. And the
uniqueness check, once the lease is held, reads current archives instead
of the resource snapshot: the PUT loads an immutable `Arc<DavResources>`
at the top of the handler (`fetch_dav_resources` in
`crates/dav/src/calendar/update.rs`), and `assert_is_unique_uid`
(`crates/dav/src/calendar/mod.rs`) intersects fresh UID-index hits with
that snapshot's `children(calendar_id)`, so an event another creator
committed after the snapshot is a hit the snapshot does not list. Under the
lease the key-account PUT reads the fresh hits (`document_ids_matching` on
`CalendarEventField::Uid`), reads each hit's current stored archive, and
compares its parent calendar ids (`CalendarEvent.names`, visible by
structure in release 1) with the target calendar; a hit in the target
calendar answers `NoUidConflict` as upstream does. The snapshot is still
used for everything else (resolving the parent, the create-versus-update
decision).

### 5.2 Recipient split

Each recipient address of each message is resolved with
`account_id_from_email`. It is **internal** when it resolves to a local key
account whose `AccountCache` entry holds a public key and which holds
`Permission::CalendarSchedulingReceive`, the permission upstream's ingest
requires of a recipient (`crates/email/src/message/ingest.rs`). Everything
else is **not attempted**: an external address, a plain local account, a
key account without a public key (setup pending), and a key account
without that permission. The sender is checked too: a REQUEST whose sender
is not the event's ORGANIZER (an attendee-originated delegate invitation,
section 5.5) is not attempted for any recipient, and a REPLY is delivered
only to the event's ORGANIZER: any other REPLY recipient (a delegator,
section 5.5) is not attempted.

A not-attempted recipient is sent nothing, and the organizer's ATTENDEE
property for it receives a `SCHEDULE-STATUS` parameter stating that
delivery was not attempted, sealed with the property. Upstream's own code
sets only `3.7` (`ITIP_STATUS_INVALID_USER` in
`crates/groupware/src/scheduling/itip.rs`), and only for an address that
cannot be parsed; the RFC 6638 code used for not-attempted delivery is a
value to fix in planning (section 13).

The address-to-account mapping exists only during the fan-out; the
organizer's event never holds an attendee's account id.

**Plain organizers.** A plain local organizer who invites a key account
takes upstream's path unchanged: the iTIP message is mailed to the key
account's mailbox, and ingest skips calendar processing for key accounts
(`!account.is_key_account()` in `crates/email/src/message/ingest.rs`, the
release 1 gate). This is the same experience as an external organizer
inviting a key account.

### 5.3 The per-recipient item

For each internal recipient R of a message:

1. Copy the message tree. For a REQUEST or CANCEL to an attendee, take R's
   token pair, from the union of section 3.3 (so a removed attendee's
   CANCEL carries the old pair), for the item's `X-ZA-EXTRA` bundle. For a
   reply to the organizer, set the visible root property `X-ZA-RSVP-TOKEN`
   from the bundle of the replying attendee's copy.
2. Generate a fresh DEK. Seal the tree under the release 1 policy (release
   1 section 6) with an `X-ZA-SEALED` property per affected component,
   associated data shaped like events' (`tree_aad`) with the scope `itip`:
   R's account id, `itip`, the component index and the policy version.
   Seal R's token pair with the same DEK into the root's `X-ZA-EXTRA`,
   bound to R's account and the UID (section 9).
3. Append `X-ZA-KEY` to the root with the `pk` wrap to R (section 3.1),
   bound to R's account and the UID.
4. Write a `CalendarEventNotification` (`crates/groupware/src/calendar/mod.rs`)
   into R's scheduling inbox collection with `event` the sealed tree,
   `event_id = None`, `changed_by = ChangedBy::PrincipalId(sender)`,
   `size` the plaintext size as upstream computes it for an inbound
   message, and `flags = EVENT_NOTIFICATION_ZA_PENDING`, the next free bit
   of the existing `flags` field (section 9).
5. Before the write, check R's quota for that size and the message against
   `itip_inbound_max_ical_size`, the two checks upstream makes at ingest. A
   failure becomes a delivery-failed `SCHEDULE-STATUS` on that recipient's
   property in the sender's copy (the ATTENDEE, or the ORGANIZER for a
   reply), never a failed PUT, and the other recipients are unaffected.

Each item is its own batch in R's account.

### 5.4 Order and partial failure

The fan-out runs after every check of the request (headers, schedule tag,
UID, quota) and immediately before the sender's commit. The recipients'
inbox writes come first, each its own batch; then the sender's event is
sealed and committed under the sender's MK, with tokens and schedule
statuses inside. If the sender's commit fails after inbox writes,
recipients hold an item for an event version that was not stored, and the
sender's client resends; the drain's merge treats the duplicate by UID and
SEQUENCE as upstream's merge does. A reply can also beat the sender's
commit: if the attendee drains, imports and replies before the organizer's
event commits, the organizer's drain finds no event for the UID and files
the reply as history (section 6.4 step 4), never retried. The window lies
inside one request of the sender, and the attendee's next participation
change produces a fresh reply. Both consequences are accepted. A retried
create delivers fresh tokens with an equal SEQUENCE, which upstream's
merge accepts (`>=` in the `Request` arm of `inbound.rs`), and the copy
adopts them under the section 6.4 rule, so the retry case needs nothing
more.

### 5.5 The paths that fan out

- **Event PUT** by the organizer, create and update (section 5.1).
- **DELETE by the organizer.** `crates/dav/src/calendar/delete.rs` computes
  `send_itip` with `!is_key_account()` today; release 2 drops that term,
  keeping the other preconditions. `DestroyArchive::delete_all`
  (`crates/groupware/src/calendar/storage.rs`) builds a CANCEL with
  `itip_cancel` only when the event has a schedule tag and lies in the
  future, and builds it from the stored tree. For a key account the stored
  tree's ATTENDEE properties are sealed, so DELETE gains an unseal step
  before `itip_cancel`. Session keys are present on every DAV request of a
  key account (release 1 section 9). The CANCEL goes to the fan-out
  instead of `ItipMessages::queue`.
- **DELETE of a calendar collection.** Upstream passes the same `send_itip`
  flag to `delete_with_events`, which cancels every scheduled event in the
  collection. Each of those events gets the same unseal step as the event
  DELETE, and its CANCEL goes to the fan-out.
- **The attendee's own PUT** of their copy that changes their participation:
  upstream's attendee path in `itip_update` yields a REPLY, which the
  fan-out writes into the organizer's inbox sealed to the organizer's
  public key with the visible `X-ZA-RSVP-TOKEN`. The organizer must be
  internal by the rules of 5.2; otherwise the reply is not attempted. An
  attendee's DELETE of their copy yields upstream's declined REPLY
  (`itip_cancel` with deletion set) the same way.
- **Delegation.** When an attendee delegates, upstream's attendee builder
  (`crates/groupware/src/scheduling/attendee.rs`, "Invite new delegates")
  also produces a REQUEST to the delegates whose sender is the attendee
  (`from_organizer = false`). At fan-out, a REQUEST whose sender is not the
  event's ORGANIZER is not attempted (section 5.2); only the REPLY to the
  organizer is delivered. The organizer's drain then invites the delegates
  (section 6.4). Upstream also addresses a delegate's REPLY to the
  delegators as well as the organizer (`is_delegated_to` into `email_rcpt`
  in `attendee.rs`), and a delegator's copy accepts it (the guarded `Reply`
  arm in `itip_process_message`, `inbound.rs`). Here a REPLY is
  authenticated by the replier's token in the recipient's event bundle,
  and a delegator's bundle holds only its own pair, so that REPLY is
  suppressed: it is delivered to the organizer only. The limit: the
  delegator's copy learns the delegate's status when the organizer next
  sends an update REQUEST, which carries every attendee's participation
  status; the organizer's copy is correct throughout.

## 6. The drain

### 6.1 Where it runs

The drain is the only place a `pk` wrap is ever opened. It runs inside a
DAV request that holds the account's session keys, before the request's
own work, so invariant 6 holds: no background code needs a key.

### 6.2 Triggers

1. The first DAV request that obtains session keys for the account in a
   session: a `SessionKeys` created by `authenticate_uncached`
   (`crates/http/src/auth/authenticate.rs`), which inserts the new
   key-cache entry, has no remembered change id.
2. Mid-session arrivals. The scheduling inbox collection has a change id in
   the in-memory DAV resource cache (`DavResources`), and `SessionKeys`
   remembers the change id it last drained. A request that finds a newer
   change id drains again. The cost is one change-log since-query for the
   inbox collection per request (`fetch_dav_resources`,
   `crates/groupware/src/cache/mod.rs`), the same query upstream already
   runs for the request's own collection; it returns nothing when nothing
   has arrived.

**The cursor.** The remembered id is the inbox change id observed
immediately before the pass enumerates pending items (the snapshot), never
a value produced by the drain's own writes. An item that arrives while a
pass is rewriting earlier items therefore carries a change id above the
snapshot and triggers the next request's pass; the drain's own rewrites do
too, and a pass that finds nothing pending just records its snapshot. A
pass that leaves any item pending (a transient error, a lost assertion, an
abort) or that could not take a creation lease for an item (section 6.3)
does not advance the remembered id, because
`fetch_dav_resources` returns unchanged state when nothing newer exists and
the item would otherwise wait for an unrelated change. A per-session retry
backoff after such a pass (`DRAIN_RETRY_SECS`, a constant of 30 seconds
with a test-mode override; plan 10 settled on no environment knob) keeps
a stuck item from draining on every request.

An invitation therefore appears at the attendee client's next poll, not at
the next login.

### 6.3 Concurrency

Two sessions of one account (two key-cache entries, for instance the
password and an app password) or two cluster nodes can drain at the same
time. Unless they are serialized, both can find no event with a UID and
both import it: the UID index is not unique (`IndexValue::Index` on
`CalendarEventField::Uid` in `crates/groupware/src/calendar/index.rs`), and
upstream's import path in `itip_ingest`
(`crates/groupware/src/calendar/itip.rs`, after
`get_or_create_default_calendar`) asserts no absence. Document-id order
alone does not serialize them: `assign_document_ids`
(`crates/store/src/dispatch/store.rs`) reserves an id in its own batch
before the item's batch commits, so one fan-out can reserve id 10 and
pause while another commits id 11 for the same UID; a pass that sees only
11 imports, 10 commits, and a second pass starting at 10 imports again,
each asserting a different item. A key account's PUT of the same UID is a
third creator: `assert_is_unique_uid` (`crates/dav/src/calendar/mod.rs`)
is a read, separate from the insert in the create branch of
`crates/dav/src/calendar/update.rs`, so a PUT and a drain pass that both
found no event both insert. (Upstream has the same read-then-insert gap
between two PUTs and between ingest and a PUT; release 2 adds the drain as
one more writer and closes the gap for key accounts.)

**A per-(account, UID) creation lease with fenced commits.**

1. **The lease.** Before a key account's session creates a calendar event
   for UID U, by the drain's import (after a first lookup found nothing,
   section 6.4) or by a PUT create (section 5.1), it
   takes a short lease on the key (account id, U) in the data store,
   written directly through `Server::store()` as an existing
   in-memory-class key (`ValueClass::InMemory(InMemoryClass::Key(..))`, the
   class `try_lock` in `crates/store/src/dispatch/lookup.rs` uses when the
   in-memory store is the data store), under a new prefix next to
   `KV_LOCK_DAV` in `crates/common/src/lib.rs` (`KV_LOCK_ZA_UID`, a name to
   confirm in section 13). It goes to the data store on purpose, not
   through the in-memory store dispatch: a Redis in-memory store would put
   the lease in a different system from the event write, and the fence in
   step 3 needs both in one transaction. The lease is taken the way
   `try_lock` takes one: read, refuse if an unexpired value is present,
   write with an assertion on the value read. A session that cannot take
   it waits briefly and retries a bounded number of times (values for
   planning); then the drain leaves the item pending, and the PUT runs its
   archive-based uniqueness check (section 5.1) once it holds the lease.
   The drain's own absence check is the UID index read
   (`document_ids_matching` on `CalendarEventField::Uid`), which is fresh,
   so the drain needs no such change.
2. **Its value** is `expiry (u64, big endian) || fence token (random
   bytes)`. The leading expiry is what the fork's sweep (step 4) reads;
   `purge_in_memory_store` (`lookup.rs`) skips the `KV_LOCK_ZA_UID` range,
   because its unconditional clear would delete a replacement lease taken
   between its scan and its clear (PR #35 review). The token makes the
   value unique to one holder.
3. **The fenced commit.** The batch that inserts the event (and, for the
   drain, rewrites the item) also asserts that the lease key still holds
   this holder's exact value (`assert_value`). An expired holder whose
   lease was taken by another cannot commit: its assertion fails, the drain
   aborts the pass, and the PUT takes the lease again and re-runs its
   uniqueness check against current archives, not the snapshot (answering
   `NoUidConflict` as upstream does if the UID now exists there). This is
   what makes the lease safe; its duration only decides how long a crashed
   holder blocks others.
4. **Release** is a batch that asserts the holder's value and clears the
   key, so a holder never removes a successor's lease. A crashed holder's
   lease (or one whose release failed) expires and is purged by the
   maintenance task's lease sweep, which runs on the data store whatever
   the lookup backend. The generic purge never touches lease keys, on any
   configuration: `purge_in_memory_store()` skips the `KV_LOCK_ZA_UID`
   range (step 2), so the sweep is the only cleanup. The fork adds one call
   to the maintenance task's purge step
   (`crates/services/src/task_manager/maintenance.rs`),
   `sweep_expired_uid_leases`, in the `PurgeData` arm after
   `purge_in_memory_store`: iterate the
   `KV_LOCK_ZA_UID` key range in the data store and clear every key whose
   leading eight-byte expiry is at or below now, each clear asserting the
   value the sweep read, so a lease re-taken in between survives. Without
   the sweep an abandoned lease is storage growth, not a fencing failure: a
   later creator of the same UID replaces the expired value.
5. **Scope.** The lease covers creation, from the absence check through
   the insert, and the delegating merge's fan-out. Updates and merges of
   an existing event are already protected by the assertion on the
   event's stored archive, which the drain's batch carries. Non-key accounts take no lease: the PUT path's
   lease is behind the key-account branch (invariant 9). A merge whose
   fan-out would deliver new items takes the UID lease of the organizer's
   account first, so two sessions of the organizer cannot both fan out.
   That fan-out is fenced on the same lease: each delegate item's batch
   asserts the holder's value ahead of the insert (the lease key is
   global, so the delegate's account can carry the assertion). A holder
   whose lease expired and was taken over while it waited inside the
   fan-out writes no further item: the first write that loses the fence
   stops the fan-out, and the merge is abandoned as on a lost commit
   fence, the item left pending for the next pass after the backoff. A
   stale token therefore never reaches a delegate after the successor's.
6. **Why it suffices.** Two creators of one UID in one account serialize on
   the lease, and the second finds the first's event when it re-checks:
   the drain through the fresh UID index, the PUT through current archives
   rather than its resource snapshot (section 5.1). This covers drain
   versus drain (the interleaving above), drain versus PUT, and PUT versus
   PUT for key accounts. Expiry cannot reopen the race, because the commit
   is fenced. The PUT's re-check keeps upstream's rule, which refuses a UID
   only within the target calendar, so a PUT into another calendar still
   creates its copy, as upstream allows.

**The pass.** The drain processes pending items in creation (document id)
order. It re-reads each item before processing it, and skips one whose
pending bit is already clear. Each item's batch holds the event write
(import or merge) and the item rewrite together and asserts the item's
archive as read (`assert_value`), and the lease when the item imports or a
merge fans out; the event write of a merge carries the index builder's
assertion on the event's stored archive. A lost assertion aborts the pass, and the next
trigger re-enumerates. A busy creation lease stops the pass at that item;
later items wait for the next pass, so items for one UID keep their order.
An async lock on the `SessionKeys` entry keeps to
one pass per session on a node without a store round trip. The drain never
fails the request that triggered it: errors are traced and the request
proceeds.

Two options were rejected: a per-account drain lock (an expiring lease
without fencing, whose `remove_lock` deletes without checking ownership;
it also left the PUT uncovered), and a permanent per-UID marker (storage
growing with events, where the lease is released at commit).

### 6.4 Steps

The drain reads the items whose pending bit is set, in creation (document
id) order. On the first item it unwraps the private key from MK, once per
session (section 3.2). For each item:

1. Open the `pk` wrap, unseal the tree (associated data naming the
   account, the scope `itip` and the component index, section 5.3), and
   open the item's `X-ZA-EXTRA` bundle when present.
2. Check the sender. The sending address (the organizer for a REQUEST or
   CANCEL, the replying attendee for a REPLY) must resolve to the principal
   in `changed_by`, the check upstream's `itip_ingest` makes against the
   sender of a mail.
3. Act by METHOD:
   - **REQUEST.** Look the UID up first (upstream's lookup on
     `CalendarEventField::Uid`). If an event exists, merge with no lease:
     unseal it, run `itip_process_message` against the unsealed copy, then
     `itip_merge_changes`, and commit the merged event through the
     existing seal path (`seal_event`: new DEK, master-key wrap) under the
     assertion on the event's stored archive, with upstream's size, quota
     and schedule-tag rules from `commit_itip_merge`. If none exists, take
     the creation lease (section 6.3), look the UID up again, and import
     only if it is still absent (merge as above if it appeared); release
     the lease after the commit. Updates therefore pay no lease round
     trip. Upstream's `itip_ingest` always adds an
     invitation whose sender is a local principal, and every release 2
     sender is one, so the attendee's copy always lands on the default
     calendar; the `itip_auto_add` setting and its address-book exception
     apply only to iMIP (release 3). The drain checks quota as upstream
     does, imports the tree with `itip_import_message` into the default
     calendar, and seals it under MK.
     When the drain accepts a REQUEST, by import or by merge, the copy
     takes the pair carried in the item into its `X-ZA-EXTRA` bundle,
     replacing any stored pair for that attendee (section 3.3). A
     re-invitation after a CANCEL, which leaves the copy in place with
     `STATUS:CANCELLED` (upstream's `Cancel` arm in `inbound.rs` rewrites
     entries and removes nothing), therefore adopts the new token.
   - **REPLY.** Find the organizer's event by UID, unseal it, match the
     attendee by address, and require the token (section 3.3). Upstream's
     merge does not reject stale replies: `handle_reply`
     (`crates/groupware/src/scheduling/inbound.rs`) updates the
     participation status with no SEQUENCE or DTSTAMP comparison. The drain
     adds a check for key organizers: a REPLY whose SEQUENCE for an
     instance (RFC 5546 section 2.1.4) is lower than the organizer's
     current SEQUENCE for that instance is not applied; it is an iTIP-level
     rejection (step 4), resealed and kept as history with a trace. A reply
     with an equal SEQUENCE applies. Non-key accounts keep upstream's
     behaviour (invariant 9 is unchanged). Then upstream's merge.
   - **Delegates.** When the merged REPLY adds attendees (delegates, which
     `handle_reply` adds through `MergeResult::Actions`), the drain brings
     the organizer's token pairs up to date (section 3.3) and fans out a
     REQUEST from the organizer to each newly added internal attendee,
     built with upstream's organizer request builder
     (`organizer_request_full`) on the merged tree and restricted to those
     attendees, under the section 5 rules. Attendance is compared per
     instance (the master and each override, by RECURRENCE-ID): an
     attendee is new when the merged event lists it in an instance it did
     not attend before, where an override the merge created inherits the
     master's attendance. A delegate who already attends another instance
     is therefore still invited to the one it joins; the new addresses are
     deduplicated for delivery. The drain runs in the organizer's own
     session and holds its keys, so this is the same fan-out a PUT uses,
     fenced on the organizer's UID lease (section 6.3, step 5). A
     delegate's REPLY reaches only the organizer (section 5.2); the
     delegator's copy learns the delegate's status from
     the organizer's next update REQUEST, which carries every attendee's
     participation status, while the organizer's copy is correct
     throughout.
   - **CANCEL.** Upstream's cancel handling on the unsealed copy, committed
     sealed.
   - Any other METHOD cannot come from the fan-out (upstream's builders
     produce only REQUEST, REPLY and CANCEL) and is a permanent error. This
     includes REFRESH, the only method for which upstream's processing
     yields an outgoing message (`MergeResult::Message`).
4. An iTIP-level rejection from upstream's processing (organizer
   mismatch, stale or out-of-sequence message, event not found, sender not
   a participant, quota) leaves the calendar unchanged and is traced as
   upstream traces `ItipMessageError`, without content. The item is then
   rewritten as in step 5, bit cleared, and kept as inbox history.
5. Rewrite the inbox item itself, resealed under MK: a new DEK, wrap type
   `0x01`, `X-ZA-SEALED` per component with the same associated data as
   the pending item (the owner's account id, the scope `itip`, the
   component index; section 9), no `X-ZA-EXTRA`, pending bit cleared,
   and `event_id` and the change flag set as upstream's ingest sets them
   for the inbox item it writes. The visible reply token is not carried
   over.

The drain does not call `itip_ingest` or `commit_itip_merge`, which write
plaintext and insert a new inbox item per message. It reuses the pure
functions they call (`itip_snapshot`, `itip_process_message`,
`itip_merge_changes`, `itip_import_message`) unchanged and replaces only the
commit, which builds event data as the PUT path does for a key account
(`CalendarEventData::new_for`) and seals before writing. Each item is one
batch, holding the event write and the item rewrite. Writes made by the
drain carry `ChangedBy::PrincipalId(sender)` from the item, as upstream's
ingest does.

### 6.5 Item errors

- **Transient** (a store error; a lost conditional write, including a
  fenced commit whose lease was lost, which also aborts the pass; a
  creation lease not taken, section 6.3): the item stays pending and is
  retried by a later pass (section 6.2).
- **Permanent** (unknown wrap type, unknown policy version, a wrap this
  account's private key does not open, a stray carrier, an unparsable tree,
  an unknown METHOD): the item is deleted with a trace naming account and
  document id, never content, because nothing could ever read it. The
  private key never changes, so a wrap it does not open today it will never
  open.
- **Token mismatch:** the item is deleted, logged as
  `SecurityEvent::Unauthorized` with details prefixed `zero-access:`, and
  the event is unchanged. A REPLY whose sender holds no pair in the
  event's bundle is rejected as not a participant and kept as history; a
  REPLY from a participant whose token does not match is deleted with a
  security trace, as is any REPLY that carries no token at all (that check
  runs before the bundle is read).
- **iTIP-level rejection** (step 4): resealed under MK, bit cleared, kept
  as history, with a trace.

### 6.6 The inbox as clients see it

`CalendarEventNotification.event` becomes a sealing target, with one
associated-data scope, `itip`, for both pending and drained items,
alongside events' (the UID) and collection timezones' (`calendar-tz`).
The client's view of `/dav/itip/<user>/inbox/` works through the existing
unseal view: `za_archive_view` (`crates/dav/src/common/za.rs`) gains a
`CalendarEventNotification` arm, and the inbox GET
(`handle_scheduling_get_request` in `crates/dav/src/calendar/scheduling.rs`),
which today returns the stored tree, unseals it, so the inbox shows the
history as upstream's does.

Pending items are not shown to clients: they are skipped in listings and
reports and answer 404 to GET until drained. The drain runs earlier in the
same request, before the response is built, so an item stays hidden only
after a failed drain.

Upstream's inbox expiry (`itip_inbox_auto_expunge`, set by the operator as
`expungeSchedulingInboxAfter` in the `DataRetention` settings) keeps
working: it deletes by creation time and needs no key. It also removes
pending items older than the hold period, so an attendee who connects no
client within that period loses the invitation. The operator sets the
period. The index task stays gated for key accounts.

## 7. Free-busy

Release 2 does not change free-busy; between users it is deferred to the
sharing release. Busy times need no key: free-busy reads DTSTART, DTEND or
DURATION, RRULE, RDATE, EXDATE, STATUS and TRANSP with their timezones, all
visible by policy, and never PARTSTAT. The one exception is the FREEBUSY
values of a VFREEBUSY component stored in a calendar, which are sealed.

Today another user's request gets the withheld answer (`ZaFreeBusy::Withheld`
in `build_freebusy_object`, `crates/dav/src/calendar/freebusy.rs`, plan 2
ruling R6) or 3.7 (the outbox POST for a key target,
`crates/dav/src/calendar/scheduling.rs`). Upstream answers free-busy only to
an asker who holds an explicit grant on the target's calendar
(`shared_items` with `ReadItems` and `SchedulingReadFreeBusy`), and key
accounts cannot make grants, so "the same as upstream" would be an empty
answer. The gate reopens in the sharing release, where grants exist, as a
one-branch change to the `Withheld` arm. The owner's own free-busy keeps
unsealing (`ZaFreeBusy::Unsealed`).

Two release 2 options were rejected. A fork-wide policy answering any
authenticated local asker would be more permissive than upstream. A
free-busy-only grant for key accounts has the right shape, but no native
client can set it up and it needs the Circulo page.

## 8. Feature gating for key accounts

| Feature | Release 1 | Release 2 |
|---|---|---|
| iTIP sending in PUT and DELETE (organizer and attendee) | `KeyAccount` state; attendees stored sealed, nothing sent, no schedule tag | `SendSealed`: sealed fan-out to internal key accounts, others not attempted, schedule tag as upstream (section 5) |
| `calendar-auto-schedule` in the authenticated DAV `OPTIONS` header | omitted for key accounts | advertised again |
| Schedule inbox and outbox URLs on the principal | answered as not found (`crates/dav/src/principal/propfind.rs`) | returned again |
| Scheduling inbox collection | never written | written by the fan-out (pending, `pk` wrap) and by the drain (MK); readable by its owner through the unseal view; pending items skipped until drained, which happens in the same request before the response is built |
| Outbox free-busy for a key target | 3.7 | unchanged (section 7) |
| `free-busy-query` REPORT by another user | refused, or withheld as empty | unchanged (section 7) |
| Inbound iMIP ingest from mail | skipped | skipped (unchanged) |
| HTTP RSVP page and attendee-copy rewrite | off | off (unchanged; external attendees, release 3) |
| Sharing and ACL grants | refused | refused (unchanged) |
| Group calendars, impersonation, master user | refused | refused (unchanged) |
| JMAP calendars | not advertised | not advertised (unchanged) |
| Full-text indexing and the index task | gated | gated (unchanged) |
| Alarm email | generic | generic (unchanged, including events imported by the drain) |
| Plain local organizer inviting a key account | upstream's mail to the key account's mailbox; calendar ingest skipped | unchanged, the same as an external organizer (section 5.2) |

## 9. Stored format additions

**Invariant: no stored struct changes layout.** Every addition rides in an
existing field: the `ICalendar` tree and the `flags` field of
`CalendarEventNotification`, properties inside trees, and the sealed
`X-ZA-EXTRA` bundle.

**Key envelopes** (`X-ZA-KEY`, base64):

| Wrap type | Bytes | Opened with |
|---|---|---|
| `0x01` (release 1) | policy version, `0x01`, wrapped DEK (72) | EWK |
| `0x02` (`pk`) | policy version, `0x02`, ephemeral public key (32), wrapped DEK (72) | recipient's private key (section 3.1) |

**Flag bit.** `EVENT_NOTIFICATION_ZA_PENDING = 1 << 2` in
`CalendarEventNotification.flags`, next to upstream's
`EVENT_NOTIFICATION_IS_DRAFT` (`1`) and `EVENT_NOTIFICATION_IS_CHANGE`
(`1 << 1`). Set by the fan-out, cleared by the drain. The item is stored in
the scheduling inbox collection (`SCHEDULE_INBOX_ID`) as upstream stores
inbound items.

**Token pairs.** A list of (attendee address, 16-byte token) pairs inside
the sealed `X-ZA-EXTRA` bundle: all of the organizer's pairs on the
organizer's event, the attendee's own pair on an attendee's copy and on a
pending REQUEST or CANCEL item. Release 1 bundles start with format byte
`1` (`FORMAT_V1` in `crates/groupware/src/calendar/seal/tree.rs`) and hold
`Extra { display_name, dead_properties }`, decoded with a checked rkyv
read. A bundle that carries pairs uses format byte `2` and a plaintext
struct that adds the pair list; format `1` bundles keep reading as `Extra`,
so no archived layout changes. An event or item with neither extra fields
nor pairs carries no bundle, as in release 1.

**Reply property.** `X-ZA-RSVP-TOKEN` on the VCALENDAR root of a pending
reply item: the token, base64url without padding, visible. On an inbox
item's root the carriers are, as the root's last entries and in this
order, `X-ZA-RSVP-TOKEN` (reply items), `X-ZA-EXTRA` (REQUEST and CANCEL
items) and `X-ZA-KEY`, the way events place `X-ZA-EXTRA` before
`X-ZA-KEY`; anywhere else either is a stray carrier (release 1 section
7.1). A drained item carries neither.

**Associated data.**

| Object | Key envelope | Bundles (`tree_aad` scope) |
|---|---|---|
| Event (release 1) | `event-key`, account, UID | UID |
| Collection timezone (release 1) | collection bundle | `calendar-tz` |
| Pending inbox item | `pk-dek`, recipient, UID (the event-key shape) | `itip`, recipient's account id, component index |
| Drained inbox item | `itip-key`, owner, UID (the event-key shape) | `itip`, owner's account id, component index |

Inbox items use the one scope `itip` for their component bundles in both
states. A pending item's `X-ZA-EXTRA` bundle is bound like an event's
(`itip-extra`, recipient, UID). Both envelopes bind the UID, so an item's
sealed parts cannot be moved onto another UID's record, as a release 1
event's cannot.

The policy version stays 1: release 2 changes no visibility.

## 10. Error handling

- Fan-out: a failure for one recipient (quota, inbound size limit, a store
  error writing that item) becomes a delivery-failed `SCHEDULE-STATUS` on
  that recipient's property; the sender's request succeeds (section 5.3).
  A failure of the sender's own commit fails the request as today and
  leaves delivered items behind (section 5.4).
- Drain: item errors follow section 6.5.
- A `pk`-wrapped item reached by a read path other than the drain should
  not happen, since pending items are skipped. If one is reached, it is an
  unseal failure handled as release 1 section 10 handles one: 500 for the
  request or the single report item, a logged error with no content,
  nothing deleted or rewritten.
- A private-key unwrap failure in `SessionKeys` (a corrupt
  `private_key_wrap`) is traced once per session and the drain is skipped;
  the request proceeds. Pending items wait, since the private key is the
  only way to open them.
- A destroyed recipient account takes its inbox with it: the destroy task
  removes all account-keyed data, pending items included.
- No new trace event types are added (the event enums are generated
  upstream). Existing ones are reused with `zero-access:` details
  prefixes, and no trace carries a tree, an address, a token or any other
  content of a message.

## 11. Testing

**Unit.** In `crates/vault`: the `pk` wrap round-trips; it fails under
another recipient key and with associated data naming another account or
purpose; the ephemeral public key differs per wrap. In the groupware seal
tests: token pairs round-trip through a format `2` `X-ZA-EXTRA` bundle and
format `1` bundles still read; an item root ending in `X-ZA-RSVP-TOKEN` or
`X-ZA-EXTRA`, then `X-ZA-KEY`, unseals while either property anywhere else
is a stray carrier.

**Integration.** New sub-modules under `tests/src/za/`. The za suite is one
test function that runs its sub-modules in order against one server on
port 8899; it calls `assert_is_empty` once, at its end, after
`destroy_key_accounts`, and every sub-module removes what it creates.
The release 2 tests add test-mode hooks, compiled only under `test_mode`
like release 1's auth-cache pauses (`za_test` in
`crates/http/src/auth/authenticate.rs`, a `Pause` with `arrived` and
`release` notifies keyed by account id): a
fan-out pause between document-id assignment and the item commit, a
drain pause after its absence check, a PUT pause after the resource fetch,
a per-account drain skip (while set, that account's drain does not run),
a one-shot failure of the sender's commit after the fan-out, and a
task-consumer pause. None of them changes behaviour when unset. The
consumer pause must live where both `http` and `services` can reach it
(`common`), a note for planning.

- Invite between two key accounts, drained on the attendee's first request,
  and on a mid-session arrival: a second request by an already connected
  attendee after the organizer's PUT sees the item.
- Drain cursor: an item that arrives while a pass is rewriting an earlier
  item is processed on the next request; a transiently failing item is
  retried on a later request without any new arrival.
- Creation lease, four cases, each ending with exactly one calendar copy.
  Drain versus drain: with the fan-out pause, two fan-outs for one UID
  publish the lower document id late (section 6.3) while two sessions of
  one attendee drain. Drain versus PUT: a drain pause after the drain's
  absence check while another session PUTs the same UID into the default
  calendar; the loser either merges or answers `NoUidConflict`. Lease
  expiry: the lease duration is test-settable (`ZA_UID_LEASE_SECS`, a name
  to confirm); a holder paused past expiry while another session creates
  the UID loses its commit. Stale snapshot: a PUT pause after the resource
  fetch; while the PUT is paused, a drain commits the UID into the default
  calendar and releases its lease; the PUT resumes, takes the lease, and
  answers `NoUidConflict`.
- Lease sweep: plant an expired lease key and a live one in the data
  store; the sweep removes only the expired one, and the generic purge
  leaves both. It runs with
  the default configuration, where the in-memory store is the data store;
  a Redis-plus-data-store run of the same check would live in the `store`
  suite's Redis container harness (`tests/src/utils/containers.rs`, used by
  `tests/src/store/lookup.rs`), not added: the sweep reads and writes the
  data store on every configuration (plan 10).
- The attendee's copy lands on the default calendar with `itip_auto_add`
  on and off (a local sender always adds).
- An organizer client that drops unknown parameters on its next PUT keeps
  every attendee's token, and a reply afterwards still applies.
- Pairs survive rewrites that build no message: the organizer adds an
  alarm (a PUT that builds no message), then a reply from the attendee
  applies; the attendee adds an alarm to their copy, then replies, and the
  reply applies.
- Accepted REQUESTs adopt the incoming pair. Remove, re-add, accept: alice
  invites bob, removes him (bob's copy becomes cancelled) and re-adds him;
  bob drains (a merge into the cancelled copy) and accepts; the reply
  applies under the new token. Failed-create retry: the commit-failure
  hook fails alice's commit once after the fan-out wrote bob's item;
  alice's client resends; bob drains both items in order (the second
  merges with an equal SEQUENCE and adopts the new token); bob's reply
  applies.
- Update and cancel propagate to the attendee's copy, and the attendee's
  own alarm survives an update.
- A reply with a matching token applies; a forged token is dropped with the
  security trace and the organizer's copy unchanged.
- Stale reply: alice invites; bob drains and replies at SEQUENCE 0, and
  the reply item lands in alice's inbox. With alice's drain skipped, alice
  reschedules (SEQUENCE 1) and the PUT commits; the hook is cleared.
  alice's next request drains the stale reply, which is kept as history
  and not applied; a fresh reply from bob then applies.
- Delegation between three key accounts: bob delegates to carol. carol's
  inbox holds exactly one REQUEST, from alice, with a token; carol's reply
  applies to alice's copy; bob's attendee-originated REQUEST was not
  attempted; bob's inbox receives nothing from carol, and bob's copy is
  unchanged by carol's reply.
- A mixed attendee list (external, plain local, key) delivers to the key
  attendee only and marks the others not attempted.
- A plain local organizer inviting a key account takes upstream's path:
  the message reaches the key account's mailbox, and nothing reaches its
  scheduling inbox or calendar.
- Task queue: the test holds the task-consumer pause across a key
  organizer's PUT and DELETE, reads the task-queue subspace while the
  consumer is held, asserts it holds no `CalendarItipMessage` task for
  that account, then releases. Without the pause the worker can run and
  clear a task before the read: both handlers call `notify_task_queue()`
  right after `commit_batch` (`crates/dav/src/calendar/update.rs`,
  `delete.rs`), and the task manager's `update_tasks`
  (`crates/services/src/task_manager/manager.rs`) clears finished tasks;
  `wait_for_delivery` (`tests/src/utils/za.rs`) likewise waits until
  tasks are gone. The end-to-end check that no canary ever reaches any
  mailbox stays.

**Leak tests** (`tests/src/za/leak.rs` pattern). An invitation carries
canary titles, locations, descriptions and attendee addresses; the
whole-store scan, over every subspace including the scheduling inbox and
the task queue, expects zero hits. A second check asserts that no token's
bytes appear in the clear anywhere except on pending reply items. The
sealed-tree check covers inbox items, with an expected `X-ZA-KEY` count
greater than zero for them.

**Key-mode CalDAV suite** (`ZA_KEY_ACCOUNTS=1 webdav_tests`,
`tests/src/webdav/za_variants.rs`). The `scheduling` variant, which today
asserts that nothing is sent, flips to asserting sealed delivery and a
drained attendee copy. Upstream's `cal_scheduling` test still does not run
in key mode; the variant asserts the drained copy, the update and the cancel
(plan 10).

**Mutation checks** in the style of plan 7: removing the token check makes
the forged-reply test fail; removing the recipient split makes the
mixed-list test fail (plaintext in the task queue); restoring the plaintext
queue path makes the task-queue test fail, reliably rather than by chance
because the consumer is paused; removing the lease assertion from the
commit, or removing the lease from the PUT create branch, makes the
creation-lease tests fail; re-checking against the snapshot instead of
current archives makes the stale-snapshot case fail; rebuilding the
`X-ZA-EXTRA` bundle from the view's fields alone makes both
pairs-survive-rewrites cases fail; keeping the stored pair on merge makes
the re-add case fail; removing the pending-bit skip makes a
read of an undrained item fail.

**Manual checklist** (`docs/zero-access/manual-checklist.md`). Item 13
changes from "nothing is delivered" to a two-client invitation between two
key accounts on each native client (Apple Calendar, Thunderbird, DAVx5),
including accept, update and cancel.

## 12. Invariants for implementers

Release 1's ten invariants hold. Release 2 adds:

11. The `pk` wrap is opened only inside a drain running in a request that
    holds the recipient's session keys.
12. Nothing derived from an invitation sent by a key account (tree,
    summary, address, token-to-identity mapping) is ever written to the
    task queue, to mail, or to any store location in plaintext.
13. The organizer's event never stores an attendee's account id, and the
    token authenticates a reply; it does not hide the sender of an inbox
    item, whose principal id is visible by structure (section 2).
14. The drain never fails the request that triggered it. It never deletes
    an item because it cannot read it unless no key can ever open it (the
    permanent errors of section 6.5); the only other deletion is a reply
    whose token does not match (section 6.5), a deliberate, traced drop of
    a forgery.
15. The fan-out and the drain reuse upstream's iTIP builders and merge
    unchanged; the drain's own checks (sender, token, stale reply) run
    around them for key accounts only, so non-key accounts' scheduling
    behaviour is unchanged (invariant 9 holds).

## 13. Open items for planning

- **Names.** The sending state (`SendSealed`), the flag bit
  (`EVENT_NOTIFICATION_ZA_PENDING`), the HKDF label (`za/v1/pk`) and the
  exact info layout, the wrap purpose string (`pk-dek`, which the `aad`
  helper prefixes with `za/v1|`), the reply property (`X-ZA-RSVP-TOKEN`),
  the `X-ZA-EXTRA` format byte for bundles with token pairs (`2`), the
  inbox items' scope (`itip`) and drained key-envelope purpose, the
  drain's retry backoff (`DRAIN_RETRY_SECS`, a constant of 30 seconds),
  the creation lease prefix (`KV_LOCK_ZA_UID`), the lease duration and its
  test knob (`ZA_UID_LEASE_SECS`), the fence token length, and the lease
  retry count and wait (values for planning). The lease key carries the
  raw UID, and UIDs can be long; it may hold a fixed-length hash of the
  UID instead, a choice for planning. The names in this spec are its
  choices, to be confirmed.
- **Where the seal step gets the pairs.** The pairs cannot live in
  `CalendarEvent` fields (invariant 1), so the seal step takes them from
  the stored bundle: for example `seal_event` receiving the previous
  bundle's pairs alongside the event, or the unseal view returning them
  beside the event. Both event seal sites need it (`update.rs` and
  `proppatch.rs` in `crates/dav/src/calendar/`), as do the drain's
  commits. COPY and MOVE keep the stored bytes and need nothing.
- **Schedule-status codes.** Which RFC 6638 code marks a not-attempted
  recipient and which marks a failed delivery. The candidates are `3.7`
  (invalid calendar user; the only code upstream sets, for unparseable
  addresses, and what the outbox answers for a key target today), `5.1`
  (could not complete delivery) and `5.3` (no scheduling support for the
  user). The choice is a name, not a behaviour: either way the recipient
  is sent nothing and the status is sealed with the property.
- **The mid-session trigger.** Options: the change-log since-query for the
  inbox per request (section 6.2), or a per-account pending counter in
  memory (never a stored struct), bumped by the fan-out and broadcast to
  other nodes with `CacheInvalidation`. Plan 10 took the first option.
- **Upstream `cal_scheduling` assertions** runnable in key mode: settled
  by section 11 (plan 10). Upstream's test still does not run in key mode;
  the key-mode `scheduling` variant asserts the drained copy, the update
  and the cancel.
- **Pending items and `sync-collection`.** A drained item changes between
  two syncs, and the expected client behaviour is a normal change. The
  resource cache knows no flags, so the skip happens where items are
  loaded; whether the item then reports as new or as changed is settled
  with the tests.
- **The lease sweep.** Its place in the maintenance task's purge step;
  whether it is skipped when the in-memory store is the data store (the
  existing purge already covers that case); and whether a
  Redis-plus-data-store run of the sweep test joins the `store` suite's
  Redis container harness. Plan 10: after `purge_in_memory_store` in the
  `PurgeData` arm, on every configuration, no Redis container test.

## 14. Alternatives considered

- **B: write the attendee's calendar copy directly at invite time**, sealed
  to the attendee, and replace it wholesale on updates. Rejected: it
  discards the attendee's own additions, such as alarms, which upstream's
  merge preserves, and replies would still need the drain.
- **C: a new sealed queue outside the DAV inbox.** Rejected: a new subspace
  and a new stored struct, against the spirit of invariant 1, when the inbox
  collection already is that queue.
- **Visible RSVP status next to each token**, as issue #17 and release 1
  section 1.2 originally put it. Rejected: participation status per
  anonymous attendee would become visible metadata, and a reply comment
  would still need the sealed path. Replies use the same sealed item type
  instead.

## 15. Release 3 and sharing-release pointers

- iMIP for external and plain local attendees, reopening the task queue
  and mail paths with sealed contents once mail is encrypted at rest.
- The sealed notification email for invitations.
- The HTTP RSVP page for external attendees.
- Issue #32: public-key fingerprints on the Circulo page and pinning at
  first use.
- Sharing release: free-busy between users, through grants (section 7).
