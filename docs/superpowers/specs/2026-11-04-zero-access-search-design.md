# Zero-access mail: search and threads (release 7) design spec

Date: 2026-11-04. Status: revision 3 (2026-11-05), approved by Jay.
Revision 2 answered the builder check of 2026-11-04: the thread map
loads on its own and every keyed request uses it as loaded (4.1, 4.3,
5.2, 7.4); the pass's log entry is an item update, since the change log
folds a delete and an insert of one id into one (6.3); the thread
history is stamped with the change id the pass consumed, so a segment is
sealed before the batch that publishes it (5.1). Revision 3 records
Jay's decision on keyless sessions and the WebSocket (section 13,
decision 1). Scope: release 7 of the zero-access fork
(jaylaney/stalwart), item 5 of the release 1 spec's section 1.2 and
GitHub issue #23 ("Search for key accounts"). Release 6 (JMAP calendars
and the web client) and this release do not depend on each other; the
number is the one the mail spec reserved.

**Precondition.** The design is written against upstream v1.0.3's
behaviour and its plan runs after the 1.0.3 merge, which is its own
branch and pull request under README-dev's "Merging upstream" (the raw
merge commit, the conflict ledger, the hook-inventory re-check,
`PINNED_UPSTREAM_TAGS`, `fork-release.toml`). Two behaviours the design
relies on arrived in 1.0.2 and were verified in v1.0.3's source, not
only its changelog: `Email/get` looks a supplied id up with
`cache.email_by_jmap_id(id)`, so a prefix that is not the message's
thread id is `notFound` (v1.0.3 `crates/jmap/src/email/get.rs:245-253`),
and `Email/queryChanges` with `collapseThreads` re-emits every message
of a thread that a created or destroyed id touched (v1.0.3
`crates/jmap/src/changes/query.rs:95-123`). Line numbers below are
v1.0.1's, the checkout the design was read against; the plan re-reads
each site after the merge.

The terms key account, master key (MK), session keys, key cache,
generation and visible by structure are those of the release 1 spec,
`2026-10-06-zero-access-calendar-design.md`, cited as "release 1". The
mail wrapping key, the stub, the sealed blob, the mail session and the
refusals of section 8 are those of the release 3 spec,
`2026-10-20-zero-access-mail-design.md`, cited as "mail spec". Every
invariant of the earlier specs continues to hold except where section
10 amends one by name.

Decisions taken with Jay in the design conversation of 2026-11-04:

- Design for up to 1 GB of mail per account, searched from one person's
  own clients, several at once (native IMAP clients and the JMAP web
  client).
- The index is a per-user sealed index, persisted and loaded into process
  memory for the session, never keyed term hashes in the shared search
  store (section 12 says why).
- Threads are real for key accounts, on JMAP as on IMAP, and the thread
  id is sealed: it lives only in the index and is substituted at read
  time. Nothing new about a key account's mail appears in the clear.

## 1. Purpose

A key account's mail is sealed at rest (mail spec). Since release 3 the
server refuses every search, sort or snippet of a key account that would
need sealed content, and threads every message as a singleton, because
nothing on the server can read a message without the person's keys. This
release gives key accounts server-side search and threading that behave
as an ordinary account's do, without weakening what the operator can
see: the index is built from messages the server opens *during the
person's session*, with the person's keys, and stored sealed under a key
only those sessions hold.

### 1.1 In scope

- A per-account search index covering what upstream's Email index
  covers: sender, recipients, subject, body, headers and extracted
  attachment text, with the same tokenizer, stems and phrase rules, so a
  search on a key account returns what the same search returns on an
  ordinary account holding the same mail (parity by construction,
  section 7.5).
- Sorting by from, to, subject and the date header; the date-header
  search keys; the attachment keyword and `hasAttachment` filter; search
  snippets.
- Threading: Message-ID, References and subject, as upstream threads, on
  JMAP (`threadId`, `Thread/*`, `collapseThreads`, the thread filters and
  sorts, the thread counts of `Mailbox/get`) and on IMAP (`THREAD`,
  `THREADID`, `EMAILID`), with the thread id sealed.
- The index built and maintained inside requests that hold the account's
  keys, persisted as sealed segments, held in memory while the account is
  in use, and rebuilt from the mail when lost.
- The refusals of mail spec section 8 narrowed to the cases where the
  index is unavailable (section 7.4); the open question of the advertised
  `emailQuerySortOptions` (plan 13 outcome, open question 2) closes, since
  the sorts are served.

### 1.2 Out of scope

- Calendar search. CalDAV `calendar-query` already evaluates a key
  account's text-match over each candidate decrypted in the request
  (`crates/dav/src/common/propfind/za.rs`, `crates/dav/src/common/search.rs:73-79`);
  JMAP calendar methods are release 6's. Contacts and files of a key
  account are in the clear and keep upstream's index.
- The SMTP duplicate drop and the `trusted_reply` spam check, which
  upstream derives from the thread index at delivery: delivery holds no
  key (section 6.4).
- Key accounts' mail stays unshareable (mail spec 6.3), so no grantee
  searches it; impersonated and master-user sessions hold no keys and
  stay refused.
- A keyed index on another search store (ElasticSearch, MeiliSearch, SQL
  full text). The index of this release is backend-independent: segment
  blobs in the blob store and one manifest value in the data store.
- Any change to delivery, the stub or the sealed blob.

### 1.3 Assumptions

- Mail spec 1.3 holds: every product account is a key account from
  creation, and provisioning refuses an account that already holds mail.
  So a product account's index is created empty at setup and is never
  backfilled in normal life; the rebuild of section 5.4 is a recovery
  path and the path for the test accounts that gain the feature with
  mail already in them.
- A session's keys are resident as release 1 and mail spec 3.3 say: on
  HTTP for the request, from the key cache (idle 15 minutes, 1 hour at
  most); on IMAP and POP3 for the life of the connection; never on
  WebSocket, EventSource, ManageSieve or SMTP sessions (section 13,
  decision 1).
- `mk` never rotates: setup is its only generation
  (`crates/http/src/api/vault.rs:1123`); a password change, a recovery
  and a recovery-key regeneration rewrap it. A key derived from it is
  therefore stable for the life of the account.
- The sizes the design is built for: 1 GB of mail is 10,000 to 30,000
  messages. The in-memory index of such an account is 20 to 50 MB with
  positions, a few hundred milliseconds to decrypt and load, its thread
  map a few hundred kilobytes, and a pass indexes a message in a few
  milliseconds (more with attachment text). These are estimates; the
  plan's first task is the benchmark of 9.6, whose figures decide the
  segment format's details and confirm or adjust the limits of 4.4.

## 2. Threat model delta

**Guarantee.** Mail spec 2 extends to the index. What the index holds
(terms, positions, sort keys, threading fields, thread ids and the
thread history) is unreadable from the stored data without the person's
credentials or a key resident in a server's memory. It is built, read
and written only by a request that holds the account's own session keys;
no background task, delivery, or keyless path reads or writes it
(release 1 invariant 6 is unchanged).

**What the operator learns, beyond mail spec 2's visible list:**

- The manifest (section 4.2), in the clear: the index format version,
  the change id the index has reached, the list of segment blobs with
  their sizes, and the rebuild cursor while a rebuild runs. From it and
  the blob store's timestamps: when the account's sessions indexed, and
  how much. Both correlate with arrivals the operator already sees.
- The sealed size of each segment, which is roughly proportional to the
  text size of the messages it indexes, compressed; the operator already
  knows each message's exact size.
- That a message's thread was reassigned, but not to what: the pass logs
  an item update of the message's own id (6.3), which is the same entry
  a flag change writes. Every message keeps its own document id as its
  stored thread id, the log and the Thread containers carry singletons
  as today, and the thread id a client sees travels only inside a keyed
  session's responses.

**Nothing else.** No term, stem, position, header name, Message-ID,
subject normalisation, sort key or thread membership leaves process
memory except sealed. Search queries are client-supplied strings and are
never traced (mail spec invariant 30).

**Limits, stated alongside the guarantee.**

- The running server holds the index in plaintext for every account in
  use, as it holds the plaintext of the messages it serves. The index is
  not key material: it is dropped, not zeroized, on eviction, except the
  decrypted segment buffers while they are deserialized (section 4.3).
