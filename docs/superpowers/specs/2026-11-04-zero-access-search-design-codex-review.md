# Zero-access search design: Codex review

## Current review — revision 18, 2026-11-07

**Verdict: APPROVE under the stopping rule. R35 is closed; no scoped findings remain.**

This pass covers section 5.5's bridge and dead-alias pruning rules, the injection proving
the alias bound, the induction preserving resolution, and one-hop folding. The related
assignment, state-resolution and padding clauses were checked as dependencies. The rest
of the design was not reopened.

Reviewed the [spec](2026-11-04-zero-access-search-design.md) at this exact baseline:

- Worktree: `/Users/jay/Development/stalwart/.worktrees/za-search-design`.
- HEAD: `9d3457e4d7c5f85058bf000ea293141fdee6ca17`.
- Spec SHA-256: `74f8ef6d4670ee76ad70e624a63cef1eba8d5749c394653e992ce32ac3f4278a`.

The stopping rule remains **approve unless a P1 in the construction exists**. This pass
found no construction or wording defect within the requested scope. No additional design
round is required for R35.

### Why the revised construction holds

**The bound is now a direct injection (1451–1466).** After record pruning and folding,
each kept alias has a retained record assigned directly to its source. Alias sources are
distinct because a thread is retired once, and a record has only one assignment. Choosing
one such record for each kept alias therefore cannot choose the same record twice:
`N_aliases <= N_records`. Fresh document-ID roots and non-reuse preserve source uniqueness
across compactions (1103–1106, 1687–1696). This proof no longer depends on the log's view
of which messages existed when a delayed publication occurred.

**R35's two counterexamples are closed.** In the three-root case, the bridge is assigned
to the survivor, and neither losing source has a retained record after the roots prune.
Both aliases drop. In the amplified case, all 1,792 unsupported aliases drop, leaving the
128 bridge records within the prescribed 64 KiB class. The test requirements explicitly cover
both timelines (2358–2362).

**The bridge rule makes direct-source pruning safe for chains (1446–1453, 1467–1474).**
The first alias in a retained record's chain has that record as its witness. For an edge
`L1 -> L2`, the bridge that created it is assigned to L2 in the same publication and is
retained while that publication is retained, even if deletion preceded publication.
It therefore witnesses L2's outgoing alias. Successor aliases have nondecreasing
publication revisions, including ties within one pass: L2 must still be a live root when
the incoming edge is created. Consequently, an expired successor cannot fold that bridge
away from L2 while the incoming edge remains retained. The induction preserves every
needed alias, so the state-limited traversal and membership rules remain unchanged
(1915–1928).

**One-hop folding preserves the retained suffix (1437–1442, 1468–1469).** Expired aliases
move assignments along the expired prefix, without following a retained alias early.
For `A -> B` below the bound and `B -> C` above it, the folded assignment is B: it still
renders B before the second publication and C afterward. With several expired hops, the
stated postcondition requires all of them to be folded before they are discarded; a
reverse-order destructive sweep that strands an assignment halfway would violate that
postcondition. Repeated compactions re-establish the same witnesses for the retained
suffix. The chain and one-hop fixture and mutation requirements cover these obligations
(2363–2366, 2672–2676).

**The public-count allowance remains valid.** Record retention still depends on deletion
and assignment-publication boundaries, not merge results. Dead-alias pruning removes
aliases without adding records. Thus the injection supplies the missing inequality for
`N_records * (R + a)`, and the encoded-maxima/framing clause accounts for serialization
before rounding (628–637). No larger padding allowance is needed for this correction.

### Review method and limits

Two independent read-only scopes checked the count/public-count proof and the resolution/
folding proof. The primary verified both against the current spec. Existing inherited
parent-model routes were reused because the originating capability ceiling was not
established; no stronger-model claim is made. This is design approval within the requested
scope, not implementation validation. No builds, tests, runtime probes or network checks
ran. Only this report was edited, and the previous review history below is preserved.

## Historical review — revision 17, 2026-11-07

**Verdict: REQUEST CHANGES under the stopping rule: one P1 construction finding.**

This final pass is limited to the requested compaction changes: section 5.5's pruning
exception and alias-count proof, section 4.2's compacted hidden-section allowance, and
section 2's public-count guarantee. It does not reopen the rest of revision 16 or the
previously accepted design decisions.

Reviewed the [spec](2026-11-04-zero-access-search-design.md) at this exact baseline:

- Worktree: `/Users/jay/Development/stalwart/.worktrees/za-search-design`.
- HEAD: `39453054f16e9887716d62778a75ef311936b06e`.
- Spec SHA-256: `6dbc33c3b4180cd82b892b18c4ae08ffb49a0e4a1b28c1bbc7f72ce4c2afa33d`.

The rule remains **approve unless a P1 in the construction exists**. R35 is one shared
cause across the three requested sections, not three separate findings. There are no
additional wording findings in this scoped pass.

### R35 — P1 — Construction: Retaining bridges does not bound aliases by records

**Spec:** 1425–1439 (pruning and proof), 612–618 (size), and 338–345 (guarantee).

The exception retains the record of each bridge whose aliases survive, but a bridge can
retire several old threads. Their old member records can all be pruned while those aliases
remain. The proof's claim that no alias retires a thread without a retained member is
therefore false after pruning. Being present when an insert is replayed does not mean a
record survives the later retention boundary.

A minimal timeline, with P the bridge's publication change ID:

1. A, B and C are already indexed in separate threads with the same subject. Their
   assignment publications precede P.
2. N is inserted with references to all three. A, B and C are then deleted, all before
   the next keyed pass. N remains present.
3. Catch-up processes N before those deletion records. The old indexed records still
   have their threading fields, so N merges the three threads. Its assignment and the
   two aliases publish at P, after the raw deletion change IDs. The pass also applies
   those deletions in their chronological positions.
4. Compaction's retention bound reaches P. A, B and C have both assignment and deletion
   IDs below P, so section 5.5 prunes them. N and the two aliases at P remain: aliases
   are dropped only when their publication is older than the bound.

That leaves **one retained record and two retained aliases**. This uses the normal
chronological replay at 931–947 and 1069–1090; it needs neither a rebuild nor a concurrent
delete. The new exception protects N, but it does not protect the roots that N retired.

The discrepancy can exceed an actual padding class, not just invalidate the proof.
Use 128 independent families of 15 old singleton roots. Each live bridge keeps its unique
own Message-ID and references to its family's 15 roots, exactly K = 16 hashes, and creates
14 aliases. Queue every bridge before the root deletions, and perform catch-up after
those deletions. Split-record checkpoints permit multiple publishing passes; every
bridge's assignment and aliases publish at or after P, the first such publication.
At a later compaction with bound P, all old roots prune, leaving:

| Quantity | Value at the stated R = 296 and a = 24 byte widths |
|---|---:|
| Retained records | 128 |
| Retained aliases | 1,792 |
| Hidden payload, 128R + 1,792a | 80,896 bytes |
| Prescribed framed allowance, 128(R + a) + 4 | 40,964 bytes |
| Prescribed class, including the 64 KiB minimum | 65,536 bytes |

The required compaction cannot fit. Refusing to publish prevents that compaction from
completing; choosing a larger class from the actual payload restores dependence on hidden
merge history and contradicts section 2. This is P1 construction because the specified
retention and padding rules cannot jointly meet the new guarantee. Correcting the proof's
wording alone cannot repair them.

**Correction:** change the retention/pruning rule and prove its resulting alias bound, or
use a larger public-count allowance. The existing bridge-retention exception supports the
straightforward bound `(K - 1) * N_records` aliases, hence an allowance of
`N_records * (R + (K - 1) * a)`, with complete framing before class rounding. That option
changes the stated padding cost. If pruning unreachable aliases instead, specify which
aliases can go and prove both retained-state resolution and the tighter count bound;
the current age-only alias pruning does not perform that operation.

**Validation required when implementing:** compact the minimal and amplified timelines
above at P. Check serialized fit and every retained-state resolution. Compare equal
public-count fixtures whose old roots were separate versus already joined; both must
successfully serialize into the same class chosen solely from the public inputs.

### Disposition of the other scoped checks

- **Hidden-section inventory:** records and aliases are the specified hidden contents.
  Tombstones and revision rows are explicitly in the public section (542–549); the
  four-byte framing prefix is explicit (601–607), and blob headers and sealing overhead
  are fixed outside the class. No additional missing hidden component was identified.
  The eventual codec must include its record framing in R/a and section framing in the
  allowance; the approximate byte figures do not replace encoded maxima.
- **Public-count selection:** the stated record-retention predicate uses deletion and
  assignment-publication boundaries, not merge outcomes. No independent defect was found
  in choosing N_records. R35 prevents that count's current allowance from being sufficient,
  so the public-count-only guarantee is not yet established by this construction.

### Review method and limits

Two independent read-only review scopes covered the pruning proof and the byte/public-count
contract. The primary checked their common counterexample against the current spec and
adjudicated it as one finding. Existing inherited parent-model routes were reused because
the originating model's capability ceiling was not established; no stronger-model claim
is made. This was source and design review only: no build, test, runtime probe or network
validation ran. Only this report was edited; all prior reviews below are preserved.

## Historical review — revision 15, 2026-11-07

**Verdict: APPROVE under the requested stopping rule. No P1 construction finding remains.**

Reviewed the [spec](2026-11-04-zero-access-search-design.md) and its responses to Codex
R27–R30 and the third [Fable review](2026-11-04-zero-access-search-design-fable-review.md)
at this exact baseline:

- Worktree: `/Users/jay/Development/stalwart/.worktrees/za-search-design`.
- HEAD: `005b1d990c13edc61fd0f7d4ecd8ae04fd93b359`, including the merged v1.0.3 fork.
- Spec SHA-256: `b5ee09753f1cff43cc900627948d7c55d9bf1a6296e44a87fb2f774a9f87be55`.

The stopping rule for this round is **approve unless a P1 in the construction remains**.
Each finding below is classified as construction (the specified algorithm cannot meet its
stated guarantee) or wording (an in-place clarification using the existing construction).
There are **six nonblocking findings: two P2 construction, three P2 wording and one P3
wording**. None meets the rejection threshold. This is design approval under that rule,
not a claim that every listed issue is resolved or that the implementation is validated.

### Disposition of the revision 14 findings

| Finding | Revision 15 disposition |
|---|---|
| R27 | P1 closed: `(S, epoch]` includes the boundary. Method-kind wording remains below. |
| R29 | Closed: explicit `complete`, inclusive partial resume and truncation handling. |
| R28 | Reported cases closed: Initial and its continuations share `from = 0` semantics. |
| R30 | Processing-cost residual explicitly accepted; two stale absolute claims remain. |

R20/R21's earlier padding attacks remain addressed. The merge survivor now follows
upstream's matching-candidate counts and lowest-ID tie-break. Per-record compression and
the fixed `K` format property remain intact. This review does not reopen the accepted
timing, compaction-size, rebuild-freshness or reconciliation-order residuals.

### R31 — P2 — Construction: Preserve Initial pagination when retention advances

**Spec:** 1735–1741 and 1796–1829.

The unconditional `from = 0` exemption makes the previous Initial continuations usable,
but it also permits continuation after the candidate stream has lost entries. The stream
is recomputed from retained records and skips `sent` items; a pinned E does not pin those
records against truncation.

A, B and C have unchanged singleton assignments and retained updates at 100, 101 and 102.
Initial with `maxChanges=1` returns created A and a continuation for `(0, 102]`, `sent=1`.
Retention then removes update 100. A has no assignment or alias transition to keep it in
the candidate union, so the recomputed stream is B/C. Skipping one drops B, and the client
can reach Exact(102) without B ever being reported.

[Truncation](../../../crates/common/src/storage/transaction.rs), lines 228–245, removes
old records; [the reader](../../../crates/store/src/query/log.rs), lines 149–151,
reports that loss. The unconditional Initial-continuation exemption suppresses its usual
protection. This is a P2 construction issue, not another classification mismatch.

**Correction and validation:** retain enough candidate/retention provenance for an Initial
continuation, or define a safe restart condition when its inputs disappear. Truncate
between pages with E unchanged; require the original stream or an explicit restart,
never a successful continuation that silently skips B.

### R32 — P2 — Construction: The youngest-segment merge is not logarithmic

**Spec:** 1305–1318; related test at 2335–2337.

Merging the youngest S blobs at a chain length of 2S repeatedly selects the growing result
of the previous merge. With S=2, oldest to newest:

`A, B, C, D -> A, B, CD`; append E and merge: `A, B, CDE`; append F: `A, B, CDEF`.

C and D are rewritten on every later append. At the default S=16, the same recurrence
happens every fifteen new segments. The chain length is bounded, but cumulative rewrite
volume remains quadratic rather than giving each byte logarithmically many rewrites.

**Correction and validation:** specify merging by size/level so the growing aggregate is
not repeatedly selected, or accept the actual cost and remove the logarithmic guarantee.
Measure total bytes rewritten over a long rebuild, not only chain length. Update the old
test that still expects no content compaction while a cursor is pending.

### R27 — P2 — Wording: Test method-visible changes at the epoch boundary

**Spec:** 1216–1227 and 1680–1682.

