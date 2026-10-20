# Zero-access mail: encryption at rest (release 3) design spec

Date: 2026-10-20. Status: revision 2, draft for review. Scope: release 3
of the zero-access fork (jaylaney/stalwart), item 3 of release 1 spec
section 1.2; GitHub issue #19, milestone "Release 3: mail encryption"
(issues #19, #20 and #21). Revision 2 incorporates the six findings and
the clarification of the 2026-10-20 design review
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
  need sealed content, until release 6 builds the per-user index.
  Searches on visible facts (flags, size, arrival time, UID) keep working.
- The mail-side gates, the index and spam-training exclusions, the trace
  redactions (issues #20 and #21), the leak-test extension and key-mode
  runs of the upstream IMAP and JMAP mail suites.

### 1.2 Out of scope

- Bearer and OAuth wraps (a master-key wrap made when the person types
  their password into the OAuth authorization page). No client Circulo
  targets needs OAuth against a self-hosted IMAP server; the wrap comes
  with release 5, when the web client holds a token.
- Search of sealed content: release 6 (issue #23). This release ships the
  refusal and the visible-field index only.
- Invitations by email (iMIP): release 4 (issue #18). The ingest gate that
  skips iMIP for key accounts stays as it is.
- Sealing Sieve scripts, vacation messages, identities and address books.
  Scripts run at delivery without a key, so they stay readable (section 2).
- Re-sealing delivered messages under the master key, key-pair rotation,
  and server-side threading for key accounts (section 14).
- Enterprise paths (deleted-item retention, archived items) are excluded
  from the product build and are not gated.

### 1.3 Assumptions

- Issue #20 (the authentication exchange masked in the mail protocols'
  raw-input traces) is merged (PR #49, `ea667dd8`, 2026-10-20). Section
  10's redactions extend the session functions it edited, and the plan
  builds on its raw-input test (`tests/src/za/raw_input.rs`) and trace
  collector.
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
  account-bound. The product copy must say that mail in transit is not
  covered, and that the operator's logs record who sent mail to whom and
  when.
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
  is present is detected and fails the read (section 10).
- **Spam filtering.** Messages are classified at SMTP DATA time on the
  transit plaintext with upstream's global model. A key account never
  contributes training samples (section 6.1), so marking mail as junk or
  not junk does not teach the filter. The product copy says so.
- **Search.** Until release 6 the server cannot search, sort or snippet a
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
whose result names a key account but carries no keys, with an `ImapEvent::Error`
(or the POP3 equivalent) carrying `ResponseCode::NoPerm` and a fixed text
naming zero access, traced as `SecurityEvent::Unauthorized` with details
`zero-access: mail login without session keys`. The keyless results are:
OAUTHBEARER and XOAUTH2 (internal OAuth tokens), API keys, master-user
impersonation (`user%master`) and any other path that returns a keyless
token for a marker account. A plain `NO` without a code would read as a
wrong password; the code and text make the refusal legible in client
error dialogs.

SMTP submission by a key account works with every mechanism upstream
offers, OAuth included: submission reads no stored mail (section 6.5).

JMAP over HTTP is unchanged in how keys arrive. The mail methods split in
two. **Keyless methods** read no message content and work with any token,
over the WebSocket included: `Mailbox/*`, `Email/query` and
`Email/queryChanges` on visible fields (section 8), `Email/changes`,
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
cannot be reached without them. The gate answers the method error `forbidden`
with a description naming zero access, traced as
`SecurityEvent::Unauthorized` with details `zero-access: mail access
without session keys`; each reader refuses the same way as defence in
depth. So a key account's state-driven sync over the WebSocket (changes,
queries, flags) works, and only content reads need an HTTP request with
the credential; release 5's web client inherits that contract. EventSource
and WebSocket push carry state ids only and keep working.

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
close, as upstream leaves every account's sessions; the product copy
states that changing the password does not disconnect clients already
connected, and that an app password's revocation ends its IMAP sessions at
their next command that refreshes and its POP3 sessions when they close.

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
sealing module lives in the email crate (`crates/email/src/message/za/`,
new) and reuses `vault::keys`.

A stub is recognised by the format byte `0x01` at the head of
`raw_headers`: a parsed header block never starts with a control byte,
and the check runs only on key accounts' archives. A key account's archive
whose `raw_headers` does not start with it is plaintext and passes through
unchanged (section 6.6); a crafted legacy message whose header block did
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
  themselves; a keyed thread index is release 6's.

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
6). A stub reached without the account gate (the destroyed-account window)
takes the same path. Unindexing is upstream's, with one care: the stub has
no subject, so the threading clear falls back to the empty-subject key,
which was never written; the builder may skip it for a stub.

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
`Email/parse` and `Blob/get` go through it and work. Every consumer that
reads a blob hash or section raw from storage is refused for any blob id
that is a sealed source, whatever its class and whichever account's
session asks, with `forbidden` and the `zero-access:` trace: `Blob/upload`
data sources (`crates/jmap/src/blob/upload.rs`), `FileNode/set` create and
update (which link the blob as a file's), and `FileNode/copy`'s replacement
blob (`crates/jmap/src/file/copy.rs`). A sealed source must never become
a file's blob, since file readers have no unsealing step, and must never
be copied into a new upload as ciphertext. The rule is keyed by the
blob's owning account and class, not by the requesting session. The
purge reads nothing. The upload quota counts plaintext.

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
for every source and `copy_message`; the assertion is one line in each
batch for every account and is recorded as an invariant 9 exception
(section 12). The key-account path asserts nothing on the record: the
public key never changes once active.

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
   sample when any recipient of the transaction is a key account, since
   that sample would hold the plaintext queue blob for the sample
   retention period. `card_is_ham` and `trusted_reply` run as today; the
   latter never matches a key account's singleton threads.
5. The iMIP branch stays gated (`&& !account.is_key_account()`, release 4
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
skipped (6.1 step 4); `received_at` follows upstream's rules.

### 6.3 Copy and move

Within the account, IMAP COPY/MOVE and JMAP Email/set mailbox changes edit
`MessageData` only, as today. `copy_message`
(`crates/email/src/message/copy.rs`), used by cross-account IMAP COPY and
JMAP Email/copy, is refused when either side is a key account, with a
`NoPerm` IMAP response or a JMAP `forbidden` set error and the
`zero-access:` trace, as calendar COPY and MOVE across accounts are in
release 1. `Blob/copy` is refused the same way. Mailbox sharing (IMAP
SETACL, JMAP Mailbox `shareWith`) is refused on mailboxes owned by key
accounts, and a key account's mailboxes are never reachable through
another session: they are left out of other accounts' LIST and
`Mailbox/get` (the `is_member` and `access_to` routes release 1 closed for
calendars), and a stub reached by a session holding no keys for that
account, whatever the route, answers `NO [NOPERM]` or `forbidden` with the
`zero-access:` trace, never an unreadable-message error. A key account may
still read mailboxes shared to it by non-key accounts, which hold
plaintext and need no key.

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

`POST /api/vault/setup-token` and `POST /api/vault/setup` refuse an
account that holds any message, as they refuse one with calendar data
(`za_has_documents` extended to `Collection::Email`), with the same 409;
the fence of 6.1 closes the window between that check and a delivery in
flight. A plaintext message found in a key account, recognised by the one
discriminator of 5.2 (a `raw_headers` field that does not start with the
format byte; `contents` is non-empty for stubs and plaintext alike), is
served as plaintext: there is no write that would seal it, and refusing
it would hide a bug behind an unreadable message. Readers, the index gate
and the leak scanner share that discriminator. The leak test's negative
control plants one and must flag it. Key accounts created during
development that hold mail are emptied by their operator before this
release is deployed; the release note says so.

### 6.7 Backup and restore

Backup copies stubs and sealed blobs as stored. Whether upstream's restore
(`restore_item`, `IngestSource::Restore`) re-parses the exported bytes or
restores archives verbatim is for planning to confirm (section 13); if it
re-parses, restoring a key account's message is refused with a trace
rather than ingesting ciphertext as a message.

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
  RFC822.SIZE, MODSEQ and OBJECTID from the cache as today.
- **POP3 RETR and TOP** (`crates/pop3/src/op/fetch.rs`): the same two
  opens. STAT, LIST and UIDL use the cache and need nothing.
- **JMAP Email/get, Email/parse, SearchSnippet/get, Blob/get,
  `/jmap/download`, EmailSubmission/set, Email/set (attachments by blob
  id)**: each reads through `blob_download` and the metadata read with the
  request's keys; a section download applies the unsealed metadata's
  offsets to the unsealed blob.
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
The refusal is `NO [CANNOT]` with a fixed text such as `Mail in this
account is encrypted at rest; the server cannot search message content.
Search your local copy instead.` (final wording in the plan), traced as
`ImapEvent::Error`, not as a security event (it must reach the client).
THREAD works and returns singleton threads (5.4). A SEARCH that mixes a
served key with a refused one is refused as a whole.

**JMAP.** `Email/query` and `Email/queryChanges` answer `unsupportedFilter`
for `text`, `from`, `to`, `cc`, `bcc`, `subject`, `body`, `header`,
`hasAttachment`, `sentBefore` and `sentAfter`, and `unsupportedSort` for
`from`, `to`, `cc`, `subject` and `sentAt`, checked before the query runs
rather than left to the store's silence on a missing field. Served:
`inMailbox`, `inMailboxOtherThan`, `hasKeyword`, `notKeyword`, the
thread-keyword filters, `inThread`, `id`, `before`, `after`, `minSize`,
`maxSize`; sorts `receivedAt`, `size`, `hasKeyword` and the thread-keyword
sorts; `collapseThreads`. `SearchSnippet/get` answers `unsupportedFilter`
whenever its filter carries a text term, and returns no snippets
otherwise. The `unsupportedFilter` description names the reason in the
same words as the IMAP text.

## 9. Feature gating for key accounts

| Feature | Release 3 behaviour for key accounts |
|---|---|
| IMAP, POP3 login with Bearer/OAuth, API key, master user | refused at login, `NO [NOPERM]`, `zero-access:` trace (section 4) |
| SMTP submission, ManageSieve | unchanged, keyless |
| Delivery to an account without a served key | temporary failure, retried by the sender's queue |
| Spam training (delivery, autolearn, user actions) | never for a key account |
| PGP/S/MIME encryption at rest | skipped |
| Threading index, `MergeThreads`, SMTP duplicate drop | not written, not scheduled, not applied |
| Full-text index | visible-field document only (5.5) |
| Server search, sort, snippets on sealed fields | refused (section 8) |
| Cross-account COPY/MOVE, Email/copy, Blob/copy | refused when either side is a key account |
| Mailbox sharing (SETACL, `shareWith`), shared-mailbox listing, group and impersonation routes | refused on key-owned mailboxes; not listed; a keyless stub read answers `NO [NOPERM]` / `forbidden` |
| JMAP key-needing methods on a keyless token (WebSocket, Bearer, API key, shared account) | `forbidden` at the dispatcher (section 4); keyless methods work |
| `/jmap/upload`, `Blob/upload` without the target account's keys | 403 / `forbidden` before any reservation (section 4) |
| Raw consumers of a sealed source (`Blob/upload` data sources, `FileNode/set`, `FileNode/copy`) | refused for any key-account upload, message or section blob id (5.7) |
| Ordinary delivery racing provisioning | the batch asserts no vault record; temporary failure and retry (6.1) |
| Milter, hook and filter events for a transaction delivering to a key account; IMAP and POP3 command events in a key-account session | contents redacted (section 10) |
| Push with email properties | state-only payload |
| iMIP ingest | gated as in release 1 (release 4) |
| Restore of a key account's message | refused if restore re-parses (6.7) |
| Setup with existing mail | 409, as with existing calendar data |
| Enterprise retention paths | excluded from the product build |

## 10. Error handling

- **Unreadable message** (a stub or blob that does not open, a policy
  version other than `1`, a wrap type the session cannot open, a bundle
  whose inner archive fails its checked decode): the read fails for that
  message with a logged `StoreEvent::DataCorruption` naming account and
  document id and never content, as `seal_error` does. Nothing is deleted
  or rewritten. IMAP answers the command with `NO [SERVERBUG]` after the
  messages it could serve; JMAP `Email/get` lists the id under `notFound`
  and `Blob/get` under `notFound`, so one damaged message does not block a
  mailbox listing; POP3 answers `-ERR` for that message.
- **Keyless read of a key account's mail**: a session of the account
  itself always has keys (section 4); a stub reached through another
  account's session (a shared mailbox, a group, impersonation) or a
  keyless JMAP token answers `NO [NOPERM]` on IMAP and POP3, `forbidden`
  on JMAP, with the `SecurityEvent::Unauthorized` trace with details
  prefixed `zero-access:`, as calendar paths do. Such a read is distinct
  from an unreadable message and never reports `SERVERBUG` or
  `DataCorruption`. No new event type is added.
- **Delivery without a served key**: temporary failure (6.1), traced with
  the account id and a fixed reason.
- Search refusals are section 8's, not errors.

**Traces.** The rules of release 1 section 10 extend to mail:

- `imap.raw-input`, `imap.raw-output`, `pop3.raw-input` and
  `pop3.raw-output` record `[redacted]` for the whole of a session
  authenticated as a key account, not only the authentication exchange
  (issue #20 masks that for every account): authenticated raw output
  carries FETCH and RETR literals, and raw input carries APPEND literals.
  ManageSieve is unchanged: scripts are visible by structure.
- `smtp.raw-input` records `[redacted]` for the DATA and BDAT phase of a
  transaction in which any recipient is a key account, and for every read
  after authentication in a session authenticated as a key account
  (submission). Envelope commands are traced as upstream traces them.
- The ingest event (`MessageIngest`) carries `[redacted]` as `MessageId`
  for a key account; the blob hash, size, ids and timings stay.
- **Structured events** (review F2). Raw-input masking leaves copies of
  content in events that format what they handled. The contract covers
  them: an event emitted while a key account's message is processed, or
  inside a key account's session, carries no header name or value, body
  text, search term or other client-supplied string. The plan inventories
  every `trc::event!` on the mail paths (SMTP DATA and its filters, ingest,
  Sieve, IMAP and POP3 operations, JMAP mail methods) whose keys (`Details`,
  `Contents`, `Reason`, `Value`, `Name`, `Key`) can carry one, and redacts
  it. The floor the review established: `MilterEvent::Write` and
  `MilterEvent::Read` (`crates/smtp/src/inbound/milter/client.rs`), which
  log every header a milter is sent and every mutation it returns, record
  `[redacted]` as `Contents` for a transaction with a key-account
  recipient (the milter itself still receives the plaintext, as the spam
  filter does: that is transit); the MTA-hook and spam-filter events that
  carry header values are treated the same; `ImapEvent::Fetch`, whose
  `Details` is the `Debug` form of the requested attributes and so carries
  the header names of a `HEADER.FIELDS` request, and every other `ImapEvent`
  and `Pop3Event` whose `Details` or `Contents` echo the command
  (`Search` criteria, `Append`, `Store`), record `[redacted]` there in a
  key-account session; and the parser errors that quote client input,
  which issue #20 sanitised before authentication for every account, are
  sanitised after authentication too in a key-account session.
- The vacation-response Sieve compile error no longer traces its reason
  with the message text (`crates/jmap/src/vacation/set.rs`, issue #21),
  for every account, as the fork removed the whole-iCalendar traces for
  every account; recorded as an invariant 9 exception.
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
delivery on retry, with no plaintext archive or blob ever stored (the
same for `copy_message`); **offsets**: a delivered message with the
server's prepended headers, a MIME attachment and a nested message
round-trips through FETCH `BODY[]`, `BODY[2]`, `BODY[HEADER.FIELDS]`,
Email/get `bodyValues` and a section download byte for byte against the
original; **uploads**: `/jmap/upload` and `Blob/upload` refused on a
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
    ordinary asserts in its batch that no vault record exists (6.1).
25. The stub carries exactly the arrival time, the blob hash, the sealed
    bundle and the empty root part; nothing else. The unsealed metadata is
    a content view; the stored stub is what writes assert on and what
    keyless paths read.
26. A mail session of a key account exists only with keys; every read of
    a key account's message holds the account's own keys or is refused.
27. No threading index, no full-text term, no spam sample and no push
    payload with content is ever written or sent for a key account; its
    search document holds arrival time and size only.
28. Plaintext of a key account's mail exists outside process memory only
    in the SMTP queue, inbound until delivery completes and outbound until
    relay, and this is stated to the user as a limit.
29. Delivery never reads a session key and never stores in the clear: a
    recipient without a served public key gets a temporary failure.
30. Traces of a key account's mail sessions and of a transaction that
    delivers to one carry no message content, header name or value,
    Message-ID, search term or other client-supplied string, in raw and in
    structured events alike.
31. No consumer reads a key account's sealed source (an upload, a message
    blob or a section of one) raw from storage: it reads through the
    unsealing reader with the owner's keys, or it is refused.
32. The vault-record assertion on ordinary writers (6.1) is the third
    all-account change after release 1's REPORT prefix check and DAV
    OPTIONS authentication, recorded as an invariant 9 exception with
    them; the trace redactions of section 10 that apply to every account
    (the vacation compile reason) join the release 1 list.

## 13. Open items for planning

Each item is for planning to look up in code; the recommendation stands
unless the code says otherwise.

1. **The stub's readers.** List every site that reads a `MessageMetadata`
   archive of a message (`root_part` and the direct `contents[0]` indexes
   in IMAP FETCH, Email/get, the snippet, submission, copy, the search
   builder, blob download, push, unindex) and convert each to the unsealed
   view or a refusal; the empty root part (5.2) is the safety net, not the
   plan. Recommendation: one `za_metadata_view` helper in the email crate
   returning `Cow<Archive<MessageMetadata>>`, as `za_event_view` does for
   events, called at every site.
2. **Restore.** Confirm whether `restore_item` re-parses exported bytes
   (6.7). Recommendation: refuse for key accounts in this release and note
   verbatim restore as a packaging-release item (issue #25).
3. **The search document.** Build it in the index task (reading
   `MessageData` for the size, one extra get) or write it synchronously at
   ingest and skip the task for key accounts. Recommendation: the task,
   with the extra read, to keep ingest's batch upstream's.
4. **Where the stub-side refusal lives in IMAP.** Confirm that a
   `ImapEvent::Error` with `ResponseCode::NoPerm` reaches the client from
   `Session::authenticate` (security events close the connection
   silently). Recommendation: `NoPerm` with the fixed text; `[CANNOT]` for
   search refusals.
5. **SMTP DATA redaction.** The session knows its recipients at DATA time;
   confirm the point where the raw-input trace decision is made relative to
   RCPT resolution (issue #20's masking lives in the same session code).
6. **Push payloads.** Confirm the gate point in
   `crates/services/src/state_manager/http.rs` that calls
   `build_email_push_object`, and that a state-only payload is what the
   subscription receives when properties were requested.
7. **The `duplicate` Sieve test.** It records Message-IDs in the lookup
   store (`KV_SIEVE_ID`); confirm whether the key is hashed. If not, it is a
   visible-by-structure item for scripts that use it, to be listed in
   section 2 rather than gated.
8. **`Threading` clear on unindex** for a stub (5.5): confirm a clear of a
   never-written key is harmless on every backend, or skip it.
9. **Upload sealing coverage.** Confirm that `put_jmap_blob` is the single
   write path for `Reserved` blobs of an account (including
   `blob_upload_many`) and that no keyless reader of a `Reserved` blob
   exists besides the purge.
10. **Key-mode harness for IMAP and JMAP.** Confirm how `imap_tests` and
    `jmap_tests` provision accounts and whether `create_key_user_account`
    fits their fixtures; the plan sizes the variants from that.
11. **The fence's cost.** `AssertValue::None` on the vault record key in
    every ordinary ingest and copy batch is one key read inside the write
    transaction on every backend; confirm it is supported by the SQL
    backends' assertion path as the calendar's `assert_value` is, and that
    a failed assertion surfaces to `deliver_message` as a retryable error
    rather than a permanent one.
12. **The structured-event inventory.** Enumerate the `trc::event!` sites
    of section 10's rule and the keys each carries; decide per site
    between `[redacted]` and dropping the key. Recommendation:
    `[redacted]`, so dashboards keep their event counts.

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
  release 6 is the answer.
- **No search document at all** for key accounts. Rejected: JMAP mailbox
  listings and IMAP date and size searches run through the search store,
  so a document with the two visible fields is needed for them to work.
- **A keyed thread index** (Message-ID hashes under an account secret).
  Rejected for this release: delivery holds no secret of the recipient, so
  anything it can compute an operator can test against; a thread index
  built while the person is online belongs with release 6's keyed index.
- **Padding the blob** to hide message sizes. Rejected: the exact
  plaintext size is visible in `MessageData` and in quota accounting, as
  the event size is in release 1.
- **HPKE** for the `pk` wrap. Rejected in release 2; the same composition
  is reused.
- **Sealing uploads not at all** (a transient plaintext limit like the
  queue). Rejected: an upload sits for the TTL at rest and the session
  that creates it holds the keys. The cost is one writer, the unseal in
  `blob_download`, and the refusal of the two consumers that take an
  upload's hash raw (5.7); the file consumers are refused rather than
  converted because files are out of scope.
- **An empty `contents` as the stub marker**, which would make a missed
  reader fail loudly. Rejected: the loud failure is a panic inside a
  spawned IMAP operation, which the client sees as a hang; the empty root
  part fails closed (no headers, no body) and the key-mode suites catch
  the missed site.

## 15. Release pointers

- Release 4 (iMIP, issue #18): writes a sealed pending item from the
  ingest branch this release leaves gated (plan 10 outcome, "Facts for
  release 3"), and sends the sealed notification email through a sealed
  path, never the task queue (release 2 invariant 12).
- Release 5 (web client): the Bearer and OAuth wraps; sealed uploads are
  already in place.
- Release 6 (search): the per-user keyed index built online, which also
  gives server-side threading; the section 8 refusals are then narrowed.
- Product copy (Circulo): the visible-and-sealed list of section 2, the
  transit and connection-residency limits, the search behaviour and the
  spam-training note, written for the person, not the operator.