- Integrity, as release 1 and mail spec 2: a store writer can delete the
  manifest or a segment, which forces a rebuild from the mail, or roll
  the manifest back to an older valid one, which the next pass heals by
  re-indexing from the older change id. A segment cannot be forged or
  moved to another account (its AEAD binds the account id and the
  segment id). A writer who can do this can drop messages themselves,
  which the index never notices: the index describes the mail that is
  present. A rebuild shows clients one round of thread reassignments
  (6.3).
- Timing: a pass runs inside the first keyed requests after mail arrives,
  so the moment of indexing is visible in the blob store and the
  manifest. It says the account was in use, which the operator's
  connection logs already say.
- Freshness is tied to presence. Mail that arrives while nobody holds the
  account's keys is indexed by the first keyed requests afterwards, so
  for that interval it is not searchable from any client and threads as
  a singleton (section 5.2). A client's own local index is unaffected.

## 3. Key material

### 3.1 The search key

`SessionKeys::new` (`crates/vault/src/session.rs:167-182`) derives a
third sub-key beside the event key and the mail wrapping key:

- `LABEL_SEARCH = b"za/v1/search"` in `crates/vault/src/keys.rs` beside
  the existing labels (`:23-30`), `derive_search_key(mk) =
  hkdf_expand(mk, LABEL_SEARCH)` beside `derive_mail_key` (`:155`), and
  `SessionKeys::swk()` beside `mwk()`.
- It is a `Secret` (`Zeroizing<[u8; 32]>` with a redacting `Debug`), lives
  only in `SessionKeys`, and follows the mail key's lifecycle: a password
  change, a recovery and an app password all yield the same `swk`, since
  `mk` is the same; Bearer and OAuth never yield keys (mail spec 4), so
  they never yield it.

Segments and nothing else are sealed under it: `seal(swk, aad, bytes)`
with `aad("search-seg", account_id) || segment_id` (the helpers of
`keys.rs:181-219`), so a segment opens only for its account and only
under the manifest entry that names it. The manifest is in the clear
(section 4.2).