The closed interval fixes the latest-pre-loss-state defect. Qualify "any record" in the
method's log: Mailbox property changes also occupy `SyncCollection::Email`, so testing
raw Email-log records would count an empty rebuild's mailbox-only boundary and recreate
the empty Email resync refusal. Email needs item changes; Thread needs its applicable
container changes.

**Correction and validation:** use the existing kind distinction in
[changes/get.rs](../../../crates/jmap/src/changes/get.rs), lines 566–568, when evaluating
the epoch condition. Keep the empty-account fresh Email-state round trip alongside the
nonempty latest-state rejection. This clarifies the intended method-specific rule without
changing the construction.

### R33 — P2 — Wording: Finalize the manifest tag after assigning commit-time fields

**Spec:** 624–642.

The tag covers every manifest field, including change IDs and epochs assigned at commit,
but the text still names `SetValue::Patched`. A tag computed before those patches would
fail on the first genuine reload: `Patched` changes the bytes without recomputing a MAC.

An existing hook already solves this. [write/mod.rs](../../../crates/store/src/write/mod.rs),
lines 1192–1209, provides `SetValue::Serializable` / `SerializeWithIds`, which serializes
with the assigned IDs available. No new storage architecture is necessary.

**Correction and validation:** name commit-time serialization and require the final assigned
fields to be serialized before computing the tag. Publish and rebuild, then reload both
manifests and require successful verification; altered fields must still fail verification.

### R34 — P2 — Wording: Allow the retry-counter write on an aborted pass

**Spec:** 932–939.

The error rule says a transient failure writes nothing while also requiring its document
ID and aborted-attempt count to persist in the manifest. Following the former literally
prevents the count from advancing across eviction/restart and defeats the bounded-attempt
policy.

**Correction and validation:** explicitly permit a manifest-only CAS that updates retry
metadata while leaving checkpoints and indexed data unchanged. This uses the existing
manifest-only transaction mechanism. Repeated failures with intervening reloads must
reach the configured threshold.

### R30 — P3 — Wording: Remove the remaining absolute progress claims

**Spec:** 291–292 and 537–539; accepted residual at 329–343.

The timing residual is now explicit, resolving the substantive finding. Two older
sentences still call compaction the sole content-dependent write channel and say the
stopping point depends only on admitted count.

**Correction:** qualify the first as the remaining size channel and the second as the
byte/count cutoff. This makes them agree with the accepted time-based progress residual;
no mechanism or additional privacy guarantee is requested.

### Review evidence and stopping decision

Three focused read-only reviewers examined privacy/integrity, protocol/history, and
persistence/recovery under the same stopping rule. Each returned approval with no P1
construction finding. The primary checked the reported scenarios and relevant source,
including the existing commit-time serialization hook, and reached the same verdict.
Reviewers inherited the parent model route because the originating model identity/capability
ceiling remains unresolved; no stronger route was assumed.

The tracked revision changes only the spec and Fable report; implementation and repository
guidance are unchanged. No builds, tests, servers, benchmarks, network requests or runtime
probes ran. Only this report was edited. Fresh verification covers HEAD/spec hash, local
links, classification/severity counts, formatting, unchanged historical report content and
working-tree scope. The stopping threshold is satisfied, so this review is complete;
the nonblocking findings do not trigger another review round as a condition of approval.

This section supersedes the earlier reviews below, whose original references remain as
history.

## Historical review — revision 14, 2026-11-07

Reviewed revision 14 of the [spec](2026-11-04-zero-access-search-design.md), including
chronological rebuilding, fixed reservations, within-record checkpoints and the revised
epoch/Initial rules, at this exact baseline:

- Worktree: `/Users/jay/Development/stalwart/.worktrees/za-search-design`.
- HEAD: `ea39108fe6f97a596ba80021677570491e7a92e1`, including the merged v1.0.3 fork.
- Spec SHA-256: `8b7ddd79aa1a831f01b1e79065e6debead86e3640dcd51535a1b3f081483e0aa`.

The prior alias-fanout and actual-alias-byte counterexamples are addressed, as is the
permanently skipped insert checkpoint. Two sync findings remain in narrower forms, and
two additional interactions need correction or an explicit contract decision. There are
**four current findings: two P1 and two P2**. The P1s concern sync identity loss and skipped
record suffixes, not the previous padding attacks. All spec references below use revision 14.

### Disposition of the revision 13 findings

| Finding | Current disposition | Revision 14 evidence |
|---|---|---|
| R20 | Privacy bound addressed | No tombstone candidates; item boundaries, 781–814. |
| R21 | Addressed for the reported alias-byte oracle | Fixed reservations, 491–502 and 906–914. |
| R26 | Addressed in design | Permanent skips checkpoint and advance coverage, 929–948. |
| R27 | Partly addressed | Empty-state loop fixed; latest pre-loss state admitted, 1103–1114. |
| R28 | Partly addressed | Kinds agree; continuation loses Initial admission, 1634–1659. |

The useful boundedness invariant is one resolved thread per `(subject, kept hash)`,
preserved by merging all matching roots. Message-ID uniqueness is not required: duplicate
headers can name several present messages, but their matching records share that root.
Thus the stronger one-message premise in the prose needs clarification, but duplicates
alone are not another fanout finding. The accepted compaction residual and rebuild
freshness tradeoff are not findings either.

### R27 — P1, remaining: Keep the latest pre-loss identity state fenced

**Current spec:** 1061–1074, 1103–1117, 1561–1566 and 1582–1606.

The empty-account exception now admits any pre-epoch state S with no method-log record
in `(S, epoch)`. Lack of activity after S does not mean the client held no identities that
the index loss changed. In particular, this admits the latest state immediately before
loss because the rebuild's boundary entry at `epoch` is excluded from that interval.

At the latest state S=100, a client holds mapped `T|D` in a nonempty account. The index
is lost without intervening mail activity, and the rebuild publishes epoch 101. A capped
rebuild has not yet reached D, so the newly loaded map treats it as singleton `D|D`.
`/changes since 100` is nevertheless admitted: there is no record in `(100, 101)`.
The lost assignment cannot supply destruction of `T|D`, and later assignments are rendered
from the singleton. The client retains an old ID that current keyed lookup rejects.

This reopens R3's latest-issued-state case while fixing R27's empty-account availability
case. It is more serious than the previous refusal loop because a successful delta can
silently fail to reconcile the client's identity set.

**Required change:** keep the identity-reset boundary effective for a nonempty account's
pre-loss states. Include the relevant boundary publication in the admission decision, or
use an equivalent per-method rule that distinguishes harmless empty states from states
whose identities were lost. Preserve the empty-account round trip.

**Regression:** a client at the latest state holding mapped messages, immediate index loss,
and a capped rebuild leaving one mapped message pending. Require refusal/resync rather
than an incomplete successful delta; retain the empty Email/Thread resync regression.

### R29 — P1, new R1 interaction: Resume the unfinished record instead of skipping it

**Current spec:** 594–595, 824–829, 855–875, 921–927 and 1790–1792.

The checkpoint now names `(record c, last admitted insert p)`, but the manifest still
describes a whole-record scalar and the concrete log reader still starts exclusive of
`indexed_to`. The reader is over physical log keys, each of which holds the entire record;
an exclusive start at c cannot retrieve the unconsumed suffix identified by p.

With a cap of one and record c containing inserts A and B, the first pass commits `(c, A)`.
After restart, the specified exclusive read omits c and never indexes B. Deferred deletions
in c are omitted too. Grouped records are a normal path:
[IngestGroup::flush](../../../crates/email/src/message/ingest/group.rs), lines 130–152,
writes one batch, and [MULTIAPPEND](../../../crates/imap/src/op/append.rs), line 186,
sizes the group from the request.

[Log reads](../../../crates/store/src/query/log.rs), lines 130–148, skip the equal key for
an exclusive query. Their truncation check at 149–151 is inside that admitted-record
branch, so the same resume rule also misses an empty truncation marker replacing c.
[Log truncation](../../../crates/common/src/storage/transaction.rs), lines 228–244,
writes exactly such a marker. The scalar shortcut and content-current comparisons also
need to distinguish a partially consumed c from a completed one, particularly when the
prefix consisted entirely of skipped inserts and created no publication.

**Required change:** carry complete versus partial checkpoints through the manifest, reader,
shortcut and content coverage. Resume a partial record inclusively, process inserts after
p, and apply its deferred deletions before treating it as complete. Only a completed record
permits an exclusive resume. Detect loss/truncation of the unfinished record explicitly.

**Regression:** restart mid-record, including an entirely skipped prefix, trailing deletions,
records with no inserts, competing reload and truncation of the unfinished record. Require
complete indexing or explicit reconciliation, never silent omission of a suffix.

### R28 — P2, remaining: Preserve Initial admission on its continuation pages

**Current spec:** 1634–1659.

Using the same classification as `Exact(m - 1)` fixes the previous created/updated mismatch.
It does not preserve Initial's admission exemption. The first page is exempt from retention
and epoch rejection; its `Intermediate` continuation is not.

With oldest retained boundary m=100 and two remaining updates, Initial with `maxChanges=1`
returns one item and `Intermediate{from:99,to:E,sent:1}`. The next request is no longer
Initial and the stated rule rejects its `from` below the retained bound. After a rebuild,
retained records between that artificial lower bound and the epoch can independently
trigger the epoch refusal. No concurrent truncation is needed for the latter case.

[The state type](../../../crates/jmap-proto/src/types/state.rs), lines 18–30, carries no
Initial flag. [Continuation handling](../../../crates/jmap/src/changes/get.rs),
lines 393–432, applies the ordinary truncation checks; actual truncation leaves the empty
boundary marker described in R29. The design must distinguish the truncation boundary
from the first surviving event when defining m, rather than assume subtracting one always
produces an admissible continuation state.

**Required change:** define a lower-bound representation and admission rule under which an
Initial-derived continuation remains valid while its pinned data is retained, including
across an epoch. Keeping the item kinds identical is only one part of that contract.

**Regression:** page Initial after actual log truncation and after an index rebuild, with
reload between pages. The complete paged result must equal the successful unpaged result.

### R30 — P2, new contract gap: Qualify the remaining time-based progress leakage

**Current spec:** 257–270, 307–310, 491–502 and 913–918.

Fixed reservations remove the actual-alias-byte stopping oracle. The stronger claim that
the checkpoint depends only on public counts still conflicts with the elapsed-time cap:
admission stops at `ZA_SEARCH_PASS_MS`, and indexing work depends on sealed content.

Two runs can have the same public records, message sizes and reservation charges, but item
P completes just below the time limit in one and just above it in the other. The first
admits Q; the second commits after P. The manifest and publication sequence disclose that
processing-cost distinction. The reused
[index_document](../../../crates/email/src/message/index/search.rs), lines 66–99,
walks message parts and branches between text processing and attachment extraction; equal
message sizes do not imply equal execution costs.

This is a processing-cost residual, not a demonstrated chosen-pair membership attack.
Its practical calibration and information content have not been measured. The existing
timing allowance describes indexing time/account activity, rather than content-dependent
partitioning of work into publicly visible checkpoints.

**Required change:** explicitly accept this residual and narrow the categorical checkpoint
claim, or make committed publication boundaries independent of elapsed processing time.
Keep the fixed-reservation and equal-size guarantees; this does not reopen their resolved
counterexamples.

**Regression:** use a controlled clock with identical public inputs/reservations but different
item-processing durations, and verify the selected progress contract.

### Acceptance clarification

The revised R16 regression at 2023–2027 deletes D while D is still pending in the rebuild
cursor, but expects ordinary grouping through D. That is the explicitly accepted missing
evidence case at 1841–1844: postponing D's log deletion cannot restore its missing message.
Delete D after the cursor has indexed it to test preserved evidence, or expect the recorded
divergence in this fixture. This is a test expectation correction, not a new runtime defect.

### Re-review evidence and limits

Three focused read-only reviewers checked the privacy proof, protocol/state rules, and
checkpoint/recovery model. The primary verified their counterexamples against the current
spec and source. Reviewers retained the inherited parent model route because the originating
model identity/capability ceiling remains unresolved; no stronger route was assumed.

Only the spec changed in tracked files since revision 13; implementation and repository
guidance are unchanged. No builds, tests, servers, benchmarks, network requests or runtime
probes ran. Only this report was edited. Fresh verification checks the exact HEAD/spec hash,
local links, finding counts, formatting, unchanged historical report content and working-tree
scope. Addressed findings are design-level conclusions, not implementation validation.

This section supersedes the earlier reviews below, whose original references remain as
history.

## Historical review — revision 13, 2026-11-07

Reviewed revision 13 of the [spec](2026-11-04-zero-access-search-design.md), including
the alias representation, the revised padding proof, and the responses to the second
[Fable review](2026-11-04-zero-access-search-design-fable-review.md).

- Worktree: `/Users/jay/Development/stalwart/.worktrees/za-search-design`.
- HEAD: `1e79e2c4d9a0e8bb09782689e6d194bdadf13f20`, including the merged v1.0.3 fork.
- Spec SHA-256: `9c3317171c238cbd0ea0a120363d90854055a40d7a445bd951a842d731d24769`.

