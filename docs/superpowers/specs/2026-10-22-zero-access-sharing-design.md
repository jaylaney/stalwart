# Zero-access calendar: sharing between key accounts (release 4) design spec

Date: 2026-10-22. Status: revision 14, draft for review. Scope: release 4
of the zero-access fork (jaylaney/stalwart); GitHub issue #24, milestone
"Release 4: calendar sharing". Revision 2 answers the builder check and
the adversarial review of revision 1: the shared bundle (3.3), the
destroyed owner (8.6), the grantee's scheduling rule (6.2), the conversion
of plaintext entries and events (3.5), and the precisions listed in 13.1.
Revision 3 replaces revision 2's refusal of a grantee's scheduling writes
with delegate sending (6.2), by Jay's decision that a grantee may do what
Stalwart lets a grantee do today. It also records two decisions of
2026-10-22 that were open items: a grant accepts either pin source (4.3),
and the single-parent rule stands as written (5.6). Revision 4 corrects
the destroyed-owner design (8.6: upstream's registry destroy already
revokes the owner's grants), answers the review of revision 3's delegate
sending (6.2), and adds the precisions listed in 13.1. Revision 5 answers
the 2026-10-22 design review
(`2026-10-22-zero-access-sharing-design-review.md`): the conversion's
single-parent check (3.5), MOVE into a shared calendar (5.6), revoke by
grant id (4.4), the paths that bypass the resolver (5.1, 5.2), a grant
racing the owner's destruction (8.6), the same-level re-key's
invalidation (4.6), DELETE's calendar fence (5.5), and two corrections
(6.2, 11.5). Revision 6 answers the re-review of revision 5 (the same
review file, updated): the conversion's abort and the linked-event write
rule (3.5, 5.5), foreign-path eligibility in the grantee's own home (5.2,
5.4), the grantee's sync token (5.2), the writer's self-delivery (6.2,
6.3), the alias rule (4.4) and `SENT-BY` per component (6.2). With it the
three earlier findings the re-review marked partial (review findings 1, 4
and 8) are complete. Revision 7 answers the re-review of revision 6: the
key-domain id (3.1, 3.2, 3.6), the sync boundary on a rename (5.2, 5.6),
self-delivery for every delegated message (6.2) and per-address self items
(6.2). With it re-review findings 3 and 4, which the review of revision 6
marked partly addressed, are complete. Revision 8 answers the re-review
of revision 7: the rename boundary recorded in two fenced steps (5.2, 5.6)
and the self-message builder (6.2, 6.3). With it re-review findings 2 and
3 of revision 6, which the review of revision 7 marked partly addressed,
are complete. Revision 9 answers the re-review of revision 8: self-message
payloads for the account's single copy (6.2), and two handoff precisions
(5.2, 6.2). Revision 10 answers the re-review of revision 9 and the
reviewers' recommendation of a bounded lifecycle pass: the writer's
self-messages are upstream's builders run under the owner's addresses (6.2),
the behaviours the writer's copy inherits from upstream are recorded (6.2),
and a differential lifecycle test is the stopping rule (11.2). With it
revision 9's re-review finding 1 is accepted as inherited (6.2's table, row
1), not fixed, and its clarification is answered by parity (row 3);
revisions 8 and 9's self-message builder is withdrawn. Revision 11 answers
the lifecycle review of revision 10: the exception narrowed to RSVP and
PARTSTAT (6.2) and the committed tree finalized when only the second run
built (6.2). Revision 12 incorporates Jay's decisions on the implementation
plan review: MOVE overwrite/source-unlink keeps upstream's no-scheduling
behaviour (5.6, 6.2); self-message recipients are classified by account
resolution, with a provenance-based self-inbox exception (6.2, case 25).
Revision 13 incorporates Jay's D3/D4 decisions on that recheck: listed
writer addresses alone get the RSVP/PARTSTAT exception; routing retains
both writer-only listed addresses and addresses resolving to the writer
(6.2). It completes the hook definitions, distinguishes builder success
from recipient filtering, mirrors both update-site builder branches,
and makes the overwrite, sub-address, group and owner-status assertions
explicit (11.1–11.2, 11.6), including the payload-locality limit in 6.2's
inherited table. Invariant 38 and the merge dependencies follow
these rules; incoming validation remains unchanged.
Revision 14 incorporates D5 and the revision 13 plan review's accepted
clarifications: the routing override is limited to directly owned listed
aliases, delegated identity roles and quiet catch-all lookup are explicit,
conversion conflicts remain safe and resumable, and MOVE covers legacy
plaintext input and an observable no-scheduling overwrite check (3.5,
5.6, 6.2, 11 and 14).

The terms key account, master key (MK), event wrapping key (EWK), data key
(DEK), `X-ZA-KEY`, `X-ZA-SEALED`, `X-ZA-EXTRA`, wrap type, collection
bundle, session keys, key cache, generation (of a vault record) and visible
by structure are those of the release 1 spec,
`2026-10-06-zero-access-calendar-design.md` (revision 7), cited below as
"release 1". The terms `pk` wrap, fan-out, drain, pending item, token pair,
UID lease, fence and `SendSealed` are those of the release 2 spec,
`2026-10-11-zero-access-invitations-design.md`, cited as "release 2". Pin,
pin record, served key, first use, the wrap mark and the contacts
endpoints are those of the key pinning spec,
`2026-10-17-zero-access-key-pinning-design.md` (revision 11), cited as
"key pinning". The mail wrapping key and the keys of mail sessions are
those of the mail spec, `2026-10-20-zero-access-mail-design.md` (revision
5), cited as "mail". Every invariant of those specs (1 to 32, with mail's
two amendments) continues to hold; this spec adds to them and relaxes
none. It supersedes sections 1 to 6, 9 and 10 of
`2026-10-08-zero-access-sharing-sketch.md` (release 2 superseded its
sections 7 and 8), and, for shared calendars, release 1 section 7.2's
sentence "Sharing is refused for key accounts, so only the owner's entry
exists". The account page's endpoint contract is `docs/api.md` in
`jaylaney/circulo-account`, cited as "api.md".

Decisions made by this spec rather than settled before it are marked
"(author's call)", so that review can find them.

## 1. Scope and goals

Two key accounts on one server share calendars. The first deployment is a
family: two people, several calendars each shared with the other, both
editing, both inviting each other to events. The owner grants a calendar to
the other person at the view or edit level from the Circulo account page,
or by hand with curl until the page exists; the grantee's unmodified CalDAV
client (Apple Calendar, Thunderbird) finds the calendar at its next sync and
reads and writes it with the rights the grant gives; the owner removes the
grant whenever they like. The server keeps the shared calendar sealed as
release 1 keeps every calendar of a key account, under a key that only the
sessions of the owner and of the current grantees can open.

### 1.1 In scope

- Grants of a key account's calendar to another key account on the same
  server, at two levels, view and edit, in Stalwart's own ACL bits (1.2).
- A collection key per shared calendar, wrapped to the owner under its
  master key and to each grantee under the public key the owner pinned for
  it; the DEKs of the calendar's events and of its preferences entries are
  wrapped under the collection key (section 3).
- Three endpoints under `/api/vault/` that the Circulo page calls: list,
  grant and revoke (section 4).
- The DAV gate resolving keys per calendar instead of per account, so that
  a grantee's own session reads and writes the owner's calendar
  (section 5).
- Invitations from a shared calendar as release 2 delivers them. A
  grantee schedules from the owner's calendar as upstream lets it, in the
  owner's name, and the messages it sends are marked `SENT-BY` the grantee
  and verified by the recipient's drain against the grant (section 6).
- Revocation, rotating the collection key for future writes (section 7).
- Tests, the leak scanner's sharing rules and a real-device check with
  Apple Calendar (section 11).

### 1.2 Grant levels

Release 4 adds no right and no bit. A grant is an entry in the calendar's
existing `acls` list (`Calendar.acls`,
`crates/groupware/src/calendar/mod.rs`), with bits from the existing `Acl`
set (`crates/types/src/acl.rs`) and the existing JMAP and CalDAV names for
them. Release 4 grants exactly two combinations:

| Level | `Acl` bits | JMAP rights (`crates/jmap-proto/src/object/calendar.rs`) | CalDAV | `current-user-privilege-set` the grantee's client reads | What the grantee can do |
|---|---|---|---|---|---|
| view | `Read`, `ReadItems` | `mayReadItems` | `read` | `read`, `read-current-user-privilege-set` | see the calendar in its home, read its properties and events, query and sync it |
| edit | view, plus `Modify`, `AddItems`, `ModifyItems`, `RemoveItems` | `mayReadItems` and `mayWriteAll` | `read`, plus `write` without `Delete` | the above, plus `write-properties`, `write-content` and `write` | view, plus create, change and delete events, and change the calendar's properties: its own name, description and time zone, and the calendar's dead properties, shared by every principal (below) |

**Colour and order are shared, as upstream.** Apple's `calendar-color` and
`calendar-order` (and Thunderbird's colour) are dead properties: upstream
has no Apple namespace, so `PROPPATCH` stores them in the calendar's one
`dead_properties` set (`crates/dav/src/calendar/proppatch.rs`), which
`PROPFIND` serves to every principal, and only a `Modify` holder may change
them. Release 4 keeps that behaviour: an edit grantee's colour change
recolours the calendar for the owner and every grantee, and a view
grantee's is upstream's 403. `CalendarPreferences.color` is never written
by the DAV crate (only JMAP writes it), so no supported client sets a
per-principal colour; release 4 claims none. What is per principal is the
display name, the description, the custom time zone and the visible sort
order, flags and default alerts of each preferences entry.

The `write` privilege of the CalDAV `ACL` method maps to `Modify`,
`Delete`, `AddItems`, `ModifyItems` and `RemoveItems`
(`crates/dav/src/common/acl.rs`); edit is that set without `Delete`.
`current_user_privilege_set` reports `write` for `RemoveItems`, so a client
shows an edit grant as writable.

Not granted in release 4, and refused by the grant endpoint, which takes a
level name and never a bit list, so no other combination can be asked for:

- `Share` (re-sharing; it also reveals the grant list through `DAV:acl`).
- `Delete` (deleting the owner's calendar).
- `SchedulingReadFreeBusy`, alone (a free/busy-only grant) or added to a
  level. It needs no key, since busy times are visible by policy (release 2
  section 7), and can be added later as policy.
- `ModifyItemsOwn`, `ModifyPrivateProperties`, `ModifyRSVP`,
  `SchedulingInvite` and `SchedulingReply`, which upstream lets a writer
  set but never enforces anywhere.
- `CreateChild` and `Submit`, which apply to mailboxes only.

### 1.3 Non-goals

- **Groups.** A grant to a group is refused (a group is not a key account,
  4.3). Operator-managed plaintext group calendars stay as in release 1: a
  key account may read them, and they are not zero-access. A group keypair
  is an alternative (section 13).
- **Re-sharing, free/busy-only grants and deleting the owner's calendar**
  (1.2). Free-busy between users, which release 2 section 7 placed in this
  release, stays deferred: no grant carries `SchedulingReadFreeBusy`, so
  the free-busy gates of release 2 stand (5.3).
- **Grants to non-key accounts**, local or external.
- **JMAP.** `shareWith` and JMAP calendars stay refused for key accounts
  until release 6.
- **Granting from a calendar app.** Upstream implements only the raw WebDAV
  `ACL` method (no CalendarServer `CS:share` invitations and no calendar
  proxy: the `CS` namespace parses only `getctag`,
  `crates/dav-proto/src/parser/property.rs`), so no mainstream CalDAV
  client can grant against it. The `ACL` method stays refused on a key
  account's calendar (release 1 section 9).
- **Resealing existing events on revoke** (7.3).
- **Mail sharing**, refused by mail section 6.3, unchanged.
- **Sharing across servers.**

### 1.4 Anti-goal: the trust model

Nothing in release 4 weakens zero access. Specifically:

- The operator cannot read a shared calendar at rest, nor while the owner
  and every grantee are logged out.
- A collection key exists in plaintext only inside a logged-in session of
  the owner or of a current grantee.
- No wrap on a calendar can be opened by anyone but the principal it
  names.
- A grantee never learns the owner's master key, event wrapping key or
  private key, and the owner never learns the grantee's.

Section 12 states these as invariants, and section 11 adds the leak
scanner rule that checks them in the store.

### 1.5 Assumptions

- The assumptions of releases 1 to 3 hold: key accounts from creation, the
  internal directory, a store that starts empty.
- Owner and grantee are key accounts whose setup is complete, so each has
  a public key (release 2 section 1.3).
- The owner has pinned the grantee's address (key pinning), on the page or
  by a first invitation (4.3).
- A deployment with Apple clients keeps assisted discovery on, its
  default (5.4).
- No setting, permission, event type or environment variable is added
  (the registry schema and the event enums are generated). The existing
  `maxShares` (`Sharing` object) and `enableAssistedDiscovery` (`WebDav`
  object) settings apply.

## 2. Trust model additions and threat cases

**Claim.** Release 1's guarantee extends to shared calendars. The sealed
fields of an event in a shared calendar, the calendar's shared dead
properties (its colour and order among them), and the names, descriptions
and custom time zones of each principal's view of it, are unreadable from
the stored data without the credentials of the owner or of a current
grantee, or a key resident in a server's memory. A collection key is
written to the store only wrapped, once under the owner's key and once to
each grantee's pinned public key, and it is resident only in sessions of
those principals.

**Limits, stated alongside the claim.**

- As in release 1, the running server holds plaintext and collection keys
  while it serves a session of the owner or of a grantee. A grantee's
  session also opens the organizer's RSVP token pairs of the events it
  rewrites (3.6).
- **Rotation, not resealing.** A revoke stops a removed grantee from
  reading anything written afterwards; events written before the revoke
  stay under the collection key the removed grantee held (7.3). The claim
  holds against a store reader, not a store writer (2.1).
- **A cluster needs pubsub** for grants and revokes to reach every node:
  upstream broadcasts cache invalidations only with pubsub, best effort
  (4.6). On a node that missed a grant the calendar is absent for the
  grantee until that node rebuilds its token; on a node that missed a
  revoke the use-time checks refuse the grantee, but the collection keys
  resident in its session keys linger until the key cache drops them, at
  most its 60-minute cap; on a node that missed the invalidation of an
  owner's destruction, a grantee's stale token lists the dead owner until
  it is evicted, and its discovery fails there (4.6, 8.6).
- **Trust in the grantee's key** is the trust in the owner's pin for the
  grantee's address (key pinning). A pin made at the first use of an
  invitation inherits that pin's exposure (key pinning section 2, limit
  1); a pin confirmed on the page after an out-of-band comparison does not
  (4.3).
- **A grantee acts for the owner.** An edit grantee sends invitations,
  updates, cancellations and replies in the owner's name, as upstream lets
  it, marked `SENT-BY` the grantee (6.2). The recipients of a delegated
  message are sealed to under the grantee's pins, not the owner's, so the
  trust in them is the grantee's. The authority for a delegated message is
  upstream's ACL at the time of the write; the recipient's drain checks
  only that the message is consistent with its sender, as defence in depth
  against server bugs, not as an authorization. So an
  item written while the grant stood is delivered after a revoke, and an
  edit grant on one calendar of the owner lets a delegated REQUEST reuse
  the UID of an event in another of the owner's calendars and rewrite its
  recipients' copies, as upstream allows (an inherited limit, 6.2).
- **Integrity** is release 1's: a store writer can drop, swap or delete
  wraps, entries, grants and events (availability), can add an ACL entry
  or flip a grant's bits, and can plant a calendar entry that names any
  owner, wrapped to a grantee's public key (which is public), so a grantee
  cannot authenticate who shared a calendar with it. None of this gives
  the writer a collection key.

**Visible metadata added** (beyond releases 1 to 3):

- Who shares which calendar with whom, and at which level: the calendar's
  `acls` list (grantee account ids and bits) and upstream's ACL index under
  each grantee's id, both plaintext as upstream stores them. The grants
  disappear with the owner's account: the registry's destroy revokes them
  (8.6).
- Upstream's share notifications (2.2).
- The number of principals of a shared calendar (its preferences entries),
  each entry's account id and its visible fields (sort order, flags such
  as subscribed or hidden, default alerts), as release 1 leaves them
  visible.
- The number of collection-key generations (one per revoke, re-key or
  sweep, 7.2) and which generation each event and entry was last written
  under, hence which events were written after which revoke.
- When a calendar was first shared: the conversion rewrites the key
  envelope of every event in a burst (3.5), each with a new ETag and a
  change-log entry.
- Which principal wrote each change (upstream's change log records
  `changed_by`), and the paths a grantee's client requests under the
  owner's URLs (request logs; paths are visible by structure).
- Who acts for whom. A delegated message's `SENT-BY` (6.2) shows its
  recipients that the grantee sent it in the owner's name. Each component
  of a recipient's stored copy carries the `SENT-BY` of the message that
  set its ORGANIZER: the import that created the copy, a later REQUEST
  that added the component, or a CANCEL merged into it; a REQUEST update
  of a component the copy already holds leaves it as stored. The copy keeps
  it after a revoke, and the recipient's REPLY echoes it (review finding 8,
  re-review finding 6). The parameter is sealed with its property, so the
  operator does not read it, but the operator sees each
  item's `changed_by` (the grantee, as release 2 records any sender) and
  can match the item's visible UID with an event of the owner's calendar.
- The sizes of the shared form's parts: rings grow by a fixed amount per
  generation and per grantee, and bundles keep release 1's 256-byte size
  classes.

**Not visible:** every name, description and custom time zone of each
principal's view of the calendar, the calendar's dead properties (its
colour and order among them), the addresses and keys the owner's ledger
records (3.3), the collection keys, and everything release 1 seals in
events.

### 2.1 Threat cases

- **A store reader** (a stolen disk, a backup, a database dump, a demand
  for stored data) reads the metadata above and nothing sealed. Opening any
  wrap needs the owner's password-derived keys or a grantee's.
- **A removed grantee, alone.** A CalDAV client never receives a key: the
  grantee saw plaintext through the server and could copy it, and that is
  all. After the revoke the server refuses its requests (7.1).
- **A removed grantee colluding with the operator.** With an old copy of
  the store (a backup taken before the revoke, or the grantee's ring saved
  then) the pair holds the grantee's grant ring; the grantee's password
  derives its MK and opens its private key; the private key opens every
  generation that ring lists, and those generations open every event and
  entry sealed under them, in the old copy and in the live store alike.
  They cannot open anything sealed under a generation created at or after
  the revoke. Resealing on revoke would take the live store's old events
  out of their reach (7.3).
- **A store writer undoing a revoke.** The rotation claim holds against a
  store reader. A store writer who restores a pre-revoke calendar archive
  from a backup restores the removed grantee's `acls` entry, its entry and
  ring and the ledger record, so the resolver re-admits it, and the
  owner's next writes seal under the newest generation the restored ring
  lists, the one the grantee holds, until the owner looks at the list and
  revokes again. This is release 1 section 2's integrity concession (a
  store writer can swap stored records); an in-session high-water mark of
  generations is recorded as optional hardening (13.2).
- **A compromised grantee account** (an attacker holds the grantee's
  password or an app password) reads every calendar shared to it and, with
  edit, changes and deletes its events, as the grantee could. It can send
  invitations, updates, cancellations and replies in the owner's name,
  marked `SENT-BY` the grantee, as upstream allows an edit grantee (6.2).
  It cannot delete the calendar, re-share it or change its ACL. The owner's
  remedy is a revoke. The operator still reads no content, and no key
  crosses between the accounts.
- **A compromised owner account** is release 1's case: the attacker reads
  the owner's calendars anyway, and can grant them to a key account it
  controls (after pinning it) or revoke grants.
- **A recovered account.** Recovery and password changes keep MK and the
  private key (release 2 section 3.2), so the owner's ring (under the EWK,
  derived from MK) and every grant ring (to the unchanged public key) stay
  valid. Nothing needs re-sharing.
- **A grantee who starts over** (a fresh setup with a new keypair, which
  key pinning section 1.4 calls rare and destructive): its old wraps open
  nothing for it. The owner's list shows the grant as `key changed`, and
  the owner re-keys it after comparing fingerprints (7.4).
- **Key substitution.** A grant requires the served key to equal the pin,
  and wraps to the pinned key (4.3); a rotation wraps to the key recorded
  at the grant, never to the served key (7.2). A store writer who
  substitutes a grantee's served key therefore gains no wrap, and the
  owner's list shows `key changed`.
- **A destroyed grantee or owner** (8.6).
- **The running server** can capture the keys and plaintext of any
  session it serves, as in release 1.

### 2.2 Share notifications

Upstream's index builder writes a `ShareNotification` under the grantee's
id whenever a calendar's `acls` change (`IndexValue::Acl` in
`crates/common/src/storage/index.rs`): the owner account, the calendar's
document id, the collection type, the account that made the change, the
old and new bits, and a name that upstream always leaves empty. Release 4
keeps them (author's call): they reveal nothing that the `acls` list and
the ACL index do not already show, they carry no calendar name, and
suppressing them would add a fork branch to an upstream hotspot for no
confidentiality gain. A grant, a level change and a revoke each write one
for the grantee concerned; a rotation changes no bits and writes none. A
sweep of a destroyed grantee (7.5) writes one under the dead account's id
too, since `IndexValue::Acl` logs a notification for every removed entry
without an existence check: an orphan record, ids only, which nothing
reads (8.6).

## 3. Key model

### 3.1 The collection key and its generations

Each shared calendar has a collection key (CK): 32 random bytes generated
by the owner's session at the calendar's first grant (3.5). A revoke, a
re-key and a sweep of a gone grantee each add a new CK (7.2), so a calendar
holds a short list of CKs, its generations, numbered from 1 upwards. Every
write seals under the newest generation; reads use the generation each
object names. No CK is discarded while the calendar exists: the owner and
every current grantee keep every generation, so events written before a
rotation stay readable to them.

**The key-domain id** (re-review finding 1, revision 6). Every conversion
to the shared form (3.5) draws a random 16-byte key-domain id. It is stored
in the visible header of the owner ring and of every grant ring (3.3),
bound into the associated data of every object of the shared form (3.2),
and part of the identity under which a session caches a CK (3.6). A
conversion undone by 3.5's revert and later done again draws a new id and a
new CK numbered generation 1 again; without the id, a resident session's
cached generation-1 key of the earlier conversion would answer for the new
one and seal new events under a key no ring holds, unreadable once that
cache entry expired. With it, a key of a previous conversion can never
answer for the current one. A revert keeps the id until the unshared entry
is written. The cost is 16 bytes in each ring header (8.7) and in every
associated data of the shared form.

A CK wraps DEKs only: the DEK of each event (3.4), the bundle DEK of each
principal's preferences entry and the DEK of the calendar's shared bundle
(3.3). Content is never sealed under a
CK directly, so a rotation, or a later reseal (7.3), never has to touch
content to change which key protects it.

### 3.2 Wraps and associated data

- **The owner's copy** of every generation is wrapped under the owner's
  EWK, the HKDF of MK under which release 1 wraps every calendar DEK
  (author's call: the EWK rather than MK itself, so that the CK sits in
  the key domain of the calendar keys it replaces). It lives in the owner
  ring (3.3), sealed together with the owner's ledger of grants.
- **A grantee's copy** of every generation is wrapped to the grantee's
  pinned X25519 public key with release 2's construction (`pk_wrap` in
  `crates/vault/src/keys.rs`: an ephemeral X25519 key, the HKDF label
  `za/v1/pk` with both public keys in the info, XChaCha20-Poly1305; no
  HPKE). It lives in the grantee's ring (3.3). Its associated data has its
  own purpose, so a grant wrap cannot be replayed as an invitation's `pk`
  key envelope or the other way round.