No per-segment data key and no envelope: the search key is never
published or wrapped to another principal, so there is nothing for an
envelope to carry. A format byte at the head of each sealed segment
(`SEARCH_FORMAT_V1 = 1`, the mail blob's convention) is the one
discriminator, and the version inside the manifest is what a loader
checks before it opens anything.

## 4. The index

### 4.1 What it holds

Per account, the index is the union of its segments (4.2). Logically it
holds, for every message that is present (not tombstoned), two kinds of
record:

**The thread record** (small, always loaded, 4.3):

- **Threading fields.** `ThreadFields::scan` of the opened message
  (`crates/email/src/message/thread.rs:30-64`): the `xxh3_128` hashes of
  Message-ID, In-Reply-To, References and Resent-Message-ID, and the
  subject's thread name hash.
- **The thread id** the index assigned (section 6), and the message's
  **thread history**: for each change of its thread id, the change id
  the pass had consumed when it made the change (5.1) and the thread id
  before it (6.3 says what it is for and 5.5 when it is pruned).

**The content record** (large, loaded for content requests):

- **Terms.** The term set upstream's `index_document` produces for the
  message (`crates/email/src/message/index/search.rs:43-153`, called on
  the opened stub row and the opened blob with the server's index fields,
  default language and extraction limits), by field: from, to, cc, bcc,
  subject, body, attachment text and the key-value headers, with stems
  and, where upstream records them, positions. The fork calls
  `index_document` itself, so the term set is upstream's by
  construction; the document size cap (1 MiB of terms,
  `crates/store/src/search/mod.rs:39`) and the extraction limits apply as
  they do to an ordinary account.
- **Sort keys.** `MessageSortKeys` as upstream computes them
  (`crates/email/src/message/sortkeys.rs:452-481`: from, to and subject,
  40 bytes, upper-cased) and the message's `sent_at` offset, which the
  stub drops (mail spec 5.4; `ingest.rs:379-380`).
- **The attachment bit** (`za_has_attachment` on the opened row,
  `crates/email/src/message/za/view.rs:402`), from which the
  `$hasattachment` and `$hasnoattachment` keywords are answered.

And, per index: the **tombstones** of messages deleted since the segment
that holds them was written, and the **thread table** (thread id → its
members), which is derived from the thread records at load and not
stored.

The in-memory representation is the builder's. The content part must
answer, for a query, each `SearchFilter` leaf upstream's planner
produces (`Text` with `Standard`, `Exact` and `Prefix`, `KeyValue` on the
headers; see `crates/store/src/search/leaf.rs:51-115` and
`document.rs:256-281` for what each means) with a document-id bitmap,
including a phrase check with the positions upstream keeps
(`term.rs:656-729`), and give per document the sort keys, `sent_at` and
the attachment bit. The thread part must give, per document, the thread
id and the history, and per thread its members. `Leaf`, `term`, `codec`
and `AccountIndexer` are `pub(crate)` in `store`, so the index model
lives in `crates/store` or re-exposes what it reuses; the builder
chooses.

### 4.2 Segments and the manifest

**Segments** are blobs. A segment is the serialized records of the
messages one pass indexed (or, after compaction, of many passes), in two
sections that a loader can read separately: the thread section (every
thread record of the segment) and the content section (the postings and
content records), each zstd-compressed, then the whole sealed under the
search key (3.1). The serialization format is the fork's own, versioned
by `SEARCH_FORMAT_V1`; it is not an upstream rkyv archive read with
`access_unchecked`, so the "stored structs never change layout" rule of
release 1 does not bind it: a format bump is a version bump in the
manifest and a rebuild (5.4). Upstream's forward-index codec
(`Document::encode`, `term.rs:629-647`) may be reused for positions if
the builder re-exposes it; the builder chooses.

Each segment is written with `put_blob` through the server's wrappers
(`crates/common/src/storage/blob.rs`) and kept alive by a `Document`
link `hash | account_id | Collection::Principal | 0`
(`BlobLink::Document`, `crates/store/src/write/key.rs:410-421`): the
principal document that already carries the vault and pin records.
Unlinking a segment (compaction, rebuild, destroy) is clearing that
link; the blob purge deletes what no link holds
(`crates/store/src/write/blob.rs:152-280`). An owner can download a
segment raw as upstream lets an owner download any linked blob of their
own account; it is ciphertext under their own key, so this is harmless
and nothing is added to refuse it. No consumer but the loader parses a
segment.

**The manifest** is a new principal field, `PrincipalField::ZeroAccessSearch
= 152` (`crates/types/src/field.rs`, beside 150 and 151), on principal
document 0, in the clear, holding:

- the format version;
- `indexed_to`: the Email change id the index has reached
  (section 5.1);
- the segment list: for each segment its 16-byte random segment id, its
  blob hash, its sealed size and the byte offset of its content section;
- the rebuild cursor, when a rebuild is in progress (5.4).

Every write of it asserts `AssertValue::Hash` of the previous bytes, as
the pin record does (`za_pin_transact`,
`crates/common/src/auth/vault.rs:691-764`), or `AssertValue::None` when
creating it. It stays under 100,000 bytes (FoundationDB's chunking
applies only to the classes `may_be_chunked` lists,
`crates/store/src/write/key.rs:877-897`, which field 152 does not join):
compaction (5.5) bounds the segment count, and at about 50 bytes per
entry the list is a few kilobytes. The manifest is not sealed because the
destroy path (5.7) must find the segments to unlink them without a key,
and because what it holds (section 2) is already visible from the blob
store.

A manifest write never invalidates the account cache: it carries no
credential and changes no login outcome; the vault record stays the sole
authority for login (release 1 invariant 2 unchanged). It is not written
through `za_vault_commit` and does not touch the generation.

### 4.3 The in-memory index and its cache

The loaded index of an account lives in a new per-process cache,
`Caches::za_search`, keyed by account id (not by token: several sessions
of one account on a node share one loaded index). Today no per-account
structure exists; every `SessionKeys` is per login and IMAP and POP3
sessions own theirs outside the key cache (`crates/vault/src/cache.rs`,
`crates/common/src/za/mail.rs:63-82`), so the index cache is its own
structure with its own rules:

- **Load** only in a request that holds the account's keys: read the
  manifest, open each segment (verify the format byte and the AEAD under
  `swk` with the segment's id in the AAD), decompress and deserialize in
  `Zeroizing` buffers, merge, derive the thread table, apply the
  tombstones. The index records `indexed_to` from the manifest. A
  **thread-only load** reads only the thread sections and is what every
  keyed mail request needs (6.2); the content sections are loaded on the
  first content request (7.1). Both are loads of the same entry.
- **Serve** from memory: every resolution of section 7 reads the loaded
  index; no query reads a segment.
- **Evict** after `ZA_SEARCH_IDLE_SECS` (default 900) without use, when
  the process budget `ZA_SEARCH_MEM_MB` (default 512) needs room (least
  recently used first, content sections before thread sections), and on
  every invalidation that drops the account's keys
  (`CacheInvalidation::Account` and `AccessToken`,
  `crates/common/src/cache/invalidate.rs:581, 608`, where
  `za_keys.remove_account` runs). There is no maximum age: a reload is
  only a cost, and the search key does not change.
- **Account cap.** An account whose content sections would exceed
  `ZA_SEARCH_ACCOUNT_MAX_MB` (default 256) loads its thread sections
  only; its content requests get the unavailable refusal (7.4), with a
  trace, and everything else is served. This is the knob for a mailbox
  far past the design size. The thread sections are never subject to it.
- **Mutations** (the pass, 5.1) apply to the loaded index under its lock
  before the segment is written; a loaded index is therefore never behind
  what the node has committed, and other nodes catch up by re-reading
  the manifest (5.6).

The environment variables follow the fork's parse-and-fall-back
convention (`KeyCacheConfig::from_env`, `crates/vault/src/cache.rs:41-53`;
`lease_secs_from`, `crates/common/src/za/lease.rs:55-60`), read once and
logged at boot; registry settings are not added (CLAUDE.md).

### 4.4 Sizing and limits

The design size (1.3) gives a loaded index of tens of megabytes per
account and a budget of 512 MB for a node serving ten to twenty such
accounts' content at once; a deployment with more concurrent large
accounts raises `ZA_SEARCH_MEM_MB`. Eviction is cheap (the segments
stay), and a reload after eviction is the load cost of 4.3. The knobs:

| Variable | Default | Meaning |
|---|---|---|
| `ZA_SEARCH_MEM_MB` | 512 | process budget for loaded indexes |
| `ZA_SEARCH_ACCOUNT_MAX_MB` | 256 | largest content part a node loads |
| `ZA_SEARCH_IDLE_SECS` | 900 | eviction after disuse |
| `ZA_SEARCH_PASS_CAP` | 64 | messages one pass indexes |
| `ZA_SEARCH_PASS_MB` | 32 | opened message bytes one pass indexes |
| `ZA_SEARCH_WAIT_SECS` | 30 | how long a content request completes passes before refusing |
| `ZA_SEARCH_SEGMENTS` | 16 | segment count that triggers compaction |

The plan's benchmark (9.6) records the measured figures for the design
size against these defaults and adjusts them before release.

## 5. Building the index

### 5.1 The pass

A pass brings an account's index from `indexed_to` to the account's
current Email change id, or part of the way. It runs inside a request
that holds the account's keys, on the request's task, under the loaded
index's lock (one pass per account per node at a time; a second request
waits for the running pass, as the drain's lock does,
`crates/vault/src/session.rs:161-164`).

1. **Delta.** Read the change log keylessly:
   `store.changes(account_id, SyncCollection::Email, Query::Since(indexed_to))`
   (`crates/store/src/query/log.rs:118-183`). Item inserts give the
   document ids to index (the thread prefix in the high bits is ignored:
   it is the singleton); item deletes give tombstones; item updates are
   flag and mailbox changes, or the pass's own entries of 6.3, and need
   nothing (a key account's message content never changes in place; an
   IMAP COPY within the account rewrites the same document with the same
   content, `crates/imap/src/op/copy_move.rs:406-418`). An insert of a
   document the index already holds, and a delete of a document the
   message cache still holds, are skipped: this makes a repeated delta
   (5.6, a failed write) harmless. A truncated log
   (`Changes::is_truncated`) takes the reconciliation of 5.4 instead.
2. **Cap.** At most `ZA_SEARCH_PASS_CAP` inserts or `ZA_SEARCH_PASS_MB`
   of opened message bytes, oldest change first; deletes are not capped.
   The pass's target, `to`, is the change id of its last processed insert
   when capped, else the delta's `to_change_id`.
3. **Index.** For each insert: read the stub and the blob as the FETCH
   path does (`za_open_metadata_row`, `za_open_message_blob`,
   `crates/email/src/message/za/view.rs`), call `index_document`,
   `MessageSortKeys::from_envelope`, `za_has_attachment` and
   `ThreadFields::scan`, assign the thread (6.1), and add the document to
   the loaded index. A message whose stub or blob is missing or does not
   open is skipped with a trace carrying the ids (section 8), and the
   pass goes on: it is not indexed and not retried, as 1.0.2's index
   queue skips a message whose blob is missing. Every thread change the
   pass makes is recorded in the history stamped with `to`, which is
   known before anything is written.
4. **Write.** Serialize the pass's additions and tombstones as one
   segment, seal, `put_blob`, link. Then one batch: the manifest with
   `indexed_to = to` and the new segment appended, asserting the previous
   manifest bytes; and the log entries of 6.3 for every message whose
   thread changed. On an assertion failure (another node wrote, 5.6)
   unlink the segment just written, reload the manifest and the index
   from it, and return; the request proceeds on the reloaded index. On
   any other failure, unlink, trace, and leave `indexed_to` where it was:
   the next pass repeats the work. The loaded index is rolled back to the
   manifest's state in both cases, so memory never claims more than the
   store holds.
5. **Compact** when the segment count reaches `ZA_SEARCH_SEGMENTS`
   (5.5).

A pass that finds nothing to do (the common case: `indexed_to` equals
the account's last Email change id, read from the message cache's
`last_change_id`, `crates/common/src/lib.rs:281-289`) costs one
comparison and no I/O.

### 5.2 Triggers

The pass is a request-time hook, as the drain is for DAV
(`handle_dav_request`, `crates/dav/src/request.rs`): before a request of
a key account that holds its keys is served, run one pass. Concretely:

- JMAP: in the dispatcher (`crates/jmap/src/api/request.rs`), for every
  mail method of a key account once the keys are known to be on the
  token (`access_token.za_keys_for(account_id)`); one pass per request,
  not per method call.
- IMAP: before each command of a session that `za_key_session()`
  answers true for (`crates/imap/src/core/mod.rs:303-306`), after the
  command is parsed, except IDLE, NOOP and LOGOUT. A client in IDLE
  learns of new mail from EXISTS and fetches it; that FETCH runs the
  pass, so the message's THREADID and EMAILID are final by the time the
  client reads them, in normal use.
- POP3: no pass. Nothing POP3 serves depends on the index.
- A keyed write that adds a message (IMAP APPEND, `Email/set` create,
  `Email/import`): one pass after its commit and before its response. In
  normal use the delta is that message and the response carries its
  final id (6.2); after a backlog (a rebuild, a long absence) the pass
  may not reach it, and the response carries the id as loaded, which a
  later pass reassigns and renders as 6.3 renders every reassignment.
  The write itself does not index; it is picked up from the log like a
  delivery.

Two classes of request follow from the two kinds of record (4.1):

- A **content request** (a search with a content filter or sort, a
  snippet, IMAP SEARCH and SORT with content keys, `Email/queryChanges`
  with such a filter) runs passes until the index is current or
  `ZA_SEARCH_WAIT_SECS` has elapsed, then answers from a current index or
  refuses (7.4).
- Every other keyed request runs one pass and then proceeds with the
  thread map **as loaded**: the thread-valued reads of 6.2 (`Email/get`
  ids and `threadId`, `Thread/get`, `Thread/changes`, `Email/changes`,
  `Email/queryChanges` without a content filter, `collapseThreads`,
  `inThread`, the thread filters and sorts, `Mailbox/get`'s thread
  counts, `Blob/lookup`, `EmailSubmission/*`, IMAP `THREAD`, `EMAILID`,
  `THREADID`) answer from whatever the map holds after that pass, and are
  never held or refused by the index. Between the pass and a current map
  lies only the capped remainder of a backlog, whose reassignments later
  passes render as 6.3 renders them; in normal use the map is current
  after the pass.

The budget bounds a content request's latency during a rebuild or after
a long absence; in normal use the delta is what arrived since the last
keyed request, indexed in milliseconds. The wait is not a background
job: release 1 invariant 6 stands, no task holds keys, and a pass that
outlives its request is impossible because it runs on the request's
task. Mail that arrives while no keyed request comes is indexed by the
first one that does.

### 5.3 Keyed writers

IMAP APPEND, `Email/set` create and `Email/import` already run with keys
and seal their messages under the mail key (mail spec 6.2). Their only
change is the pass of 5.2 after the commit, which indexes and threads
the new message, and the mapped id in their responses (6.2). The
`PendingThread` recheck and `MergeThreads` scheduling of the plain
branches (`crates/email/src/message/ingest/recheck.rs`) stay off for key
accounts: threads merge in the pass (6.1).

### 5.4 Rebuild and reconciliation

A **rebuild** starts when a keyed request finds no manifest on an
account that holds mail, a manifest of another format version, a
segment that is missing or does not open, or a manifest that does not
parse. It writes a fresh manifest (empty segment list, `indexed_to` set
to the account's current Email change id, and a rebuild cursor), unlinks
every previous segment, and from then on each pass draws its inserts
first from the cursor, newest document id first, then from the change
log since `indexed_to`. The cursor is the set of document ids present in
the message cache when the rebuild started, walked downwards; the
manifest stores its lower bound. Newest first means recent mail is
searchable while older mail is still being indexed. The rebuild is done
when the cursor is exhausted; until then the content part is
incomplete, so a content request waits for it up to the budget and
refuses beyond (7.4), while the thread map serves as loaded and
reassigns as the rebuild reaches older mail (6.3): a rebuild shows
clients one round of thread reassignments.

A **reconciliation** runs when the change log is truncated below
`indexed_to` (`is_truncated`; the log keeps `maxChangesHistory`
records, 10,000 by default): the pass compares the index's document set
with the message cache's, tombstones what the cache lacks, and indexes
what the index lacks through a cursor over the difference, then sets
`indexed_to` to the cache's change id. It is the rebuild restricted to
the difference, and shares its machinery.

A product account gets an empty manifest at setup (`za_setup`,
`crates/http/src/api/vault.rs:1081`), in the batch that writes the vault
record, with `indexed_to` set to the account's current Email change id
(the account holds no mail: mail spec 6.6's refusals). It therefore never
rebuilds unless the index is lost.

### 5.5 Compaction

When a pass's manifest write would list `ZA_SEARCH_SEGMENTS` or more
segments, the pass merges the loaded index into one new segment, writes
it, and writes a manifest naming only it (same assertion as 5.1 step 4),
then unlinks the old segments. Tombstones and the pruned part of the
thread history are dropped at compaction: a thread transition older than
the change log's oldest retained change id can never be asked for
(`Email/changes` with an older state gets `cannotCalculateChanges`), so
it is pruned. Compaction needs the content sections loaded; a node that
holds only the thread sections (the account cap, or an eviction) skips
compaction and the segment count grows until a node that holds them
compacts. Compaction is the only write proportional to the whole index;
at the design size it is a second or two every sixteen passes.

### 5.6 Cluster

Two nodes with keyed sessions of one account run passes independently.
The pass is deterministic: the same change-log delta, processed in change
order with the same thread rules, yields the same additions, the same
thread assignments and the same segment bytes before sealing. Two nodes
may therefore do the same work; correctness comes from the manifest
assertion, not from a lock: exactly one of two concurrent manifest writes
lands, the other node unlinks its segment, reloads and continues. A node
whose loaded index is behind the manifest (another node advanced it)
notices at its next pass, since `indexed_to` in its memory is older than
the manifest's; it reloads before processing. A node serves from its
loaded index between passes, so a search on one node can miss a message
another node indexed seconds ago until the next request's pass reloads.
That is the bound: one request.

The UID lease (`crates/common/src/za/lease.rs`) and the index queue's
partition lock are not used: neither prevents anything the assertion
does not already prevent, and a lease would add a sweep.

### 5.7 Account lifecycle

- **Setup** writes the empty manifest (5.4).
- **Password change, recovery, recovery-key regeneration, app
  passwords** change nothing: `swk` is derived from `mk`. The account
  invalidation these cause evicts the loaded index (4.3), which reloads
  on the next keyed request.
- **Destroy.** Account destruction clears the Property subspace by range
  (`danger_destroy_account`, `crates/store/src/dispatch/store.rs:428`),
  which removes the manifest, but blob links are hash-prefixed and
  `destroy_account_blobs` (`crates/services/src/task_manager/tasks/destroy_account.rs:291-366`)
  walks only Email, FileNode and SieveScript archives. The fork adds one
  step before the range delete: read the manifest, unlink every segment
  it names. A segment the manifest no longer names (an orphan of a failed
  pass, 5.1 step 4) was unlinked by the pass that failed; the purge
  removes it. The za suite's `assert_is_empty` exempts field 152 and the
  segments while their account exists, as it exempts fields 150 and 151,
  and the suite's destroy-then-purge-then-assert order verifies that
  nothing remains.
- **Provisioning fence.** No ordinary writer touches the index, so no
  fence is added; `za_assert_no_data` need not check field 152 (an
  unprovisioned account cannot have one).

## 6. Threads

### 6.1 Assignment

The pass threads a message as upstream's ingest does for an ordinary
account (`find_thread_id`, `crates/email/src/message/ingest.rs:1043-1142`,
and `ThreadMerge::merge`, `:1430-1455`), against the index instead of the
`Threading` key class:

- Candidates are the indexed messages with the same subject thread-name
  hash whose Message-ID family hashes intersect the new message's.
- No candidate: the message is a new thread whose id is its own document
  id.
- One thread among the candidates: the message joins it.
- Several: they merge into the one with the most members (ties to the
  lowest thread id), the new message joins it, and every member of the
  losing threads is reassigned, each with a thread-history entry stamped
  with the pass's `to`.

Upstream reads at most the 256 most recent rows of a subject bucket
(`THREAD_SCAN_WINDOW`) and merges across the whole bucket in
`MergeThreads`; the index has every member in memory and looks them all
up, a superset that yields the same grouping on upstream's own test
corpora (section 9's `thread_merge` variant pins it).

The thread id of a message is thus the document id of some member of its
thread, stable until a merge reassigns it, and held only in the index.
The stored `MessageData.thread_id`, the `Thread` container the stub's
ingest logs and the change log's prefixes stay the singleton (mail spec
5.4 unchanged at the store).

### 6.2 Ids

Upstream's JMAP Email id is `(thread_id << 32) | document_id`
(`Id::from_parts`, `crates/types/src/id.rs:67-69`), so the thread id is
inside every Email id a session emits, and the singleton is inside every
Email entry of the keyless change log (`build_id`,
`crates/store/src/write/log.rs:316-322`). For a key account, a keyed
session **maps** on the way out and **checks** on the way in, from the
thread map as loaded (5.2):

- Every Email id emitted carries the map's thread id as its prefix:
  `Email/get` (ids and `threadId`, `crates/jmap/src/email/get.rs:185-196, 421-425`),
  `Email/query` results, `Email/changes`, `Email/queryChanges`,
  `Email/set` and `Email/import` responses
  (`ingested_into_object`, `crates/jmap/src/email/mod.rs:29-37`),
  `Thread/get` (`crates/jmap/src/thread/get.rs:98-104`), `Blob/lookup`
  (`crates/jmap/src/blob/get.rs:188-193`), `EmailSubmission/get`'s
  `emailId` and `threadId` (the submission record stores the singleton
  prefix, never the mapped one: `crates/jmap/src/submission/set.rs:449-450`
  takes it from the request id, so the set path writes the document id
  as the prefix), IMAP `FETCH` `EMAILID` and `THREADID`
  (`crates/imap/src/op/fetch/mod.rs:585-592`).
- Every Email id a client supplies to a keyed method is translated
  before upstream's lookup: 1.0.3's `Email/get` resolves an id with
  `cache.email_by_jmap_id(id)`, whose prefix comparison would reject
  every mapped id against the cache's singleton. The fork checks the
  supplied prefix against the map and hands the lookup the singleton
  form; a prefix that is not the message's current thread id answers
  `notFound`, as 1.0.2 answers for an id a thread merge destroyed.
  `SEARCH EMAILID` requires the prefix to match
  (`crates/imap/src/op/search.rs:565-574`) through the map, and `SEARCH
  THREADID` resolves the thread through the thread table.
- Thread ids a client supplies (`Thread/get`, `inThread`,
  `EmailSubmission` `threadIds`) resolve through the thread table; an
  unknown one is `notFound`, as upstream.
- The thread counts of `Mailbox/get` (`totalThreads`, `unreadThreads`,
  `crates/jmap/src/mailbox/get.rs:185-189`), `collapseThreads`,
  `inThread`, the `*InThreadHaveKeyword` filters, the thread-keyword
  sorts and IMAP `THREAD` group by the map's thread id instead of the
  cache's (the cache's `thread_ids` column holds singletons and is left
  as it is).

**Keyless sessions.** Mail spec 3.3 lets a key account's keyless tokens
(a Bearer or OAuth token, and the WebSocket and EventSource sessions,
which the HTTP layer strips of keys, `crates/http/src/request.rs:342, 359`)
run the methods that need no content: `Email/query`, `Thread/get`,
`Thread/changes`, `Email/changes`, `Email/queryChanges`, `Mailbox/get`,
`Blob/lookup`, `EmailSubmission/*` and `Email/set` flag and mailbox
updates, all gated with `za_assert_mail_route` only
(`crates/jmap/src/api/request.rs:296-455, 546-556, 816`), and promises
the web client state-driven sync over the WebSocket. Every one of these
emits or takes Email ids, so a keyless session would see singleton
prefixes where a keyed one sees mapped ones: two representations of one
account's ids for one client. Jay decided this on 2026-11-05 (section
13, decision 1):

- the methods that emit a thread id or a thread-prefixed Email id are
  gated on keys for a key account, like `Email/get` (`za_assert_mail_keys`,
  `request.rs:865-874`), and a keyless session gets the keyless refusal
  there, as it does on every content read;
- `Email/set` flag and mailbox updates stay keyless: the lookup is by
  document id with the prefix unchecked, as 1.0.1 looked ids up, and the
  response echoes the supplied id; a stale prefix is not detected there,
  a difference from 1.0.2's `notFound` recorded in 7.5;
- EventSource stays keyless and unaffected: `StateChange` carries change
  ids and types, no Email ids, and a key account's content push is
  already none.

### 6.3 Changes

Upstream renders a thread merge as the old id destroyed and the new id
created (`merge_index`, `crates/common/src/storage/index/build.rs:414-443`),
and 1.0.2's `Email/queryChanges` with `collapseThreads` re-emits every
message of a thread that a created or destroyed id touched. A key
account's keyless log never shows a merge (the singleton never changes),
so the keyed session renders it from the thread history:

- The pass that reassigns a message writes, keylessly, in the batch of
  5.1 step 4, an Email item update of the message's singleton id
  (`log_item_update`, `crates/store/src/write/batch.rs:379-474`) and a
  Thread container property change of the singleton
  (`log_container_property_change`). An item update is what the store
  can hold: `log_item_insert` folds a delete and an insert of one id in
  one batch into an update (`crates/store/src/write/log.rs:369-382`), and
  `Changes::push_change` discards an insert a later record deletes
  (`crates/store/src/query/log.rs:347-395`); an update reloads the cache
  row, bumps the Email and Thread states and the message's modseq, and
  gives the renderer below a raw entry to work from. The entries carry
  ids only. The history records `(to, thread before)` for the message.
- `Email/changes` since state `S`, per document `D` in the raw delta:
  `T0` is `D`'s thread as of `S` (the last history entry stamped at or
  below `S`, else the current one if `D` had no change after `S`, else
  the singleton if `D` was unindexed at `S`), `T1` its thread now. `D`
  created after `S`: created `T1|D`. `D` destroyed after `S`: destroyed
  `T0|D`. `T0 ≠ T1`: destroyed `T0|D`, created `T1|D`. Else updated
  `T1|D`. A pass's stamp `to` is at or below the change id of its own
  update entry, so a client whose `S` lies between the two sees the
  update rendered as an update, which is harmless: it refetches.
