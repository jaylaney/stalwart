# Zero-access mail: encryption at rest (release 3) design spec

Date: 2026-10-20. Status: revision 5, for approval. Revision 3
answered the review's second pass: the fence's backend rule (6.1, F1a),
the two download branches (7, F5) and the revocation wording (4, C1).
Revision 4 answers its third: the compensating blob delete is withdrawn
in favour of upstream's staging behaviour, stated as a limit (2, 6.1,
12; F1b.1 and F1b.2), provisioning rechecks emptiness after the fence is
up (6.1, F1c), and the refresh explanation is narrowed (4, C1).
Revision 5 (2026-10-22) records the plan 13 outcome as built, following
its outcome note (`../plans/2026-10-20-zero-access-plan13-outcome.md`),
and the release renumbering of the same day that made calendar sharing
release 4. Scope: release 3 of the zero-access fork
(jaylaney/stalwart), item 3 of release 1 spec section 1.2; GitHub
issue #19, milestone "Release 3: mail encryption" (issues #19, #20 and
#21). Revision 2 incorporates the six findings and the clarification of
the 2026-10-20 design review
(`2026-10-20-zero-access-mail-design-review.md`): the provisioning fence
on ordinary deliveries (6.1, F1), structured trace events (10, F2),
uploads without keys (4 and 5.7, F3), raw consumers of sealed sources
(5.7, F4), the message offset coordinates (7, F5), the one stub marker
(6.6, F6) and which connections keep keys after a credential change (4,
C1). The revision history is in git.

The terms key account, master key (MK), data key (DEK), wrap type, session
keys, key cache, generation and visible by structure are those of the
release 1 spec, `2026-10-06-zero-access-calendar-design.md` (revision 7),
cited below as "release 1". The `pk` wrap, the private key in session keys
and the self-check are those of the release 2 spec,
`2026-10-11-zero-access-invitations-design.md`, and the key pinning spec,
`2026-10-17-zero-access-key-pinning-design.md`. Every invariant of those
specs continues to hold except where section 12 amends one by name.

## 1. Purpose

A key account's mailbox is an ordinary mailbox. Mail from anyone on the
internet arrives over SMTP, and the person reads it in Apple Mail,
Thunderbird or whatever client they already use, over IMAP, with nothing
installed beyond the account itself. The server stores every message of a
key account sealed, so that an operator cannot read it at rest or while
the person is logged out, and opens messages only inside a session that
holds the person's keys. The trust model is release 1's: native clients
unmodified, the operator unable to read stored data, the running server
holding plaintext while it serves a session.

Mail is built before email invitations (milestone order swapped
2026-10-20) because inbound iMIP for key accounts will reuse mail sealed at
rest.

### 1.1 In scope (release 3)

- Every message stored for a key account is sealed: the raw message, every
  parsed header value, the MIME structure, the preview and the full-text
  index. Section 2 lists what stays visible.
- Delivery while the person is logged out, sealed to the account's public
  key; messages written during a session (IMAP APPEND, JMAP Email/set and
  Email/import) sealed under the master key.
- IMAP and POP3 sessions that hold the session keys for the life of the
  authenticated connection; JMAP mail methods served with the keys the
  HTTP layer already attaches.
- Password and app-password logins on the mail protocols. Bearer, OAuth and
  API-key logins to a key account's mail are refused (section 4).
- A clear refusal of every server-side search, sort or snippet that would
  need sealed content, until release 7 builds the per-user index.
  Searches on visible facts (flags, size, arrival time, UID) keep working.
