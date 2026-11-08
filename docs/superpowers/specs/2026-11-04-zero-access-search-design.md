# Zero-access mail: search and threads (release 7) design spec

Date: 2026-11-04. Status: revision 19 (2026-11-07), approved.
Revision 19 is wording only, two corrections plan 16 asked for and Jay
ruled on 2026-11-07 (its calls B1 and E1). The record and alias maxima
are the encoded sizes, `R = 304` and `a = 15` bytes, not the earlier
estimates of 296 and about 24: the subject bucket is upstream's 128-bit
hash and an alias three u32s with its framing; the inequality, the
reservation and the 16 MiB class at 30,000 messages all still hold
(4.2). Thread compaction keeps the newest revision-table row below the
bound as a floor row, so `R(X)` is defined at every servable state
after the older rows are pruned; the row count is public already (5.5).
Revision 18 was approved by Jay on 2026-11-07 after Codex approved it
under the stopping rule.
Revision 18 answers Codex R35, the one P1 construction finding of the
pass scoped to revision 17's compaction change: the alias bound's proof
was wrong. A catch-up pass that merges threads whose roots were deleted
before it ran publishes the aliases at a change id after the deletions,
so at the retention bound the roots are pruned while the aliases
survive, and 128 such bridges put 80,896 bytes in a 64 KiB class.
Revision 18 adds the second pruning rule Codex's correction asked for:
at thread compaction an alias is dropped when no retained record is
assigned to its source thread, which gives the bound by an injection
(alias sources are unique, so each kept alias names a distinct retained
record) and keeps every retained record's resolution (the bridge rule
of revision 17 retains, assigned to the target, the bridge of every
kept alias a chain uses), and it states that folding a sub-bound alias
moves an assignment one hop, not to the end of the chain (5.5, 13,
R35). The allowance and the cost of (c) are unchanged. A second scoped
Codex pass is requested on 5.5.
Revision 17 recorded decision 2's option (c) as taken by Jay on
2026-11-07, after a walk-through of the decision: a compacted thread
blob's hidden section pads to `N_records · (R + a)` rounded to a class,
a function of public counts, so no write's size depends on what the
index found and the time cap is decision 2's only residual (2, 4.2,
5.5, 13, invariant 48). The bound's proof needed one pruning rule, that
a deleted message's record stays while the publication that assigned
it is retained, which closes the one case (H2's race) where an alias
could outlive the record that bridged it (5.5). This change, and that
rule, are the scope of the Codex pass requested on this revision.
Revision 16 answered the tenth Codex round (R31 to R34 new; R27 and R30
as wording) and the fourth Fable review (I1 to I7, saved in the Fable
review file). Both approved revision 15 under the stopping rule agreed
for this round, no P1 in the construction, and every finding is folded
in. Two rules change: an `Initial` stream and its continuations are
every document present at the pinned end, in document-id order, so no
retained record decides a page and a truncation between pages cannot
skip a survivor (6.3, R31); and content merging while a cursor is
pending is by tier, so each byte is rewritten once per tier instead of
the previous merge's result being re-selected every time (5.5, R32).
The rest is wording within the construction: the manifest is serialized
at commit with its change id in hand, so the keyed tag covers the final
bytes, and the search key travels in the batch object for it (3.1, 4.2,
5.1, R33); an aborted pass checkpoints its attempt count (5.1, R34);
the epoch rule counts records of the method's kind (5.4, 6.3, R27); two
absolute progress claims are qualified (2, 4.2, R30); a reconciliation
applies the gap's deletions before the retained records (5.4, I1); the
compaction-class residual is restated as sharply as the review put it
and a public-count pad is offered under decision 2 for Jay (2, 13, I2);
existence at the start of a range takes the log outside the range
before the record, as the end already did (6.3, I3); the post-commit
swap has a version precondition (5.1, I4); a Thread method reads its
state after the manifest (6.2, I5); a store fault's remedy is named
(5.1, I6); and the clarity items (I7) where they fall.
Revision 15 answered the ninth Codex round (R27 and R28 remaining, R29
and R30 new) and the third Fable review (H1 to H23, saved in the Fable
review file), and adds no mechanism: the epoch interval is closed,
`(S, epoch]`, so the boundary's own entries fence the latest pre-loss
state (5.4, 6.3, R27, H1); a partial record is resumed inclusively and
the checkpoint says whether it is complete (5.1, R29); `Initial` is
`Exact(0)` with an always-admissible continuation (6.3, R28); the time
cap's processing-cost channel is recorded as a residual and invariants
48 and 50 narrowed to the byte and count charges (2, 10, R30, H4);
existence at a range's end comes from the message cache or the log
outside the range before the record (6.3, H2); a reconciliation inserts
first and tombstones last (5.4, H3); error classes gain the codec's
missing key, a retry window for an absent blob and a bounded attempt
count per document (5.1, H5, H6); the rebuild cost model is stated and
the caps made consistent (4.4, 5.5, 9.6, H7); a thread-part load is
never refused for budget (4.3, H8); the manifest carries a keyed tag
(4.2, H9); the merge survivor rule is upstream's (6.1, H10); a missing
manifest is loss on any key account and a lost content blob a
content-only rebuild (5.4, H11, H23); and the clarity items (H12 to
H22) where they fall.
Revision 14 answered the eighth Codex round (R20 and R21 remaining, R26
to R28 new) by making the pass chronological, upstream's own order: a
rebuild cursor runs oldest first and no post-epoch record is applied
until it is exhausted, so each of an item's hashes names one present
message in one thread and an item retires at most `K − 1` threads, a
proof that replaces the retention rule for pending-cursor tombstones
(5.1, 5.4, 6.1, R20); the checkpoint may name a position inside a
record, so the overshoot is one message whatever a MULTIAPPEND group
holds (5.1, R20); every admitted item is charged one fixed reservation,
so where a pass stops is a function of public counts (2, 4.2, 5.1,
R21); a consumed insert or deletion that staged nothing checkpoints
(5.1, R26); the epoch refuses a state only when the method's log holds
a record between the state and the epoch, never `Initial` (5.4, 6.3,
R27); and `Initial` rendered as `Exact(m − 1)` with kinds preserved
(6.3, R28; revision 15 makes it `Exact(0)`). Revision 13 answered the second Fable review, of revision 12 (G1 to
G22, saved in `2026-11-04-zero-access-search-design-fable-review.md`),
and the seventh Codex round (R20 to R25, with R16 and R19 remaining).
Their P1s met at one point: a merge rewrote every member of the losing
thread, so a merge's size in bytes was the losing thread's size, and
tombstones and a soft insert cap put bytes the operator could size
into the padded payload (R20, G1); an operator who knows two
Message-IDs could fill both threads with helpers and read off a bridge's
class whether they were one thread (R21). Revision 13 therefore records
a merge as one **alias** entry instead of rewriting members (4.1, 6.1,
6.3), moves tombstones to an unpadded public section, bounds the hidden
section by an enforced byte budget with the overshoot of one grouped
record derived, and chooses the minimum class above that total, so a
publishing pass's thread blob is always in the smallest class and the
per-pass size channel carries nothing (2, 4.2, 13). Also: content
records compressed one by one, so the content blob is not a
compression oracle (4.2, G5); the validated snapshot compares the Email
group's counter, setup writes epoch 0, and a no-message rebuild still
allocates a boundary (5.4, 6.2, G3); retention bounds are the shorter
of the Email and Thread logs (5.5, 6.3, G4); a reconciliation attributes
by the retained log before stamping (5.4, R24, G2); thread parts have
their own memory budget, loads are single-flight, and the account cap
fits the load's transient (4.3, 4.4, G6); cursor progress is durable
even when every item was skipped, and update-only runs checkpoint
(5.1, R22, G19); `content_to` and a suspended flag, and a content-only
rebuild that skips on content presence (4.2, 5.4, R23); membership as
of a pinned end is defined from the records (6.3, R25); pending-cursor
evidence survives compaction (5.1, 5.5, R16); the destroying marker
carries the inventory and a manifest-creating batch asserts the vault
record (5.7, R19, G12); mailbox entries are property changes (6.3, G7);
a per-pass time cap (5.1, G18); `State::Initial` rendered (6.3, G13);
"current" defined (7.4, G14); the remaining clarity items where they
fall (G8 to G22). Revision 12 recorded Jay's re-confirmation of
decision 2 on its then premise; revision 13 restates the premise again
(section 13).
Revision 11 answered the fresh Fable review of revision 10 (F1 to F24)
and the Codex re-review of revisions 9 and 10 (R16 for the rebuild
cursor, R18, R19). The one P1, F1: the thread-blob size class was an
operator-driven oracle, since a sender chooses a record's size; records
are now bounded (at most `K` reference hashes) and the minimum class is
chosen so that a full pass of maximal inserts stays well under it, which
makes the number of reassignments a merge needs to show a stated lower
bound (2, 4.2, 13). The P2s: the validated snapshot also requires the
cache's state to be at or past the newest revision row, and the pass
commits through `commit_batch` (6.2, F2); error classes separate loss
from a transient failure, which never skips a message for good or
rebuilds (5.1, 5.4, 8, F3); a pass with nothing to stage writes nothing
(5.1, F4); the pass lock and the index's read lock are distinct, and no
read waits on a pass's I/O (5.1, F5); the manifest carries the chain's
length and size, and content indexing stops above the cap (4.2, 4.3,
F6); a newer manifest format is "no map", not loss (5.4, F7); 6.2
carries the method table (F8); a reconciliation stamps its records with
the log's oldest retained change id (5.4, F9); the overlay reads records
unfolded (6.3, F10); a tombstone keeps its threading fields while a
cursor is pending, as a candidate for cursor items only (5.1, R16); the
loaded index is versioned by the hash of the manifest it loaded, with an
incremental reload (4.3, 5.1, R18, F20); destroy writes a marker into
the manifest before its inventory (5.7, R19). The P3s are folded in
where they fall (F11 to F24; the thread part's size is megabytes, not
kilobytes, 1.3, F21). Revision 10 recorded decision 2 as taken;
revision 9 consolidated the pass into one model (5.1, 5.8); revisions 4
to 8 answered the first four Codex rounds (R1 to R17); revision 3
recorded decision 1; revision 2 answered the builder check of
2026-11-04. Scope: release 7 of the zero-access fork
(jaylaney/stalwart), item 5 of the release 1 spec's section 1.2 and
GitHub issue #23 ("Search for key accounts"). Release 6 (JMAP calendars
and the web client) and this release do not depend on each other; the
number is the one the mail spec reserved.

**Precondition, met.** The design is written against upstream v1.0.3's
behaviour. The 1.0.3 merge landed on `main` on 2026-11-06 (PR #82,
`f148b2973`, fork `0.4.0-beta.1` on upstream `v1.0.3`), and the design
branch carries it. Two behaviours the design relies on arrived in 1.0.2
and were verified in v1.0.3's source, not only its changelog: `Email/get`
looks a supplied id up with `cache.email_by_jmap_id(id)`, so a prefix
that is not the message's thread id is `notFound`
(`crates/jmap/src/email/get.rs:233-276`), and `Email/queryChanges` with
`collapseThreads` re-emits every message of a thread that a created,
updated or destroyed id touched (`crates/jmap/src/changes/query.rs:105-137`).
Every citation below was re-verified on the merged tree on 2026-11-06,
twice (the fork's own sweep and the Fable review), and the line numbers
are the merged tree's.

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
  keys, persisted as sealed blobs, held in memory while the account is
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
  full text). The index of this release is backend-independent: blobs in
  the blob store and one manifest value in the data store.
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
  messages. The in-memory content part of such an account is 20 to 50 MB
  with positions, a few hundred milliseconds to decrypt and load; its
  thread part is a few megabytes (a record holds ids, a thread id, the
  subject hash and up to `K` 16-byte reference hashes, 100 to 350 bytes
  largely incompressible, plus history: 3 to 10 MB for 30,000 messages,
  F21), tens of milliseconds to load; and a pass indexes a message in a
  few milliseconds (more with attachment text). These are estimates; the
  plan's first task is the benchmark of 9.6, whose figures decide the
  blob format's details and confirm or adjust the limits of 4.4.

## 2. Threat model delta

**Guarantee.** Mail spec 2 extends to the index. What the index holds
(terms, positions, sort keys, threading fields, thread ids and the
thread history) is unreadable from the stored data without the person's
credentials or a key resident in a server's memory. It is built, read
and written only by a request that holds the account's own session keys;
no background task, delivery, or keyless path reads or writes it
(release 1 invariant 6 is unchanged). Thread membership, which messages
belong together, never leaves process memory unsealed: not in a stored
field, not in the change log, not in the pattern of the pass's writes,
which is a function of the keyless log alone (6.3) up to the size class
of the thread blob a pass writes, whose residual is bounded in bytes
below (4.2; decision 2 in section 13).

**What the operator learns, beyond mail spec 2's visible list:**

- The manifest (section 4.2), in the clear: the index format version,
  its revision and the change id of the latest publication, the change
  id the index has reached, the identity epoch, the thread blobs with
  their sizes, the head of the content chain with the chain's length and
  total sealed size, and the rebuild cursor while a rebuild runs. From
  it and the blob store's timestamps: when the account's sessions
  indexed, and how much. Both correlate with arrivals the operator
  already sees.
- The sealed size of each content blob: the sum, over the messages it
  indexes, of each message's term set compressed on its own (4.2, G5).
  The messages' exact sizes are visible already (mail spec 2); how well
  each one's vocabulary compresses is new, a coarse property of its own
  text. Nothing is compressed across messages, so a message the
  operator delivers beside a target cannot shrink the blob by what the
  two share, and the size says nothing about which words any message
  holds.
- That a session made a content request, and when: the first content
  request of a session reads the content chain from the blob store, a
  content compaction reads and writes it, and the `ZA_SEARCH_*` traces
  (section 8) carry ids, counts and reason classes (F11). A pass reads
  the message blobs it indexes, which the log already names.
- The size of each thread blob (4.2): its public section, which is a
  known multiple of the pass's tombstone count, a count the delete
  records already show; and the class of its hidden section, which for
  a publishing pass is always the smallest class `M`, and for a
  compacted blob the class of `N_records · (R + a)`, a function of the
  retained record count (5.5, decision 2 (c)). The hidden part
  holds one record per message the pass indexed and one alias per
  thread a merge retired, nothing per member of a merged thread (R21);
  the pass stops staging at a byte budget `H` with an overshoot of at
  most one grouped record; and `M` is chosen above that total, an
  inequality the configuration enforces (4.2, R20). So an operator who
  delivers the mail chooses what a pass indexes but cannot push the
  hidden section past `M`, whatever the pass found, whether a message
  threaded with existing mail, merged two threads, or merged two
  threads the operator had filled with helpers to tell whether two
  Message-IDs they know share a thread (R21, G1: before the bound, a
  sender-chosen record size, an unbounded tombstone count or a merge
  that rewrote every member made the class an oracle on exactly these).
  Where a pass stops, which `indexed_to` and the cursor bound publish,
  depends on how many items it admitted, since every item is charged
  the same reservation whatever its aliases came to (R21: a charge of
  actual alias bytes would have moved the stopping point by one record
  according to whether a probe's two references already shared a
  thread), and on the time cap, which is the processing-cost channel
  the limits below record (R30, H4). The per-pass size channel
  therefore carries nothing, and the checkpoint carries only processing
  cost. The class of a **compacted** thread blob (5.5) carried one more
  channel until revision 16, and it was sharper than the "coarse count"
  earlier revisions called it (I2): the compacted hidden section is
  every retained record and alias, and of these only the alias count is
  hidden, since the tombstone and row counts are public, publications
  are public (the clear revision counter, the log entries), and a
  record's size is a function of its ids, its subject hash and
  `min(K, distinct reference hashes) · 16` bytes, which the operator
  who delivered the message read in the clear at SMTP time (mail spec
  12: plaintext in the SMTP queue); only a message the owner appended
  or imported under keys has a size the operator does not know. An
  operator who padded the retained total to one alias below a class
  boundary with filler messages of chosen sizes would have learned, at
  the next compaction, whether a chosen bridge merged two threads: one
  bit per compaction about a chosen message, for megabytes of filler
  mail at megabyte classes, and only where the owner's own appended
  mail added no sizes the operator could not see. Revision 17 closes it
  (decision 2 (c)): a compacted hidden section pads to
  `N_records · (R + a)`, where `N_records` counts the retained records,
  present and tombstoned, both public, and the retained aliases never
  exceed the retained records (5.5), so the class is a function of
  counts the operator already has. No write's size therefore depends on
  what the index found; the time cap's channel below is decision 2's
  one residual.
- Per publishing pass, which is every pass that indexed at least one
  message (6.3): one Email item update of a witness, the lowest document
  id among the messages the pass indexed, a choice the operator can make
  from the log themselves; one Thread property change of that same id;
  and a container update of every mailbox of the account. So: that a
  pass indexed something, which the arrivals already said, and nothing
  that depends on what the pass found in the mail. A pass that indexed
  nothing writes no entry at all. The rebuild's boundary entry (5.4) is
  the same shape, with the lowest message present as its witness.

**Nothing else.** No term, stem, position, header name, Message-ID,
subject normalisation, sort key or thread membership leaves process
memory except sealed. Search queries are client-supplied strings and are
never traced (mail spec invariant 30).

**Limits, stated alongside the guarantee.**

- The running server holds the index in plaintext for every account in
  use, as it holds the plaintext of the messages it serves. The index is
  not key material: it is dropped, not zeroized, on eviction, except the
  decrypted blob buffers while they are deserialized (section 4.3).
- Integrity, as release 1 and mail spec 2, with a divergent outcome
  stated plainly (F12): a store writer can delete the manifest or a
  blob, which forces a rebuild from the mail and a resync of every
  client (5.4). A writer who rolls the manifest back to an older valid
  one gets one of two outcomes, neither signalled as an attack: if a
  compaction since then unlinked the blobs the old manifest names, the
  next load finds them gone and rebuilds (an epoch move, every client
  resynced); if the blobs still exist, the next pass reopens a revision
  number already published under another change id, and clients that
  synced the first publication are never told of what the second
  renders differently. A blob cannot be forged or moved to another
  account or position (its AEAD binds the account id, the segment id,
  its kind and, for a content blob, its predecessor's hash and segment
  id). A writer who can do this can drop messages themselves, which the
  index never notices: the index describes the mail that is present.
- Timing: a pass runs inside the first keyed requests after mail arrives,
  so the moment of indexing is visible in the blob store and the
  manifest. It says the account was in use, which the operator's
  connection logs already say. A pass's **duration** is visible too,
  from the hold and commit timestamps and the request's response time,
  and where the time cap `ZA_SEARCH_PASS_MS` stops a pass is published
  by the checkpoint (R30, H4): both are the processing cost of the
  messages the pass admitted, tokenization and attachment-text
  extraction, which equal sealed sizes do not fix (a text-heavy PDF and
  an encrypted archive of one size cost differently). An operator who
  delivers the mail can calibrate with their own messages and read a
  target's processing cost off a pass's duration or its stopping point,
  a coarse property of the message's content. This is the one channel
  of a pass's writes that the byte and count charges do not close, and
  decision 2 is stated with it.
- Freshness is tied to presence. Mail that arrives while nobody holds the
  account's keys is indexed by the first keyed requests afterwards, so
  for that interval it is not searchable from any client and threads as
  a singleton (section 5.2). A message that arrives and is deleted
  before any keyed request indexes it is never read, so a later message
  that referenced only it threads without that evidence (5.8). A
  client's own local index is unaffected.
- IMAP `CHANGEDSINCE` does not surface a changed `EMAILID` or `THREADID`
  of a key account's message (7.5): bumping each reassigned message's
  modseq would write each member, and that cohort is what the guarantee
  above withholds.
- Threading keeps at most `K` reference hashes per message (4.2), where
  upstream keeps every one; a message whose `K` nearest referenced
  ancestors are all absent while an older one is present threads as a
  singleton where upstream would have joined it (7.5).

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
- It leaves `SessionKeys` for one place (R33): the manifest object a
  publishing or checkpointing batch holds carries a copy in a `Secret`,
  because the manifest's keyed tag (4.2) is computed inside the commit,
  in `serialize_with_ids`, once the batch's change id is assigned; the
  copy is zeroized when the batch is dropped, the serialization is
  deterministic and reruns on a FoundationDB retry, and nothing of it
  is logged or stored (invariant 48).