The structural change removes the original write cost proportional to a losing thread's
membership. Six of the eight previous findings are addressed. Two privacy findings remain
in narrower forms: the worst-case alias fanout is not bounded as claimed, and the actual
hidden-byte budget exposes merge-dependent pass progress even when blob sizes are equal.
Three additional correctness findings concern the newly specified transitions below.
There are **five current findings: two P1 and three P2**. All spec references in this section
use revision 13.

### Disposition of the previous Codex findings

| Finding | Current disposition | Revision 13 evidence |
|---|---|---|
| R16 | Addressed in design | Cursor evidence survives compaction/reload, 769–784, 1109–1110. |
| R19 | Addressed in design | Retained destroy inventory and vault assertion, 1176–1198. |
| R20 | Partly addressed | Explicit widths and public tombstones; overshoot premises remain false. |
| R21 | Partly addressed | Original size amplification fixed; visible stopping rule still leaks. |
| R22 | Addressed in design | Skipped cursor progress commits manifest-only, 884–899. |
| R23 | Addressed in design | Durable coverage and content-presence backfill, 583–586, 1043–1049. |
| R24 | Addressed in design | Retained lifecycle events attributed before stamping, 1071–1080. |
| R25 | Addressed in design | Historical endpoint membership, 1504–1509 and 1549–1553. |

The public tombstone section addresses G1's specific padding contribution. Per-record
compression with a static dictionary addresses G5's cross-message compression window.
The shared retention bound addresses G4's previously servable-but-pruned Thread states.
G3's setup admission is corrected, but its empty-rebuild response states remain incomplete
(R27). G13's new Initial rule needs continuation semantics (R28).

The accepted compacted-blob size residual is not another finding. This review checks the
stronger per-pass guarantee stated in revision 13 and does not request a new privacy goal.

### R20 — P1, remaining: Establish the actual maximum alias fanout and grouped overshoot

**Current spec:** 469–490, 769–778, 866–879 and 1278–1293.

The new formula assumes an insert creates at most `K - 1` aliases. That depends on each
matching hash identifying at most one candidate thread. Cursor candidates include retained
tombstones, while ordinary log inserts ignore those same tombstones; those two rules can
leave many independent live threads reachable through one retained hash.

Keep an old cursor message C, whose own Message-ID is `x`, pending. Index D1 with own ID
`y1` and reference `x`; index N1 referencing only `y1`; then delete D1. D1 keeps its fields
for the cursor and N1 remains live. A later logged D2 referencing `x` ignores D1's tombstone
and cannot match N1, whose fields contain no `x`, so it starts a separate thread. Index N2
referencing D2, delete D2, and repeat while log work consumes the cursor's budget.

When C is finally processed, its single hash matches every retained Di. Each corresponding
thread has a live Ni, so C merges N distinct roots and writes `N - 1` aliases. Eighteen
roots already exceed the default fifteen-alias bound. Scaling this permitted sequence
makes one indivisible cursor item exceed the assumed overshoot and eventually `M` itself.
The claim that those tombstones existed at the epoch (777–778) is not enforced by their
retention or candidate-admission rules; the Di in this sequence arrived after the epoch.

There is also a separate unsupported premise in the same formula: `G = 32` is a default,
not a universal log-record maximum. [IMAP APPEND](../../../crates/imap/src/op/append.rs),
line 186, constructs `IngestGroup::with_max_emails(arguments.messages.len())`.
[group.rs](../../../crates/email/src/message/ingest/group.rs), lines 61–65, honors that
instance limit and separate byte/batch limits. This disproves the cited universal bound;
this review does not claim that APPEND alone necessarily exceeds the default 64 KiB class.

**Required change:** prove and enforce a fanout bound for every eligible cursor state and
derive overshoot from every writer's actual record limit. Alternatively, define how a
larger merge can be staged without violating whole-record checkpoint correctness or the
fixed-size publication contract. Fix the post-epoch tombstone eligibility premise explicitly.

**Regression:** retain more than `K` separate live roots connected only through tombstones
matching one pending cursor hash, including a case beyond the proposed byte budget. Cover
the configured MULTIAPPEND group path as well as default ingest grouping.

### R21 — P1, remaining: Choose pass boundaries independently of actual merge bytes

**Current spec:** 228–244, 472–490, 869–879, 916–920 and 2212–2219.

Aliases fix the original helper-cohort rewrite oracle. The new stopping rule nevertheless
charges the actual bytes of aliases, whose count depends on whether the referenced
messages already share a thread. The clear `indexed_to`, cursor bound and publication
sequence expose where the pass stopped. Equal padded blob sizes do not hide that boundary.

Using the proposed widths (`R = 296`, alias size 24, `H = 24576`) illustrates the gap:

1. Prepare 37 known filler inserts, each retiring 15 known roots. Each can match sixteen
   roots through its own ID (a prepared root references that future ID) and fifteen
   references. Their hidden contribution is `37 * (296 + 15 * 24) = 24272` bytes.
2. Follow them with a full-width probe referencing known A and B, plus dummy references
   that match nothing. If A/B already share a thread, it adds only 296 bytes: total 24568.
   If they are separate, its additional alias makes the total 24592.
3. Place one more insert after the probe. The pass consumes it in the first case and
   defers it in the second. The checkpoint reveals the selected pair's prior membership,
   even though both hidden sections pad to 64 KiB.

This uses only 38–39 small insert records, so the count and message-byte caps can remain
nonbinding; choose a non-compaction pass and a nonbinding time budget. The precise filler
counts follow the eventual fixed serialization widths, but the dependency exists whenever
actual hidden bytes control the publicly visible stop.

**Required change:** determine boundaries using a charge independent of actual reference
matches and aliases, such as a conservative fixed reservation per admitted message. Pair
that policy with a sound worst-case bound from R20. Padding the emitted payload alone does
not establish that the pass's writes are a function of public inputs.

**Regression:** compare identical public inputs with A/B joined versus separate. Require
equal checkpoint/cursor progress, publication sequence and blob-size observations, not
only equal padded classes. Preserve the original amplified-merge regression too.

### R26 — P2, new: Define a durable transition for permanently skipped log inserts

**Current spec:** 787–801, 808–810, 818–824, 843–846, 884–905 and 1720–1722.

R22's cursor case is fixed, but the enumerated write cases do not say how a log delta
containing only permanently skipped inserts commits progress. Manifest-only triggers name
the periodic threshold and cursor movement; the delayed no-op case names update-only scans.

Let `indexed_to = content_to = 100`, with no cursor. Insert 101 has a permanently unreadable
blob. The pass stages no record or tombstone and does not meet the 1000-record checkpoint
threshold. If that empty staging is treated as a no-op, only `scanned_to` becomes 101.
The next-pass shortcut returns, but the new content-current predicate still refuses because
an insert remains in `(indexed_to, scanned_to]`. An otherwise idle account cannot search
its unaffected mail. Eviction instead reopens a message described as permanently skipped.

This is a missing transition, not a claim that every implementation must choose the no-op
interpretation. The general checkpoint invariant permits the correct action, but the
concrete cases need to require it.

**Required change:** checkpoint consumed permanent skips even when no index record changed.
Reserve delayed checkpoints for update-only work, and define matching content coverage for
the skipped-only transition.

**Regression:** one unreadable logged insertion on an otherwise idle account, followed by
repeated content requests and eviction/restart. Unaffected messages remain searchable and
the permanently skipped message is not reopened.

### R27 — P2, new G3 remainder: Return usable method states after an empty rebuild

**Current spec:** 1010–1026, 1051–1057, 1333–1356 and 1586–1595.

An empty rebuild now allocates a fresh epoch through mailbox property changes. Comparing
the Email group's counter lets validation succeed, but does not advance the Email item
state or the separate Thread log that the methods return.

If the last messages were deleted at state 150 and the empty rebuild sets epoch 200,
`last_change_id` can be 200 while `Email/get` and `Thread/get` still return states below
200. A client resyncs successfully, then `/changes` rejects that freshly returned state
as pre-epoch. Repeating the full resync returns the same unusable token until mail activity
causes a publication. The assertion that every boundary touches each method's log does not
hold for the mailbox-only branch.

[Email state](../../../crates/jmap/src/changes/state.rs), lines 132–139, uses
`emails.change_id`; [cache application](../../../crates/email/src/cache/mod.rs),
lines 449–483, leaves that value unchanged for mailbox-only changes.
[Thread/get](../../../crates/jmap/src/thread/get.rs), lines 80–85, reads the Thread log.

**Required change:** define boundary states that all affected methods can actually return,
and keep their returned tokens consistent with epoch admission, including empty accounts.

**Regression:** lose the index after the last message is deleted. A fresh Email and Thread
resync must each support an empty `/changes` round trip without new mail, while pre-loss
states remain refused.

### R28 — P2, new G13 interaction: Preserve Initial semantics on continuation pages

**Current spec:** 1569–1585.

The new Initial rule forces every candidate absent at `m`, so surviving candidates are all
created. An ordinary `Exact(m)` range retains their real existence and may classify them
as updated. Both routes use the same intermediate-state shape, and continuation recomputes
the ordinary range without retaining which rule produced its first page.

Let A and B predate m and have updates at `m + 1` and `m + 2`. With `maxChanges = 1`,
Initial's first page returns created A and must continue with created B. Exact(m)'s first
page returns updated A and must continue with updated B. Both have
`Intermediate{from: m, to: E, items_sent: 1}`. The specified continuation cannot distinguish
them and changes Initial's second item from created to updated.

[The state type](../../../crates/jmap-proto/src/types/state.rs), lines 18–30, has no mode
field. Upstream Initial reads `Query::All` without imposing the all-created rule
([changes/get.rs](../../../crates/jmap/src/changes/get.rs), lines 315–334), and
[page classification](../../../crates/jmap/src/changes/page.rs), lines 383–395, preserves
the change kinds.

**Required change:** preserve upstream classification, or explicitly carry Initial mode
through every continuation. A pinned range alone does not encode the new endpoint override.

**Regression:** page Initial and Exact(m) over preexisting updated messages; each complete
paged result must equal its own unpaged result, including after reload between pages.

### Re-review evidence and limits

Three focused read-only reviewers examined privacy/representation, protocol/history, and
persistence/recovery. The primary checked their scenarios against the current spec and
relevant source. Reviewers retained the inherited parent model route because the originating
identity/capability ceiling remains unresolved; no stronger route was assumed.

One proposed pruning finding was not retained: consecutive publication revisions and prefix
pruning let the first retained row `(r, C)` supply the base revision `r - 1` for a servable
state before C. The stored data therefore suffices to evaluate the historical rule; this
review does not classify the absence of that implementation detail as lost history.

Only the spec and Fable report changed in tracked files since the prior reviewed HEAD;
the implementation and repository guidance are unchanged. No builds, tests, servers,
benchmarks, network requests or runtime probes ran. Only this report was edited. Fresh
verification checks the exact HEAD/spec hash, local links, finding counts, formatting,
unchanged historical report content and working-tree scope. Addressed findings are
design-level conclusions, not implementation validation.

This section supersedes the earlier reviews below, whose original references remain as
history.

## Historical review — revisions 11 and 12, 2026-11-06

Reviewed the revised [spec](2026-11-04-zero-access-search-design.md), including revision
11's responses to the [Fable review](2026-11-04-zero-access-search-design-fable-review.md)
and revision 12's reconfirmed padding decision, against this exact baseline:

- Worktree: `/Users/jay/Development/stalwart/.worktrees/za-search-design`.
- HEAD: `fc509f9edd94efe4e197826fccb814c87678fc9b`, including the merged v1.0.3 fork.
- Spec SHA-256: `e9ecd8287b76dd06e346fd185cb89b7976d2e35f0de007892e8df6013ab84c0a`.

R18 is addressed. R16 and R19 each retain a narrower failure case. The revision also
introduces concrete cursor, content-recovery and historical-paging inconsistencies, and
the padding argument still overstates the protection against an active operator.
There are **eight current findings: two P1 and six P2**. These are two carried-forward
findings and six additional scenarios, mostly interactions within the existing problem
areas, rather than eight new architectural requirements. All spec references below use
revision 12.

### Disposition of earlier findings

| Finding | Current disposition | Evidence or remaining case |
|---|---|---|
| R16 | Partly addressed | Cursor retention, 666–677; compaction still discards it, 955–962. |
| R18 | Addressed in design | Full manifest-hash pairing, 540–558 and 707–718. |
| R19 | Partly addressed | Marker fences existing manifests; removal reopens `None` creation. |
| F1 | Partly addressed | Reference bound added; total-byte bound and active inference remain. |
| F2 | Addressed in design | State covers the newest publication and epoch, 1147–1168. |
| F3 | Addressed in design | Explicit loss/transient split, 693–705. |
| F4 | Partly addressed | Update-only writes removed; skipped cursors can stall, R22. |
| F5 | Addressed in design | Separate pass/read locks, 567–572. |
| F6 | Partly addressed | Chain bounded; durable content coverage and recovery missing, R23. |
| F7 | Addressed in design | Newer format refuses without rebuilding, 870–877. |
| F8 | Addressed in design | Explicit method table includes keyed `Mailbox/get`, 1212–1234. |
| F9 | Partly addressed | Reconciliation timestamps erase retained lifecycle facts, R24. |
| F10 | Addressed in design | Unfolded lifecycle overlay, 1314–1329. |
| F14 | Partly addressed | Historical membership formula needs its pinned upper bound, R25. |