- The mail-side gates, the index and spam-training exclusions, the trace
  redactions (issues #20 and #21), the leak-test extension and key-mode
  runs of the upstream IMAP and JMAP mail suites.

### 1.2 Out of scope

- Bearer and OAuth wraps (a master-key wrap made when the person types
  their password into the OAuth authorization page). No client Circulo
  targets needs OAuth against a self-hosted IMAP server; the wrap comes
  with release 6, when the web client holds a token.
- Search of sealed content: release 7 (issue #23). This release ships the
  refusal and the visible-field index only.
- Invitations by email (iMIP): release 5 (issue #18). The ingest gate that
  skips iMIP for key accounts stays as it is.
- Sealing Sieve scripts, vacation messages, identities and address books.
  Scripts run at delivery without a key, so they stay readable (section 2).
- Re-sealing delivered messages under the master key, key-pair rotation,
  and server-side threading for key accounts (section 14).
- Enterprise paths (deleted-item retention, archived items) are excluded
  from the product build and are not gated.
- Key accounts on PostgreSQL and MySQL (6.1): their assertion path cannot
  fence provisioning against a delivery in flight; the per-account
  database lock described there is the follow-up that lifts the
  restriction.

### 1.3 Assumptions

- Issue #20 (the authentication exchange masked in the mail protocols'
  raw-input traces) is merged (PR #49, `ea667dd8`, 2026-10-20). Section
  10's redactions extend the session functions it edited, and the plan
  builds on its raw-input test (`tests/src/za/raw_input.rs`) and trace
  collector.
- Every product account is a key account from creation (release 1
  section 1.3), and an account that held data is not converted: the
  provisioning refusals of section 6.6 guard the window between an
  account's creation and its `setup-token`, and an account that setup
  refuses is deleted and created again.
- No shipped key account holds mail. Key accounts created before this
  release during development may hold plaintext messages; section 6.6
  states how they are treated.
- A key account with TOTP enrolled logs in to IMAP, POP3 and SMTP with an
  app password, as any upstream account with TOTP does: the mail protocols
  carry no TOTP code, and the marker diversion answers `MissingMfaToken`
  there (release 1 section 4.3).

## 2. Threat model and claims

**Guarantee.** Release 1's guarantee extends to mail. A key account's
messages, listed in the sealed set below, are unreadable from the stored
data without the person's credentials (password, recovery key or an app
password) or a key resident in a server's memory. This protects against
stolen disks and backups, database and blob-store dumps, a database
administrator, and demands for stored data.

**Limits, stated alongside the guarantee.**

- The running server holds plaintext: a mail session while it serves
  fetches, delivery while the message is parsed, filtered and sealed, and
  submission while a message is queued.
- **Transit.** Mail reaches the server in the clear over SMTP (TLS ends at
  the server) and leaves it the same way. The SMTP queue holds the
  plaintext of an inbound message, as upstream's queue blob, from
  acceptance until local delivery completes and the next blob purge; and
  of an outbound message from submission until it is relayed, which can be
  days for an undeliverable message. The queue is upstream's and is not
  account-bound. A delivery that loses the provisioning race (6.1) is a
  failed ordinary ingest and leaves what any failed ingest leaves: if it
  had staged its blob before its batch (a message Sieve modified), that
  plaintext blob stays under its 60-second temporary link until the first
  blob purge after the link expires, provided no other link to the same
  bytes exists (another account's link means another account's copy of
  identical bytes). The product copy must say that mail in transit is not
  covered, and that the operator's logs record who sent mail to whom and
  when.
- **What an account held while it was still plain.** Mail reaches an
  account before its provisioning only in the window between its creation
  and its `setup-token`, and the provisioning refusals (6.6) keep an
  account that holds mail, calendar data, unfinished tasks, spam training
  samples or keys in the internal search index from becoming a key account.
  What they cannot see stays as upstream leaves it: the plaintext blobs of
  messages deleted before provisioning, unlinked, until the next blob purge;
  a JMAP upload or `Blob/copy` made while the account was plain, under its
  temporary link for the upload TTL (one hour by default); and a
  server-wide training sample that SMTP autolearn took from mail
  delivered to the account while it was plain, which keeps the queued
  message's blob in plaintext for its hold period (180 days by default)
  and carries no account id, so no per-account check or deletion finds
  it (plan 13, R53).
- **Other search stores.** The search-index checks of provisioning (6.6)
  and the index-write fence (6.1) cover the internal search store. With
  any other search store (ElasticSearch, MeiliSearch, or a PostgreSQL or
  MySQL search store), an account whose deleted message's search document
  outlived its unindex, or whose ordinary index write landed after its
  message's unindex, can be provisioned with those terms still in that
  store, and a superseded index write (a worker that outlived its task
  lease on a cluster) can add them after provisioning. On an internal
  search store separate from the data store, the fence's assertion reads
  the search store's key space and passes vacuously, which a superseded
  worker can reach only on a FoundationDB one, so its first commit chunk
  can land after provisioning there; the later-chunk guard holds there,
  and the scans run, but on FoundationDB they can read up to 4 s stale
  against the task-queue check (plan 13, R51, R58, R62, R63). The
  task-queue check sees an index write in flight only while its task
  record is in the queue.
- **Sessions opened while the account was plain.** A session of a key
  account opened before its provisioning (the owner's, a master-user or
  impersonated IMAP or POP3 session, an SMTP submission) holds no keys.
  Every content read it makes is refused, and an impersonated IMAP
  session is refused every use of the account's mailboxes (6.3), so
  nothing stored is exposed; but the owner's IMAP session keeps its flag
  updates and a POP3 session its STAT, LIST, UIDL and DELE, and what any
  of them sends is traced as an ordinary session's until the client
  reconnects (plan 13, R33, R48; section 10).
- **Key residency on connections.** A mail client cannot be re-asked for
  its password, so a session's keys live as long as its authenticated
  connection (section 3.3). For an IMAP connection kept open with IDLE,
  that is as long as the client stays connected. The three things release
  1 section 2 keeps distinct (how long an authentication result is cached,
  how long a key is resident, whether the client is connected) coincide on
  a mail connection, and the product copy says so.
- **Sieve scripts and the vacation message.** The server runs the person's
  Sieve script at delivery without a key, so the script, including filing
  rules, forwarding addresses and the vacation text, is stored and
  readable in the clear. Identities (display name, signature) are account
  settings and are also in the clear.
- **Key substitution.** As in release 2: delivery seals to the public key
  the server reads from the store. A write-capable operator who
  substitutes it can read mail delivered until the person's next session,
  whose self-check (key pinning section 3.3) restores the key; the
  messages sealed to the substituted key are then unreadable to the
  person. Mail is confidential against a reader of the store, not against
  a writer who substitutes keys.
- **Integrity.** As in release 1: a store writer can drop, swap or delete
  sealed messages and their stubs silently; tampering with a message that
  is present is detected and fails the read (section 10). A `pk`-sealed
  message has no authenticity by design, since anyone can deliver one
  over SMTP, so a store writer can also forge a whole message (a stub and
  a blob under a DEK of its own); it cannot pair a forged stub with a
  genuine blob, which the forged DEK does not open (plan 13, R17).
- **Spam filtering.** Messages are classified at SMTP DATA time on the
  transit plaintext with upstream's global model. A key account never
  contributes training samples (section 6.1), so marking mail as junk or
  not junk does not teach the filter. The product copy says so.
- **Search.** Until release 7 the server cannot search, sort or snippet a
  key account's mail by sender, recipient, subject, date header, body or
  attachment, and tells the client so (section 8). Clients that keep a
  local copy search locally; a client that relies on server search gets
  the refusal.

**Visible metadata** (an allowlist, as release 1 section 6; everything not
listed here is sealed):

- Mailboxes: names, hierarchy, roles (Inbox, Sent, Trash, ...), subscriptions,
  UID validity, ACLs, message and unread counts, and quota counters. Mailbox
  names route IMAP commands the way collection slugs route URLs, so they
  are visible by structure.
- Per message: the mailboxes it is in and its IMAP UID in each; its flags
  and keywords, including the names of client-chosen keywords (free text in
  a visible slot, at most 128 characters, release 1's TZID rule); its exact
  plaintext size; its arrival time; its document id, which is also its
  thread id (section 5.4); its change ids and modseq; when it was moved to
  Trash or Junk (upstream's deletion timestamp); the content hash of its
  sealed blob; and the size class of its sealed header bundle (a multiple of
  256 bytes).
- The change log and the vanished-UID log: when messages arrived, were
  flagged, moved or deleted.
- The visible-field search document: arrival time and size (section 5.5).
- Sieve scripts and the vacation message; identities.
- SMTP transit metadata in queue records and logs: envelope sender and
  recipients, message ids, sizes, timestamps, the sending host; and the
  queue's plaintext copy while it exists.
- Account metadata: login times, connection counts, the credential used.

**Sealed:** the raw message, every header (sender, recipients, subject,
date header, Message-ID, In-Reply-To and References, list headers, every
`X-` header, the delivery and spam headers upstream adds), the MIME
structure (part types, encodings, sizes, attachment names, content ids),
the preview, the has-attachment bit, the body, every attachment, the
full-text index of any of these, and the threading index (not written,
section 5.4).

## 3. Key material

### 3.1 The mail wrapping key

`SessionKeys` (`crates/vault/src/session.rs`) gains a mail wrapping key,
MWK = HKDF-SHA256(MK, `za/v1/mail`), derived in `SessionKeys::new` next
to the EWK (`derive_event_key`, `crates/vault/src/keys.rs`). It wraps the
per-message DEK of every message written during a session. It is never
stored and is zeroed with the session keys.

### 3.2 Wrap types

The mail envelope reuses release 1 and 2's `X-ZA-KEY` byte layout:

| Wrap type | Bytes | Written by | Opened with |
|---|---|---|---|
| `0x01` (`mk`) | policy version `1`, `0x01`, wrapped DEK (72) | IMAP APPEND, Email/set create, Email/import, JMAP uploads | MWK |
| `0x02` (`pk`) | policy version `1`, `0x02`, ephemeral public key (32), wrapped DEK (72) | delivery (section 6.1) | the account's private key |

The `pk` construction is release 2 section 3.1 unchanged (`pk_wrap`,
`pk_unwrap` in `crates/vault/src/keys.rs`: ephemeral X25519, HKDF label
`za/v1/pk` with both public keys in the info, XChaCha20-Poly1305 with a
24-byte nonce). The policy version byte is carried for format consistency
with the calendar; mail defines no field policy, since everything in a
message is sealed, so the byte is `1` and a reader refuses any other value
(section 10). Associated data is in section 5.3.

### 3.3 Keys on mail sessions

Release 1 section 5 ("in-flight copies") attached `SessionKeys` only in
the HTTP layer, for one request. This release amends that (invariant 24):

- An IMAP session stores `Option<Arc<SessionKeys>>` in `SessionData`
  (`crates/imap/src/core/mod.rs`), set by `Session::authenticate` from
  `authenticate_with_keys` (`crates/common/src/auth/authentication.rs`,
  already `pub`) and copied by the STARTTLS rebuild
  (`try_replace_stream_tx`). Every token the session builds for its own
  account afterwards, including the keyless tokens `refresh_access_token`
  produces and APPEND passes into ingest, is re-attached with
  `with_session_keys` before use. The keys are dropped by LOGOUT,
  UNAUTHENTICATE, connection close, the authenticated inactivity timeout
  (`core.imap.timeout_auth`, 30 minutes by default) and server shutdown;
  a spawned operation keeps its own `Arc` until it finishes, as a request
  keeps its copy in release 1.
- A POP3 session stores them in `State::Authenticated`
  (`crates/pop3/src/lib.rs`), set by `handle_auth`, carried by `into_tls`,
  dropped with the state.
- The private key is unwrapped lazily by `Server::za_private_key`
  (`crates/common/src/auth/vault.rs`), which runs the self-check once per
  `SessionKeys` and records the outcome in it, exactly as the drain does.
  A session's first read of a `pk`-sealed message pays one vault read.
- SMTP submission and ManageSieve need no keys and keep calling
  `Server::authenticate`, which strips them. The HTTP layer is unchanged:
  JMAP mail methods read `access_token.za_keys_for(account_id)`.
- The key cache is not consulted for mail logins; each IMAP or POP3 login
  derives afresh with Argon2, as every upstream mail login verifies its
  hash afresh. The key-derivation concurrency limiter (release 1) applies.
  A builder may later key the cache by a fingerprint of the login
  credential; nothing here depends on it.

`SessionData`'s and `State`'s `Debug` stay silent about the keys
(`SessionKeys` prints only the account and generation).

## 4. Authentication and sessions

A mail session of a key account exists only with keys (invariant 26).
`Session::authenticate` in IMAP and `handle_auth` in POP3 refuse a login
whose result names a key account but carries no keys: IMAP answers
`<tag> NO [NOPERM] Zero-access accounts log in to mail with the password
or an app password.` and POP3 `-ERR` with the same text. The refusal is
traced as `SecurityEvent::Unauthorized` with details `zero-access: mail
login without session keys`, an event of its own beside the error the
client receives (section 10; plan 13, R22). The keyless results are:
OAUTHBEARER and XOAUTH2 (internal OAuth tokens), API keys, master-user
impersonation (`user%master`), the recovery administrator's
impersonation and any other path that returns a keyless token for a
marker account. A plain `NO` without a code would read as a wrong
password; the code and text make the refusal legible in client error
dialogs. The refusal does not count as an authentication failure, so a
client that tries a keyless mechanism first is not locked out; the
per-IP rate limit bounds a client that repeats it, and one connection
can retry it indefinitely within that limit (plan 13, R49).

SMTP submission by a key account works with every mechanism upstream
offers, OAuth included: submission reads no stored mail (section 6.5).
No OAuth authorization or device code is issued to an impersonated
login of a key account (a master-user or recovery-administrator login):
the code names only the account, so the Bearer token it is exchanged for
would pass as the owner's (plan 13, R36). An OAuth token issued while the
account had no password credential, which a pending account can obtain
only through impersonation before its provisioning, survives
provisioning as the owner's keyless token; it reaches only the keyless
methods below, whose data is visible at rest by design (section 2) to
the operator who minted it (plan 13, R37).

JMAP over HTTP is unchanged in how keys arrive. The mail methods split in
two. **Keyless methods** read no message content and work with any token
the account's owner holds, over the WebSocket included: `Mailbox/*`,
`Email/query` and `Email/queryChanges` on visible fields (section 8),
`Email/changes`,
`Thread/get` and `Thread/changes`, `Email/set` keyword and mailbox updates
and destroys, `Identity/*`, `VacationResponse/*`, `SieveScript/*`,
`PushSubscription/*`. **Key-needing methods** are gated in the JMAP
dispatcher (`crates/jmap/src/api/request.rs`, next to
`za_assert_calendar_allowed`) when the request's token holds no keys for
the target account (`za_keys_for` is `None`: a WebSocket request, a Bearer
or API-key token, a shared or impersonated account): `Email/get`,
`Email/parse`, `SearchSnippet/get`, `Blob/get`, `Blob/upload`,
`Email/import`, `Email/copy`, `EmailSubmission/set`, and `Email/set` for
its `create` argument at its own site. The HTTP upload endpoint
(`/jmap/upload`, `crates/http/src/request.rs`) answers 403 before any blob
reservation or write when the token holds no keys for the target account,
whether the token is a Bearer token or belongs to another account; the
sealing writer (5.7) takes the target account's keys as an argument and
cannot be reached without them. `/jmap/download` answers 403 the same way
for a token without the keys of a key account's sealed source, where
upstream answers an error outside a method call with 400 (plan 13, R30);
both 403 bodies carry the client text of section 10. The gate answers
the method error `forbidden` with a description naming zero access,
traced as `SecurityEvent::Unauthorized` with details `zero-access: mail
access without session keys`; each reader refuses the same way as
defence in depth. A token of another account, an impersonated token
included, reaches neither kind on a key account's mail (6.3; plan 13,
R34). So a key account's state-driven sync over the WebSocket (changes,
queries, flags) works, and only content reads need an HTTP request with
the credential; release 6's web client inherits that contract.
EventSource and WebSocket push carry state ids only and keep working.

A credential change does not end an open mail session beyond what
upstream does for every account, and the keys follow the connection
(section 3.3). Precisely: an IMAP session logged in with an app password
fails at its next token refresh (`refresh_access_token`, which renews a
scoped token and finds the credential gone) once that app password is
revoked; an IMAP session logged in with the password renews an unscoped
token that compares neither the password nor the vault generation, so it
keeps its keys until it ends; a POP3 session has no refresh and keeps its
keys until it ends whatever credential it used. A password change or
recovery therefore leaves existing password sessions running until they
close, as upstream leaves every account's sessions. A failed refresh need
not end the connection either: SELECT and APPEND refresh on the main
session path, where an authorization failure can close it, but a spawned
operation such as LIST logs the failure without changing the session
state, so an app-password IMAP session whose credential was revoked may
keep its keys, and a selected mailbox it can still fetch from, until the
connection ends. The product copy therefore
says that neither a password change nor an app-password revocation
disconnects clients already connected, and that their sessions end when
the client disconnects or the server's inactivity timeout closes them. A
guaranteed disconnect on revocation would be a design of its own and is
not part of this release.

Directory logins, registry credential edits and the vault endpoints are
release 1's, unchanged.

## 5. Stored format

**Invariant: no stored struct changes layout.** Ciphertext rides in
existing fields: the `raw_headers` field of the metadata archive and the
message blob. `MessageData`, `UidMailbox`, `Keyword`, the mailbox record,
the change log and every counter keep upstream's content.

### 5.1 The sealed blob

A key account's message blob is

```
0x01 (format) || envelope (section 3.2) || seal(DEK, aad_blob, raw message)
```

where `seal` is `vault::keys::seal` (nonce, ciphertext, tag) and the raw
message is the bytes upstream would have stored. The blob is
content-addressed by upstream's BLAKE3 hash of these bytes, so two sealed
copies never share a blob (a message delivered to two key accounts is
sealed twice). The envelope is duplicated in the stub (5.2) so that each
can be opened alone. Nothing is padded: the exact plaintext size is
visible anyway (section 2).

### 5.2 The stub

The `MessageMetadata` archive (`EmailField::Metadata`, field 71,
`crates/email/src/message/metadata.rs`) of a sealed message is a **stub**:

| Field | Stub value | Why |
|---|---|---|
| `contents` | one root entry holding one part with no headers, body `Binary`, flags and offsets 0, empty body lists | a reader not yet converted sees "no headers" instead of indexing `contents[0].parts[0]` out of bounds (see below) |
| `rcvd_attach` | arrival time, attachment bit clear | arrival time is visible; the bit is sealed |
| `blob_hash` | the sealed blob's hash | keyless paths (unindex, account destruction, blob purge) need it |
| `blob_body_offset` | 0 | unused |
| `preview` | empty | sealed |
| `raw_headers` | the sealed bundle | the carrier |

The bundle is

```
0x01 (format) || envelope || seal(DEK, aad_meta, u32 LE length || rkyv(MessageMetadata) || zero padding to a multiple of 256)
```

whose plaintext is the full metadata archive upstream would have written
(every part, every parsed header, the preview, the real `raw_headers`, the
real `blob_body_offset`, the attachment bit), serialized with rkyv and
decoded with a checked read, as release 1's bundles are (`FORMAT_V1`,
`decode_archive` in `crates/groupware/src/calendar/seal/tree.rs`). The
inner archive's `blob_hash` names the sealed blob, the hash the stub
carries, so the unsealed metadata names the blob a reader fetches (plan
13, D1). The sealing module lives in the email crate
(`crates/email/src/message/za/`, new) and reuses `vault::keys`.

A stub is recognised by the format byte `0x01` at the head of
`raw_headers`: a parsed header block never starts with a control byte,
and the check runs only on key accounts' archives. A reader treats a
`0x01`-headed archive as a stub only when it holds the account's keys; a
keyless read of a key account's message is refused before it reaches the
archive (section 10), and a reader without keys passes an archive
through unchanged, so an ordinary account's archive is never opened as a
stub (plan 13, R29). A key account's archive whose `raw_headers` does not
start with it is plaintext and passes through unchanged (section 6.6); a
crafted legacy message whose header block did
start with `0x01` would fail to open and be reported as unreadable
(section 10), which is acceptable for data that should not exist. The stub
deliberately keeps a well-formed empty root part rather than an empty
`contents`: upstream's readers index `contents[0].parts[0]` unchecked
(`root_part`, and a dozen sites in IMAP FETCH, Email/get, the snippet,
submission and copy code), and an IMAP operation runs in a spawned task,
so a reader missed by the conversion would panic and leave the command
unanswered until the client's timeout. With the empty root part a missed
reader degrades to a message with no headers and an empty body, which
leaks nothing and is caught by the key-mode suites. A message whose stub
or blob cannot be opened is unreadable (section 10).

### 5.3 Associated data

| Object | Purpose | Shape |
|---|---|---|
| Envelope | `mail-key` | `za/v1\|mail-key\|<account id>` |
| Blob body | `mail-blob` | `za/v1\|mail-blob\|<account id>` |
| Metadata bundle | `mail-meta` | `za/v1\|mail-meta\|<account id>\|<blob hash>` |

The envelope binds the account, so a wrap cannot be opened as a calendar
wrap or for another account; the bundle binds the blob hash, so a stub
cannot be moved onto another message's blob. Neither binds the document
id: a copy within the account (section 6.3) keeps the stub and the blob
as stored, as a collection copy does in release 1. Swapping two messages'
stubs within one account needs write access to the store, outside the
threat model.

### 5.4 `MessageData` and threads

`MessageData` (field 50) is written as upstream writes it, with two rules:

- `size` is the plaintext size, as upstream computes it, so quota
  accounting, `RFC822.SIZE`, `STATUS SIZE` and JMAP `size` are unchanged.
  The quota pre-check uses the plaintext length as today; the sealed
  bytes exceed it by a constant (envelope, nonce and tag) that is not
  charged.
- `thread_id` is the message's own document id. For a key account ingest
  does not compute a thread (`find_thread_id`), does not write the
  threading index (`EmailField::Threading`, whose key holds a normalised
  subject verbatim when it is 15 bytes or shorter, `CheekyHash`), and
  schedules no `MergeThreads` task. Each message logs a new thread
  container as a thread-less message does today. Consequences: JMAP
  threads are singletons, `THREAD=REFERENCES` returns one thread per
  message, and the SMTP duplicate drop (which matches on the threading
  index) does not apply to key accounts. Clients thread by headers
  themselves; a keyed thread index is release 7's.

### 5.5 The visible-field search document

The index task's `build_email_document`
(`crates/services/src/task_manager/index.rs`) gains the calendar builder's
two gates (`za_is_key_account`, then the stub check) but, instead of
`NotIndexed`, returns for a key account a document holding exactly two
fields, `ReceivedAt` (from the stub) and `Size` (from `MessageData`), and
no terms. It reads no blob. This is what lets JMAP list a mailbox sorted
by arrival time, `Thread/get` order its messages, and IMAP answer
`SEARCH BEFORE`, `SINCE`, `LARGER`, `SMALLER` and `SORT (ARRIVAL)`,
`SORT (SIZE)`: upstream serves all of these from the search store, with
nothing to fall back on. The document is built without a key (invariant
6). A stub of an account that no longer exists (the destroyed-account
window) takes the same path; an existing ordinary account's archive gets
upstream's full document whatever its header block starts with (plan 13,
R47). This document is written without the index-write fence, which
guards an ordinary account's documents (6.1). Unindexing is upstream's,
with one care: the stub has no subject, so the threading clear falls back
to the empty-subject key, which was never written; clearing it is
harmless and stays (section 13, item 8).

