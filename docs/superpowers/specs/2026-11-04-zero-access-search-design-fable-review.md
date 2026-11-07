# Zero-access search design: Fable review of revision 10

Reviewed on 2026-11-06 by a fresh Fable 5.1 reviewer, read-only, against
revision 10 of [the design](2026-11-04-zero-access-search-design.md) at
`6af3c94a9` on `za-search-design` (the merged v1.0.3 fork). The brief:
treat the design as a whole and try to break it; report every finding
with severity and confidence, no filter; give an independent verdict on
the three Codex findings then open (R16 for the rebuild cursor, R18,
R19). Revision 11 answers every item below; its header says where.

**Citation check.** Every file:line the document cites was re-verified
and holds. Four claims were wrong or loose (F13, F12, F19, F21). Two
upstream facts the design depends on but did not cite, verified: change
ids are a per-account store counter allocated at commit
(`crates/store/src/write/mod.rs:671, 873, 1565`; FDB `write.rs:717`),
and Thread shares Email's change group (`crates/types/src/collection.rs:228-231`),
so Email and Thread change ids are one ordered space; document ids are a
monotonic reservation counter (`fill_slots`, `write/mod.rs:1573-1580`)
deleted only with the account (`ACCOUNT_KINDS`, `key.rs:1048-1054`).

## P1

### F1. The thread-blob size class is an operator-driven oracle, not a near-boundary coincidence (confidence high on the mechanism, medium on calibration)

Spec: 2, 4.2, 13 decision 2. The operator controls delivery and
therefore every pass's delta. `ThreadFields::scan`
(`crates/email/src/message/thread.rs:30-62`) keeps one 16-byte `xxh3_128`
per Message-ID, In-Reply-To, References and Resent-Message-ID entry with
no cap, so a thread record's size is chosen by the sender: a filler
message with 1,000 References ids is a ~16 KB record of incompressible
hashes; the insert cap does not bound it. Between two keyed requests the
operator delivers a filler whose References length places the pass's
framed payload ε bytes under a class boundary (calibrated on a control
pass) and a probe referencing a target Message-ID; the published class
says whether the probe's merge added ≥ ⌈ε / record⌉ records, one bit per
pass, a threshold of the operator's choice; repeated, it bisects the
target thread's size and tells which of two targets share a thread.

Resolution: bound the stored reference hashes per record at `K` (the
nearest `K` ancestors plus the message's own id; record the parity
deviation) so the per-record size has a known maximum `R`; with the
insert cap `C`, choose the minimum class `M` so that `C · R` is well
under `M`; the threshold becomes a stated lower bound. Also record the
two neighbours of this channel: a compacted thread blob's class is the
cumulative record-plus-history size within retention, and an unpadded
content blob's size is the compressed size of a message's term set,
which depends on its vocabulary.

## P2

### F2. The validated snapshot allows the state to lag the map's newest publication, and the lag is not harmless (high)

Node A publishes revision r+1 at change id C. Node B, inside the 5 s
revalidation window (`RevalidateInterval::DEFAULT`,
`crates/common/src/cache/mod.rs:72`; `revalidate_messages` reads the log
only when the entry is not fresh, `crates/email/src/cache/mod.rs:55-57, 240-262`),
reloads the map but reads a state E < C from its stale cache, and
returns T1|D under E. Then `Email/queryChanges` since E renders created
T1|D for an id the client holds, which upstream's queryChanges puts in
`added` with nothing in `removed` (`crates/jmap/src/changes/query.rs:348-378`);
`Email/get` of T0|D, valid at E, answers notFound; after a rebuild on
another node, E can sit below the new epoch. On one node the lag exists
only if the pass's batch bypasses `Server::commit_batch`, which marks the
cache stale and broadcasts (`crates/common/src/cache/invalidate.rs:49-53`).

Resolution: after the manifest read, require the cache's Email state to
be ≥ the change id of the manifest's newest revision row and ≥ the
epoch; if not, mark the account's Email group stale and re-read, up to
three rounds, then fail. Require the pass's batch to go through
`commit_batch`.

### F3. Transient store or blob-store errors are classified as loss (high)

A blob-store timeout while opening a message's blob skips the message
for ever; a transient error opening a thread blob at load starts a
rebuild (a new epoch, every blob unlinked, every client resynced), and
two nodes can start it at once. 1.0.2's index queue skips a *missing*
blob and retries the partition on any other error. Resolution: define
error classes once for the pass and the loader: a stub that is gone or
a blob that fails its AEAD or parse is a skip (pass) or a loss (load);
any other error aborts the pass with `indexed_to` unchanged, or fails
the request as "no map" / unavailable without a rebuild.