The accepted choice to expose coarse size classes is not itself a finding. R20 checks
the promised bound; R21 identifies an inference missing from the stated residual.
Earlier accepted parity tradeoffs remain separate. This review does not change the spec
or approve implementation.

### R20 — P1, new: Bound every contribution to the thread blob before claiming the margin

**Current spec:** 190–210, 318–324, 382–413, 754–763 and 2327–2333.

The privacy argument treats `C · R` as the maximum non-merge payload and derives a minimum
observable merge size from it. The actual pass admits bytes outside that bound:

- `C` is a soft insert cap: records must be consumed whole. With the default cap of 64,
  63 singleton inserts followed by a 32-message grouped record produce 95 inserts.
  [group.rs](../../../crates/email/src/message/ingest/group.rs), line 17, sets the default
  group limit to 32; lines 130–152 commit the group in one batch. This already invalidates
  the stated `C · R` maximum and the promised one-third margin.
- The same blob contains tombstones, but deletions consume neither the insert count nor
  opened-message byte budget. A deletion backlog can put the non-merge payload much
  closer to a padding boundary than the inserts-only formula allows.
- A changed record carries its retained reassignment history. Bounding references to 16
  does not bound that history to the small number implicit in the estimated 350 bytes.
  Retention limits history in principle, but the design must derive its worst-case
  serialized size; a maximum measured on the benchmark corpus is not that bound.

Thus the accepted premise that the operator cannot place a baseline payload near a
boundary, and the resulting minimum number of reassigned records, do not follow from
the specified algorithm. Correcting the default arithmetic alone is insufficient.

**Required change:** establish an enforced byte bound covering whole-record overshoot,
tombstones, retained histories, revision rows and framing. Derive the padding margin and
merge threshold from that bound, or narrow the guarantee to what the actual construction
provides. Preserve whole-record checkpoint correctness while making the bound explicit.

**Regression:** exercise maximal grouped overshoot, deletion-heavy passes and long retained
histories, with and without a small merge. Verify the claimed worst-case byte inequality,
not only representative corpus sizes.

### R21 — P1, new: Account for chosen-message inference through amplified merges

**Current spec:** 200–210, 382–385, 398–413, 1097–1110 and 2327–2347.

Even a correct hard bound for R20 does not justify "never which messages." The threat
model already permits an operator to deliver chosen messages and observe blob classes.
Suppose it knows two existing Message-IDs, A and B, with the same normalized subject, but
does not know whether they share a thread. It can prepare this probe over ordinary passes:

1. Deliver a large helper cohort referencing A only, and a disjoint cohort referencing
   B only. Each helper has a unique own ID; unique dummy references within `K` can keep
   the record hashes largely incompressible. Neither cohort references the other target,
   so preparation does not join two previously separate target threads.
2. Allow both cohorts to be indexed. Make each large enough that rewriting its records
   exceeds the minimum padding class by a comfortable margin.
3. On a non-compaction pass, deliver one bridge referencing A and B.

If A and B already shared a thread, the bridge adds its own record. If they were separate,
the assignment rule merges them and rewrites at least the smaller helper-filled thread.
The visible class distinguishes these cases. The observation answers a membership
question about the selected pair, despite revealing only a coarse size for the merge.
This does not require defeating the corrected per-pass insert bound.

**Required change:** include this chosen-message membership inference in the active-operator
residual and the privacy decision, or change the construction to prevent amplification.
Acceptance of an unidentified large-merge count does not establish acceptance of this
stronger inference under the current "never which messages" wording.

**Regression:** compare two otherwise equivalent prepared accounts where A/B start joined
versus separate; index the same helpers and bridge, and compare operator-visible classes.
If retained as an accepted residual, document the distinguishability explicitly.

### R16 — P2, remaining: Preserve pending-cursor evidence through compaction

**Current spec:** 666–677, 901–913, 955–962 and 1401–1406.

Keeping a tombstoned message's fields until the cursor finishes fixes the previous N/D/A
example before compaction. The compaction rule still drops the entire deleted record as
soon as its deletion ages out of the retained log, even while that cursor is pending.

In the earlier example, a rebuild indexes N and D together while A remains pending;
D's fields are the only connection from A to N. Delete D, then keep the cursor pending
through enough new logged work for D's deletion to expire. Thread compaction removes D's
record. After reload, indexing A cannot reconnect it to N. The discarded evidence was
already indexed, so the accepted never-indexed-message parity exception does not cover it.

**Required change:** retain the threading evidence needed by pending cursor work across
compaction and reload, even when its sync-history retention obligation has expired.
Make the whole-record pruning rule agree with the field-retention rule and invariant 56.

**Regression:** extend the existing A/D/N cursor case through deletion-retention expiry,
compaction and cold reload before A is processed; A/N must still group together and D
must remain absent from current reads.

### R19 — P2, remaining: Keep the destroy fence effective after the marker is removed

**Current spec:** 511–514, 879–893, 1016–1032 and 1759–1761.

The marker closes the previously reported existing-manifest race while it is present.
However, a first-use rebuild can read an absent manifest, prepare its first batch with
`AssertValue::None`, and pause. Destroy then writes the marker, finishes its inventory and
deletes the Property range, including that marker. The paused batch resumes after destroy
finishes: absence is true again, so it can recreate the manifest and boundary records.

[RocksDB assertion handling](../../../crates/store/src/backend/rocksdb/write.rs),
lines 355–365, tests current absence; it remembers no intervening marker.
[Account destruction](../../../crates/store/src/dispatch/store.rs), lines 450–468,
deletes Property independently. The specified first rebuild batch is sufficient to
recreate index metadata; no later pass or successful blob publication is needed to
contradict the cleanup contract.

**Required change:** make absent-manifest creation depend atomically on durable account
liveness, or retain a destruction fence outside the range that cleanup removes. The
fence must cover pre-admitted requests after destruction has fully completed.

**Regression:** pause an absent-manifest rebuild before its first commit; complete destroy
including marker removal, then resume. Require the commit to fail and no account/index
data to reappear. The current test resumes while the marker still fences it.

### R22 — P2, new F4 interaction: Commit cursor progress when every item is skipped

**Current spec:** 679–691, 693–705, 729–736, 754–775 and 901–909.

Cursor items use the insert path. Missing or corrupt messages are skipped, but a pass
whose inserts were all skipped writes nothing and leaves the manifest alone. All cursor
movement is explicitly commit-only. If every item in the remaining capped cursor batch
has disappeared, an otherwise idle account processes and skips the same items forever.
The pending cursor bypasses the shortcut, and memory-only `scanned_to` tracks log changes,
not durable cursor advancement. A rebuild therefore cannot finish and search keeps refusing.

**Required change:** distinguish no record changes from no checkpoint changes. Persist
cursor advancement/completion in a manifest-only batch when necessary, without a thread
blob or identity publication. Keep the update-only optimization for passes that truly
have no durable progress to record.

**Regression:** remove every remaining cursor item, run the pass, evict/restart, and require
the cursor to finish without creating an otherwise unnecessary blob or log publication.

### R23 — P2, new F6 interaction: Persist content coverage and define content-only recovery

**Current spec:** 488–509, 592–598, 729–740, 783–791 and 911–913.

Content writes stop above the cap while thread indexing and `indexed_to` continue. The
manifest records chain size, but no content-coverage watermark or suspension state. After
restart with a raised cap, or on a node with a higher cap, the chain can be loadable and
the shared checkpoint current even though messages indexed during suspension have no
content records. The metadata does not establish that search is complete.

The proposed remedy also conflicts with the pass: a content-only rebuild keeps the thread
part and uses the same cursor, whose insert path skips a document the index already holds.
Those existing thread records are precisely the messages whose content needs rebuilding.
Following that rule cannot backfill them.

**Required change:** persist enough content coverage/suspension state and cursor mode to
recognize incomplete content across reloads and nodes. Give content-only recovery its own
skip/completion rules based on content coverage; preserve thread identities and refuse
content queries until coverage is current.

**Regression:** suspend content, index more mail, restart with a raised cap, and verify
refusal until backfill completes. Then search both earlier and intervening messages,
with thread identities unchanged. Include a second node with a different configured cap.

### R24 — P2, new F9 interaction: Preserve lifecycle events still in the retained log

**Current spec:** 325–334, 926–942, 1314–1349 and 1378–1387.

Reconciliation stamps every discovered deletion with `oldest_retained - 1`. Being absent
now does not prove a message was absent at every still-servable state: its actual deletion
may remain in the retained suffix. The same distinction applies to discovered creations.

For example, a lagging index is at checkpoint 110. A changes page pins `(155, 165]`, with
mapped D's update at 160 still pending on a later page. D is then deleted at 180 and the
retention floor advances to 150. Reconciliation records D's deletion as 149. The pinned
range has D's update but no lifecycle event, so the endpoint rule falls back to that false
timestamp and now drops D as absent at both endpoints. Yet 155 is above the retention
floor, so the continuation remains valid and must reproduce the same stream.

[Log range reads](../../../crates/store/src/query/log.rs), lines 130–175, preserve the
upper bound; the later deletion at 180 cannot repair the earlier range's raw overlay.

**Required change:** retain/replay actual insertion and deletion timestamps wherever their
records still exist. Use a synthetic pre-retention timestamp only for lifecycle events
proven to precede the retained suffix. Reconciliation must preserve every servable range,
not merely produce the right present document set.

**Regression:** pin the range above, reconcile after the later deletion, and require the
remaining stream items and their order to stay identical, including after reload.

### R25 — P2, new F14 interaction: Reconstruct membership at the pinned upper bound

**Current spec:** 359–361, 1147–1168, 1351–1359 and 1378–1393.

The concrete membership formula starts from a thread's "present members" and reverses
joins/leaves in `(S, E]`. The thread table contains current members, while a continuation's
E can be historical and snapshot validation loads the latest map. Current membership is
therefore not a valid starting set for that formula.

Let T's sole member D exist at S=100 and E=110, with an update at 105 making T a pending
page item. Delete D at 120 before the continuation. Current T is empty; D neither joined
nor left within `(100, 110]`, so the formula adds nothing and incorrectly makes T empty
at S. The pending updated-thread item can change classification. Its retained tombstone
correctly says D existed at both endpoints, but the supplied set formula does not use
that fact to reconstruct its starting membership.

The high-level endpoint requirement at 1388–1390 is correct; the concrete derivation
needs to agree with it. This is a missing reconstruction step, not a new sync guarantee.

**Required change:** construct members as of pinned E, accounting for deletions and merges
after E, before reversing the range to obtain members at S. Define "present" at that
endpoint explicitly rather than using the current derived thread table.

**Regression:** page `Thread/changes` across deletion of a thread's last member and a
post-E merge; the pinned stream and classifications must remain unchanged.

### Re-review evidence and limits

Three focused read-only reviewers checked persistence/recovery, protocol/history, and
privacy/method coverage. The primary checked the reported timelines against the revised
spec and relevant source, and retained only the findings above. Reviewers deliberately
inherited the parent route because the originating model identity/capability ceiling
could not be established from explicit runtime information; no stronger route was assumed.

Since the prior reviewed HEAD, the tracked changes are the revised spec and Fable report;
the implementation baseline and repository guidance are unchanged. No builds, tests,
servers, benchmarks, network requests or runtime probes ran. Only this report was edited.
Fresh report verification covers the spec hash and HEAD, local links, finding counts,
formatting, preserved historical sections and working-tree scope. Addressed findings are
design-level conclusions, not implementation or runtime validation.

This section supersedes the earlier reviews below; their original references remain as
history.

## Historical review — revision 9 consolidation and revision 10, 2026-11-06

Reviewed revision 9's consolidated model, then the revision 10 update that landed during
review. Revision 10 only records the padding decision; it does not change the lifecycle
algorithm. Current [spec](2026-11-04-zero-access-search-design.md) and source baseline:

- Worktree: `/Users/jay/Development/stalwart/.worktrees/za-search-design`.
- HEAD: `6af3c94a9e22d27c3f1f463b42369c6954359f43`, including the merged v1.0.3 fork.
- Spec SHA-256: `e0212028a4be228293090d4832b24f2d5cd9eef74fe5ff01c3b7d9d2309808fc`.
- Initial revision 9 HEAD: `78362eb85874d913a2d75f6b70ee56016c321121`.
- Revision 9 SHA-256: `c6a46f01566a9f987ea32d4a9aa56d137c4be314ced5afe90b81a89da55ac92c`.

The consolidation improves the ordinary catch-up model: the reported R16 sequence works,
R17's cache-dependent deletion skip is removed, and the R14 response overlay remains.
The integrated review found a remaining R16 case during rebuild and two additional
boundary contracts to settle. There are **three current P2 findings; no current P1**.
All line references in this section use revision 10.

### Disposition of revision 8 findings

| ID | Current disposition | Revision 10 evidence |
|---|---|---|
| R16 | Partly addressed | Normal catch-up fixed; the cursor still needs a discard rule. |
| R17 | Addressed in design | Store/log existence and unconditional deletion, 585–623 and 657–680. |
| R15 | Addressed in design | Byte-class prose and framed overhead agree, 382–393 and 1993–2013. |