### 5.6 Mailboxes, keywords and the change log

Unchanged. Mailbox records (names, roles, hierarchy, subscriptions, ACLs,
UID counters), keywords on `MessageData`, the change log, the vanished log
and the deletion timestamp are upstream's.

### 5.7 Uploads

A JMAP upload (`/jmap/upload`, `Blob/upload`) by a key account is sealed
under the MWK in the same blob format (5.1), with a fresh DEK and the
`mail-blob` associated data, by a writer that takes the account's keys
(section 4 refuses the request before it when they are absent). A key
account's blobs then fall in two kinds. **Sealed sources**: every
`Reserved`-class blob (a sealed upload), every `Linked` blob of
`Collection::Email` (a message, opened through its stub) and every
section blob id of one (an attachment, located through the unsealed
metadata). **Plaintext**: `Linked` blobs of other collections (file nodes,
Sieve scripts), which are out of scope and read as upstream reads them.

`blob_download` (`crates/jmap/src/blob/download.rs`) is the one plaintext
read of a sealed source: with the request's keys it unseals an upload or a
message and applies section offsets to the unsealed message (section 7);
without them it refuses. `Email/import`, `Email/set` attachments,
`Email/parse` and `Blob/get` go through it and work, `Email/import` and
`Email/set` only into the source's own account (6.3). A key account's
upload id whose blob is not sealed reads as not found. Every consumer that
reads a blob hash or section raw from storage is refused for any blob id
that is a sealed source, whatever its class and whichever account's
session asks, with `forbidden` and the trace `zero-access: raw read of a
sealed source`: `Blob/upload` data sources
(`crates/jmap/src/blob/upload.rs`), `FileNode/set` create and update
(which link the blob as a file's), `FileNode/copy`'s replacement blob
(`crates/jmap/src/file/copy.rs`), `Blob/copy`, and
`x:SpamTrainingSample/set`, which would pickle the message's sender and
subject into the registry (plan 13, R32). `SieveScript/set` from a blob
id refuses a message or a section of one, which would store mail as a
plaintext script; a key account's own upload passes, read through
`blob_download` with the request's keys, since Sieve scripts are
plaintext (section 2; plan 13, R32 as amended). A sealed source must
never become a file's blob, since file readers have no unsealing step,
and must never be copied into a new upload as ciphertext. The rule is
keyed by the blob's owning account and class, not by the requesting
session. The purge reads nothing. The upload quota counts plaintext.

Every upload of a key account is a sealed source, so a key account
creates no JMAP `FileNode` with content; files written over WebDAV are a
separate path and plaintext in this release. Whether to keep the refusal
or give files a sealed blob path is open (issue #50, release 8).

## 6. Write paths

### 6.1 Delivery

`deliver_message` (`crates/email/src/message/delivery.rs`) runs with a
keyless token, as today.

**The provisioning fence.** Ingest reads the account's classification near
its start (`email_ingest`, the `account` read) and commits its batch at
the end, with Sieve in between. A delivery to a credentialless account
that reads "ordinary" before `POST /api/vault/setup-token` runs, and
commits after it, would store a plaintext message in a now-key account
(review F1). Setup-token issuance writes the vault record first and the
marker second (release 1 section 4.1), so the record's presence is the
fence: every batch that stores a message into an account it classified
as ordinary carries `AssertValue::None` on that account's vault record key
(`PrincipalField::ZeroAccessVault`, `crates/store/src/write/assert.rs`).
If the record appeared in between, the commit fails, `email_ingest`
returns a temporary failure, the queue retries, and the retry sees the
marker and takes the key-account path (a temporary failure until the
account is active, then a sealed delivery). The writers are `email_ingest`
for every source; `copy_message`, whose batch that loses the assertion
fails with a server error (JMAP `serverUnavailable`) and stores nothing,
and whose retry into an account that has become a key account is then
refused as a cross-account copy (6.3; plan 13, R3); the two batches of
`itip_ingest` and the iTIP merge commit, which the iMIP branch of an
ordinary delivery writes on its own (plan 13, R25); and the standalone
training batch of a cross-account JMAP `Email/copy` or IMAP COPY or
MOVE, which adds the source account's sample after the copies committed
and carries the source account's assertion in each of its commit chunks
(plan 13, R55), so that losing it fails the training commit after the
copies committed (IMAP `NO`, or a JMAP method error). The index write is
fenced too (below). The assertion is one line in each batch for every
account and is recorded as an invariant 9 exception (section 12). An
iMIP batch that the calendar storage helpers split at their commit
points (above 1,000 operations or 5 MB) carries it in its last chunk
only, so a first chunk committed after the recheck can store one
oversized plaintext event in an account provisioned during its delivery,
which needs an authenticated iMIP sender (plan 13, R27). The key-account
path asserts nothing on the record: the public key never changes once
active.

**The recheck** (review F1c). The assertion handles provisioning winning
the race; the reverse order would strand the account: `setup-token`'s
emptiness check passes, a delivery commits plaintext while the record is
still absent (its assertion rightly succeeds), the record and marker are
written, the token is returned, and `setup` then finds the message and
answers 409 for an account that is now marked and pending. So
`setup-token` rechecks, after writing the pending record and before
writing the marker, the whole emptiness check of 6.6. Once the record
exists no fenced writer can commit, and on the supported stores a writer
that read the key as absent before the record's commit fails its own
commit, so the recheck sees every message that could ever land. This
holds for the fenced writers, the index write below included; an
unfenced calendar write (CalDAV or JMAP by a sharee or an administrator)
during two concurrent
`setup-token` calls can still leave a marker without a record (plan 13,
R26). If the recheck finds data, `setup-token` removes the pending
record it wrote (identified by its revision and token hash, the delete
conditional on its bytes) unless the marker has appeared since, re-read
just before the delete, and answers the 409 the first check would have
given. A crash between the record write and the recheck leaves a record
without a marker beside the data: a reissue whose first check finds data
next to such a record removes it the same way and answers the same 409
(plan 13, R28), and a reissue that finds no data rechecks before it
writes the marker. Only `setup-token` creates the record, so `setup`
keeps its existing check as a backstop; an account that setup refuses is
deleted (issue #10's operator path) and created again (6.6).

**The index write** (plan 13, R58, R62, R63). The index task builds an
ordinary message's or event's search document from the plaintext and
writes it later, in commit chunks split above 1,000 operations or 5 MB,
the first holding the document's `Document` key, the one key that lists
all the others. Two interleavings would leave that content in an account
that is then provisioned: an execution that outlived its one-hour task
lease on a cluster, while another node's execution cleared its task
record, writing after setup; and an unindex that lands between two chunks
of one document, deleting the `Document` key and what it lists, before
the later chunks write keys that nothing lists. So the internal search
store writes every Email or Calendar document built for an account
classified as ordinary under two assertions. Its first chunk asserts that
the account holds no vault record (R58): a document whose first chunk
loses it is not written, the batch's other documents are, and the task is
retried; a first chunk that commits first is seen by the recheck's scans
(6.6). Every later chunk asserts that the document's own `Document` key
is still stored (R62): an unindex deletes that key and every key it lists
in one write, so a later chunk that commits after the unindex writes
nothing, and the document stops there without an error. FoundationDB
reports a read error at an assertion as a mismatch, so a failed guard is
confirmed by a read of the `Document` key outside the transaction (R63):
absent, the document stops and its remaining clears (a calendar merge's
removal of the previous version's keys, which no other document lists)
are written without the guard, its new keys dropped; stored, or the read
failing, the error is returned and the batch is retried. Nothing is
attached on PostgreSQL or MySQL, or without an internal search store. A
key account's visible-field document (5.5) and contact cards, which are
not sealed, are written unfenced.

For an ordinary account the fence costs one assertion per document, and
the guard one read per later chunk of a document of two or more chunks
(on FoundationDB that read is of the document's archive, quadratic in
its term count; a cheaper sentinel key is deferred, R62). A pending
record left without its marker beside plain mail (the crash above)
holds that account's index tasks in retry until `setup-token` is issued
again and repairs it; the mail is unsearchable meanwhile and nothing is
exposed. On a FoundationDB cluster the guard can leave an ordinary
document's index incomplete, never an orphan: a confirmation read
served from an older read version can stop a first index, and a clear
can race another execution's re-created document and remove a key it
lists (plan 13, R63).

**What the assertion relies on, per store.** The fence is only as good as
the backend's handling of an assertion on an absent key. RocksDB evaluates
it with `get_for_update` inside an optimistic transaction taken on a
snapshot, so a vault record written by another transaction in between
conflicts at commit and the retried batch re-evaluates and fails;
FoundationDB's non-snapshot read of the key adds a read conflict range,
which the record's insert collides with; SQLite runs every batch under an
immediate write transaction, so the two never overlap. PostgreSQL and
MySQL evaluate assertions as `SELECT ... FOR UPDATE` at read-committed
isolation (`crates/store/src/backend/{postgres,mysql}/write.rs`), which
has no row to lock when the key is absent, does not block the concurrent
insert, and is not re-checked at commit (review F1a). On those two
stores the assertion is not a fence. This release therefore supports key
accounts on RocksDB, FoundationDB and SQLite (decided with Jay,
2026-10-20): `POST /api/vault/setup-token` refuses issuance with 409
`key accounts are not supported on PostgreSQL or MySQL` when the data
store is PostgreSQL or MySQL, so a deployment learns it at provisioning
rather than from a leak. The follow-up that lifts the restriction
(decided with Jay, 2026-10-20) is a **per-account database lock on the
SQL backends**: a batch flag meaning "serialize on this account", set by
the two parties to the race (the batches that carry the absence
assertion, the fenced writers above, and the vault writes of
`setup-token` and `setup`), which the PostgreSQL backend honours with a
backend honours with a transaction-scoped advisory lock
(`pg_advisory_xact_lock` keyed by a hash of the account id) and the MySQL
backend with a named lock (`GET_LOCK`, released after commit or
rollback), taken as the first statement of the transaction. Neither needs
a row to exist, the absence assertion that follows then sees any
committed vault record, and a provisioning write waits for an ingest in
flight to commit, after which its own no-messages check refuses. The
other stores are untouched. The alternatives, a gap-locking isolation
level for those batches with a retry on serialization failure, or a lock
on the account's registry row ordered before the assertion, were set
aside as a new failure mode and a dependence on operation order. The
follow-up's cost beyond the store crate is CI: the fence stress test and
the key-mode suites on PostgreSQL and MySQL containers, and a check of the
visible-field search document against those backends' own full-text
implementations, which the leak scanner skips today. The product build
ships RocksDB.

**Persistence before the batch** (review F1b). A delivery that Sieve
modified, and every ingest that arrives without a queue blob, stages its
blob through `put_temporary_blob` before the batch, under a 60-second
temporary link. When the batch then fails the vault assertion, or any
other step after the staging fails (UID allocation, the batch write, a
cancelled task), that blob is a plaintext copy outside the queue. The
design does **not** compensate by deleting it: blobs are content-addressed
and global, so a hash this delivery wrote may already be linked by
another account that ingested identical bytes, a written flag cannot
establish ownership, and a delete that left the commit marker behind
would make a later ingest of the same bytes skip its write and store a
message with no body. Instead the staged blob takes upstream's path for
every failed ordinary ingest of every account: its temporary link expires
after 60 seconds and the first blob purge after that removes the bytes
and the commit marker, provided no document link and no unexpired
temporary link of another writer exists; if another account linked the
same bytes, the bytes stay as that account's copy. That lifetime holds
for every exit after staging, crash included, and depends on the
maintenance task running; it is stated as a limit next to the queue
(section 2) and qualifies invariants 24 and 28. A delivery that reuses
the queue blob stages nothing.

For a key-account recipient:

1. The served public key is read fresh from the vault record with
   `za_served_public_key` (`crates/common/src/auth/vault.rs`), as the
   fan-out does, never from the account cache. An account whose key is
   unavailable (state `PendingSetup`, a missing record) gets
   `LocalDeliveryStatus::TemporaryFailure`, so the sender's queue retries
   and the message is never stored in the clear; setup tokens live seven
   days and upstream's queue retries for five, so a provisioned user who
   finishes setup promptly loses nothing, and a sender whose mail cannot be
   delivered gets a bounce, as for a full mailbox.
2. The spam filter has already run at DATA time; the Sieve script runs as
   today on the in-memory plaintext (`fileinto`, `vacation`, `redirect`,
   `reject`, `discard`, `duplicate` all work). `redirect` sends the
   plaintext on; `vacation` answers from the clear-text script (section 2).
3. `email_ingest` (`crates/email/src/message/ingest.rs`) takes the parsed
   message to the point where upstream builds `MessageData` and the
   metadata, then: draws a DEK, wraps it to the public key (`pk`), seals
   the raw message into a new blob (5.1) through `put_temporary_blob`,
   never reusing the queue blob, builds the full metadata and seals it
   into the stub (5.2), and writes `MessageData` (5.4) with the message's
   own document id as thread id. The `IndexDocument` task is scheduled as
   today and builds the visible-field document (5.5).
4. No training sample is written for a key account, at delivery
   (`add_spam_sample`) or later (6.4). DATA-time autolearn skips its global
   sample when the transaction's authenticated sender or any recipient is
   a key account, a recipient that a milter, an MTA hook or a script adds
   at DATA included, since that sample would hold the plaintext queue
   blob for the sample retention period. `card_is_ham` and
   `trusted_reply` run as today; the latter never matches a key account's
   singleton threads.
5. The iMIP branch stays gated (`&& !account.is_key_account()`, release 5
   reopens it). Upstream's optional PGP/S/MIME encryption at rest
   (`crates/email/src/message/crypto.rs`) is skipped for key accounts: a
   key account has no encryption-at-rest key, and the two must not stack.
6. The queue blob keeps upstream's life: it is unlinked when the queue
   message completes and deleted by the next blob purge (section 2).

### 6.2 Writes in a session

IMAP APPEND, JMAP Email/set create and Email/import call the same ingest
with the session's keys attached to the token (section 3.3). Ingest seals
exactly as in 6.1 except that the DEK is wrapped under the MWK (`mk`).
Email/import does not reuse the uploaded blob's hash; the upload is
unsealed, re-sealed as a message blob, and left to its TTL. Training is
skipped (6.1 step 4); `received_at` follows upstream's rules. A session
write that arrives without keys for a key account is refused (`NO
[NOPERM]` or `forbidden`), never sealed to the public key, which only
SMTP delivery uses (plan 13, R22).

### 6.3 Copy and move

Within the account, IMAP COPY/MOVE and JMAP Email/set mailbox changes edit
`MessageData` only, as today. A copy or move of messages across accounts
is refused, before anything is read, when either account is a key
account, as calendar COPY and MOVE across accounts are in release 1:
IMAP COPY and MOVE answer `<tag> NO [NOPERM]` and JMAP `Email/copy` the
method error `forbidden`, each with the text `Mail in this account is
encrypted at rest and is not available to this session.`, traced as
`SecurityEvent::Unauthorized` naming the source account with details
`zero-access: mail copy across accounts`. `Email/copy` answers for the
whole call, not with a `forbidden` set error per created message: the
call is invalid for that pair of accounts, and the dispatcher's gate
(section 4) already answers at the method level (plan 13, D2). A token
without keys for a key account the call names (a WebSocket, Bearer or
API-key token, another account's or an impersonated one) is refused at
that gate first, with the same method error and the trace of section 4.
`copy_message`
(`crates/email/src/message/copy.rs`), which both protocols reach, refuses
the same pair again before any read, for a classification that changed
since. `Blob/copy` naming a key account as either account answers the
same method error, traced with the copy details naming that account, and
refuses a sealed source whatever the accounts (5.7). A sealed source of
a key account feeds `Email/import` and `Email/set` attachments only into
its own account: into another, `Email/import` answers the method error
`forbidden` and `Email/set` a `forbidden` set error for that create, with
the same text and the copy trace naming the owning account (plan 13,
R31).

Sharing a key account's mailbox is refused for every grant that adds or
widens a right (IMAP SETACL, JMAP Mailbox `shareWith`), with `NO
[NOPERM]` or `forbidden` and the trace `zero-access: sharing refused on a
key account's mailbox`; `DELETEACL`, `SETACL` with `-rights` and
`shareWith` removals pass, since they only reduce access (plan 13, I3).
A key account's mailboxes are never reachable through another session.
They are left out of other accounts' LIST and `Mailbox/get` (the
`is_member` and `access_to` routes release 1 closed for calendars): an
access token drops a key account's mailboxes and messages from its
entries for the accounts that granted it access (plan 13, D3), and
provisioning invalidates the tokens of the accounts named in the
account's mailbox grants, so a grant made while the account was plain
stops listing once those tokens are rebuilt (plan 13, R5). The listing is
not the enforcement: a use-time ownership rule refuses every route into a
key account's mail by a token that is not its owner's, whatever the
token holds (a grant made before provisioning, a group's grant, a token
cached or kept by a WebSocket from before), at every JMAP mail method
whose `accountId` is such an account, at IMAP's mailbox access check and
at every route that resolves another account's mailbox, with `NO
[NOPERM]` or `forbidden` and the `zero-access: mail access without
session keys` trace (plan 13, R34). A token from a master-user or
recovery-administrator login of the key account names the account but is
not its owner's, and is refused the same way (plan 13, R35). A stub
reached by a session holding no keys for that account, whatever the
route, answers the same, never an unreadable-message error. What stays
visible: a group grantee's members keep listing a share until their own
tokens are rebuilt; the JMAP session object of a stale or impersonated
token still lists the key account, and an impersonated IMAP session its
mailbox names, every use refused; and the account-level objects
(Identity, VacationResponse, SieveScript, Quota, PushSubscription), which
are not mail and are visible by structure (section 2), are not gated by
the ownership rule, so a holder of the Impersonate permission reaches
them. A key account may still read mailboxes shared to it by non-key
accounts, which hold plaintext and need no key.

### 6.4 Flags, deletion and expunge

`\Seen` writeback, STORE, Email/set keyword and mailbox updates, EXPUNGE,
Email/set destroy, POP3 DELE, auto-expunge of Trash and Junk, Mailbox
destroy with its messages and account destruction all read `MessageData`
or the stub's `blob_hash` only and are unchanged. The spam-training calls
on these paths (`add_account_spam_sample` from IMAP STORE, COPY/MOVE,
Email/set and Email/copy) return without writing for a key account.

### 6.5 Submission

SMTP submission by a key account is upstream's: the message goes to the
queue in the clear (section 2). JMAP `EmailSubmission/set` reads the stored
message through the unsealed view and the unsealed blob (section 7),
strips Bcc from the plaintext, and queues it as today; `onSuccessUpdateEmail`
and `onSuccessDestroyEmail` touch `MessageData` only. The IMAP client's
copy to Sent is an APPEND (6.2).

### 6.6 Pre-existing mail

Every product account is a key account from creation (release 1 section
1.3), and an account that held data is not converted to one. A
credentialless account can still receive mail between its creation and
its `setup-token`, and the provisioning refusals guard that window:
`POST /api/vault/setup-token` and `POST /api/vault/setup` refuse an
account that holds data, or whose plaintext could still reach its store
or the internal search index. The remedy for an account that setup
refuses is to delete it and create it again; for an account that holds
nothing else, waiting for its tasks, destroying its failed tasks and
deleting its spam training samples remain possible. The check runs in
`setup-token`'s first check, in its recheck (6.1) and in `setup`, and
answers 409 with the reason of the first term that applies, in this
order:

1. `account already holds calendar data`: a calendar event, a
   scheduling notification, or a calendar other than the untouched
   default one (release 1 section 4.1).
2. `account already holds mail`: a message, or the metadata archive of a
   destroyed message whose unindex task has not run yet, which still
   holds that message's plaintext beside its blob link and search
   document (plan 13, R46).
3. `account has unfinished tasks`: an `IndexDocument`, `UnindexDocument`,
   `MergeThreads` or `CalendarItipMessage` task of the account in the
   task queue, pending, retrying, in flight or failed. A task's record
   stays in the queue from the batch that schedules it until its worker
   records the outcome, so this term sees an index write in flight, which
   lands after any scan of stored output, and a retained thread-merge
   task, whose hashed strings hold an input of up to 15 bytes (a short
   subject or Message-ID) verbatim. It precedes the scans of terms 5 to 7
   on purpose: a worker that writes its document and clears its task
   between two scans would otherwise pass both. Setup never deletes a
   task: a pending one is waited for, and only a failed one may be
   destroyed, since destroying the record of a pending or in-flight task
   does not stop its worker (plan 13, R54).
4. `account already holds spam training samples`: a training sample
   indexed under the account. One written while the account was plain
   keeps its message's plaintext blob linked and its sender and
   subject in the registry until it expires; setup refuses rather than
   deleting it (plan 13, R52).
5. `account already holds mail`: an Email search document of the account
   in the internal search store, found by its `Document` key, of which
   each indexed document has exactly one. The unindex task deletes the
   search document in a write of its own after the one that clears the
   metadata archive, and that write can fail or be skipped (plan 13,
   R51).
6. `account already holds calendar data`: a Calendar search document of
   the account, in the same position after term 3, so that term 3's
   ordering argument holds for an event's index write too (plan 13, R60).
7. `account already holds mail`, then `account already holds calendar
   data`: any `Term` or `Index` key of the account in the Email index,
   then in the Calendar index, of the internal search store. A
   document's keys can outlive its `Document` key where the guard of 6.1
   does not reach: an unindex that straddles a rewrite's first chunk, a
   multi-chunk calendar merge that fails after its first chunk, an Email
   re-index whose output changed since the first index, and, on a
   cluster, two executions of one document whose listings differ, or
   archive reads up to 4 s stale on FoundationDB. Each is upstream
   behaviour that leaves keys no `Document` key lists, and this term
   refuses the account. The Contacts index is neither fenced nor
   scanned: contact cards are not sealed (plan 13, R62).

Terms 5 to 7 read the internal search store only (section 2, "Other
search stores"). The fence and the recheck of 6.1 close the window
between the check and a writer in flight, in either order.

A plaintext message found in a key account, recognised by the one
discriminator of 5.2 (a `raw_headers` field that does not start with the
format byte; `contents` is non-empty for stubs and plaintext alike), is
served as plaintext: there is no write that would seal it, and refusing
it would hide a bug behind an unreadable message. Readers, the index gate
and the leak scanner share that discriminator. The leak test's negative
control plants one and must flag it. Key accounts created during
development that hold mail are deleted and created again before this
release is deployed, as an account that setup refuses is; the release
note says so.

### 6.7 Backup and restore

Backup copies stubs and sealed blobs as stored. Upstream's restore
(`restore_item`, `IngestSource::Restore`) re-parses the exported bytes,
so restoring a key account's message is refused rather than ingesting
ciphertext as a message: ingest's key-account branch answers a permanent
failure (code 550), traced as `SecurityEvent::Unauthorized` with details
`zero-access: restore of a key account's message`, and the restore task
fails permanently (plan 13, D6). Verbatim restore is a packaging item
(issue #25).

## 7. Read paths

Every path that reads a key account's message holds the account's keys
(section 4) and opens the stub before any use of the metadata, and the blob
before any use of the body. The two-views rule of release 1 section 7.3
applies: the **unsealed metadata** is a content view built by opening the
bundle and decoding the inner archive, with the stub's arrival time
authoritative for `rcvd_attach`'s time bits and the inner archive's bit
for the attachment flag; the **stored stub** remains the value every write
asserts on and every keyless path reads. The unsealed metadata and the
unsealed blob are byte for byte what upstream would have stored: the
metadata with its own `raw_headers` (the delivery headers upstream
prepends, `Delivered-To` and `X-Spam-Status`, followed by the message's
header block), its `blob_body_offset` and its part offsets, which count in
the coordinates of the **reconstructed message**, and the blob holding the
original message as received. Sealing is transparent to upstream's two
coordinate systems: readers reconstruct `raw_headers` followed by the
blob from `blob_body_offset`, as IMAP FETCH does, and translate a part's
offsets back to stored-blob coordinates when they build a section blob
id, as Email/get does (`crates/email/src/message/body.rs`); neither
computation changes, it only runs on the unsealed pair. Applying metadata
offsets to the blob directly would start reads late by the length of the
prepended headers (review F5).

- **IMAP FETCH** (`crates/imap/src/op/fetch.rs`): the per-message metadata
  read becomes the unsealed view; `needs_blobs` reads the sealed blob and
  opens it. ENVELOPE, BODYSTRUCTURE, BODY[...], RFC822.*, PREVIEW,
  INTERNALDATE and the header sections come from the views; FLAGS, UID,
  RFC822.SIZE, MODSEQ and OBJECTID from the cache as today. The keys are
  taken only when an attribute needs content, so flag, UID and MODSEQ
  reads and the session's own synchronisation work without them, and a
  key account's session opened while it was plain keeps its flag updates
  and gets `NO [NOPERM]` for content (plan 13, R33).
- **POP3 RETR and TOP** (`crates/pop3/src/op/fetch.rs`): the same two
  opens. STAT, LIST and UIDL use the cache and need nothing.
- **JMAP Email/get, Email/parse, SearchSnippet/get, Blob/get,
  `/jmap/download`, EmailSubmission/set, Email/set (attachments by blob
  id)**: each reads through `blob_download` and the metadata read with the
  request's keys. `blob_download` keeps upstream's two branches on the
  unsealed pair: a whole-message download reconstructs `raw_headers`
  followed by the unsealed blob from `blob_body_offset`; a section
  download applies the blob id's offset and size, which Email/get already
  translated into stored-blob coordinates when it built the id, to the
  unsealed original blob, then transfer-decodes the section
  (`get_blob_section`, `crates/common/src/storage/blob.rs`). Neither
  branch applies metadata offsets to the blob or stored-blob offsets to
  the reconstructed message.
- **Thread/get, Mailbox/*, Email/query** read no content.
- **Server-initiated push with content** (`build_email_push_object`,
  `crates/services/src/state_manager/email_push.rs`, upstream's extension
  that puts Email properties into a web-push payload) runs with no session
  and is skipped for key accounts: their push subscriptions receive state
  changes only. Push subscriptions that request email properties are
  accepted and get the state-only payload.
- The in-memory message cache (`MessageStoreCache`) is built from
  `MessageData` alone and needs no key, as the DAV resource cache does.

## 8. Search, sort, snippets and threads

Decided with Jay (2026-10-20): a search that would need sealed content is
**refused with a clear error**, never served by decrypting and scanning.

**IMAP.** `SessionData::query` (`crates/imap/src/op/search.rs`) checks the
filter before building it. Served: sequence sets, UID, ALL, the flag keys
(ANSWERED, DELETED, DRAFT, FLAGGED, SEEN, KEYWORD and their UN- forms,
RECENT, NEW, OLD as today), MODSEQ, EMAILID, THREADID, BEFORE, ON, SINCE,
OLDER, YOUNGER, LARGER, SMALLER. Refused: BCC, BODY, CC, FROM, HEADER,
SENTBEFORE, SENTON, SENTSINCE, SUBJECT, TEXT, TO. SORT serves ARRIVAL and
SIZE and refuses CC, DATE, FROM, DISPLAYFROM, TO, DISPLAYTO and SUBJECT.
The refusal is `NO [CANNOT]` with the fixed text `Mail in this account is
encrypted at rest; the server cannot search message content. Search your
local copy instead.`, traced as `ImapEvent::Error`, not as a security
event (it must reach the client). THREAD works and returns singleton
threads (5.4); its search criteria follow the SEARCH rule. A SEARCH that
mixes a served key with a refused one is refused as a whole.

**JMAP.** `Email/query` and `Email/queryChanges` answer `unsupportedFilter`
for `text`, `from`, `to`, `cc`, `bcc`, `subject`, `body`, `header`,
`hasAttachment`, `sentBefore` and `sentAfter`, and `unsupportedSort` for
`from`, `to`, `cc`, `subject` and `sentAt`, checked before the query runs
rather than left to the store's silence on a missing field, and again
when `Email/queryChanges` finds the state unchanged, where upstream
answers before running the query (plan 13, R38). Served:
`inMailbox`, `inMailboxOtherThan`, `hasKeyword`, `notKeyword`, the
thread-keyword filters, `inThread`, `id`, `before`, `after`, `minSize`,
`maxSize`; sorts `receivedAt`, `size`, `hasKeyword` and the thread-keyword
sorts; `collapseThreads`. `SearchSnippet/get` answers `unsupportedFilter`
whenever its filter carries a `text`, `subject` or `body` condition,
anywhere in the tree, under `NOT` included, and otherwise returns each
requested snippet bare, with no subject or preview. Each description
names the refused property, followed by the IMAP text. The JMAP session
object still advertises upstream's `emailQuerySortOptions` for a key
account, the refused sorts among them; whether to trim it per account is
an open question of the plan 13 outcome.

## 9. Feature gating for key accounts

| Feature | Release 3 behaviour for key accounts |
|---|---|
| IMAP, POP3 login with Bearer/OAuth, API key, master user | refused at login, `NO [NOPERM]` / `-ERR`, `zero-access:` trace, not counted as a failed login (section 4) |
| OAuth authorization or device code for an impersonated login | refused (section 4) |
| SMTP submission, ManageSieve | unchanged, keyless |
| Delivery to an account without a served key | temporary failure, retried by the sender's queue |
| Spam training (delivery, autolearn, user actions) | never for a key account |
| PGP/S/MIME encryption at rest | skipped |
| Threading index, `MergeThreads`, SMTP duplicate drop | not written, not scheduled, not applied |
| Full-text index | visible-field document only (5.5) |
| Server search, sort, snippets on sealed fields | refused (section 8) |
| Cross-account COPY/MOVE, Email/copy, Blob/copy | refused when either side is a key account: `NO [NOPERM]`, the method error `forbidden` (6.3) |
| A key account's sealed source into another account's message (`Email/import`, `Email/set` attachments) | refused (6.3) |
| Mailbox sharing (SETACL, `shareWith`), shared-mailbox listing, group and impersonation routes | grants and widenings refused on key-owned mailboxes, removals allowed; not listed; every route by a token that is not the owner's, impersonation included, answers `NO [NOPERM]` / `forbidden` (6.3) |
| JMAP key-needing methods on a keyless token (WebSocket, Bearer, API key, shared account) | `forbidden` at the dispatcher (section 4); keyless methods work for the owner's tokens |
| `/jmap/upload`, `/jmap/download`, `Blob/upload` without the target account's keys | 403 / `forbidden`, an upload before any reservation (section 4) |
| Raw consumers of a sealed source (`Blob/upload` data sources, `FileNode/set`, `FileNode/copy`, `Blob/copy`, `x:SpamTrainingSample/set`; `SieveScript/set` for a message or a section) | refused for any key-account upload, message or section blob id (5.7) |
| Ordinary writers racing provisioning (ingest, copy, iMIP, a cross-account copy's training batch, the index write) | the batch asserts no vault record; temporary failure and retry; `setup-token` rechecks emptiness after the record write; a staged blob follows upstream's purge (6.1) |
| Key accounts on PostgreSQL or MySQL | `setup-token` refuses issuance, 409 with a fixed reason (6.1) |
| Milter, spam-filter and trusted-script events for a transaction delivering to or submitted by a key account; IMAP and POP3 command events in a key-account session | contents redacted (section 10) |
| Push with email properties | state-only payload |
| iMIP ingest | gated as in release 1 (release 5) |
| Restore of a key account's message | refused, a permanent failure (6.7) |
| Setup with existing data, unfinished tasks, training samples or search index keys | 409 with the reason of the first term that applies (6.6) |
| Enterprise retention paths | excluded from the product build |

## 10. Error handling

- **Unreadable message** (a stub or blob that does not open, a policy
  version other than `1`, a wrap type the session cannot open, a bundle
  whose inner archive fails its checked decode): the read fails for that
  message with a logged `StoreEvent::DataCorruption` naming account and
  document id and never content, as `seal_error` does. Nothing is deleted
  or rewritten. IMAP answers the command with `NO [SERVERBUG] Message
  could not be read.` after the messages it could serve (a FETCH that the
  server limited to a batch of UIDs then loses its `[LIMIT]` code); JMAP
  `Email/get` lists the id under `notFound` and `Blob/get` under
  `notFound`, so one damaged message does not block a mailbox listing;
  POP3 answers `-ERR` with the same text for that message.
- **Keyless read of a key account's mail**: a session of the account
  itself has keys (section 4) unless it opened while the account was
  plain (section 2); a stub reached through such a session, through
  another account's session (a shared mailbox, a group, impersonation)
  or with a keyless JMAP token answers `NO [NOPERM]` on IMAP, `-ERR` on
  POP3, `forbidden` on JMAP and 403 on `/jmap/upload` and
  `/jmap/download`, each with the text `Mail in this account is encrypted
  at rest and is not available to this session.`, with the
  `SecurityEvent::Unauthorized` trace with details prefixed
  `zero-access:`, as calendar paths do. Such a read is distinct from an
  unreadable message and never reports `SERVERBUG` or `DataCorruption`.
  No new event type is added.
- **The kind of a refusal.** A refusal the client sees is never a
  security error: those are not written, so an IMAP client would wait for
  a tagged response that never comes. The `SecurityEvent::Unauthorized`
  is emitted as an event of its own, and the client receives the JMAP
  `forbidden` error, which IMAP writes as `NO [NOPERM]` with its text
  (plan 13, R22).
- **Delivery without a served key**: temporary failure (6.1), traced with
  the account id and the reason `zero-access: delivery without a served
  public key`.
- Search refusals are section 8's, not errors.

**Traces.** The rules of release 1 section 10 extend to mail:

- `imap.raw-input`, `imap.raw-output`, `pop3.raw-input` and
  `pop3.raw-output` record `[redacted]` for the whole of a session that
  holds a key account's keys, not only the authentication exchange
  (issue #20 masks that for every account): authenticated raw output
  carries FETCH and RETR literals, and raw input carries APPEND literals.
  The BAD errors that can quote client input are traced redacted in such
  a session too. Every IMAP or POP3 login of an account that is already a
  key account holds its keys or is refused (section 4), so the only
  unmasked sessions of a key account are those opened while it was still
  plain, traced until they reconnect (section 2; plan 13, R48). Masking
  those would need a key-account lookup at every trace site of every
  ordinary session. ManageSieve is unchanged: scripts are visible by
  structure, so a key account's ManageSieve session, keyless by design,
  traces its authenticated raw input and output, script bodies included,
  in the clear.
- `smtp.raw-input` records `[redacted]` for every read after
  authentication in a session authenticated as a key account
  (submission), and, in a transaction with a key-account recipient, for
  every read of the DATA and BDAT phase and every read that, joined to
  the unparsed start of its request line, holds a DATA or BDAT command (a
  BDAT chunk can follow its command in one read). A recipient is
  classified at RCPT, a lookup error counting as a key account (plan 13,
  R12), and again at the end of DATA for recipients that a milter, an MTA
  hook or a script added. A read that holds an RCPT command together with
  a DATA or BDAT command is traced before its recipient is classified, so
  it is recorded `[redacted]` for every account, as issue #20 masks a
  read that holds AUTH (plan 13, R41); an ordinary account's pipelined
  envelope, or a body read that holds both words, loses its raw trace. A
  key-account recipient named after a BDAT chunk, which was traced, is
  refused with `503 5.5.1 Recipients must precede message data.` (RFC
  3030 orders RCPT before BDAT). Envelope commands are otherwise traced
  as upstream traces them, and so is SMTP raw output, whose replies are
  fixed or operator-configured, with the one exception below (a trusted
  script's reject text).
- The ingest event (`MessageIngest`) carries `[redacted]` as `MessageId`
  for a key account; the blob hash, size, ids and timings stay. A key
  account's own Sieve script traces a runtime error at delivery with the
  reason `[redacted]`, since an `error` action can quote the message.
- **Structured events** (review F2). Raw-input masking leaves copies of
  content in events that format what they handled. The contract covers
  them: an event emitted while a key account's message is processed, or
  inside a key account's session, carries no header name or value, body
  text, search term or other client-supplied string. Every `trc::event!`
  on the mail paths (SMTP DATA and its filters, ingest, Sieve, IMAP and
  POP3 operations, JMAP mail methods) whose keys can carry one records
  `[redacted]` there, so that dashboards keep their event counts. For a
  transaction delivering to or submitted by a key account:
  - `MilterEvent::Write` and `MilterEvent::Read`
    (`crates/smtp/src/inbound/milter/client.rs`), which log every header a
    milter is sent and every mutation it returns, record `[redacted]` as
    `Contents`, and the milter's `FrameInvalid` and `UnexpectedResponse`
    details likewise (plan 13, R42); the milter itself still receives the
    plaintext, as the spam filter does: that is transit. MTA-hook events
    carry no message content; their error text is the hook's own.
  - The spam filter withholds the DNSBL `Hostname`, the LLM `Details` and
    the `Eval` results of its rules and expressions, and redacts the DNSBL
    error's cause and a URL-resolution error (plan 13, D5, R42). The
    domains that its domain and URL analysis take from the body and
    attachments are looked up without the cache events that carry the
    name, and their error traces are redacted (plan 13, R56). The
    recipient list of `Spam(Classify)` is transit metadata and stays, as
    do the domains of the DKIM, DKIM2 and DMARC events.
  - A trusted Sieve script that an SMTP stage runs evaluates its values
    from the message, so every value its runtime evaluated is
    `[redacted]` in the interpreter's events and in the plugin traces it
    reaches: the runtime error's reason (an `error` action's text), the
    details of an unsupported action, of a reject, of a missing include
    (its name) and of a missing list (its name), a list redirect's
    address and `QuotaExceeded`'s recipient; a plugin's error is traced
    by its type alone, and `is_local_domain` looks its domain up without
    the cache events (plan 13, R57, R59). Ids, the stage script's
    configured name, elapsed times, fixed reasons and the configured
    return path stay. One exception: a script's `query` plugin call is
    traced by the store itself, with its arguments, query and result,
    before any redaction can apply; a script that passes message content
    to SQL sends it to the operator's database in any case, so the trace
    adds no exposure beyond the call the operator wrote (plan 13, R61).

  In a key-account session, `ImapEvent::Fetch`, whose `Details` is the
  `Debug` form of the requested attributes and so carries the header
  names of a `HEADER.FIELDS` request, and `ImapEvent::Store`, whose
  `Details` carries keyword names, visible by structure but redacted with
  `Fetch`'s (plan 13, D4), record `[redacted]` there; mailbox names in
  `MailboxName` keys are routing structure and stay, and the `Search`,
  `Sort` and `Append` events carry ids, counts and times only (`Append`
  its mailbox name too, routing structure as above). The parser errors
  that quote client input, which issue #20 sanitised before
  authentication for every account, are sanitised after authentication
  too in a key-account session.

  What stays outside the rule for such a transaction: a trusted
  redirect's target, resolved before the quota check, and the redirected
  message's envelope, traced by the queue and delivery as every envelope
  is (transit metadata); a trusted script's reject text, which reaches
  the client and the raw output; the spam filter's domain-id cache events
  (an id only) and its redirector analysis, which fetches a URL from the
  body; the DNS and HTTP requests that plugins make, which go to the
  network; and, for a key-account recipient that a milter, an MTA hook or
  a trusted script's `SetEnvelope` adds at DATA, what was traced before
  that recipient was classified (the transaction's DATA reads, the spam
  filter's evaluations and its DNSBL and LLM events, the trusted script's
  diagnostics).
- The vacation-response Sieve compile error traces its reason as its
  position only (`compile error at line N, column M`), for every account,
  since the compiler's message can quote the script, which holds the
  response's text (`crates/jmap/src/vacation/set.rs`, issue #21), as the
  fork removed the whole-iCalendar traces for every account; recorded as
  an invariant 9 exception.
- JMAP and HTTP body traces are already withheld for key accounts
  (release 1); `Email/query` refusals carry the filter property names, not
  values.

## 11. Testing

**Unit** (`crates/vault`, `crates/email`): MWK derivation is deterministic
and independent of the EWK (different labels, no shared bytes); a sealed
blob and stub round-trip under both wrap types; the stub's visible fields
are exactly arrival time and blob hash; the bundle length is a multiple of
256; a tampered blob, a tampered bundle, a bundle moved to another blob
hash, an envelope moved to another account, a policy byte other than `1`
and an unknown wrap type each fail cleanly; a plaintext metadata archive
is detected as plaintext; the unsealed view's offsets locate every part of
a corpus of real messages (multipart, nested message/rfc822, 8-bit, long
headers, a message with only a header block).

**Integration.** The upstream IMAP suite (`imap_tests`) and the mail
modules of the JMAP suite (`jmap_tests`) run in a key-account mode
(`ZA_KEY_ACCOUNTS=1`, as `webdav_tests` does): every message they write is
sealed, and every sub-test that searches, sorts or snippets sealed fields,
copies across accounts, shares a mailbox or trains the filter runs a
key-mode variant asserting the refusal; the rest run unchanged. That mode
is the real guard, as it is for CalDAV. New `za_tests` modules cover: IMAP
and POP3 login with the password and an app password, and the refusal of
OAUTHBEARER, XOAUTH2 and the master user; a key account with TOTP over
IMAP (refused with the password, accepted with an app password); delivery
to a pending account (temporary failure, nothing stored, delivered after
setup); delivery through a Sieve script (`fileinto` to a new folder,
`vacation`, `redirect`); the same message delivered to a key and a plain
account (two blobs, no sample); APPEND, Email/set and Email/import sealed
under `mk`; COPY within the account keeping the stub and blob; the
cross-account, sharing and push gates; every IMAP SEARCH and SORT key and
every JMAP filter and sort of section 8 on both sides of the line; the
visible-field document's content; threads as singletons; `EmailSubmission`
from a sealed message with Bcc stripped; a corrupted stub and a corrupted
blob (the listing continues, the trace names ids only); keys gone after
LOGOUT, UNAUTHENTICATE and a dropped connection (a `test_mode` counter of
live `SessionKeys` per account); a plaintext message in a key account
served as plaintext, and the classification test that a sealed stub and a
legacy plaintext archive, both with non-empty `contents`, are told apart
by the `raw_headers` marker alone; **the provisioning fence**: a delivery
paused by a test hook after its account read, while `setup-token` and
`setup` complete, then resumed, ends in a temporary failure and a sealed
delivery on retry, with no plaintext archive stored; the Sieve-modified
variant of it leaves its staged blob, which is unlinked after the hold
and gone after a purge, and the identical bytes ingested meanwhile by a
plain account stay readable there; the reverse order, `setup-token`
paused after its first emptiness check while a delivery commits, ends in
409 with no record and no marker, and a reissue after the mail is
removed succeeds; the same race for `copy_message` ends in the
cross-account refusal; and, because the deterministic pause cannot sit
inside the store's transaction, a stress test races ingest batches
carrying the assertion against vault-record writes through the store API
in a loop and asserts that no message batch ever commits after a record
exists, run on RocksDB in CI and on FoundationDB and SQLite when those
stores run; `setup-token` refused on PostgreSQL and MySQL; **offsets**:
a delivered message with
the server's prepended headers, a MIME attachment and a nested message
round-trips through FETCH `BODY[]`, `BODY[2]`, `BODY[HEADER.FIELDS]`,
Email/get `bodyValues` and a section download, where the whole-message
reads equal upstream's reconstructed result for the same message in a
plain account (the prepended delivery headers included) and the section
reads equal the decoded original attachment; **uploads**: `/jmap/upload`
and `Blob/upload` refused on a
Bearer token, over the WebSocket and into another account, accepted and
sealed with the credential, and a sealed upload imported and attached;
**raw consumers**: `Blob/upload` data sources, `FileNode/set` create and
update and `FileNode/copy` refused for a whole-message blob id, an
attachment section id and an upload id of a key account, from its own
session and from another account's; **traces**: with a milter configured,
a delivery to a key account leaves no header in the milter events; a
FETCH of `HEADER.FIELDS (X-Private-Canary)` leaves no header name in the
FETCH event; an authenticated malformed command leaves no quoted input;
the same three against a plain account show upstream's behaviour.

**Leak regression test.** `leak::scan` (`tests/src/za/leak.rs`) already
decodes every cached message of an account and every blob through the
configured blob store. The mail extension delivers and appends messages
with canaries in the subject (one 15 bytes or shorter, to catch the
verbatim index key), sender name and address, recipient, Message-ID,
body, an attachment's name and content, a custom header name and value,
and the preview; then asserts for the account: every metadata archive is a
stub, every message blob opens only with the keys, no `Threading` key, no
search term and only `ReceivedAt` and `Size` fields in its search
documents, no training sample and no queue blob linked after delivery
completes, no canary in any subspace, blob, task or search record, and no
canary in the collected traces of a key-account IMAP, POP3 and SMTP
session. Positive controls: a client keyword name and a mailbox name are
found where expected. Negative controls: a planted plaintext message and
a planted plaintext metadata archive must be flagged.

**Manual checklist** (`docs/zero-access/manual-checklist.md` gains a mail
section): Apple Mail on macOS and iOS, Thunderbird, and an Android client
(FairEmail or Thunderbird for Android): add the account with the password
and with an app password; receive, read, reply with a Sent copy, forward
an attachment, move, flag, delete, create and rename a folder, server
search (observe the refusal or the empty server result and the working
local search), IDLE push, reconnect after a password change, a TOTP
account through an app password, vacation on and off.

## 12. Invariants for implementers

Release 1's invariants 1 to 10, release 2's 11 to 15 and key pinning's 16
to 23 hold, with two amendments:

- Release 1 section 5 ("in-flight copies", and the README-dev line "only
  HTTP requests carry keys") is amended: IMAP and POP3 sessions hold
  `SessionKeys` for the life of the authenticated connection (section
  3.3). WebSocket, EventSource, ManageSieve and SMTP sessions still never
  do.
- Release 2 invariant 11 ("the `pk` wrap is opened only inside a drain")
  is amended: a mail session that holds the owner's keys opens `pk`
  envelopes of the owner's messages on read.

Added:

24. Every message stored for a key account is a stub plus a sealed blob,
    whatever wrote it (delivery, Sieve, APPEND, Email/set, Email/import,
    copy). No writer stores a plaintext blob or a plaintext metadata
    archive for a key account, and a writer that classified the account as
    ordinary asserts in its batch that no vault record exists (6.1:
    ingest, copy, the iMIP batches, a cross-account copy's training batch
    and the first commit chunk of an ordinary search document); the blob
    such a writer staged before its failed batch is upstream's transient
    of invariant 28, not a stored message.
25. The stub carries exactly the arrival time, the blob hash, the sealed
    bundle and the empty root part; nothing else. The unsealed metadata is
    a content view; the stored stub is what writes assert on and what
    keyless paths read.
26. A mail session of a key account exists only with keys; every read of
    a key account's message holds the account's own keys or is refused.
    A session opened while the account was still plain is the
    transitional exception of section 2: it holds no keys, every content
    read it makes is refused, and it ends when the client reconnects.
27. No threading index, no full-text term on the internal search store,
    no spam sample and no push payload with content is ever written or
    sent for a key account; its search document holds arrival time and
    size only. Section 2 states what another search store can hold; the
    fence and the scans of 6.1 and 6.6 do not reach it.
28. Plaintext of a key account's mail exists outside process memory only
    in the SMTP queue, inbound until delivery completes and outbound until
    relay, and in the staged blob of an ordinary ingest that failed after
    staging during provisioning, until its temporary link expires and the
    next blob purge runs (6.1); both are stated to the user as limits.
    What an account held while it was still plain is outside this
    invariant: provisioning refuses an account whose store, task queue or
    internal search index still holds it (6.6), and section 2 lists what
    that check cannot see.
29. Delivery never reads a session key and never stores in the clear: a
    recipient without a served public key gets a temporary failure.
30. Traces of a key account's mail sessions and of a transaction that
    delivers to one or that one submits carry no message content, header
    name or value, Message-ID, search term or other client-supplied
    string, in raw and in structured events alike. A session opened while
    the account was plain is traced until it reconnects (section 2), and
    section 10 lists what stays outside the rule, a trusted script's
    `query` call among it.
31. No consumer reads a key account's sealed source (an upload, a message
    blob or a section of one) raw from storage: it reads through the
    unsealing reader with the owner's keys, or it is refused.
32. The vault-record assertion on ordinary writers (6.1) is the third
    all-account change after release 1's REPORT prefix check and DAV
    OPTIONS authentication, recorded as an invariant 9 exception with
    them. With it: the later-chunk guard of an ordinary search document
    (6.1), an assertion class of its own (the document's own `Document`
    key present) confined to fenced documents; the trace redactions of
    section 10 that apply to every account (the vacation compile reason,
    and the masking of a read that holds an RCPT and a DATA or BDAT
    command, plan 13, R41), which join the release 1 list; and the
    key-account lookups that fail closed for every account: a store error
    while classifying fails a plain mail login (plan 13, R20) and an
    ordinary account's STORE `$Junk`, COPY, SELECT, `Email/get`,
    `Mailbox/get` and `/changes` (plan 13, R50), where upstream would
    have served them, and redacts an SMTP transaction's DATA traces (plan
    13, R12).

## 13. Open items, as resolved by plan 13

Each item is for planning to look up in code; the recommendation stands
unless the code says otherwise. Plan 13 resolved each one (its outcome note,
"Spec 13, the open items"), and the outcome stands under the item.

1. **The stub's readers.** List every site that reads a `MessageMetadata`
   archive of a message (`root_part` and the direct `contents[0]` indexes
   in IMAP FETCH, Email/get, the snippet, submission, copy, the search
   builder, blob download, push, unindex) and convert each to the unsealed
   view or a refusal; the empty root part (5.2) is the safety net, not the
   plan. Recommendation: one `za_metadata_view` helper in the email crate
   returning `Cow<Archive<MessageMetadata>>`, as `za_event_view` does for
   events, called at every site.
   Outcome: one view helper at every reader.
2. **Restore.** Confirm whether `restore_item` re-parses exported bytes
   (6.7). Recommendation: refuse for key accounts in this release and note
   verbatim restore as a packaging-release item (issue #25).
   Outcome: restore re-parses, and is refused for key accounts (section
   6.7).
3. **The search document.** Build it in the index task (reading
   `MessageData` for the size, one extra get) or write it synchronously at
   ingest and skip the task for key accounts. Recommendation: the task,
   with the extra read, to keep ingest's batch upstream's.
   Outcome: the search document is built in the index task.
4. **Where the stub-side refusal lives in IMAP.** Confirm that a
   `ImapEvent::Error` with `ResponseCode::NoPerm` reaches the client from
   `Session::authenticate` (security events close the connection
   silently). Recommendation: `NoPerm` with the fixed text; `[CANNOT]` for
   search refusals.
   Outcome: the refusal reaches the client as `NO [NOPERM]` once it is not a
   security error, which is never written (section 10).
5. **SMTP DATA redaction.** The session knows its recipients at DATA time;
   confirm the point where the raw-input trace decision is made relative to
   RCPT resolution (issue #20's masking lives in the same session code).
   Outcome: one read site and two flags carry the SMTP redaction (section
   10).
6. **Push payloads.** Confirm the gate point in
   `crates/services/src/state_manager/http.rs` that calls
   `build_email_push_object`, and that a state-only payload is what the
   subscription receives when properties were requested.
   Outcome: confirmed.
7. **The `duplicate` Sieve test.** It records Message-IDs in the lookup
   store (`KV_SIEVE_ID`); confirm whether the key is hashed. If not, it is a
   visible-by-structure item for scripts that use it, to be listed in
   section 2 rather than gated.
   Outcome: the test stores a hash, never the Message-ID, so nothing is
   visible.
8. **`Threading` clear on unindex** for a stub (5.5): confirm a clear of a
   never-written key is harmless on every backend, or skip it.
   Outcome: the clear of a stub's `Threading` key is harmless, and kept.
9. **Upload sealing coverage.** Confirm that `put_jmap_blob` is the single
   write path for `Reserved` blobs of an account (including
   `blob_upload_many`) and that no keyless reader of a `Reserved` blob
   exists besides the purge.
   Outcome: there is one upload writer, and its raw readers are refused
   (section 5.7).
10. **Key-mode harness for IMAP and JMAP.** Confirm how `imap_tests` and
    `jmap_tests` provision accounts and whether `create_key_user_account`
    fits their fixtures; the plan sizes the variants from that.
    Outcome: the key-mode harness fits.
11. **The fence's plumbing.** Confirm that a failed `AssertValue::None`
    surfaces from `email_ingest` to `deliver_message` as a temporary
    failure rather than a permanent one, how `setup-token` learns the data
    store's kind for the PostgreSQL and MySQL refusal, and that the
    pending record can be removed conditional on its revision for the
    recheck's abort (`za_vault_commit` or a sibling that deletes).
    Recommendation: a store-kind accessor on `Storage`, and the delete as
    one more conditional vault write through the common helper.
    Outcome: the fence's plumbing is confirmed.
12. **The structured-event inventory.** Enumerate the `trc::event!` sites
    of section 10's rule and the keys each carries; decide per site
    between `[redacted]` and dropping the key. Recommendation:
    `[redacted]`, so dashboards keep their event counts.
    Outcome: every site of the inventory records `[redacted]` (section 10).

## 14. Alternatives considered

- **Re-sealing delivered messages under the master key** in a drain-like
  pass at login, so that reads never open a `pk` wrap. Rejected: a pass
  over a large mailbox is a write-on-read with its own concurrency rules,
  and the saving is one X25519 operation per message on first read (tens
  of microseconds, below the blob read). The option stays open: both wrap
  types are readable, and a key-pair rotation would need the pass.
- **Sealing the metadata inside the blob** and keeping the stub to the
  blob hash. Rejected: header fetches and mailbox listings would read the
  blob store for every message; keeping the sealed metadata in the data
  store keeps upstream's access pattern.
- **Leaving subject and sender visible** for client convenience. Rejected
  by the allowlist rule (release 1 section 6) and confirmed with Jay: the
  visible column is structure, counts, sizes, times and flags.
- **Serving searches by decrypting and scanning.** Rejected with Jay: it
  works and then stops working as mailboxes grow; the refusal is honest and
  release 7 is the answer.
- **No search document at all** for key accounts. Rejected: JMAP mailbox
  listings and IMAP date and size searches run through the search store,
  so a document with the two visible fields is needed for them to work.
- **A keyed thread index** (Message-ID hashes under an account secret).
  Rejected for this release: delivery holds no secret of the recipient, so
  anything it can compute an operator can test against; a thread index
  built while the person is online belongs with release 7's keyed index.
- **Padding the blob** to hide message sizes. Rejected: the exact
  plaintext size is visible in `MessageData` and in quota accounting, as
  the event size is in release 1.
- **HPKE** for the `pk` wrap. Rejected in release 2; the same composition
  is reused.
- **Sealing uploads not at all** (a transient plaintext limit like the
  queue). Rejected: an upload sits for the TTL at rest and the session
  that creates it holds the keys. The cost is one writer, the unseal in
  `blob_download`, and the refusal of the consumers that take an
  upload's hash raw (5.7); the file consumers are refused rather than
  converted because files are out of scope.
- **An empty `contents` as the stub marker**, which would make a missed
  reader fail loudly. Rejected: the loud failure is a panic inside a
  spawned IMAP operation, which the client sees as a hang; the empty root
  part fails closed (no headers, no body) and the key-mode suites catch
  the missed site.
- **A vault assertion on every commit chunk of an ordinary search
  document** instead of the later-chunk guard (6.1). Rejected: on one
  node an unindex between two chunks lands before any record exists, so
  the orphaned keys would stay and the account would be refused for
  good, while the guard writes nothing after the unindex there and also
  stops a superseded worker's tail after setup. Serializing the unindex
  against the index task (node-local, and invasive in upstream's task
  manager), one transaction per document (FoundationDB's transaction
  limits) and writing the `Document` key last (orphans on every crash)
  were set aside too.
- **Purging an account's leftover search keys at setup.** Rejected:
  every product account is a key account from creation and none is
  converted, so an account that setup refuses is deleted and created
  again, which loses nothing.

## 15. Release pointers

- Release 5 (iMIP, issue #18): writes a sealed pending item from the
  ingest branch this release leaves gated (plan 10 outcome, "Facts for
  release 3"), and sends the sealed notification email through a sealed
  path, never the task queue (release 2 invariant 12).
- Release 6 (JMAP calendars and the web client): the Bearer and OAuth
  wraps; sealed uploads are already in place.
- Release 7 (search, issue #23): the per-user keyed index built online,
  which also gives server-side threading; the section 8 refusals are then
  narrowed.
- Product copy (Circulo): the visible-and-sealed list of section 2, the
  transit, connection-residency and while-plain limits, the search
  behaviour and the spam-training note, written for the person, not the
  operator.
- PostgreSQL and MySQL for key accounts: the per-account database lock
  of section 6.1, with its CI containers, as its own small plan when a
  deployment needs those stores.