### F4. A non-publishing pass writes a padded thread blob for every flag change (high)

Step 3 wrote the thread blob unconditionally. Every update-only delta
(an IMAP STORE, a JMAP flag change, a MOVE, a mailbox rename) is
consumed by the next keyed request's non-publishing pass, which writes an
empty padded blob, a manifest, and one segment towards
`ZA_SEARCH_SEGMENTS`; a client flagging 100 messages one STORE at a time
writes 1.6 MB and triggers six thread compactions. Resolution: a pass
whose staging holds no record change writes nothing, with step 0
comparing against the newest insert-or-delete record it scanned.

### F5. The index lock serializes every keyed request of an account behind the pass's blob I/O (high)

A pass opens up to 64 messages or 32 MB from the blob store under the
index lock; on a slow blob store every keyed request of the account
waits, against 7.4's "never wait". Resolution: distinguish the pass lock
from the index's read lock; thread-valued reads take the loaded index
under a read lock (the post-commit swap is the only writer); only a
second pass waits for the first; a content request alone waits on the
pass under its budget; eviction takes the same lock.

### F6. The content chain is unbounded above the account cap, and its size is not knowable without walking it (medium)

The manifest records only the content head's hash and segment id, so the
account cap has no input without a walk; above the cap the chain grows
by one blob per pass with inserts, each walked by a destroy or rebuild,
unlinked in one batch, and recoded by `BlobRecode`. Resolution: carry the
chain's length and total sealed size in the manifest; above the cap stop
extending the chain, with a content-only rebuild when the cap is raised.

### F7. A manifest of a newer format version triggers a rebuild on an older node (high)

During a rolling upgrade an old-binary node treats the new node's
manifest as loss and rebuilds under the old format with a new epoch; the
new node then rebuilds again. Resolution: a newer version is "no map";
only an older version rebuilds.

### F8. The keyless/keyed method table is contradictory, and `Mailbox/get`'s thread counts fall into the gap (high; plan-blocking)

6.2 gated "the methods that emit a thread id or a thread-prefixed Email
id"; `Mailbox/get` emits neither, but its `totalThreads`/`unreadThreads`
(`crates/jmap/src/mailbox/get.rs:185-189`) group by the map, which a
keyless request may not read. Section 10 said keyless sessions "get push
and nothing else of mail", which also refuses `Mailbox/*` and `Email/query`
without content, while 6.2 kept `Email/set` flag updates keyless; the
amended list of mail spec 3.3 was never written down. Resolution: a
table of every method with its gate, `Mailbox/get` gated.

### F9. The reconciliation's creation and deletion change ids are wrong or unspecified (medium-high)

A reconciliation tombstoned with the cache's current change id and said
nothing about the creation change id of the messages it indexes; both
are consulted by 6.3 whenever the range shows no raw entry, so a client
that already saw a deletion gets it again as destroyed, and one that
holds a message gets it as created. Resolution: both ids are the log's
oldest retained change id minus one; state what happens when the log
truncates while a rebuild cursor is pending.

### F10. The raw overlay must read records unfolded, or a lagging deletion resurrects through upstream's folding (medium-high)

`Store::changes` folds an insert followed by a delete into nothing
(`push_change`, `crates/store/src/query/log.rs:386-391`); a message
indexed by a pass with its creation in the range, then deleted behind
the cap, showed the renderer neither entry, and its record said present.
Resolution: the overlay reads the range with the fork's unfolded record
reader; the folded view classifies only the never-indexed document.

## P3

- **F11.** The operator learns when a session searched (blob access
  pattern, traces); add it to section 2's list.
- **F12.** "A rollback to an older valid manifest, which the next pass
  heals" is wrong: after a thread compaction the older manifest names
  purged blobs and the next load rebuilds; if the blobs exist, the next
  pass reopens a revision already published under another change id.
  State it as an integrity attack with a divergent, un-signalled outcome.
- **F13.** "Content-addressed blobs of the same bytes dedupe" is false:
  `seal` draws a random nonce (`crates/vault/src/keys.rs:201-203`) and
  the segment id is random.
- **F14.** `Thread/changes` needs thread membership as of S; derivable
  but unstated, and upstream's `allowed_ids` filter
  (`crates/jmap/src/changes/get.rs:465-475`) takes thread ids from the
  cache's singleton column and must be bypassed.