- `Thread/changes` since `S`: the threads touched, derived the same way:
  a thread that had no member as of `S` is created, one that has none
  now is destroyed, else updated. `Thread/get`'s `state` stays the
  change id of the Thread log, which the pass's batch advances.
- `Email/queryChanges` is `email_query` plus the diff of ids
  (`crates/jmap/src/changes/query.rs:305-330`), both mapped, so
  `collapseThreads` renders the representative change as 1.0.2 does
  (its `touched_threads` are collected from mapped prefixes and matched
  against the map's thread id, v1.0.3 `changes/query.rs:95-123`).
- A thread history entry older than the change log's oldest retained
  change id is pruned at compaction (5.5); a client asking since an older
  state gets `cannotCalculateChanges` from the log itself.

Push: a key account's content push is already none
(`build_email_push_object`, `crates/services/src/state_manager/email_push.rs:62-70`),
and `StateChange` carries change ids, not thread ids; nothing is added.

### 6.4 What stays off

Delivery still cannot thread (it holds no key), so a delivered message
is a singleton until the next keyed request's pass; the SMTP duplicate
drop (`checks::has_duplicate_in`) and the `trusted_reply` spam
heuristic, which upstream evaluates from the thread index at delivery,
stay off for key accounts, as mail spec 5.4 records. `IngestGroup`
threading (upgrade ruling 59) stays off. `MergeThreads` tasks are never
scheduled for a key account; the setup refusal that counts them (mail
spec 6.6) is unchanged.

## 7. Serving

### 7.1 JMAP `Email/query`, sorts, filters

`email_query` keeps upstream's shape (`crates/jmap/src/email/query.rs:105-246`):
the keyless conditions go to the row filter over the message cache, the
text conditions and the nested residual go to a `SearchQuery`, and the
result bitmap is intersected with the row filter
(`filter.rs:105-108`). For a key account whose content part is loaded
and current, the fork replaces the one call into the search store
(`filter_account`, `query.rs:181-196`):

- Every `Text` and `KeyValue` leaf of the residual is resolved against
  the index into a `DocumentSet`; `Integer` leaves other than the account
  id do not occur in Email queries. Resolution is exhaustive: a leaf the
  index cannot answer refuses the query (7.4), since
  `SearchQuery::filter` treats an unresolved leaf as absent and widens
  the result (`crates/store/src/search/local.rs:109-113`). No leaf of a
  key account's query reaches `SearchStore::filter_account`, and the
  PostgreSQL-only windowed walk (`walk_windowed`,
  `crates/jmap/src/email/query/window.rs:31-92`, `supports_windowed_walk`,
  `crates/store/src/dispatch/search.rs:148-162`) is not taken for a key
  account.
- The resolved tree is merged with upstream's own
  `SearchQuery::new(SearchIndex::Email).with_filters(..).with_mask(readable).filter()`:
  a pure function, no store.
- `from`, `to` and `subject` sorts become
  `SearchComparator::sorted_set` ranks computed from the index's sort
  keys with upstream's rule (pad to 40 bytes, sort, dense rank,
  `sortkeys.rs:202-218`), passed as `MessageComparator::Search`, which
  `query_emails` takes as given (`sortkeys.rs:120`). `sentAt` sorts, and
  `sentBefore`/`sentAfter`, use the index's `sent_at` instead of the
  cache's. `hasAttachment` and the attachment keywords use the index's
  bit. The `threadId` sort stays `unsupportedSort` for every account
  (upstream, `query.rs:681-685`).