The index's blobs and nothing else are sealed under it: `seal(swk, aad,
bytes)` (the helpers of `keys.rs:181-235`, one random nonce and one tag
per blob) with `aad("search", account_id) || header`, where `header` is
the blob's clear header of 4.2 (its kind, its own segment id and, for a
content blob, its predecessor's hash and segment id, or zeros). So a
blob opens only for its account, only as the kind the loader expects,
only under its own identity, and only in the position the manifest or
the chain gives it. The manifest is in the clear (section 4.2).

No per-blob data key and no envelope: the search key is never published
or wrapped to another principal, so there is nothing for an envelope to
carry. Each blob begins in the clear with a format byte
(`SEARCH_FORMAT_V1 = 1`, the mail blob's convention) and the header; the
loader checks the format version in the manifest before it opens
anything, and the AEAD covers the header through the AAD.

## 4. The index

### 4.1 What it holds

Per account, the index is the union of its blobs (4.2). Logically it
holds two kinds of record:

**The thread record** (small, always loaded, 4.3), for every message
that is present and for every deleted message whose deletion the change
log still retains (R3):

- **Threading fields.** `ThreadFields::scan` of the opened message
  (`crates/email/src/message/thread.rs:30-64`): the `xxh3_128` hashes of
  Message-ID, In-Reply-To, References and Resent-Message-ID, kept to at
  most `K` (4.2: the message's own id and its `K − 1` nearest
  references), and the subject's thread name hash. Dropped once the
  message is tombstoned (5.1).
- **The assignment**: the thread id the index gave the message when it
  indexed it (section 6.1) and the revision (4.2) of the pass that did,
  `(T0, r0)`. Before `r0` every client saw the singleton (the message's
  own document id); from `r0` on, its thread is `T0` carried through the
  alias table below. A message's record is written once and never
  rewritten by a merge (R21): what a merge changes is recorded as an
  alias, not as a change to each member.
- Its **creation change id**: the change id of the log record that
  inserted it; the epoch for a message the rebuild's cursor indexed (it
  existed at the epoch, and no state before the epoch is served); the
  log's oldest retained change id minus one for a message a
  reconciliation indexed (it existed at every state the log can serve,
  F9). 6.3 decides existence at a state from it (R14).
- For a deleted message: its **deletion change id**, the change id of
  the log record that deleted it, which the pass reads when it tombstones
  (5.1); the log's oldest retained change id minus one for a
  reconciliation's tombstone (F9).

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
  stub drops (mail spec 5.4; `ingest.rs:390-391`).
- **The attachment bit** (`za_has_attachment` on the opened row,
  `crates/email/src/message/za/view.rs:411`), from which the
  `$hasattachment` and `$hasnoattachment` keywords are answered.

And, per index: the **alias table** (R21): one entry per thread a merge
retired, `(losing thread L → surviving thread W, revision r)`, written
by the pass whose insert caused the merge (6.1), immutable, chained when
a survivor later loses; a message's thread at any state is its
assignment carried through the aliases of that state (6.3), so the
table is the whole history of every merge, a few dozen bytes each
whatever the threads' sizes; the **revision table** (revision → the
change id of the batch that published it, 4.2), carried sealed in the
thread blobs except its newest row, which the manifest carries in the
clear; the **tombstones** of messages deleted since the blob that holds
their content was written; and the **thread table** (thread id → its
present members), derived from the records and the aliases at load and
not stored.

The in-memory representation is the builder's. The content part must
answer, for a query, each `SearchFilter` leaf upstream's planner
produces (`Text` with `Standard`, `Exact` and `Prefix`, `KeyValue` on the
headers; see `crates/store/src/search/leaf.rs:51-115` and
`document.rs:256-281` for what each means) with a document-id bitmap,
including a phrase check with the positions upstream keeps
(`term.rs:656-729`), and give per document the sort keys, `sent_at` and
the attachment bit. The thread part must give, per document, its
assignment and its lifecycle change ids, per thread its present
members, and the alias chain of any thread, and must answer "the thread
of D as of state S" and "the members of T as of state S" by the rules
of 6.3. `Leaf`, `term`, `codec`
and `AccountIndexer` are `pub(crate)` in `store`, so the index model
lives in `crates/store` or re-exposes what it reuses; the builder
chooses.

### 4.2 Blobs and the manifest

**Blobs.** A pass writes up to two blobs (R9: each authenticated on its
own, so a loader reads one without the other):

- a **thread blob**, in two sections sealed separately (R20, G1): the
  **hidden section**, the records of the messages the pass indexed and
  the aliases of the merges it made, padded to a class as below; and
  the **public section**, its tombstones (document id and deletion
  change id, facts the delete records already name) and the revision
  table rows it carries (the manifest's previous newest row joins the
  pass's thread blob, sealed, so the manifest keeps one row),
  fixed-width and unpadded. The blob's length is the hidden section's
  class plus a known multiple of the tombstone count, so the operator,
  who sees that count in the log, learns the class and nothing more;
- a **content blob**: the content records and postings of the messages
  the pass inserted, when it inserted any and the account is under the
  cap.

Nothing is compressed across records (G5): a content record is one
zstd frame of its own, with the fork's static dictionary and no window
over its neighbours, so a blob's size is the sum of per-message sizes,
each a function of that message's own text, and a chosen message
delivered beside a target cannot shrink the blob by what they share
(the CRIME class of attack); thread records and aliases are mostly
hashes and are stored uncompressed. Each section is sealed under the
search key with the AAD of 3.1, the section kind in it. The
serialization format is the fork's own, versioned by
`SEARCH_FORMAT_V1`; it is not an upstream rkyv archive read with
`access_unchecked`, so the "stored structs never change layout" rule of
release 1 does not bind it: a format bump is a version bump in the
manifest and, on the newer binary, a rebuild (5.4). Upstream's
forward-index codec (`Document::encode`, `term.rs:629-647`) may be
reused for positions if the builder re-exposes it; the builder chooses.

**Bounded records and the padding** (F1, R15, R20, R21, G1). A thread
record keeps at most `K` reference hashes (`K = 16`, a constant of
format version 1, not a knob, H22: two nodes with different `K` would
write records of different maxima into one index and check the
inequality against different `R`, so a change of `K` is a format bump
and a rebuild; upstream keeps every one, so 7.5 records the
deviation): the message's
own Message-ID hash, then its In-Reply-To hash (the parent), then its
References hashes from the last (the nearest ancestor) backwards, then
Resent-Message-ID, taken in that order before `ThreadFields::scan`'s
sort and dedup (`crates/email/src/message/thread.rs:33, 61-62`) by a
fork wrapper that captures header order (G10). A record is written once
and holds no history (4.1), so its size has a derived maximum `R`: the
ids, the change ids, the 128-bit subject hash and the entry's framing,
48 bytes encoded, plus `16 · K` bytes of hashes, `R = 304` bytes at
`K = 16`; an alias is `a = 15` bytes encoded (its source, target and
revision, three u32s, and the framing). The hidden
section of a pass's thread blob is bounded by construction (R20, R21):
an item's record is at most `R` and, under 5.1's chronological order,
its aliases at most `K − 1`, so one admitted item contributes at most
one **reservation** `R + (K − 1) · a` bytes; the pass charges every
admitted item exactly that reservation, whatever it comes to, and stops
at the next insert boundary once the reservations reach
`H = ZA_SEARCH_PASS_HIDDEN_KB` (5.1), so the overshoot is one
reservation and the hidden section is never more than
`H + R + (K − 1) · a` bytes, 32 KiB + 304 + 225 at the defaults. The
byte and count cutoff therefore depends only on how many items the pass
admitted, a public count, never on what their records or aliases
actually came to (R21), and no record of any writer, a MULTIAPPEND
group of any size included, enters the bound (R20); the time cap is the
one other cutoff, and it is the processing-cost residual of section 2
(R30). The hidden section is padded before sealing, in the
stub's framing (`seal_padded`'s form: a u32 length prefix, zero padding,
then the seal; `crates/groupware/src/calendar/seal/tree.rs:272-282`,
whose own 256-byte rounding is not this policy): with `p` its length
and `p + 4` the framed length, the padded length `B` is
`max(M, 2^ceil(log2(p + 4)))` with `M = ZA_SEARCH_PAD_MIN`, 64 KiB at
4.4's defaults, chosen so that `H + R + (K − 1) · a + 4 <= M`, an
inequality the configuration refuses to violate (9.1). A publishing
pass's hidden section therefore always pads to `M`: its sealed size is
the same whatever the pass found, and the per-pass size channel carries
nothing. The public section is unpadded, `t` bytes per tombstone plus
the revision row. A compacted thread blob (5.5) pads its hidden section
not by its length but to `N_records · (R + a)` framed, then rounded by
the same class formula, where `N_records` is the count of retained
records, present and tombstoned, a public count (decision 2 (c), I2):
the content always fits, since every record is at most `R` and the
retained aliases never exceed the retained records (5.5), so the class
is a function of public counts and says nothing of how many merges the
window held; `R` and `a` are the encoded maxima, per-entry framing
included, and the allowance carries the section's framing before the
rounding (R35). A
content blob is not padded, since its size is the sum of per-message
sizes, which follow the messages' sizes, visible already.

**Blob headers** (R11). Every blob carries, in the clear and bound by
its AAD: the format byte, the kind byte (thread or content), its own
16-byte random segment id, and, for a content blob, its predecessor's
blob hash and segment id (zeros for the first). A loader therefore has
everything it needs to authenticate a blob from the blob itself and the
descriptor that led it there, with nothing retained in process memory:
the manifest names each thread blob and the content head by hash and
segment id, and each content blob names its predecessor the same way.

**Content blobs chain** (R8, F6): each content blob's header names the
previous content blob, or none; the manifest names only the head, and
carries the chain's length and total sealed size, updated in the same
batch as the head. A content load walks the chain from the head, which
costs nothing beyond the reads a load makes anyway; a destroy walks it
to unlink (5.7); a content compaction (5.5) collapses it into one blob.
The manifest's size is therefore independent of how many passes have
run: it lists the thread blobs, whose count any node bounds by thread
compaction, and one head. Above the account cap (4.3) the chain is not
extended: content indexing is suspended with a trace while the thread
part is maintained, so the chain's length is bounded by
`ZA_SEARCH_SEGMENTS` plus the passes between two compactions.

**Staging and linking** (R7, F17): a pass stages each blob with
`hold_blob` (`crates/common/src/storage/blob.rs:308-360`), a temporary
link that outlives the pass's remaining wait budget, and in the
manifest's batch (5.1 step 3) makes it permanent as upstream does,
through `BlobHold::release(batch)` (`storage/blob.rs:108-122`: it clears
the temporary link and writes the blob's `Commit` key, without which
`blob_exists`, `crates/store/src/write/blob.rs:100-110`, stays false and
later holds re-upload) plus `BlobOp::Link{hash, to: BlobLink::Document}`
with `hash | account_id | Collection::Principal | 0`
(`crates/store/src/write/key.rs:410-421`), the principal document that
already carries the vault and pin records; and it clears that link for
each blob the batch retires. A batch that fails or loses its assertion
leaves only holds, which expire, and the purge then deletes the blobs
(`crates/store/src/write/blob.rs:159-287`): no permanent link ever
exists that the committed manifest does not name, or that a chain
reachable from it does not name, except through the chain-break case
5.4 and 5.7 handle by scan. An owner can download a blob raw as upstream
lets an owner download any linked blob of their own account; it is
ciphertext under their own key, so this is harmless and nothing is
added to refuse it. No consumer but the loader parses one.

**The blob codec** (1.0.3). Every blob now passes through
`crates/store/src/write/blob_codec/`: `hold_blob` and `put_blob` take a
`BlobOpts` (compression, and encryption under the server's registry
`SymmetricKey` when one is configured), and `get_blob` returns the
decoded bytes. The index's blobs are written with the options sealed
mail uses (`BlobOpts::new(CompressionAlgo::None, core.blob.email.encryption)`,
as `za_stage_sealed` does, `crates/email/src/message/za/ingest.rs:171-181`):
no codec compression, since sealed bytes do not compress and the pass
compresses before sealing, and the operator's at-rest encryption on top
of the seal, as for mail, so an operator who audits "every blob under my
key" finds it so. The `BlobRecode` task, which rewrites blobs when the
operator changes compression or rotates the codec key, walks
`user_blob_hashes` (`crates/store/src/write/blob.rs:291-355`), which
lists the `Document` links of Email, SieveScript and FileNode only
(`:333-338`); the fork extends that walk to the `Document` links of
`Collection::Principal` document 0, so a codec key rotation recodes the
index's blobs instead of stranding them under a retired key.
`BlobRecode` never opens the seal (upgrade:
`index_task::test_blob_recode_keeps_sealed`).

**The manifest** is a new principal field, `PrincipalField::ZeroAccessSearch
= 152` (`crates/types/src/field.rs`, beside 150 and 151), on principal
document 0, in the clear, holding:

- the format version;
- `revision`: a counter incremented by every publishing write and never
  reset (a rebuild continues it), and the newest row of the revision
  table, `(revision, change_id)`, whose change id the batch fills in at
  commit: the manifest is written as a `SetValue::Serializable`
  (`SerializeWithIds`, `crates/store/src/write/mod.rs:1148-1151,
  1192-1209`), whose `serialize_with_ids` runs at commit with the
  batch's assigned change id in hand (`AssignedIds::current_change_id`,
  `:1648`), serializes the manifest with it and computes the tag below
  over the final bytes last (R33: a byte patch applied after the tag,
  `SetValue::Patched`, would fail every reload and force a rebuild on
  every publish), so the revision, the change id clients see for it and
  the tag are bound in one commit (R2);
- `indexed_to`: the checkpoint `(c, p, complete)` of 5.1: the record the
  index has reached, the last insert done in it, and whether the record
  is complete (R29);
- a **tag**: a keyed BLAKE3 tag under the search key over every other
  field (H9), computed at commit over the serialized manifest with its
  change-id fields filled in (R33), which a loader verifies and treats as loss when it fails,
  and which destroy ignores, so a store writer who rewrites the manifest
  from authentic blobs (dropping the cursor, advancing the checkpoint,
  zeroing the epoch) forces a rebuild rather than a silently incomplete
  index; every manifest writer holds keys, so the tag is free;
- `epoch`: the identity epoch, the change id at which the current
  identity history began (5.4, 6.3), `0` at setup (nothing has been
  issued, G3) and filled in at commit, by the same serialization (R33),
  when a rebuild's boundary batch
  sets it. The pair `(epoch, revision)` is the manifest's **identity
  version**, which 6.2's validation and 6.3's epoch rule read; the
  loaded index's own version is the hash of the manifest bytes it loaded
  (4.3, R18);
- the thread blobs: for each its segment id, blob hash and sealed size;
- the content head: the hash and segment id of the newest content blob,
  or none, with the chain's length and total sealed size (F6), and
  `content_to`: the change id and cursor bound up to which every indexed
  message also has its content record, with a **suspended** flag while
  the account is over the cap (R23); a content-only rebuild (5.4) is
  what moves `content_to` forward again, over a cursor bound of its own
  in this field, so it can be pending beside the rebuild cursor below
  (I7);
- the rebuild cursor, when a rebuild is in progress (5.4): the last
  document id done and the highest document id present at the epoch,
  its end (H15); and the **reconciliation marker** when one is pending
  (G11), whose bound is the oldest retained change id of the shorter
  log (G4) as the pass that wrote the marker read it, which every later
  pass of the reconciliation uses for its attribution and its stamps
  (5.4, I7): the difference set itself is not stored but recomputed per
  pass;
- or, in place of all of the above, the **destroying marker** (5.7,
  R19), which carries the inventory of the manifest it replaced, so a
  destroy that dies after writing it retries from the marker (G12).

Every write of it asserts `AssertValue::Hash` of the previous bytes, as
the pin record does (`za_pin_transact`,
`crates/common/src/auth/vault.rs:691-764`), or `AssertValue::None` when
creating it. It stays a few kilobytes: `ZA_SEARCH_SEGMENTS` thread
entries at most (5.5) and fixed fields, far under FoundationDB's
100,000-byte chunking bound, which applies only to the classes
`may_be_chunked` lists (`crates/store/src/write/key.rs:877-897`) and
which field 152 does not join. The manifest is not sealed because the
destroy path (5.7) must find the blobs to unlink them without a key, and
because what it holds (section 2) is already visible from the blob
store.

The manifest is created in setup's vault batch (5.4) and never touched
by the vault path afterwards (F22): a manifest write carries no
credential and changes no login outcome, so it never invalidates the
account cache; the vault record stays the sole authority for login
(release 1 invariant 2 unchanged), and the pass does not touch the
generation.

### 4.3 The in-memory index and its cache

The loaded index of an account lives in a new per-process cache,
`Caches::za_search`, keyed by account id (not by token: several sessions
of one account on a node share one loaded index). Today no per-account
structure exists; every `SessionKeys` is per login and IMAP and POP3
sessions own theirs outside the key cache (`crates/vault/src/cache.rs`,
`crates/common/src/za/mail.rs:63-82`), so the index cache is its own
structure with its own rules:

- **Load** only in a request that holds the account's keys: read the
  manifest, open each blob (verify the format byte and the AEAD under
  `swk` with the header in the AAD), decompress and deserialize in
  `Zeroizing` buffers, merge, derive the thread table, apply the
  tombstones. The index records the manifest's identity version, the
  pair `(epoch, revision)`, its `indexed_to`, and the **hash of the
  manifest bytes** it loaded, which is its own version (R18): any
  committed change to the manifest, a deletion-only pass's included,
  changes that hash. A **thread-only load** reads the thread blobs only
  and is what every keyed mail request needs (6.2); the content chain is
  loaded on the first content request (7.1), or for a content compaction
  (5.5). Both are loads of the same entry.
- **Reload incrementally** (F20). Thread blobs and the content chain are
  append-only between compactions, so a node whose loaded index is behind
  the manifest loads only what the manifest added: the thread blobs it
  does not hold, and the chain from the new head back to a segment id it
  holds; a compaction, which names blobs it does not hold and drops
  ones it does, is loaded whole. Either way the loaded index ends as a
  fresh load of the current manifest would.
- **Serve** from memory: every resolution of section 7 reads the loaded
  index; no query reads a blob.
- **Never ahead of the store.** A pass builds its additions in a staging
  structure and swaps them into the loaded index, under the index's
  write lock, only after its batch has committed (5.1 step 3). A
  response therefore never maps an id with a revision the store does not
  hold; 6.2's validated snapshot ensures the converse, that it never
  maps with a revision older than the state it returns.
- **Locks** (F5, G21). The index entry has a read lock for every reader
  and the staging swap, and a separate pass lock for the pass itself. A
  thread-valued read never waits for a pass's I/O: it takes the read
  lock, which a pass holds only for the swap. A request that finds a
  pass running does not wait for it (5.1). Eviction takes the write
  lock (F18), so it never races a reader or a swap. A validated
  snapshot's reload (6.2) also takes the write lock and may run while a
  pass is staging on the same node; the pass is unharmed, because its
  batch asserts the manifest hash it read at step 0, and a reload means
  that hash moved, so the pass loses its assertion and discards.
  **Loads are single-flight per account** (G6): a second request that
  needs a load the first is making waits for it rather than paying a
  second transient.
- **Evict** after `ZA_SEARCH_IDLE_SECS` (default 900) without use, when
  the process budget `ZA_SEARCH_MEM_MB` (default 512) needs room (least
  recently used first, content parts before thread parts), and on every
  invalidation that drops the account's keys
  (`CacheInvalidation::Account` and `AccessToken`,
  `crates/common/src/cache/invalidate.rs:642, 615`, where
  `za_keys.remove_account` runs; 1.0.2's registry `PublicKey` change
  emits the `Account` one, `:236-241`, and its `default_role_changed`
  branch, `:719-723`, touches neither `za_keys` nor the index). There is
  no maximum age: a reload is only a cost, and the search key does not
  change. The budget counts a load's transient footprint too (F18): the
  sealed bytes, the decompressed `Zeroizing` buffer and the deserialized
  part coexist for a moment, two to three times the loaded size, and a
  compaction's staging likewise; a load that would exceed the budget
  with its transient evicts first or fails as "no map". **Thread parts
  have a budget of their own** (G6), `ZA_SEARCH_THREAD_MEM_MB`, apart
  from the content budget (a compacted thread blob's sealed bytes are
  about twice its records', decision 2 (c), so the transient of a
  design-size thread load is a 16 MiB read beside the unsealed part,
  well inside the default): every keyed mail request of an account needs
  its thread part, so content parts can never crowd thread parts out,
  and a node that serves many accounts at once degrades search before it
  degrades mail. A thread-part load is **never refused for budget
  alone** (H8): it evicts idle thread parts first and, when every
  resident one is in use, exceeds the soft budget with a trace
  (`ZA_SEARCH_OVER_BUDGET`), since refusing would fail every mail method
  of the account; at the defaults a node holds the thread parts of some
  tens of design-size accounts at once before it overruns, and the
  deployment sizes the budget from its account count.
- **Never swapped** (F16). 1.0's cache swap tier persists snapshots of
  the caches that implement `SwapCached`
  (`crates/common/src/cache/swap/evict.rs:20-26`); `Caches::za_search`
  never implements it, and invariant 49 says so.
- **Account cap.** An account whose content part would exceed
  `ZA_SEARCH_ACCOUNT_MAX_MB` (default 128, at most a third of the
  content budget so that a load's transient fits, G6), read from the
  manifest's chain size without a walk (4.2), loads its thread part
  only; its
  content requests get the unavailable refusal (7.4), with a trace,
  content indexing is suspended (4.2), and everything else is served.
  This is the knob for a mailbox far past the design size; raising it
  takes a content-only rebuild (5.4). The thread part is never subject
  to it.

The environment variables follow the fork's parse-and-fall-back
convention (`KeyCacheConfig::from_env`, `crates/vault/src/cache.rs:41-53`;
`lease_secs_from`, `crates/common/src/za/lease.rs:55-60`), read once and
logged at boot; registry settings are not added (CLAUDE.md).

### 4.4 Sizing and limits

The design size (1.3) gives a loaded content part of tens of megabytes
per account, a thread part of a few megabytes, and a budget of 512 MB
for a node serving ten to twenty such accounts' content at once; a
deployment with more concurrent large accounts raises
`ZA_SEARCH_MEM_MB`. Eviction is cheap (the blobs stay), and a reload
after eviction is the load cost of 4.3. The knobs:

| Variable | Default | Meaning |
|---|---|---|
| `ZA_SEARCH_MEM_MB` | 512 | process budget for loaded content parts, transients included |
| `ZA_SEARCH_THREAD_MEM_MB` | 256 | process budget for loaded thread parts, transients included (G6) |
| `ZA_SEARCH_ACCOUNT_MAX_MB` | 128 | largest content part a node loads or extends; at most a third of `ZA_SEARCH_MEM_MB`, for the load's transient (G6) |
| `ZA_SEARCH_IDLE_SECS` | 900 | eviction after disuse |
| `ZA_SEARCH_PASS_CAP` | 48 | admitted items after which a pass stops at the next insert boundary (`C`); at most `H / (R + (K − 1) · a)`, so that it binds (H7) |
| `ZA_SEARCH_PASS_MB` | 32 | opened message bytes after which a pass stops at the next insert boundary |
| `ZA_SEARCH_PASS_HIDDEN_KB` | 32 | reserved hidden thread bytes after which a pass stops at the next insert boundary (`H`, 4.2) |
| `ZA_SEARCH_PASS_MS` | 2000 | elapsed time after which a pass stops at the next insert boundary (G18; its leak is section 2's) |
| `ZA_SEARCH_WAIT_SECS` | 30 | how long a content request completes passes before refusing |
| `ZA_SEARCH_SEGMENTS` | 16 | thread-blob count, and content-chain length, that trigger compaction |
| `ZA_SEARCH_CHECKPOINT` | 1000 | update-only records after which a pass writes a manifest-only checkpoint (G19) |
| `ZA_SEARCH_SKIP_AFTER` | 8 | aborted passes on one document after which it is skipped as loss (H5) |
| `ZA_SEARCH_ABSENT_RETRY_SECS` | 10 | how long a blob a committed manifest names may be reported absent before that is loss (H6) |
| `ZA_SEARCH_PAD_MIN` | 65536 | the smallest hidden-section class in bytes (`M`) |

`K = 16` is a constant of the format, not a knob (4.2, H22). The
configuration refuses `M < H + R + (K − 1) · a + 4`,
`ZA_SEARCH_PASS_CAP > H / (R + (K − 1) · a)` and
`ZA_SEARCH_ACCOUNT_MAX_MB > ZA_SEARCH_MEM_MB / 3` at boot, with `R` and
`a` derived from `K` and the record layout, not measured (R20). At
these defaults a pass admits up to 48 items, so a design-size rebuild of
30,000 messages is about 625 passes (H7, 9.6), with content merged by
tier along the way, three tiers at that count, so no content byte is
rewritten more than three times before the full merge (5.5, R32). The plan's benchmark
(9.6) records the measured figures for the design size against these
defaults and adjusts them before release.

## 5. Building the index

### 5.1 The pass: one model

A pass brings an account's index from `indexed_to` towards the account's
current Email change id, by change-log records in their order, a pending
rebuild or reconciliation cursor (5.4) worked first. It runs inside a request that holds the account's keys, on the
request's task, under the index's pass lock (4.3): one pass per account
per node at a time. A request that finds the pass lock held does not
wait (F5): it proceeds on the loaded index, which the running pass will
advance, and only a content request waits on the pass under its budget
(5.2). Four rules define the pass; the steps below apply them, and 5.8
checks them against every failure scenario the reviews raised.

**Inputs, and what bounds a pass.** A pass reads the manifest (its
hash, its identity version, `indexed_to`, the cursor), the change-log
records after `indexed_to` in key order, and the stubs and blobs of the
messages it indexes, all from the store. The message cache enters only
the shortcut of step 0: whether a message exists is decided by the store
(its stub opens or is gone) and by the log (its deletion record), never
by a cache snapshot, so no pass can be misled by a snapshot older than
the records it reads (R17). A response, by contrast, takes one cache
snapshot and uses it for both the state it returns and the set of
messages it treats as present (6.2).

**Order.** Strictly chronological, which is upstream's own order (R16,
R20). A pending rebuild cursor is worked first, oldest document id
first, and no post-epoch log record is applied until the cursor is
exhausted: every message the cursor holds predates every record the
rebuild's log holds, since document ids are a monotonic counter (4.1)
and the cursor is the set present at the epoch. Then records are
consumed in change-id order, and within a record its inserts in
document-id order, before its deletions, as upstream's ingest ran
before the batch that carried both committed. A record may be left
part-way through at an insert boundary (R1's second option, R20): the
checkpoint then names the record and the last document id indexed in
it, and the record's deletions are applied when it completes. This
order is what makes an insert see exactly the messages that existed
when it was logged: every message deleted later is still in the index,
every message deleted earlier is a tombstone, and no message newer than
the insert has been indexed before it. That last clause is what bounds
a merge: within a subject bucket, every present message whose kept
hashes contain a given hash is in one thread, an invariant each insert
re-establishes for every hash it carries by merging every thread of
every candidate it matches, which deletions (they only remove members),
aliases (they never split a thread) and folding at compaction (it keeps
every resolution) preserve; so at the moment an item is indexed, each of
its at most `K` hashes resolves to at most one thread, duplicate
Message-IDs included (they share the root), and an item retires at most
`K − 1` threads and writes at most `K − 1` aliases, with no exception
for the cursor (R20: a newest-first
cursor let later messages be indexed before an older one they
referenced, and one old item could then merge arbitrarily many
threads). The one inversion the order admits is a pair of concurrent
batches whose change ids commit in the other order from their document
ids, a bounded, single-step disorder, not a fan-in.

**What is discarded, and when.** A message's threading fields go when it
is tombstoned: under the chronological order every insert that could
have used them has already been processed, as upstream threaded it with
the message still present, and a rebuild's cursor is exhausted before
any deletion record is applied, so no tombstone exists while a cursor
is pending (R16, both forms, by construction rather than by a retention
rule). A message's identity (its
assignment, its creation and deletion change ids) stays until its
deletion change id leaves the retained log, and an alias until its
revision's change id does, where "the retained log" is the shorter of
the Email and Thread logs (5.5, G4), for the rendering of 6.3; at that
point compaction folds the alias into the records it governs (5.5). Its
content records go at content compaction. Nothing is discarded for any
other reason.

**Checkpoints, and what each guarantees.** `indexed_to = (c, p,
complete)` means every record below `c` has been consumed whole and, of
record `c`, its inserts up to document id `p`, with `complete` set when
`c` is done (every insert processed and its deletions applied): every
such insert is indexed, or recorded as skipped because its message was
gone or would not open, with its thread assigned as of its position;
every deletion of a completed record is a tombstone; it says nothing
about what lies beyond, which the renderer's raw overlay (6.3) and the
cache intersection (6.2) cover for every read. A pass that resumes from
a partial `c` reads record `c` itself, **inclusively** (R29: the log
reader skips the from-key for an exclusive query,
`crates/store/src/query/log.rs:130-148`, so an exclusive resume would
lose the record's suffix for good), processes the inserts after `p`,
then applies `c`'s deletions, and only then reads on exclusively; a
record `c` found replaced by the truncation marker (the empty value
`truncate_change_log` writes, `crates/common/src/storage/transaction.rs:228-244`)
is loss of the unfinished record and takes the reconciliation of 5.4.
Where `indexed_to` is compared (the shortcut of step 0, "current" in
7.4), a partial `c` counts as not consumed. The cursor's bound means
every message present at the epoch with a lower document id is indexed
or skipped, up to the highest document id present at the epoch, which
the manifest stores as the cursor's end (H15). The identity version `(epoch, revision)` means every identity
transition up to that revision is published with its change id (4.2).
`content_to` means every message indexed up to it has its content
record too (R23). The manifest hash is the version of everything above
together (R18). All of them move only in the one batch of step 3; a
pass that has nothing to stage and no checkpoint to move writes
nothing (step 2).

**Error classes** (F3, H5, H6). Two kinds of failure, told apart once,
for the pass and the loader alike: **loss**, a stub that the store
reports absent, a blob the store reports absent for longer than a
short retry window, or a blob that fails its AEAD or does not parse
(the blob codec's truncation and decompression failures included); and
**transient**, any other error: a store or blob-store timeout, a
refused connection, a budget that cannot make room, a blob the store
reports absent within the retry window (H6: a blob store without
read-after-write consistency across nodes can answer "not found" for a
blob another node committed moments ago, and a committed manifest
names it, so absence is loss only when it persists past
`ZA_SEARCH_ABSENT_RETRY_SECS`), and the codec's missing-key error
(`BlobKeyring::unavailable_key`, a `StoreEvent::DecryptError`,
`crates/store/src/write/blob_codec/mod.rs:206-222`: an operator who
removed a codec key is a configuration fault to repair, not a reason to
rebuild every key account's index). In a pass, loss of a message's stub
or blob skips that message (a trace, never retried, as 1.0.2's index
queue skips a message whose blob is missing); a transient error aborts
the pass with no index write, no blob, no checkpoint, `indexed_to`
unchanged, and the next pass retries; the one write an aborted pass
makes is a manifest-only checkpoint (above) carrying the id of the
document whose open failed and its count of aborted attempts, public
facts (R34: held in memory alone the count would not survive an
eviction or a restart and the bound would never be reached), and a
document whose count reaches `ZA_SEARCH_SKIP_AFTER` is then skipped as
loss with `ZA_SEARCH_SKIPPED` carrying
the persistent class (H5: a single object answering a persistent
non-absent error, an S3 403 on one key or a backend fault, must not
wedge the account's index and every later message's threading for
ever). In a load, loss of a thread blob is loss of the index (a
rebuild, 5.4), loss of a content blob is loss of the content part only
(a content-only rebuild, 5.4, H23); a transient error is "no map" for
this request (6.2) and nothing more. Nothing is ever skipped for good,
or rebuilt, because the blob store was slow. A blob store that answers
"not found" for every key is not slow (I6: a wrong bucket or prefix, a
restore to another path): past the window every admitted message is
skipped as loss while its stub stands, and threads as a singleton until
a rebuild, which only loss triggers. The remedy is upstream's
per-account reindex maintenance task
(`TaskAccountMaintenanceType::Reindex`,
`crates/services/src/task_manager/tasks/maintenance.rs:433`): for a key
account it also writes the index's destroying marker (5.7) without
touching the account, so the next keyed request after the repair
rebuilds; a one-line hook at that site, and the operator's step after
repairing the store.

0. **Shortcut, and catching up with other nodes** (R10, R18, F19). The
   pass is done at once when the manifest as loaded holds no cursor
   **and** the message cache's `last_change_id`
   (`crates/common/src/lib.rs:288-296`; the cache behind it revalidates
   against the log when older than 5 s or marked stale,
   `crates/email/src/cache/mod.rs:55-57`, so the comparison is free and
   the snapshot it reads is at most that stale) is not ahead of the
   loaded index's `scanned_to` (step 2). Otherwise it reads the manifest
   (one key); if its hash differs from the loaded index's, it reloads,
   incrementally (4.3), before anything else (5.6): a different hash
   means another node committed something, a deletion-only pass or a
   rebuild included, whatever the identity version says.
1. **Consume, in order, up to the cap.** Read the change log keylessly
   in record order with each record's change id. `Store::changes` folds
   records into one flat vector and keeps only range-level change ids
   (`crates/store/src/query/log.rs:118-183`), so the fork adds a reader
   beside it, `za_changes_records`, that iterates the same keys
   (`LogKey{account_id, collection, change_id}`) from `indexed_to`'s
   record, inclusively when that record is partial and exclusively when
   it is complete (R29), and yields `(change_id, Vec<Change>)` per
   record, deserialized by upstream's record decoder, unfolded, and
   reports truncation as `changes` does, the marker in place of a
   partial record included. A truncated log takes the
   reconciliation of 5.4 instead. For each record, in order:
   - an **insert** (the thread prefix in the high bits is ignored: it is
     the singleton): skipped if the index already holds the document;
     otherwise read the stub and the blob as the FETCH path does
     (`za_open_metadata_row`, `za_open_message_blob`,
     `crates/email/src/message/za/view.rs`); on loss (the message was
     deleted by a record the pass has not reached, or does not open) the
     document is recorded as skipped, with a trace carrying the ids
     (section 8); on a transient error the pass aborts; otherwise call
     `index_document`, `MessageSortKeys::from_envelope`,
     `za_has_attachment` and `ThreadFields::scan` (keeping `K` hashes,
     4.2), assign the thread among the records the staging index holds
     as present at this point (6.1), writing one alias per thread the
     assignment retires and never touching another record (R21), and
     add the document;
   - a **deletion**: a tombstone, unconditionally, recording the
     record's change id as the deletion change id and dropping the
     threading fields. A document id is never
     reused (`ValueClass::DocumentId` is a per-collection counter,
     `crates/store/src/write/key.rs:493-496`, `Subspace::Counter`,
     filled at commit, `write/mod.rs:1573-1580`), so a logged deletion
     is final whatever any cache shows (R17); for a document the index
     does not hold, nothing;
   - an **update**: nothing. It is a flag or mailbox change, or the
     pass's own entry of 6.3; a key account's message content never
     changes in place, and an IMAP COPY within the account rewrites the
     same document with the same content
     (`crates/imap/src/op/copy_move.rs:407-419`).
   A pending cursor (5.4) comes first, oldest document id first, each
   item indexed as an insert is, with the epoch as its creation change
   id; log records are read only once the cursor is exhausted (the
   order above). The **cap** (R1, R20, R21, G18) bounds what a pass
   opens and stages, by charges that depend on public counts alone: each
   admitted insert or cursor item is charged one **reservation** of
   `R + (K − 1) · a` hidden bytes whatever its record and aliases come
   to (R21: charging actual alias bytes made the stopping point, which
   `indexed_to` publishes, depend on whether a probe's references
   already shared a thread), and the pass stops at the next insert
   boundary once the admitted items reach `ZA_SEARCH_PASS_CAP`, their
   reservations `ZA_SEARCH_PASS_HIDDEN_KB`, their opened message bytes
   `ZA_SEARCH_PASS_MB`, or the elapsed time `ZA_SEARCH_PASS_MS`
   (attachment text extraction runs inline in the triggering command,
   `crates/email/src/message/index/search.rs:91-99`, so time is bounded
   too). It always admits at least one item, so the overshoot is one
   reservation; a record is left part-way when the cap falls inside it
   (R20: a grouped record is not bounded by `MAX_GROUP_EMAILS`, since
   IMAP MULTIAPPEND builds `IngestGroup::with_max_emails(messages.len())`,
   `crates/imap/src/op/append.rs:186`, so a record's size cannot be the
   overshoot). `to`, the pass's target, is the checkpoint `(c, p)` of
   the last insert it admitted, or the last whole record it consumed, or
   `indexed_to` unchanged when it processed cursor items only. A
   repeated delta (5.6, a failed write) is harmless: an insert already
   held is skipped and a tombstone is idempotent.
2. **Decide what to write** (F4, R13, R22, R26, G19). Three cases, told
   apart by what changed. A pass that consumed only update records,
   staging nothing and reaching no cursor item, writes nothing at all:
   it notes in the loaded index, in memory only, the change id it
   scanned to (`scanned_to`), which step 0 compares against so the same
   records are not re-read on this node, and leaves the manifest alone;
   an eviction forgets `scanned_to` and the next pass re-reads from
   `indexed_to`. A pass that consumed any insert or delete record, or
   any cursor item, but staged nothing, because every insert was skipped
   for loss or already held and every deletion named a document the
   index does not hold (R22, R26: a permanently unreadable message must
   not be reopened on every pass, and must not hold `content_to` back,
   since coverage means every *indexed* message has content), and a pass
   that only crossed `ZA_SEARCH_CHECKPOINT` update records since
   `indexed_to`, write a **manifest-only checkpoint**: the manifest with
   `indexed_to`, `content_to`, the cursor, or a document's aborted-attempt
   count (the error classes above, R34) advanced, asserting the
   previous bytes, no blob, no log entry, a few kilobytes that are a
   function of the log alone; so a long run of flag changes never forces
   a reconciliation, a cursor of vanished messages still finishes, and a
   skipped insert is consumed once. A pass that tombstoned but indexed
   nothing is **non-publishing**: it writes a thread blob with no
   hidden section and no revision row, its public section the
   tombstones alone (H13: an empty padded section would cost `M` per
   deletion-only pass and hide nothing, since the pass's kind is public
   already), advances `indexed_to`, opens no revision and writes no log
   entry. A
   pass that indexed at least one message is **publishing**: it opens
   revision `revision + 1`, known before anything is written (R2),
   records under it every assignment it makes and every alias, and
   writes the entries of 6.3.
3. **Write.** Seal and stage the thread blob (its hidden section padded,
   its public section plain, 4.2) and, if there were inserts and the
   account is under the cap, the content blob (its predecessor the
   current head), holding each for the upload TTL class, an hour, not
   the wait budget (G15: a compacted content blob to a slow store can
   outlive a short hold, and a hold that expires before the batch lets
   the purge delete a blob the committed manifest names). Then one
   batch, committed through
   `Server::commit_batch` (`crates/common/src/storage/transaction.rs`;
   F2: it marks the account's caches stale and broadcasts the
   `StateChange` the entries of 6.3 rely on): the manifest, asserting
   the previous bytes, with `indexed_to = to`, the new thread blob
   appended, the content head, length, size and `content_to` moved, and,
   for a publishing pass, `revision + 1` and its change-id row, filled in
   at commit (R33); the
   holds released and the permanent links of the new blobs; the unlinks
   of any blobs the batch retires (5.5); and, for a publishing pass, the
   log entries of 6.3. On an assertion failure (another node wrote, or
   destroy's marker, 5.6, 5.7) discard the staging structure, let the
   holds expire, reload from the manifest, and return; the request
   proceeds on the reloaded index. On any other failure, trace, discard,
   and leave `indexed_to` where it was: the next pass repeats the work.
   After a successful commit, swap the staging structure into the loaded
   index under the write lock (4.3) and record the new manifest hash,
   provided the loaded index is still at the hash the pass read at step
   0 (I4: between the commit and the swap a request's validated snapshot
   may have reloaded the index at this manifest or a later one, and the
   swap would then mislabel a newer index, or put back aliases a
   compaction folded); otherwise discard the staging, since the reload
   already holds its content.
4. **Compact** when either count reaches `ZA_SEARCH_SEGMENTS` (5.5).

### 5.2 Triggers

The pass is a request-time hook, as the drain is for DAV
(`handle_dav_request`, `crates/dav/src/request.rs`): before a request of
a key account that holds its keys is served, run one pass, or proceed at
once if one is running (5.1). Concretely:

- JMAP: in the dispatcher (`crates/jmap/src/api/request.rs`), for every
  mail method of a key account once the keys are known to be on the
  token (`access_token.za_keys_for(account_id)`); one pass per request
  and keyed account, the owner's, not per method call (I7).
- IMAP: before each command of a session that `za_key_session()`
  answers true for (`crates/imap/src/core/mod.rs:304-307`), after the
  command is parsed, except IDLE, NOOP and LOGOUT. A client in IDLE
  learns of new mail from EXISTS and fetches it; that FETCH runs the
  pass, so the message's THREADID and EMAILID are final by the time the
  client reads them, in normal use.
- POP3: no pass. Nothing POP3 serves depends on the index.
- A keyed write that adds a message (IMAP APPEND, `Email/set` create,
  `Email/import`): one pass after its commit and before its response,
  beside the pass the dispatcher or the command loop ran before the
  method, so such a request runs at most two, each bounded (H20). In
  normal use the delta is that message's record and the response carries
  its final id (6.2); after a backlog (a rebuild, a long absence) the
  pass may not reach it, and the response carries the id as loaded,
  which a later pass reassigns and renders as 6.3 renders every
  reassignment. The write itself does not index; it is picked up from
  the log like a delivery.

Two classes of request follow from the two kinds of record (4.1):

- A **content request** (a search with a content filter or sort, a
  snippet, IMAP SEARCH and SORT with content keys, `Email/queryChanges`
  with such a filter) runs passes, or waits for the running one, until
  the index is current or `ZA_SEARCH_WAIT_SECS` has elapsed, then
  answers from a current index or refuses (7.4).
- Every other keyed request runs one pass if none is running and then
  proceeds with the thread map **as loaded**, under the validated
  snapshot of 6.2: the thread-valued reads (`Email/get` ids and
  `threadId`, `Thread/get`, `Thread/changes`, `Email/changes`,
  `Email/queryChanges` without a content filter, `collapseThreads`,
  `inThread`, the thread filters and sorts, `Mailbox/get`'s thread
  counts, `Blob/lookup`, `EmailSubmission/*`, IMAP `THREAD`, `EMAILID`,
  `THREADID`) answer from whatever the map holds after that pass, and are
  never held or refused by the index while a map can be loaded and
  validated. Between the pass and a current map lies only the capped
  remainder of a backlog, whose reassignments later passes publish and
  6.3 renders; in normal use the map is current after the pass.

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

A **rebuild** starts when a keyed request finds **loss** (5.1's error
class): no manifest on a key account, whether or not it holds mail
(H11: an account provisioned before this release, or one whose empty
manifest was deleted, must get one, and the no-message boundary below
writes it), a manifest of an *older* format version, a manifest that
does not parse or whose tag fails (4.2), or a thread blob that the
store reports absent past the retry window or that fails its AEAD or
parse. Loss of a content blob alone, a chain node absent or unopenable,
is not loss of the index (H23): the thread part and every identity are
intact, so it takes the content-only rebuild below, dropping the chain
and resetting `content_to`, and no client resyncs. A manifest of a
*newer* format version is not loss (F7): during
a rolling upgrade the older binary answers "no map" (6.2) and
unavailable (7.4) for that account until it is upgraded, and never
rebuilds under the older format. A transient error is never loss.

Its first batch (R3) is a publishing write by construction, whether or
not it indexes anything: it writes a fresh manifest (no blobs, the
revision counter continued from the old manifest, or restarted at 1
when the manifest is missing or fails its tag, which is harmless since
the manifest hash's equality and the fresh epoch, never the counter,
decide validation (6.2, I7), `indexed_to` set to the account's current
Email change id, and a rebuild cursor), the unlinks of every previous
blob the old inventory reaches, and, when the account holds any message,
the boundary entries: one Email item update of the lowest document id
present, one Thread property change of that singleton, and a container
property change of every mailbox (6.3). The boundary is a publishing
write: it opens `revision + 1` and its row `(revision + 1, epoch)`, so
`R(X)` for every `X` from the epoch on resolves to it (H14), and the
batch's change id, filled in at commit as that row is (R33), becomes
`epoch`. It is
therefore newer than every state any client was ever issued, including
the latest one, and distinct for every loss, so no issued state
survives the boundary and no boundary is reused. An account that holds
no message still writes the mailbox property changes (every key account
has mailboxes), so its boundary is a fresh change id too (G3): a client
at a pre-loss state can hold the mapped id of a message deleted since,
whose record the loss took, and must resync like any other. Two nodes
that find loss at once write competing first batches, and the manifest
assertion decides (5.6).

The old inventory is read as far as it goes: the thread list, and the
content chain from its head. If the traversal is incomplete anywhere, a
missing or unopenable node in the middle included (R7), the batch is
preceded by the principal-link scan of 5.7, so that every blob the
account ever linked is unlinked, not only those the readable part names.

From then on each pass works the cursor first, oldest document id
first, and reads no log record after `indexed_to` until the cursor is
exhausted (5.1's chronological order, R20); step 0's shortcut never
applies while the cursor is pending (R10). The cursor is the set of
document ids present in one message-cache snapshot taken when the
rebuild started, walked upwards; the manifest stores the last id done
and the highest id of that snapshot as the cursor's end, and the
rebuild's `indexed_to` is that same snapshot's `last_change_id`, so a
message between the snapshot and the first batch is in the log after
`indexed_to` and in neither gap (H15). Oldest first is
upstream's own order, which is what bounds a merge (5.1); its cost is
freshness during the rebuild: mail that arrives while the cursor runs
is indexed only after it, so until then it is not searchable and
threads as a singleton, a loss-only case of section 2's freshness
limit. The rebuild is done when the cursor is exhausted; until then
the content part is incomplete, so a content request waits for it up to
the budget and refuses beyond (7.4), while the thread map serves as
loaded. A **content-only rebuild** (R23), taken when the account cap is
raised over a suspended chain (4.3) or when a node finds `content_to`
behind `indexed_to` with the chain loadable, keeps the thread part and
walks a cursor over the messages the index holds without a content
record, newest first (nothing threads, so the order is free and recent
mail is searchable first), skipping on *content* presence rather than
on the record's, under `content_to`'s own cursor bound (4.2), which
can be pending beside a thread rebuild's cursor; `content_to` itself
does not move until that cursor is exhausted, since it promises content
for every indexed message up to it (I7): no epoch moves, no identity
changes, and content requests
refuse until `content_to` has caught up with `indexed_to`.

The identities a lost index had published cannot be reconstructed, so
`epoch` invalidates the sync states that would need them: a `/changes`
or `/queryChanges` call of Email or Thread with a since-state `S` below
the epoch (an intermediate state's `from` included) answers
`cannotCalculateChanges` when that method's log holds a record of the
method's kind, an item change for Email and a container change for
Thread (the Email log also carries the mailbox container changes of
6.3, which do not count; upstream's own kind filter,
`crates/jmap/src/changes/get.rs:566-568`; R27), with a
change id in `(S, epoch]`, closed at the epoch (R27, H1: the boundary
batch's own Email witness update and Thread property change sit *at*
the epoch, and they are what a client at the latest pre-loss state must
not pass; an open interval let it through); when it holds none, nothing
was lost for that client and the call is served (R27: an account whose
messages were all deleted before the loss writes no Email or Thread
entry at its boundary, so its clients' states stay usable). `Mailbox/*`
is outside the rule (H12): nothing of the mailbox history is lost at a
loss, and a keyless session has no map to carry an epoch. `Initial`,
and its continuation (6.3), are never refused, since a client at
`Initial` holds nothing. Clients refused resync as JMAP clients do on
that error (a full query and get).
From the epoch on, every assignment the rebuild makes is an ordinary
transition from the singleton with a published revision, rendered by 6.3
for clients that synced after the epoch. IMAP keeps no object-id sync
state, so nothing is invalidated there.

A **reconciliation** runs when the change log is truncated below
`indexed_to` (`is_truncated`; the log keeps `maxChangesHistory`
records, 10,000 by default). It is the one place a pass consults the
message cache for existence (an exception to 5.1's rule, G11): the pass
writes a reconciliation marker into the manifest, with the retained
log's bound as it read it (4.2, I7), and each
pass recomputes the difference between the index's document set and a
revalidated cache snapshot's, indexing what the index lacks and
tombstoning what the cache lacks through a cursor over the difference,
in this order (H3, I1): the gap items first (those the retained log
does not show), their inserts oldest document id first and then their
deletions, so that a gap insert finds every message it references that
the gap deleted, as upstream did whenever the insert came first (a gap
insert and a gap deletion carry no order of their own, so an insert
that in fact arrived after the deletion threads with the deleted
message where upstream did not: 7.5); then the retained records in
record order, inserts before deletions within a record, which find the
gap's deletions applied, since every gap deletion preceded every
retained record (I1: a retained insert that references a message the
gap deleted threads as upstream did, without it); then sets
`indexed_to` to the cache's change
id. Before stamping
anything it attributes by the retained log (R24, G2): it reads the
retained records once, unfolded, and a message of the difference whose
raw insert or delete lies among them takes that record's change id, in
record order after the gap items; only a message the retained log does
not show takes the log's oldest retained change id minus one (F9: the
message was gone, or existed, at every state the log can still serve).
So 6.3 never renders a deletion a client already saw, never a creation
of a message it already holds, and never loses a deletion whose record
is still in the log and whose client state is still servable. "The
retained log" is the shorter of the Email and Thread logs (G4). It is
the rebuild restricted to the difference, shares its machinery and the
cursor rule of step 0, keeps the aliases and does not move the epoch:
the truncated log already refuses the states it can no longer serve. If
the log truncates while a rebuild cursor is pending, the difference
cursor subsumes the rebuild's (the rebuild's remaining items are in it),
`indexed_to` moves as a reconciliation moves it, and the epoch stays
where the rebuild put it.

A product account gets an empty manifest at setup (`za_setup`,
`crates/http/src/api/vault.rs:1081`), in the batch that writes the vault
record, with `indexed_to` set to the account's current Email change id
(the account holds no mail: mail spec 6.6's refusals) and `epoch` set to
`0`, since no state has been issued and nothing needs refusing (G3). It
therefore never rebuilds unless the index is lost.

### 5.5 Compaction

Two compactions, each a pass's step 4 and each published in the pass's
own batch (new links, old unlinks, the manifest, atomically):

- **Thread compaction**, when the thread-blob list reaches
  `ZA_SEARCH_SEGMENTS`: any node, since the thread part is always
  loaded. It merges every thread record into one new thread blob and
  prunes what no client can ask for, by the bound of the shorter of the
  Email and Thread logs (G4: `delete_changes` truncates each to
  `maxChangesHistory` records on its own,
  `crates/common/src/storage/transaction.rs:116-139`, and the Thread
  log, which carries no flag changes, keeps older change ids than the
  Email log): a deleted message's record whose deletion change id is
  older than that bound; a revision-table row older than it, except the
  newest such row, kept as the **floor row** so that `R(X)` is defined
  at every servable state between the bound and the next retained row
  (plan 16 call E1; the row count is public already); and an
  alias older than it, which
  is first **folded** into the records it governs (every record whose
  assignment is the alias's source takes the alias's target as its
  assignment, one hop, so the retained aliases above it still apply to
  that record at the states they were published for (R35); a rewrite of
  members, but at compaction, where every record is rewritten and the
  class is a function of counts), then dropped
  (`Email/changes` with an older state gets `cannotCalculateChanges`
  from the log itself, and states before the epoch are refused by 5.4).
  Two more pruning rules make the compacted blob's size a function of
  public counts (decision 2 (c), R35). **The bridge rule**: a deleted
  message's record and its tombstone entry stay while the publication
  that assigned it is retained, that is until both its deletion change
  id and its assignment's change id are below the bound; so the bridge
  of every retained alias is a retained record, assigned to the alias's
  target. **The dead-alias rule**: an alias is dropped when, after the
  record pruning and the folding above, no retained record is assigned
  to its source thread. The rules answer the timeline Codex R35 gave: a
  pass catching up on a backlog applies records in order, so a bridge
  inserted before its roots were deleted merges them, and its aliases
  publish at a change id after the deletions; at a bound between the
  two, the roots are pruned and the aliases would survive with no
  record behind them (three roots and one bridge leave one record and
  two aliases; 128 bridges of fifteen roots each leave 128 records and
  1,792 aliases, 80,896 bytes in a 64 KiB class). The index's view of
  which threads existed at a change id and the log's view differ inside
  a backlog, so no argument from "members present at the merge" bounds
  the aliases; the dead-alias rule does, by an injection. **Bound**: a
  thread is retired once, so alias sources are distinct; each kept alias
  has a retained record assigned to its source; a record has one
  assignment; so the kept aliases never exceed the retained records.
  **Resolution**: for a retained record `D` and a servable state `X`,
  the chain from `D`'s assignment uses aliases at or above the bound
  (those below were folded into `D`, one hop each); if the chain uses
  `L1 → L2`, that alias's bridge is retained by the bridge rule and is
  assigned to `L2`, so `L2`'s own alias, if any, has a record assigned
  to its source and is kept; by induction every alias on every retained
  record's chain is kept, and "the thread of `D` as of `X`" and "the
  members of `T` as of `X`" (6.1) are unchanged by the rule. An alias
  the rule drops governed only pruned records, whose deletions sit
  below every servable state. The compacted hidden section is therefore
  padded to `N_records · (R + a)` (4.2), a function of the public
  counts, and its class is the same whatever the window merged (in
  R35's timelines every dead alias drops and the records fit). About
  twice the records' bytes at the design size (1.3), a 16 MiB class at
  30,000 messages, read once per load (4.3), and what keeps the manifest
  bounded (R8).
- **Content compaction**, when the content chain reaches
  `ZA_SEARCH_SEGMENTS` and no cursor is pending; while a cursor is
  pending, a **tiered** merge instead (G17, H7, R32: a full merge every
  sixteenth pass would rewrite everything indexed so far, near-quadratic
  in writes; no merge at all would leave a rebuild's chain of hundreds
  of nodes to the first content load; and revision 15's rule, the
  youngest `ZA_SEARCH_SEGMENTS` blobs merged whenever the chain reached
  twice that count, re-selected the previous merge's result every time,
  a quadratic rewrite that only looked logarithmic). Every content blob
  carries a **tier** in its authenticated header, `0` for a pass's own
  blob; when the chain holds `ZA_SEARCH_SEGMENTS` blobs of one tier they
  merge into one blob of the next tier, cascading within the pass when
  the merge completes the next tier. Blobs of one tier are always a
  contiguous run at the chain's young end when they merge, since a
  tier's blob is made only when the tier below has been emptied, so the
  chain stays a sequence of tiers, oldest highest. Each byte is then
  rewritten once per tier it passes through, `⌈log_S n⌉` times over a
  rebuild of `n` passes at `S = ZA_SEARCH_SEGMENTS`, and the chain holds
  at most `S − 1` blobs of each tier plus the one forming, under
  `(S − 1) · ⌈log_S n⌉ + 1` nodes: 31 for 256 passes and 46 for 4,096 at
  the default; the full merge follows once the cursor is exhausted: a
  node that has the content part loaded, or that can load
  it (under the account cap), loads it for this and merges the chain
  into one content blob with no predecessor, dropping the content of
  tombstoned messages. Above the cap the chain is not extended (4.2), so
  it never grows past `ZA_SEARCH_SEGMENTS` plus one. Compaction is the
  only write proportional to the whole content; at the design size it
  is a second or two every sixteen passes.

### 5.6 Cluster

Two nodes with keyed sessions of one account run passes independently.
The pass is deterministic in what it decides: the same records,
processed in order with the same thread rules, yield the same additions
and the same assignments (not the same bytes: every seal draws a random
nonce and every blob a random segment id, F13, so the loser's blobs are
purged after their holds). Correctness comes from the manifest
assertion, not from a lock: exactly one of two concurrent manifest
writes lands, the other node discards its staging, reloads and
continues. A node learns that another changed the manifest at step 0 of
its next pass, which every keyed request runs: the shared cache's
`last_change_id` moves with the other node's publishing commit
(`commit_batch` marks caches stale and broadcasts only when the batch
logged a change, `crates/common/src/storage/transaction.rs:37-48`, H16,
so a non-publishing pass or a manifest-only checkpoint moves no other
node's counter and is learnt at that node's next manifest read, which
every id-emitting response makes) or with any delivery, the manifest
read shows a different hash, and the node reloads before it serves; and
every id-emitting
response validates its map against the manifest and its state against
the newest publication after reading its state (6.2), which closes the
windows between the pass and the response that the cache's own refresh
can open. Between a commit on one node and the next validated response
on another, the second node serves the previous manifest under a state
that manifest covers.

The UID lease (`crates/common/src/za/lease.rs`) and the index queue's
partition lock are not used: neither prevents anything the assertion
does not already prevent, and a lease would add a sweep.

### 5.7 Account lifecycle

- **Setup** writes the empty manifest (5.4).
- **Password change, recovery, recovery-key regeneration, app
  passwords** change nothing: `swk` is derived from `mk`. The account
  invalidation these cause evicts the loaded index (4.3), which reloads
  on the next keyed request.
- **Destroy** (R19). Account destruction clears the Property subspace by
  range (`danger_destroy_account`, `crates/store/src/dispatch/store.rs:428`),
  which removes the manifest, but blob links are hash-prefixed and
  `destroy_account_blobs` (`crates/services/src/task_manager/tasks/destroy_account.rs:317-392`,
  called at `:214` before the range delete at `:246`) walks only Email,
  FileNode and SieveScript archives. A keyed pass can be in flight while
  this runs: an IMAP session's token and keys outlive the principal
  until a command refreshes the token, and the pass asserts only the
  manifest it read. The fork adds one step before `destroy_account_blobs`,
  and it fences first: it writes field 152 as a **destroying marker**
  that carries the inventory of the manifest it replaces (G12),
  asserting the previous bytes (or `None` when the account has no
  manifest), so that every in-flight pass, and a rebuild asserting
  `None`, loses its assertion from then on, and a loader that reads the
  marker treats it as "no map"; then it unlinks, from the marker's
  inventory, every thread blob and every content blob the chain from
  its head reaches, so a destroy that dies after the marker retries from
  the marker with nothing lost. If the manifest was missing or did not
  parse, or the chain cannot be traversed to its end (R7), the step
  instead scans the blob-link subspace for this account's `Document`
  links on `Collection::Principal` document 0 and clears each in chunks
  (F23: a walk of the link keys with the account, collection and
  document in their suffix, shaped like the purge's shard iteration,
  `crates/store/src/write/blob.rs:159-287`, and not `user_blob_hashes`,
  which returns hashes without their links; the keys are hash-first, so
  the walk reads every link of every account once, a cost stated here
  and paid only on this path). The range delete then removes the marker.
  A batch that *creates* a manifest where none exists (a rebuild's first
  batch, `AssertValue::None`) also asserts the vault record's presence
  (field 150, `AssertValue::Hash`), since the range delete takes marker
  and vault record together and a pass admitted before the destroy
  could otherwise recreate the manifest after it (R19, second form).
  Blobs a failed pass staged hold only expiring links (4.2) and need no
  step. The scan is the ordinary path for one class of account (F23): a
  key account provisioned before this release that never ran a keyed
  request afterwards has no manifest, and its destroy takes the scan,
  finding nothing. The za suite's
  `assert_is_empty` exempts field 152 and the index's blobs while their
  account exists, as it exempts fields 150 and 151, and the suite's
  destroy-then-purge-then-assert order verifies that nothing remains.
- **Provisioning fence.** No ordinary writer touches the index, so no
  fence is added; `za_assert_no_data` need not check field 152 (an
  unprovisioned account cannot have one).

### 5.8 The model against the scenarios

Every failure scenario the reviews raised, checked against the four
rules of 5.1, the identity rules of 6.2 and 6.3, and the storage rules
of 4.2, as one model rather than one patch each:

| Scenario (review) | What handles it | Why it holds |
|---|---|---|
| A grouped record split by the insert cap, one message skipped forever (R1) | records consumed whole; `indexed_to` is a record boundary | the cap stops only between records |
| An unindexed message fetched as `D\|D` under a later state, then assigned (R2, first form) | transitions stamped with the publishing revision; the revision row's change id filled in at commit | the client's state is below the publication, so `/changes` renders it |
| A response whose map lags the state it returns, across nodes or epochs (R2, later forms) | the validated snapshot: the manifest hash and the identity version, equality not ordering | a publication's log entry and its row commit together |
| A response whose state lags the map's newest publication (F2) | the validated snapshot also requires the cache's state at or past the newest revision row and the epoch, re-reading a stale cache | no mapped id is ever returned under a state below its publication |
| A client at the latest state across an index loss (R3) | the rebuild's boundary batch allocates a fresh change id for `epoch` | every issued state is below it |
| A deleted message's old mapped id (R3) | identity retained until the deletion leaves the log | the raw delete entry keys the rendering; the record supplies `T0` |
| The reassigned cohort visible in the log (R4, both forms) | one witness chosen by id among the indexed messages; every indexing pass publishes; every mailbox bumped | the entries are a function of the log alone |
| A history lookup that returned the wrong "before" (R5) | the forward lookup over the revision table | earliest transition after the state, else current |
| A permanent link without a manifest, a broken chain (R7) | holds, then release and links in the manifest's batch; the scan on any incomplete traversal | nothing permanent exists outside a committed manifest's reach, or the scan finds it |
| A manifest that grows with every pass (R8) | thread compaction by any node; the content chain | the manifest lists a bounded thread list and one head |
| A thread-only load that must decrypt content (R9) | separate blobs, each one AEAD | the loader reads the kind it needs |
| A rebuild stalled by the no-work shortcut (R10) | the cursor overrides the shortcut | step 0 checks the cursor before the comparison |
| A chain node without its segment id (R11) | the authenticated header carries own and predecessor identities | the AAD is built from the header |
| Synthetic pages counted by raw entries (R12) | the fork's page over a pinned range with intermediate states | count, order and bound agree by construction |
| A deletion-only pass with no carrier (R13) | non-publishing; the tombstone carries the deletion's log change id | the raw delete entry is the carrier |
| A message created and deleted inside one range (R14, first form) | existence at both ends from the unfolded raw log, then the record | absent at both ends yields nothing |
| A deletion behind the insert cap, the old id never destroyed (R14, second form; F10) | the unfolded raw overlay in the renderer; thread reads intersect the cache's present set | a delete record in `(S, E]` makes `D` absent at `E` whatever the record or the folded view says |
| A later deletion erasing the evidence an earlier insert needed (R16, log) | strict order; fields dropped only at tombstoning | the insert is processed before the deletion, with the deleted message still a candidate, as upstream had it |
| The same during a rebuild, an older cursor item pending (R16, cursor, both forms) | the cursor runs oldest first and no post-epoch record is applied until it is exhausted | no tombstone exists while a cursor is pending, so no evidence can be dropped |
| One cursor item merging arbitrarily many threads; a record larger than any fixed overshoot (R20) | chronological order; the intra-record checkpoint; a fixed reservation per admitted item | each hash names one present message in one thread, so at most `K − 1` aliases; the overshoot is one reservation whatever the record |
| The stopping point moving with a probe's actual aliases (R21) | every admitted item charged the same reservation | `indexed_to` is a function of the admitted count |
| A permanently skipped insert never checkpointed, holding search back (R26) | a consumed insert or delete record checkpoints even when nothing was staged | only update-only work is a no-op |
| An empty account's post-rebuild states refused for ever (R27) | the epoch refuses a state only when the method's log holds a record of the method's kind in `(S, epoch]`; `Initial` never | nothing was lost for a client whose delta has no such record; mailbox entries in the Email log do not count |
| An `Initial` page continued as `Exact` with another classification, or refused below the retained bound or the epoch (R28, both forms) | `Initial` is `Exact(0)`; `from = 0` is never refused | a client there holds nothing, and 0 is below every record |
| An `Initial` continuation after a truncation skipping a survivor (R31) | the `S = 0` stream is every document present at `E`, in document-id order | no retained record decides it, so the pages are the same before and after the truncation |
| The latest pre-loss state admitted by an open interval (R27, H1) | the interval is `(S, epoch]` | the boundary's own Email and Thread entries sit at the epoch |
| A split record's suffix skipped by an exclusive resume (R29) | the checkpoint carries `complete`; a partial record is resumed inclusively; its marker is loss | the reader skips the from-key only for a complete record |
| The time cap making the checkpoint a function of processing cost (R30, H4) | recorded as the one remaining channel; invariants 48 and 50 narrowed to the byte and count charges | duration is observable anyway |
| A deletion committed between a pass's log read and its commit, a phantom `created` (H2) | existence at `E` from the cache's present set, or from the log outside the range for a pinned `E`, before the record | a lagging record never decides presence at `E` |
| A reconciliation tombstoning before it inserts (H3, I1) | gap inserts oldest first, then the gap deletions, then the retained records | a gap insert finds what the gap deleted; a retained insert, which came after every gap deletion, does not, as upstream |
| One persistently failing object wedging the index (H5) | a per-document attempt count in the manifest; skipped as loss after `ZA_SEARCH_SKIP_AFTER` | a persistent fault is bounded; a transient one still retries |
| A lagging blob store read as loss (H6) | absence is loss only past `ZA_SEARCH_ABSENT_RETRY_SECS` | a committed manifest's blob exists |
| A thread-part load refused for budget, failing every mail method (H8) | thread loads evict idle parts and overrun the soft budget with a trace, never refuse | mail degrades after search |
| An unauthenticated manifest rewritten from authentic blobs (H9) | the keyed tag; a bad tag is loss | every manifest writer holds keys |
| A tag computed before the change id is filled in, failing every reload and forcing a rebuild on every publish (R33) | the manifest serializes at commit with its ids in hand, the tag last | `SerializeWithIds` runs at commit |
| An empty account without a manifest in "no map" for ever (H11) | a missing manifest is loss on any key account; the boundary writes it | the no-message boundary exists |
| A content node's loss resetting every identity (H23) | loss of a content blob is a content-only rebuild | the thread part is intact |
| A deletion skipped against a stale cache snapshot (R17) | the pass consults no cache for existence; a logged deletion is applied unconditionally; ids are never reused | there is no snapshot to be stale |
| A fresh checkpoint adopted with stale records after a deletion-only pass (R18) | the loaded index is versioned by the manifest's hash, the value the write asserts | any committed change reloads, identity publication or not |
| A pass publishing between destroy's inventory and its range delete (R19) | destroy writes the marker first, asserting the manifest | every in-flight pass and every rebuild loses its assertion |
| An operator-driven size oracle on the thread blob (F1) | bounded records, `C · R` under the minimum class | the operator cannot place a pass near a boundary |
| A transient blob-store error skipping a message for good, or rebuilding (F3) | the error classes | only loss skips or rebuilds |
| A padded blob written per flag change (F4) | a pass with nothing to stage writes nothing | `scanned_to` keeps the shortcut cheap |
| Reads serialized behind a pass's blob I/O (F5) | the read lock and the pass lock; a request never waits for a running pass | the swap is the only write a reader meets |
| A chain unbounded above the cap, its size unknowable (F6) | length and size in the manifest; no extension above the cap | the cap is read, not walked |
| A newer manifest format on an older node (F7) | newer is "no map", not loss | only an older format rebuilds |
| A reconciliation's change ids rendering seen events again (F9) | the oldest retained change id minus one, for what the retained log does not show | existed, or gone, at every servable state |
| A reconciliation stamping a deletion the retained log still holds, the old id never destroyed (R24, G2) | attribution by the retained records before stamping | a record in the log keeps its real change id |
| Tombstones and a soft insert cap put sizable bytes in the padded payload (R20, G1) | the public section; the enforced hidden budget `H` with a derived overshoot; `M` above the total | the hidden section always pads to `M` |
| Two known Message-IDs tested for a shared thread through an amplified merge (R21) | aliases: a merge writes one entry, never its members | a merge's size does not depend on the threads' sizes |
| A chosen message shrinking a content blob by what it shares with a target (G5) | per-record compression | no cross-record window |
| An empty account refused every id-emitting method; a no-message rebuild without a boundary (G3) | the snapshot compares the Email group's counter; setup writes epoch 0; the no-message boundary writes mailbox entries | every boundary and publication moves what is compared |
| A `Thread/changes` state the Thread log serves but the Email log does not (G4) | the shorter log's bound for pruning, stamps and refusals | nothing needed is pruned while either log serves the state |
| Thread parts crowded out, or an account cap no load can satisfy (G6) | a thread budget of its own; single-flight loads; the cap at a third of the budget | mail degrades after search, never before |
| Evidence a pending cursor needs dropped at compaction (R16, compaction form) | a pending cursor holds every tombstone through compaction and reload | the discard rule and the pruning rule agree |
| A manifest recreated after destroy by a batch asserting `None` (R19, second form) | the creating batch also asserts the vault record | the range delete takes both |
| A cursor of vanished messages that never finishes (R22) | cursor movement is durable progress: a manifest-only checkpoint | the empty-staging rule exempts checkpoints |
| Content missing for messages indexed while suspended (R23) | `content_to` and the suspended flag; a content-only rebuild that skips on content presence | coverage is in the manifest, not inferred from the chain |
| A thread's members at a pinned end taken from the present table (R25) | membership as of `X` defined from the records and aliases | a member deleted or merged after `X` still counts at `X` |

Two residuals the model leaves, both recorded in 7.5: a message that
arrives and is deleted before any keyed request indexes it (a POP3-only
reader, say) gives no threading evidence to a later message that
referenced only it, where upstream's ingest threaded with it present;
and the same for a message deleted during a rebuild before the cursor
reached it. Both are the freshness limit of section 2 in another form:
the index can only use what it was given a chance to read.

## 6. Threads

### 6.1 Assignment

The pass threads a message as upstream's ingest does for an ordinary
account (`find_thread_id`, `crates/email/src/message/ingest.rs:1098-1197`,
and `ThreadMerge::merge`, `:1487-1512`), against the index instead of the
`Threading` key class:

- Candidates are the messages the index holds as present at the point
  of the insert in the log's order (5.1: every message deleted later is
  still present, every message deleted earlier is a tombstone, which is
  what upstream's ingest saw when it threaded the message) with the same
  subject thread-name hash whose kept reference hashes intersect the
  new message's. Under that order each kept hash names at most one
  present message, so an item joins or retires at most `K − 1` threads
  (5.1, R20).
- No candidate: the message is a new thread whose id is its own document
  id.
- One thread among the candidates: the message joins it.
- Several: they merge into the one with the most matching candidates
  (upstream's `ThreadMerge` counts one entry per matched row and the
  survivor is the thread with the most, ties to the lowest thread id,
  `crates/email/src/message/ingest.rs:1449-1451, 1487-1512`; not the
  most members, H10), the new message joins it, and each losing thread
  gets one alias to the survivor under the pass's revision (4.1, R21);
  no member's record is touched, so a merge costs the same few bytes
  whether the losing thread holds one message or a thousand, and an
  insert can retire at most `K − 1` threads, one per reference.

Upstream reads at most the 256 most recent rows of a subject bucket
(`THREAD_SCAN_WINDOW`, `ingest.rs:1430`) and merges across the whole
bucket in `MergeThreads`; the index has every member in memory and looks
them all up, a superset that yields the same grouping on upstream's own
test corpora (section 9's `thread_merge` variant pins it), except for
the `K` bound of 4.2 (7.5).

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
`crates/store/src/write/log.rs:299-314`). For a key account, a keyed
session **maps** on the way out and **checks** on the way in, from the
thread map as loaded (5.2) and validated (below):

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
- **The validated snapshot** (R2, R18, F2). A response that carries both
  a state and mapped ids, or that depends on which revision the map is
  at, takes its state and its map together: it reads the state from the
  message cache, or, for a Thread method, from the store's change
  counter (`Thread/get` reads `get_state(SyncCollection::Thread)`,
  `crates/jmap/src/thread/get.rs:80-85`: the Email group's counter,
  which Thread shares, `crates/types/src/collection.rs:230`; I5); then
  reads the manifest (one key, the same read as 5.1
  step 0); if the manifest's hash differs from the loaded index's, it
  reloads (incrementally, 4.3); then it requires the cache's
  `last_change_id`, the Email group's counter, which every publication's
  entries and every boundary's advance (G3; the item state alone does
  not move on a container-only record, `crates/email/src/cache/mod.rs:449-483`,
  so comparing it would refuse an empty account for ever), to be at or
  past the change id of the manifest's newest revision row and at or
  past the epoch, and if it is not, the cache is behind a publication
  (another node's, within the cache's 5 s revalidation): it marks the
  account's Email group stale, re-reads the cache, and repeats; after
  three rounds without agreement, or on a transient error of the
  manifest read, it fails as "no map, no ids" below. The state the
  method then returns, Email, Thread or Mailbox, reflects the
  publication because each publishing batch and each boundary batch
  touches that method's log (6.3), and a Thread method reads the state
  it returns from the store after the manifest, holding it to the same
  bound and re-reading it when it falls short (I5: read before the
  manifest, it could sit below a publication the map already holds, and
  the client's next `Thread/changes` would render the transitions once
  more as updates). Only then does it map, and the epoch
  checks of 6.3 use this loaded, validated epoch. Since a publication's
  log entry and its revision row commit in
  one batch, and a rebuild's boundary entry and its epoch do too, a
  state that reflects a publication or a rebuild is only ever returned
  with a map of that history that holds it, and a mapped id is never
  returned under a state below its publication: the two conditions
  together are what make upstream's own invariant, ids and state from
  one snapshot, hold here. Equality of the hash, not ordering of any
  counter, is what makes this hold across a rebuild or a deletion-only
  pass. The pass's own reload at step 0 is not enough on its own: the
  message cache can refresh between the pass and the handler's state
  read.
- Every Email id a client supplies to a keyed method is translated
  before upstream's lookup: 1.0.3 resolves a supplied id with
  `cache.email_by_jmap_id(id)` (`crates/email/src/cache/email.rs:620-624`,
  a lookup by document id filtered on the prefix) in `Email/get`
  (`get.rs:233, 261, 276`), `SearchSnippet/get` (`snippet.rs:183, 213`),
  `Email/copy` (`copy.rs:134, 242`, refused for key accounts anyway) and
  `Email/set` destroy (`set.rs:1620-1621`), and `Email/set` update
  filters `message_data(document_id)` on the prefix the same way
  (`set.rs:1173-1174`); each would reject every mapped id against the
  cache's singleton. The fork checks the supplied prefix against the map
  and hands the lookup the singleton form; a prefix that is not the
  message's current thread id answers `notFound`, as 1.0.2 answers for
  an id a thread merge destroyed. `SEARCH EMAILID` requires the prefix
  to match (`crates/imap/src/op/search.rs:565-574`) through the map, and
  `SEARCH THREADID` resolves the thread through the thread table. Two
  more supplied ids are compared as whole u64s against result ids and
  so are matched after mapping (H17): `Email/query`'s `anchor`
  (`crates/jmap/src/api/query.rs:45, 98`; a stale prefix is
  `anchorNotFound`) and `Email/queryChanges`'s `upToId`
  (`changes/query.rs:369`).
- Thread ids a client supplies (`Thread/get`, `inThread`,
  `EmailSubmission` `threadIds`) resolve through the thread table; an
  unknown one is `notFound`, as upstream.
- The thread counts of `Mailbox/get` (`totalThreads`, `unreadThreads`,
  `crates/jmap/src/mailbox/get.rs:185-189`), `collapseThreads`,
  `inThread`, the `*InThreadHaveKeyword` filters, the thread-keyword
  sorts and IMAP `THREAD` group by the map's thread id instead of the
  cache's (the cache's `thread_ids` column holds singletons and is left
  as it is). Which messages exist is always the message cache's answer,
  never the map's: a thread-valued read intersects the map with the
  cache's present set, so a message the cache no longer holds is never
  emitted whatever its record says (R14).
- **No map, no ids** (R3, F3). A keyed request whose thread part is not
  loaded and cannot be loaded or validated (a transient error reading
  the manifest or a blob, the budget cannot make room for the thread
  part, the validation above does not converge, a manifest of a newer
  format, the destroying marker) fails the methods above with
  `serverUnavailable` on JMAP and `NO [UNAVAILABLE]` on IMAP, and is
  traced; it never emits singleton ids under a state that has seen
  mapped ones, and never serves an older loaded map in place of a
  validated one. A rebuild (5.4) is the only path that resets
  identities, and it moves the epoch past every issued state.

**The method table** (F8). Mail spec 3.3 lets a key account's keyless
tokens (a Bearer or OAuth token, and the WebSocket and EventSource
sessions, which the HTTP layer strips of keys,
`crates/http/src/request.rs:383, 366`) run the methods that need no
content, all gated with `za_assert_mail_route` only
(`crates/jmap/src/api/request.rs:296-455, 546-556, 816`), and promises
the web client state-driven sync over the WebSocket. Every method that
emits or takes an Email or thread id, or whose answer depends on the
map, would show a keyless session singletons where a keyed one sees
mapped ids: two representations of one account's ids for one client.
Jay decided this on 2026-11-05 (section 13, decision 1), and this table
is the amended list, replacing mail spec 3.3's and 9's for key
accounts:

| Method | Gate for a key account | Why |
|---|---|---|
| `Email/get`, `Email/query`, `Email/changes`, `Email/queryChanges`, `Email/import`, `Email/parse`, `Email/copy` | keys (`za_assert_mail_keys`, `request.rs:865-874`) | emit or take thread-prefixed ids, or read content |
| `Email/set` create | keys | reads content; its response carries the mapped id |
| `Email/set` update and destroy | keyless, prefix unchecked | a flag or mailbox change needs no content; the fork looks the id up by document id alone beside 1.0.3's prefix checks (`set.rs:1173-1174`, `:1620-1621`), and the response echoes the supplied id; a stale prefix is not detected there (7.5) |
| `Thread/get`, `Thread/changes` | keys | emit thread ids and member ids |
| `Mailbox/get` | keys | `totalThreads` and `unreadThreads` come from the map, and must match the keyed view |
| `Mailbox/query`, `Mailbox/changes`, `Mailbox/set` | keyless | mailbox ids and states only |
| `EmailSubmission/get`, `/query`, `/changes` | keys | emit `emailId` and `threadId` |
| `EmailSubmission/set` | keys (already) | reads the message |
| `Blob/lookup` | keys | emits Email and Thread ids |
| `Blob/get`, `Blob/upload`, `Blob/copy`, `SearchSnippet/get` | keys (already) | content |
| `Identity/*`, `VacationResponse/*`, `PushSubscription/*`, `Core/echo` | keyless | nothing of mail |
| EventSource, WebSocket push | keyless | `StateChange` carries change ids and types, no Email ids, and a key account's content push is already none |

A keyless session therefore keeps push, mailbox listing and
management, flag and mailbox updates, identities and vacation, and
nothing that carries an Email or thread id or depends on the map.
Section 10's amendment of mail spec 3.3 says the same.

### 6.3 Changes

Upstream renders a thread merge as the old id destroyed and the new id
created (`merge_index`, `crates/common/src/storage/index/build.rs:414-443`),
and 1.0.2's `Email/queryChanges` with `collapseThreads` re-emits every
message of a thread that a created, updated or destroyed id touched. A
key account's keyless log never shows a merge (the singleton never
changes), so the keyed session renders it from the sealed thread history
and the revision table.

**What a publishing pass writes keylessly** (R4, R6, R13), in the batch
of 5.1 step 3, beside the manifest and the links. A publishing pass is
one that indexed at least one message (5.1 step 2). Its **witness** `W`
is the lowest document id among the messages it indexed: a choice the
operator can make from the log's inserts, or from the cursor's order,
without the mail, and that never depends on what any message's threading
turned out to be. It writes:

- one Email item update (`log_item_update`,
  `crates/store/src/write/batch.rs:379-474`) of `W`'s singleton id. An
  item update is what the store can hold (`log_item_insert` folds a
  delete and an insert of one id in one batch into an update,
  `crates/store/src/write/log.rs:369-382`, and `Changes::push_change`
  discards an insert a later record deletes,
  `crates/store/src/query/log.rs:347-395`); it bumps the Email state,
  gives every client a reason to ask, and allocates the change id the
  revision row is filled in with (R33);
- one Thread container property change of that same singleton
  (`log_container_property_change`), which bumps the Thread state;
- one container **property** change of every mailbox of the account
  (`log_container_property_change` of `SyncCollection::Email` with the
  mailbox id, the entry upstream itself writes for a count change,
  `crates/common/src/storage/index/build.rs:434-443`; G7), which bumps
  the Mailbox state without a re-read of the mailbox archives
  (`crates/email/src/cache/mod.rs:476-479`) and lets `Mailbox/changes`
  report the four counts as `updatedProperties`
  (`crates/jmap/src/changes/page.rs:116-125`), so clients refetch
  counts, not every property; every mailbox, so that the set says
  nothing about where the reassigned messages are.

Nothing names the reassigned messages, and nothing says whether any
message was reassigned: a pass that indexed a reply and a pass that
indexed a new thread write the same entries. The renderer does not need
the log to tell it which documents changed thread, because the history
does. A pass that indexed nothing writes nothing: a deletion needs no
carrier, since the raw delete entry the deletion itself logged is what
the renderer keys on. What the operator sees is section 2's list. A
reassigned message's `MessageData` is not rewritten, so its modseq and
`change_id` stay; IMAP `CHANGEDSINCE` therefore does not surface its new
`EMAILID` or `THREADID` (7.5): the alternative writes every member,
which is the cohort the guarantee withholds. The witness's entry has an
IMAP side too (F15): an IMAP session with `W`'s mailbox selected reports
it in an unsolicited `FETCH (FLAGS ...)` with an unchanged MODSEQ after
every publishing pass (`sync_view` takes the changed ids from the log,
`crates/imap/src/core/view.rs:813-838`, and `View::sync` reports every
present changed uid, `:645-647`), and the mailbox's HIGHESTMODSEQ
advances; recorded in 7.5.

**The revision of a state.** For a change id `X`, `R(X)` is the newest
revision whose published change id is at or below `X` (the revision
table, 4.1); a revision's transitions were published after `X` exactly
when its row's change id is above `X`.

**The thread of `D` as of `X`** (R5, R21): if `D`'s assignment `(T0,
r0)` has `r0 > R(X)`, or `D` has no record, its thread is the singleton
`D`, which is what any client that fetched it then received; otherwise
start at `T0` and follow the alias table, `(L → W, r)` with `r <= R(X)`,
from `T0` until no alias applies; the thread reached is `D`'s thread as
of `X`. For a deleted `D`, the same rule over its retained record. The
current thread is the rule at the current revision.

**The members of `T` as of `X`** (F14, R25): every `D`, present or a
retained tombstone, that exists at `X` (the rule below) and whose
thread as of `X` is `T`. It is defined for any `X` the log retains,
including the pinned end of an intermediate page, directly from the
records and the aliases, never from the present thread table, so a
member deleted or merged away after `X` is still counted at `X`.

**The stream of a range** `(S, E]`, for a since-state `S` and an upper
bound `E` (R12, R14, F10): the raw records of `(S, E]` are read
**unfolded**, with the fork's record reader (`za_changes_records`,
5.1), not through `Store::changes`, whose folding cancels an insert
followed by a delete (`push_change`, `crates/store/src/query/log.rs:347-395`)
and would hide a lifecycle the sealed records then resurrect. The
candidates are the documents with any raw record in `(S, E]` **and**
those whose thread as of `S` differs from their thread as of `E` (an
assignment or an alias on their chain with a revision whose change id
is in `(S, E]`), or with a deletion change id in `(S, E]`; the union is
taken on document ids. What each candidate yields is decided by its **existence** at the
two ends, not by which entry made it a candidate, and existence is a
physical fact taken from the raw records first and from the sealed
record only for what the range cannot show (R14): any insert record of
`D` in `(S, E]` makes `D` absent at `S`; any delete record of `D` in
`(S, E]` makes `D` absent at `E`, whatever `D`'s record says, since the
record can lag the log by the unconsumed part of a backlog; where the
range shows neither, existence at `E` is decided before the record is
consulted (H2: a deletion committed between a pass's log read and its
commit leaves a present record for a message a client may already have
seen deleted): when `E` is the current state, `D` exists at `E` exactly
when the message cache's present set holds it; when `E` is a pinned
past state, `D` exists at `E` when the cache holds it or the unfolded
log shows a delete of `D` after `E`; and in either case a `D` the cache
lacks with no delete record anywhere in `(S, now]` was deleted at or
before `S`, absent at both ends (I3: with `E` current, a deletion
logged at or before `S` and not yet consumed by a pass leaves a record
without a deletion change id, and consulting it would emit a second
`destroyed` of an id the client already destroyed, harmless under RFC
8620 but not upstream's stream). Only after that, and only for existence at `S`, a
document the index holds (present or as a retained tombstone) exists at
`X` when its creation change id is at or below `X` and its deletion
change id, if any, is above `X`, and a
document the index does not hold (never indexed, or not yet reached in
a backlog) exists at `X` as its own raw records in the range say, with
the singleton as its thread at both ends. The thread lookup stays the
history's, separate from existence, so a lagging record still gives the
old mapped id to destroy. Then, with `T0` the thread as of `S` and `T1`
the thread as of `E` (the rule above): absent at `S` and at `E`:
nothing, however many entries it had in between (a message created and
deleted within the range, R14); absent at `S`, present at `E`: created
`T1|D`; present at `S`, absent at `E`: destroyed `T0|D`; present at
both and `T0 ≠ T1`: destroyed `T0|D`, created `T1|D`; present at both,
`T0 = T1`, and a raw update in the range: updated `T1|D`; otherwise
nothing. The stream is ordered by the change id that put each document
in it (its raw entry's, its revision's, or its deletion's; the earliest
when there are several), then by document id, and so is deterministic
for a fixed `(S, E]` as long as the history is retained, which 5.5
guarantees for every state the log retains and the refusals below
enforce for every `S > 0`. For `S = 0`, `Initial` and its
continuations, the rule reduces to a stream that needs no retained
record (R31): no document existed at 0, so every document present at
`E` is a candidate, rendered created `T1|D`, and nothing else is (a
deleted message is absent at both ends); the candidates are the cache's
present set corrected by the raw records after `E`, as the existence
rule says, and the order is by document id alone, since no entry's
change id is needed. A truncation between two pages of an `Initial`
stream therefore renders the same remaining pages, where the general
rule would have dropped a singleton whose only evidence was a raw
record the truncation removed (its thread is the same id at both ends,
so no transition names it) and let the page skip a survivor instead
(R31). This is what an ordinary account's folded log would show,
except that upstream's `Initial` after a truncation omits the messages
whose inserts it lost (7.5).

**Thread membership for `Thread/changes`** (F14, R25): a thread's
members at `S` and at `E` are "the members of `T` as of `X`" above,
computed at each pinned end from the records and the aliases, never by
reversing the range from the present thread table (which can have lost
a member to a deletion or a merge after `E`). For `S = 0` every thread
with a member at `E` renders created, ordered by its lowest member's
document id (R31). Upstream's
`Thread/changes` filters its thread ids through the cache's singleton
column (`crates/jmap/src/changes/get.rs:465-475`); the fork's page
bypasses that filter for a key account, since every thread id it emits
is the map's.

**Pages** (R12). Upstream paginates `/changes` with
`State::Intermediate{from_id, to_id, items_sent}`
(`crates/jmap-proto/src/types/state.rs:18-30`) and, on a continuation,
re-reads the raw range, counts its raw entries and compares with
`items_sent` (`crates/jmap/src/changes/get.rs:393-419`,
`changes/page.rs:230-253`); a synthetic stream has more items than raw
entries, so the fork renders a key account's `Email/changes` and
`Thread/changes` pages itself, inside `ChangesLookup::changes`
(`changes/get.rs:148-608`), keeping upstream's state shapes:

- for `Initial` (G13, R28, R31): the stream of `(0, E]`, `Exact(0)`,
  with the continuation `Intermediate{from: 0, to: E, sent}`; no
  message existed at change id 0, so a client at `Initial` or at
  `Exact(0)` holds nothing, and every document present at `E` renders
  as created, in document-id order, from the cache and the records
  after `E` alone (the `S = 0` form of the stream rule above). The lower
  bound 0 is never refused by the truncation check (upstream reads
  `Query::All` from 0 without it, `changes/get.rs:314-335`) or by the
  epoch (5.4), so a continuation is always admissible and needs no mode
  of its own: the first page and every later one render the same stream
  by the same rule, whatever the log lost in between (R28's two forms: a
  shared `Intermediate` shape and a continuation that fell below the
  retained bound or the epoch; R31: a client on a continuation holds the
  earlier pages, so the stream it continues must not depend on what
  retention removes; upstream's own `Initial` continuation, whose `from`
  is the first retained id, `changes/get.rs:593-597`, refuses after a
  truncation instead). Upstream's `Query::All` preserves each record's
  kind, so a pre-existing message with a retained update renders there
  as updated and here as created (7.5, H19), and it omits a message
  whose insert the log lost, which renders here (7.5); to a client that
  holds nothing these are the same instruction;
- for `Exact(S)`: `E` is the validated current state (6.2); if `S == E`
  the response is empty, as upstream's equality return; otherwise the
  stream of `(S, E]` is computed, the first `maxChanges` items are
  returned, and the new state is `Exact(E)` when the stream is
  exhausted, else `Intermediate{from: S, to: E, items_sent: n}` with
  `hasMoreChanges`;
- for `Intermediate{from, to, sent}`: the stream of `(from, to]` is
  recomputed, which gives the same items in the same order because the
  bound is pinned and the history retained, the first `sent` are
  skipped, and the page continues; when that stream is exhausted the
  call continues with `Exact(to)` as upstream continues with
  `Since(to)`;
- `cannotCalculateChanges` when `S`, or an intermediate `from`, is below
  the epoch of the loaded, validated map and the method's log holds a
  record of the method's kind in `(S, epoch]` (5.4, R27, H1), or when
  `S` is below what the retained log holds, the shorter of the Email and
  Thread logs (G4: a key account's `Thread/changes` is computed from
  the Email log's records, so a truncation of either refuses; the
  unfolded reader reports the marker as upstream's does,
  `query/log.rs:149-151`), as upstream answers for a truncated log
  (`changes/get.rs:424-433`); never for `from = 0`, which is `Initial`
  and its continuations (R28, R31: a client at `Initial` holds nothing
  and a client on its continuation holds pages of a stream that no
  retained record decides, so neither refusal protects anything, and a
  continuation that could be refused would loop the client between a
  first page that succeeds and a second
  that fails);
- `Thread/changes` renders the threads touched, from the same union and
  the membership rule above: a thread that had no member as of `S` is
  created, one that has none as of `E` is destroyed, else updated,
  ordered and paged the same way. The Thread log itself is read only
  for its state, its truncation bound and the epoch check (H18): its
  records are the stub ingest's singleton container inserts,
  `log_threads_emptied`'s deletes and the witness's property change,
  none of which names a mapped thread. `E` for `Thread/changes`, and
  `Thread/get`'s `state`, is the Thread log's change id, which the
  pass's property change advances and which lags the Email state by
  flag-only records;
- `Email/queryChanges` consumes the same page through its `changes` call
  (`changes/query.rs:65-74`) and diffs it against the mapped query as
  upstream does (`:348-378`); its `collapseThreads` handling
  (`:105-137`) takes the prefixes of the page's created, updated and
  destroyed ids, mapped, and selects the messages it re-emits as updated
  by the map's thread id as of `E` instead of the cache's (`:129, :133`).

**Pruning.** An alias, a deleted message's record and a revision-table
row are kept until their change id is older than the oldest retained
change id of the shorter of the Email and Thread logs (G4), then
dropped at thread compaction (5.5), an alias by folding first: a client
asking since an older state gets `cannotCalculateChanges` from the log
itself, on either method. The fork's `Thread/changes` refuses below the
Email log's bound as well as the Thread log's, since its rendering
needs both.

Push: a key account's content push is already none
(`build_email_push_object`, `crates/services/src/state_manager/email_push.rs:64-70`),
`StateChange` carries change ids, not thread ids, and 1.0.2's
`EmailPush` on delivery, APPEND and `Email/import` reaches a key
account's subscribers, WebSocket and EventSource only as a `StateChange`
(`crates/services/src/state_manager/za.rs:11-17`, `push.rs:366-391`;
`crates/common/src/ipc.rs:439-450`); nothing is added.

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
  `crates/jmap/src/email/query/window.rs:242-354`, over the `TextSource`
  hooks at `:31-90`; `supports_windowed_walk`,
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
(`changes/query.rs:74-82`, plan 13 R38) for the same reason.

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
changes but the id translation of 6.2 on the supplied `emailIds`, which
1.0.3 prefix-checks with `email_by_jmap_id` (`snippet.rs:183, 213`).
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

- the content part cannot be loaded: a rebuild is in progress (5.4), a
  transient error (5.1's class), the account exceeds
  `ZA_SEARCH_ACCOUNT_MAX_MB`, the budget cannot make room, or the
  manifest is of a newer format (F7);
- the index is not current after `ZA_SEARCH_WAIT_SECS` of passes (a
  rebuild or a reconciliation in progress, or a very long absence),
  where **current** means, against the same `E` the response returns
  (6.2): `scanned_to >= E` with no insert or delete record in
  `(indexed_to, scanned_to]`, and `content_to` at `indexed_to` (G14);
- a leaf cannot be resolved (a defect, traced as such).

A content request is never answered from an index that is behind the
request's view of the account, and never by decrypting and scanning
(mail spec 14's rejection stands). Partial answers are not given: an
IMAP client cannot tell a partial SEARCH from a complete one. The
original text, `ZA_MAIL_SEARCH_REFUSED`, is kept for the keyless case
(a session without keys), where the server still cannot search.

The thread-valued reads of 6.2 are never refused by a current or
lagging index, and never wait for a pass's I/O (4.3, F5): they answer
from the map as loaded after the request's pass, or after the running
pass they did not wait for, and validated (5.2, 6.2), which in normal
use is current, and during a rebuild or a backlog is the map so far,
with later publications rendered by 6.3. They fail only when no map can
be loaded or validated (6.2, "no map, no ids"). `Email/get`'s content,
flag updates, mailbox listing, FETCH of content and every keyed write
never wait for the content part.

### 7.5 Parity

The contract is the one release 5 made for scheduling (its section
3.1): what a key account's client observes from a search, a sort, a
snippet, a thread or a changes call is what an ordinary account's client
observes for the same mail and the same requests, in the same order,
apart from the recorded differences:

- freshness (section 2: indexing waits for a keyed request), and the
  one-response bound between nodes (5.6);
- the unavailable refusal of 7.4 and the no-map failure of 6.2, which an
  ordinary account never gives;
- the keyless refusals of mail spec 26 and the method table of 6.2, and
  the unchecked prefix on a keyless `Email/set` update;
- the thread window superset of 6.1, and the `K` bound of 4.2: a
  message whose `K` nearest referenced ancestors are all absent while
  an older one is present threads as a singleton where upstream joins
  it;
- a rebuild invalidates every sync state issued before it (5.4), where
  upstream's index loss changes nothing a client sees;
- IMAP `CHANGEDSINCE` does not surface a reassigned message's new
  `EMAILID` or `THREADID`, and the message's modseq does not move on a
  reassignment (6.3); `Mailbox/changes` reports every mailbox after a
  publishing pass, not only the affected ones;
- the witness entry of every publishing pass: an `Email/changes` update,
  an unsolicited IMAP `FETCH (FLAGS ...)` of the witness with an
  unchanged MODSEQ in sessions that have its mailbox selected, and a
  HIGHESTMODSEQ that advances with no flag changed (6.3, F15), none of
  which an ordinary account shows;
- a message deleted before any keyed request indexed it, or before a
  rebuild's cursor reached it, gives no threading evidence to a later
  message that referenced only it (5.8), where upstream's ingest
  threaded with it present;
- a rebuild assigns thread ids in the cursor's order, chronological by
  document id, which can differ from the log's original order where
  concurrent commits inverted it and in which thread survives a merge,
  so the id that names a thread can differ from before the loss; JMAP
  clients resync (5.4), but IMAP has no sync for object ids,
  so every threaded message's `EMAILID` and `THREADID` can change across
  a rebuild with no `CHANGEDSINCE` signal (G16);
- a keyless `Email/set` update or destroy accepts a stale prefix (6.2's
  table), where 1.0.2 answers `notFound` (G20);
- `EmailSubmission/get` returns a submission's `threadId` as the map has
  it now, where upstream returns the prefix stored at submission time,
  stale after a merge (G22);
- `Initial` renders every present message as created, where upstream's
  `Query::All` renders a pre-existing message with a retained update as
  updated (6.3, H19) and omits a message whose insert the log has lost
  (6.3, R31); to a client that holds nothing the two are the same
  instruction, and the omission is upstream's gap, not this design's;
- after a truncation, a reconciliation's gap insert that referenced a
  message the gap also deleted threads with it, where upstream did so
  only when the insert came first (5.4, H3);
- a pass's duration, and where its time cap stops it, reveal the
  processing cost of the messages it admitted (section 2, R30, H4);
- what 6.4 keeps off at delivery.

Parity is by construction where it matters: the same `index_document`,
the same `MessageSortKeys`, the same `ThreadFields::scan` (bounded), the
same `Leaf` plan semantics, upstream's own `SearchQuery::filter` merge
and `query_emails` ranking, and a changes stream built to upstream's own
rendering rules. Section 9.5's differential test holds it.

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
| `ZA_SEARCH_PASS` | a pass committed | account id, inserts, tombstones, whether it published, revision, `indexed_to` |
| `ZA_SEARCH_SKIPPED` | a message was skipped in a pass for loss (5.1) | account id, document id, the loss class (stub absent, blob absent, would not open) |
| `ZA_SEARCH_ABORTED` | a pass aborted on a transient error (5.1) | account id, the error class |
| `ZA_SEARCH_LOST` | a manifest write lost its assertion | account id |
| `ZA_SEARCH_REBUILD` | a rebuild, content-only rebuild or reconciliation started or finished | account id, which, the loss class, the cursor size, the epoch |
| `ZA_SEARCH_SUSPENDED` | content indexing stopped above the account cap | account id, the chain's size |
| `ZA_SEARCH_UNAVAILABLE` | a content request was refused by 7.4, or a thread-valued read failed for want of a validated map (6.2) | account id, the reason class |
| `ZA_SEARCH_EVICTED` | the cache evicted an index or its content part | account id, the reason (idle, budget, invalidation) |
| `ZA_SEARCH_UNRESOLVED` | a leaf could not be resolved (a defect) | account id, the leaf's kind as an enum (text, key-value, integer), never its field name, which for a header leaf is the client's string (G9) |

A failure inside the pass never fails the request that triggered it,
except as 5.2 and 6.2 say: a content request refuses by 7.4, and a
thread-valued read with no loadable or validated map fails. A flag
update whose pass failed is served, with the trace. Loss at load is
`ZA_SEARCH_REBUILD`, not an error to the client, and the rebuild moves
the epoch; a transient error at load is `ZA_SEARCH_UNAVAILABLE` and
nothing else. A transient error in the pass aborts it with nothing
written; the request is then served only if it emits no thread-valued
data, and otherwise fails (6.2): singleton ids are never substituted,
and an older loaded map is never served in place of a validated one.

## 9. Testing

### 9.1 Unit

In the crate that holds the index model: the blob codecs round-trip a
populated index, and the thread part loads without the content part; a
blob sealed for one account, segment id, kind or predecessor does not
open under another; a content chain reloads from its head with nothing
but the manifest's head descriptor (R11); an incremental reload of
appended thread blobs and chain nodes, and a whole reload after a
compaction, each end as a fresh load (F20); the thread rules reproduce
`find_thread_id` and `ThreadMerge::merge` on a fixture of references
and subjects, merges included, and the `K` bound keeps the nearest
references (4.2); the "thread of `D` as of `X`" rule, the membership
rule (F14) and the stream of 6.3 over a history and a revision table
give upstream's destroyed and created pairs, for states before, at,
between and after two successive publications, for a first assignment
from the singleton, for a deleted message, for a message created and
deleted within the range, singleton and threaded, which yields nothing
(R14), for an insert followed by a delete that the folded reader would
cancel and the unfolded one shows (F10), and for a pinned range whose
end is older than the current revision (R2, R3, R5, R12); a deletion the
record lacks but the range shows renders as destroyed, exactly once
(R14); the manifest-hash comparison reloads on a deletion-only change
with an equal identity version (R18), and the identity version rejects
a map of another epoch with an equal or lower revision (R2); the state
check of the validated snapshot rejects a counter below the newest
revision row, and passes an empty account whose item state never moved
(F2, G3); the padding formula is checked at framed lengths just below,
at and above each class boundary, payloads of `M − 5` and `M − 4`
bytes and `2M` included, the hidden section of a maximal pass (`H`
plus one grouped record of `G` messages each with `K − 1` aliases)
stays under `M`, and a pass with any number of tombstones, aliases or
merges of any size pads its hidden section to `M` exactly (F1, R15,
R20, R21, G1); a compaction over fixtures of equal record and tombstone
counts with no merge, with every message in one thread, with a chain of
merges through one surviving message, and with bridging messages
deleted between a pass's open and its commit (hook
`SEARCH_COMMIT_PAUSE`) pads to the same class, and the race-deleted
records stay until their publication's row is pruned (decision 2 (c),
I2); R35's timelines, three roots deleted before the catch-up pass that
merges them and 128 families of fifteen roots with live bridges,
compacted at a bound of the bridges' publication: the dead aliases
drop, the records fit the public-count class, and every retained
record resolves as before the compaction at every servable state; a
chain of three merges with the middle roots pruned keeps the aliases
the surviving records' chains use, and a folded sub-bound alias moves
an assignment one hop so the record still renders its pre-merge thread
at a state before the next alias (R35); the configuration refuses `M`
below the inequality
(R20); the `K` capture keeps In-Reply-To, then the last References
first, before the sort (G10); the alias chain resolves "the thread of
`D` as of `X`" and "the members of `T` as of `X`" across two merges and
a deletion after `X` (R25); folding an alias at compaction leaves
every resolution unchanged (5.5); a log fixture with an insert followed by a
deletion of the message it references, consumed in order, gives the
control's grouping, and the same fixture consumed with the deletion
first does not (R16); a cursor fixture with newer messages referencing an older pending
item threads the control's way under the chronological order and fans
in under a newest-first order (R16, R20); a fixture of `K` references
each naming a present message in a distinct thread writes exactly
`K − 1` aliases, and none can write more (R20); two fixtures equal in
public inputs, with a probe whose references share a thread in one and
not the other, stop at the same checkpoint (R21); a record of more
inserts than the cap is left part-way with the checkpoint naming the
record and the last document id (R20, R1); `Initial` paged with
`maxChanges = 1` over pre-existing updated messages equals its unpaged
result (R28); the epoch rule serves a state below the epoch when the
method's log holds no record in `(S, epoch]` and refuses one when it
does, the boundary's own entries included, and never refuses
`from = 0` (R27, H1); a partial record resumed inclusively indexes its
suffix and applies its deletions, and a marker in its place is loss
(R29); a deletion committed between a pass's log read and its commit
renders nothing for the deleted message at a state that already saw
it (H2); a reconciliation fixture with a gap insert referencing a
gap-deleted message threads them, one whose deletion is in the
retained log threads as the control does (H3), and one whose insert is
retained and whose referenced message the gap deleted stays a singleton
as the control does (I1); the survivor of a merge
is the thread with the most matching candidates, ties lowest (H10); a
manifest with a bad tag is loss, and a published and a rebuilt manifest
each reload with a verified tag, computed at commit over the filled-in
change id, while an altered field fails it (H9, R33); a deletion logged
but not yet consumed, then a publication, then `/changes` from the
state that already saw the deletion emits nothing for it (I3); a
validated snapshot that reloads between a pass's commit and its swap
(hook `SEARCH_SWAP_PAUSE`) leaves the index at the newer hash, and
nothing a compaction folded comes back (I4); a `Thread/get` whose state
read races another node's publication returns a state at or past the
row (I5); upstream's per-account reindex task on a key account writes
the marker and the next keyed request rebuilds, after a blob store
that answered "not found" for every key had skipped the backlog (I6);
transient failures of one document across an eviction and a restart
reach `ZA_SEARCH_SKIP_AFTER` through the checkpointed count (R34); a logged
deletion is applied whatever a presence snapshot says (R17); the error
classes map a store timeout to abort and an absent stub to skip (F3);
paging the stream with a small `maxChanges` reproduces every item
exactly once, with a publication and a deletion landing between pages
(R12); the record reader yields records whole with their change ids and
reports truncation; the witness is a function of the indexed set's ids
alone (R4); the phrase check agrees with `matches_phrase` on positioned
fields and reduces to co-occurrence on `Language::None` fields
(`term.rs:728`); the manifest stays a few kilobytes at
`ZA_SEARCH_SEGMENTS` thread blobs whatever the chain's length; the
environment variables parse and fall back, and `M` refuses a value that
`C · R` leaves no margin under. In `crates/vault`: `swk` derivation is
stable across a rewrap.

### 9.2 The za suite

`tests/src/za/mail/search.rs` turns around: its refused tables become
served tables with expected results, keeping the keyless cases refused
(its `nested_search_refused`, which pins 1.0.3's nesting limit, stays).
New modules, in the mail block after `search` and before `traces`
(`tests/src/za/mod.rs:216-225`; `mail::gateway` closes the block at
`:225`), with the next free names `keymail46` and `plainmail50` onwards
(the merge's `gateway.rs` took `keymail36`-`39` and `plainmail41`-`44`,
and `fence.rs` and `index_task.rs` reach `keymail45` and `plainmail49`):

- `mail::threads`: the same conversation delivered to a key account and
  a plain control gives the same `Thread/get` groupings, `Email/query`
  with `collapseThreads`, IMAP `THREAD`; a reply delivered while no keyed
  request runs is a singleton until the next keyed request, then joins;
  a merge of two existing threads renders in `Email/changes` and
  `Thread/changes` as the plain control's does, and in
  `Email/queryChanges` with `collapseThreads` as 1.0.2 renders it; two
  successive merges, with changes requested from states before, at,
  between and after the two publications (R5); the backlog case (R2): a
  capped pass, a fetch of an unindexed message and its state kept, the
  catch-up completed, `Email/changes` applied, and the client's id set
  equal to `Email/query`'s; the cross-node case (R2): a pause between
  the pass and the handler's snapshot (hook `SEARCH_SNAPSHOT_PAUSE`), a
  competing publication written to the store in the gap, resume, and the
  ids returned under the returned state reconcile with `Email/changes`
  since it; the stale-state case (F2): the same pause with the message
  cache held at a state below the competing publication (hook
  `SEARCH_CACHE_HOLD`), and the response either re-reads the cache or
  fails, never returning a mapped id under the stale state, with
  `Email/queryChanges` never emitting an `added` for an id the client
  holds; a deleted merged message, a thread compaction and a reload,
  then changes from a retained pre-delete state rendering the destroyed
  mapped id (R3); the publishing pass's keyless writes are exactly one
  item update of the lowest indexed id, one Thread property change and
  every mailbox's container update, in the bridge case with losing-thread
  members of lower ids than the bridge, where the witness is the bridge
  and the members are absent from the log (R4); a deletion-only pass
  writes no entry and renders the deletion from the retained record,
  including the account's last message (R13); pages of a large merge
  with a tiny `maxChanges`, with another merge and a deletion between
  pages, reconstruct the query's id set (R12); a message created, indexed
  and deleted within one range, singleton and threaded, renders as the
  plain control's folded log does, with pages and a publication between
  them (R14), and the same with the insert reached by a pass and the
  deletion behind the cap (F10); the backlog deletion (R14): an indexed
  message `D` whose mapped id a client holds, more inserts than the pass
  cap, then `D`'s deletion, changes requested and paged before the
  catch-up finishes, then the catch-up finished, and the client's
  accumulated set equal to a full query with `T|D` destroyed exactly
  once; the ordering case (R16): indexed `A`, indexed `D` replying to
  `A`, then `N` delivered with only `In-Reply-To: D`, then `D` deleted
  through POP3 `QUIT` (which runs no pass) before the next keyed
  request, the catch-up completed, and `A` and `N` grouped as the plain
  control groups them with `D` absent, repeated with the insert cap
  falling between `N`'s insert and `D`'s deletion; the rebuild form
  (R16, R20): the chain `A`, `D`, `N` indexed, the index lost, a capped
  rebuild under way with `A` and `D` done and `N` pending, `D` deleted
  through POP3 and a new `N2` replying to `D` delivered mid-rebuild,
  the cursor finished and then the log consumed, with `A`, `N` and `N2`
  grouped as the control groups them, `D` absent from every read, and
  no alias count above `K − 1` in any pass; and its residual, `D`
  deleted while still pending in the cursor, where `N2` threads as a
  singleton and the control does not (the accepted missing-evidence
  case of 7.5, pinned rather than expected to match); the checkpoint case (R21):
  two accounts with identical public inputs, thirty-seven fillers and a
  probe whose two references share a thread in one account and not the
  other, with equal `indexed_to`, cursor bound, publication sequence
  and blob sizes after the pass; an unreadable logged insert on an
  otherwise idle account (R26): checkpointed once, never reopened, and
  the account's other mail searchable through repeated content
  requests, an eviction and a restart; a loss after the last message
  was deleted (R27): a fresh Email and a fresh Thread resync each make
  an empty `/changes` round trip without new mail while pre-loss states
  are refused; `Initial` and `Exact(0)` paged with a tiny
  `maxChanges` over pre-existing updated messages, each equal to its own
  unpaged result, after a reload between pages too (R28), and after a
  truncation between pages (the log's history limit lowered under the
  page, or a direct `delete_changes`) the remaining pages are the same
  and no message is skipped, where the control refuses (R31); the
  residual pinned beside them: `N` and `D` both arriving and `D` deleted
  before any keyed request, where `N` stays a singleton and the control
  does not (5.8); the stale-snapshot case (R17): `D`'s deletion
  committed between the pass's cache comparison and its log iteration
  (hook `SEARCH_SWEEP_PAUSE`), the tombstone persisted, and after
  retention expiry and compaction `D`'s record and content gone; a
  message with more than `K` references threads through its nearest
  ones, and the `K` deviation of 7.5 pinned (F1); `Mailbox/changes`
  reports every mailbox after a publishing pass, IMAP `CHANGEDSINCE`
  does not report the reassigned message, and an IMAP session with the
  witness's mailbox selected gets the unsolicited `FETCH` with an
  unchanged MODSEQ (R6, F15, all pinned); a client-supplied id with the
  old prefix is `notFound` on `Email/get` and served on a keyless
  `Email/set` flag update and destroy (G20); a keyless token is refused
  on every gated method of 6.2's table and served on every keyless one,
  `Mailbox/get` refused among them (F8); `Mailbox/get` thread counts
  match the control, and `Mailbox/changes` after a publishing pass
  reports `updatedProperties` of the counts only (G7); a FETCH after a
  keyed APPEND carries the final `EMAILID` (H21: APPEND itself answers
  `APPENDUID`), as does `Email/set` create's response; the
  amplified-merge probe (R21): two
  threads filled with helpers referencing two known Message-IDs, then a
  bridge, with the thread blob's hidden class the same whether the two
  were one thread or two, and the alias table holding one entry; a
  reconciliation whose difference includes a deletion still in the
  retained log, with a client at a servable state above the marker,
  renders the destroyed mapped id exactly once (R24, G2); a pinned page
  of `Thread/changes` across the deletion of a thread's last member and
  a merge after the page's end keeps its items and classification
  (R25); `Thread/changes` from a state the Thread log retains but the
  Email log does not is refused (G4, with `maxChangesHistory` lowered
  and flags churned to separate the two bounds); an empty account
  provisioned on a warm cache with a trailing container-only record
  serves every `Email/*` method (G3); a loss on an account whose
  messages were all deleted while a client holds their mapped ids
  refuses that client's state (G3).
- `mail::index`: persistence (a second session serves a search without
  opening a message blob, by a new test-mode counter of opened message
  blobs); a thread-only load reads no content blob (a test-mode counter
  of blob reads, R9) and serves `Email/get` and `Thread/get`; a content
  chain of several blobs reloads after a thread compaction and an
  eviction and every node is searchable (R11); an incremental reload
  after another node's pass reads only the added blobs (F20); catch-up
  after offline deliveries; the pass caps crossed inside a grouped
  ingest's record with a restart between passes, every message indexed
  exactly once (R1); the wait budget (hook `SEARCH_PASS_PAUSE`), and a
  flag update and a `Mailbox/get` answered while a pass is paused on
  its blob I/O (F5); a hundred flag changes between two keyed requests
  write no blob and no manifest, and `scanned_to` keeps the shortcut
  cheap (F4); thread compaction at `ZA_SEARCH_SEGMENTS` by a node that
  holds no content, over more passes than the manifest could otherwise
  list, the manifest bounded and the chain growing (R8); content
  compaction leaves one content blob and the old blobs unlinked and
  purged; every link and manifest transition interrupted (hook
  `SEARCH_COMMIT_PAUSE` before and after the batch, `SEARCH_HOLD_EXPIRE`
  to age the holds), then a restart, a destroy and a purge, with no blob
  and no permanent link surviving (R7); a middle content blob deleted,
  then a rebuild, a destroy and a purge, with no older blob or link
  surviving (R7); a destroy with an unparseable manifest, and one of an
  account provisioned before the feature with no manifest, take the scan
  and leave nothing (F23); a keyed pass paused after staging while a
  destroy runs its marker and inventory, then resumed, losing its
  assertion, and the purge leaving nothing (R19); a rebuild from a
  deleted blob, a deleted manifest and an older format version moves the
  epoch past the latest issued state, which `Email/changes` then refuses
  with `cannotCalculateChanges`, a resynced client's new state works,
  and a second loss with no mail activity in between gives a second,
  distinct boundary (R3); a manifest of a newer format version gives
  "no map" and the unavailable refusal, and no rebuild (F7); a blob
  store that times out once (hook `SEARCH_BLOB_FAIL_ONCE`) aborts the
  pass with no index write, its attempt count checkpointed, and the
  next pass indexes the message, and
  the same at load gives "no map" once and no rebuild (F3); a node
  whose loaded map is of the previous epoch with an equal or higher
  revision than the rebuilt manifest's (hook `SEARCH_SNAPSHOT_PAUSE`
  held across a rebuild) reloads or fails and never serves the old map
  (R2); a deletion-only pass on one node with another node holding the
  old same-version map, then a message referencing the deleted one
  indexed on the old node, threads correctly with the tombstone
  persisted, after eviction and reload too (R18); a rebuild of several
  passes over an otherwise idle mailbox, with a restart between passes,
  finishes its cursor (R10); a failed manifest read at validation fails
  the thread-valued methods and never emits a singleton (R2, R3); a
  reconciliation after a truncated log (a lowered `maxChangesHistory`
  for the case), whose tombstones and indexed messages render nothing a
  client already saw (F9), and a truncation during a pending rebuild
  (F9); an account over the cap serves threads, refuses searches, stops
  extending the chain, and a raised cap, or a second node with a
  higher cap, takes a content-only rebuild that backfills the messages
  indexed while suspended, refusing content requests until `content_to`
  catches up, with thread identities unchanged (F6, R23); a cursor
  whose every remaining item has vanished finishes through a
  manifest-only checkpoint, after an eviction and a restart too, with
  no blob and no log entry (R22); ten thousand flag changes with no
  insert write manifest-only checkpoints and never force a
  reconciliation (G19); a destroy that dies after writing the marker
  retries from the marker's inventory and leaves nothing (G12); a
  compacted content blob staged to a slow store (hook `SEARCH_BLOB_SLOW`)
  outlives the wait budget and is never purged (G15); a rebuild of many
  passes merges content by tier while its cursor is pending, the bytes
  rewritten over the rebuild measured against `⌈log_S n⌉` times the
  content written and the chain's length against
  `(S − 1) · ⌈log_S n⌉ + 1`, and does the full merge once the cursor is
  exhausted (G17, H7, R32); a pass that opens attachment-heavy mail stops at the time cap
  and the triggering command returns within it (G18); a node serving
  many accounts at once keeps every thread part loaded while content
  parts are evicted, and two concurrent first searches of one account
  pay one load (G6); a lost manifest assertion (`SEARCH_COMMIT_PAUSE` with a
  competing write in the gap) discards the loser's staging and serves the
  winner's; eviction by idle, by budget and by invalidation, and the
  reload after each, with a load whose transient would exceed the
  budget evicting first (F18); a skipped message (hook
  `SEARCH_OPEN_FAIL_ONCE` as loss) is absent from results and traced
  with ids only; a message deleted between two passes is gone from
  results before compaction; the manifest is created at setup and no
  account without mail ever rebuilds; a repeated delta (the pass's own
  update entry, a replayed insert) changes nothing.
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
`tests/src/za/websocket.rs` and `mail::gates` gain the keyless cases of
6.2's table: a key account's WebSocket session and a Bearer token are
refused on each gated method, served on each keyless one, and still get
`StateChange` pushes.

### 9.4 Leak coverage

`leak::test_mail` gains the rules: every blob linked to the account's
principal document is a sealed index blob (format byte, kind, no
canary); field 152 holds no canary and parses as a manifest; no
`Threading` key, no term and an ids-only document in the internal search
store, as today; the change log of a publishing pass holds one item
update of the lowest indexed id, one Thread property change and every
mailbox's container update, and no entry for any other message (R4);
after the suite destroys its key accounts and purges, no blob and no
permanent link remains. The mail canaries (`VISIBLE_CANARIES`,
`tests/src/za/leak.rs:241`) are searched for in every blob and in the
manifest, a negative control being a plain account's blob-less state
plus a planted plaintext blob. The scan reads blobs through `get_blob`,
which undoes the codec (1.0.3's `leak::test_codec` covers the codec
itself, and a codec key rotation must recode the index's blobs as 4.2
says, pinned beside `index_task::test_blob_recode_keeps_sealed`);
`scripts/local-server/leakcheck.sh` still does not decrypt the codec and
counts encrypted values as failures, as before. The membership rule is a
review case, not a canary scan (R4, F1): two multi-message threads and
one bridge whose losing-thread members have lower ids, every keyless
write enumerated, and the writes, the manifest and the thread blob's
size class shown to be the same as for a bridge that merged nothing;
then the oracle attempts: a filler with a thousand references delivered
with a probe (F1), a filler of a thousand junk messages expunged by the
operator's schedule beside a probe (G1), and two helper cohorts around
two known Message-IDs with a bridge (R21), with the hidden section's
class `M` in every case and the public section's length exactly the
tombstone count's multiple; and a compaction after the mailbox is
padded to a class boundary, whose class must equal the public-count
formula's with and without the probe's merge (decision 2 (c); a pad by
content length must fail the equal-counts fixtures, the bridge rule
without the dead-alias rule must fail R35's fit, the dead-alias rule
without the bridge rule must fail the chain fixture's resolution, and a
fold to the end of the chain must fail the pre-merge rendering, R35).

### 9.5 Differential test

A corpus of a few hundred messages (threads, replies, forwards, attachments
with text, non-ASCII subjects, phrases, messages with more than `K`
references) delivered to a key account and a plain control; for every
IMAP SEARCH and SORT form of `search.rs`'s tables, every JMAP filter,
sort and `collapseThreads` combination of the `query` variant, and
`SearchSnippet/get` on each result, the key account's answer equals the
control's (ids compared through the thread map), except where 7.5's `K`
deviation is pinned. This is the parity contract of 7.5, run in the za
suite.

### 9.6 Benchmark

The plan's first task, recorded in its outcome note: a key account
holding 1 GB of generated mail; the content part's loaded size with and
without positions, with per-record compression against one frame (the
ratio the G5 rule costs), the thread part's size, the measured record
size against the derived bound `R` at `K = 16`, the load time of each,
the pass rate (messages per second with
and without attachment text), the compaction times, the cost of the
validation read per request on RocksDB and on a networked store, the
cost of the mailbox property changes per publishing pass, the
first-search latency after an absence of one day's volume, and a
design-size rebuild end to end (H7): at the defaults about 625 passes
of 48 items, each a padded thread blob, a content blob and a
publication, with the thread compactions and the chain's tiered
content compactions along the way (the bytes they rewrite measured
against `⌈log_S n⌉` times the content written, R32), the number of content requests that
refuse before the first succeeds, and the wall time under a client
that issues a keyed request every few seconds. The figures
decide the blob format's details and confirm or adjust the defaults of
4.4, `M` against `C · R` among them, before the rest of the plan is
executed.

### 9.7 Mutation sample

The plan runs the fork's mutation sample on: the AAD's segment id, kind
and predecessor (a blob of another account or position must not open),
the assertion on the manifest write (a lost write must not advance
`indexed_to` or the revision), the record boundary (a cap applied inside
a record must lose a message, caught by R1's case), the change id of
the revision row filled in at commit (a stale change id must break
R2's case; a tag computed before it must fail R33's reload), the
validation read and its state check (skipping either must break the
cross-node or the stale-state case), the manifest-hash comparison (a
version-pair comparison must fail R18's case), the history lookup's
direction (R5's case), the range end of the stream (a "now" in place of
`E` must break the paging case), the exhaustive leaf resolution (an
unresolved leaf must refuse, not widen), the prefix check on supplied
ids, the key gates of 6.2's table, the witness rule (a witness chosen
among the reassigned must fail the R4 log check), the publishing rule (a
pass publishing only on reassignment must fail the same check), the
retention of a deleted message's record (R3's case), the epoch's fresh
change id (an epoch equal to the current change id must let the latest
pre-loss state through), the cursor's precedence over the shortcut
(R10's case), the chain header's segment id (R11's case), the staging
swap after commit (a swap before commit must let a failed batch leave a
mapped id in memory, caught by the lost-assertion case), the links in
the manifest's batch (a link before the batch must survive an
interrupted pass in R7's case), the scan on an incomplete traversal
(R7's middle-node case), the existence rule (a created-and-deleted
document must not appear, R14's case), the unfolded overlay (the folded
reader must resurrect F10's case), the strict order (a deletion applied
before an earlier insert must fail R16's case), the cursor evidence (a
tombstone dropping its fields with a cursor pending must fail R16's
rebuild case), the unconditional tombstone (a skip on cache presence
must fail R17's case), the destroying marker (an inventory before the
marker must leave R19's link), the error classes (a timeout treated as
loss must rebuild in F3's case), the empty-staging rule (a blob written
for an update-only delta must fail F4's count; a cursor that moves
without a checkpoint must fail R22's case), the record bound, the
hidden budget and `M` (a record with unbounded references, tombstones
in the padded section, a merge that rewrites members, or `M` under the
inequality must each fail an oracle case of 9.4), the reservation
charge (charging actual alias bytes must fail R21's checkpoint case),
the cursor order (a newest-first cursor, or a log record applied before
the cursor is exhausted, must fail R20's fan-in case), the intra-record
checkpoint (a whole-record-only stop must fail the MULTIAPPEND case),
the skipped checkpoint (treating a skipped-only pass as a no-op must
fail R26's case), the epoch rule (refusing without the `(S, epoch]`
check must fail R27's empty rebuild, and an open interval must admit the latest
pre-loss state in H1's case), the `Initial` rule (a continuation that
the retained bound or the epoch can refuse must loop R28's client), the
inclusive resume (an exclusive resume of a partial record must lose its
suffix in R29's case), the existence rule's cache step (a record
consulted first must emit H2's phantom `created`), the reconciliation
order (a tombstone applied before the gap inserts must fail H3's
fixture, and a gap deletion applied after the retained records must
fail I1's), the survivor rule (a most-members rule must fail H10's
fixture), the manifest tag (an unverified manifest must accept H9's
rewrite), per-record
compression (one frame over a blob must fail G5's differential), the
thread blob's padding (an unpadded blob, or one rounded to a 256-byte
multiple, must fail R15's byte-class checks), the alias resolution (a
member rewritten at merge, or an alias dropped without folding, must
fail the R25 page case), the reconciliation's attribution (stamping
without reading the retained records must fail G2's case), the shorter
log's bound (pruning by the Email log alone must fail G4's case), the
counter comparison (comparing the item state must fail G3's empty
account), the vault assertion on creation (a creating batch without it
must recreate the manifest in R19's second case), the skip of an
already-held insert, and the destroy step's chain walk and scan.

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
- Mail spec 3.3's keyless-route contract is amended by the method table
  of 6.2 (section 13, decision 1): for a key account, every method that
  emits or takes an Email or thread id, or whose answer depends on the
  map, `Mailbox/get` included, requires keys; a keyless session keeps
  push, mailbox listing and management, flag and mailbox updates,
  identities and vacation.

Added:

48. A key account's search terms, positions, sort keys, threading fields,
    thread ids, thread history and revision table exist outside process
    memory only in blobs sealed under the search key, bound to the
    account, the segment id, the kind and the predecessor. The manifest
    holds versions, revisions, change ids, segment ids, blob hashes,
    sizes and counts, and nothing derived from content. The stored
    thread id of every key account's message, its `Thread` container and
    every entry of the change log stay the message's own document id,
    and the pass's writes are a function of the keyless log and the
    message cache alone: a publishing pass's thread blob has a hidden
    section that always pads to the smallest class, since its records
    are bounded, merges cost one alias each and the staged bytes are
    budgeted under the class (4.2), and a public section that is a
    multiple of the tombstone count the log shows; the same whatever
    the mail said, in bytes and in counts. What the byte and count
    charges do not fix is time: a pass's duration, and where its time
    cap stops it, follow the processing cost of the admitted messages
    (section 2, R30, H4). Only that depends on what the index found: a
    compacted thread blob pads to a function of the public counts (4.2,
    5.5, decision 2 (c)), so no write's size does. The
    search key leaves `SessionKeys` for one place, the batch object of a
    manifest write, which holds a zeroized copy for the tag computed at
    commit (3.1, R33).
49. The index is loaded, built and written only inside a request that
    holds the account's own session keys, on that request's task; no
    background task, delivery, or keyless path reads or writes it, a
    loaded index is dropped on every invalidation that drops the
    account's keys, and it is never swapped to disk by the cache swap
    tier. Release 1 invariant 6 is unchanged.
50. The index is a deterministic function of the account's present mail
    and the change log's records in order, and, after a loss, of the
    rebuild cursor's order, which is chronological (G16). `indexed_to`
    names a record, the last insert done in it and whether it is
    complete; a partial record is resumed inclusively and a marker in
    its place is loss (R29); it advances, with
    the revision, the blob links and the unlinks, only in one batch that
    asserted the previous manifest, committed through `commit_batch`; a
    pass whose batch fails or loses its assertion leaves the store, the
    links and the loaded index as the committed manifest describes, and
    the next pass repeats the work, which a repeated delta does not
    disturb; a pass that consumed only update records writes nothing,
    and one that moved a checkpoint without staging a record writes the
    manifest alone. Order is chronological: a pending cursor first,
    oldest first, before any post-epoch record; then records in order,
    inserts in document-id order before deletions within a record, a
    record left part-way at an insert boundary when the cap falls inside
    it; every admitted item is charged one fixed reservation, so where a
    pass stops is a function of how many it admitted and of the time
    cap, the processing-cost channel section 2 records; a logged deletion
    is always applied, since a document id is never reused; a transient
    error aborts a pass and never skips a message or rebuilds. No permanent link exists that
    the committed manifest, or a chain it names, does not reach, except
    after a chain break, which the next rebuild or destroy finds by scan.
51. Every content leaf and content sort of a key account's query is
    resolved from a current content part or the query is refused; no
    content leaf, sort or snippet of a key account is ever answered by
    the search store, by decrypting and scanning, or from an index behind
    the request's view of the account. Results are merged, masked, ranked
    and paged by upstream's own code.
52. A thread id of a key account leaves the server only inside a keyed
    session's response, mapped from a thread map validated against the
    manifest's hash and identity version after the response's state was
    read, with the account's change counter at or past the newest
    publication and the epoch, so that the
    map is of the history the state belongs to, holds every publication
    the state reflects, and the state reflects every publication the map
    holds; every Email id such a session emits carries it, every id
    supplied to a keyed method is checked against it, and a thread
    change is rendered as upstream renders a merge, from the
    assignments, the alias table and the revision table, with a
    document's thread and a thread's members defined as of any retained
    state, over a pinned range read unfolded, for every
    state at or after the epoch that the log retains, with a document's
    existence at the range's ends taken from the raw records before the
    sealed record and the set of existing messages always the message
    cache's. A request with no loadable or validated map fails; nothing
    ever substitutes a singleton id, or an older map's id, under a state
    that has seen mapped ones. No thread-valued read waits for a pass's
    I/O.
53. The pass's keyless writes are ids only: the manifest, the blob links
    and unlinks, and, for every pass that indexed a message, one Email
    item update and one Thread property change of the lowest indexed
    document id and a container property change of every mailbox;
    nothing for a pass that indexed none but a manifest-only checkpoint.
    The rebuild's boundary batch is the same shape with the lowest
    message present, and the mailbox entries alone when no message is
    present.
54. Traces of the index carry account and document ids, counts, change
    ids, revisions and reason classes, never a term, header, Message-ID,
    sort key, query string or thread membership.
55. Destroying a key account fences first, with the destroying marker in
    the manifest carrying the replaced manifest's inventory, then
    unlinks every blob that inventory names or its chain reaches, or
    every principal-linked blob of the account when the manifest cannot
    be read or the chain cannot be walked to its end, before the
    Property range is removed; a batch that creates a manifest asserts
    the vault record's presence; and the blob purge then holds nothing
    of the index.
56. A deleted message's identity is retained, sealed, with its deletion
    change id, and an alias with its revision, until that change id
    leaves the shorter of the Email and Thread logs; a rebuild that
    loses identities allocates a fresh change id for its epoch, with or
    without a message to witness it, and refuses every sync state below
    it whose method's log holds a record in `(S, epoch]`, the boundary's
    own entries and the latest issued state included, and never
    `Initial` or its continuations; a newer manifest format is never
    loss, and loss of a content blob alone never moves the epoch.

## 11. Merge hotspots and silent dependencies

New hotspots, fork insertions in upstream files: `crates/jmap/src/email/query.rs`
(the resolution before `query_emails`, the mapped ids, `windowed = None`),
`crates/jmap/src/email/get.rs` and the cache's `email_by_jmap_id`
(`crates/email/src/cache/email.rs:620-624`; ids, `threadId`, the prefix
translation, the validated snapshot), `crates/jmap/src/email/{set,import}.rs`
(the pass after commit, the mapped response, the prefix translation at
`set.rs:1173-1174` and `:1620-1621` with the keyless filter beside it),
`crates/jmap/src/email/snippet.rs:183, 213` (the same translation),
`crates/jmap/src/changes/{get,query}.rs` (the key-account page of 6.3
inside `ChangesLookup::changes`, the epoch check, the bypassed
`allowed_ids` filter, the mapped `touched_threads`),
`crates/jmap/src/thread/get.rs`, `crates/jmap/src/mailbox/get.rs`
(thread counts, now key-gated), `crates/jmap/src/blob/get.rs`
(`Blob/lookup`), `crates/jmap/src/submission/{set,get,query}.rs` (the
singleton stored, the mapped read), `crates/jmap/src/api/request.rs`
(the pass trigger and the gates of 6.2's table),
`crates/imap/src/op/{search,thread}.rs`, `crates/imap/src/op/fetch/mod.rs`
(object ids), `crates/imap/src/core/session.rs` or wherever commands
dispatch (the pass trigger), `crates/store/src/query/log.rs`
(`za_changes_records` beside `changes`), `crates/common/src/storage/transaction.rs`
(`commit_batch`, which the pass's batch goes through),
`crates/services/src/task_manager/tasks/destroy_account.rs` (the marker,
the unlink step and the scan), `crates/store/src/write/blob.rs`
(`user_blob_hashes` extended to principal links, 4.2; the link scan of
5.7), `crates/common/src/storage/blob.rs` (`hold_blob` with the sealed
mail `BlobOpts`, `BlobHold::release` in the manifest's batch),
`crates/email/src/message/thread.rs` (the `K` bound on
`ThreadFields::scan`, or a fork wrapper over it), `crates/types/src/field.rs`
(field 152), `crates/vault/src/{keys,session}.rs` (the search key),
`crates/common/src/cache/` (`za_search`, its invalidation arms, and its
exclusion from the swap tier), `crates/http/src/api/vault.rs` (the empty
manifest at setup), and `crates/store/src/search/` where the index model
re-exposes `pub(crate)` items.

Silent dependencies, each a re-read when upstream changes it: the term
set and positions come from `index_document` and `tokenize`
(`crates/email/src/message/index/search.rs`, `crates/store/src/search/{tokenize,term}.rs`),
and the leaf semantics from `Leaf::plan` (`leaf.rs`) and
`SearchFilter::has_text` (`document.rs:256-281`); a change there changes
what the index must answer, and the differential test (9.5) is the
detector. The thread rules mirror `find_thread_id` and
`ThreadMerge::merge` by hand (`ingest.rs:1098-1197, 1487-1512`), as the
drain mirrors `itip_ingest`. The change rendering of 6.3 mirrors
`merge_index` (`build.rs:414-443`), 1.0.2's `Email/queryChanges` and
upstream's paging (`changes/get.rs:148-608`, `changes/page.rs`,
`jmap-proto/src/types/state.rs`), and rests on the change log's record
layout and decoder (`query/log.rs`), on `log_item_update`'s entry, on
`push_change`'s folding rules (`query/log.rs:347-395`), on the Mailbox
state advancing on container changes (`cache/mod.rs:470-479`), on the
message cache refreshing from item-log updates and its 5 s revalidation
(`cache/mod.rs:55-57, 449-462`; `common/src/cache/mod.rs:72`), on
`commit_batch` marking caches stale and broadcasting
(`common/src/cache/invalidate.rs:49-53`), and on `SetValue::Serializable`
resolving at commit with the assigned change id in hand
(`SerializeWithIds` and `AssignedIds::current_change_id`,
`write/mod.rs:1148-1151, 1192-1209, 1648`; R33).
Change ids are a per-account store counter allocated at commit
(`write/mod.rs:671, 873, 1565`) and Thread shares Email's change group
(`types/src/collection.rs:228-231`), so Email and Thread change ids are
one ordered space, which 6.3's `R(X)` assumes; document ids are a
monotonic reservation counter (`write/mod.rs:1573-1580`) deleted only
with the account (`key.rs:1048-1054`), which 5.1's unconditional
tombstone assumes. The Email id layout (`Id::from_parts`), the log's
prefixing (`build_id`, `SyncCollection::is_prefixed`) and 1.0.3's
`email_by_jmap_id` lookup are assumed; a change to any changes 6.2. The
sort ranks mirror `message_sort_ranks` (`sortkeys.rs:134-218`) and pass
through `query_emails` as precomputed comparators (`sortkeys.rs:120`).
`SearchQuery::filter`'s treatment of an unresolved leaf as absent
(`local.rs:109-113`) is why resolution must be exhaustive. The
manifest's size bound rests on thread compaction and the content chain,
not on `may_be_chunked` (`key.rs:877-897`), which only says why it must
stay small. The purge's rules (a `Document` link keeps a blob, an expired
`Temporary` link does not, `blob.rs:422-466`), `hold_blob`'s staging
(`storage/blob.rs:308-360`), `BlobHold::release` (`:108-122`),
`user_blob_hashes`'s walk (`blob.rs:291-355`, which `BlobRecode`
follows and the fork extends) and `destroy_account_blobs`'s walk and
order (`:214, :246`) are what 4.2 and 5.7 rely on; a change to the blob
codec's options or to what `BlobRecode` selects needs a re-read of 4.2.
The swap tier's `SwapCached` set (`swap/evict.rs:20-26`) must never
gain `za_search`. The in-memory compare of 5.1 step 0 rests on the
message cache's `last_change_id` being the account's current Email
change id on this node, within its revalidation, and on
`CacheInvalidation` moving it across nodes; the validated snapshot of
6.2 rests on a committed batch being visible to a later point read of
the manifest on every backend. The IMAP side of the witness entry rests
on `sync_view` and `View::sync` (`imap/src/core/view.rs:813-838, 645-647`).

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
- **An item update per reassigned message** (revision 3's publication).
  Rejected by the Codex review's R4: the members of a losing thread
  would appear together in one batch's log, and an operator who delivers
  a bridge and reads the cohort learns the thread. The renderer works
  from the sealed history instead, and one entry bumps the state.
- **A witness chosen among the inserted or reassigned messages**
  (revisions 4 and 5). Rejected by the re-review's R4: with a losing
  member older than the bridge, the witness is that member, and an
  observer who knows the bridge was the only insert learns it joined the
  bridge's thread. The witness is now chosen among the indexed messages
  alone, by id.
- **Publishing only when a thread assignment changed** (revisions 4 and
  5). Rejected: a publishing pass is visible, so an operator would learn
  of each delivered message whether it threaded with existing mail. Every
  pass that indexed a message publishes, and the entries are the same
  whatever it found.
- **Bumping each reassigned message's modseq** (R6's request). Rejected
  for the same reason: it rewrites each member. IMAP `CHANGEDSINCE` not
  surfacing a new `EMAILID` or `THREADID` is recorded in 7.5.
- **Stamping transitions with the change id the pass consumed**
  (revision 3). Rejected by R2: a client can hold an unindexed message
  under a state newer than that stamp, and the rendering then never
  destroys the id it holds. The revision table binds each transition to
  the change id of the batch that published it.
- **"State before map" ordering alone** (revisions 4 and 5) as the
  response's consistency rule. Rejected by the re-review's R2: the
  message cache can refresh with another node's publication between the
  pass and the handler's state read, and the loaded map then lags the
  returned state. The validated snapshot reads the manifest after the
  state and reloads when it moved.
- **A state allowed to lag the map** (revisions 6 to 10: "harmless").
  Rejected by the Fable review's F2: a node's cache can be up to 5 s
  behind another node's publication, and a mapped id returned under a
  state below its publication makes `Email/queryChanges` add an id the
  client holds and `Email/get` refuse an id that was valid at that
  state. The validated snapshot requires the state at or past the newest
  revision row.
- **An epoch equal to the change id current at the loss** (revision 4).
  Rejected by the re-review's R3: the latest issued state equals it and
  passes the check while holding lost identities, and repeated losses
  reuse it. The rebuild's batch allocates a fresh change id.
- **Validating the map by its revision counter alone** (revision 6), and
  **by the identity version alone** (revisions 7 to 10). Rejected by R2
  and R18: a rebuild that reset the counter could present a different
  epoch under an equal number, and a deletion-only pass changes the
  checkpoint, the blobs and the tombstones without touching the pair.
  The loaded index is versioned by the hash of the manifest bytes it
  loaded, the value the write asserts, and reloads on any difference.
- **Classifying a changes candidate by the entry that made it one**
  (revision 6). Rejected by R14: a message created and deleted within
  the range, which upstream's folded log cancels, would be resurrected
  from its sealed record. Existence at both ends decides.
- **Existence from the sealed record alone** (revision 7). Rejected by
  the re-review of revision 7, R14: the record lags the log by the
  unconsumed part of a backlog, so a deletion behind the insert cap was
  rendered as "present at both ends" and then fell out of every later
  range. The raw log of the range decides existence; the record fills in
  what the range cannot show.
- **The overlay over the folded log** (revisions 8 to 10). Rejected by
  the Fable review's F10: `Store::changes` cancels an insert followed by
  a delete, so a message indexed and then deleted behind the cap showed
  the renderer neither entry, and its record said present. The overlay
  reads records unfolded.
- **Deletions swept ahead of the insert cap** (revision 8). Rejected by
  the re-review of revision 8, R16: tombstoning a message before an
  earlier insert is processed drops the evidence that insert needed,
  and the grouping diverges from upstream's for good. Strict order
  gives every insert the candidates upstream's ingest had, and the
  renderer's overlay already covers the reads the sweep was for.
- **Dropping a tombstone's fields at once during a rebuild** (revision
  9). Rejected by the re-review of revisions 9 and 10, R16: the cursor
  runs newest first, so an older item can still be pending when a
  newer, already indexed message is deleted. Revision 9 to 13 kept the
  tombstone's fields while a cursor was pending; revision 14 made the
  cursor chronological instead (the bullet above), under which no
  tombstone exists while a cursor is pending and no retention rule is
  needed.
- **Skipping a logged deletion the cache still shows** (revisions 4 to
  8). Rejected by R17: the cache snapshot can predate the record, and
  the skipped deletion then falls behind `indexed_to` forever. Document
  ids are never reused, so a logged deletion is final and is applied
  without consulting any cache.
- **A thread blob written by every pass** (revisions 4 to 10). Rejected
  by the Fable review's F4: every flag change makes the next keyed
  request's pass write an empty padded blob and a manifest. A pass with
  nothing to stage writes nothing.
- **One lock for the pass and its readers** (revisions 4 to 10).
  Rejected by F5: every keyed request of the account waited behind a
  pass's blob I/O, against 7.4's promise. The pass lock and the read
  lock are distinct, and a request never waits for a running pass.
- **Treating every failed open or load as loss** (revisions 4 to 10).
  Rejected by F3: a blob-store timeout skipped a message for good or
  rebuilt the index. The error classes tell loss from a transient
  failure.
- **Rebuilding on a manifest of any other format version** (revisions
  4 to 10). Rejected by F7: an older node in a rolling upgrade would
  rebuild under its format, and the newer node again. A newer version is
  "no map".
- **Unbounded thread records** (revisions 4 to 10). Rejected by F1: a
  sender chooses a record's size through References, so an operator
  could place a pass's payload at a class boundary and read a probe's
  merge off the class. Records keep `K` hashes and `M` is chosen over
  `C · R`.
- **A message-count privacy threshold for the padding** (revision 7).
  Rejected by R15: records vary in length and the rounding is on bytes,
  so no count is a guarantee; the formula is stated in bytes and the
  counts are the benchmark's estimates, now with a bound (F1).
- **Unpadded thread blobs** (revisions 4 to 6). Rejected by R15: a
  bridge that reassigns a hundred messages writes a hundred records more
  than a message that threads with nothing, and the sealed size shows
  it. Padding to coarse classes hides a merge that stays inside the
  class the pass would have written anyway; writing the whole thread set
  on every pass would hide every merge but costs a rewrite of megabytes
  per pass at the design size, and was not taken. Accepting the unpadded
  size channel was decision 2's other answer.
- **Rewriting every member of a losing thread at a merge** (revisions
  3 to 12). Rejected by the seventh Codex round's R21: a merge's cost in
  bytes was the losing thread's size, and an operator who knows two
  Message-IDs, as the SMTP logs show every inbound one, could fill both
  threads with helpers and read off a bridge's class whether they were
  one thread. A merge is one alias entry; members are never rewritten
  except at compaction, where everything is.
- **Tombstones and revision rows in the padded payload, and a bound
  argued from a measured record size and a soft insert cap** (revision
  11). Rejected by R20 and the second Fable review's G1: a grouped
  record overshoots the cap, a deletion backlog the operator can
  schedule adds sizable bytes, and a measured maximum is not a bound.
  Tombstones go to an unpadded public section, the hidden bytes are
  budgeted with the overshoot derived, and `M` sits above the sum by an
  inequality the configuration enforces.
- **One zstd frame over a content blob** (revisions 4 to 12). Rejected
  by G5: a chosen message delivered beside a target shrank the blob by
  what the two shared, a compression oracle. One frame per record, with
  a static dictionary.
- **Membership as of a state reversed from the present thread table**
  (revision 11). Rejected by R25: the present table has lost members
  deleted or merged away after the pinned end. Membership as of `X` is
  defined from the records and aliases directly.
- **Comparing the method's item state to the epoch** (revision 11).
  Rejected by G3: an empty account's item state never moves, so the
  comparison refused it for ever, and a no-message rebuild had no fresh
  boundary. The Email group's counter is compared, and the boundary is
  written with or without a message.
- **Pruning and stamping by the Email log's bound alone** (revisions 7
  to 12). Rejected by G4: the Thread log keeps older change ids, so
  `Thread/changes` served states whose transitions were pruned. The
  shorter log's bound governs both.
- **A newest-first rebuild cursor, with log records consumed ahead of
  it** (revisions 4 to 13). Rejected by the eighth Codex round's R20:
  it let messages newer than a pending item be indexed before it, so
  one old item could merge arbitrarily many threads when it was finally
  reached, and it needed a tombstone-retention rule to keep the
  evidence those items had dropped. Oldest first, before any post-epoch
  record, is upstream's own order and bounds every item at `K − 1`
  aliases; recent mail waits for the cursor, a loss-only cost.
- **Charging a pass's budget by actual alias bytes** (revision 13).
  Rejected by R21: the stopping point, which the checkpoint publishes,
  then moved by one record according to whether a probe's references
  already shared a thread. Every admitted item is charged one fixed
  reservation.
- **Whole-record checkpoints only** (revisions 4 to 13). Rejected by
  R20's second part: a record is not bounded by `MAX_GROUP_EMAILS`, since
  MULTIAPPEND sizes its group by the request, so no fixed class could
  hold the overshoot. The checkpoint names a record and the last insert
  done in it.
- **An all-created `Initial`** (revision 13). Rejected by R28: a
  continuation page cannot tell it from `Exact(m − 1)`, which preserves
  kinds. Revision 14 made `Initial` `Exact(m − 1)`; revision 15 made it
  `Exact(0)` (the bullet below), whose continuation nothing can refuse.
- **Refusing every state below the epoch** (revisions 6 to 13).
  Rejected by R27: an empty account's Email and Thread states never pass
  a boundary that touched only the mailbox log, so its clients were
  refused for ever. The refusal applies when the method's log holds a
  record between the state and the epoch, the part of the delta the lost
  history would have rendered.
- **An open interval for the epoch refusal, `(S, epoch)`** (revision
  14). Rejected by the ninth Codex round's R27 and the third Fable
  review's H1: the boundary's own entries sit at the epoch, so the
  latest pre-loss state passed and R3's defect was back. The interval is
  closed at the epoch.
- **An exclusive resume of a partial record** (revision 14, implied by
  "from `indexed_to` exclusive"). Rejected by R29: the reader skips the
  from-key, so a split record's suffix was lost. A partial record is
  resumed inclusively, and the checkpoint says whether it is complete.
- **`Initial` as `Exact(m − 1)`** (revision 14). Rejected by R28: its
  continuation carried a `from` below the retained bound, which the
  truncation check and the epoch refused, looping the client between a
  first page that succeeded and a second that failed. `Initial` is
  `Exact(0)`, and `from = 0` is never refused.
- **No time cap, so that the checkpoint depends on counts alone.**
  Rejected: a pass of attachment-heavy mail would stall the triggering
  command without bound, and the duration channel exists whatever the
  cap does; the processing-cost leak is recorded (section 2, R30, H4).
- **An unauthenticated manifest** (revisions 4 to 14). Rejected by H9:
  a store writer could rewrite it from authentic blobs into a silently
  incomplete index, where a dropped message is at least noticeable. The
  keyed tag costs nothing, since every writer holds keys.
- **Loss of a content blob as loss of the index** (revisions 11 to 14).
  Rejected by H23: it reset every identity and resynced every client for
  a part that holds no identity. It is a content-only rebuild.
- **A session-scoped background task holding keys** to index after
  login, so the first search never waits. Rejected: it is background code
  holding a key (release 1 invariant 6), and the request-time pass with a
  wait budget gives the same result for the design size.
- **A pass lease across nodes** (the UID lease's form). Rejected as
  unnecessary: the manifest assertion decides, duplicate work on two
  nodes is rare and harmless, and a lease would add a sweep.
- **A sealed manifest.** Rejected: destroy must find the blobs without a
  key, and the manifest's contents are visible from the blob store
  anyway.
- **Index data as data-store values** instead of blobs. Rejected:
  FoundationDB's 100,000-byte value limit would force hundreds of small
  values per account and chunked classes, and the blob store is where the
  mail already is.
- **A segment list blob** to bound the manifest. Rejected in favour of
  the content chain: the chain adds no write and no read a content load
  does not make anyway, and a destroy walks it.
- **One blob per pass holding both sections** (revision 3). Rejected by
  R9: one AEAD cannot be opened in part, so a thread-only load would
  decrypt the whole content.
- **Indexing in the keyed writer** (APPEND, `Email/set`, `Email/import`
  indexing their own message in the same request before the commit).
  Rejected for one code path: the pass after the commit indexes the new
  message from the log like any other, and a lost commit then leaves no
  index entry to retract.
- **Partial answers during a rebuild.** Rejected: an IMAP client cannot
  tell; the unavailable refusal is honest, and the rebuild only happens
  on loss.
- **Reconstructing lost identities after a rebuild** so clients need not
  resync. Rejected by R3: nothing records them once the index is gone;
  the epoch and `cannotCalculateChanges` are what JMAP provides for
  exactly this.
- **Expanding the synthetic changes stream after upstream's paging**
  (revision 5, implicitly). Rejected by the re-review's R12: upstream's
  continuation counts raw entries, and a page expanded afterwards would
  exceed `maxChanges` or drop items; the fork renders the page.
- **A liveness assertion in every batch** as the destroy fence.
  Rejected in favour of the marker (R19): the marker makes every
  in-flight assertion fail with no new assertion anywhere, and a rebuild
  asserting `None` fails too.

## 13. Decisions and calls made

**Decision 1 (Jay, 2026-11-05): keyless sessions and the WebSocket
stay keyless.** Mail spec 3.3 promises a key account's keyless tokens
the methods that need no content, and the web client state-driven sync
over the WebSocket, which the HTTP layer strips of keys. With mapped ids
(6.2) a keyless session would see different ids from a keyed one. Two
answers were tenable:

- (a) **Taken.** The methods of 6.2's table are gated on keys for a key
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

**Decision 2 (Jay, 2026-11-06, re-confirmed the same day on the
corrected premise): the thread-blob size channel, padded over bounded
records.** A thread blob holds one record per message a pass indexed or
reassigned, and its sealed size is visible (section 2). Jay first took
the padded answer on the residual as revision 10 stated it: "a pass
whose payload sits just under a boundary can cross it with one more
record". The Fable review of revision 10 (F1) showed that residual to
be operator-controllable, since a sender chooses a record's size
through References and the operator delivers the mail: a filler could
place a pass's payload at a boundary and a probe's merge could be read
off the class, one bit per pass. Revision 11 added a bound and returned
the decision with the changed premise; Jay re-confirmed it in revision
12. The seventh Codex round and the second Fable review (R20, R21, G1)
then showed that bound unsound (a soft cap, tombstones, measured sizes)
and the merge rewrite itself an oracle on chosen messages; revision 13
replaces the construction, and the premise is now stronger than the one
Jay confirmed, not weaker:

- (a) **Taken.** A merge is one alias entry (4.1, 6.1), records keep at
  most `K` reference hashes (16) and are written once, tombstones go to
  an unpadded public section, the pass runs in upstream's chronological
  order so an item retires at most `K − 1` threads (5.1), every admitted
  item is charged one fixed reservation `R + (K − 1) · a` so the pass's
  stopping point depends only on how many it admitted (5.1), the
  reservations are budgeted (`H`, 32 KiB) with an overshoot of one, and
  the minimum class `M` (64 KiB) sits above that total by an inequality
  the configuration enforces (4.2). A publishing pass's hidden section
  therefore always pads to `M`, and its checkpoint is a function of
  public counts and of the time cap: the per-pass channel carries
  nothing of what merged, in size or in boundary; what it does carry is
  the processing cost of the admitted messages, through the pass's
  duration and the time cap's stopping point (section 2, R30, H4), a
  coarse property of their content that the byte and count charges
  cannot close and that revision 15 states rather than claims away.
  What remained beyond that was the class of a
  compacted blob, cumulative over the mailbox, once per compaction,
  which the fourth Fable review sharpened (I2) to one bit per
  compaction about a chosen message for an operator who delivered
  every message and fills the mailbox to a boundary (section 2), and
  which (c) below closes. The costs: `M − (p + 4)` of padding per publishing pass,
  about 64 KiB, until the next thread compaction folds it away; and the
  `K` deviation from upstream's threading (7.5), a message whose sixteen
  nearest referenced ancestors are all absent while an older one is
  present.
- (b) Not taken, kept for the record: accept the unpadded size, where
  the operator learns, per pass, roughly how many thread records
  changed, and so whether a merge happened and how large, for every
  merge, and where the oracle of F1 is fully available. No padding cost;
  the guarantee would have been narrowed to the notification entries
  alone.
- (c) **Taken by Jay on 2026-11-07**, offered by the fourth Fable review
  (I2) and walked through at his request. A compacted thread blob's
  hidden section pads not by what it holds but to a function of public
  counts, `N_records · (R + a)` rounded up to a class (4.2), where
  `N_records` is the retained records, present and tombstoned (public:
  the message count and the public section's tombstone entries); the
  revision rows sit in the unpadded public section and need no term.
  The bound holds because the retained aliases never exceed the
  retained records (5.5), which took two pruning rules beyond the
  review's argument. The review's "every retained alias is survived by
  a record of a thread it retired" is true at the merge but not at the
  bound: revision 17 kept a deleted bridge while its publication is
  retained, and Codex R35 then showed that the roots a bridge retires
  can all be pruned while its aliases survive, since a catch-up pass
  publishes a merge after the deletions it precedes in the log, to the
  point of 80,896 bytes in a 64 KiB class. Revision 18 drops an alias
  whose source thread has no retained record assigned to it, which
  bounds the aliases by the records through an injection and keeps
  every retained record's resolution through the bridge rule (5.5). It
  removes the compaction-class channel
  entirely and leaves the time cap as decision 2's only residual, at
  about twice the thread part's sealed size on disk and in a load's
  transfer (a 16 MiB class against 8 MiB at 30,000 messages), with
  memory unchanged, since the padding is dropped at unsealing, and the
  load's transient within the thread budget (4.4). Section 2 states the
  channel it closes as sharply as I2 did.

Hiding every merge, including the largest, would need a full rewrite of
the thread records on every pass, which section 12 rejects on cost.

**Calls made in this document**, each reversible at plan time:

1. Mapped Email ids (6.2) rather than singleton ids with a mutable
   `threadId` (section 12).
2. The pass runs on every keyed mail request that finds no pass running,
   capped by count and bytes at record boundaries, a pending cursor
   last; content requests wait up to `ZA_SEARCH_WAIT_SECS`; no partial
   answers; thread-valued reads use the map as loaded and validated and
   never wait for a pass's I/O.
3. Attachment text is indexed under the server's existing extraction
   limits, as for ordinary accounts.
4. The manifest is field 152 in the clear with one revision row filled
   in at commit,
   the chain's length and size, and the destroying marker's form; thread
   records and content are separate blobs linked to principal document
   0, each with an authenticated header carrying its identity, content
   blobs chained, holds released and links in the manifest's batch.
5. The defaults of 4.4, pending the benchmark.
6. No lease; the manifest assertion decides between nodes.
7. The unavailable refusal is a second fixed text, distinct from the
   keyless one; a thread-valued read with no loadable or validated map
   fails rather than falling back.
8. The 1.0.3 merge is its own branch and pull request before this
   release's plan (the precondition above, met).
9. Every pass that indexed a message publishes, with one item update of
   the lowest indexed document id, one Thread property change of it and
   every mailbox's container update, and the renderer works from the
   sealed history (R4); a reassigned message's modseq does not move, a
   recorded IMAP difference (R6); a pass that indexed nothing writes no
   entry, and one that staged nothing writes nothing (R13, F4).
10. A rebuild allocates a fresh change id for its epoch in its first
    batch, with the lowest message present as its witness, and every
    sync state below it gets `cannotCalculateChanges` (R3); a newer
    manifest format is "no map", not loss (F7).
11. The index's blobs take the sealed mail `BlobOpts` (the operator's
    codec encryption on top of the seal, no codec compression), and
    `BlobRecode`'s walk gains principal-linked blobs so a codec key
    rotation recodes them (4.2); the alternative, `BlobOpts::PLAIN`, would
    have left them outside the operator's at-rest key.
12. An id-emitting response validates its map against the manifest's
    hash and identity version with one point read after reading its
    state, requires the state at or past the newest publication, and
    fails rather than serve an older map, one of another epoch, or a
    stale state (R2, R18, F2); the benchmark measures the read's cost.
13. The fork renders a key account's `Email/changes` and `Thread/changes`
    pages over a pinned range read unfolded, with upstream's intermediate
    states, and `Email/queryChanges` consumes the same page (R12, F10).
14. The changes stream decides by a document's existence at both ends of
    the range, from the raw records first and from the creation and
    deletion change ids its record keeps, so a message created and
    deleted within a range yields nothing, as upstream's folded log
    (R14).
15. A publishing pass's thread blob always pads its hidden section to
    the smallest class, by 4.2's byte formula over bounded records, a
    budgeted pass and alias merges: decision 2, taken by Jay on
    2026-11-06, re-confirmed the same day, and restated in revision 13
    on a stronger premise (R15, F1, R20, R21, G1); a compacted thread
    blob pads to `N_records · (R + a)`, a function of public counts, a
    deleted record stays while the publication that assigned it is
    retained, an alias whose source thread has no retained record is
    dropped, and a folded alias moves an assignment one hop: decision 2
    (c), taken by Jay on 2026-11-07 (I2, R35).
21. A merge is recorded as one alias entry, never as a rewrite of the
    losing thread's members except at compaction (R21); a document's
    thread and a thread's members as of any retained state are defined
    from the assignments and the aliases (R25).
22. Content records are compressed one by one with a static dictionary;
    nothing is compressed across records (G5).
23. Thread parts have a memory budget of their own, loads are
    single-flight per account, and the account cap is at most a third of
    the content budget (G6).
24. The validated snapshot compares the Email group's counter; setup
    writes epoch 0; a no-message rebuild still allocates a fresh
    boundary (G3); the retention bound is the shorter of the Email and
    Thread logs (G4); a reconciliation attributes by the retained log
    before stamping (R24, G2).
25. The epoch refuses a state only when the method's log holds a record
    in `(S, epoch]`, closed at the epoch, and never `from = 0` (R27, H1);
    `Initial` is `Exact(0)`, every present message created, its
    continuation always admissible (R28, H19); `Mailbox/*` is outside the
    rule (H12).
27. The checkpoint carries whether its record is complete and a partial
    record is resumed inclusively (R29); the manifest carries a keyed
    tag (H9); the merge survivor is the thread with the most matching
    candidates (H10); a missing manifest is loss on any key account
    (H11), a lost content blob a content-only rebuild (H23); absence of
    a blob is loss only past a retry window and a persistently failing
    document is skipped after `ZA_SEARCH_SKIP_AFTER` passes (H5, H6);
    a thread-part load is never refused for budget (H8); content
    compaction is tiered while a cursor is pending, by tier so that each
    byte is rewritten once per tier (H7, R32); a reconciliation applies
    the gap's inserts, then its deletions, then the retained records
    (H3, I1); existence at
    the range's end comes from the message cache or the log outside the
    range before the record (H2); `K` is a constant of the format (H22);
    the time cap's processing-cost channel is recorded, not claimed away
    (R30, H4).
26. Cursor movement, a consumed insert or deletion that staged
    nothing, and an aborted pass's attempt count (R34), are durable
    progress through a manifest-only checkpoint,
    which update-only runs also write past `ZA_SEARCH_CHECKPOINT`
    records (R22, R26, G19); `content_to` and a
    suspended flag govern content-only rebuilds (R23); mailbox entries
    are property changes (G7); a per-pass time cap (G18); holds last the
    upload TTL class (G15); content compaction waits for a pending
    cursor (G17); the destroying marker carries the inventory and a
    manifest-creating batch asserts the vault record (R19, G12).
16. The pass is one model (5.1): chronological order, a pending cursor
    oldest first before any post-epoch record, records in order with a
    checkpoint that may name a position inside one, a fixed reservation
    charged per admitted item, a logged deletion applied unconditionally,
    no cache consulted for existence, threading fields dropped at
    tombstoning, error classes that never skip for a transient failure,
    and a changes range taking a document's existence from the raw
    records before the sealed record (R14, R16, R17, R20, R21, F3). The
    exceptions of revisions 4 to 13 are withdrawn, and 5.8 checks the
    model against every scenario.
17. The loaded index is versioned by the hash of the manifest it loaded
    and reloads incrementally on any difference (R18, F20); the pass
    lock and the read lock are distinct (F5); the index is never swapped
    (F16).
18. Destroy fences with a marker in the manifest before its inventory
    (R19); an unreadable manifest, an untraversable chain and an account
    without a manifest take the link scan (R7, F23).
19. The method table of 6.2, `Mailbox/get` keyed (F8).
20. Content indexing is suspended above the account cap and resumed by a
    content-only rebuild (F6).
28. The `S = 0` stream is every document present at `E` in document-id
    order (6.3, R31), rather than a fourth field in the fork's
    intermediate state carrying the first retained id at the first page
    (the fourth Fable review's alternative, which upstream's state
    encoding could carry, `jmap-proto/src/types/state.rs:42-74`): a
    stream that needs no record needs no field.
29. Content merges by tier (5.5, R32), each blob's tier in its
    authenticated header, rather than by size.
30. The remedy for a store fault repaired after the backlog was skipped
    is upstream's per-account reindex task, which for a key account
    writes the index's destroying marker (5.1, I6), rather than a
    conditional loss class for a present stub's absent blob.

## 14. Release pointers

Supersedes, in the mail spec: section 1.1's "until release 7" clause and
1.2's "search of sealed content: release 7"; section 5.4's last sentence;
section 8 as a whole (its refusals become 7.4's fallback); section 14's
"keyed thread index" note; section 15's release 7 line; and sections
3.3's and 9's keyless-route list for key accounts, replaced by the
method table of 6.2 (section 13, decision 1). Section 5.5's "arrival
time and size" was already superseded by upgrade ruling 64 (ids only).
The plan 13 outcome's open question 2 (`emailQuerySortOptions`) closes:
the sorts are served. The `Threading` key's harmless clear at unindex
(plan 13, item 8) is unchanged.

Issue #23 is this release. No new follow-up issue is opened by the
design; the plan's outcome note records what it defers.