The spec now records Decision 2 as taken: coarse thread-blob padding with its stated
residual leakage. It also explicitly adds the parity limitation for messages deleted
before initial indexing or before the rebuild cursor reaches them (938–944, 1377–1380).
That limitation is distinct from R16 below, where the deleted message was already indexed.
The design remains marked for reapproval; this review does not approve the new parity
limitation, the earlier R6 IMAP tradeoff, or implementation. Other addressed findings and
the met v1.0.3 prerequisite retain their dispositions.

### R16 — P2, remaining: Retain threading evidence needed by the rebuild cursor

**Current spec:** 596–610, 668–680, 795–804 and 938–944.

Strict log order fixes the original already-indexed A/D then N-insert/D-delete sequence.
The discard rule now says every insert that could need a deleted message's fields has
already been processed. That premise does not hold for the newest-first rebuild cursor,
whose older messages can remain pending after newer messages have been indexed.

Existing A, D and N have increasing document ids and the same normalized subject. A has
Message-ID `<a>`, D has `<d>` and only `In-Reply-To: <a>`, and N has `<n>` and only
`In-Reply-To: <d>`; they form one thread. During a capped rebuild, the cursor indexes N
and D, which join, but has not reached A. D is then deleted through POP3. The next pass
processes that deletion first and drops D's fields, then indexes A from the cursor.
N's remaining hashes contain only `<n>` and `<d>`, so A becomes a separate thread.
The resulting two-thread grouping persists after the rebuild completes.

[ThreadFields::scan](../../../crates/email/src/message/thread.rs), lines 30–62,
supplies those reference hashes; ordinary matching selects the referenced message's
thread in [ingest.rs](../../../crates/email/src/message/ingest.rs), lines 1152–1187.
[POP3 QUIT](../../../crates/pop3/src/op/delete.rs), lines 74–105, can delete D without
running a pass. This case is outside the newly recorded residual: the rebuild cursor
had already indexed D before deletion, so its evidence was available and then discarded.

**Required change:** make evidence retirement account for pending cursor work as well as
the consumed log prefix. Retain the threading fields and define when cursor assignments
may consult them, while keeping deleted messages out of current results. Alternatively,
explicitly broaden the proposed parity exception to this additional loss of evidence.

**Regression:** rebuild the A/D/N chain with a cap leaving A pending after N and D, delete
D, and finish the cursor. Compare A/N's grouping with the plain control and verify D is
absent from current reads. Retain the existing ordinary catch-up ordering test as well.

### R18 — P2, new: Pair the loaded records with the checkpoint used to resume

**Current spec:** 475–478, 511–518, 585–594, 625–640 and 685–702.

The inputs now name the freshly read manifest's checkpoint and cursor. Reloading the
loaded records, however, depends only on `(epoch, revision)`. A deletion-only pass changes
the committed tombstones, blob list and `indexed_to` without changing that pair. Version
equality therefore does not establish that the loaded records cover the read checkpoint.

Two nodes have A and D indexed in one thread at checkpoint 100. Node A consumes D's
deletion at 101 and commits it without publishing a revision. N later arrives at 102,
referencing only D. Node B reads the manifest at 101 but sees its old version pair. If it
uses the freshly read checkpoint with its old loaded records, it skips D's deletion and
threads N through the stale D. Asserting the newly read manifest bytes can succeed, so
the incorrect assignment can become durable.

This is a missing pairing contract, not a claim that every implementation must fail.
Replaying from the loaded map's own checkpoint with its matching manifest assertion is
safe; adopting another node's checkpoint without its records is not. The consolidated
model should require the safe relationship explicitly, including cursor-only changes.

**Required change:** keep loaded records, checkpoint, cursor and asserted manifest from
one committed state. Either reload when that state changes, even without an identity
publication, or retain/replay the loaded checkpoint and assert its matching manifest.
The identity publication version alone is insufficient as the state comparison.

**Regression:** let one node commit a deletion-only pass while another retains the old
same-version map; then process a new message referencing the deleted one on the old node.
Require correct threading and persisted tombstones, including after eviction/reload.

### R19 — P2, new: Fence index publication against account destruction

**Current spec:** 420–429, 484–499, 690–702 and 889–906.

Destroy inventories and unlinks index blobs before deleting the account's Property range.
A pass asserts only the manifest it read. An already admitted keyed pass can therefore
prepare blobs against manifest M, pause while destroy inventories and unlinks M's blobs,
then commit new permanent links while M still exists. Its assertion succeeds. Destroy
then removes the new manifest by range, leaving the new links outside every inventory.

The existing cleanup calls blob destruction at
[destroy_account.rs](../../../crates/services/src/task_manager/tasks/destroy_account.rs),
line 214, and removes account data later at 246–250.
[commit_batch](../../../crates/common/src/storage/transaction.rs), lines 27–29,
does not automatically add an account-liveness assertion. Permanent document links remain
live to blob purge in [write/blob.rs](../../../crates/store/src/write/blob.rs),
lines 455–458, even after the account's properties are gone.

The inherited [upgrade outcome](../plans/2026-10-27-stalwart-1-upgrade-outcome.md),
lines 6151–6155, accepts orphaned ordinary-account index terms in a destroy race.
That does not establish this design's stronger promise that no sealed index blob or
permanent link remains. This finding concerns retention and cleanup, not plaintext access.

**Required change:** establish an atomic destruction/liveness fence that every index
publication checks, including manifest creation during rebuild, before destroy takes its
final inventory. Ensure an in-flight pass cannot publish after that inventory. If the
inherited residual is intentionally extended to these blobs, state and approve that
different cleanup contract explicitly.

**Regression:** pause a keyed pass after staging, let destroy finish its inventory, then
resume the pass before Property deletion. Complete destroy and purge; no index blob or
permanent link may survive. Also cover a rebuild attempting to create an absent manifest.

### Re-review evidence and limits

Reviewed the complete revision 9 diff and the small revision 10 decision update. Two
focused read-only reviewers checked prior findings, and a fresh reviewer independently
checked the integrated model without the earlier findings report. The primary verified
the reported scenarios against current spec and source. Reviewers inherited the model
route because the originating model identity/capability ceiling remains unavailable.

Only the spec changed between the reviewed commits. No builds, tests, servers, benchmarks,
network requests or runtime probes were run. Only this report was edited. Verification
covers the current spec hash and HEAD, local links, formatting, preserved review history
and working-tree scope. This section supersedes the earlier reviews below; their
original references remain as history. Addressed findings are design-level conclusions.

## Historical review — revision 8, 2026-11-06

Re-reviewed [revision 8](2026-11-04-zero-access-search-design.md) in
`/Users/jay/Development/stalwart/.worktrees/za-search-design`, against the merged
v1.0.3 fork at `e331021c748119db5419eff81fb071717c1700d2`.
Spec SHA-256:
`3b41f9e7c7ed335a49727895f72929217b5fb1db4d118980c9034f4fb895779a`.

The reported R14 deletion-loss scenario is addressed, and R15's substantive padding
contract is corrected. Two additional findings concern deletion ordering and the cache
snapshot used by the sweep. A lower-priority consistency cleanup remains under R15.
There are **three current findings: two P2 and one P3; no current P1**.

### Disposition of revision 7 findings

| ID | Current disposition | Revision 8 evidence |
|---|---|---|
| R14 | Addressed in design | Raw lifecycle overlay, 1052–1078; current-presence mask, 947–950. |
| R15 | Substantively addressed | Byte formula and explicit residual, 364–374 and 1865–1878. |

The raw overlay now destroys the old mapped id even when its sealed record lags the
deletion, and the original cancellation case remains covered. R15's message counts are
explicit estimates; accepting size-class leakage remains Decision 2, pending Jay's choice
(1860–1885 and 1933–1935). R6's IMAP `CHANGEDSINCE` exception remains a proposed
compatibility tradeoff. Other addressed findings and the met v1.0.3 prerequisite retain
their dispositions. These statuses describe the design, not an implemented fix.

### R16 — P2, new: Preserve chronological threading evidence across the deletion sweep

**Current spec:** 292–295, 588–617, 859–868 and 1255–1278.

Applying every pending deletion before indexing older inserts can permanently change
thread grouping. Deleted messages lose their threading fields, and assignment considers
only indexed, present candidates, so a later deletion can erase the only reference needed
by an earlier pending insert.

With the same normalized subject, indexed A has Message-ID `<a>` and indexed D has
Message-ID `<d>` with a reference to `<a>`; both belong to thread A. N then arrives with
Message-ID `<n>` and only `In-Reply-To: <d>`. Before the next pass, D is deleted. The new
sweep drops D's fields first. N's hashes no longer intersect any candidate, so N becomes
a singleton instead of joining surviving A. Catch-up does not repair that assignment.

This ordering is reachable without a deletion command first indexing N: the spec runs
no pass for POP3 (661), while
[POP3 QUIT](../../../crates/pop3/src/op/delete.rs), lines 74–105, commits deletions.
Ordinary ingest assigns N through D's matching reference in
[ingest.rs](../../../crates/email/src/message/ingest.rs), lines 1152–1187. Its
[deletion path](../../../crates/email/src/message/delete.rs), lines 93–125 and
424–443, deletes D and logs emptied threads; it does not regroup A or N. The eventual
one-thread versus two-thread result exceeds the specified freshness exception.

**Required change:** separate current-presence tombstones from chronological threading
evidence. Retain and consult D's threading fields until inserts preceding its deletion
have been assigned, with explicit candidate lifetime rules, while continuing to exclude
D from current reads. A future deletion must not alter an older insert's candidate set.

**Regression:** run the A/D/N sequence against a key account and a plain control, delete D
through a path that does not run a pass, and catch up completely. A and N must share the
control's grouping while D remains absent. Repeat with the insert cap between N's insert
and D's deletion.

### R17 — P2, new: Do not checkpoint a deletion skipped against an older cache snapshot

**Current spec:** 563–589, 609–612, 627–632 and 779–792.

The sweep reads through the log's end but ignores a deletion if the message cache still
contains the document. The spec does not bind that presence snapshot to the scan's upper
watermark. A concurrent deletion can therefore be observed in the log and permanently
discarded because the earlier cache snapshot has not seen it.

Start with `indexed_to = 100` and a cache snapshot at 101 that contains indexed D. D's
deletion commits at 102 before log iteration. The pass sees that record, skips its
tombstone because the cache contains D, and commits `indexed_to = 102`. Once the cache
refreshes to 102, step 0 exits because it is not ahead of `indexed_to`; subsequent scans
also start after 102. D never receives its deletion metadata. Current-read masking avoids
immediate resurrection, but ordinary compaction cannot discard the untombstoned thread
record or content, leaving the persisted index wrong until a separate repair.

The cache API returns an `Arc` snapshot, and updates publish a replacement, in
[cache/mod.rs](../../../crates/email/src/cache/mod.rs), lines 59–67 and 169–178.
The current log reader's `Since` range independently extends to `u64::MAX` in
[query/log.rs](../../../crates/store/src/query/log.rs), lines 130–145. Neither API
establishes the missing ordering on behalf of the proposed pass.

**Required change:** bound the sweep by a watermark covered by its presence snapshot,
or refresh/revalidate presence before treating a logged deletion as obsolete. Do not
advance the consumed checkpoint past deletion work deferred for an uncertain snapshot.

**Regression:** commit D's deletion between cache acquisition and log iteration. After
the pass, a cache refresh and an otherwise idle request, require a persisted tombstone;
after retention expiry and compaction, require removal of D's index record and content.

### R15 — P3, remaining: Align the surrounding padding explanations with the formula

**Current spec:** 201–203, 369–372, 1788 and 1875–1877.

The central byte formula and Decision 2 now address the previous P2 finding. Some prose
still contradicts them: 201–203 calls byte classes record-count classes and treats a class
above the minimum as evidence of a large re-threading; 1788 still says every ordinary
merge is hidden. The revised explanation at 194–200 correctly permits one additional
record to cross a boundary, so these older statements should follow it.

The overhead explanation also needs to distinguish the payload from its four-byte
length prefix. For `p = 16381`, the specified `p + 4 = 16385` rounds to 32768, although
the payload alone is below 16 KiB. The stated below-16-KiB shortcut does not apply there.
For padded length B, zero padding is `B - (p + 4)`; framing plus padding is `B - p`.
The four-byte prefix is confirmed by
[seal_padded](../../../crates/groupware/src/calendar/seal/tree.rs), lines 275–281.

**Required change:** use compressed framed bytes consistently, remove the remaining
record-count/ordinary-merge guarantees, and state the minimum-class condition as
`p + 4 <= 16384`. Include payloads 16380 and 16381 in the boundary checks. This is
lower-priority clarification because the normative allocation formula is already correct.

### Re-review evidence and limits

Compared the full revision 8 diff with the surrounding contracts and relevant current
source, including ordinary threading/deletion, POP3 deletion, cache snapshots and log
ranges. Both read-only reviewers' findings were independently checked. Only the spec
changed between revisions 7 and 8. The reviewers retained the inherited model route
because the originating model identity/capability ceiling remains unavailable.

No builds, tests, servers, benchmarks, network requests or runtime probes were run.
Only this report was edited. Verification covers the spec hash and HEAD, report links
and formatting, retained historical sections, and working-tree scope. This section
supersedes the earlier reviews below; their original references remain as history.