- `collapseThreads`, `inThread` and the thread filters and sorts use the
  map (6.2).

The refusal classifiers of `crates/jmap/src/email/za.rs`
(`filter_needs_content`, `sort_needs_content`, `query_filter_needs_content`,
`za_refuse_content_query`) stay as the decision of *whether the content
part is needed*; what follows a "needs content" verdict is now "ensure
the content part is loaded and current, then resolve" and only on
failure the refusal of 7.4. `Email/queryChanges` keeps its early check
(`changes/query.rs:69-77`, plan 13 R38) for the same reason.

### 7.2 IMAP SEARCH, SORT, THREAD

`SessionData::query` (`crates/imap/src/op/search.rs:292`) builds the same
`SearchFilter` tree (`:587-736`) and sort list (`:740-775`) and calls
`query_emails`; the fork resolves the content leaves and sorts exactly as
7.1 before that call. `SENTBEFORE`, `SENTON` and `SENTSINCE` read the
index's `sent_at` instead of the cache's (`:431-458`); `HEADER` keys are
`KeyValue` leaves on the index's headers; `TEXT` is upstream's `Or` over
the fields, resolved per field. `THREAD` (`crates/imap/src/op/thread.rs:91-104`)
groups by the map's thread id. `KEYWORD $HasAttachment` is served. The
mixed-key rule of mail spec 8 (a search that mixes a served key with a
refused one is refused as a whole) survives only in 7.4's unavailable
case.