- **F15.** Every publishing pass makes each IMAP session with the
  witness's mailbox selected emit an unsolicited `FETCH (FLAGS ...)`
  with an unchanged MODSEQ (`sync_view`, `crates/imap/src/core/view.rs:813-838`;
  `View::sync`, `:645-647`), and HIGHESTMODSEQ moves.
- **F16.** Exclude the index from the swap tier explicitly
  (`SwapCached`, `crates/common/src/cache/swap/evict.rs:20-26`).
- **F17.** The link step must also commit the hold: `BlobHold::release`
  (`crates/common/src/storage/blob.rs:108-122`) clears the temporary
  link and writes the `Commit` key, without which `blob_exists`
  (`crates/store/src/write/blob.rs:100-110`) stays false.
- **F18.** Eviction takes the index lock; a content load's transient
  footprint (two to three times the loaded size) is counted against the
  budget.
- **F19.** Step 0 is not "one comparison and no I/O": the cache behind
  `last_change_id` revalidates when older than 5 s or marked stale.
- **F20.** Full reload on every cross-node version change; thread blobs
  and the chain are append-only between compactions, so reload
  incrementally.
- **F21.** The thread-part size estimate is an order of magnitude low:
  100-250 B per record, largely incompressible, 3-6 MB for 30,000
  messages.
- **F22.** Setup's manifest write vs "not written through
  `za_vault_commit`": created in setup's vault batch, never touched by
  the vault path afterwards.
- **F23.** `user_blob_hashes` returns hashes without account or link, so
  the destroy fallback is a different walk; and every key account
  provisioned before this release that never ran a keyed request has no
  manifest and takes the scan on destroy.
- **F24.** Every publishing pass makes each node's cache re-read every
  mailbox archive (`crates/email/src/cache/mod.rs:467-473`).

## Verdicts on the three open Codex findings

**R16 (rebuild cursor discards evidence): valid, P2.** The scenario is
exact. The fix: while a cursor is pending, a tombstoned message keeps
its threading fields and is a candidate for cursor items only, never for
log inserts and never for a read.

**R18 (reload pairing): valid, P2.** A deletion-only pass changes
`indexed_to`, the blob list and the tombstones without touching (epoch,
revision). The fix: version the loaded index by the manifest bytes'
hash, the value the assertion already compares, and reload
(incrementally) on any difference.

**R19 (destroy fence): valid, P2.** An IMAP session's keys and token
survive the principal's removal until a command refreshes the token
(`crates/imap/src/op/mailbox.rs:149, 612, 683`, `append.rs:137`;
`renew_access_token`, `crates/common/src/auth/oauth/grant.rs:200-215`).
The fix: destroy's first index step writes field 152 as a "destroying"
marker, asserting the previous bytes (or `None`), then inventories and
unlinks; every in-flight pass loses its assertion, a rebuild asserting
`None` loses too, the loader treats the marker as "no map", and the
range delete removes it.

## Closing judgement

The identity model (version pair, validated snapshot, revision table,
raw overlay, pinned pages) and the storage model (holds then links in
one asserting batch, authenticated chain headers, a bounded manifest)
are sound in their main lines and the ten revisions have closed the
holes the earlier reviewer found. The design was **not yet ready to
plan from** at revision 10: the padding oracle (F1) changes a guarantee
Jay approved on an understated residual; the state-lag rule (F2) and
the unfolded overlay (F10) were the last two gaps in the id contract;
the error-class rule (F3) was missing. The remaining P2s each have a
short, local resolution. After that revision the plan can be written
without guessing, provided it also carries the keyless method table
(F8) and the reconciliation change ids (F9).

# Second round: Fable review of revision 12

Reviewed on 2026-11-06 by a fresh Fable 5.1 reviewer, read-only,
against revision 12 at `fc509f9ed`. Prior findings (R1-R19, F1-F24) were
not re-reported except where the revision-11 answer was incomplete.
Every cited file:line was re-verified. Revision 13 answers every item
below; its header says where.

## P1