Associated data, in the notation of release 2 section 9 (`aad(purpose,
account)` is `za/v1|<purpose>|<account>`; account ids, calendar document
ids, generations and principal ids are big-endian `u32`; `domain` is the
conversion's 16-byte key-domain id, 3.1; `|` is the byte `0x7c`):

| Object | Sealed or wrapped under | Associated data |
|---|---|---|
| Owner ring payload (CKs and ledger) | owner's EWK | `aad("ck-owner", owner) \|\| "\|" \|\| calendar \|\| "\|" \|\| domain \|\| "\|" \|\| ring header` (the visible bytes before the nonce, 3.3) |
| Grant wrap of generation `g` to grantee `r` | `pk_wrap` to the public key the ledger records for `r` | `aad("ck-grant", owner) \|\| "\|" \|\| calendar \|\| "\|" \|\| domain \|\| "\|" \|\| r \|\| "\|" \|\| g` |
| Event key envelope, wrap type `0x03` | CK generation `g` | `aad("ck-dek", owner) \|\| "\|" \|\| calendar \|\| "\|" \|\| domain \|\| "\|" \|\| g \|\| "\|" \|\| uid` |
| Entry bundle envelope of principal `p` | CK generation `g` | `aad("ck-bundle", owner) \|\| "\|" \|\| calendar \|\| "\|" \|\| domain \|\| "\|" \|\| g \|\| "\|" \|\| p` |
| Entry bundle of principal `p` | that entry's bundle DEK | `aad("ck-calendar-bundle", owner) \|\| "\|" \|\| calendar \|\| "\|" \|\| domain \|\| "\|" \|\| p` |
| Shared bundle envelope | CK generation `g` | `aad("ck-shared-dek", owner) \|\| "\|" \|\| calendar \|\| "\|" \|\| domain \|\| "\|" \|\| g` |
| Shared bundle (the calendar's dead properties) | the shared bundle's DEK | `aad("ck-shared-bundle", owner) \|\| "\|" \|\| calendar \|\| "\|" \|\| domain` |
| Custom time zone of an entry | that entry's bundle DEK | release 1's `tree_aad(owner, "calendar-tz", component index)` |
| Event trees, `X-ZA-EXTRA` bundles, token pairs | the event's DEK | release 1 and 2, unchanged: the owner's account and the UID |

`owner` is always the calendar's account, also for objects a grantee
writes: an event in the owner's calendar belongs to the owner's account
whoever wrote it. Binding the calendar's document id (not its slug) means a
wrap or bundle moved to another calendar fails to open, while a rename,
which keeps the document id, rewraps no key and reseals no event or bundle
(it records only the sync boundary in the ring headers, 5.2). This is
stricter than release 1's collection bundle, which binds the account only
so that a collection COPY needs no reseal; a shared calendar is never
copied (5.6).

### 3.3 Where each is stored: the shared form

No stored struct changes layout (release 1 invariant 1). Everything rides
in fields that exist: `Calendar.preferences[*].name` (a `String`),
`Calendar.acls`, and the event's `X-ZA-KEY` root property.

A calendar of a key account is in one of two forms:

- **Unshared** (release 1): one preferences entry, the owner's, whose
  `name` is `$za$` + base64(release 1 key envelope, wrap type `0x01`) +
  `|` + sealed bundle. Its events carry `0x01` key envelopes under the
  owner's EWK.
- **Shared**: one preferences entry per principal, the owner's and one per
  grantee. A grantee's entry has a `name` of three parts: `$za$` +
  base64(ring) + `|` + base64(bundle envelope) + `|` + sealed personal
  bundle. The owner's entry has the same three parts followed by two more,
  the calendar's shared bundle: `|` + base64(shared bundle envelope) + `|`
  + sealed shared bundle (author's call: the owner's entry exists for the
  calendar's whole life, so the calendar-level bundle rides there as its
  fourth and fifth parts). Its events carry `0x03` key envelopes (3.4). The
  ring's type byte, `0x04` for the owner (five parts) and `0x05` for a
  grantee (three parts), and the number of parts tell the forms apart; a
  reader refuses any other shape.

A calendar enters the shared form at its first grant and stays in it after
its last revoke, with an owner ring and no grantee (author's call:
converting back would rewrap every event again for no confidentiality
gain, since the CKs the removed grantees held are already retired).

**Owner ring** (`0x04`): policy version (1 byte) || `0x04` || calendar
document id (4) || key-domain id (16) || rename change id (8) ||
boundary-pending flag (1) || generation count `n` (2) || the `n` generation
numbers (4 each, ascending) || nonce (24) || XChaCha20-Poly1305 ciphertext
and tag of the rkyv `ZaOwnerRing`, length-prefixed and padded to a multiple
of 256 bytes as release 1 pads bundles, decoded with a checked rkyv read.
The generation numbers stay visible so that the leak scanner can check
events against them; the associated data binds them. The key-domain id is
the conversion's (3.1); the rename change id is the change id the store
assigned to the commit of the calendar's last slug change, zero until one
(5.2, 5.6); the boundary-pending flag (`0` or `1`, any other value a parse
failure) is set while a rename's boundary is not yet recorded (5.2).

| Struct | Field | Type | Meaning |
|---|---|---|---|
| `ZaOwnerRing` | `version` | `u8` | `1`; any other value is an unseal failure. |
| | `keys` | `Vec<[u8; 32]>` | The CK of each generation, in header order. |
| | `converted` | `bool` | Every event of the calendar carries a `0x03` envelope (3.5). |
| | `reverting` | `bool` | A conversion that found a linked event is being undone: writes seal under the EWK while it is set (3.5). |
| | `grants` | `Vec<ZaGrant>` | The ledger: one per current grantee, unique by account id. |
| `ZaGrant` | `account_id` | `u32` | The grantee's account, as `acls` names it. |
| | `address` | `String` | The address the grant was made to, normalized as key pinning section 4.3 normalizes pins. |
| | `public_key` | `[u8; 32]` | The key every generation is wrapped to for this grantee: the pin's key, equal to the served key, when the grant or its last re-key was made. |
| | `level` | `u8` | `1` view, `2` edit, kept equal to the `acls` bits by every write. |
| | `granted_at` | `i64` | Unix seconds of the grant or of its last re-key. |
| | `change_id` | `u64` | The grant change id, as the grant ring's header carries it (5.2); kept by a re-key and a level change. |

The ledger is the owner's own record of whom it trusted with which key,
sealed under the owner's EWK so that neither a store writer nor a grantee
can read or alter it. Rotations wrap to its keys (7.2), and the list
endpoint reports its addresses (4.2), never the directory's. The structs
have a silent `Debug` (they hold keys and addresses).

**Grant ring** (`0x05`): policy version || `0x05` || calendar document id
(4) || key-domain id (16) || grantee account id (4) || grant change id (8)
|| rename change id (8) || boundary-pending flag (1) || count `n` (2) || `n`
records of generation (4) || ephemeral public key (32) || wrapped CK (72).
It is not sealed as a whole: each record is a `pk_wrap` whose associated
data binds the owner, the calendar, the key-domain id, the grantee and the
generation (3.2). The grantee account id in the header must equal the
entry's `account_id`, and the key-domain id the owner ring's. The grant
change id is the change id the store assigned to the grant's own commit
(zero until it is recorded) and the rename change id the owner ring's; the
greater of the two is the grantee's sync boundary (5.2). The flag is set
while either is not yet recorded, and while it is set the grantee's
incremental syncs of the calendar are refused and its initial syncs served
(5.2).

**Bundle envelope**: policy version || `0x03` || calendar document id (4)
|| generation (4) || the entry's bundle DEK wrapped under that generation's
CK (72), the same layout as an event's `0x03` key envelope (3.4). The
**shared bundle envelope** has the same layout and wraps the shared
bundle's own DEK.

**Bundles.**

- A **personal bundle**, one per entry, holds that principal's name and
  description: for the owner, release 1's `Bundle` with its
  `dead_properties` empty; for a grantee, its name and description. A
  custom time zone of the entry is sealed in place under the same DEK, as
  release 1 section 7.2 does.
- The **shared bundle**, one per calendar, holds the calendar's dead
  properties, Apple's `calendar-color` and `calendar-order` among them,
  under its own DEK wrapped under the newest generation. Dead properties
  belong to the calendar, as upstream keeps them, and are shared by every
  principal (1.2).

The entries' `description` and `color` fields and the calendar's
`dead_properties` field stay empty; sort order, flags, default alerts and
the account id stay visible. No entry's bundle carries a colour a client
could set: `CalendarPreferences.color` is never written by the DAV crate
(1.2). Every rewrite of a bundle draws a fresh DEK and wraps it under the
newest generation.

**A grantee's entry** is created by the grant as upstream's
`preferences_mut` would create it at that moment
(`crates/groupware/src/calendar/mod.rs`: a copy of the owner's entry): the
owner's current name, description, sort order, default alerts and time
zone, with flags cleared, sealed for the grantee. It is a snapshot
(author's call): a later rename by the owner does not reach a grantee's
view, where upstream would show the owner's live entry until the grantee's
first personal change. Because the grant must create the entry up front
(it holds the grantee's ring), following the owner live would need an
inherit marker in the grantee's bundle; section 13 records that option.

**Who can open what.** The owner opens its ring (EWK) and, through the
CKs, every bundle and every event. A grantee opens its own ring (private
key) and, through the CKs, its own personal bundle and the shared bundle;
it never opens the owner's ring, and its session never opens or rewrites
the owner's personal bundle nor another grantee's. The CK would open those
two (it wraps their DEKs), so this is a rule of the code that a unit test
checks (11.1), not a cryptographic bound. A grantee's personal name is
therefore opened by the owner's sessions, which open every bundle, and by
another grantee's only if the code broke that rule; it is never returned
to anyone but the grantee by any response (upstream answers each principal
from its own entry, `personal_id`), and never readable by the operator.

`Calendar.acls` stays upstream's plaintext list. Each grant's bits are the
level's (1.2). Every non-owner entry has exactly one `acls` entry and one
ledger record, and the reverse.

### 3.4 Event keys and the key domain

An event's DEK is wrapped under the **key domain** of its calendar: the
owner's EWK for an unshared calendar (release 1's `X-ZA-KEY` wrap type
`0x01`), or the newest generation of the calendar's CK for a shared one, as
wrap type `0x03`:

| Wrap type | Bytes | Opened with |
|---|---|---|
| `0x01` (release 1) | policy version, `0x01`, wrapped DEK (72) | the owner's EWK |
| `0x02` (release 2) | policy version, `0x02`, ephemeral public key (32), wrapped DEK (72) | the recipient's private key; inbox items and mail only |
| `0x03` (new) | policy version, `0x03`, calendar document id (4), generation (4), wrapped DEK (72) | that generation of that calendar's CK |

Only the key envelope differs: the event's trees, `X-ZA-SEALED` bundles,
`X-ZA-EXTRA` bundle and token pairs are sealed under the DEK with release 1
and 2's associated data, unchanged. A `0x03` envelope's calendar must be
the event's only parent (5.6) and its generation one the calendar lists;
anything else is an unseal failure (release 1 section 10). The envelope
does not repeat the key-domain id: a reader takes it from the calendar's
ring header for the associated data (3.2), so an envelope sealed under a
key of an earlier conversion fails to open rather than being read under
the wrong key. The policy version stays 1: release 4 changes no
visibility.

### 3.5 The first grant: conversion and the DEK rewrap

The first grant of a calendar converts it to the shared form inside the
owner's grant request, in three phases (author's call):

1. **Conversion.** One batch: open the owner's entry under the EWK, draw a
   fresh key-domain id (3.1), generate CK generation 1, reseal the owner's
   name and description into a personal bundle and the calendar's dead
   properties into the shared bundle, each under a fresh DEK wrapped under
   generation 1, and write the owner ring (`converted` false, no grants),
   asserting the calendar archive as read and the owner's vault record at
   the verified revision (4.5). An owner entry without the `$za$` marker
   (the untouched default calendar the server creates keyless, which release
   1 section 4.1 tolerates and section 7.3 passes through as plaintext) has
   nothing to open: its values are sealed straight into the shared form.
   From this commit on, every write into the calendar seals under the CK
   (5.5). A grant that finds the calendar already in the shared form reuses
   its ring.
2. **The DEK rewrap.** Every already-sealed event of the calendar that still carries a
   `0x01` envelope is rewritten with its DEK opened under the EWK and
   rewrapped under the newest generation as a `0x03` envelope. Nothing
   else changes: the sealed trees, bundles and token pairs stay as they
   are, because the DEK does not change. An event with no carriers at all
   (plaintext, which release 1 section 7.3 tolerates until its next write,
   `seal/event.rs` and `dav_seal::test_legacy`) is sealed under the CK in
   full, as a PUT would seal it, and counted separately as `sealed`.
   The phase scans before it rewraps (re-review finding 1): it first walks
   every event of the calendar checking its parentage (exactly one parent,
   this calendar), and only when the walk is clean does it start
   rewrapping. Rewrites commit in batches of 100 events (author's call),
   each event asserting its own stored archive; an event that loses its
   assertion is re-read (a concurrent writer has sealed it under the CK,
   5.5) and skipped when it already carries `0x03`. An opaque assertion
   loss on a phase-1, phase-2, `reverting`-ring or final unshared rewrite
   returns a typed conversion conflict to the HTTP coordinator, with
   committed counts and progress information; the coordinator reapplies
   fresh vault, account, calendar and request-budget guards before retry.
   The phase ends with a
   scan that finds no event of the calendar without a `0x03` envelope and
   no event with a second parent.
3. **The grant.** One batch: the `acls` entry, the grantee's entry with its
   ring holding every generation, the ledger record and `converted` set,
   asserting the calendar archive, the vault record, the pin record and the
   owner's registry account (4.5). It is published only after phase 2's
   complete, valid scan.

**The single-parent check is fenced by the conversion** (review finding
1). The check 4.3 makes before phase 1 is a fast refusal and nothing more:
it is not fenced, and an in-account `COPY` between it and phase 1 can add
a second parent to an event of the calendar (a `COPY` appends a name and
updates only the event, `crates/dav/src/calendar/copy_move.rs:507-523`),
which phase 1's assertions do not see. Once phase 1 has committed the
shared form no new link can form, since the single-parent rule refuses a
`COPY` of an event with a shared parent and a `MOVE` that would leave one
linked (5.6), so the window is the precheck's alone, and phase 2's walk
closes it. A linked event found by the walk aborts the conversion before
any rewrap, and the request begins reverting phase 1 in the owner's
session (re-review finding 1):

1. it rewrites the owner ring marked `reverting`, so that writes into the
   calendar seal under the EWK again while the ring still holds the CK
   (5.5);
2. it rewraps back to the EWK every event that a concurrent write sealed
   under the CK since phase 1 (the owner's session holds both keys; a
   concurrent write that read the ring before step 1 loses its calendar
   assertion and seals again under the EWK);
3. it rewrites the owner's entry in its unshared form, the ring and with
   it the key-domain id gone (the first two steps keep the id; the next
   conversion draws a new one),

each step asserting the calendar archive as read, and then answers the
precheck's 409, `calendar has linked events` once rollback completes
(author's call on the
`reverting` step: without it, an event sealed under the CK between phase 1
and the revert would be left unreadable once the ring went).
Ring-only changes to mark `reverting` assemble the same entry set with
rewritten headers and unchanged sealed bundles; they cannot go through a
bundle sealer that only retains unopened entries. A request that dies
between phase 1 and the end of the revert leaves the shared form with
`converted` unset and no grant, a state the owner reads in full (valid
no-carrier legacy plaintext, `0x01`, or `0x03` under its own ring), which the write rule of 5.5
keeps safe. A terminal guard, exhausted retry budget or store error may
also end a request in that safe `converted=false` or `reverting` journal
state. The next grant dispatches by state: without a ring it starts phase 1
with a fresh domain id; with `converted=true` it skips conversion; with
`converted=false` it resumes phase 2 and repeats the complete parentage
walk for this request before any rewrap; with `reverting` it finishes
rollback before beginning any new conversion. It does not start another
phase 1 while a ring remains. The owner
removes the link (deletes the other calendar's copy of the event, which
unlinks without resealing, 5.5) and grants again.

**Cost.** For a sealed event, a DEK rewrap, not a content reseal (a
plaintext event costs one seal, as its next write would have): per event,
one archive read,
two AEAD operations on 32 bytes and one archive rewrite through the index
builder (the stored archive's version, and so the event's ETag, changes,
and the change log records it; time ranges, alarms and the UID index do
not). The cost is linear in the calendar's events, and the work runs inside
the one grant request, in bounded batches, however large the calendar is;
no background job takes part (release 1 invariant 6). Every client of the
owner refetches the calendar's events at its next sync, once. Later grants
of the calendar skip phases 1 and 2, since `converted` is set.

**An interrupted first grant** (a timeout, a crash, a client that gave up
and dropped the connection) leaves the calendar in the shared form with no
grantee and some events still `0x01` or plaintext. That state is complete
for the owner, who holds both the EWK and the CK, and invisible to anyone
else, and 5.5's write rule keeps a linked event in it from being sealed
under the CK. The next
grant of the calendar finishes phase 2 before its own write; nothing else
needs it, because a grantee exists only after phase 3.

### 3.6 What sessions hold, and for how long

- **`SessionKeys`** (`crates/vault/src/session.rs`) gains a per-session
  cache of collection keys, keyed by (owner account, calendar document id,
  key-domain id, generation), each in a `Zeroizing` buffer with a silent
  `Debug`, filled at the time of use. The key-domain id is read with the
  calendar's ring on every use, so a key cached for an earlier conversion
  of the calendar never answers for the current one: a session whose
  cached domain id differs from the archive's reads the ring afresh
  (re-review finding 1, revision 6). The cache is filled as follows: the
  owner's session opens its ring under the EWK; a grantee's session opens
  its grant ring with its private key, unwrapped lazily through
  `Server::za_private_key`, which runs key pinning's self-check once per
  `SessionKeys` (its section 3.3). The cache's size is bounded by the
  calendar generations the account can open (author's call: no separate
  bound). Read-only operations may memoize the resolved calendar context
  by (owner, calendar) for one request, within 4.6's accepted in-flight
  read window; every write reads afresh and never uses that read memo.
- **Residency is the session keys' own** (release 1 section 5): the key
  cache's 15-minute idle timeout and 60-minute cap, the sweep, LRU
  eviction, zeroing when the last reference drops, and removal by any
  access-token invalidation of the account, which grants and revokes
  trigger for the grantee (4.6). A CK is never cached outside
  `SessionKeys`, never shared across nodes and never written to the shared
  lookup store.
- **The current generation is read, never cached**: every write reads the
  calendar archive and seals under the newest generation its ring lists
  (5.5). The cache only spares repeated unwraps.
- **The organizer's RSVP token pairs.** Every rewrite of an event carries
  the stored `X-ZA-EXTRA` token pairs into the new bundle (release 2
  section 3.3; `update.rs` and `proppatch.rs` read them with
  `event_token_pairs`), so an edit grantee's session that rewrites an event
  of the owner's opens the owner's pairs, the organizer's list of attendee
  tokens or the owner's own attendee token. They are held in that request's
  memory only and are never returned to a client. The server uses them as
  the owner's session would: a delegated REQUEST hands out tokens from the
  owner's event, and a delegated REPLY carries the owner's own token (6.2).
  No client can use them to forge a reply: items are written only by the
  fan-out, which builds a message in the owner's name only for a write
  into the owner's calendar that upstream's ACL and 5.2's resolver allowed
  (6.2).
- **The share endpoints** build their own request-scoped `SessionKeys`
  (4.1), dropped with the request.
- **IMAP and POP3 sessions** hold `SessionKeys` for the connection (mail
  section 3.3) and never open a CK; WebSocket, EventSource, ManageSieve and
  SMTP sessions hold none.

### 3.7 Stored format additions

| Where | Stored value | Protected by |
|---|---|---|
| `Calendar.preferences[owner].name` of a shared calendar | `$za$` + base64(owner ring) + `\|` + base64(bundle envelope) + `\|` + sealed personal `Bundle` + `\|` + base64(shared bundle envelope) + `\|` + sealed shared bundle | ring payload: owner's EWK; bundle DEKs: CK; bundles: their DEKs |
| `Calendar.preferences[grantee].name` | `$za$` + base64(grant ring) + `\|` + base64(bundle envelope) + `\|` + sealed personal bundle | ring records: `pk_wrap` to the recorded key; bundle DEK: CK; bundle: bundle DEK |
| `Calendar.preferences[*].time_zone` (custom) | release 1's sealed tree | the entry's bundle DEK |
| `Calendar.acls` | upstream's list: grantee account ids and the level's bits | plaintext (upstream) |
| Event `X-ZA-KEY` in a shared calendar | wrap type `0x03` (3.4) | CK generation |

## 4. Account API additions

### 4.1 Common to the three endpoints

Three endpoints join the twelve under `/api/vault/` (release 1 section
4.1, key pinning section 6): `shares`, `shares/grant` and `shares/revoke`
(author's call on the names). `handle_vault_request` in
`crates/http/src/api/vault.rs` routes (`shares`, none), (`shares`,
`grant`) and (`shares`, `revoke`), the shape the contacts endpoints use, to
a new module `crates/http/src/api/shares.rs`, which shares `vault.rs`'s
verification and response helpers. All are `POST` with JSON and follow
api.md's conventions exactly (CORS, `Cache-Control: no-store`, the
anonymous-request rate limit, the two error body shapes, failed-attempt
accounting); all are password-verified with api.md's shared fields
(`username`, `password`, `totp`) and status table, whose heading changes
from nine endpoints to twelve (and the rate-limit note's count from eleven
to fourteen).

- **Keys.** Verification (`za_verify_primary`) builds this request's own
  `SessionKeys` from the password, as the contacts endpoints do; it is not
  a key-cache entry, the key cache is never consulted, and nothing is
  cached. These per-request keys are the owner's session keys a grant
  needs: the EWK for the ring, MK for the pin record. A grant or revoke
  never needs the owner's private key; the list opens the caller's private
  key for the calendars shared with it.
- **Permission.** `shares/grant` and `shares/revoke` require the
  permission upstream's CalDAV `ACL` method requires, `DavCalAcl`
  (author's call), and answer 403 with the problem body without it, as
  `setup-token` answers a missing permission. `shares` requires none.
- **Inputs.** `calendar` is the path segment of one of the caller's own
  calendars, its URL slug as `shares` lists it (author's call: what a
  person reads in a client's URL and types with curl); it is a 400 when
  empty, longer than 255 bytes or containing `/`, and the scheduling inbox
  and outbox are not calendars. `address` is normalized by key pinning
  section 4.3's helper, and one that fails sanitization is a 400. `level`
  is `"view"` or `"edit"`. Every 400 on these fields is checked before the
  password is verified.
- **Bodies and responses** stay out of traces (release 1 section 4.1).

### 4.2 POST /api/vault/shares

Purpose: the page's sharing view: the caller's calendars with their
grants, and the calendars other key accounts share with the caller.
Request: the shared fields only. It writes nothing of its own; key
pinning's self-check, run when the private key is first opened, may repair
the caller's own stored public key (its section 3.3).

Success (200):

| Field | Type | Meaning |
|---|---|---|
| `calendars` | array | One object per calendar of the caller's own account, shared or not, sorted by `calendar`. |
| `shared_with_me` | array | One object per calendar of another key account whose `acls` grants the caller a level, sorted by `owner`, then `calendar`. |

Each object in `calendars`:

| Field | Type | Meaning |
|---|---|---|
| `calendar` | string | The calendar's slug (its path segment). |
| `name` | string | Its display name, as the owner's entry holds it. |
| `shared` | boolean | Whether the calendar is in the shared form; it stays `true` after the last revoke (3.3). |
| `grants` | array | One object per grant, sorted by `address`; empty when none. |

Each object in `grants`:

| Field | Type | Meaning |
|---|---|---|
| `grant` | integer | The grant's stable id: the grantee's account id, which `acls` and the ledger already key (review finding 3). `shares/revoke` takes it. |
| `address` | string | The address the grant was made to, from the owner's ledger (normalized). |
| `level` | string | `"view"` or `"edit"`. |
| `fingerprint` | string | The fingerprint (key pinning section 3.1) of the key the grant is wrapped to. |
| `granted_at` | integer | Unix seconds of the grant or of its last re-key. |
| `pin_source` | string or null | The source of the owner's current pin for `address` (key pinning section 4.1): `"first-use"` or `"confirmed"`; null when the address is no longer pinned. A page can suggest confirming a first-use pin on the contacts screen (4.3). |
| `state` | string | `"active"`; `"key changed"` when the grant's account now serves a key other than the recorded one (the person started over, or storage access replaced the served key; only an out-of-band comparison tells them apart, 7.4); `"gone"` when the account no longer exists or is no longer a key account (7.5). |

Each object in `shared_with_me`:

| Field | Type | Meaning |
|---|---|---|
| `owner` | string | The owner's account name (its address), as the directory gives it. Not authenticated (section 2, integrity). |
| `calendar` | string | The calendar's slug in the owner's home. |
| `href` | string | Its CalDAV URL path, `/dav/cal/<owner>/<calendar>/`, percent-encoded as the server writes it. |
| `name` | string or null | The caller's own name for the calendar (a copy of the owner's at the grant, until the caller renames it); null when the caller's grant ring does not open. |
| `level` | string | `"view"` or `"edit"`, from the calendar's `acls` bits for the caller. |

Calendars of plain accounts shared with the caller (which release 1 lets a
key account read) are not listed (author's call): they are upstream's
shares, and the page cannot manage them.

| Code | When | Body |
|---|---|---|
| 200 | Listed | the object above |
| 400, 401, 402, 409, 429 | As in the shared table | |

### 4.3 POST /api/vault/shares/grant

Purpose: share one of the caller's calendars with another key account at a
level, change the level of an existing grant, or re-key a grant whose
grantee's key changed. Request: the shared fields, `calendar`, `address`
and `level` (all strings, required).

The endpoint:

1. Resolves the calendar by slug in the caller's account, and the address
   as `contacts/lookup` resolves it (key pinning section 6.3): a local key
   account whose setup is complete, its served key read fresh through the
   public-key helper. It reads the pin record fresh (key pinning section
   4.2) and, when the record is absent or unusable, the wrap mark. The
   grantee must not be the caller. It requires a pin for the normalized
   address, of either source, whose key equals the served key: a first-use
   pin from an earlier invitation is enough, as a pin confirmed on the
   page is (decided 2026-10-22), and the listing shows the pin's source
   (`pin_source`, 4.2) so that the page can suggest confirming it.
2. Decides:
   - **A new grantee.** It refuses when the calendar already holds
     `maxShares` grants (`max_shares_per_item`, which upstream's JMAP path
     enforces and its `ACL` method does not) and, when the calendar is not
     yet converted, when any of its events also belongs to another calendar
     (5.6), a fast precheck that the conversion repeats under its fence
     (3.5, review finding 1). Then it converts the calendar (3.5, phases 1
     and 2) unless `converted` is set, and writes the grant (phase 3).
   - **An existing grantee, same level, recorded key equal to the pin**:
     nothing to do; 200 with `changed: false`.
   - **An existing grantee, another level, same key**: the level change
     (7.4).
   - **An existing grantee whose recorded key differs from the pin**: the
     re-key, at the requested level (7.4).
   - In the same write, every gone grantee of the calendar is swept (7.5).
   An existing grantee is found by the account id the address resolves to,
   not by the address: the ledger records the address given at the grant,
   and a grant to a second address of an existing grantee (an alias, a
   sub-address, each pinned separately as key pinning section 4.3 pins
   them) is that grantee's level change, re-key or `changed: false`, and
   leaves the ledger's address as recorded. A calendar holds at most one
   grant per grantee account id, which is also the grant's id (4.2).
3. Commits (4.5) and, after the commit, invalidates (4.6).

Success (200): `{"ok": true, "changed": <bool>, "rewrapped": <int>,
"sealed": <int>}`. `changed` is false only when nothing was written;
`rewrapped` counts the events whose DEK this call rewrapped and `sealed`
the plaintext events it sealed (3.5), both non-zero only on a conversion.

| Code | When | Body |
|---|---|---|
| 200 | Granted, changed, re-keyed, or already in place | the object above |
| 400 | Shared causes; `calendar` missing, empty, longer than 255 bytes or containing `/`; `address` missing or not a bare address; `level` missing or not `"view"` or `"edit"` | problem body |
| 401, 402, 429 | As in the shared table | |
| 403 | The account may not share calendars (`DavCalAcl`) | problem body |
| 409 | No calendar with that slug in the account | `{"error": "unknown calendar"}` |
| 409 | The address is not a local key account whose setup is complete (unknown addresses, plain accounts, groups and accounts pending setup get the same answer) | `{"error": "not a key account"}` |
| 409 | The address is the caller's own | `{"error": "cannot share with yourself"}` |
| 409 | No pin for the address | `{"error": "contact not pinned"}` |
| 409 | The served key's fingerprint differs from the pin's | `{"error": "contact key changed"}` |
| 409 | The account's pin record is missing under a pinned wrap (key pinning's `record_missing`) | `{"error": "pin record missing"}` |
| 409 | The calendar already holds `maxShares` grants | `{"error": "too many shares"}` |
| 409 | An event of the calendar also belongs to another calendar, found by the precheck or by the conversion's rewrap and scan (3.5) | `{"error": "calendar has linked events"}` |
| 409 | The caller's account was deleted from the registry while the request ran (4.5) | `{"error": "account deleted"}` |
| 409 | The write lost its eighth attempt (4.5) | `{"error": "conflict"}` |
| 409 | Other shared 409s | as in the shared table |

Notes: `not a key account` is not counted as a failed authentication. A
grant drops the grantee's resident keys (4.6); its clients re-authenticate
transparently at their next request. A first grant of a large calendar
takes as long as rewriting each event's key envelope once (3.5).

### 4.4 POST /api/vault/shares/revoke

Purpose: stop sharing a calendar with one grantee. Request: the shared
fields, `calendar` (string, required) and exactly one of `grant`
(integer, the grant id `shares` lists) and `address` (string).

Revoke works by grant id (review finding 3). Addresses and aliases are
mutable (an account's address list is ordinary registry data, and the
key-account update restriction refuses credential changes only,
`crates/jmap/src/registry/mapping/principal.rs:79-92`), so an alias can
move from one grantee to another, and a revoke that resolved the displayed
address through the directory would remove the wrong grant. With `grant`,
the endpoint revokes exactly that `acls` entry, its grant ring and its
ledger record. An `address` is a convenience (re-review finding 5): the
endpoint matches it against the ledger's recorded addresses (exact, after
key pinning section 4.3's normalization) and resolves it through the
directory. When only one of the two names a grant, or both name the same
one, that grant is revoked: an alternate alias of a grantee whose account
holds exactly one grant on the calendar revokes it whatever address the
ledger recorded, and a recorded address of a gone account revokes its
grant. When the ledger's exact match and the resolved account name
different grants (the reassigned-alias case), the request is refused with
the 409 below so that the page revokes by id. The endpoint removes the
grant and rotates (7.1, 7.2), sweeping gone grantees in the same write.

Success (200): `{"ok": true, "changed": <bool>}`; `changed` is false when
the calendar holds no such grant (a no-op, still 200, as `contacts/forget`
answers for an address without a pin).

| Code | When | Body |
|---|---|---|
| 200 | Revoked, or nothing to revoke | the object above |
| 400 | Shared causes; `calendar` missing or malformed (as for `shares/grant`); neither or both of `grant` and `address`; `grant` not an unsigned 32-bit integer; `address` not a bare address | problem body |
| 401, 402, 429 | As in the shared table | |
| 403 | The account may not share calendars (`DavCalAcl`) | problem body |
| 409 | No calendar with that slug in the account | `{"error": "unknown calendar"}` |
| 409 | The ledger's exact match for `address` and the account it resolves to name different grants (an alias reassigned from one grantee to another) | `{"error": "address is ambiguous, revoke by grant"}` |
| 409 | The caller's account was deleted from the registry while the request ran (4.5) | `{"error": "account deleted"}` |
| 409 | The write lost its eighth attempt (4.5) | `{"error": "conflict"}` |
| 409 | Other shared 409s | as in the shared table |

Notes: revoking is effective from the 200: the grantee's next request is
refused and its client drops the calendar at its next sync. What the
grantee already saw, it could already have copied (7.3).

### 4.5 The write, its assertions and its attempts

Every grant, level change, re-key, revoke and sweep is one batch in the
owner's account (the conversion's phases commit separately, 3.5), which
carries:

- the calendar archive, written through upstream's `update` with the
  stored archive as current, so the batch asserts the archive as read (the
  calendar's change id, its archive version) and the index builder updates
  the ACL index and writes the share notifications;
- the owner's vault record asserted at the bytes re-read after
  verification (as `za_reread_verified` reads it): release 1 section 4.1's
  rule that an endpoint write is conditional on the generation it
  verified, here an assertion without a write, so a password change,
  recovery or TOTP change that commits between verification and the write
  refuses it;
- for a grant and a re-key, the pin record asserted at the bytes read
  (`AssertValue::Hash`, key pinning section 4.2), so a concurrent
  `contacts/pin` or `contacts/forget` of the address makes the grant
  decide again;
- the owner's registry Account item asserted present (`AssertValue::Some`
  on `ValueClass::Registry(RegistryClass::Item { object_id:
  ObjectType::Account, item_id: owner })`), the barrier against the
  owner's destruction (review finding 5). The registry and the data store
  are one `Store` (`crates/store/src/registry/bootstrap.rs:55`,
  `crates/common/src/config/storage.rs:46`; the registry writer commits
  through it, `crates/store/src/registry/write.rs:218-304`), so one batch
  can assert it. The registry's destroy deletes that item before it scans
  the owner's grants (`crates/jmap/src/registry/set.rs:689-712`, 8.6): a
  share batch that commits before the delete is seen by the scan, and one
  that commits after loses its assertion. On PostgreSQL and MySQL an
  assertion with no later set of its key is a `SELECT ... FOR UPDATE`
  compare inside the transaction
  (`crates/store/src/backend/postgres/write.rs:362-387`), which serializes
  against the delete (moot for the product build, which stores on RocksDB,
  but stated for completeness).

The calendar, vault and pin keys live in the owner's account and the
Account item in the same store, so one batch carries them all, each
assertion before its key's set, as PostgreSQL and MySQL require. A lost
calendar or pin-record assertion re-reads everything and decides again
from step 1, up to eight attempts in all, key pinning section 4.2's count;
the eighth loss is 409 `conflict`. A lost vault assertion is 409 `vault
changed since verification` at once, and a lost Account assertion is 409
`account deleted` at once (the caller's account is gone; nothing is
decided again). The grant writes nothing in the
grantee's account beyond what upstream's index builder writes for any
grant (the ACL index key and the share notification under the grantee's
id).

The sync boundary (5.2; re-review finding 1, revision 7) adds one write
to a grant and one duty to the others. A grant's batch creates the grant
ring with its boundary-pending flag set and its grant change id zero;
right after the commit, the request records the grant's own change id
(the one the commit's result reports) in a completion write of its own,
which asserts the calendar archive as the grant left it and, on a lost
assertion, re-reads the archive and retries. A level change, re-key,
revoke or sweep whose batch finds a flag set in the archive it read
completes the boundary in that same batch, with the owner account's last
change id read after that archive read.

### 4.6 Caches after the commit

After the commit, and only then, the endpoint invalidates the cached
access token of every grantee whose entry it wrote (added, changed,
re-keyed, revoked or swept) and, after a rotation, of every remaining
grantee. It issues those `CacheInvalidation::AccessToken` entries itself,
through its own `CacheInvalidationBuilder` and `invalidate_caches`, rather
than through upstream's `refresh_acls(new, old)`
(`crates/common/src/sharing/acl.rs:11-50`), which invalidates an existing
principal only when its bits differ (`:22-23`, `:36-37`) and so would
miss a same-level re-key, whose rings and ledger change while its bits do
not (review finding 6). Upstream's own ACL writers invalidate first and
commit after
(the DAV `ACL` method in `crates/dav/src/common/acl.rs`, JMAP `Calendar/set`
in `crates/jmap/src/calendar/set.rs`), so a grantee request between the two
can rebuild its token from the old grants and cache it with no later
invalidation; the endpoints do not inherit that gap.

That invalidation is `CacheInvalidation::AccessToken`, applied locally and
by the cluster broadcast (`invalidate_local_caches` in
`crates/common/src/cache/invalidate.rs`): it bumps the account epoch, drops
the cached token and the account's `http_auth` entries, and removes the
account's key-cache entries (`za_keys.remove_account`). A grant, level
change, re-key, revoke or sweep therefore drops the grantee's resident
session keys, and the collection keys they cache, on every node the
broadcast reaches; the grantee's next request re-verifies with its keyed
credential (one Argon2 run) and builds a token with the new grants. The
owner's tokens and keys are untouched: the owner's sessions read the new
ring from the archive at their next use (3.6).

A cluster therefore needs pubsub for grants and revokes to reach every
node: upstream broadcasts cache invalidations only when pubsub is
configured (`crates/common/src/manager/boot.rs`), and even then best effort
(a send failure is only traced); the token cache has no time limit. A node
that missed a **grant** keeps the grantee's old token, which lacks the
owner in `access_to`, so the URI gate refuses the owner's calendar there
and the calendar is absent for the grantee until that node rebuilds its
token. A node that missed a **revoke** keeps the grantee's old token and
keys, and the use-time checks still refuse the grantee there (5.2):
container ACLs come from the resource cache, refreshed from the change log
on every fetch, and the grant ring is read from the stored archive; but
the collection keys resident in its session keys linger until the key
cache drops them, at most the 60-minute cap (release 1 section 5). A
revoked grantee's request already in flight finishes on its own copy of the
keys; a write it attempts, a `PUT` or a `DELETE`, loses its calendar
assertion and is refused (5.5). A node that missed the invalidation an
owner's destruction sends to each grantee (8.6) keeps the grantee's stale
token, which lists the dead
owner in `access_to` until it is evicted, and the grantee's discovery
fails there with 500: `build_home_set` calls `server.account` for the dead
owner (`crates/dav/src/principal/propfind.rs`), the assisted-discovery
block loads its resources (`data.resources`, through `server.account`
again), and a depth-1 `PROPFIND /dav/cal/` fails likewise.

### 4.7 Traces

Traces use existing events with `zero-access:` details and carry the
owner's account id, the calendar's document id, the grantee's account id
(already visible in the calendar's `acls`), the level and counts
(rewrapped and sealed events, swept grantees); never an address, a
fingerprint, key bytes or a calendar name (key pinning invariant 21).

### 4.8 Flows and what the page shows (for api.md)

- **Share a calendar.** `shares` lists the caller's calendars. To share one
  with someone: `contacts/lookup`, compare the fingerprint out of band and
  `contacts/pin` (key pinning section 6.6) unless the person is already
  pinned, then `shares/grant` with the level. `contact not pinned` and
  `contact key changed` send the user to that comparison; `pin record
  missing` to key pinning's `record_missing` flow.
- **Change the level.** `shares/grant` with the new level.
- **Stop sharing.** `shares/revoke` with the grant's id from the listing
  (`grant`, 4.2), never only the displayed address, which can have moved
  to another account (4.4). The page says: "Removing someone
  stops them seeing anything new. What they could already see, they could
  already have copied."
- **A grant shown `key changed`.** Compare the person's fingerprint out of
  band. If it is new and they confirm it, `contacts/pin` with it, then
  `shares/grant` again (the re-key); if it is the recorded one, someone
  replaced the key the server serves, and the grant is safe as it is.
- **A grant shown `gone`.** `shares/revoke`; any grant or revoke on the
  calendar also removes it.
- **A grant with `pin_source` `"first-use"`.** The grant stands; the page
  may suggest comparing the person's fingerprint and confirming the pin
  (`contacts/lookup`, then `contacts/pin`).
- **A `3.8` on an attendee of the owner's event that a grantee's delegated
  message refused** (6.2): it was decided under the grantee's pins, so the
  owner's `contacts` listing shows nothing for it and no endpoint of the
  owner's redelivers it; the page has no remedy to offer. Only an edit of
  the event that builds a message to that attendee sends again.

The page shows, for each of the caller's calendars, its name, whether it
is shared, and each grant with its address, level, state, pin source and
recorded fingerprint; and, for each calendar shared with the caller, its
owner, name and level. It explains that Apple devices show shared
calendars by themselves and that Thunderbird and DAVx5 find them at their
next discovery, offering `href` for a client set up by hand; that the
first share of a calendar makes the owner's devices resync it once; that
an edit grantee's invitations, updates, cancellations and replies from the
shared calendar go out in the owner's name, marked as sent by the grantee,
as Stalwart sends them for any grantee (6.2); and that a person invited to
an event of a calendar shared with them answers from their own copy of the
invitation, not from the shared calendar (6.3). api.md's "What the page
never sees" is unchanged: no key material is returned, and fingerprints
and addresses go only to the record's owner.

## 5. Access and keys at use time

### 5.1 The gate

`validate_uri` (`crates/dav/src/common/uri.rs`) calls `za_session_keys`
for calendar and scheduling URIs today, and `za_session_keys`
(`crates/dav/src/common/za.rs`) answers 403 with the trace `zero-access:
calendar access without session keys` unless the token holds that
account's keys (`AccessToken::za_keys_for`, which yields keys only for the
token's own account). Release 4 keeps that rule for scheduling URIs (the
inbox and outbox under `/dav/itip/<account>/` stay the owner's alone, as
upstream keeps them member-only) and relaxes it for calendar URIs only:
the gate passes a key account's calendar URI when the token holds that
account's keys (the owner), or when the token

- holds session keys of its own account (`access_token.session_keys()`
  whose account is `access_token.account_id()`), so it is a key account's
  own password or app-password request, not a Bearer, OAuth, API-key,
  WebSocket or background token;
- is not an impersonation copy (`za_is_impersonated`, mail ruling R35);
- does not carry the `Impersonate` permission
  (`has_permission(Permission::Impersonate)`); and
- lists the owner for the `Calendar` collection in its `access_to`, which
  upstream builds from the ACL index
  (`crates/common/src/auth/access_token.rs`).

The owner arm is tried first, so an administrator's key account still
reaches its own calendars. The `Impersonate` refusal (review finding 4) is
invariant 40 and release 1's rule read literally: `za_is_impersonated` is
R35's flag on a token copy obtained by impersonation
(`access_token.rs:482-491`), not the permission, and a token that carries
the permission is `is_member` of every account (`access_token.rs:557-561`),
which upstream's handlers trust ahead of any container check. Through it
such a token would list every path of the owner with `PROPFIND propname`,
which answers before the loader's view (`crates/dav/src/common/
propfind.rs:1263-1276`, `:424` against `:481`); lock any calendar or event
of the owner, since LOCK and UNLOCK check only `is_member` after
`validate_uri` (`crates/dav/src/common/lock.rs:116-118`, `:865-878`); and
rename, move or copy the owner's collections, which need only
`is_member` of the destination account (`crates/dav/src/calendar/
copy_move.rs:378`, `:383-395`, `:434-447`, `rename_container` at
`:1077-1102`). For a non-owner token on a user account `is_member` is
true only through `Impersonate` (`member_of` holds group ids), so refusing
that permission at the gate closes all three. It costs nothing
product-visible: a product key account never holds an administrator role.

Otherwise it answers release 1's 403 and trace. The gate decides only that
the request may proceed into the owner's calendars; which keys it holds
there is decided per calendar, at use (5.2). `za_keys_for` is unchanged:
sharing never yields another account's account keys.

### 5.2 Resolving keys per calendar

A per-calendar resolver replaces `za_session_keys` wherever calendar data
is opened or sealed: the handlers' re-checks (`get.rs`, `update.rs`,
`delete.rs`, `proppatch.rs` and `copy_move.rs` in
`crates/dav/src/calendar/`; `mkcol.rs` keeps its existing `za_session_keys`
check, so a grantee meets the member check's 403 there), the PROPFIND
and REPORT loader's view (`za_archive_view` in
`crates/dav/src/common/za.rs`), the owner's arm of
free-busy (`ZaFreeBusy::Unsealed`, whose events may now carry `0x03`
envelopes), the drain, and `contacts/pin`'s reseal. Given the token, the
owner account, a calendar's stored archive and the bits the operation
needs, it yields:

- **For the owner** (the token holds the owner's keys): the EWK for an
  unshared calendar; for a shared one, the CKs from the owner ring, opened
  under the EWK.
- **For a grantee**: the CKs from the grantee's own grant ring, opened
  with its private key, only when the calendar is in the shared form, its
  `acls` holds an entry for the token's own account id whose bits include
  those the operation needs (`Read` to see the collection, `ReadItems` to
  read events, and the write bits upstream's handlers require: `AddItems`,
  `ModifyItems`, `RemoveItems` or `Modify`), and a grant ring for that
  account exists.
- **Otherwise** a refusal, with `SecurityEvent::Unauthorized` and the
  detail `zero-access: calendar access without a collection key`. A
  request addressed to a calendar the resolver refuses answers 403. Inside
  a listing (a `PROPFIND` of the owner's home, the assisted-discovery
  listing of the grantee's own home, a depth-1 `PROPFIND /dav/cal/`), a
  refused calendar and its events are skipped instead, so the listing
  shows only what the token's own grant covers and the request succeeds.

**Foreign calendar eligibility** (re-review finding 2). The gate of 5.1
sees only a request that addresses the owner's URI. A grantee's
`PROPFIND /dav/cal/<own>/` passes the owner arm, and the
assisted-discovery block then loads the owner's resources through
`data.resources` with no gate and appends their paths under `is_member`
(`crates/dav/src/common/propfind.rs:1500`, `:1705`), and a `propname`
request emits those paths before the loader's view (`:370`, `:424`);
`shared_containers` (`crates/common/src/sharing/resources.rs:13`) accepts
a grant through group membership. So one check decides whether a calendar
of another key account may appear in anything a session emits: the
calendar is eligible only when the token carries no `Impersonate`, and the
calendar's `acls` holds the token's own account id with `Read`; group
membership and `is_member` never count. It runs before the assisted block
appends a foreign path, before a `propname` response lists one, in the
resolver's grantee arm (so a token carrying `Impersonate` opens no foreign
calendar there either), and inside the vanished filter below. An
ineligible calendar is omitted from an otherwise valid listing of the
grantee's own home, never an error, and the direct-URI gate of 5.1 stays as
the second line. Calendars of plain accounts keep upstream's listing
(release 1 invariant 9; a key account may read them, release 1 section
9) (author's call on that scope).

The resolver checks the grant's bits itself, against the `acls` entry for
the token's own account id, and never honours `is_member` (author's call).
Upstream's handlers skip their container checks for `is_member`, which is
true for group members and for any token with the `Impersonate`
permission; on a key account's calendar that shortcut never stands in for
a grant. The gate already refuses a token with `Impersonate` (5.1), so the
resolver's own-id rule is defence in depth there, and the rule that
decides for a token whose access comes from a group's grant (one planted in
the store, since the endpoints never make one), which gets nothing and
lists nothing. The own-id check also
closes upstream's first-member-grant quirk: `has_access_to_container`
(`crates/common/src/sharing/resources.rs`) answers with the bits of the
first `acls` entry the token is a member of, where the resolver reads only
the entry of the token's own account. `MKCOL` and `MKCALENDAR` in the
owner's home need no resolver: upstream's member check refuses them before
any calendar exists to resolve (`crates/dav/src/calendar/mkcol.rs`).

Reads choose the key from the stored object: an event's envelope type
(`0x01` needs the owner's EWK, so a grantee meeting one fails that item as
an unseal failure, release 1 section 10; `0x03` names the calendar, which
must be the event's parent, and the generation, which must be listed); a
bundle's envelope likewise. A calendar's view for the owner unseals the
owner's personal bundle and the shared bundle; for a grantee, its own
personal bundle and the shared bundle, never the owner's personal bundle.
Other grantees' bundles stay sealed in either view, and no grantee's view
ever opens the owner's ring. A grantee whose
private key does not open (key pinning's "neither" mark) fails the shared
calendar's items as unseal failures (500), with release 1's logged error.

**Vanished paths in `sync-collection`** (review finding 4). One listing
path answers before any archive is loaded, so neither the loader's view nor
the resolver sees it: for a non-member token a `REPORT sync-collection`
fetches the vanished list account-wide (`server.store().vanished(account_id,
..)` in `get`, `crates/dav/src/common/propfind.rs:1374`; `Store::vanished`
is keyed by account and collection only,
`crates/store/src/query/log.rs:136-175`) and writes each entry as a 404
(`:1386`, `:1421`). A view grantee of one calendar would receive the URLs
of events deleted, moved or renamed in the owner's other calendars, and of
the owner's deleted or renamed calendars, `/dav/cal/<owner>/<slug>/<file>`:
client-chosen names release 1 lists as visible to the operator, not to
another user. A fork branch in `get` right after the vanished fetch, for a
non-member token on a key account, keeps only the vanished paths under the
current path of an eligible calendar (above), the prefix set built from
the calendars that pass the eligibility check rather than from
`shared_containers` (which accepts a grant through a group) and before
any intersection with the change log (the intersected
`display_containers` would drop a granted calendar that lost an event but
saw no container change). It also respects the requested collection: a
`REPORT` on one calendar returns only that calendar's tombstones, and one
on the home only the eligible calendars'. A granted calendar that was
itself deleted or renamed has no current path, so its own 404 is dropped,
and the client rediscovers from the home listing (5.3).

**The grantee's sync token is bound to its grant** (re-review finding 3).
Vanished paths carry no calendar identity (`Store::vanished`), a slug can
be reused once its calendar is gone (`crates/dav/src/calendar/mkcol.rs:63`
refuses a slug only while its path exists), and a sync position is a bare
change id (`DavQuery::changes`, `crates/dav/src/common/mod.rs:253`;
`Urn::parse`, `crates/dav/src/common/uri.rs:197`). A grantee syncing from a
token older than its grant could therefore receive the tombstones of a
deleted private calendar that once had the granted calendar's slug. So each
grant records the change id the store assigned to its own commit, kept in
the grant ring's visible header, where the grantee's session reads it, and
in the ledger (author's call on both: the ledger is sealed under the
owner's EWK, which a grantee's session never opens). The id is read back
after the commit (re-review finding 1, revision 7), never read before it:
the grant's commit creates the grant ring with its boundary-pending flag
set and its grant change id zero, and the same request then records the
commit's own id by a completion (below), a lost assertion retrying it.
That suffices for a grant, with no first step of its own: before the
grant's commit the grantee has no access at all, so its ring is born
pending and no earlier state needs fencing. A re-key or a level change
keeps the recorded id; a revoke and a new grant start a new one.

**The sync boundary** (re-review finding 2, revision 6). A grant's change
id alone does not cover a calendar that is already granted and is then
renamed into the former slug of a deleted private calendar: a token taken
between the grant and the rename is newer than the grant, and the vanished
fetch reads the account's retained paths since the token, the private
calendar's tombstones under that slug among them (`rename_container`,
`crates/dav/src/calendar/copy_move.rs:1099`, changes only the name). So the
sync boundary of a shared calendar for a grantee is the greater of that
grantee's grant change id and the change id of the calendar's last slug
change. A rename of a shared calendar (the owner's `MOVE` to another slug,
5.6) records the change id of its own commit as the rename change id in
the owner ring's header and in every grant ring's header, in the two steps
below (the owner's session reseals the owner ring's payload at each, since
its associated data binds the header). A grantee's `sync-collection` whose
token is at or below its boundary is refused with 403 and the
`DAV:valid-sync-token` precondition (RFC 6578 section 3.2), so the client
runs a full sync; a later token is served with the vanished filter above.
For an incremental sync, the calendar's current path and its boundary are
read together, from the archive read after the vanished fetch, never a
path retained in the resource cache combined with a newer ring
(re-review clarifications, revision 8).

**The boundary is recorded after its commit, in two fenced steps**
(re-review finding 1, revision 7). A change id read before the rename's
commit does not stand for it: the rename's assertion fences only the renamed
calendar's archive (`crates/common/src/storage/index.rs:349-351`),
`get_last_change_id` is an independent log scan
(`crates/store/src/query/log.rs:192`), and the store assigns the commit's
change id inside its transaction
(`crates/store/src/backend/rocksdb/write.rs:174-189`). Another session can
therefore create a private calendar at the vacant destination, create and
delete an event in it and delete it between that read and the commit; a
token above the recorded id but below that history would pass the boundary
and admit the private tombstone under the new prefix. So a rename of a
shared calendar is two fenced writes:

- **Step 1**, the rename itself (the archive with the new slug, in the same
  transaction as today, `rename_container`), also sets the boundary-pending
  flag in the owner ring's header and in every grant ring's header. This
  ring-only rewrite assembles the same entry set with changed headers and
  unchanged sealed bundles; a bundle sealer that retains only unopened
  parts cannot perform it. While a
  grant ring's flag is set, every incremental `sync-collection` of that
  grantee (`SyncType::From`, whatever its token) on the calendar, and on a
  home whose eligible calendars include it, is refused with 403
  `DAV:valid-sync-token`, and an initial sync (`SyncType::Initial`, which
  an empty or absent token selects, `crates/dav/src/common/mod.rs:261`) is
  served: its branch fetches no vanished paths
  (`crates/dav/src/common/propfind.rs:1432`), so it is always safe, and it
  is the full sync this section promises (re-review clarifications,
  revision 8).
- **Step 2**, run by the same request right after step 1 commits, takes the
  rename's own change id from the commit's result
  (`AssignedIds::last_change_id`, `crates/store/src/write/mod.rs:430`,
  which `commit_batch` returns,
  `crates/common/src/storage/transaction.rs:21`: the id the store assigned
  inside step 1's transaction, the one the calendar's log entry carries)
  and completes the boundary with it.

A **completion** with a change id `X` writes, in every ring whose flag is
set, `X` for a grant change id that is still zero (and into that grant's
ledger record), and, while the owner ring's flag is set (a rename is
pending), `X` for the rename change id of the owner ring and of every grant
ring; it clears every flag and asserts the calendar archive as it read it.
`X` is the request's own commit id when the completion is the step 2 of a
rename or of a grant and its assertion holds (the archive as that commit
left it), and otherwise the owner account's last change id read after the
archive read that saw the flags. Either is at or above the commit of every
write that set a flag it clears, which is all the boundary needs: a boundary
at or above the rename's own commit refuses every token from before the
rename, the destination's earlier history included, and a higher one costs
only a full sync. A
step 2 whose assertion is lost re-reads the archive and retries as a
completion of the second kind. A request that dies between the steps leaves
the flags set, so the grantees get full syncs and nothing is exposed until a
later owner write on the calendar completes the boundary: the next rename's
or grant's step 2, a level change, re-key or revoke (each completes in its
own batch, 4.5), or the repair the owner's session runs as a write of its
own when its resolver next reads the calendar with a flag set. So no
boundary is ever derived from a value read before the commit it bounds: a
step 2 uses the id the store assigned to that commit, and every other
completion a change id read after it.

### 5.3 Method by method

For a grantee of a key account's calendar (the owner's requests are
unchanged apart from the key domain, 5.5):

| Request by a grantee | view | edit |
|---|---|---|
| `PROPFIND` of the owner's home `/dav/cal/<owner>/` | lists the calendars granted to it, as upstream lists `ReadItems` calendars in another account's home, with its own names; calendars the resolver refuses are skipped (5.2) | same |
| `PROPFIND` of a granted calendar or of its events | served, unsealed with the CK | same |
| `GET`, `HEAD` of an event | served | served |
| `REPORT` `calendar-query`, `calendar-multiget` | served; candidates unsealed before the content filters, as release 1 section 8.2 runs them | same |
| `REPORT` `sync-collection` | served from a token newer than the sync boundary, the later of the grant and the calendar's last rename (an older one, and every incremental sync while the boundary is pending, is refused with 403 `valid-sync-token`, so the client runs an initial sync, which is served); vanished entries limited to the requested collection and to paths under the current path of an eligible calendar, so a granted calendar that was deleted or renamed gets no 404 of its own and the client rediscovers it from the home listing (5.2) | same |
| `REPORT` `free-busy-query` on a granted calendar | no busy periods (below) | same |
| `PUT` creating an event | 403 (`AddItems`) | stored sealed under the CK, under the owner's UID lease (6.4); schedules as upstream, messages in the owner's name delegated with `SENT-BY` (6.2) |
| `PUT` over an event | 403 (`ModifyItems`) | stored sealed under the newest generation; schedules as upstream, delegated with `SENT-BY` |
| `DELETE` of an event | 403 (`RemoveItems`) | deleted; schedules as upstream, delegated with `SENT-BY` (6.2) |
| `PROPPATCH` of an event | 403 (`ModifyItems`) | stored sealed (it changes no scheduling property) |
| `PROPPATCH` of the calendar | 403 (`Modify`), the colour and order included | writes the grantee's own personal bundle (name, description, time zone) through upstream's `personal_id`, and the calendar's dead properties (colour and order among them, shared by every principal) into the shared bundle (5.5) |
| `MKCOL`, `MKCALENDAR` in the owner's home | 403 (upstream's member check, before any calendar exists to resolve) | 403 |
| `DELETE` of the calendar | 403 (`Delete` and `RemoveItems` needed) | 403 (`Delete` is never granted) |
| `ACL` | 403 | 403 |
| `LOCK`, `UNLOCK` | upstream's 403 (`crates/dav/src/common/lock.rs:116`: only members lock) | 403 |
| `COPY`, `MOVE` | 5.6 | 5.6 |
| The owner's scheduling inbox and outbox | 403 | 403 |
| A calendar of the owner not granted to it | 403 | 403 |
| JMAP | refused (5.7) | refused |

**Free-busy.** A `free-busy-query` on a shared calendar by a grantee goes
through `za_freebusy_access`, which answers `ZaFreeBusy::Withheld` (no
busy periods) for any token without the owner's keys, unchanged. That is
upstream's own answer to a grant without `SchedulingReadFreeBusy`: both the
REPORT and the outbox POST select calendars with
`shared_items([ReadItems, SchedulingReadFreeBusy], match_any = false)`
(`crates/dav/src/calendar/freebusy.rs`), and no release 4 grant carries
that bit. Release 2 section 7 had placed free-busy between users in this
release; this spec follows the decision to defer free/busy grants, so that
gate does not reopen. A grantee reads every event of the calendar anyway.
The outbox POST (3.7 for a key target, `crates/dav/src/calendar/
scheduling.rs`) and JMAP `Principal/getAvailability` (which skips key
accounts) are unchanged.

### 5.4 Discovery

`calendar-home-set` (`build_home_set` in
`crates/dav/src/principal/propfind.rs`) works as upstream's:

- With assisted discovery on (the `WebDav` object's default,
  `enableAssistedDiscovery: true` in its generated `Default`), it lists the
  own home only, and a depth-1 `PROPFIND` of the own home also lists other
  accounts' granted calendars under their owners' URLs (the
  assisted-discovery block of `get` in `crates/dav/src/common/propfind.rs`,
  whose comment calls it invalid but needed for clients that read one home
  set). Each listed calendar goes through the loader's view with its own
  account, so the resolver opens the grantee's ring and the listing shows
  the grantee's names.
- With assisted discovery off, it lists the account's own home and one
  href per account in `access_to`, so multi-home clients (Thunderbird,
  DAVx5) find the owner's home and, in it, the granted calendars.

Apple Calendar reads only the first `calendar-home-set` href, so a
deployment with Apple clients keeps `enableAssistedDiscovery` on, as it is
by default (an operator who turned it off turns it back on); the
real-device check (11.5) verifies an Apple device syncing a shared
calendar through the fork's gate. Thunderbird and other multi-home clients
work either way. Discovery needs the foreign calendar eligibility check of
5.2 (re-review finding 2): the assisted block and a `propname` response
list a foreign calendar only when it is eligible, and omit it otherwise.

### 5.5 Writes: the key domain and the calendar assertion

Every write of a key account's event, by the owner or by a grantee, reads
the archive of the calendar the event belongs to (both calendars for a
MOVE), seals under that calendar's key domain (the newest generation of a
shared calendar's CK, otherwise the owner's EWK; the EWK again while a
conversion is `reverting`, 3.5) and asserts that calendar archive as read
in the event's batch (author's call). This covers `PUT`, `PROPPATCH` of an
event, `COPY` and `MOVE`, the drain's import and merge (release 2 section
6.4), `contacts/pin`'s reseal (key pinning section 6.4), and an event
`DELETE`, which seals nothing but asserts the calendar too (below).

**The write rule for linked events** (re-review finding 1). Any write that
would seal or rewrap an event whose names include a shared calendar and at
least one other calendar is refused with 409, traced as `zero-access:
event linked into a shared calendar` (the single-parent refusal; CalDAV
defines no precondition element for it, so the body is upstream's plain
conflict): a `PUT` or `PROPPATCH` of the event through any of its paths,
the drain's merge into it (the recipient's default calendar being shared
and the event linked; the item stays pending as an item error, release 2
section 6.5, until the link is gone), a reseal by `contacts/pin` (the entry
counts `failed`), and the conversion's rewrap. It is the single-parent
invariant enforced at write time, so it also covers the window 3.5's
abort and revert leave. Recovery is unlink-only: a `DELETE` through the
other calendar's path removes that name without resealing
(`DestroyArchive::delete_za`, `crates/groupware/src/calendar/
storage.rs:491`), after which the event has one parent and is written as
any other.

- **What it closes.** A write that read the calendar before a first
  grant's conversion committed would otherwise store a `0x01` event in a
  shared calendar its grantees cannot read; a write that read the ring
  before a revoke committed would otherwise seal under a generation the
  removed grantee holds. With the assertion such a write loses, and the
  conversion (3.5) and the revoke's guarantee (7.2) hold without
  exceptions.
- **On `AssertValueFailed`** in an event write the request cannot tell
  which assertion lost, because the store reports one failure without
  naming the key; the batch also asserts the event's own stored version
  (an update, through the index builder) or the UID lease fence (a
  create). So it re-reads the calendar archive. If the calendar's version
  changed, the calendar assertion is the one that lost: the request rewraps
  the event's DEK under the newest generation of the domain now in force
  and commits again, once, and the sealed content, the token pairs and any
  items the fan-out already delivered stand, because only the key envelope
  depends on the calendar; a second loss is upstream's answer below. If it
  did not change, the loss is the other assertion and takes its existing
  branch: in an update batch the event's own version, upstream's 409 for a
  concurrent `PUT` (`crates/dav/src/request.rs`), and in a create batch the
  UID lease fence, `za_lost_create_fence`'s existing branch
  (`crates/dav/src/calendar/mod.rs`, release 2 section 5.4). For the drain
  a loss stays a transient item error (release 2 section 6.5). A writer
  that no longer holds a key for the calendar after the re-read (a revoked
  grantee) gets 403.
- **Cost.** One more archive read per key-account event write (the
  resource cache holds slugs and `acls`, not rings), and a retry only when
  a calendar write lands in between, which is rare: a calendar archive
  changes on `PROPPATCH` of the calendar and through the share endpoints,
  not on event writes.

**The event `DELETE`'s calendar fence** (review finding 7). The event
`DELETE` checks access once, from the resource cache
(`crates/dav/src/calendar/delete.rs:192-195`), unseals the event and fans
out its CANCEL before the commit (`:255-272`), and commits a batch whose
only assertion is the event's own archive (`delete_all_za`,
`crates/groupware/src/calendar/storage.rs:617-623`). A revoke rewrites the
calendar, not the event, so without a fence a `DELETE` paused after its
check and resumed after a revoke would delete the event, deliver its
CANCELs and answer 204, against 4.4's "effective from the 200" and 4.6's
in-flight rule. So the event `DELETE`'s batch asserts the calendar's
archive as read, placed before `delete_za` (`delete.rs:241`) with
`with_account_id(owner).with_collection(Calendar).with_document(calendar_id)
.assert_value(..)`. `delete_all_za` sets its own context
(`storage.rs:567-570`) and `delete_za`'s `commit_point()` (`:526`) keeps
the assertion in the same chunk, and an assertion with no later set of its
key is valid on every backend. On a lost assertion the request re-reads
the calendar: if its version changed and the writer still holds
`RemoveItems` (a calendar `PROPPATCH`, or a grant to someone else, landed
in between), it commits once more; if the writer no longer holds a key for
the calendar, it answers 403; if the version did not change, the event's
own assertion lost, and the answer is upstream's 409. The fan-out boundary
is the `PUT`'s: CANCELs delivered before a refused commit stand, as 5.4 of
release 2 accepts for a lost commit, limited by the drain's SEQUENCE
check.

**Calendar writes.** A `PROPPATCH` of the calendar reseals the writer's
own personal bundle when it changes the name, description or time zone,
and the shared bundle when it changes a dead property (Apple's colour and
order among them, so an edit grantee's colour change recolours the calendar
for every principal, as upstream). An edit grantee's write rewrites the
owner's entry's fourth and fifth parts only, keeping the owner's ring and
personal bundle bytes as stored; it never opens either. Every such write
uses the newest generation and upstream's archive assertion; a calendar
write that loses that assertion has no recommit and answers upstream's 409
(`crates/dav/src/request.rs:712-714`). `MKCALENDAR` creates an unshared
calendar (release 1).

**Single-owner assumptions relax** for shared calendars only:
`seal_calendar` accepts the shared form and `owner_index` resolves a
principal's entry, while `Calendar::insert`'s default entry and the
provisioning check (`za_has_user_calendars`, 8.1) keep their release 1
meaning.

### 5.6 COPY and MOVE

Across accounts, `COPY` and `MOVE` stay refused with 403 when either side
is a key account (release 1 section 8.1), so a grantee cannot move an
event between its own calendar and the owner's.

Within the owner's account, upstream gives one event document several
parents: an event `COPY` adds a name to the same document (`copy_event` in
`crates/dav/src/calendar/copy_move.rs`), and a collection `COPY` adds one
to each of its events (`copy_container`). An event wrapped under one
calendar's CK and also listed in another calendar would leave the second
calendar's readers with the wrong key, or none. Release 4 therefore keeps
the **single-parent rule** (decided 2026-10-22, as revision 1 proposed it):
an event of a shared calendar has that calendar as its only parent. The
real-device check (11.5) records what Apple Calendar sends when an event is
dragged between calendars.

- An event `COPY` whose source or destination calendar is shared answers
  403, with the trace `zero-access: copy into or out of a shared calendar`,
  and so does a `COPY` of an event with a shared parent through any of its
  paths, an unshared calendar's included (re-review finding 1).
- A collection `COPY` or `MOVE` whose source or destination is a shared
  calendar answers 403 with the same trace, except a `MOVE` to a new name
  in the owner's home, which upstream implements as a rename that keeps the
  document id (`rename_container`). The wraps bind the document id, so the
  rename needs no rewrap, but it is no longer a bare name change: in the
  same write it sets the boundary-pending flag in the owner ring's header
  and every grant ring's header, and a second write right after its commit
  records the change id the store assigned to that commit as the rename
  change id there and clears the flags, advancing the grantees' sync
  boundary (5.2; re-review finding 2, revision 6; re-review finding 1,
  revision 7). A grantee never copies
  or moves a collection: upstream requires membership of the destination
  account, and the gate refuses the one grantee token that would be a
  member, one carrying `Impersonate` (5.1).
- An event `MOVE` between calendars of different key domains (an unshared
  and a shared calendar, or two shared calendars) rewraps an already-sealed
  event's DEK under the destination's domain in the mover's session and
  asserts both calendar archives (5.5). A valid legacy plaintext event
  with no carrier is fully sealed under a fresh DEK and the destination's
  domain: its newest CK generation for an active shared calendar, or the
  EWK for an unshared or reverting destination;
  malformed or partial carrier sets are refused as corruption, never
  treated as plaintext. The owner holds every domain of its account; a
  grantee moves only between two calendars granted to it at the edit level
  (upstream's checks: `RemoveItems` on the source, `AddItems` on the
  destination) and holds both CKs. A `MOVE` within one domain copies the
  stored bytes, as release 1 does. A `MOVE` that overwrites a destination
  event deletes it without scheduling, as upstream does: the key-account
  overwrite passes the resolved destination domain and writer keys to
  `delete_za` with `send_itip=false`; source-unlink and any collection
  overwrite also retain that flag and never enter fan-out
  (`copy_move.rs:747-756`; collection overwrite `:931-943`). An
  in-account event MOVE removes its source name with `names.swap_remove`
  (`:678`), not a source-event DELETE. The cross-account source-deletion
  paths are unreachable for key accounts under the existing cross-account
  refusal (`:122`).
- An event `MOVE` into a shared calendar must leave that calendar the
  event's only parent (review finding 2). Upstream's `MOVE` removes the
  source name and adds the destination one (`move_event`,
  `copy_move.rs:660-690`), so an event linked into A and B and moved from
  A into a shared C would keep B. Such a `MOVE` is refused with 403 and the
  single-parent trace unless the event's resulting names are exactly the
  destination; an event with no other link moves as above, rewrapped under
  C's CK.
- The first grant refuses a calendar any of whose events already has a
  second parent (`calendar has linked events`, 4.3). Deleting the event in
  the other calendar removes just that name (`DestroyArchive::delete_za`
  unlinks an event that has several), after which the grant succeeds.

Clients rarely use `COPY`: Apple Calendar duplicates an event with a new
`PUT`. The real-device check records what Apple does when an event is
dragged between a shared and an unshared calendar (11.5).

### 5.7 JMAP

Unchanged. JMAP calendar methods answer `accountNotSupportedByMethod` for a
key account's data (`za_assert_calendar_allowed`,
`crates/jmap/src/api/request.rs`), which also refuses `shareWith`; the
owner appears in a grantee's JMAP session as a secondary account without
the calendar capabilities (`crates/jmap/src/api/session.rs`). Release 6.

## 6. Invitations and scheduling with a shared calendar

In a shared calendar the owner organizes as in any of its calendars, and
an edit grantee schedules as upstream lets a grantee schedule, on the
owner's behalf, with every such message marked and verified (release 4's
rule, following upstream).

### 6.1 The owner organizes

The owner's writes in a shared calendar schedule exactly as in its other
calendars (release 2, key pinning): its own session and keys, its own
pins, the UID lease keyed by its account, the fan-out sealing each item to
the recipient's public key. Only the owner's event changes: it is sealed
under the calendar's CK instead of the EWK. The drain's merge of a reply
into that event, its import of an invitation into a default calendar that
happens to be shared, and `contacts/pin`'s redelivery all run in the
owner's session and reseal under the calendar's domain (5.5).

### 6.2 A grantee schedules for the owner: delegate sending

**Upstream's behaviour, which release 4 mirrors.** Scheduling runs for
whoever writes, and the organizer is a set of addresses, the writer's plus
the owner's (`scheduling_account_info`,
`crates/common/src/cache/principals.rs:813-833`), all of which
`itip_create`, `itip_update` and `itip_cancel` count as local, while
`ItipSendStatus::resolve` (`crates/groupware/src/calendar/itip.rs`) looks
only at the writer's token. An edit grantee writing into the owner's
calendar can therefore:

- create an event with ORGANIZER set to the owner's address and with
  attendees: upstream sends the REQUESTs from the owner's address
  (`crates/groupware/src/scheduling/organizer.rs:390-396`, and the iMIP
  task sends as the account that address resolves to,
  `crates/services/src/task_manager/imip.rs:125-139`);
- change such an event: upstream sends the update as the owner;
- delete it: a CANCEL as the owner;
- change the owner's own ATTENDEE line on an invitation organized by
  someone else: a REPLY as the owner;
- organize under its own address from the owner's calendar.

Only the ACL authorizes this, and upstream does not mark the messages as
sent on anyone's behalf.

**Release 4's rule: delegate sending, marked.** A grantee's write into a
key account's shared calendar (a `PUT` or a `DELETE`; MOVE overwrite and
source-unlink do not schedule) schedules as upstream decides, and runs release 2's
fan-out in the grantee's session, on the owner's behalf:

1. **Identity in the message.** In every component the fan-out sends, the
   property that names the sending party (the ORGANIZER of a REQUEST, an
   update or a CANCEL; the acting ATTENDEE of a REPLY, the line of the
   address the REPLY answers for) carries
   `SENT-BY="mailto:<writer's address>"` (RFC 5545 section 3.2.18) when
   that address is the owner's and the writer is a grantee. The server sets
   it at fan-out time, from the writer's primary calendar address (the
   first of the writer's own addresses in its unmerged account info), and
   replaces any `SENT-BY` the client wrote on that property, so
   a grantee cannot name a sender of its choosing. When the property names
   the writer's own address (including a group address both owner and
   writer hold), no `SENT-BY` is added,
   any the client wrote is removed, and the fan-out runs exactly as release
   2's; the owner's own writes are treated the same way. The placement
   scrubs the naming property only: a client's `SENT-BY` on any other line
   goes out as upstream sends it (`itip_export_component` copies every
   parameter but the `SCHEDULE-*` ones), and the stored event keeps its
   lines as the client wrote them, a client's `SENT-BY` on the owner's view
   included; only the messages carry the server's value on the naming
   property (author's call).
2. **Keys, pins and the event's account.** The fan-out uses the writer's
   session keys and the writer's pin record for the recipients' public
   keys: the owner's pin record is sealed under the owner's master key,
   which a grantee never holds, so the trust in a delegated message's
   recipients is the writer's, with key pinning's first-use pins and `3.8`
   refusals applying to the writer's record as for any of its fan-outs (the
   statuses land on the owner's event). The event in the owner's calendar
   is sealed under the calendar's CK, as any grantee write is (5.5), and
   its token pairs come from and return to it (3.6). The pending item in
   each recipient's inbox is sealed to the recipient's pinned public key
   exactly as release 2's, with `changed_by` the writer, as upstream's
   change log also records it. Release 2's check that the session's keys
   are the sender's (`keys.account_id == sender_id`,
   `crates/groupware/src/calendar/fanout.rs:220-227`) holds with the writer
   as the sender. The UID lease of a create and the SEQUENCE floor already
   run in `update.rs` on the resource's account, the owner's
   (`crates/dav/src/calendar/update.rs:448-453` and `:236-238`), and only
   the lease's fence reaches the fan-out. What the fan-out needs beside the
   writer's merged `account_info` is the event's account, the owner's, as a
   separate parameter (13.1 names it), for three things. The merged sender
   remains upstream's scheduling identity; the unmerged writer info,
   reconstructed separately if needed, supplies the writer's own addresses
   including groups for `SENT-BY`; owner-only info supplies locality and
   snapshots. The override's directly owned alias set expands the writer's
   listed aliases without groups (6.2's mechanism):
   - **Token locality.** The token union and the pairs persisted in the
     owner's event are computed against the owner's own addresses, not the
     merged list, so every attendee that is not the owner holds a pair, the
     writer included, while upstream's builders keep the merged list for
     routing the messages. Judged by the merged list, a delegated write
     would drop the writer's own pair from the owner's event, and the
     writer's later reply from its own copy would be refused as from a
     non-participant.
   - **The writer's own attendance: self-delivery, by parity** (re-review
     finding 4; re-review findings 3 and 4, revision 6; re-review finding
     2, revision 7; re-review finding 1, revision 8; re-review finding 1
     and recommendation, revision 9). Under the merged list upstream's
     builders count the writer's listed addresses as local and address nothing to
     them (`send_update_messages`,
     `crates/groupware/src/scheduling/mod.rs:221`; `NothingToSend` when no
     recipient remains, `organizer.rs:133`; cancellation discovery over
     external attendees only, `organizer.rs:58`; `itip_cancel`'s
     non-empty recipient set, `event_cancel.rs:47`, `:60`), so without a
     rule of its own the writer's copy would never see a delegated write.

     **The contract.** After a delegated write, the writer's personal copy
     receives exactly the messages upstream's organizer builders would
     address to the writer recipients defined below had the owner made the same write
     in her own session, with the writer an ordinary attendee, with one
     deliberate exception (below). Whatever the owner's own write would do
     to the writer's copy, the delegated write does; a behaviour the
     owner's own write also produces is inherited, recorded in the table
     after point 5, and is not a defect of this design. The fork may fix
     an inherited behaviour later for every attendee, never for the writer
     alone, since that would break the oracle of the differential test
     (11.2, case 25). This replaces revisions 8 and 9's hand-derived
     self-message builder (its full-tree REQUEST trigger, per-component
     CANCELs, prior-tree SEQUENCE rule, empty case, REQUEST-before-CANCEL
     order and alias rule), which this revision withdraws.

     **The mechanism.**
     - *The gate.* A delegated write (the resource's account is not the
       authenticated account) whose ORGANIZER in the new tree for a create,
       or the prior tree for an update or delete, is local under the owner's
       own addresses. A group address both accounts hold counts as owner-
       local for this gate and as writer-owned for the `SENT-BY` decision.
       The owner's addresses are
       `account_info(owner)`
       (`crates/common/src/cache/principals.rs:808-811`); the merged list
       upstream builds for the write (`scheduling_account_info`,
       `:813-829`) is the writer's addresses followed by the owner's that
       the writer lacks. The unmerged writer info is required to distinguish
       shared owner/writer addresses. Define two sets over the ATTENDEE
       addresses of every scheduling component of both supplied trees.
       **Listed writer addresses (L)** are the merged-list
       addresses that are not local under the owner's list, including a
       writer-only group's address. **Writer recipients (R)** are L union
       addresses resolving to the writer's account through delivery's quiet
       `za_account_id_from_email` lookup, always excluding owner-local
       addresses. Thus R includes sub-addresses, custom-rule and catch-all
       matches. **Override aliases (A)** are L intersected with the writer's
       directly owned listed aliases (the account's own addresses expanded
       over domain names, without membership groups). A is classified from
       listed identity, independently of recipient lookup success. Failure
       to prepare that listed identity or expand domain names propagates as
       a normal identity-preparation error for the write; per-recipient
       delivery lookup failures instead stay in the request's `Failed` map
       and yield `5.1` attempts. Only A
       gets the RSVP/PARTSTAT exception (D3/D4/D5); writer-only groups stay
       in L and R for ordinary retention and removal, without forced send
       or an added status when RSVP/PARTSTAT suppresses them.
       Apply upstream's owner-local exclusion first; compare set keys using
       `za_normalize_address(&attendee.email.email)` and the same normalization
       for message recipients. Payload addresses are unchanged. An address
       both accounts hold (a group's) stays
       local and gets no self-message (the table, row 4). On the attendee
       side (the ORGANIZER external under both lists) there is no second
       run: both runs would take `attendee_handle_update`
       (`crates/groupware/src/scheduling/event_update.rs:25-28`) and build
       the same REPLY for the owner's line.
     - *The second run* is upstream's organizer builders run once more,
       under the owner's addresses alone, before the first run (upstream's
       own, under the merged list) finalizes the committed tree:
       `itip_finalize` (`crates/groupware/src/scheduling/itip.rs:355-389`:
       it strips `SCHEDULE-FORCE-SEND`, then increments the listed
       SEQUENCEs) runs only on the first run's `Ok`
       (`event_update.rs:30-32`), so the second run is placed before
       `update.rs:259` for an update and `:495` for a create. It snapshots
       the prior and the committed trees under the owner-only list
       (`itip_snapshot`, `snapshot.rs:17`); on every ATTENDEE entry of both
       snapshots whose address is in A it sets
       two routing-only fields, `force_send = Some(Request)` and
       `rsvp = None` (public fields, `scheduling/mod.rs:34-49`, `:92-105`;
       outside `Attendee`'s equality and hash, `:277-301`; read by no
       payload, `organizer.rs:259-267`, `:338-345`,
       `event_cancel.rs:61-67`), and leaves `is_server_scheduling` as the
       line says (lifecycle review finding 1, revision 10; the exception
       below); and it calls upstream's inner builders, unchanged, from fork
       code in
       `crates/groupware/src/calendar/` (they are `pub(crate)`). At the
       existing-event PUT site, mirror upstream's actual branch
       (`update.rs:259-266`): when `schedule_tag.is_some()`, the second run
       calls `organizer_handle_update(old, new, old_itip, new_itip, &mut
       scratch)` (`organizer.rs:22-28`), mirroring `itip_update`'s organizer
       check, mismatch check and cancel fallback (`event_update.rs:18-48`);
       otherwise it takes the create branch, just as the first run does.
       A create-branch call (including a new-event PUT) calls
       `organizer_request_full(ical, &itip, None, true)`
       (`organizer.rs:309-314`), mirroring `itip_create`'s checks
       (`event_create.rs:17-25`); a deletion calls the organizer branch of
       `itip_cancel` (`event_cancel.rs:34-81`), which has no seam today (it
       snapshots at `:28`), so upstream's function is split into a
       snapshot-taking `pub(crate)` function behind the existing entry
       point, with no behaviour change (the three-line split, not a fork
       copy; 14 lists it). The first run ends `Ok` (the tree finalized),
       `NothingToSend` (the tree untouched by it, neither stripped nor
       incremented, `organizer.rs:133-135`, `:400-402`) or with another
       error (untouched). When the first run returned `NothingToSend` and
       the second run built, the committed tree is finalized exactly as the
       first run's success would have finalized it (lifecycle review finding
       2, revision 10): `itip_finalize` with the second run's increments for
       the update-builder branch (the scratch vector is kept for this case; both runs
       compute the same increments, the SEQUENCE bullet) and with none for a
       create-builder branch (`itip_create` finalizes with `&[]`, `event_create.rs:23-25`),
       so `SCHEDULE-FORCE-SEND` is stripped from every ORGANIZER and
       ATTENDEE line of every scheduling component (`itip.rs:355-369`) and
       the listed components are incremented (`:371-389`). Exactly one
       increment results, since `NothingToSend` returns before the first
       run's finalize. In every other case the scratch vector is discarded:
       the first run's finalize has done both, or upstream leaves the tree
       unfinalized and so does the fork. The cancel fallback
       (`event_update.rs:34-46`, taken when the committed snapshot fails and
       the prior organizer is local) stays unfinalized in both runs, as
       upstream leaves it. An error that `failed_precondition()` maps
       (`crates/dav/src/calendar/mod.rs:298-318`) aborts the PUT with 412
       and discards both runs; the other errors behave as upstream's
       (the first run keeps its trace and the tag is bumped once even when
       the second run built). On the organizer side
       the second run's errors equal the first run's, `NothingToSend`
       excepted, the only error that depends on the address list.
     - *What is kept from the second run*: the messages whose `to` names a
       writer recipient, in upstream's order (the full REQUEST first, then
       the partials, `organizer.rs:181-205`, `:207-304`), delivered to the
       writer recipients only, one attempted item per emitted message/address. Every other
       recipient is served by the first run. The second run's full and
       partial REQUESTs equal the first run's apart from DTSTAMP (each
       builder calls `PartialDateTime::now()`, `organizer.rs:220`, `:316`;
       `event_cancel.rs:29`), but a partial CANCEL whose recipient group
       includes the writer also carries the writer's line
       (`build_cancel_component` keeps the group's ATTENDEE lines,
       `event_cancel.rs:164-176`), and the `to` grouping and the partials'
       hash order differ, so nothing of the second run goes to anyone
       else. Listed writer addresses are local under the merged list, but
       an unlisted sub-address can also appear in the first run. Remove
       all writer-recipient addresses from first-run recipient lists on
       this delegated-organizer path, dropping empty messages, before
       appending the retained second-run messages. The combinator alone
       removes R; fan-out consumes the resulting disjoint provenance keys.
       Do not submit the removed
       attempts to fan-out or record a refusal for them. Thus every writer
       recipient is served solely by the second run, once per emitted
       message/address, even when both builders addressed it.
       L's writer-only group addresses remain normal delivery targets: a
       plain group records the ordinary `5.3` status when attempted, never an item
       redirected into the writer's inbox. R determines retention, not
       permission to bypass delivery checks.
     - *SEQUENCE.* Both runs compute the same `increment_sequence` (it
       depends on entry changes, attendee-set changes and added or removed
       instances, `organizer.rs:33-131`, and `Attendee` equality ignores
       locality) and read the same tree before finalize, so the writer's
       SEQUENCE equals the other recipients'. The second run's kept
       messages are appended to the first run's list, and one
       `za_reconcile_sequences`
       (`crates/groupware/src/calendar/fanout.rs:233-235`;
       `sequence.rs:161-198`) runs over the combined list
       (`za_set_sequences`, `update.rs:372`, `:599`) for the reasons it
       exists, the floor and the increments upstream's partial branch
       loses. When only the second run built, the finalize above has
       already applied the increment, so the reconciliation finds nothing
       to raise (revision 10 said it stood in for finalize; lifecycle
       review finding 2, revision 10, corrects that): a sole-attendee
       writer's DTSTART edit sends itself n+1 and the finalized tree is
       n+1; its SUMMARY edit sends n and increments nothing, upstream's
       own rule (`organizer.rs:41-53`). Deletions skip the reconciliation
       as today
       (`fanout.rs:230-231`). The floor (`sequence.rs:107-127`, applied at
       `update.rs:236-238` before either run) raises the incoming tree
       only, so both runs see the same tree. A CANCEL's SEQUENCE is
       whatever upstream's builder gives it: the prior component's plus
       one (`organizer.rs:222-226`, `:247-248`), for a deletion the
       master's (`event_cancel.rs:55-57`); revision 9's prior-tree
       SEQUENCE rule (re-review clarifications, revision 8) is withdrawn,
       since the builder applies that rule itself.
     - *Either run built* is the success rule for the fan-out call and
       `za_messages` (`update.rs:292-296`, `:354`, `:503-505`, `:566`) and
       for the schedule tag, set to 1 on a create (`update.rs:500`) or
       bumped on an update when either run built. A sole-attendee
       writer's create that left the tag `None` (`:515-531`) would send
       every later PUT through `itip_create`, which has no cancellation
       discovery, and make the DELETE skip scheduling
       (`crates/groupware/src/calendar/storage.rs:581-584`), so the
       writer's copy would never be updated or cancelled.
       Here "built" means the builder returned `Ok`, before recipient
       filtering. An `Ok` first run remains built and finalized even if
       filtering empties it; do not finalize it again. An `Ok` second run
       stays built even if no R recipient remains when the first run also
       built. Under the R union, when the first run returns `NothingToSend`
       and the second returns `Ok`, at least one second-run recipient is
       retained. Test that implication rather than redefining an `Ok`
       result as `NothingToSend` after filtering.
     - *DELETE.* The second run sits in the `delete.rs:257` loop over
       `ZaDeleteMessage.event` (the prior tree unsealed in `delete_all_za`,
       `storage.rs:585-592`, `:603-609`, under the gate `send_itip`, a
       schedule tag and an event ending in the future, `:581-584`; the
       container delete, `delete.rs:153-170`, feeds the same loop, but
       only the owner deletes a calendar, so the second run's gate never
       passes there). The loop already converts `message.message`
       (`:261`, an `Option`) into a vector; remove R recipients from the
       first-run CANCEL, drop it if emptied, then append the retained
       second-run CANCEL with its provenance. The owner's
       addresses are fetched there too (`delete.rs:79-81` holds the merged
       list). The deletion's pin read and the retirement of withheld
       entries (`fanout.rs:326-357`) stay off for a delegated fan-out, as
       this point rules below, so a message list that is non-empty only
       because of the second run does not switch them on.
     - *Delivery details that stay*: the token union over the owner's
       addresses (`fanout.rs:244-248`, `:948-969`; token locality above);
       the self-inbox carve-out in `resolve_inner` (`fanout.rs:1086-1089`),
       admitted only by the item's retained-second-run provenance on this
       delegated-organizer path, never just because an address resolves to
       the writer; admission also requires resolution to the writer and
       all receive permissions and key checks. On this path exclude the
       event owner's inbox too: an unlisted owner address resolving to that
       account records `5.3`, as the owner's own write would, even though
       the authenticated sender is the grantee;
       and the self item's delivery status recorded on the writer's line
       of the owner's event exactly as any recipient's (`update.rs:371`),
       so a later CANCEL, which clones ATTENDEE lines verbatim with their
       `SCHEDULE-STATUS` (`event_cancel.rs:147-179`), carries the same line
       under both paths. Account-resolution results used to classify
       recipients are request-local and shared with fan-out; a lookup error
       for a listed L alias, including a declined or `RSVP=FALSE` member of
       A, remains a `5.1` attempted failure with no item. An unlisted failed
       lookup remains a first-run attempt and also records `5.1`; neither
       failure permits a self-inbox item. The quiet lookup stays quiet
       through configured catch-all fallback as well as direct lookup
       (R19). Incoming snapshot locality and validation are
       unchanged: delivery to an inbox alone does not guarantee an import.

     **The one exception: RSVP and PARTSTAT never suppress a message to A's
     directly owned listed writer aliases** (author's call of revision 8,
     kept; narrowed by lifecycle
     review finding 1, revision 10). Under the owner-only list the writer is
     not local, and three things still drop it: `is_server_scheduling`
     (false for `SCHEDULE-AGENT=CLIENT` or `NONE`, `snapshot.rs:152-153`),
     `rsvp == Some(false)` (`mod.rs:214`, `:224`) and the PARTSTAT clauses:
     `send_update_messages` drops DECLINED, and `send_invite_messages`,
     which runs on a create and on a new override (`organizer.rs:97-103`,
     `:368`) and in `itip_create`, drops anything but NEEDS-ACTION. The two
     routing fields lift the last two, so `RSVP=FALSE` and any PARTSTAT (a
     DECLINED line on an update; an ACCEPTED or DECLINED line on a create or
     a new override) still reach the writer's own copy, and the writer's
     drain accepts their REQUESTs and CANCELs: its incoming check counts a
     local line by locality and scheduling agent only (`snapshot.rs:217`,
     `has_local_emails |= is_local && (force || is_server_scheduling)`). The
     scheduling agent is not overridden. A line with `SCHEDULE-AGENT=CLIENT`
     or `NONE` declares that the server does not schedule for that attendee;
     upstream sends it nothing, and the drain refuses for every account a
     message whose only local line is not server-scheduled
     (`snapshot.rs:217`, `:353-361`; `drain.rs:584`). A forced REQUEST would
     import a copy (export strips the parameter, `itip.rs:252-256`) whose
     later CANCEL, which clones the line with the parameter
     (`event_cancel.rs:147-179`), the writer's drain would refuse, and
     carrying an exception through that check would weaken the incoming
     validation for one path. Such a writer line therefore gets no
     self-message, as the owner's own write gives it none: parity, with no
     copy to maintain and no refusal; revision 11 withdraws revision 10's
     inclusion of `SCHEDULE-AGENT=CLIENT` and `NONE` in the exception. The
     exception, so narrowed, is the only designed divergence from parity.
     Revision 13 confines it to listed addresses; revision 14 confines it
     further to A. An unlisted sub-address, custom-rule
     target or catch-all address in R but not A retains upstream's RSVP
     and PARTSTAT filters. Account resolution does not turn such a line
     into a listed identity, and does not authorize a forced REQUEST. A
     writer-only group in L likewise retains those filters; ordinary
     server-scheduled group delivery and sole-group finalize still apply.
     The override is made on the snapshots, not in the trees: rewriting the
     writer's lines in cloned trees would change the payloads
     (`itip_export_component` keeps every parameter but `SCHEDULE-STATUS`,
     `SCHEDULE-AGENT` and `SCHEDULE-FORCE-SEND`, `itip.rs:254-272`, RSVP
     included, and a CANCEL clones lines verbatim), and `force_send` alone
     leaves `rsvp` blocking (`mod.rs:214`, `:224`).

     **One self item per address** (re-review finding 4, revision 6).
     Tokens belong to addresses, not accounts, and a pending item carries
     exactly one address and its token (`seal/item.rs:152`, `:210`), so the
     fan-out attempts each kept message once per writer recipient its `to`
     names. A recipient resolving to the writer gets one self item with
     that address's pair; other retained targets, such as a writer-only
     group, follow normal delivery/status checks. Self items use the existing
     pending format; the drain's REQUEST merge keeps pairs arriving for
     different addresses (`drain.rs:1128`). A self item's address is a
     token carrier and never an independent copy: every item of the UID
     merges into the account's one copy by upstream's merge, unchanged
     (two addresses of one account are two attendees to the builders and
     one copy to the drain: the table after point 5, row 2). A writer
     attending the master as one alias and an override as another
     therefore holds both pairs, and a later reply from either address is
     attempted (`fanout.rs:915`).

     Each self item is sealed to the writer's own public key and merged by
     its drain into its own copy (a merge builds no message, so nothing
     loops), so the writer's own copy follows the owner's event as an
     ordinary attendee's would, and its later replies are neither stale nor
     unauthenticated. `resolve_inner`'s rule that a fan-out never writes
     into the sender's own inbox (`fanout.rs`) is carved out for these
     items.
   - **The `SENT-BY` decision**, which splits the merged list into the
     owner's addresses and the writer's (point 1).

   A delegated fan-out neither records nor consumes key pinning's withheld
   entries (author's call): an entry is bound to a document of the sender's
   own account (key pinning section 4.1), and the writer's `contacts/pin`
   could not redeliver from the owner's event. A refused delegated delivery
   keeps its `3.8` on the owner's event. It was decided under the writer's
   pins, so the owner's page has no remedy for it (4.8); only an edit of the
   event that builds a message to that recipient sends again.
3. **The recipient's drain accepts a delegated item.** Release 2's step 2
   (`crates/groupware/src/calendar/drain.rs:644-661`) requires the address
   an item names, the ORGANIZER of a REQUEST or CANCEL or the acting
   ATTENDEE of a REPLY, to resolve to the item's `changed_by` principal.
   That rule stays for every item without `SENT-BY`. The drain carries the
   candidate as `ZaSenderCandidate { named, sent_by }`: `named` supplies
   the merge sender and reply-token match, while `sent_by` supplies the
   delegated consistency check. An item whose naming
   property carries `SENT-BY` passes instead when its `SENT-BY` address
   resolves to an address of the item's `changed_by` principal,
   `changed_by` is a key account, and the named address resolves to a
   local key account other than `changed_by`. A named address that
   resolves to no account, or to `changed_by` itself, is refused
   explicitly, as is a `SENT-BY` that does not resolve to `changed_by`:
   release 2's iTIP-level rejection (`SenderIsNotOrganizerNorAttendee`, the
   item kept as history, release 2 section 6.4 step 4), traced with a
   reason of its own (section 10). For the merge, the item's sender is the
   named address, the owner's, never the `SENT-BY` one: upstream's
   `handle_reply` finds the replying attendee by it, and a delegated
   REPLY's token is matched against that address's pair.

   **The authority is the write-time ACL, as upstream's.** A client can
   make the fan-out build a message in the owner's name only by writing
   into the owner's account: the owner's addresses count as local only
   through `scheduling_account_info(writer, owner)`, called with the
   resource's account, and that write passed upstream's ACL check
   (`has_access_to_container` before each `PUT` branch in `update.rs` and
   before `delete_za` in `delete.rs`) and 5.2's resolver. The drain's check
   is defence in depth against server bugs, not the
   authorization, and it reads nothing of the owner's account. So an item
   written while the grant stood is delivered after a revoke, since the
   write was authorized when it was made, and the window between a
   fan-out and a write whose commit then fails is release 2 section 5.4's.
4. **Replies.** A reply to a delegated REQUEST goes to the ORGANIZER's
   address, the owner, into the owner's inbox, and the owner's drain merges
   it into the shared calendar's event as into any of its own (the event is
   in the owner's account). A reply to an event the grantee organized under
   its own address goes to the grantee's inbox and is not merged: the
   grantee's drain looks a UID up in the grantee's own account only
   (`drain.rs:665, 825`), as upstream's ingest does
   (`crates/groupware/src/calendar/itip.rs:171-183`), and files the reply
   as history (`EventNotFound`). An inherited limit; section 13.1 records
   the later refinement. Beside it, the owner cannot reschedule or cancel
   such an event: in the owner's session the grantee's address is not
   local, so upstream takes the attendee branch (its 412 or a store without
   a message), and the owner's `DELETE` builds a declining REPLY or
   nothing; inherited as well.
5. **What a grantee cannot do** stays what upstream's bits refuse: a view
   grantee writes nothing; no grantee deletes the calendar, re-shares it or
   changes its ACL, since release 4 never grants those bits; `MKCOL` in the
   owner's account is member-only; and the single-parent rule (5.6)
   stands. A writer without `CalendarSchedulingSend`, or a write upstream
   does not announce (no attendees, no ORGANIZER, a past event, a change
   without scheduling content), sends nothing and is stored as upstream
   stores it, and a `PROPPATCH` of an event runs no scheduling
   (`crates/dav/src/calendar/proppatch.rs`).

**What the writer's copy inherits from upstream** (re-review
recommendation, revision 9). Each row is a behaviour the owner's own write
produces in an ordinary attendee's copy, so by point 2's contract the
writer's copy shows it too. The differential test (11.2, case 25) asserts
equality, not the behaviour; a fix would be for every attendee, never for
the writer alone.

| Behaviour | Upstream's cause | Disposition |
|---|---|---|
| 1. A later full REQUEST reactivates an override an instance CANCEL had cancelled (re-review finding 1, revision 9) | The full REQUEST carries every component with attendees to every attendee of any component (`organizer.rs:325-329`, `:366-373`); the CANCEL merge removes SEQUENCE and STATUS and adds back `STATUS:CANCELLED` without a SEQUENCE (`inbound.rs:198-230`); the REQUEST merge accepts `>=` against the absent SEQUENCE and replaces STATUS (`inbound.rs:101-133`; STATUS is a snapshot entry, `snapshot.rs:299`). If the override lost its last attendee the REQUEST omits it and the merge removes it (`inbound.rs:157-166`): active either way. The plain inbox path uses the same merge (`crates/groupware/src/calendar/itip.rs:204-212`, `:788`) as the drain (`drain.rs:1075-1081`, `:1099`). | Inherited; accepted, not fixed. |
| 2. Two addresses of one account are two attendees to the builders and one copy to the drain | Removing alias A from the master while alias B attends an override emits the full REQUEST (to B among others) and a partial CANCEL of the main instance to A, in that order; the single copy (`drain.rs:702-714`) applies the REQUEST, then the master CANCEL cancels the series, the override included (`inbound.rs:231-232`, `:265-305`). The fork's delivery keeps the emitted order (items written in message order, `fanout.rs:455-549`, applied in id order, `drain.rs:417-423`), so the outcome is deterministic; upstream's own plain path is not (the queue shuffles scans of more than three messages, `crates/smtp/src/queue/manager.rs:157-159`). | Inherited; accepted. Revision 9's alias rules (the whole-event payload per alias, the account-level CANCEL) are withdrawn by this revision; revision 6's one self item per address stands. |
| 3. An override-only writer's copy stays active after the event's deletion (re-review clarification, revision 9) | `itip_cancel` collects recipients over every component (`event_cancel.rs:41-52`) but builds a master-only CANCEL with the master's ATTENDEE lines (`:55-67`, `:164-165`); the writer's drain finds no local line and refuses it (`snapshot.rs:353-361`; `drain.rs:584-589`), as a plain account's ingest does (`crates/groupware/src/calendar/itip.rs:157`). | Inherited; accepted: answered by parity, with no component retained in the CANCEL. |
| 4. A group address both accounts hold gets no self-message | The address is local under the owner-only list too (6.2's gate), as it is for the owner's own write. | Inherited; accepted. |
| 5. A REPLY as the owner when the writer also attends (the limits below) | `local_attendee()`, the first local line of a hash set under the merged list (below). | Inherited; accepted. The second run is skipped there (6.2's gate). |
| 6. A message whose payload has no listed, server-scheduled line of the recipient can reach its inbox but fail incoming validation | `Email::new` uses exact local-address membership (`scheduling/mod.rs:244-250`); the incoming snapshot requires some local server-scheduled line (`snapshot.rs:217`, `:353-361`). Account resolution for delivery does not change that snapshot rule. | Inherited; accepted in both runs. Case 25 separately checks transport parity and receiver rejection; no self-only validation exception. |
| 7. A shared owner/writer group as ORGANIZER enters the delegated-organizer path, then its item is refused at the recipient's drain | The shared group is owner-local for the gate and writer-owned for `SENT-BY`, so the fan-out adds no `SENT-BY`; the named group resolves to the group id rather than the writer's `changed_by`, and the ordinary sender check rejects it. | Inherited; accepted. Case 25 checks the gate, omitted `SENT-BY` and receiver rejection. |

**Upstream limits delegate sending inherits** (recorded, not fixed):

- **MOVE overwrite.** The overwritten event's attendees, including any
  personal copy of the writer's, keep their active copies: MOVE sends no
  CANCEL, as the owner's own MOVE sends none (5.6; case 12).
- **UID collision across the owner's calendars.** The recipient's copy is
  matched by UID, and upstream refuses a duplicate UID only within the
  target calendar (`assert_is_unique_uid`, a rule the fork's
  `za_assert_is_unique_uid` keeps, `crates/dav/src/calendar/mod.rs`). So an
  edit grant on one calendar of the
  owner lets the grantee create there an event with the UID of an event in
  another, unshared calendar of the owner, organized by the owner, and the
  delegated REQUEST rewrites the recipients' copies of that other event.
  Upstream has the same hole for any grantee. Binding a copy to its source
  calendar was considered and not taken (13.2).
- **A REPLY as the owner when the writer also attends.** Upstream's
  attendee update acts for `local_attendee()`, the first local line of a
  hash set under the merged list, so when the writer is also an attendee of
  the invitation the REPLY is built for the owner's line or the writer's,
  nondeterministically (a REPLY for the writer's own line is not attempted,
  or upstream answers 412 `CannotModifyAddress`). No second run happens
  on this side (6.2's gate; the table above, row 5).
- **An attendee-side REPLY to an unlisted owner sub-address.** The second
  run is skipped on this side. An unlisted owner sub-address may resolve to
  the event owner's inbox and reach it, whereas the owner's own write would
  record `5.3`. This inherited limitation does not change the delegated-
  organizer owner-inbox exclusion above or incoming validation.
- **`SENT-BY` in stored copies, per component** (review finding 8,
  re-review finding 6). Each component of a recipient's stored copy takes
  its ORGANIZER line, `SENT-BY` included, from the message that last set
  it: an import takes every component's from the incoming REQUEST (an
  import keeps the tree), and so does a component a later REQUEST adds to
  the copy (the merge's `AddComponent` copies every property of the new
  component, `crates/groupware/src/scheduling/inbound.rs:138`); a
  component the copy already holds keeps its ORGANIZER through REQUEST
  updates; and a CANCEL replaces it (the CANCEL merge removes ORGANIZER and
  copies it from the incoming message, `inbound.rs:198-229`, and a master
  cancellation does the same for the instances it covers, `:269-301`). So
  a copy the owner created gains the grantee's `SENT-BY` on an override the
  grantee adds and on what a delegated cancellation covers, and a copy a
  delegated REQUEST created loses it at the owner's cancellation. A copy
  keeps its `SENT-BY` after a revoke, and the recipient's REPLY echoes it
  (`itip_export_component`).

The drain and a pin redelivery are never a grantee's write: each runs in
an account's own session and writes only into that account. The drain
looks a UID up and imports only in the recipient's own account
(`find_event(server, account_id, uid)` and
`get_or_create_default_calendar(account_id, account_id)`, `drain.rs:665,
825, 864-867`), and `contacts/pin` reseals only the caller's own source
event, so neither writes into a calendar another account shares with it.
When an account's own default calendar is shared, the drain's import into
it is the owner's own write and seals under that calendar's CK (5.5).
Delegated sending adds no trace event; a drain's refusal of a delegated
item is traced as an iTIP-level rejection with a reason of its own
(section 10), without content.

### 6.3 An attendee who is also a grantee

When the owner invites a grantee to an event in a shared calendar, the
grantee sees the event twice: once in the shared calendar, through the
grant, and once as its own copy, which is where it answers. This is
inherited upstream behaviour, not a fork choice: upstream delivers the
invitation copy into the attendee's own default calendar whatever grants
the attendee holds (`itip_ingest` dedups by UID only inside the
recipient's own account and imports a local sender's invitation into the
default calendar, `crates/groupware/src/calendar/itip.rs`; no check for
access to the organizer's calendar exists), and the fork's fan-out and
drain do the same (the fan-out skips only the sender's own account, and the
drain's `find_event` looks only in the recipient's account before
importing into `get_or_create_default_calendar`). The grantee's answer
travels as any reply does (release 2 section 4.4) and updates the owner's
event, which the grantee then also sees in the shared calendar.

A grantee that changes its own participation in the owner's copy instead
of its own gets upstream's behaviour: under the merged addresses the
owner's event is organized locally, so the change is an organizer-side
update, sent to the other attendees as the owner with `SENT-BY` the
grantee (6.2), and no REPLY is built. Self-delivery (6.2, point 2) gives
the grantee's own copy what upstream sends an ordinary attendee when the
owner makes the same write (re-review finding 3, revision 6; re-review
finding 2, revision 7; re-review finding 1, revision 9), the grantee the
event's only attendee included:

- **an update** of a component the grantee attends, in any field its copy
  shows: upstream's update REQUEST, the full one when a master change
  reaches the grantee (`organizer_request_full` from
  `organizer_handle_update`, `organizer.rs:181-205`), a partial one
  otherwise (`:207-304`); a sole attendee's SUMMARY edit gets the full
  REQUEST at the unchanged SEQUENCE (`organizer.rs:41-53`);
- **a decline**: the update REQUEST as for any change of the grantee's
  line, by 6.2's one exception (upstream drops a DECLINED attendee,
  `send_update_messages`, `mod.rs:221`; re-review finding 4);
- **a deletion** of the shared event: `itip_cancel`'s CANCEL
  (`event_cancel.rs:34-81`), the sole attendee's included;
- **the removal of the grantee's own line**, other attendees remaining or
  not: the partial CANCEL of each instance it no longer attends, the main
  instance's when its line left the master (`organizer.rs:58-74`,
  `:207-304`);
- **the deletion of an override** the grantee attends: the partial CANCEL
  of that instance (`organizer.rs:111-131`), at the override's prior
  SEQUENCE plus one.

When the same write also sends the grantee the full update, that update
governs and the partials are dropped, as upstream drops them for any
attendee (`organizer.rs:136-168`).

In each, the grantee's drain merges the item into its own copy as an
ordinary attendee's copy would merge it, the behaviours of 6.2's
inherited table included, and the grantee's later reply from its own copy
is neither stale nor refused. The page (4.8) and the real-device check
(11.5) still say that an attendee answers from its own copy, the path
every client expects.

### 6.4 The SEQUENCE floor and the UID lease

Both stay keyed by the owner's account, which is the calendar's account.
The `PUT` path takes the UID creation lease on the account that owns the
target calendar (`za_take_uid_lease` with the resource's account,
`crates/dav/src/calendar/update.rs`), so a grantee's create and the
owner's own creates of a UID serialize on the same lease, and the drain's
import into the owner's default calendar takes it too. The SEQUENCE floor
(key pinning section 5.1) applies to every `PUT` of an existing event in a
key account's calendar, the grantee's included, with the merged address
list. Release 2 section 5.1's rule that the sending state follows the
calendar's owner stands (a key owner's calendar resolves to `SendSealed`
under the writer's permissions), and a grantee's write reaches the
fan-out in the grantee's session with the owner's account as the event's
account (6.2).

## 7. Revocation and rotation

### 7.1 Revoke

A revoke is one conditional write (4.5) in the owner's request:

- the grantee's `acls` entry, its preferences entry (and with it its grant
  ring and its personal bundle) and its ledger record are removed;
- a new CK generation is added (7.2), wrapped to the owner in the owner
  ring and to each remaining grantee in its grant ring;
- gone grantees are swept (7.5).

After the commit the endpoint invalidates the removed grantee's caches
(4.6). From then on, as upstream's access-token refresh already does for
any revoked grant, the grantee's next request builds a token without the
owner, the URI gate answers 403 for the owner's calendar URLs, and the
calendar vanishes from the grantee's home at its next sync (a `PROPFIND`
of its home no longer lists it). On a node the broadcast missed, the
resolver refuses the grantee at use (4.6), since its grant ring is gone.
Nothing else changes: no event is touched.

### 7.2 Rotation

Every revoke, re-key and sweep adds a generation: the owner's session
generates a new CK and wraps it, in the same batch, under the EWK into the
owner ring and to the public key the ledger records for each remaining
grantee into its grant ring. From that commit on, every write into the
calendar seals under the new generation (5.5), so nothing written after the
revoke is wrapped under a key the removed grantee held: the calendar
assertion makes a write that read the old ring lose and rewrap.

Events and bundles already sealed keep their wraps. The old generations
stay in the owner ring and in every remaining grantee's ring, so the owner
and the remaining grantees keep reading everything. A personal bundle
moves to the newest generation at its entry's next write, and the shared
bundle at the next dead-property write by any principal. A grantee added
later receives
every generation, because upstream shows a new grantee every event of the
calendar. A rotation costs one archive write and needs no grantee session:
it uses only public keys from the owner's own sealed ledger, never the
served key and never the directory.

### 7.3 The product claim, and resealing deferred

The product claim, for the page and for product copy: **"Removing someone
stops them seeing anything new; what they could already see, they could
already have copied."**

Resealing the calendar's existing events under a fresh key at the revoke
would add one protection: against the operator colluding with the removed
grantee, using the grantee's old grant ring (from a backup, or saved
before the revoke) and its password to open the old generations, and
through them the old events still in the live store (2.1). It would cost a
rewrite of every event's content in the owner's session at every revoke,
the conversion's cost plus a full unseal and reseal of each event. It is
deferred. The format allows it later without a change: every wrap names
its generation, so a reseal would rewrite the events under the newest
generation and then drop the retired generations from every ring, and the
leak scanner's rules (11.4) already accept any set of listed generations.

### 7.4 Level changes and re-keying

- **A level change** (`shares/grant` with another level and the same key)
  rewrites the grant's `acls` bits and its ledger level in one write,
  without a rotation: an edit grantee downgraded to view still reads
  everything, and an upgrade adds rights, not keys. The grantee's caches
  are invalidated after the commit, as for any grant change.
- **A re-key** (`shares/grant` for a grantee whose recorded key differs
  from the pin, after the owner confirmed the new key with `contacts/pin`)
  is a revoke of the old key and a grant to the new one in one write: a
  new generation, so nothing new reaches the old key, and the grantee's
  ring rebuilt with every generation wrapped to the new key, so the person
  reads the calendar again. The ledger records the new key and time.

### 7.5 Gone grantees

A grant is gone when its account no longer exists or is no longer a key
account. Its entry is inert: its ring opens only with that account's
private key, which was wrapped under an MK whose vault record went with the
account. The owner's list reports it as `gone`; the owner's next grant or
revoke on the calendar removes it, with its `acls` entry and ledger record,
in the same write and with that write's rotation (author's call: only an
owner session can rotate, and a removal without one would keep the gone
key's generations current). A revoke of its address removes it too.
Nothing removes it in the background.

## 8. Interactions with releases 1 to 3

### 8.1 Provisioning

Unchanged. `setup-token` and `setup` refuse an account whose calendars
carry `acls` or more than one preferences entry (`za_has_user_calendars`,
`crates/http/src/api/vault.rs`), and that stays right: a key account's
grants are written only by the share endpoints, which need an active
account.

### 8.2 The drain and the fan-out

The drain still runs on the requesting account's own inbox on every DAV
request, whatever account the request targets
(`crates/dav/src/request.rs`), with that account's keys. It imports into
the account's default calendar, which may be shared, and merges into
events that may live in a shared calendar; both seal under the calendar's
domain and assert the calendar archive, and a lost assertion is a transient
item error (5.5). Its sender check (release 2 section 6.4, step 2) gains
the delegated branch of 6.2: an item whose ORGANIZER or acting ATTENDEE
carries `SENT-BY` passes when its `SENT-BY` resolves to `changed_by`, a
key account other than the one the named address resolves to; every
other item is checked as before. The fan-out runs
for the owner's writes and, delegated, for a grantee's (6.2), with the
event's account as a parameter beside the sender; its items are sealed to
the recipients' public keys (`0x02`) as before, never under a CK.

### 8.3 Key pinning

A grant requires a pin whose key equals the served key, and wraps to it
(4.3); rotations wrap to the keys the ledger recorded (7.2);
`contacts/pin`'s redelivery reseals its source event under that event's
domain (5.5). A delegated fan-out uses the writer's pin record and neither
records nor consumes withheld entries (6.2). Pins, withheld entries and the
pin record's format are unchanged.

### 8.4 The leak scanner

Section 11.4.

### 8.5 Mail

Nothing changes. Mailbox sharing stays refused (mail section 6.3); IMAP
and POP3 sessions never open a CK; the access token keeps stripping a key
account's mailboxes and messages from other accounts' entries, and keeps
the `Calendar` collections that grants now legitimately produce.

### 8.6 Account destruction

- **A destroyed owner.** Upstream already handles it, and release 4 adds
  no code. The registry's account destroy (`x:Account/set` destroy,
  `crates/jmap/src/registry/set.rs:700-712`, the only product path, which
  the SCIM crate and the test suite also take) calls
  `store().acl_revoke_all(id)` before `schedule_account_destruction`.
  `acl_revoke_all` (`crates/store/src/query/acl.rs:84-142`) deletes every
  ACL index key whose owner is the destroyed account and returns the
  grantee ids, and each gets `CacheInvalidation::AccessToken`, flushed at
  the end of the set. So every grantee's token is invalidated and rebuilt
  without the owner at its next request, its discovery works, no residual
  ACL key names the owner, and the grantee's resident collection keys leave
  memory with that invalidation (4.6). The destroy task
  (`crates/services/src/task_manager/destroy_account.rs`, ending in
  `danger_destroy_account`) then removes every record of the account, its
  calendars, rings, ledgers, entries and events with them; the task's own
  ACL range is the grantee side. On a cluster node that missed the
  invalidation the grantee's stale token lists the dead owner until it is
  evicted, and discovery fails there (4.6).
- **A grant racing the owner's destruction** (review finding 5). The
  registry destroy deletes the Account object
  (`crates/jmap/src/registry/set.rs:689-699`), then revokes the owner's
  grants with a token invalidation per grantee (`:704-709`), then queues
  the destroy task (`:712`; `crates/jmap/src/registry/mapping/
  principal.rs:472-490`), whose `danger_destroy_account` clears the
  grantee-side ACL range and the owner's Property range
  (`crates/store/src/dispatch/store.rs:360-392`), which holds the archives
  and the vault record. Without a barrier, a grant paused after its checks
  and resumed after the revoke scan would pass its calendar, vault and pin
  assertions (all Properties the registry deletion does not touch) and
  write a `(grantee, owner, Calendar, doc)` key that neither scan removes;
  the grantee's token build keeps it (`za_is_key_account` answers
  `Ok(false)` for a dead id), and discovery's `server.account(dead_id)`
  then errors for good. The barrier is the share batch's assertion on the
  owner's registry Account item (4.5): the deletion precedes the revoke
  scan, so a grant that commits before it is revoked by the scan, and one
  that would commit after it loses and answers 409 `account deleted`. This
  claim stands for every share endpoint's write, the sweep included.
- **A destroyed grantee.** Upstream's destroy removes the grantee's ACL
  index keys (they are under its id), so its access ends with the account.
  Its entry, ring and ledger record stay in the owner's calendar, inert,
  until the owner's next grant or revoke on that calendar sweeps them
  (7.5). That sweep's ACL change writes a share notification under the
  dead account's id (2.2), an orphan record holding ids only. The sibling
  race, a grant to a grantee whose account is being destroyed, can write
  the grant's ACL key under the dead grantee's id after the destroy task
  has purged that range; it is inert, since no token is ever built for a
  dead id, and the owner's next grant or revoke on the calendar sweeps the
  grant (7.5).

### 8.7 Recovery, backup, alarms, indexing and quota

- Recovery and password changes keep MK and the private key, so every wrap
  survives (2.1).
- Backup and restore copy the sealed records as stored (release 1 section
  9).
- Alarm emails stay generic and go to the calendar's account, the owner
  (release 1 section 9); a grantee's clients run their own display alarms.
- Full-text indexing stays gated for the owner's account.
- The shared form's entries count against the owner's quota, as release 1
  section 7.1 counts a sealed collection's stored bytes: a few hundred
  bytes per principal, one more padded bundle per shared calendar in the
  owner's entry (the shared bundle), a fixed header per ring (the 16-byte
  key-domain id, the 8-byte rename change id and the 1-byte
  boundary-pending flag in each, and the 8-byte grant change id in each
  grant ring), plus 108 bytes per generation in each grant ring and 36 in
  the owner ring, before padding and base64.

### 8.8 Deleting a shared calendar

The owner's `DELETE` of a shared calendar runs upstream's path: its events
(each with that calendar as its only parent, 5.6) are deleted with it, a
scheduled event's CANCEL goes to the fan-out (release 2 section 5.5), and
the index builder removes the ACL index keys and writes the share
notifications. Upstream's `DELETE` invalidates no grantee's cache; for a
shared calendar of a key account the handler additionally invalidates the
grantees' access tokens after its commit (author's call), so their resident
keys, and the collection keys they cache, leave memory. The default
calendar cannot be deleted (upstream).

## 9. Feature gating for key accounts

| Feature | Release 3 | Release 4 |
|---|---|---|
| Granting a key account's calendar | refused everywhere | view and edit grants to current key accounts the owner has pinned, through the share endpoints only (section 4) |
| CalDAV `ACL` method on a key account's calendar | 403 | 403 (unchanged) |
| JMAP `Calendar/set` `shareWith`, JMAP calendars | `accountNotSupportedByMethod`; not advertised | unchanged (release 6) |
| Grants to non-key accounts and groups; `Share`, `Delete`, free/busy-only and the unenforced bits | n/a | refused by the endpoint (`not a key account`; levels only) |
| A grantee's `PROPFIND`, `GET`, `HEAD` and `REPORT` (`calendar-query`, `calendar-multiget`, `sync-collection`) | 403 at the URI gate | served, unsealed with the grantee's CK (5.3) |
| A grantee's `PUT`, `DELETE`, `MOVE` and `PROPPATCH` | 403 | per level; sealed under the CK; scheduling as upstream, with `SENT-BY` (6.2); MOVE and PROPPATCH send nothing |
| A grantee's `PROPPATCH` of the calendar's colour or order | 403 | upstream's: shared dead properties, 403 at view, recolours for every principal at edit (1.2) |
| `MKCOL`, `MKCALENDAR`, calendar `DELETE`, `ACL` by a grantee | 403 | 403 (upstream: no membership, no `Delete`, no `Share`) |
| `calendar-home-set` and assisted discovery for a grantee | the key owner's calendars unreachable | listed, with the grantee's names (5.4) |
| The owner's scheduling inbox and outbox, for a grantee | 403 | 403 (unchanged) |
| `free-busy-query` on a shared calendar by a grantee | 403 | no busy periods, upstream's answer without the free/busy bit (5.3) |
| Free-busy, outbox POST and `Principal/getAvailability` for other users | 403, 3.7, skipped | unchanged |
| In-account `COPY` and `MOVE` touching a shared calendar | n/a | event `MOVE` rewraps; event `COPY` 403; collection `COPY` and `MOVE` 403 except a rename (5.6) |
| Cross-account `COPY` and `MOVE` | 403 | 403 (unchanged) |
| Impersonation, master user, group membership, `is_member` | 403 | 403: a grant to the token's own account and its own grant ring are required, and a token carrying `Impersonate` is refused at the gate (5.1, 5.2) |
| A grantee's `LOCK`, `UNLOCK`, and `sync-collection` vanished entries | 403 at the URI gate | `LOCK` and `UNLOCK` upstream's 403; a sync token older than the sync boundary (the later of the grant and the calendar's last rename), or any incremental sync while the boundary is pending, refused with `valid-sync-token` (an initial sync served); vanished entries limited to the requested collection and the eligible calendars' paths (5.2, 5.3) |
| Foreign calendars in a grantee's own-home listing (assisted discovery, `propname`) | the key owner's calendars unreachable | listed only when eligible: the token's own account in the calendar's `acls` with `Read`, no `Impersonate` (5.2) |
| Invitations from the owner in a shared calendar | n/a | release 2's, sealed under the CK; a grantee who is an attendee also gets its own copy (6.3) |
| A grantee's invitations, updates, cancellations and replies in the name of an account whose shared calendar it holds an edit grant on | n/a | as upstream, with `SENT-BY` the grantee, under the grantee's keys and pins, authorized by the write-time ACL (6.2) |
| A delegated item at the recipient's drain | n/a | accepted when its `SENT-BY` resolves to its `changed_by`, a key account other than the named one; refused otherwise, with a reason of its own (6.2, 10) |
| A grantee organizing under its own address from the owner's calendar | n/a | as upstream, sent as the grantee; replies to it are not merged (6.2) |
| The owner's `DELETE` of a shared calendar | n/a | upstream's, plus the grantees' caches invalidated after the commit (8.8) |
| Provisioning an account whose calendars carry grants | 409 | 409 (unchanged) |
| Share notifications | none (no grants) | upstream's, without names (2.2) |
| Mail sharing | refused | refused (unchanged) |
| Alarm email, full-text indexing | generic; gated | unchanged |

## 10. Traces and errors

- **No content, no key, nothing new beyond ids.** No new trace event type
  is added (the enums are generated upstream). Existing events carry
  `zero-access:` details and account ids, document ids, levels and counts
  only: never an address, a fingerprint, key bytes, a calendar or event
  name, or any other content. Endpoint bodies and responses are never
  traced (release 1 section 4.1).
- **The gate and the resolver** refuse with 403 and
  `SecurityEvent::Unauthorized`: `zero-access: calendar access without
  session keys` at the URI gate (release 1), `zero-access: calendar access
  without a collection key` at use (5.2; inside a listing the calendar is
  skipped instead), and `zero-access: copy into or out of a shared
  calendar` for 5.6's refusals.
- **Delegated sending** adds no error and no trace of its own. A drain
  that refuses a delegated item (6.2, point 3) traces it as release 2
  traces an iTIP-level rejection, with ids and no content, under its own
  reason string, `zero-access: delegated sender not verified`: the drain's
  `itip_reason` (`crates/groupware/src/calendar/drain.rs`) maps each
  rejection to a fixed string, and this one gets a fork string of its own,
  with no enum change, so that an operator can tell it from release 2's
  refusals (author's call on the string).
- **Unseal failures** under a CK (a ring, wrap, envelope or bundle that
  does not open, an unknown wrap type or ring version, a `0x03` envelope
  naming another calendar or an unlisted generation, a grant ring whose
  header names another account) follow release 1 section 10: 500 for the
  request or the single report item, a logged `StoreEvent::DataCorruption`
  through `seal_error` naming the account and document id, nothing deleted
  or rewritten. An endpoint that meets one in the owner's ring answers the
  generic 500.
- **Endpoints** answer as section 4 states: the shared table, their own
  409 reasons, and the generic problem body for everything else.

## 11. Testing

### 11.1 Unit

In `crates/vault`: a grant wrap round-trips, fails under another private
key and under associated data naming another owner, calendar, grantee or
generation, and cannot be opened as a `pk-dek` wrap nor the other way
round. In the groupware seal tests: the owner ring round-trips with its
ledger and refuses an edited header; a grant ring whose header names
another account is refused; a `0x03` event envelope opens only with its
generation's CK and its calendar's id; the owner's five-part names, a
grantee's three-part names and release 1's two-part names are told apart,
and any other shape is refused; a conversion's rewrap leaves the trees,
bundles and token pairs byte for byte unchanged, a plaintext owner entry
converts straight into the shared form, and a plaintext event is sealed
under the CK, with both calendar and event assertions; a grantee's view of
a calendar never opens the owner's ring
nor the owner's personal bundle, and a grantee's write of a dead property
rewrites the shared bundle and leaves the owner's ring and personal bundle
bytes unchanged. In the fan-out: 6.2's `SENT-BY` placement, per method and
component (set on the ORGANIZER of a delegated REQUEST, update and CANCEL
and on the acting ATTENDEE of a delegated REPLY; a client's `SENT-BY`
replaced; none, and a client's removed, when the property names the
writer's own address or the writer is the owner; a client's `SENT-BY` on
another line left as it is); the token union computed against the
owner's addresses, so that the writer holds a pair; L, R and A classification
including writer-only groups, directly owned aliases, sub-addresses, custom
rules and catch-all; the RSVP/PARTSTAT exception applies to A only. The
ordinary-provenance self-inbox refusal needs a server-backed za fixture:
an ordinary item addressed to the sender's listed alias or sub-address is
not delivered even with a served key; only retained-second-run provenance,
writer-account resolution and ordinary checks admit it. A deterministic
test-mode `DELEGATION_LOOKUP_FAIL_ONCE` keyed by sender id and normalized
address injects one failure before resolver or ordinary `resolve_inner`
lookup, reusing the request's `Failed` result. A listed L alias, including
declined or `RSVP=FALSE`, records `5.1` with no item; an unlisted failed
lookup retains its first-run attempt and records `5.1`. A separate quiet
catch-all fixture uses a dedicated canary domain, restores
`catchAllAddress` and custom rules, and checks public `Traced` output
against `EventType::variants()`; its cold control checks both local parts,
its warm control checks domain names and both local parts. The quiet runs
show classification ran through cache population or a positive companion,
and leak none of the canaries. An unlisted owner address on a delegated
organizer write records `5.3` and never reaches the owner's inbox. In the
drain: 6.2's delegated sender
check against fixture data (a `SENT-BY` resolving to `changed_by`, a key
account other than the named one, passes; a `SENT-BY` that does not
resolve to `changed_by`, a plain `changed_by`, a named address that
resolves to no account or to `changed_by` itself are refused, each with
the delegated reason string); a shared group ORGANIZER without `SENT-BY`
is refused by the inherited ordinary sender check.

### 11.2 Integration: the za suite

New sub-modules under `tests/src/za/`, after the pinning module (grants need
pins) and before the leak tests, so that the scan covers shared calendars:
`sharing.rs` for the behaviour and `sharing_races.rs` for the concurrency
cases (author's call on the split). They pin through the contacts endpoints,
write through CalDAV as each account, remove what they create
(`assert_is_empty` runs once at the suite's end), and use account names
unique across modules. New test hooks in
`crates/common/src/za/test_hooks.rs`, compiled only under `test_mode`:
`SHARE_COMMIT_PAUSE` (an endpoint after its decision, before its commit),
`SHARE_PRECHECK_PAUSE` (a first grant after its linked-event precheck,
before phase 1), `SHARE_REWRAP_PAUSE` (a conversion after phase 1, before
its parentage walk, and between two rewrap batches) and
`SHARE_REWRAP_FAIL_ONCE` (a conversion cut after phase 1) and
`SHARE_RENAME_PAUSE` (a rename of a shared calendar after its destination
lookup, before its commit) and `SHARE_BOUNDARY_PAUSE` (after the rename
commit, before boundary completion, 5.2), as independent pause hooks with
setters compatible with `HookParked`, beside the
existing `COMMIT_PAUSE` (a leased create, `update.rs:631`) and two new
one-shot pauses using the same `Pause` shape, keyed by the resource's
account: `UPDATE_COMMIT_PAUSE` (update PUT after fan-out and seal, before
`update.rs:405`) and `DELETE_COMMIT_PAUSE` (event DELETE after fan-out,
before `delete.rs:276`). Test helpers open rings and envelopes with an account's
password-derived keys to assert generations; they are test code and never
part of the product build.

Cases, with alice and bob (and carol and dave where named) as key
accounts:

1. Grant, view: alice pins bob and grants a calendar; bob's client finds it
   (assisted discovery on and off), reads its name and its events (`GET`,
   `calendar-query`, `calendar-multiget`, `sync-collection`), and gets 403
   for `PUT`, `DELETE`, `PROPPATCH` (a `calendar-color` one included),
   `MKCALENDAR`, `LOCK`, deleting the calendar and `ACL`. While bob syncs
   the granted calendar incrementally, alice deletes an event from an
   ungranted calendar (bob's sync shows no 404 for it) and one from the
   granted calendar (its 404 is present); a `REPORT` on the granted
   calendar returns only its own tombstones (5.2). Slug reuse (re-review
   finding 3): alice deletes an event from her private calendar at
   `/family/`, deletes that calendar, creates a new one at the same slug
   and grants it to bob; bob's sync from a token taken before the deletion
   answers 403 `valid-sync-token`, and his full sync shows nothing of the
   old calendar. A rename into a former slug (re-review finding 2,
   revision 6): alice grants bob her calendar at `/other/`, bob syncs and
   keeps the token, alice deletes an event of her private calendar at
   `/family/` and removes that calendar, then renames the granted calendar
   to `/family/`; bob's sync from the kept token answers 403
   `valid-sync-token`, and his full sync shows nothing of the old
   calendar. The paused rename (re-review finding 1, revision 7): bob
   holds a grant on alice's calendar at `/other/` and keeps a token; alice
   renames it to the vacant `/family/` with `SHARE_RENAME_PAUSE` holding
   the rename before step 1's commit, while another session of alice's
   creates a private calendar at `/family/`, creates and deletes an event
   in it and deletes the calendar; `SHARE_BOUNDARY_PAUSE` then holds the
   request between the steps, when
   bob's sync answers 403 `valid-sync-token` with the kept token and with a
   numeric token above every change id so far, and his initial sync (no
   token) is served; after the rename completes, a numeric token one above
   the last change id before the private writes answers 403
   `valid-sync-token`, and neither bob's full sync nor any later token
   admitted afterwards returns the private event's tombstone.
2. Grant, edit: bob creates, changes and deletes events and sets his own
   name, which alice's view does not show; his `calendar-color` change
   recolours the calendar in alice's view too (shared dead properties, as
   upstream); alice sees his events and keeps her own name.
3. The conversion: alice's existing events (some with attendees and token
   pairs) read for bob after the first grant; `rewrapped` equals their
   count; their content and pairs are unchanged; a reply to one of them
   still applies (release 2's tokens survive the rewrap). A second run
   shares alice's untouched default calendar (a plaintext owner entry) with
   a plaintext event planted in it (`plant_event`): the conversion seals
   both, `sealed` counts the event, and bob reads it.
4. Refusals: not pinned, key changed, pin record missing, not a key
   account (plain, unknown, pending, a group), oneself, too many shares,
   linked events, unknown calendar, a bad level, a missing permission.
5. Idempotency and changes: a repeated grant answers `changed: false`; a
   level change takes effect at bob's next request without a rotation; a
   revoke of an address without a grant answers `changed: false`; a grant
   to bob's alias (pinned separately) changes his level and leaves the
   ledger's address as recorded, and a revoke by that alternate alias, which
   resolves to bob's account and its one grant, removes his grant
   (re-review finding 5). The alias transfer (4.4): bob's grant records
   `family@` (an
   alias of bob's), carol also holds a grant, and the alias moves from bob
   to carol; a revoke by bob's grant id removes bob's grant and leaves
   carol's, and a revoke by the address `family@` answers 409 `address is
   ambiguous, revoke by grant` and removes nothing.
6. Revoke: bob's next request is refused, his home no longer lists the
   calendar, and the calendar has a new generation; alice's event written
   after the revoke carries it, and bob's saved grant ring does not list
   it; events written before it are untouched.
7. Rotation keeps readers: carol, a second grantee, reads events of both
   generations after bob's revoke; dave, granted later, reads every event.
8. Re-key: bob starts over with a new keypair (a test helper replaces
   it); alice's list shows `key changed`; after `contacts/pin` and a grant,
   bob reads every event again and the calendar has a new generation.
9. Delegate sending (6.2), with bob an edit grantee of alice's calendar,
   carol a key account bob has pinned, and dave a key account without a
   grant:
   - a delegated REQUEST: bob creates an event in alice's calendar with
     ORGANIZER alice and carol as attendee; carol's drain imports it, her
     copy's ORGANIZER carries `SENT-BY` bob, and the item's `changed_by` is
     bob; a `SENT-BY` naming dave that bob's client wrote is replaced by
     bob's;
   - a delegated update: bob moves the event's time; carol's inbox item
     carries `SENT-BY` bob, and her stored copy, imported from the
     delegated REQUEST, keeps its `SENT-BY` (an existing component keeps
     its ORGANIZER through a REQUEST update, 6.2);
   - `SENT-BY` per component (re-review finding 6): carol holds a copy of
     a recurring event of alice's, imported from alice's own REQUEST; bob
     adds a recurrence override through the shared calendar; carol's
     merged copy holds the new override with `SENT-BY` bob on its ORGANIZER
     while the master keeps alice's line without one;
   - a delegated CANCEL: bob deletes the event; carol's drain, after the
     deletion committed, applies the CANCEL;
   - `SENT-BY` across cancellations (6.2): a copy of carol's that alice's
     own REQUEST created gains `SENT-BY` bob when bob's delegated CANCEL is
     merged into it, and a copy that bob's delegated REQUEST created loses
     its `SENT-BY` when alice's own CANCEL is merged;
   - a delegated REPLY: bob, who is not an attendee of it, changes alice's
     PARTSTAT on an invitation from carol held in the shared calendar;
     carol's drain accepts it (`SENT-BY` bob on alice's ATTENDEE line,
     alice's token) and applies it; when bob instead sets alice's line to
     `DELEGATED-TO` bob, carol receives one REPLY as alice and bob's inbox
     receives nothing from that write (no second run on the attendee
     side, 6.2's gate);
   - carol's reply to the delegated REQUEST reaches alice's inbox and
     alice's drain merges it into the event in the shared calendar;
   - bob organizes under his own address from alice's calendar: the
     REQUEST is sent as bob with no `SENT-BY`, and carol's reply lands in
     bob's inbox as history (`EventNotFound`), the inherited limit;
   - the write-time authority: dave, without a grant, PUTs into his own
     calendar an event with ORGANIZER alice and carol as attendee; nothing
     reaches carol's inbox (alice's address is not local to dave's write);
   - the drain's consistency check: items planted in carol's inbox by a
     fixture are refused with the delegated reason string when the
     `SENT-BY` (bob) does not resolve to `changed_by` (dave), when the
     named address resolves to no account, and when it resolves to
     `changed_by` itself;
   - a revoke in between: with carol's drain skipped, bob writes, alice
     revokes bob, and carol's next drain applies the item, written while
     the grant stood;
   - a write that schedules nothing (an event without attendees, an alarm
     on alice's scheduled event) and a `PROPPATCH` of an event are stored
     and send nothing.
10. The owner organizes: alice invites bob and carol from the shared
    calendar; bob's drain imports his own copy into his default calendar
    (two copies, 6.3); bob's reply from his own copy applies to alice's
    event, which bob then reads in the shared calendar with his
    participation. Bob's PARTSTAT change from the shared copy instead is an
    update sent to carol as alice with `SENT-BY` bob and to bob's own
    inbox, whose drain merges it into bob's own copy (6.3). After any such
    organizer-side write of bob's on an event he attends, bob still holds
    his pair in alice's event, his own copy carries the new SEQUENCE, and
    his next reply from his own copy applies (neither refused as from a
    non-participant nor stale). The sole-attendee decline (re-review
    finding 4): in an event alice organizes whose only attendee is bob, bob
    declines through the shared copy without `SCHEDULE-FORCE-SEND`; upstream
    builds no message, yet bob's own copy converges to DECLINED after his
    drain. Updates and deletions (re-review finding 3, revision 6): bob
    edits only the SUMMARY of alice's event through the shared calendar (no
    SEQUENCE change), and his own copy shows the new summary after his
    drain and a refresh; bob deletes the shared event, and his own copy is
    cancelled after his drain and a refresh. The sole-attendee and removal
    cases (re-review finding 2, revision 7), each checked on bob's own copy
    after his drain and a refresh: in an event alice organizes whose only
    attendee is bob, bob edits only its SUMMARY through the shared calendar
    (upstream's builder returns `NothingToSend`), and his copy shows the new
    summary; bob then deletes that event through the shared calendar
    (upstream's CANCEL builder builds nothing), and his copy is cancelled;
    in an event alice organizes with bob and carol, bob removes his own
    ATTENDEE line through the shared calendar, carol's copy loses bob's
    line, and bob's copy is cancelled. Aliases (re-review finding 4,
    revision 6): bob creates through the shared calendar an event alice
    organizes, attending the master as `bob@` and an override as
    `bob-work@`; his inbox receives one self item per address, his own copy
    holds both pairs, and replies from both aliases apply at alice's drain,
    both after the self-import and after a later REQUEST replaced the
    tokens. A sole-attendee create (6.2's success rule): bob creates
    through the shared calendar an event alice organizes whose only
    attendee is bob, then edits its SUMMARY and deletes it; his own copy is
    created, updated and cancelled, each after his drain and a refresh.
    With `SCHEDULE-FORCE-SEND=REQUEST` on bob's line (lifecycle review
    finding 2, revision 10), alice's stored event no longer carries the
    parameter after the create, and the deletion's CANCEL merged into
    bob's copy carries none.
    Revision 9's alias-payload and CANCEL-SEQUENCE cases are replaced by
    case 25's differential form (re-review finding 1, revision 9).
11. The gate: a token with its own keys but no grant, an impersonation
    copy, a Bearer token, and a grant to a group planted in the store by a
    fixture are refused on the shared calendar; the group member's
    `PROPFIND` of alice's home succeeds and lists nothing (the resolver
    skips the refused calendar, 5.2). A grantee whose token carries the
    `Impersonate` permission (a role granting it) is refused at the gate on
    every request to alice's calendars, `PROPFIND propname`, `LOCK`,
    `UNLOCK`, a collection `MOVE`, a rename and a collection `COPY`
    included (5.1). Through each grantee's own home (re-review finding 2),
    with both a `propname` request and ordinary properties and with
    assisted discovery on: the `Impersonate` token lists nothing of
    alice's, nor does the group member of the planted group grant, while
    bob's own listing shows exactly his granted calendar. Bob reaches none
    of alice's unshared calendars (403 when addressed, absent from
    listings).
12. `COPY` and `MOVE`: an event `COPY` into or out of the shared calendar,
    and a collection `COPY` of it, answer 403; a rename keeps the grant;
    alice's event `MOVE` from an unshared calendar into the shared one is
    readable by bob, and once moved back out it is not; bob's `MOVE`
    between two calendars granted to him rewraps. The linked move (5.6):
    alice copies an event between her unshared calendars A and B, then
    moves its A link into the shared C; the `MOVE` answers 403, and the
    event keeps its names A and B under the EWK. A valid planted legacy
    plaintext event moves from an unshared source into an already converted
    shared destination: the stored tree and event are asserted together,
    the destination is its sole parent, its envelope is `0x03`, raw
    plaintext is absent from stored bytes, and owner and grantee can read
    it. Moving that now sealed event back to an unshared calendar rewraps
    its existing DEK under the EWK. A same-domain MOVE preserves its
    accepted stored bytes;
    partial or malformed carrier inputs are refused as corruption.
    In a separate future scheduled-event fixture, bob's MOVE between two edit-
    granted calendars overwrites an event in the destination: no CANCEL is
    built, queued iMIP task or pending item written, and its attendees' existing copies stay
    active, as in alice's own MOVE. A test-only
    `MOVE_OVERWRITE_OBSERVATION` records the actual overwrite deletion's
    returned inner scheduling-message boolean by owner and event id:
    normal overwrite is `Some(false)`, while the mutation that flips only
    `send_itip` to true must produce `Some(true)` for this scheduled fixture.
    Compare both runs' unchanged pending-item counts and queued scheduling
    tasks for owner and bob after synchronous MOVE completion, holding
    `TASK_CONSUMER_PAUSE` through `hold_task_consumer` and checking
    `itip_tasks_for`, `pending_ids` and `inbox_ids`; include an ordinary
    account control. These counts complement the return observation.
13. Recovery: alice recovers her account and bob changes his password;
    both keep reading.
14. Destruction: a destroyed grantee is listed as `gone` and removed by
    alice's next revoke, with a rotation, which writes the orphan share
    notification under the dead id (2.2); the case removes that
    notification before the suite's emptiness check and leaves no gone
    grantee for the leak scan. A destroyed owner, destroyed through the
    registry as the suite destroys accounts: at bob's next request his
    `calendar-home-set` (assisted discovery off), the assisted listing of
    his own home (on) and his depth-1 `PROPFIND /dav/cal/` all succeed and
    no longer name the owner, and no ACL index key under bob's id names the
    owner (8.6). The race (8.6): with `SHARE_COMMIT_PAUSE` holding alice's
    grant to carol after its checks, alice is destroyed through the
    registry; the grant resumes and answers 409 `account deleted`, no ACL
    index key under carol's id names alice, and carol's discovery works
    after a fresh token.
15. Free-busy: bob's `free-busy-query` on the shared calendar returns no
    busy periods.

Concurrency, in `sharing_races.rs`:

16. Invalidation after the commit: with `SHARE_COMMIT_PAUSE` holding
    alice's revoke, bob makes a request (his token is rebuilt from the old
    grants); after the release and the 200, bob's next request is refused.
    The same for a grant: bob's next request after the 200 sees the
    calendar.
17. Resident keys: after a grant, after a revoke, and after a same-level
    re-key (bob's recorded key replaced with the bits unchanged, 4.6),
    bob's key-cache entry is gone on the node (the cache contents and
    `vault::session::live::session_keys`), and his next request
    re-verifies; after a rotation so is carol's, a remaining grantee.
18. The vault fence: with the pause holding a grant, alice's password
    changes; the grant answers 409 `vault changed since verification` and
    nothing is written.
19. The pin fence: with the pause holding a grant, `contacts/forget`
    removes bob's pin; the grant decides again and answers `contact not
    pinned`.
20. Two grants racing on one calendar (bob and carol), and a grant racing
    a revoke: both outcomes land, each grantee with exactly one entry,
    ring and ledger record.
21. A `PUT` across a conversion: with `SHARE_REWRAP_PAUSE` holding the
    first grant after phase 1, alice's other device writes an event; in a
    second run, a `PUT` that read the calendar before phase 1 is held by
    `COMMIT_PAUSE` for a create, then in a separate variant by
    `UPDATE_COMMIT_PAUSE` for an update, until after it. Both variants run;
    every event ends `0x03`.
22. A `PUT` across a revoke: in two independent runs, alice's and bob's
    updates are each held by `UPDATE_COMMIT_PAUSE` while alice revokes carol
    and each commits under the new
    generation; when bob is the one revoked, his held `PUT` answers 403
    and stores nothing. A `DELETE` across a revoke (5.5): bob's `DELETE` of
    an event, held by `DELETE_COMMIT_PAUSE` before its commit while alice revokes
    bob, answers 403 and the event stays; a `DELETE` held across a calendar
    `PROPPATCH` by alice commits on its retry.
    Both hooks run after fan-out: items already delivered by a held write
    remain delivered when its commit is later refused with 403 (4.6).
    Assert no event mutation, not an empty recipient inbox.
23. An interrupted conversion: `SHARE_REWRAP_FAIL_ONCE` cuts the first
    grant; alice reads every event; the next grant completes the rewrap
    and the grant. Direct converter tests inject opaque assertion loss into
    phase 1, calendar and event chunks, the `reverting` ring and final
    unshared rewrite, both with unchanged guards and raced vault/Account
    guards; each reports typed conflict, committed counts and progress,
    and the next request resumes from its journal state. HTTP coordinator
    tests own terminal-guard precedence, the eighth-loss 409 `conflict`,
    and the 3,000-event budget succeeding across later chunks.
24. A link made after the precheck (3.5): with `SHARE_PRECHECK_PAUSE`
    holding alice's first grant after its linked-event precheck, alice's
    other device copies an event of the calendar into another of her
    calendars; the grant resumes and answers 409 `calendar has linked
    events` with no `acls` entry, grant ring or ledger record written and
    the calendar back in its unshared form (3.5's revert), an event alice's
    other device wrote between phase 1 and the revert back under the EWK
    and readable; an ordinary `PUT` of the linked event then succeeds under
    the EWK and both links stay readable. In a second run the conversion is
    cut right after phase 1 (`SHARE_REWRAP_FAIL_ONCE`), leaving the shared
    form with `converted` unset and the linked event under the EWK: an
    ordinary `PUT` or `PROPPATCH` of that event through either of its paths
    answers the write rule's 409 (5.5, re-review finding 1), both links stay
    readable, and after alice's `DELETE` of the other copy a new grant
    completes. In a third run (re-review finding 1, revision 6) one of
    alice's DAV sessions (one app password, so one resident `SessionKeys`)
    is kept across a rollback and a reconversion: with `SHARE_REWRAP_PAUSE`
    holding the first grant after phase 1, that session reads the calendar's
    properties, which opens the owner ring and caches generation 1 of the
    first conversion's CK; the grant resumes, finds the raced-in link and
    reverts; alice deletes the other copy and grants again, which converts
    with a new key-domain id and a new generation-1 CK; the same session
    then reads the calendar and creates a new event, and a freshly
    authenticated session of alice's reads that event and the calendar.
25. The differential lifecycle test (re-review recommendation, revision
    9; 6.2's contract; it replaces case 10's revision 9 alias and
    CANCEL-SEQUENCE cases), in `sharing.rs` though numbered after the
    concurrency cases, so that the earlier numbers stay. The same edit
    sequence runs twice, against two fixtures with separate UIDs: in run A
    alice writes in her own session and bob, a key account, is an ordinary
    invited attendee; in run B bob writes through his edit grant on
    alice's shared calendar. Carol, a third key account, attends in both.
    After every step the test drains bob (`trigger_drain`,
    `tests/src/utils/za.rs:1254-1267`), fetches bob's copy in both runs
    (`copy_href`, `:1281-1305`), normalizes both and compares them.
    - Normalized: DTSTAMP, UID and href; `SENT-BY` on ORGANIZER (per
      component, delegated messages only); LAST-MODIFIED and CREATED
      unless both runs PUT identical bodies.
    - Compared: the component set, the RECURRENCE-IDs, SEQUENCE (never
      normalized: comparing it is the point), STATUS, every ATTENDEE line
      with its parameters (PARTSTAT and `SCHEDULE-STATUS` included), RRULE
      and the dates; the token pairs at store level, by presence per
      address only (tokens are minted fresh, `fanout.rs:977-990`).
    - The steps, each asserting equality: a create with bob on the master
      and on an override; a master SUMMARY edit; a DTSTART edit; an
      override added that bob attends; bob removed from that override
      while carol stays; a master SUMMARY edit again (6.2's inherited
      table, row 1, shows in both runs: the test asserts equality, not
      cancellation); bob re-added to the override; an override bob
      attends deleted; bob's line removed from the master while carol
      stays; bob re-invited; the event deleted.
    - A second fixture runs the alias steps of revision 9's case 10, with
      bob attending as `bob@` and `bob-work@` (a master-only SUMMARY edit
      with the override attended through the other alias, `bob@` removed
      from the master while `bob-work@` attends the override, `bob@`
      replaced by `bob-work@` on the master), in the same differential
      form, asserting equality only.
    - A distinct sub-address fixture has `bob@` and carol on the master,
      and `bob@`, unlisted `bob+x@`, and carol on override O1. Confirm the
      configured resolver maps `bob+x@` to bob. Steps in both runs:
      create; master SUMMARY edit; DTSTART edit; remove `bob+x@` from O1;
      re-add it to O1; delete O1; delete the event. The re-add makes both
      Bob lines present in the prior O1 when that override is deleted.
      Preserve upstream payloads: removal's partial CANCEL has only the
      unlisted line and is rejected in both runs; O1 deletion's CANCEL has
      both lines and is accepted. Do not add lines to exported messages.
      Compare per-step inbox message/address counts as multisets (partial
      groups iterate in hash order), token-address presence and copies.
      Also compare `SCHEDULE-STATUS` on every R line in the owner's stored
      event after each surviving write, including before DELETE, using
      `schedule_statuses` (`tests/src/utils/za.rs:813`). REQUEST export
      strips that status, so the recipient's copy cannot expose a refused
      first-run duplicate attempt. After DELETE, compare absence instead.
      A sub-address-only variant compares inbox delivery and the identical
      incoming rejection when neither payload has any listed local line;
      it does not broaden receiver validation. A group address shared with
      alice stays local and gets no self item in either run.
    - A writer-only `team@` group fixture (bob a member, alice not) is
      server-scheduled with ordinary RSVP/PARTSTAT. Compare the owner's
      stored `5.3` status in both runs and no item redirected to bob. With
      DECLINED or `RSVP=FALSE`, both runs suppress it: no override and no
      added status. A
      sole-group create exercises first-run `NothingToSend` and second-run
      `Ok`: the group attempt is retained, the schedule tag is set and
      finalize runs once. Also compare a listed Bob plus unlisted owner
      `alice+y@` fixture: the owner-resolving address records `5.3` and
      receives no item in both runs.
      A separate ORGANIZER fixture uses a group both alice and bob hold:
      it enters the delegated-organizer path, has no `SENT-BY`, and its
      delivered item is refused by the ordinary sender check because the
      named group id differs from bob's `changed_by`. An attendee-side
      REPLY for unlisted `alice+y@` records the inherited owner-inbox
      delivery difference without changing the delegated-organizer rule.
    - A fixture of its own runs a line of bob's with
      `SCHEDULE-AGENT=CLIENT` and one with `NONE` (lifecycle review finding
      1, revision 10), each through create, drain, DELETE, drain: nothing
      is delivered in either run and bob holds no copy; equality is
      asserted.
    - A sole-attendee fixture (lifecycle review finding 2, revision 10),
      without carol, so that the first run returns `NothingToSend` (the
      main fixture's third attendee makes the first run succeed and hides
      this path): bob's line carries `SCHEDULE-FORCE-SEND=REQUEST`, and
      the create, drain, DELETE and drain steps compare every ATTENDEE
      parameter of bob's copy.
    - A separate run asserts the delta of 6.2's one exception: bob's line
      DECLINED, `RSVP=FALSE`, and a line not NEEDS-ACTION on a create and
      on a new override: run B delivers and run A does not, and bob's copy
      in run B shows the change (revision 11 withdraws `CLIENT` and `NONE`
      from this run).
      This exception fixture uses directly owned listed A aliases only. Separate
      unlisted sub-address, custom-rule and catch-all addresses resolving
      to bob run the same DECLINED/RSVP=FALSE and non-NEEDS-ACTION cases:
      both runs suppress those recipients equally. Compare target-specific
      attempts/statuses, not unrelated listed attendees' REQUESTs. A
      writer-organizer unlisted sub-address remains on upstream's attendee
      path with no second run, and keeps the ordinary own-inbox exclusion.
    - Harness: `DrainOn` (`tests/src/za/drain.rs:51-66`; the suite skips
      drains by default, `tests/src/za/mod.rs:121-124`); the fan-out is
      synchronous inside the PUT and the DELETE (`update.rs:354-374`,
      `delete.rs:255-273`), so no `wait_for_delivery`, which hangs on an
      alarm not yet due (`tests/src/utils/za.rs:625-647`); the events end
      in the future (`crates/groupware/src/calendar/itip.rs:1092-1093`); a
      pass that leaves an item pending defers the next by 30 s, and a
      pass is capped at 32 items; everything created is deleted for
      `assert_is_empty`. The comparison helper, modelled on upstream's
      private `normalize_ical` (`tests/src/webdav/cal_itip.rs:420-451`),
      is new test code (13.1).

    **The stopping criterion** (re-review recommendation, revision 9):
    the self-message design is complete when no edit sequence
    demonstrates a difference between the two runs beyond 6.2's one
    exception, and every behaviour the owner's own write also produces is
    recorded in 6.2's inherited table. A later review's finding is judged
    against it: a divergence between the runs is a defect; an inherited
    behaviour is a row of the table.

### 11.3 Key-mode CalDAV suite

`ZA_KEY_ACCOUNTS=1 webdav_tests` runs the `acl` sub-module as the key-mode
variant in `tests/src/webdav/za_variants.rs`, which today asserts that the
`ACL` method on a key account's calendar is refused and that another key
account cannot read it. The `ACL` refusal stays; the variant now also
grants through `shares/grant` (pinning first) and asserts that the grantee
reads the calendar, that a view grantee cannot write and an edit grantee
can, and that a revoke ends access; its group-calendar half is unchanged.
Release 1's test-plan rows "acl (grants refused)" and "sharing refused"
(its section 11) become "the `ACL` method refused; grants through the
endpoints". The `copy_move` variant is unchanged (its calendars are not
shared); the single-parent refusals are a za case (12).

### 11.4 The leak scanner

`tests/src/za/leak.rs` gains the sharing rules, each with a negative
control that must fail:

- **Every CK wrap belongs to its owner or a current grantee.** A calendar
  in the shared form has an owner ring in the owner's entry; every other
  entry belongs to an account listed in `acls`, holds a grant ring whose
  header names that account and this calendar, and lists exactly the
  generations the owner ring lists; every account in `acls` has such an
  entry and is a current key account, or a gone grantee awaiting its sweep
  (7.5; the suite's cases leave none at scan time, 11.2 case 14); every
  `acls` entry's bits are a level's. Controls: an entry for an account not
  in `acls` (a stale wrap); an `acls` entry without an entry; a grant ring
  missing a generation.
- **Every event DEK of a shared calendar is wrapped under a CK the
  calendar lists.** An event whose parent is a shared calendar has exactly
  one parent and a `0x03` envelope naming that calendar and a listed
  generation; an event of an unshared calendar has no `0x03` envelope.
  Controls: a `0x01` event planted in a shared calendar; a `0x03` envelope
  naming an unlisted generation; an event with two parents, one of them
  shared.
- **Sealed entries.** In a shared calendar the owner's entry has the
  five-part form and every grantee's entry the three-part form, each with
  empty `description` and `color` and a custom time zone that passes the
  tree check; the calendar's `dead_properties` field is empty. This
  replaces, for shared calendars, release 1's rule that a sealed collection
  holds only the owner's entry, which still applies to unshared ones.
- **Canaries.** The owner's and the grantees' names and the calendar's
  dead properties (a colour among them) are canaries in the whole-store
  scan, like event content.

The scanner checks structure, not cryptography: it reads the visible ring
headers and envelope fields, and the cases of 11.2 open the wraps with the
test helpers.

### 11.5 Real-device check

`docs/zero-access/manual-checklist.md` gains a sharing section, run with
`enableAssistedDiscovery` on. Apple Calendar on macOS and on iOS, each
logged in with the grantee's app password, shows the owner's shared
calendar under the grantee's account, through the fork's gate, and syncs
it; a view grant is read-only; an edit grant's changes reach the owner's
devices and back; an edit grantee's colour change recolours the calendar
on the owner's devices too (shared, as upstream), and a view grantee's
colour change is refused with 403, recording what Apple does with it
(keeps it locally, reverts it, or shows an error); the grantee adds
invitees to an event in the shared calendar and they receive it as the
owner's invitation, recording whether Apple offers invitees there at all
and how the recipient's Apple Calendar shows the `SENT-BY`; the grantee
answers an invitation from the shared copy instead of its own, and after
the grantee's drain and a client refresh both copies, the owner's and the
grantee's own, show the new status (6.3; review finding 9), with the
client's immediate presentation before the refresh recorded separately,
and the same for a grantee who is the event's only attendee and declines
from the shared copy without `SCHEDULE-FORCE-SEND` (its own copy ends
DECLINED, re-review finding 4), while the checklist still tells an
attendee to answer from its own copy;
an alarm the grantee adds to the owner's scheduled event is
stored; dragging an event between the shared calendar and the grantee's
own calendars, and between the owner's shared and unshared calendars,
recording what Apple sends and shows; a revoke removes the calendar at the
next refresh and a new grant brings it back; the owner renames the
calendar. Then Thunderbird with assisted discovery off finds the calendar
through its second home-set href. DAVx5 is optional.

### 11.6 Mutation checks

In the style of plan 7, each failing the named case: invalidating before the
commit (16); no access-token invalidation (17); no vault assertion (18); no
pin-record assertion (19); no calendar assertion on the endpoint's write
(20); no calendar assertion on event writes (21 and 22); no rewrap in the
conversion (3); no rotation on revoke (6); rotation wrapping to the served
key instead of the ledger's (8, with a remaining grantee's served key
substituted during a revoke); the resolver honouring `is_member` or skipping
its bit check (11); a refusal inside a listing failing the request instead
of skipping the calendar (11); the gate accepting a token without its own
keys (11); no `SENT-BY`-to-`changed_by` check in the drain's delegated
branch, which lets the planted item naming bob with `changed_by` dave
through (9); no `SENT-BY` placement, which makes the delegated REQUEST fail
release 2's sender check at carol's drain (9); attendee locality judged by
the merged list, which drops bob's pair from alice's event so that his later
reply is refused (10); the writer's own inbox excluded from a delegated
fan-out, which leaves bob's own copy at the old SEQUENCE so that his later
reply is stale (10); no single-parent rule (12); a `MOVE` into a shared
calendar that keeps the event's other names (12); the share batch without
the registry Account assertion (14, the race); the endpoint's invalidation
through `refresh_acls` alone, which misses the same-level re-key (17); an
event `DELETE` without the calendar assertion (22); revoke resolving an
address through the directory before the ledger (5); the gate admitting a
token with `Impersonate` (11); `sync-collection` without the vanished-path
filter (1); phase 2 rewrapping before its parentage walk (24); the write
rule dropped (24, second run); assisted discovery without the eligibility
check (11); the token bound check removed (1, slug reuse); self-delivery
only when the builders produced messages (10, sole-attendee decline); the
cache keyed without the key-domain id (24, third run); the rename not
advancing the boundary (1, rename into a former slug); the boundary taken
from a change id read before the rename's commit (1, paused rename); no full
sync while the boundary is pending (1, paused rename); no self-CANCEL (10,
deletion); one self item for all aliases (10, aliases); the second run under
the merged list, which delivers nothing to bob (25, every step); the second
run placed after the first run's finalize, which sends bob SEQUENCE n+2 (25,
the DTSTART edit); upstream's public entry point run on the real tree for
the second run, a double increment (25, the DTSTART edit); `force_send` set
alone, so that an `RSVP=FALSE` writer gets nothing (25, the exception's
run); the schedule tag left unset when only the second run built, so that
the sole-attendee create's later deletion builds nothing (10, sole-attendee
create); bob delivered from the first run's output, which gives him nothing
or a foreign partial group's lines (25); the second run on the attendee
side, which builds a delegate invitation the fan-out never sends
(`attendee.rs:256-271`, `fanout.rs:902-911`; 9, the delegation through the
shared copy, and planning records it as equivalent if nothing of it is
observable); the differential test's SEQUENCE normalized, which lets the n+2
mutation through (25); the scheduling agent overridden (25, the `CLIENT` and
`NONE` fixture: a `CLIENT` line gets a REQUEST in run B and no copy in run
A, so the create step fails); the reconciliation without the finalize (25,
the sole-attendee fixture, and 10, sole-attendee create: the stored line
keeps `SCHEDULE-FORCE-SEND`, so the deletion's CANCEL differs between the
runs); a new grantee given only the newest generation (7); a grantee's dead
property written into the owner's personal bundle (2, with the unit test of
11.1); a plaintext event skipped by the conversion (3); writer recipients
classified by exact listed-address membership (25, sub-address item counts
and token presence); first-run writer recipients left in the combined
output (25, the owner's stored per-line `SCHEDULE-STATUS`); self-inbox
admission based on account resolution without retained-second-run
provenance (a server-backed negative routing za fixture with an ordinary-provenance item to
the writer's own account); exception applied to R or all L instead of A
(25, unlisted sub-address/custom/catch-all and declined writer-only group
negative exception runs); writer-only group omitted from R (25, `team@`
owner status and sole-group finalize); quiet catch-all fallback replaced
by a traced lookup (11.1, cold and warm canary controls and cache proof);
MOVE overwrite scheduling enabled (12, the returned-message observation
plus pending-item and queued-task counts).

## 12. Invariants for implementers

Release 1's ten, release 2's five (11 to 15), key pinning's eight (16 to
23) and mail's nine (24 to 32), with mail's two amendments, hold. Release
2's invariant 11 is unchanged: it governs `pk` key envelopes (`X-ZA-KEY`
wrap type `0x02`), and the grant wraps of this release are a different
object under their own purpose and rule (34). Release 2's rule on who
sends, the drain's sender check that invariant 15 counts among the drain's
own checks (release 2 section 6.4, step 2: an item's ORGANIZER or acting
ATTENDEE resolves to its `changed_by` principal), is qualified, not
relaxed: it holds unchanged for every item without `SENT-BY`, and an item
whose naming property carries `SENT-BY` passes only by 6.2's consistency
check (its `SENT-BY` resolves to `changed_by`, a key account other than
the named one), with the write-time ACL as the authority (38). This spec
adds:

33. A collection key exists in plaintext only in process memory, in a
    request or session that holds the session keys of the calendar's owner
    or of a current grantee, inside `SessionKeys` under release 1's
    key-cache rules. The one exception is a cluster node that missed a
    revoke's broadcast (4.6): there a removed grantee's resident session
    keys keep the collection keys they cached, unusable through the server
    (the use-time checks refuse it), until the key cache drops them, at
    most its 60-minute cap. It is never written to the store unwrapped,
    nor to a log, a trace or an error.
34. Every wrap of a collection key names one principal and opens only for
    it: the owner's copy under the owner's EWK, a grantee's copy to the
    public key the owner's ledger records for it, which equalled the
    owner's pin and the served key when the grant or its re-key was made.
    The associated data binds the owner, the calendar, the conversion's
    key-domain id, the principal and the generation, and a session caches
    a collection key only under its key-domain id. No wrap is made to any
    other key, and only the principal's own session opens it.
35. The share endpoints are the only writers of a key account's calendar
    `acls` and of collection-key wraps (apart from deleting the calendar or
    the account, and from the sync boundary's writes, which rewrite ring
    headers and reseal the owner ring's payload with its keys unchanged,
    5.2). Each grant, level change, re-key, revoke and sweep is one
    conditional write in the owner's freshly verified request (a grant
    followed by its boundary's completion write, 4.5), asserting
    the calendar archive, the owner's vault record at the verified
    revision and the owner's registry Account item, and the pin record for
    a grant or re-key; the caches of every grantee whose entry it wrote,
    and of every remaining grantee after a rotation, are invalidated only
    after the commit. A grant is published only after the conversion's
    complete, valid scan, and a revoke names its grant by id.
36. An event of a shared calendar has that calendar as its only parent and
    its DEK wrapped under a generation that calendar lists; an event of an
    unshared calendar is wrapped under the owner's EWK. Every write of a
    key account's event seals under the newest generation of its
    calendar's key domain and asserts the calendar archive it read, and no
    write seals or rewraps an event whose names include a shared calendar
    and another calendar. Conversion completes its full parentage walk
    before any CK rewrap; finding a linked event starts rollback. A
    terminal guard, exhausted retry budget or store error may leave a
    safe readable unconverted/reverting journal, which the next grant
    re-evaluates and resumes under 3.5 before any grant is published.
37. A revoke rotates in the same write: no write committed after a revoke
    is sealed under a generation the removed grantee held. Ciphertext
    already stored is never resealed by a revoke, and no generation is
    dropped while the calendar exists.
38. Every scheduling message a session sends names, in ORGANIZER or the
    acting ATTENDEE, either an address of the session's account or, with
    `SENT-BY` an address of the session's account, an address of an
    account whose shared calendar the session's account holds an edit
    grant on. The authority for the second case is the write-time ACL, as
    upstream's; the recipient's drain checks, as defence in depth against
    server bugs and not as the authorization, that the
    `SENT-BY` resolves to the item's `changed_by`, a key account other than
    the named one. The fan-out seals with the session's own keys and pins,
    computes token locality against the event account's own addresses,
    and gives the writer's own copy exactly the messages upstream's
    organizer builders would address to writer recipients R had the
    owner made the same write in her own session, with the writer an
    ordinary attendee, the RSVP and PARTSTAT filters lifted for A alone.
    R is L union addresses resolving to the writer, excluding owner-local
    addresses; A is L intersected with directly owned listed writer
    aliases, excluding membership groups and independent of lookup success.
    Only retained-second-run provenance plus writer-account
    resolution and normal delivery checks admits a self item, one per
    address as its token carrier (6.2). Other retained recipients keep
    normal routing, and the event owner's inbox stays excluded on the
    delegated-organizer path. The
    UID lease and the SEQUENCE floor of a shared calendar's event are
    keyed by the calendar's account.
39. Sharing never yields another account's account keys: a session holds
    only its own MK, EWK, mail wrapping key and private key, and the
    collection keys of the calendars it owns or is granted. A grantee never
    learns the owner's MK, EWK or private key, nor the owner the
    grantee's; `za_keys_for` is unchanged.
40. A grantee's rights on a key account's calendar come only from the
    calendar's `acls` entry for the grantee's own account id, at the view
    or edit level. Group membership, impersonation, the `Impersonate`
    permission and the `is_member` shortcut never reach a key account's
    calendar, and `Share`, `Delete`, `SchedulingReadFreeBusy` and the
    unenforced bits are never granted on one.

## 13. Open items for planning; alternatives considered

### 13.1 Open items for planning

Revision 2 also made these precisions, beside the four changes its header
names: rotation holds against a store reader, not a writer who restores a
pre-revoke archive (2.1); a lost assertion in an event write re-reads the
calendar to tell which assertion lost (5.5); a calendar the resolver
refuses inside a listing is skipped, the resolver reads only the token's
own `acls` entry, and `MKCOL` is refused before any resolution (5.1, 5.2);
a sweep of a destroyed grantee writes an orphan share notification (2.2,
8.6); a cluster needs pubsub for grants and revokes to reach every node
(2, 4.6, invariant 33); a grantee's session opens the organizer's RSVP
token pairs of the events it rewrites (3.6); grant and revoke match an
existing grantee by its resolved account id, so aliases work (4.3, 4.4);
and the `acls` correspondence counts non-owner entries only, while the
leak scanner tolerates a gone grantee awaiting its sweep (3.3, 11.4).

Revision 4 adds these precisions: a calendar write that loses its archive
assertion answers upstream's 409, with no recommit (5.5); `mkcol.rs`
keeps its own `za_session_keys` check (5.2); the shared bundle counts in
8.7's sizes and moves to the newest generation at the next dead-property
write (7.2); 5.3's `DELETE` row says what 6.2 says; a stored copy keeps
a `SENT-BY` only when the delegated REQUEST created it (6.2, 2); a refused
delegated delivery's `3.8` has no remedy on the owner's page (6.2, 4.8);
the `SENT-BY` placement scrubs the naming property only (6.2, point 1); a
REPLY as the owner is nondeterministic upstream when the writer also
attends (6.2); a refused delegated item has its own trace reason (10); the
owner cannot reschedule or cancel an event a grantee organized under its
own address (6.2, point 4); and a delegated item merges with the named
address as its sender (item 10).

Each item below is for planning to settle in code; the recommendation
stands unless the code says otherwise.

1. **Names and constants.** The endpoint names; the wrap type bytes
   (`0x03` envelope, `0x04` owner ring, `0x05` grant ring); the purposes
   `ck-owner`, `ck-grant`, `ck-dek`, `ck-bundle`, `ck-calendar-bundle`,
   `ck-shared-dek` and `ck-shared-bundle`; `ZaOwnerRing` and `ZaGrant`;
   the rewrap batch size (100); the attempt count (8, key pinning's); the
   trace strings; the 409 reasons revision 5 adds (`account deleted`,
   `address is ambiguous, revoke by grant`); the `reverting` flag, the
   grant change id's width and place in the grant ring's header, and the
   write rule's trace string (revision 6); the boundary-pending flag's
   place in both ring headers and the request point of the owner's
   boundary repair (revision 8); the test hooks, among them
   `SHARE_PRECHECK_PAUSE`, `SHARE_RENAME_PAUSE` and
   `SHARE_BOUNDARY_PAUSE`.
2. **The calendar assertion on event writes** (5.5): the assertion class
   (the archive's version, as upstream's `update` asserts it, or a hash of
   the bytes read) and its place before the set on PostgreSQL and MySQL.
   On `AssertValueFailed`, which does not name the key that lost, the write
   re-reads the calendar: a changed version means the calendar assertion
   lost, and the write rewraps the envelope under the newest generation and
   recommits once; an unchanged one means the other assertion lost and
   takes its existing branch (upstream's 409 for the event's own version in
   an update, `za_lost_create_fence` for the UID lease fence in a create).
   Confirm that the rebuilt batch's other parts (the alarm task, the index
   builder's own assertion on the event, a create's lease fence) are carried
   unchanged, and that the drain's commit treats any loss as release 2
   section 6.5's transient error.
3. **Where the resolver lives** (`crates/dav/src/common/za.rs` for DAV, a
   helper in `groupware` shared with the drain and `contacts/pin`), the
   seal functions' new key argument (a key domain instead of `SessionKeys`
   alone: `seal_event` today requires `keys.account_id == account_id`), and
   the shape of the CK cache in `SessionKeys`.
4. **The extra calendar read** per event write (5.5): whether a hint in
   the resource cache (built keyless from the stored archives, release 1
   section 8.2) can spare it for unshared calendars without reopening the
   conversion race. Recommendation: read it; the cost is one key lookup.
5. **The grantee's entry**: a snapshot of the owner's at the grant (3.3,
   chosen), or an inherit marker that follows the owner's live values
   until the grantee's first personal change (upstream's behaviour), at
   the cost of a marker in the grantee's bundle and a merge in the view.
6. **How the list finds calendars shared with the caller**: through the
   caller's `access_to` and each key owner's resource cache, opening the
   caller's grant ring per calendar; the owner's address is the
   directory's.
7. **Client behaviour** for the real-device check (11.5): whether Apple
   Calendar offers invitees in a shared calendar at all, how it shows a
   received invitation's `SENT-BY`, what it does when a grantee answers
   from the shared copy, what it sends when an event is dragged between
   calendars, how it treats `write` without `bind` and `unbind`, and what
   it does with a view grantee's colour change, which upstream refuses
   (`PROPPATCH` needs `Modify`). Recommendation: checklist items, no code.
8. **A grant's request time** on a large calendar (3.5): measure the
   conversion on a calendar of a few thousand events and state the figure
   in api.md.
9. **The fan-out's event account** (6.2, point 2): `za_fan_out`
   (`crates/groupware/src/calendar/fanout.rs`) today derives everything
   from `sender: &AccountInfo` (the writer's merged addresses) and `keys`.
   The UID lease is taken and the SEQUENCE floor runs in `update.rs` on the
   resource's account already, and only the lease's fence reaches the
   fan-out. A delegated write needs the event's account, the owner's,
   passed beside `sender` for three things: the token union and the
   persisted pairs computed against the owner's own addresses; the split of
   merged sender for upstream routing, unmerged writer addresses for
   `SENT-BY` (including groups) and direct alias expansion for A (without
   groups), plus owner-only addresses for locality and snapshots; and the
   writer's self-delivery, with
   `resolve_inner`'s exclusion of the sender's own inbox carved out for it.
   How the writer's lines are served is decided (re-review finding 4;
   re-review findings 3 and 4, revision 6; re-review finding 2, revision
   7; re-review finding 1, revision 8; re-review finding 1 and
   recommendation, revision 9): by parity, the second run of 6.2, point 2,
   which replaces revisions 8 and 9's self-message builder. Planning
   carries out: the second run in fork code in
   `crates/groupware/src/calendar/`, calling upstream's `pub(crate)`
   builders on snapshots taken under the owner's addresses, with two
   routing fields (`force_send`, `rsvp`) set on A entries only and
   `is_server_scheduling` left as the line says (revision 11); the split
   of `itip_cancel`
   into a snapshot-taking `pub(crate)` function behind the existing entry
   point, with no behaviour change; the owner's addresses fetched at the
   three sites (the update and the create in `update.rs`, the DELETE loop
   in `delete.rs`); the "either run built" rule at the fan-out's call, at
   the schedule tag and, when only the second run built, at the committed
   tree's finalize (revision 11); writer recipients R (the listed and
   resolved union), their removal from first-run output, and a
   provenance-keyed self-inbox carve-out with writer-account resolution
   and the ordinary checks (revision 13); and the normalizing comparison helper of
   11.2's
   case 25, modelled on upstream's private `normalize_ical`
   (`tests/src/webdav/cal_itip.rs:420-451`), as new test code. Case 25's
   stopping criterion (11.2) decides whether a later finding is a defect
   or a row of 6.2's inherited table. The parameters' names and the call
   sites (`update.rs`, `delete.rs`, the calendar `DELETE` per event) are
   planning's; the owner's and a plain writer's calls pass their own
   account and make no second run.
10. **The drain's delegated branch** (6.2, point 3): the `SENT-BY` parsing
    (a `mailto:` URI, compared after key pinning section 4.3's
    normalization) and its resolution to `changed_by`, the key-account test
    of `changed_by` and of the named address, both through the existing
    quiet address lookup, and no read of the owner's account. For the
    merge, the item's sender is the named (owner's) address, not the
    `SENT-BY` one: upstream's `handle_reply` looks the replying attendee up
    by it, and the token match runs over the owner's addresses.
11. **Per-user colour and order for Apple's namespace**, a later
    refinement: release 4 keeps upstream's shared dead properties (1.2).
    Giving each principal its own `calendar-color` and `calendar-order`
    would take a per-principal dead-property set (a part of each personal
    bundle), a split in `PROPPATCH` that routes those two properties to the
    writer's own set, and a merge in the `PROPFIND` view that overlays the
    caller's set on the shared one; and a decision whether a view grantee,
    which lacks `Modify`, may write its own set. It invents behaviour
    upstream does not have, so it waits for a product reason.
12. **Replies to a grantee-organized event**, a later refinement: the
    drain could look a REPLY's UID up, after the session's own account,
    in the calendars shared to the session's account with edit, and merge
    it into the event there under that calendar's CK, so that a grantee
    organizing under its own address from the owner's calendar receives its
    attendees' answers (6.2, point 4). Release 4 keeps upstream's lookup in
    the recipient's own account.

### 13.2 Alternatives considered

- **Refusing a grantee's scheduling writes** (revision 2's rule): a
  grantee's write that touched an ORGANIZER or a protected ATTENDEE line,
  or that upstream would announce, answered 412
  `allowed-organizer-scheduling-object-change`, so only the owner ever
  organized from a shared calendar. Not taken: it refused what upstream
  allows an edit grantee, which is what users expect of a shared calendar.
  What it saved is what delegate sending costs: the fan-out's event-account
  parameter, the drain's delegated sender check, the `SENT-BY` placement,
  and their tests.
- **Re-checking the grant at the recipient's drain** (revision 3's rule):
  the drain looked up the owner's event of the item's UID, its calendar
  and that calendar's `acls`, and accepted a delegated item only on the
  sender's current edit grant there. Not taken: it authorized nothing the
  write-time ACL had not, a revoke between the write and the drain lost
  the message for good (a CANCEL for a deleted event most of all), and its
  branch for a create still in flight read the UID lease after the index
  with a race between the two reads. The drain keeps a consistency check
  instead (6.2, point 3).
- **Binding a recipient's copy to its source calendar** (hardening against
  6.2's inherited UID collision, not taken): the delegated REQUEST would
  carry the source calendar's id in the copy's `X-ZA-EXTRA` bundle (a
  `FORMAT_V3`, four bytes per copy), and the drain would apply a later
  message only from the same calendar. A `MOVE` between the owner's
  calendars would leave copies bound to the old one until the owner's next
  message, and upstream has the same hole for every grantee; recorded, not
  adopted.
- **Groups through a group keypair** (the sketch's section 6): a group
  private key wrapped to each member and rewrapped by a member session on
  every membership change, since the server cannot do it alone. A feature
  of its own, decided on demand.
- **HPKE** for the grant wrap. Rejected as in release 2 and mail: one
  more dependency and a format the fork does not control, where release
  2's X25519 composition already serves.
- **Resealing on revoke** (7.3). Deferred; the format allows it later.
- **Free/busy first** (the sketch's section 7): a free/busy-only grant
  needs no key, only a policy change to the `Withheld` arm. Deferred: it
  needs the page to set it up, and the family deployment's grantees read
  the whole calendar anyway.
- **Granting through JMAP** (`shareWith`): JMAP calendars are refused for
  key accounts until release 6, and `Calendar/set` has no vocabulary for
  the pin prompts. Release 6 can route `shareWith` on a key account's
  calendar through the same grant code.
- **Granting through the CalDAV `ACL` method** in the owner's session,
  which holds the owner's keys: no mainstream client sends it; it replaces
  the whole ACL, accepts groups and arbitrary privileges, and has no way
  to tell a client to pin first.
- **No collection key, each event's DEK wrapped to each grantee.** A grant
  would rewrap every event once per grantee, every write would wrap once
  per grantee, and a revoke would rewrite every event; the collection key
  makes each of these one write.
- **Grant wraps stored in the grantee's account.** Every grant, revoke and
  rotation would write across accounts, and a revoke would race the
  grantee's own writes; the calendar's preferences entry is where upstream
  keeps a principal's view of the calendar.
- **A new grantee receiving only the newest generation.** Upstream shows a
  new grantee every event of the calendar.
- **A lazy rewrap at the first grant** (each event rewrapped at its next
  write): the grantee could not read the calendar's older events.
- **Converting a calendar back to the unshared form at its last revoke**:
  a second full rewrap for no confidentiality gain (3.3).
- **Per-principal dead properties** (each principal's bundle holding its
  own set, a snapshot at the grant, merged in the view): it would give each
  person their own Apple colour and order and remove every grantee write
  into the calendar-level bundle. Not taken: upstream keeps one dead
  property set per calendar, changed only by `Modify` holders, and release
  4 keeps Stalwart's behaviour and invents none; the shared bundle (3.3)
  already keeps a grantee's session out of the owner's personal bundle.
  Recorded as a later refinement (13.1, item 11).
- **An in-session high-water mark of generations** (optional hardening,
  not adopted): `SessionKeys` would remember the highest generation count
  it has seen per calendar and refuse a ring with fewer, so a store writer
  who restores a pre-revoke archive (2.1) would be caught by any session
  that saw the rotation. It covers only sessions alive across the restore,
  costs a per-calendar counter in memory, and integrity against a store
  writer is not claimed (release 1 section 2).
- **Keying the CK cache by a hash of the freshly read ring** (instead of
  3.1's key-domain id): a cached key would be found only under the ring it
  came from, so a key of an earlier conversion could not answer for a later
  one. Not taken: the id serves the cache as well and also binds the
  associated data of every object of the shared form to its conversion,
  which a hash of the ring would not, and it stays stable across a
  rotation, a grant and a rename, which change the ring's bytes and would
  empty the cache at each.

## 14. Upstream interactions

Merge hotspots this release adds (fork insertions in upstream files):

- `crates/dav/src/common/uri.rs` (the gate's grantee branch, its refusal
  of a token carrying `Impersonate`) and `crates/dav/src/common/za.rs`
  (fork-owned: the resolver); `crates/dav/src/common/propfind.rs` (the
  loader's view, which serves the assisted-discovery block; the foreign
  calendar eligibility check in that block and in the `propname` branch;
  the vanished-path filter and the grantee's sync-token bound in `get`
  after its `vanished` fetch);
  `crates/dav/src/calendar/{update,delete,proppatch,copy_move,get,
  freebusy}.rs` (the resolver at each re-check, the calendar assertion of
  event writes and of the event `DELETE`, the linked-event write rule in
  `update.rs` and `proppatch.rs`, the single-parent rule, the refused
  linked `MOVE` and the rewrap in `copy_move.rs`, the cache invalidation
  after a shared calendar's `DELETE` in `delete.rs`). In `update.rs` and
  `delete.rs` the fan-out calls now pass the event's account beside the
  merged writer `account_info`, separately reconstructed unmerged writer
  identity, and keys (6.2), both make the second run of
  6.2 (the update and the create before the first run's finalize, the
  DELETE in its message loop), and the fan-out call and the schedule tag
  follow "either run built"; `copy_move.rs` also holds the rename's two
  boundary writes (5.2).
- `crates/groupware/src/scheduling/event_cancel.rs` (upstream's file:
  `itip_cancel` split into a snapshot-taking `pub(crate)` function behind
  the existing entry point, with no behaviour change, for the second
  run's deletion; revision 10).
- `crates/groupware/src/calendar/fanout.rs` (fork-owned: the event-account
  parameter, the owner-address token locality, the delivery of the
  second run's kept messages to writer recipients R (listed plus resolved),
  filtering those recipients from the first run, and `resolve_inner`'s
  provenance-based carve-out, the A-only routing override, the `SENT-BY`
  placement, no withheld
  entries for a delegated fan-out), the second run's own fork module in
  `crates/groupware/src/calendar/`, and
  `crates/groupware/src/calendar/drain.rs` (fork-owned: the delegated
  branch of the step-2 sender check and its reason string). The delegate
  path depends on `scheduling_account_info`'s merged addresses (the
  writer's first, then the owner's), on the drain's step-2 rule that an
  item's ORGANIZER or acting ATTENDEE resolves to `changed_by`, on
  `changed_by` being the writer in every item the fan-out writes, on
  upstream's builders never addressing a local address
  (`send_update_messages`, `organizer.rs`, `event_cancel.rs`), with explicit
  R filtering for listed and resolved writer recipients in the
  first run (revisions 12–13), on the REQUEST merge's
  `AddComponent` and the CANCEL merge's ORGANIZER copy (`inbound.rs`),
  which 6.2's per-component `SENT-BY` statement describes, on that merge's
  full update and master cancellation (`inbound.rs:94`, `:157`, `:231`,
  `:265`) and the drain's one copy per UID (`drain.rs:662`), which 6.2's
  inherited table records, and on upstream's write-time ACL checks in
  `update.rs` and `delete.rs`, which are the authority for a delegated
  message. The second run (revision 10) depends on
  `organizer_handle_update`, `organizer_request_full` and the split
  `itip_cancel` staying reachable and pure apart from their increments
  vector, on the two routing fields (`force_send`, `rsvp`) staying
  outside `Attendee`'s equality and hash and outside every payload, on
  the incoming snapshot counting a local line by locality and scheduling
  agent only (`snapshot.rs:217`), on `itip_finalize`'s two operations
  (stripping `SCHEDULE-FORCE-SEND`, then the increments) and on its
  running only on `Ok`, with `NothingToSend` returning before it, and on
  `build_cancel_component`'s recipient filter; an upstream change to any
  of them needs a re-read of 6.2's mechanism.
- `crates/groupware/src/calendar/seal/{event,collection,tree}.rs` (fork)
  and `crates/groupware/src/calendar/{drain,storage}.rs` (the key domain).
- `crates/http/src/api/mod.rs` and `vault.rs` (routing), the new
  `shares.rs`, and `crates/http/src/api/contacts.rs` (the reseal's
  domain).
- `crates/vault/src/session.rs` (the CK cache) and `keys.rs` (the
  purposes).
- Delivery's account resolution in
  `crates/common/src/cache/principals.rs` is a merge hotspot for R19:
  `account_id_from_email_inner`'s catch-all fallback must pass its quiet
  flag through a new composing `rcpt_id_from_email_inner` helper. The
  existing `domain_inner` and `rcpt_id_from_parts_inner` carry that flag;
  ordinary callers keep their result and trace behavior. R uses quiet
  Account-only results after canonical address handling, configured `+`
  stripping, custom rules and catch-all fallback. L uses the scheduling
  address list including writer-only groups; A uses directly owned listed
  aliases expanded over domain names, excluding membership groups. Neither
  changes the incoming snapshot's exact locality check. Re-read L/R/A if
  those upstream rules change.

Upstream code the design depends on without editing it, to re-read when a
merge touches it:

- COPY/MOVE overwrite deletions keep `send_itip=false`
  (`copy_move.rs:747-756`, `:931-943`; 5.6). The key-account overwrite
  passes its resolved domain and writer keys so a future scheduled event
  could build a CANCEL if that flag were flipped; the return observation
  in case 12 detects such a mutation even when callers discard the
  returned message. The no-key queue path can queue an iMIP message only
  for a plaintext scheduling tree. Case 12 also checks pending items and
  queued tasks while the consumer is held, with an ordinary control.
- The scheduling decision delegate sending mirrors: `itip_create`,
  `itip_update`, `itip_cancel` and `ItipSendStatus::resolve`, and
  `scheduling_account_info`'s merged address list
  (`crates/common/src/cache/principals.rs:813-833`); a change to who
  counts as local, or to when they return messages, changes what a grantee
  sends in the owner's name. The existing-event PUT chooses update versus
  create by `schedule_tag` (`update.rs:259-266`); the second run must mirror
  that branch and keep both builders' raw success outcomes through filtering.
  Upstream's iMIP sender resolution
  (`crates/services/src/task_manager/imip.rs`) is the behaviour mirrored
  for plain accounts and is not on the key path.
- In-account `COPY` and collection `COPY`, which add names to one document
  (`copy_event`, `copy_container`), `move_event`, which swaps one name and
  keeps the rest, and `DestroyArchive::delete_za`'s unlink of a document
  with several names: the single-parent rule, its fenced recheck in the
  conversion and the linked-`MOVE` refusal exist because of them.
- `is_member`'s `Impersonate` arm and every handler that trusts it ahead
  of a container check (`PROPFIND propname`, `LOCK` and `UNLOCK` in
  `crates/dav/src/common/lock.rs`, collection `COPY`, `MOVE` and
  `rename_container`) on a direct URI, and the assisted-discovery block
  and the `propname` branch on the grantee's own home: the gate's
  `Impersonate` refusal and the foreign calendar eligibility check cover
  them, so a new handler or listing of that shape needs the same reading.
  `Store::vanished` (`crates/store/src/query/log.rs`), which returns paths
  without a calendar identity, the slug rule of `mkcol.rs` (a slug refused
  only while its path exists) and the bare change id of a sync token
  (`DavQuery::changes`), on which the vanished filter and the grantee's
  sync-token bound rely; `shared_containers`, which the filter no longer
  uses for its prefixes. The change id a commit's result reports
  (`AssignedIds::last_change_id`, returned by `commit_batch`), which the
  store assigns inside the commit's transaction
  (`crates/store/src/backend/rocksdb/write.rs:174-189`): the boundary's
  step 2 records it (5.2).
- `Calendar::preferences` and `preferences_mut`
  (`crates/groupware/src/calendar/mod.rs`), whose behaviour the grantee's
  entry follows, and the resource cache's per-entry reads of flags and time
  zones (`crates/groupware/src/cache/calcard.rs`).
- The access model: `personal_id` and `is_member`
  (`crates/common/src/auth/access_token.rs`);
  `has_access_to_container` and `effective_acl`
  (`crates/common/src/sharing/`); `refresh_acls`
  (`crates/common/src/sharing/acl.rs`), which the endpoints do not rely on
  for their invalidations (4.6); the `AccessToken` arm of
  `invalidate_local_caches`, which drops resident keys
  (`crates/common/src/cache/invalidate.rs`); `IndexValue::Acl` and the
  share notifications (`crates/common/src/storage/index.rs`).
- Discovery and privileges: `build_home_set`
  (`crates/dav/src/principal/propfind.rs`), the assisted-discovery block,
  and `current_user_privilege_set` (`crates/dav/src/common/acl.rs`), on
  which a client's view of an edit grant as writable depends.
- The registry's account destroy and `acl_revoke_all`
  (`crates/jmap/src/registry/set.rs`, `crates/store/src/query/acl.rs`),
  which revoke a destroyed owner's grants and invalidate each grantee's
  token, and on which 8.6 relies, the Account item's deletion before the
  revoke scan included (the barrier of 4.5: a change of that order, or of
  the registry and data store being one `Store`, needs a re-read of the
  barrier); `danger_destroy_account`'s own ACL range
  (`crates/store/src/dispatch/store.rs`) is the grantee side.

Upstream v1.0.0 is imminent and changes the free/busy path (the fix for a
free/busy-only grant on CalDAV, support thread 1988); its merge needs a
re-read of `freebusy.rs`, `scheduling.rs` and the `Withheld` arm, and of
anything it changes in how grants are checked.