### 7.3 Snippets

`SearchSnippet/get` already opens the stub and the blob with keys
(`crates/jmap/src/email/snippet.rs:164-301`), and upstream's snippet
generator works on the opened text without the index
(`crates/nlp/src/language/search_snippet.rs:57-122`); the refusal
(`za_refuse_snippet_terms`, `za.rs:176-189`) is lifted and nothing else
changes but the id translation of 6.2 on the supplied `emailIds`.
Snippets of an ordinary account are produced the same way, so parity
holds including upstream's quirks (one language for all conditions,
substring matching).

### 7.4 Availability and the refusal

The refusal of mail spec 8 stays as the fallback for content requests
and keeps its shape (`NO [CANNOT]` with a fixed text on IMAP,
`unsupportedFilter` / `unsupportedSort` on JMAP), with a second fixed
text for this release: `ZA_MAIL_SEARCH_UNAVAILABLE`, "Search for this
account is not available right now; try again shortly." It is answered,
for a content request only, when:

- the content part cannot be loaded: the manifest or a segment fails and
  a rebuild has started (5.4), the account exceeds
  `ZA_SEARCH_ACCOUNT_MAX_MB`, or the budget cannot make room;
- the index is not current after `ZA_SEARCH_WAIT_SECS` of passes (a
  rebuild or a reconciliation in progress, or a very long absence);
- a leaf cannot be resolved (a defect, traced as such).