## Historical review — revision 7, 2026-11-06

Re-reviewed [revision 7](2026-11-04-zero-access-search-design.md) in
`/Users/jay/Development/stalwart/.worktrees/za-search-design`, against the merged
v1.0.3 fork at `0ddaafb4a9d05926a8f76dd547d5f06ffbea1954`.
Spec SHA-256:
`0fc00f74179bfdf16d0b6716094eb421bd804a5ddcae91be7e9b1103fe2e5645`.

R2 is addressed in the proposed design. R14 fixes the original cancellation case but
introduces a backlog interaction that can permanently omit deletions. R15 now records the
privacy choice explicitly, but its protection and cost claims need correction before that
choice. There are **two current findings: one P1 and one P2**.

### Disposition of revision 6 findings

| ID | Current disposition | Revision 7 evidence |
|---|---|---|
| R2 | Addressed in design | Epoch/revision equality in both paths, 545–550 and 875–894. |
| R14 | Partly addressed | Cancellation fixed; lagging records can still hide deletions. |
| R15 | Partly addressed | Decision 2 states the residual; byte-class claims remain inaccurate. |

The earlier addressed findings retain their dispositions, and the v1.0.3 prerequisite
remains met. R6's IMAP `CHANGEDSINCE` exception remains a proposed compatibility tradeoff.
Decision 2 is explicitly pending Jay's choice (1796–1814 and 1862–1863); this review does
not treat the proposed size-class leakage as approved or as an undisclosed new leak.

### R14 — P1, remaining: Apply raw deletions when the indexed record lags the range end

**Current spec:** 564–565, 573–583, 649–660, 1025–1039 and 1057–1061.

The new boundary-existence rule fixes a message created and deleted within the same
range. For an indexed document, however, it derives existence only from that document's
sealed creation/deletion fields. Those fields can lag the response state: `Email/changes`
runs one capped pass and is explicitly allowed to serve before the backlog is consumed.

The client holds an indexed message D as `T|D` at S. More than one pass of new mail then
arrives, followed by D's deletion. A changes request processes only the first capped
portion, so D's indexed record still has no deletion timestamp. The raw range through the
current state E already includes D's delete. Nevertheless, the rule at 1025–1028 uses the
stale indexed record, concludes D exists at both endpoints, and emits nothing when its
thread is unchanged and it has no raw update. The response advances the client to E.

When a later pass finally tombstones D, it uses the original deletion change id, already
at or below E. That deletion is outside subsequent ranges, so the client retains `T|D`
indefinitely. Version validation does not prevent this: the loaded map can agree with the
manifest while both are behind the physical log's lifecycle changes.

The underlying reader returns the raw records independently of index progress in
[query/log.rs](../../../crates/store/src/query/log.rs), lines 130–166. The message
cache's state advances on item changes in
[cache/mod.rs](../../../crates/email/src/cache/mod.rs), lines 449–462, and supplies
the changes response state in
[changes/get.rs](../../../crates/jmap/src/changes/get.rs), lines 349–364. The missing
step is combining those authoritative lifecycle facts with retained sealed identities.

**Required change:** overlay raw birth/death facts through E when computing existence at
S and E, including for documents already in the index. Alternatively, establish lifecycle
coverage through E before returning the response. Keep historical thread lookup separate
from physical existence so the old mapped id can be destroyed while indexing lags.

**Regression:** index D and retain its client id/state, queue more inserts than the pass
cap, then delete D beyond the first pass. Request and paginate changes before catch-up
finishes, finish catch-up, and compare the accumulated client set with a full query.
`T|D` must be destroyed exactly once. Severity is P1 because this interaction can lose a
deletion permanently, beyond R14's earlier cancellation/parity defect.

### R15 — P2, remaining: Describe padding protection and overhead in bytes

**Current spec:** 181–189, 278–288, 350–355, 1296–1298 and 1801–1807.

The proposed coarse leakage contract is now explicit. Its recommended option still says
roughly 100 reassigned messages fit the smallest class, ordinary merges are hidden, and
padding costs at most 16 KiB per pass. The actual algorithm rounds the compressed payload
to a power of two with a 16 KiB minimum; those claims do not follow from that rule.

Thread records contain variable-length reference hashes and retained transition history.
[ThreadFields::scan](../../../crates/email/src/message/thread.rs), lines 30–62,
collects every distinct Message-ID-family reference into a vector. A small number of
records can therefore exceed 16 KiB, and one additional reassignment can cross a class
boundary. There is no universal message-count threshold below which a merge is hidden.

The overhead also exceeds the stated cap: a length-prefixed compressed payload of
32 KiB + 1 byte rounds to 64 KiB, adding almost 32 KiB. Larger blobs, including compacted
blobs, can add more. The cited
[seal_padded helper](../../../crates/groupware/src/calendar/seal/tree.rs), lines 272–282,
provides the framing example; its existing 256-byte-multiple rounding is not the new
power-of-two policy.

**Required change:** define the exact byte-rounding formula, including the length prefix,
and describe the proportional overhead above the minimum class. State that even a small
merge can cross a class boundary. Keep message-count examples as measured estimates,
rather than privacy guarantees, so Decision 2 accurately describes the proposed tradeoff.

**Regression/review:** test payloads immediately below, at and above byte-class boundaries,
including 32 KiB, and vary reference/history lengths independently of message count.
Replace the universal record-count threshold test with these byte-based checks.

### Re-review evidence and limits

Compared the full revision 7 diff with its surrounding contracts and relevant current
source, and adjudicated both read-only reviewers' findings. Only the spec changed between
the revision 6 and 7 commits. Both reviewers retained the inherited model route because
the originating model identity/capability ceiling remains unavailable.

No builds, tests, servers, benchmarks, network requests or runtime probes were run.
Only this report was edited. Verification covers the spec hash and HEAD, report links
and formatting, retained historical sections, and working-tree scope. All addressed
statuses describe the proposed design only. This section supersedes the earlier reviews
below, whose original line references and conclusions are retained as history.

## Historical review — revision 6, 2026-11-06

Re-reviewed [revision 6](2026-11-04-zero-access-search-design.md) in
`/Users/jay/Development/stalwart/.worktrees/za-search-design`, against the merged
v1.0.3 fork at `07593fab2255d767de11bdb4c24343bdd6e18afe`.
Spec SHA-256:
`4f53b626195fc38b4b7dd62196721a9b98fea95506b096adcd92352d46f9d4a0`.

Seven of the eight revision 5 findings are addressed in the proposed design. R2 remains
open across identity epochs, and two additional findings concern the new stream contract
and the privacy guarantee. There are **three current findings: one P1 and two P2**.
These are design findings, not implementation or runtime results.

### Disposition of revision 5 findings

| ID | Current disposition | Revision 6 evidence |
|---|---|---|
| R2 | Partly addressed | Post-state validation fixes the earlier race within one epoch. |
| R3 | Addressed in design | Fresh commit-stamped boundary, 651–663 and 681–688. |
| R4 | Addressed in design | Indexed-only witness and uniform notifications, 928–959. |
| R7 | Addressed in design | Broken-chain scans in rebuild/destroy, 665–669 and 771–777. |
| R10 | Addressed in design | Pending cursors override the shortcut, 510–517 and 671–679. |
| R11 | Addressed in design | Authenticated self/predecessor segment identities, 335–342. |
| R12 | Addressed in design | Synthetic counts and pinned ranges replace raw paging, 981–1031. |
| R13 | Addressed in design | Deletion-only passes use original deletion IDs, 529–530 and 557–565. |

R1, R5, R8 and R9 retain their addressed dispositions. R6 remains a proposed compatibility
tradeoff awaiting reapproval: IMAP `CHANGEDSINCE` still does not report reassigned object
ids (1181–1184). The v1.0.3 prerequisite remains met. R14 below concerns net lifetime
classification, not the raw-count pagination defect resolved under R12; R15 concerns
length leakage, not the content-selected witness resolved under R4.

### R2 — P1, remaining: Validate the epoch as well as the revision

**Current spec:** 514–516, 651–663 and 840–855.

The post-state manifest read and refusal on read failure fix the previous snapshot race
within an epoch. Rebuild, however, resets `revision`, while both catch-up and response
validation reload only when the manifest's numeric revision is newer. Different identity
epochs can therefore appear to agree.

Node A finishes its preliminary pass with epoch A/revision 1, then pauses before its
handler samples the state. Node B rebuilds, publishes epoch B/revision 0, and completes
one rebuild pass, reaching B/revision 1. A refreshes its message-cache state and reads the
manifest, but its cached revision 1 equals the manifest's revision 1. It validates the old
epoch's map and emits old identities under the new epoch's state. A subsequent changes
request starts after the loss boundary, so epoch rejection cannot repair those identities;
the old history is gone.

The separate handler snapshot and state sampling remain visible in
[email/get.rs](../../../crates/jmap/src/email/get.rs), lines 175–199. The design must
make its new validation reject this equal-counter case; atomic publication by itself
does not establish that the two maps belong to the same history.

**Required change:** validate an epoch-qualified map version. Any epoch difference must
invalidate and reload the map regardless of revision ordering, and changes-state checks
must use that loaded epoch. Apply the same rule to step 0 and response validation.

**Regression:** pause after A's pass, rebuild on B until both maps have the same numeric
revision, then resume A. Its response must use B's map or fail validation. Also cover a
new epoch whose revision is lower than A's cached revision.

### R14 — P2, new: Coalesce a message's lifetime before rendering its changes

**Current spec:** 254–271 and 981–994.

The custom stream unions raw log documents with retained history and deletion records,
then classifies a document as created, deleted, or reassigned. It does not define the
overlap where a message was both created and deleted within the pinned range. For example,
since state 100: insert D at 101, index it at 102, delete it at 103, and request changes
through 103. The stated creation rule emits `T1|D` although D is absent at the range end;
applying both rules instead can place the same singleton id in both result categories.

Upstream cancels an insert followed by a delete in
[query/log.rs](../../../crates/store/src/query/log.rs), lines 387–391. Folding the raw
log first is insufficient by itself: the proposed union can reintroduce the canceled
document from its retained history or deletion record. The revised renderer needs an
explicit net-lifetime rule to preserve the ordinary-account result it promises.

**Required change:** determine existence at S and E before selecting created, destroyed,
updated or identity-replacement output. A document absent at both boundaries must not
become a created result. Preserve enough birth/death or folded-log information to exclude
canceled documents even when sealed records add them to the candidate union.

**Regression:** create, index and delete both a singleton and a threaded message within
one range; compare with the plain account's folded result. Repeat with small pages and a
later publication between pages, ensuring the canceled document does not reappear.

### R15 — P2, new: Reconcile thread-blob size leakage with the privacy guarantee

**Current spec:** 154–179, 319–327, 406, 955–957 and 1443–1452.

The spec now promises that the pass's observable writes depend only on the keyless log
and message cache, whatever the mail says. However, a thread blob contains every added
or changed thread record, is compressed and sealed without specified padding, and exposes
its size in the manifest. One incoming bridge that reassigns 100 existing messages writes
roughly 101 records; an otherwise comparable message that starts a new thread writes one.
Those lengths can reveal information about whether a merge occurred and its volume,
despite identical notification entries.

[vault/keys.rs](../../../crates/vault/src/keys.rs), lines 200–216, seals the supplied
bytes with nonce and authentication overhead; it does not hide their length. Section 2
already acknowledges visible thread-blob sizes, so this is not a claim that lengths or
exact member identities are newly exposed. The unresolved contract is the stronger claim
that the observable writes reveal nothing dependent on reassignment.

**Required change:** explicitly accept and describe inference from thread-blob lengths,
narrowing the write-pattern guarantee, or specify a padding/update policy that supports
the stronger guarantee. Treat this as a privacy decision in the spec's reapproval.

**Regression/review:** extend the bridge-versus-no-merge comparison at 1369–1373 to include
thread-blob lengths and manifest writes, alongside the log entries. Check the resulting
observables against the chosen leakage contract.

### Re-review evidence and limits

The revised contracts and both read-only reviewers' reports were checked against the
current spec and relevant source. Only the spec changed between the revision 5 and 6
commits. The two reviewers retained their inherited model route because the originating
model identity/capability ceiling remains unavailable; no stronger route was guessed.

No builds, tests, servers, benchmarks, network requests or runtime probes were run.
Only this report was edited. Verification covers the exact spec hash and HEAD, report
links and formatting, preserved review history, and working-tree scope. All addressed
statuses describe the proposed design only. The earlier reviews below retain their
original line references and conclusions as history; this section supersedes them.

## Historical review — revision 5, 2026-11-06

Re-reviewed [revision 5](2026-11-04-zero-access-search-design.md) against the merged
v1.0.3 fork in the same `za-search-design` worktree, at
`ee904b36376160f2488c4b421d323c56afb29ace`.
Spec SHA-256:
`969eb2b4575dc5481b6ba6bcef8fb91705532b7a6459bd34d4191007bd3a4815`.

The v1.0.3 prerequisite is met. Four original findings are addressed in the proposed
design, four remain partly open, and one is now an explicit contract tradeoff awaiting
the spec's reapproval. Four additional findings concern the revised algorithms. There are
**eight current findings: five P1 and three P2**. These are source-backed design findings;
no implementation or runtime qualification is implied.