- **G1. Tombstones ride in the padded thread blob, so the F1 oracle is
  reopened through a lever the operator controls.** Tombstones per pass
  are unbounded (a delete record is consumed whole and the insert cap
  does not count deletions), and an operator can schedule a mass
  deletion through the Junk auto-expunge (`purge_account`,
  `crates/email/src/message/delete.rs:155-159`; `expungeSchedule`,
  `crates/common/src/config/mailstore/email.rs:337, 362`), deliver a
  probe just before it, and choose the tombstone count so the framed
  payload sits ε bytes under a class boundary. Resolution: pad only the
  hidden part; tombstones and the revision row in an unpadded section.

## P2

- **G2. A reconciliation stamps every tombstone with "oldest retained
  − 1", including deletions whose raw record is still in the retained
  log**, so a client at a servable state never learns of the deletion
  (R14's class). Resolution: attribute by the retained range before
  stamping.
- **G3. The validated snapshot compares the method's item state against
  an epoch nothing moves it to**: an empty account provisioned after a
  container-only record can be refused every `Email/*` method for ever
  (`crates/jmap/src/changes/state.rs:133-139`; `crates/email/src/cache/mod.rs:449-483`);
  and the no-message rebuild allocated no fresh change id (R3's defect
  reintroduced) while a client could hold the mapped id of a since
  deleted message. Resolution: compare `last_change_id`, write epoch 0
  at setup, and let the no-message boundary write its mailbox entries.
- **G4. The retention bound is per log**: `delete_changes` truncates
  Email and Thread independently (`crates/common/src/storage/transaction.rs:116-139`),
  the Thread log keeps older change ids, and `Thread/changes` served
  states whose transitions were pruned. Resolution: the shorter log's
  bound for pruning, stamps and refusals.
- **G5. The content blob is a chosen-plaintext compression oracle**: one
  zstd frame over up to 64 messages lets a probe delivered beside a
  target shrink the blob by what they share, and a rebuild's clear
  cursor bound lets the operator place the probe (CRIME class).
  Resolution: one frame per record, or an accepted residual.
- **G6. The memory budget has no separate bound for thread parts, and
  the defaults cannot load an account near the cap**: thread parts share
  the budget with content and transients, so a busy node fails every
  mail method; `ZA_SEARCH_ACCOUNT_MAX_MB = 256` with a 2-3x transient
  exceeds a 512 MB budget; loads are not single-flight. Resolution: a
  thread budget, single-flight loads, the cap at a third of the budget.

## P3

G7 mailbox entries as `log_container_property_change`
(`crates/common/src/storage/index/build.rs:434-443`; `changes/page.rs:116-125`);
G8 the margin arithmetic at the defaults did not meet its own rule; G9
`ZA_SEARCH_UNRESOLVED` must not carry a header leaf's field name; G10
"nearest references in header order" undefined, since `ThreadFields::scan`
sorts and dedups (`thread.rs:33, 61-62`); G11 the reconciliation cursor
is a difference set, not a bound, and is the one place the pass
consults the cache; G12 the destroy inventory lived only in memory
between the marker and the unlink batch, and the scan walks every
link of every account; G13 `State::Initial` was not rendered
(`changes/get.rs:314-335`); G14 "current" for a content request was
undefined against the returned state; G15 the hold duration was the
wait budget, too short for a compacted blob; G16 a rebuild changes
IMAP `EMAILID`/`THREADID` silently; G17 content compaction during a
rebuild is near-quadratic; G18 attachment extraction runs inline with
no time bound (`index/search.rs:91-99`); G19 update-only traffic never
advanced `indexed_to`; G20 a keyless `Email/set` destroy passes with a
stale prefix, 7.5 named update only; G21 a request's reload can run
under a pass on the same node; G22 `EmailSubmission/get` maps at read
where upstream returns the stored prefix.

## Judgement

Not ready to plan from at revision 12: G1 defeated the padded-class
guarantee decision 2 rested on, G2 lost a deletion, G3 could refuse an
empty account, G4 broke `Thread/changes` retention; G5 was a decision
for Jay; the rest were plan-level clarifications, of which G7, G10,
G11, G13 and G14 a plan could not be written from without guessing.

# Third round: Fable review of revision 14

Reviewed on 2026-11-07 by a fresh Fable 5.1 reviewer, read-only,
against revision 14 at `ea39108fe`, with both prior review files as the
list of what not to re-report. About forty-five citations re-verified;
two semantically loose (H10, H16). The reviewer tried to break the
`K − 1` proof and could not: the invariant that carries it is that,
within a subject bucket, every present message whose kept hashes
contain a given hash is in one thread; merges re-establish it for every
hash they carry, deletions only remove members, aliases never split,
folding at compaction preserves every resolution; so it holds for log
inserts, cursor items, the concurrent-commit inversion, content-only
rebuilds and either reconciliation order. Revision 15 answers every
item below; its header says where.

## P1

- **H1. The R27 epoch rule's open interval lets the latest pre-loss
  state pass.** The boundary batch's change id *is* the epoch, so its
  Email witness update and Thread property change sit at the epoch, not
  inside `(S, epoch)`; a client at the latest pre-loss state is served,
  the pre-loss revision table is gone, and the stream renders destroyed
  `D|D`, created `T'|D` for every message while the client keeps `T|D`.
  Resolution: close the interval, `(S, epoch]`; drop Mailbox from the
  rule.

## P2

- **H2.** A deletion committed between a pass's log read and its commit
  leaves a present record for a message a client already saw deleted;
  at that client's next `/changes` the record says "present at both
  ends" and the stream emits a phantom `created`. Resolution: existence
  at `E` from the message cache's present set for the current state, or
  from the log outside the range for a pinned one, before the record.
- **H3.** The reconciliation's processing order was unspecified; one
  reading tombstones first and reintroduces R16's evidence loss, and the
  difference cursor subsuming a rebuild cursor contradicts "no tombstone
  exists while a cursor is pending". Resolution: inserts oldest first,
  gap deletions last; record the gap-order residual.
- **H4.** The time cap and the pass's duration make the checkpoint and
  the write pattern a function of content (processing cost), which
  sections 2, 48 and 50 denied. Resolution: record the channel; narrow
  the invariants to the byte and count charges.
- **H5.** A persistent non-absent error on one object (an S3 403, a
  backend fault, the codec's missing key,
  `crates/store/src/write/blob_codec/mod.rs:206-222`) wedges the
  account's index and every later message's threading for ever.
  Resolution: classify codec errors; a bounded per-document attempt
  count, then skip as loss.
- **H6.** Absence of a blob is loss at once, so a blob store without
  cross-node read-after-write consistency turns replication lag into a
  full rebuild. Resolution: a retry window before loss.
- **H7.** Rebuild throughput at the defaults is unstated and low: about
  38 items per pass (`H / (R + (K−1)·a)`), ~790 passes for the design
  size, `ZA_SEARCH_PASS_CAP = 64` never binding, ~50 thread compactions
  and a content chain of hundreds of nodes. Resolution: state the cost
  model, make the caps consistent, measure a design-size rebuild.
- **H8.** The thread-part budget is a hard ceiling on concurrently
  active accounts; crossing it fails every id-emitting mail method.
  Resolution: never refuse a thread-part load for budget alone.

## P3

H9 the manifest is unauthenticated (a keyed tag under `swk` is free);
H10 the merge survivor rule is upstream's "most matching candidates,
ties lowest" (`ingest.rs:1449-1451, 1487-1512`), not "most members";
H11 a missing manifest on an empty account was not loss, so it stayed
"no map" for ever; H12 `Mailbox/changes` cannot apply the epoch rule;
H13 the non-publishing thread blob's shape; H14 whether the boundary
batch opens a revision; H15 the cursor's end bound and a single
snapshot; H16 `commit_batch` broadcasts only when the batch logged a
change (`transaction.rs:37-48`); H17 `Email/query`'s `anchor` and
`queryChanges`'s `upToId` are matched as whole ids; H18 the Thread log
is read for its state and bound only; H19 `Initial` renders
cursor-indexed messages as created; H20 two passes per keyed write
request; H21 APPEND answers `APPENDUID`, not `EMAILID`; H22 `K` is a
property of the format; H23 loss of a content node need not reset
identities.

## Judgement

The construction of revisions 13 and 14 holds where it was attacked
before. Not ready to plan from at revision 14 for one defect and two
gaps, H1 (a one-token fix), H2 and H3; H4 a wording correction Jay
should see; H5-H8 resource and failure items a plan can carry once
stated; of the P3s, H11, H13, H14, H15, H17 and H18 the ones a plan
could not be written from without guessing.

# Fourth round: Fable review of revision 15

Reviewed on 2026-11-07 by a fresh Fable 5.1 reviewer, read-only,
against revision 15 at `005b1d990`, with both prior review files as the
list of what not to re-report and the stopping rule in the brief:
classify every finding as construction (the design as built cannot
meet a stated guarantee) or wording (a slip the text can fix in place),
and approve unless a P1 in the construction remains. About 110
citations re-verified; every one holds. The reviewer attacked the
`(S, epoch]` interval with the boundary's own entries at the epoch,
`Initial` as `Exact(0)` against the record-based existence rule, the
inclusive resume of a partial record against `Store::changes`'
exclusive skip and the truncation marker, the validated snapshot across
a cross-node publication in both read orders, the `K − 1` proof under
the concurrent-commit inversion, duplicate Message-IDs and a cursor
item referenced earlier, the rebuild cursor's single snapshot against
an inverted commit, the empty-account boundary against `Email/get`'s
and `Thread/get`'s states, the pruning and folding rules at the
shorter-log bound, the hold/release/link/purge sequence, the destroy
marker against an in-flight pass, and the public section's tombstone
count as a function of the log, and broke none. The tenth Codex round
landed while it read; it had reached R33 and R27-wording independently
before seeing them, verified R31 against upstream (upstream's `Initial`
continuation carries `from = first retained id`,
`changes/get.rs:593-597`, and refuses after a truncation where the
fork's `from = 0` continuation recomputed a stream that had lost
entries), concurred with all six Codex findings without re-reporting
them, and added to R33 that the tag key must then travel in the batch
object (a `Secret`, zeroized, rerun on a FoundationDB retry; 3.1,
invariant 48) and to R31 that upstream's state encoding could carry a
fourth field (declined in revision 16, call 28: a stream that needs no
record needs no field). Revision 16 answers every item below; its
header says where.

**Verdict under the stopping rule: no P1 in the construction; the
design is approved for planning.** Every finding of this round is P3
and classified wording.

## P3

- **I1.** Reconciliation order: with the gap's deletions applied last, a
  retained insert referencing a message the gap had deleted threaded
  with it, where upstream did not, since that deletion certainly
  preceded the insert. Resolution: gap inserts, then gap deletions,
  then the retained records (5.4, 7.5, 9); the gap-insert/gap-deletion
  deviation, whose order is unknowable, stays recorded.
- **I2.** The compaction-class residual is sharper than "coarsely":
  record sizes are public for delivered mail (headers in the clear at
  SMTP time), tombstone and row counts and publications are public, so
  the alias count is the only hidden term, and a filler to one alias
  below a class boundary reads one bit per compaction about a chosen
  bridge; a pad to a function of public counts,
  `N_records · (R + a) + N_rows · row`, would close the channel at about
  twice the thread part's sealed size, since aliases never exceed
  records. Resolution: section 2 and invariant 48 restate the residual
  as I2 did; option (c) under decision 2 offers the pad, for Jay.
- **I3.** Existence at `S` from a lagging record emitted a duplicate
  `destroyed`, the mirror of H2, when a deletion at or before `S` had
  not yet been consumed by a pass. Resolution: the "cache lacks `D` and
  no delete record in `(S, now]`" clause applies for a current `E` as
  well as a pinned one (6.3).
- **I4.** The post-commit swap had no version precondition: a reload
  between commit and swap could be mislabelled or lose a compaction's
  folding until the next validated snapshot. Resolution: swap only when
  the loaded index is still at the hash the pass read at step 0, else
  discard the staging (5.1).
- **I5.** `Thread/get` reads its state from the store
  (`thread/get.rs:80-85`, the Email group's counter Thread shares), not
  the cache, so the validated snapshot's comparison did not name the
  state a Thread method returns. Resolution: a Thread method reads its
  state after the manifest and holds it to the same bound (6.2).
- **I6.** A blob store answering "not found" for every key skipped the
  backlog as loss after the retry window with no named remedy.
  Resolution: upstream's per-account reindex task writes the index's
  destroying marker for a key account, so the next keyed request after
  the repair rebuilds (5.1, call 30).
- **I7.** Clarity: the reconciliation marker's bound defined (4.2, 5.4);
  the content-only rebuild's own cursor bound, and `content_to` not
  moving until that cursor is exhausted (4.2, 5.4); the revision counter
  restarting after a lost manifest, harmless (5.4); `H` 24 KiB corrected
  to 32 KiB in decision 2; one pass per request and keyed account
  (5.2); "every present document renders as created" at `Initial` is
  true under R31's rule, and 7.5 records that upstream omits what its
  log lost.

## Judgement

Approved for planning under the stopping rule agreed for this round.
The four Codex P2s of the tenth round (R31 to R34) and R27-wording are
repaired in revision 16 within the construction; I2's sharper premise
and the public-count pad are for Jay to see under decision 2.