A content request is never answered from an index that is behind the
request's view of the account, and never by decrypting and scanning
(mail spec 14's rejection stands). Partial answers are not given: an
IMAP client cannot tell a partial SEARCH from a complete one. The
original text, `ZA_MAIL_SEARCH_REFUSED`, is kept for the keyless case
(a session without keys), where the server still cannot search.

The thread-valued reads of 6.2 are never refused by the index: they
answer from the map as loaded after the request's pass (5.2), which in
normal use is current, and during a rebuild or a backlog is the map so
far, with later reassignments rendered by 6.3. `Email/get`, flag
updates, mailbox listing, FETCH of content and every keyed write never
wait for the content part.

### 7.5 Parity

The contract is the one release 5 made for scheduling (its section
3.1): what a key account's client observes from a search, a sort, a
snippet, a thread or a changes call is what an ordinary account's client
observes for the same mail and the same requests, in the same order,
apart from the recorded differences:

- freshness (section 2: indexing waits for a keyed request), and the
  one-request bound between nodes (5.6);
- the unavailable refusal of 7.4, which an ordinary account never
  gives;
- the keyless refusals of mail spec 26 and 6.2, and the unchecked
  prefix on a keyless `Email/set` update (6.2);
- the thread window superset of 6.1;
- the one round of reassignments a rebuild shows (5.4);
- what 6.4 keeps off at delivery.

Parity is by construction where it matters: the same `index_document`,
the same `MessageSortKeys`, the same `ThreadFields::scan`, the same
`Leaf` plan semantics, upstream's own `SearchQuery::filter` merge and
`query_emails` ranking. Section 9.5's differential test holds it.

## 8. Error handling and traces

Traces of this release carry ids and counts and nothing else: no term,
header, Message-ID, subject, sort key, thread membership or
client-supplied string (mail spec invariant 30 extends to the index).
The fork adds no event variants (the enums are generated); it traces
through the fixed-constant convention of release 5
(`crates/common/src/za/imip.rs`'s `ZA_IMIP_*` strings on an existing
event), with these constants:

| Constant | When | Carries |
|---|---|---|
| `ZA_SEARCH_PASS` | a pass wrote a segment | account id, inserts, tombstones, reassignments, `indexed_to` |
| `ZA_SEARCH_SKIPPED` | a message was skipped in a pass (step 3) | account id, document id, the error class |
| `ZA_SEARCH_LOST` | a manifest write lost its assertion | account id |
| `ZA_SEARCH_REBUILD` | a rebuild or reconciliation started or finished | account id, which, the cursor size |
| `ZA_SEARCH_UNAVAILABLE` | a content request was refused by 7.4 | account id, the reason class |
| `ZA_SEARCH_EVICTED` | the cache evicted an index or its content part | account id, the reason (idle, budget, invalidation) |
| `ZA_SEARCH_UNRESOLVED` | a leaf could not be resolved (a defect) | account id, the leaf's field |

A failure inside the pass never fails the request that triggered it,
except the content requests of 5.2, which refuse by 7.4; a flag update
whose pass failed is served, with the trace. A failure to open a
segment at load is `ZA_SEARCH_REBUILD`, not an error to the client. A
store error reading the manifest or the change log fails the pass and,
for a content request, refuses; the request is otherwise served from the
map as loaded, or with singleton ids if nothing is loaded, which a later
pass reassigns.

## 9. Testing

### 9.1 Unit

In the crate that holds the index model: the segment codec round-trips
a populated index, and the thread sections load without the content
sections; a segment sealed for one account or segment id does not open
under another; the thread rules reproduce `find_thread_id` and
`ThreadMerge::merge` on a fixture of references and subjects, merges
included; the change rendering of 6.3 over a thread history gives
upstream's destroyed/created pairs, the update for a client between the
stamp and the entry included; the phrase check agrees with
`matches_phrase` on positioned fields and reduces to co-occurrence on
`Language::None` fields (`term.rs:728`); the manifest stays under
100,000 bytes at `ZA_SEARCH_SEGMENTS`; the environment variables parse
and fall back. In `crates/vault`: `swk` derivation is stable across a
rewrap.

### 9.2 The za suite

`tests/src/za/mail/search.rs` turns around: its refused tables become
served tables with expected results, keeping the keyless cases refused.
New modules, in the mail block after `search` and before `traces`
(`tests/src/za/mod.rs:214-222`), with the next free names `keymail36`
and `plainmail41` onwards:

- `mail::threads`: the same conversation delivered to a key account and
  a plain control gives the same `Thread/get` groupings, `Email/query`
  with `collapseThreads`, IMAP `THREAD`; a reply delivered while no keyed
  request runs is a singleton until the next keyed request, then joins;
  a merge of two existing threads renders in `Email/changes` and
  `Thread/changes` as the plain control's does, and in
  `Email/queryChanges` with `collapseThreads` as 1.0.2 renders it; the
  pass's log entry is an item update of the singleton and nothing else;
  a client-supplied id with the old prefix is `notFound` on `Email/get`
  and served on a keyless `Email/set` flag update; a keyless token is
  refused on every gated method of 6.2; `Mailbox/get` thread counts
  match the control; a keyed APPEND's response carries the final id.
- `mail::index`: persistence (a second session serves a search without
  opening a message blob, by a new test-mode counter of opened message
  blobs); a thread-only load serves `Email/get` and `Thread/get` without
  the content sections; catch-up after offline deliveries; the pass
  caps and the wait budget (hook `SEARCH_PASS_PAUSE`); compaction at
  `ZA_SEARCH_SEGMENTS` leaves one segment and the old blobs unlinked and
  purged; a rebuild from a deleted segment, a deleted manifest and a
  bumped format version, with the one round of reassignments rendered
  and ids consistent before, during and after; a reconciliation after a
  truncated log (a lowered `maxChangesHistory` for the case); a lost
  manifest assertion (hook `SEARCH_COMMIT_PAUSE` with a competing write
  in the gap) unlinks the loser's segment and serves the winner's;
  eviction by idle, by budget and by invalidation, and the reload after
  each; the account cap refuses searches and serves everything else; a
  skipped message (hook `SEARCH_OPEN_FAIL_ONCE`) is absent from results
  and traced with ids only; a message deleted between two passes is
  gone from results before compaction; the manifest is created at setup
  and no account without mail ever rebuilds; a repeated delta (the
  pass's own update entries, a replayed insert) changes nothing.
- The existing `mail::search` cases that assert `key.terms == 0` and
  `documents_are_ids_only()` stay: the internal search store still holds
  no term of a key account (invariant 27).

### 9.3 Variants that narrow

`tests/src/jmap/za_variants.rs`'s `query`, `search_snippet`,
`query_changes`, `thread_get` and `thread_merge`, and
`tests/src/imap/za_variants.rs`'s `search` and `thread`, assert
refusals and singletons today. With this release they assert upstream's
outcomes, with a keyed pass run where upstream's body waits on the index
queue (the variant's wait replaces the queue wait), plus the keyless
refusal on a keyless token. Where a variant becomes upstream's body plus
that difference, it says so and mirrors the body (plan 13 R44).
`tests/src/za/websocket.rs` and `mail::gates` gain the keyless-refusal
cases of 6.2: a key account's WebSocket session and a Bearer token are
refused on each gated method and still get `StateChange` pushes.

### 9.4 Leak coverage

`leak::test_mail` gains the rules: every blob linked to the account's
principal document is a sealed segment (format byte, no canary); field
152 holds no canary and parses as a manifest; no `Threading` key, no term
and an ids-only document in the internal search store, as today; the
change log holds no entry but item updates and Thread property changes
for a reassignment; after the suite destroys its key accounts and purges,
no segment remains. The mail canaries (`VISIBLE_CANARIES`,
`tests/src/za/leak.rs:235`) are searched for in every segment and in the
manifest, a negative control being a plain account's segment-less state
plus a planted plaintext blob.

### 9.5 Differential test

A corpus of a few hundred messages (threads, replies, forwards, attachments
with text, non-ASCII subjects, phrases) delivered to a key account and a
plain control; for every IMAP SEARCH and SORT form of `search.rs`'s
tables, every JMAP filter, sort and `collapseThreads` combination of the
`query` variant, and `SearchSnippet/get` on each result, the key
account's answer equals the control's (ids compared through the thread
map). This is the parity contract of 7.5, run in the za suite.

### 9.6 Benchmark

The plan's first task, recorded in its outcome note: a key account
holding 1 GB of generated mail; the index's loaded size with and without
positions, the thread sections' size, the load time of each, the pass
rate (messages per second with and without attachment text), the
compaction time, and the first-search latency after an absence of one
day's volume. The figures decide the segment format's details and
confirm or adjust the defaults of 4.4 before the rest of the plan is
executed.

### 9.7 Mutation sample

The plan runs the fork's mutation sample on: the AAD's segment id (a
segment of another account must not open), the assertion on the manifest
write (a lost write must not advance `indexed_to`), the exhaustive leaf
resolution (an unresolved leaf must refuse, not widen), the prefix check
on supplied ids, the key gates of 6.2, the item-update log entry of a
reassignment (its absence must leave a client without a refetch signal,
caught by the `Email/changes` case), the history stamp (a stamp above the
entry's change id must break the rendering case), the thread-history
pruning bound, the skip of an already-held insert, and the destroy
step's unlink.

## 10. Invariants for implementers

Mail spec invariants 24 to 32 hold, with these amendments:

- Invariant 27 ("No threading index, no full-text term on the internal
  search store ... its search document holds arrival time and size
  only") holds as written at the search store (and at 1.0 the document
  holds the ids only, upgrade ruling 64); the sealed index of this
  release is not on the search store and is covered by 48 below.
- Mail spec 5.4's "clients thread by headers themselves" and section 8's
  refusals are superseded by sections 6 and 7; the refusal text stays for
  the keyless case and the unavailable case.
- Mail spec 3.3's keyless-route contract is amended (section 13,
  decision 1): the methods of 6.2 that emit a thread id or a
  thread-prefixed Email id require keys for a key account; a key
  account's WebSocket and EventSource sessions and its Bearer tokens get
  push and nothing else of mail.

Added:

48. A key account's search terms, positions, sort keys, threading fields,
    thread ids and thread history exist outside process memory only in
    segments sealed under the search key, bound to the account and the
    segment id. The manifest holds versions, change ids, segment ids,
    blob hashes, sizes and offsets, and nothing derived from content. The
    stored thread id of every key account's message, its `Thread`
    container and every entry of the change log stay the message's own
    document id.
49. The index is loaded, built and written only inside a request that
    holds the account's own session keys, on that request's task; no
    background task, delivery, or keyless path reads or writes it, and a
    loaded index is dropped on every invalidation that drops the
    account's keys. Release 1 invariant 6 is unchanged.
50. The index is a deterministic function of the account's present mail
    and the change log's order. `indexed_to` advances only in a manifest
    write that asserted the previous manifest; a pass whose write fails
    or loses its assertion leaves the store and the loaded index as the
    manifest describes, and the next pass repeats the work, which a
    repeated delta does not disturb. Two nodes' concurrent passes are
    identical work decided by the assertion.
51. Every content leaf and content sort of a key account's query is
    resolved from a current content part or the query is refused; no
    content leaf, sort or snippet of a key account is ever answered by
    the search store, by decrypting and scanning, or from an index behind
    the request's view of the account. Results are merged, masked, ranked
    and paged by upstream's own code.
52. A thread id of a key account leaves the server only inside a keyed
    session's response, mapped from the thread map as loaded after the
    request's pass; every Email id such a session emits carries it, every
    id supplied to a keyed method is checked against it, and a
    reassignment is rendered as upstream renders a merge, from the thread
    history. The thread-valued reads are never held or refused by the
    index.
53. The pass's keyless writes are ids only: the manifest, the segment
    link, an Email item update and a Thread property change of the
    singleton for each reassigned message.
54. Traces of the index carry account and document ids, counts, change
    ids and reason classes, never a term, header, Message-ID, sort key,
    query string or thread membership.
55. Destroying a key account unlinks every segment its manifest names
    before the manifest is removed, and the blob purge then holds
    nothing of the index.

## 11. Merge hotspots and silent dependencies

New hotspots, fork insertions in upstream files: `crates/jmap/src/email/query.rs`
(the resolution before `query_emails`, the mapped ids, `windowed = None`),
`crates/jmap/src/email/get.rs` and the cache's `email_by_jmap_id` (ids,
`threadId`, the prefix translation), `crates/jmap/src/email/{set,import}.rs`
(the pass after commit, the mapped response, the unchecked keyless
update), `crates/jmap/src/changes/{get,query}.rs` (the rendering of
6.3), `crates/jmap/src/thread/get.rs`, `crates/jmap/src/mailbox/get.rs`
(thread counts), `crates/jmap/src/blob/get.rs` (`Blob/lookup`),
`crates/jmap/src/submission/{set,get,query}.rs` (the singleton stored,
the mapped read), `crates/jmap/src/api/request.rs` (the pass trigger and
the new key gates), `crates/imap/src/op/{search,thread}.rs`,
`crates/imap/src/op/fetch/mod.rs` (object ids), `crates/imap/src/core/session.rs`
or wherever commands dispatch (the pass trigger), `crates/services/src/task_manager/tasks/destroy_account.rs`
(the unlink step), `crates/types/src/field.rs` (field 152),
`crates/vault/src/{keys,session}.rs` (the search key), `crates/common/src/cache/`
(`za_search` and its invalidation arms), `crates/http/src/api/vault.rs`
(the empty manifest at setup), and `crates/store/src/search/` where the
index model re-exposes `pub(crate)` items.

Silent dependencies, each a re-read when upstream changes it: the term
set and positions come from `index_document` and `tokenize`
(`crates/email/src/message/index/search.rs`, `crates/store/src/search/{tokenize,term}.rs`),
and the leaf semantics from `Leaf::plan` (`leaf.rs`) and
`SearchFilter::has_text` (`document.rs:256-281`); a change there changes
what the index must answer, and the differential test (9.5) is the
detector. The thread rules mirror `find_thread_id` and
`ThreadMerge::merge` by hand (`ingest.rs:1043-1142, 1430-1455`), as the
drain mirrors `itip_ingest`. The change rendering of 6.3 mirrors
`merge_index` (`build.rs:414-443`) and 1.0.2's `Email/queryChanges`, and
rests on the change log folding a delete and an insert of one id into an
update (`write/log.rs:369-382`) and on `push_change`'s rules
(`query/log.rs:347-395`). The Email id layout (`Id::from_parts`), the
log's prefixing (`build_id`, `SyncCollection::is_prefixed`) and 1.0.3's
`email_by_jmap_id` lookup are assumed; a change to any changes 6.2. The
sort ranks mirror `message_sort_ranks` (`sortkeys.rs:134-218`) and pass
through `query_emails` as precomputed comparators (`sortkeys.rs:120`).
`SearchQuery::filter`'s treatment of an unresolved leaf as absent
(`local.rs:109-113`) is why resolution must be exhaustive. The
manifest's size bound rests on `may_be_chunked` (`key.rs:877-897`). The
purge's rule that a `Document` link keeps a blob (`blob.rs:328-372`) and
`destroy_account_blobs`'s walk are what 5.7 relies on. The in-memory
compare of 5.1 rests on the message cache's `last_change_id` being the
account's current Email change id.

## 12. Alternatives considered

- **Keyed term hashes in the shared search store** (a per-user secret
  hashing each token, riding upstream's `Term` keys and planner).
  Rejected with Jay: the operator controls delivery, so they can inject a
  message of chosen words, watch which posting lists gain its document
  id when the person's session indexes it, learn `hash(word)` for every
  chosen word and test every other message with them: the file-injection
  attack on searchable encryption. The `Document` forward index makes it
  worse (every message's term list in hashed form), and upstream stores
  short tokens verbatim and range-scans prefixes, which keyed hashes
  cannot give. The mail spec's note that delivery holds no secret was the
  same objection.
- **Decrypting and scanning at query time**, as the calendar does.
  Rejected for mail in the mail spec (section 14) and still: it works and
  then stops working as mailboxes grow. It stays the calendar's answer
  while calendars are small.
- **Singleton Email ids with a mutable `threadId`.** Keep every Email
  id of a key account as `document|document` forever and substitute only
  the `threadId` property and the thread-valued methods, rendering a
  reassignment as an Email update. About a third fewer sites than 6.2
  and no prefix checks. Rejected: RFC 8621 makes `threadId` immutable,
  upstream relies on it (a merge destroys and recreates the id, and
  1.0.2's `notFound` on a stale prefix depends on it), and the fork's
  variants compare key accounts with upstream's bodies; a client that
  caches `threadId` would hold a stale one with no signal. The mapped
  ids give parity.
- **A keyless singleton representation alongside the mapped one** (the
  keyless methods keep serving singletons). Rejected: a client that
  learns ids over the WebSocket and fetches over HTTP would hold two ids
  for one message; section 13's decision 1 records the two tenable
  answers and the one taken.
- **A session-scoped background task holding keys** to index after
  login, so the first search never waits. Rejected: it is background code
  holding a key (release 1 invariant 6), and the request-time pass with a
  wait budget gives the same result for the design size.
- **A pass lease across nodes** (the UID lease's form). Rejected as
  unnecessary: the manifest assertion decides, duplicate work on two
  nodes is identical and rare, and a lease needs a sweep.
- **A sealed manifest.** Rejected: destroy must find the segments
  without a key, and the manifest's contents are visible from the blob
  store anyway.
- **Segments as data-store values** instead of blobs. Rejected:
  FoundationDB's 100,000-byte value limit would force hundreds of small
  values per account and chunked classes, and the blob store is where the
  mail already is.
- **Indexing in the keyed writer** (APPEND, `Email/set`, `Email/import`
  indexing their own message in the same request before the commit).
  Rejected for one code path: the pass after the commit indexes the new
  message from the log like any other, and a lost commit then leaves no
  index entry to retract.
- **Partial answers during a rebuild.** Rejected: an IMAP client cannot
  tell; the unavailable refusal is honest, and the rebuild only happens
  on loss.
- **One monolithic segment, loaded whole or not at all.** Rejected: the
  thread map is needed by every keyed mail read and is a hundredth of the
  index; splitting the sections keeps mail readable under the account cap
  and the budget.

## 13. Decisions and calls made

**Decision 1 (Jay, 2026-11-05): keyless sessions and the WebSocket
stay keyless.** Mail spec 3.3 promises a key account's keyless tokens
the methods that need no content, and the web client state-driven sync
over the WebSocket, which the HTTP layer strips of keys. With mapped ids
(6.2) a keyless session would see different ids from a keyed one. Two
answers were tenable:

- (a) **Taken.** The methods of 6.2 are gated on keys for a key
  account; a keyless token (Bearer, OAuth, WebSocket, EventSource) gets
  the keyless refusal on them. The web client does its method calls over
  HTTP, where the key cache serves every request, and uses the WebSocket
  and EventSource for push only (`StateChange` is id-free). Bearer and
  OAuth sessions of a key account, which cannot read mail today, lose
  the keyless list and sync calls. `Email/set` flag and mailbox updates
  stay keyless with the prefix unchecked. This amends mail spec 3.3 and
  what release 6 may assume, and adds no key-holding session type: keys
  live on HTTP requests under the key cache's limits, and the server
  never holds a key tied to a long-lived browser connection.
- (b) Not taken, kept for the record: a WebSocket session authenticated
  with a credential that yields keys (a password or an app password)
  holds `SessionKeys` for its life, as IMAP does, and is then a keyed
  session for every method. It would have amended release 1 invariant 5
  and mail spec 3.3's "WebSocket, EventSource, ManageSieve and SMTP
  sessions still never do", and brought the IMAP-style limit of key
  residency after a credential change (mail spec R48) to a browser tab
  left open, for a convenience the web client does not need. "For now":
  a later release may revisit it with socket invalidation plumbing.

**Calls made in this document**, each reversible at plan time:

1. Mapped Email ids (6.2) rather than singleton ids with a mutable
   `threadId` (section 12).
2. The pass runs on every keyed mail request, capped by count and bytes;
   content requests wait up to `ZA_SEARCH_WAIT_SECS`; no partial
   answers; thread-valued reads use the map as loaded.
3. Attachment text is indexed under the server's existing extraction
   limits, as for ordinary accounts.
4. The manifest is field 152 in the clear; segments are blobs linked to
   principal document 0, in two sections.
5. The defaults of 4.4, pending the benchmark.
6. No lease; the manifest assertion decides between nodes.
7. The unavailable refusal is a second fixed text, distinct from the
   keyless one.
8. The 1.0.3 merge is its own branch and pull request before this
   release's plan (the precondition above).

## 14. Release pointers

Supersedes, in the mail spec: section 1.1's "until release 7" clause and
1.2's "search of sealed content: release 7"; section 5.4's last sentence;
section 8 as a whole (its refusals become 7.4's fallback); section 14's
"keyed thread index" note; section 15's release 7 line; and section
3.3's keyless-route contract (section 13, decision 1). Section 5.5's
"arrival time and size" was already superseded by upgrade ruling 64 (ids
only). The plan 13 outcome's open question 2 (`emailQuerySortOptions`)
closes: the sorts are served. The `Threading` key's harmless clear at
unindex (plan 13, item 8) is unchanged.

Issue #23 is this release. No new follow-up issue is opened by the
design; the plan's outcome note records what it defers.