### Disposition of revision 3 findings

| ID | Current disposition | Reason |
|---|---|---|
| R1 | Addressed in design | Record-preserving reader and whole-record checkpoints, 504–531. |
| R2 | Partly addressed | Commit stamps fix backdating; response snapshots still race. |
| R3 | Partly addressed | Deleted history retained; epoch misses the latest old state. |
| R4 | Partly addressed | One witness still reveals a reassigned member. |
| R5 | Addressed in design | Correct lookup and coalescing, 264–269 and 909–916. |
| R6 | Contract narrowed | Mailbox notifications added; IMAP modseq limitation retained. |
| R7 | Partly addressed | Links now commit atomically; broken-chain recovery still strands links. |
| R8 | Addressed in design | Thread compaction and a single content head bound manifest size. |
| R9 | Addressed in design | Thread/content blobs are separately sealed and independently loaded. |

R6 is **not a modseq fix**: lines 898–902, 1091–1094 and 1470–1472 explicitly retain
`CHANGEDSINCE`'s failure to report reassigned `EMAILID`/`THREADID`. The mailbox-state part
is corrected at 889–894. Revision 5 is marked "for re-approval", so this report records
the IMAP behavior as a proposed compatibility tradeoff, not an already approved exception.
The content chain's unbounded growth above the content cap is also now explicit; R8's
original unbounded-manifest finding is addressed, independently of R11 below.

### R2 — P1, remaining: Fence the response state to the identity-map snapshot

**Current spec:** 498–503, 569–572, 800–804 and 1127–1130.

Publication revisions fix the original backdated-history problem. However, reading the
response state before mapping ids does not ensure the loaded map covers that state.
Node A can finish its preliminary pass at state/map 100; node B then publishes a
reassignment at 101 and invalidates A's message cache. A's handler reads refreshed Email
state 101 but maps through its still-loaded revision from 100. The client receives old
`T0|D` under state 101. Its next `Email/changes(since=101)` can load the new map, but
neither the raw carrier nor the transition is after 101: the old id is never destroyed.

The current handler obtains its own message snapshot at
[email/get.rs](../../../crates/jmap/src/email/get.rs), lines 175–178, and derives its
response state from that snapshot at 197–199. The message cache advances independently
from item-log updates in [cache/mod.rs](../../../crates/email/src/cache/mod.rs),
lines 449–462. [changes/get.rs](../../../crates/jmap/src/changes/get.rs), lines 364–371,
can immediately return when the supplied state equals the current state. The explicit
policy of serving an older loaded map after a manifest-read failure permits the same
mismatch even without the narrow race.

**Required change:** acquire or validate a coherent message-state and identity-map
snapshot after the preliminary pass. Every returned state must cover only identity
transitions represented by that map; define retry/refusal when it cannot catch up.
The "state before map" ordering alone does not establish this invariant.

**Regression:** pause between the preliminary pass and handler snapshot, publish on a
second node, resume, and apply changes from the returned state. Repeat with a failed
manifest read and an older loaded map. The resulting client id set must equal a full query.

### R3 — P1, remaining: Give identity loss a fresh publication boundary

**Current spec:** 628–633 and 645–652.

Rebuild copies the existing current Email change id C into `epoch`, then rejects only
states below C. A fully synchronized client already holds state C and the lost map's
`T|D` identities. Its `Email/changes(since=C)` passes the epoch check even though those
identities can no longer be reconstructed. New rebuild history starts from `D|D` and cannot
tell that client to remove `T|D`. Repeated losses without intervening mail changes can
also reuse the same boundary.

The current state is returned by
[email/get.rs](../../../crates/jmap/src/email/get.rs), lines 197–199. Exact-state
handling and the equality return are in
[changes/get.rs](../../../crates/jmap/src/changes/get.rs), lines 336–371; its log-retention
checks at 424–433 cannot detect lost sealed history.

**Required change:** atomically publish a fresh identity-loss boundary newer than all
previously issued states, advancing the affected sync states with the replacement
manifest; alternatively encode an identity epoch in those states. Changing `<` to `<=`
alone would also reject freshly issued states at the reused boundary.

**Regression:** retain the latest pre-loss state, lose the index, and require an explicit
resync. Verify newly issued resync states work, and repeat loss without intervening mail
activity. The current planned case at 1202–1205 only promises rejection of "older" states.

### R4 — P1, remaining: Select the witness without consulting reassigned membership

**Current spec:** 154–156, 176–179, 878–881 and 896–898.

The witness is the lowest id among documents inserted **or reassigned**. In a pass with
one newly delivered bridge N and an older losing-thread member D where D < N, the logged
witness is an old reassigned document, not the new bridge. An observer who knows N was the
only insertion can infer that the old witness joined N's resulting thread. Reducing the
cohort to one content-selected member narrows the leak but does not fulfill the unchanged
no-membership-from-write-pattern guarantee.

This follows directly from the revised selection rule and the operator-controlled
delivery case in section 12. The witness's singleton id remains visible through
[write/log.rs](../../../crates/store/src/write/log.rs), lines 392–422. The assertion
at 896 that nothing names reassigned messages is false for this case.

**Required change:** choose a notification witness only from content-independent
consumed log inputs, or use a fixed notification object. Its selection must not depend
on the reassigned set. This finding does not repeat the now-explicit mailbox-notification
tradeoff.

**Regression:** deliver one bridge whose losing-thread members have lower ids, enumerate
the keyless writes, and prove witness selection is independent of those members. The
planned test at 1177–1180 cannot pass its losing-members-absent condition as written.

### R10 — P1, new: Pending rebuild work must bypass the no-work shortcut

**Current spec:** 498–503, 543–547 and 628–643.

Step 0 ends the pass whenever the message cache is not ahead of `indexed_to`. Rebuild
initialization sets `indexed_to` to that same current cache change id while leaving a
cursor of existing mail. Therefore the next pass can exit without touching the cursor.
Even if initialization forces one batch through, a multi-pass rebuild of unrelated
singleton messages writes no publication: the cache state remains unchanged and every
later request takes the same early return. With no new deliveries or flag changes, content
search stays unavailable indefinitely.

**Required change:** make a pending rebuild/reconciliation cursor take precedence over
the equality fast path. The no-I/O shortcut must establish both consumed-log freshness
and absence of cursor work, not just compare change ids.

**Regression:** rebuild more than one pass of unrelated messages while the mailbox is
otherwise idle, including a restart between passes, and require the cursor to finish.

### R11 — P1, new: Carry the authentication identity of every content-chain node

**Current spec:** 231–246, 335–337 and 396–399.

Opening a blob requires its random segment id in the AAD. The manifest retains that id
for the content head only, and each predecessor descriptor carries just a hash. The
prescribed clear header contains format, kind and predecessor hash, but not the blob's
own segment id. After reading the head and fetching its predecessor by hash, the loader
has no segment id with which to authenticate that predecessor. Any accidental association
with a contemporaneous thread blob would also disappear after thread compaction.

[vault/keys.rs](../../../crates/vault/src/keys.rs), lines 219–234, authenticates using
the exact supplied AAD. The missing random id cannot be derived from the content hash.
Separate sealing resolves R9, but this new chain format cannot reload older content.

**Required change:** include each blob's segment id in its authenticated clear header, or
carry authenticated `{hash, segment_id}` predecessor descriptors. Specify enough framing
to construct every node's AAD without retained process memory.

**Regression:** write multiple content blobs, compact the thread blobs, evict the cache,
then reload and search every content-chain node.

### R7 — P2, remaining: Scan links when a content chain cannot be traversed completely

**Current spec:** 628–635 and 730–737.

Atomic staging/publication fixes the original interruption gaps. The new cleanup fallback,
however, only scans principal links when the manifest is missing or unparseable. For a
valid manifest naming `C3 -> C2 -> C1`, remove C2's blob. Recovery can identify C3 and C2,
but cannot read C2's header to discover C1. It then publishes the replacement manifest
after unlinking only the blobs it can name. C1's permanent link is abandoned; later
destruction sees a valid replacement manifest and never scans for it.

The current purge still considers every document link live:
[write/blob.rs](../../../crates/store/src/write/blob.rs), lines 455–458. The planned
enumerator extension at 372–377 provides a way to find the link, but the trigger does
not cover this supported missing-blob recovery case.

**Required change:** invoke an account-filtered principal-link scan whenever traversal
is incomplete, including missing or corrupt middle nodes, before discarding the old
inventory. Define the same fallback for destruction.

**Regression:** delete/corrupt a middle content blob, rebuild, destroy and purge; no older
content blob or permanent link may remain. Head-loss and malformed-manifest cases alone
do not cover this path.

### R12 — P2, new: Paginate the synthesized changes, including intermediate states

**Current spec:** 904–942; relevant acceptance cases at 1168–1182.

The renderer treats S as a scalar change id and unions raw changes with sealed history.
Upstream pagination instead uses `Intermediate { from_id, to_id, items_sent }` and tests
whether a range is exhausted using its raw change count before rendering. One raw carrier
can now represent hundreds of identity replacements, so those counts no longer agree.

For example, a first synthetic page sends two changes from a range with one raw carrier.
The next request compares `items_sent = 2` with raw total 1 and advances past the range,
dropping the remaining replacements. Expanding only after upstream paging instead makes
the first page exceed `maxChanges`. The spec also needs a stable historical upper bound
when a second publication arrives between pages.

See [types/state.rs](../../../crates/jmap-proto/src/types/state.rs), lines 18–30 and
95–99; [changes/get.rs](../../../crates/jmap/src/changes/get.rs), lines 393–419 and
596–600; and [changes/page.rs](../../../crates/jmap/src/changes/page.rs), lines 230–253.
The raw-count fast path occurs before the new rendering would ordinarily run.

**Required change:** define a deterministic synthetic stream whose count, order, pinned
upper revision and continuation cursor agree. Adapt range exhaustion and epoch checks
for intermediate states, and ensure `Email/queryChanges` receives the required full delta.

**Regression:** paginate a large merge with very small `maxChanges`, publish another merge
and delete a message between pages, and compare the reconstructed client set with a query.

### R13 — P2, new: Define publication when a pass contains only deletions

**Current spec:** 543–554 and 878–894.

A pass that tombstones a message is publishing even if no thread assignment changes.
Its required carrier is nevertheless chosen only from inserted or reassigned documents,
an empty set in a deletion-only pass. The Thread property change needs the same carrier,
and no mailbox update provides a fallback because no assignment changed. This includes
deleting the account's last message, where another live document cannot be chosen.

The revision table requires a publication change id. Its specified patch operation reads
the batch's current change id in
[write/mod.rs](../../../crates/store/src/write/mod.rs), lines 1685–1687; merely appending
a tombstone to an encrypted blob does not define the required Email/Thread publication.
The algorithm currently has no valid choice for its mandatory log writes in this case.

**Required change:** define deletion-only publication, including its revision/change-id
allocation and notification carrier or alternative state mechanism. It must work for an
empty account and retain the history needed for later deletion rendering.

**Regression:** process only a deletion, then delete the last message in an account; verify
publication, reload, and changes from retained states without inventing a live carrier.

### Re-review evidence and limits

The complete revision and both reviewers' findings were checked against current source.
Two read-only reviewers retained their inherited model route; the originating model
identity/capability ceiling remains unavailable, so no stronger route was guessed.
No builds, tests, servers, benchmarks, network requests or runtime probes were run for
this re-review. Only this report was edited; the spec and separate upstream work were
left untouched. Report verification covers links, whitespace, the exact spec hash and
working-tree scope. All "addressed" statuses describe the proposed design only.

The revision 3 report below is retained as history. Its verdicts, source baseline and
line references belong to that earlier snapshot and are superseded by the dispositions
above for revision 5.

## Historical review — revision 3, 2026-11-05

Reviewed on 2026-11-05. Scope: revision 3 of
[the design](2026-11-04-zero-access-search-design.md), including decision 1.

Reviewed spec SHA-256:
`3e7d46b53b1e52cc1555dcf5b88c81134407c5428717242934c3b05abded5037`.
The reviewed bytes were committed during this review as
`cfc08d3fecf0062c9fc3abc20684a5c68f8b4e1d` on `za-search-design`.
Source references below name this worktree's v1.0.1-based fork unless marked v1.0.3.

Nine findings: four P1 and five P2. These are defects in the proposed contracts, not claims
about an implementation. Resolve them before implementing the affected parts.

The separately managed v1.0.3 merge is an accepted prerequisite, not a finding. Direct reads
of upstream v1.0.3 confirmed the two prerequisites in the spec:
[Email/get's prefix-aware lookup][upstream-get] and
[collapsed Email/queryChanges' touched-thread expansion][upstream-query-changes].
The relevant grouped-ingest, change-log, modseq, mailbox-state, and blob-purge behaviors were
also checked against v1.0.3. The final merged fork still needs its integration review.

## R1 — P1: Checkpoint complete log records, or retain an intra-record cursor

**Spec:** section 5.1, lines 405–421.

The pass caps the number/bytes of inserts and advances `indexed_to` to the last processed
insert's change id. Multiple inserts can share that id. With 63 individual inserts followed
by a two-message grouped commit at C, the default 64-message cap stops after its first
message. Publishing `indexed_to = C` makes the next `Query::Since(C)` skip the second
message permanently. Later searches can report a current index while silently omitting it.

The specified `store.changes` API also discards each item's source change id: `Changes`
contains a flattened `Vec<Change>` plus range-level bounds, and the reader folds records
together. See [query/log.rs](../../../crates/store/src/query/log.rs), lines 25–78 and
130–183; this remains true in [v1.0.3][upstream-log]. Grouped ingestion writes multiple
pending messages in one batch in
[ingest/group.rs](../../../crates/email/src/message/ingest/group.rs), lines 130–152
([v1.0.3 equivalent][upstream-group]).

**Required change:** specify an ordered reader that retains physical record boundaries and
either checkpoints only complete records or persists progress within a record. Define how
a single record or message larger than a configured pass limit makes progress.

**Regression:** cross both pass limits inside grouped commits, restart between passes, and
prove every message is eventually indexed exactly once.

## R2 — P1: Version identity transitions by publication, not consumed mail

**Spec:** sections 5.1, 5.2 and 6.3, lines 420–432, 469–494 and 726–734.

Stamping a transition with the consumed `to` cannot describe which identity clients actually
saw. For example, 200 arrivals await indexing. One capped request processes the first 64,
then returns an unindexed message D as `D|D` with Email state 200. A later pass assigns D to
thread T, stamps that transition at a consumed change id at or below 200, and publishes its
update after 200. Section 6.3 explicitly calls a client state between the stamp and the
publication an ordinary update: `updated: T|D`. It never destroys the client's `D|D`.
Refetching the new id cannot tell the client to remove the old immutable id.

This scenario follows the spec's accepted backlog behavior without requiring a race.
[Email/get](../../../crates/jmap/src/email/get.rs), lines 197–199, returns the current
Email state. Upstream's actual identity transition records deletion and insertion in
[index/build.rs](../../../crates/common/src/storage/index/build.rs), lines 415–425.

**Required change:** bind the transition to its publication revision, or define a separate
mapped-identity revision and consistent response snapshot. Explicitly solve obtaining that
revision before sealing; choosing an earlier known watermark does not solve the problem.
This remains necessary after correcting R5's historical lookup.

**Regression:** fetch beyond a capped pass, retain the returned ids/state, complete catch-up,
apply `Email/changes`, and compare the resulting client id set with `Email/query`.

## R3 — P1: Preserve deleted identities and invalidate states when history is lost

**Spec:** sections 4.1, 5.4, 5.5, 6.3 and 8, lines 220–259, 517–532, 550–556,
726–746 and 908–915.

The model retains records for present messages and drops tombstones at compaction, but
changes rendering needs deleted messages' old mapped identities until their deletion leaves
the retained log. If a client knows `T|D`, D is deleted, and compaction removes its record,
the remaining clear log contains only `D|D`. A changes request from a still-valid pre-delete
state can no longer identify the object the client must remove. Retaining only live
messages' transition history is insufficient.

Rebuilds have the same problem on a larger scale: losing the manifest or a segment discards
previously published identities and their history. The promised "one round of reassignments"
cannot reconstruct destroyed ids that no remaining data records. Section 8 also permits
singleton ids after a transient manifest-read failure with no loaded map, potentially
changing identities under an unchanged Email state and without any committed transition.

The stored id is formed from its stored prefix in
[write/log.rs](../../../crates/store/src/write/log.rs), lines 329–333; retained deltas are
read by [query/log.rs](../../../crates/store/src/query/log.rs), lines 130–183. Neither
can recover a lost sealed prefix. The normal changes refusal checks log retention, not
sealed-history retention: [changes/get.rs](../../../crates/jmap/src/changes/get.rs),
lines 424–433.

**Required change:** retain sealed deletion identities and the needed history until the
corresponding sync history expires. When recovery loses that information, invalidate the
old mapped sync states and require resynchronization. Specify a failure policy that does
not silently substitute singleton identities under existing states.

**Regressions:** delete a merged message, compact and reload, then request changes from a
retained pre-delete state. Separately lose a manifest/segment, and fail a manifest read after
eviction; verify old states are explicitly invalidated when correct history is unavailable.

## R4 — P1: Reassignment batches reveal grouping despite the no-membership guarantee

**Spec:** sections 2, 6.1 and 6.3, lines 151–163, 620–623 and 714–725.

Section 2 acknowledges that individual reassignments are visible but promises that thread
membership never leaves memory unsealed. The proposed publication reveals more: a bridge
message joining two established threads causes clear updates for every member of the losing
thread, together in the reassignment batch. An observer can infer that those document ids
now belong together, even without learning the winning numeric thread id.

This is observable under the spec's own operator-controlled-delivery model. Arrange one
bridge to be indexed at a time and inspect the resulting clear update cohort. Using the
same entry shape as a flag update does not hide the grouping when the operator controls
the trigger and observes the batch. The clear item ids and container changes are explicit
in [write/log.rs](../../../crates/store/src/write/log.rs), lines 392–422.

**Required change:** either amend the threat model to explicitly accept this inference of
thread membership, or change notifications so they do not identify just the reassigned
documents. This requires a design decision; it does not challenge decision 1 about keyless
sessions. The current absolute guarantee and the specified observable writes cannot both
hold.

**Review case:** two multi-message threads and one chosen bridge; enumerate all keyless
writes and the membership an observer can infer, then compare that with the approved
leakage contract. A plaintext-canary scan alone cannot establish this guarantee.

## R5 — P2: Reverse the lookup direction for history containing previous values

**Spec:** section 6.3, lines 725–729; record definition at lines 225–233.

The history stores `(transition, thread_before)`, but the lookup chooses the last entry at
or below S. For A→B at 10 and B→C at 20, the records are `[(10,A), (20,B)]`.
At state 15, the correct thread is B; the specified lookup returns A. After 20 it selects B
rather than C. This produces incorrect destroyed ids independently of the timestamp issue
in R2.

**Required change:** with before-values, use the earliest transition strictly after S,
otherwise the current value. Define the initial unindexed value and how multiple transitions
within one publication are coalesced. Alternatively change the record representation and
state the matching lookup rule consistently.

**Regression:** two successive merges, with changes requested before, exactly at, between,
and after the publication revisions.

## R6 — P2: Publish message modseq and mailbox-state changes with reassignment

**Spec:** section 6.3, lines 714–724; invariant 53, lines 1092–1094.

The spec says `log_item_update` bumps the message's modseq, but it only appends a log entry:
[write/batch.rs](../../../crates/store/src/write/batch.rs), lines 396–410. Cache reload
still reads `data.change_id` from the unchanged archive:
[cache/email.rs](../../../crates/email/src/cache/email.rs), lines 112–134. That value
advances when the row is serialized with `PatchSource::ChangeIdBe`:
[message/index/mod.rs](../../../crates/email/src/message/index/mod.rs), lines 58–72.
These behaviors persist in [v1.0.3's cache][upstream-cache-email] and
[message serializer][upstream-message-index]. Consequently, IMAP
`FETCH ... CHANGEDSINCE` can omit an already observed message whose `EMAILID`/`THREADID`
changed: it filters on the stored message change id in
[fetch/mod.rs](../../../crates/imap/src/op/fetch/mod.rs), lines 254–259.

The transaction also omits mailbox property changes even though mapped `totalThreads` and
`unreadThreads` can change. Mailbox state comes from `mailboxes.change_id`, not Email's
item state: [changes/state.rs](../../../crates/jmap/src/changes/state.rs), lines 132–138.
It advances only on container changes in
[cache/mod.rs](../../../crates/email/src/cache/mod.rs), lines 470–479
([v1.0.3 equivalent][upstream-cache]). Thus a client can retain obsolete thread counts
without a `Mailbox/changes` signal.

**Required change:** define concurrency-safe message modseq updates and mailbox-property
notifications in the reassignment publication, preserving singleton stored thread ids.
Amend invariant 53 to permit the required bookkeeping.

**Regressions:** reassign an already observed message, then check IMAP CHANGEDSINCE and
`Mailbox/changes` from states taken before the reassignment.

## R7 — P2: Make permanent blob links atomic with manifest publication and retirement

**Spec:** sections 5.1, 5.5 and 5.7, lines 433–443, 550–553 and 590–601.

A pass links its segment before the manifest transaction; compaction replaces the manifest
before unlinking old segments. A crash or request cancellation in either gap leaves a
permanent `Document` link that no manifest names. Error-return cleanup cannot run after a
crash. Purge treats document links as live without checking the principal or manifest:
[write/blob.rs](../../../crates/store/src/write/blob.rs), lines 328–364, and
[v1.0.3][upstream-blob], lines 455–458. The proposed destroy step discovers only the
current manifest's entries, so destroy followed by purge can leave index blobs indefinitely.
Loss or corruption of the manifest also removes the proposed cleanup inventory.

**Required change:** stage uploads with expiring holds; add new permanent links and remove
retired links atomically with the asserted manifest write. The existing `BlobHold` and
`hold_blob` provide staging primitives in
[storage/blob.rs](../../../crates/common/src/storage/blob.rs), lines 101–123 and 298–348.
Also define how recovery/destruction discovers principal-document links if the manifest is
missing or cannot be parsed. Do not rely solely on the interrupted request's cleanup.

**Regression:** interrupt every link/manifest transition, restart, destroy the account and
purge; assert that neither segments nor their permanent links survive.

## R8 — P2: Bound the manifest even when no node loads the content index

**Spec:** sections 4.2, 4.3 and 5.5, lines 313–320, 358–362 and 550–560.

The manifest's size bound relies on compaction, but compaction is skipped indefinitely when
only thread sections are loaded. That can be permanent above the account content cap, or
simply because an IMAP client fetches mail and searches locally. Every productive pass adds
a segment; after enough passes, the manifest exceeds its stated 100,000-byte limit. No
node is guaranteed ever to load the content. This can occur with many small arrivals while
mail remains within the intended product size.

Field 152 is not in the chunkable property set:
[write/key.rs](../../../crates/store/src/write/key.rs), lines 877–899. The proposed unit
test only checks the manifest at `ZA_SEARCH_SEGMENTS`, although the specified algorithm
allows the count to grow beyond that threshold without bound.

**Required change:** make bounded compaction possible without loading the whole content
index, or choose another hard-bounded manifest representation. Waiting for a content-loading
node does not establish a bound. Preserve progress of thread-only requests above the cap.

**Regression:** keep every node in thread-only mode over more productive passes than the
manifest could otherwise hold, verifying bounded storage and continued indexing.

## R9 — P2: Authenticate the sections independently for independent loading

**Spec:** sections 3.1, 4.2 and 4.3, lines 201–205, 276–281 and 340–347.

The two sections are compressed separately but then sealed together as one AEAD message.
The specified `seal` helper creates one nonce and tag for the complete input, while `open`
requires and decrypts the complete ciphertext:
[vault/keys.rs](../../../crates/vault/src/keys.rs), lines 200–234. A content-section byte
offset cannot let the loader authenticate and open just the thread section.

After compaction, a thread-only load therefore still fetches and decrypts the entire large
segment, contrary to the independent-read contract and its account-cap/resource rationale.
Skipping content decompression would save some memory, but does not fulfill the promised
independent blob read and decryption.

**Required change:** independently authenticate the sections, binding each to the account,
segment id, format and section kind, with authenticated framing. Alternatively explicitly
accept and budget whole-segment reads/decryption and revise the load guarantees.

**Regression:** instrument blob reads and allocations for a compacted segment with a large
content section and demonstrate that a thread-only load does not fetch/decrypt that section.

## Review and validation limits

Two read-only reviewers examined persistence and protocol contracts independently; their
findings were checked against the actual spec and cited source before inclusion. The
runtime did not expose a reliable originating model identity/capability ceiling, so both
reviewers deliberately inherited the parent route rather than guessing a stronger model.

No product code or design text was edited. No builds, tests, servers, benchmarks or runtime
probes were run. The cases above are proposed acceptance coverage, not executed results.
Only this report was written. Verification covered its references, whitespace, the reviewed
spec hash, and working-tree scope. Accepted keyless-session behavior, delayed indexing,
the absence of background key holders, and the separate v1.0.3 merge remain scope decisions.

[upstream-get]: https://github.com/stalwartlabs/stalwart/blob/v1.0.3/crates/jmap/src/email/get.rs
[upstream-query-changes]:
  https://github.com/stalwartlabs/stalwart/blob/v1.0.3/crates/jmap/src/changes/query.rs
[upstream-log]: https://github.com/stalwartlabs/stalwart/blob/v1.0.3/crates/store/src/query/log.rs
[upstream-group]:
  https://github.com/stalwartlabs/stalwart/blob/v1.0.3/crates/email/src/message/ingest/group.rs
[upstream-cache-email]:
  https://github.com/stalwartlabs/stalwart/blob/v1.0.3/crates/email/src/cache/email.rs
[upstream-message-index]:
  https://github.com/stalwartlabs/stalwart/blob/v1.0.3/crates/email/src/message/index/mod.rs
[upstream-cache]: https://github.com/stalwartlabs/stalwart/blob/v1.0.3/crates/email/src/cache/mod.rs
[upstream-blob]: https://github.com/stalwartlabs/stalwart/blob/v1.0.3/crates/store/src/write/blob.rs
