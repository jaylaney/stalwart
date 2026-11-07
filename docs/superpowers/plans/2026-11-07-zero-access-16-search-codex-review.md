# Zero-access plan 16: Codex review

Date: 2026-11-07. **Verdict: REQUEST CHANGES before execution.**

Reviewed the [plan](2026-11-07-zero-access-16-search.md) against the approved
[revision 18 spec](../specs/2026-11-04-zero-access-search-design.md) and current source.
The review excludes the plan's Claude model-tier assignments, as requested.

- Branch/worktree: `za-search-design`,
  `/Users/jay/Development/stalwart/.worktrees/za-search-design`.
- HEAD: `c02d3c4bab20f197d05480055183ca4b904310ea`.
- Plan SHA-256: `0bdc9075741a836463a82d45cbfbbf25d28bdfed8f593f8090de4d3855be12cf`.
- Spec SHA-256: `74f8ef6d4670ee76ad70e624a63cef1eba8d5749c394653e992ce32ac3f4278a`.

There are **19 findings: eight P1 construction issues and eleven P2 issues**. Related
snapshot failures are consolidated in PL3, shortcut failures in PL4, and missed query
paths in PL9. Construction means the prescribed algorithm or validation cannot meet its
contract; wording means a local instruction correction without changing the mechanism.
The P1 findings fail the prior stopping rule of approving unless a P1 construction defect
remains. The spec's approval is not withdrawn: these findings concern the new execution
instructions, including places where they fail to implement the approved construction.

## PL1 — P1 — Construction: Authenticate the hidden section's presence

**Plan:** 1302–1312; consuming loader at 1506–1508.

The AAD covers the header and section kind, but the optional hidden section's length sits
outside it. A store writer can replace a publishing blob's
`header | hidden_length | hidden_ciphertext | public_ciphertext` with
`header | 0 | public_ciphertext`. The public section still authenticates, and the parser
accepts an absent hidden section. The tagged manifest is unchanged, but the loader only
prescribes a header check, not verification of the fetched bytes against its blob hash.
[Blob fetching](../../../crates/store/src/dispatch/blob.rs), lines 29–45, reads and
decodes without recomputing that hash. The result silently loses records and aliases
instead of taking the authenticated-loss path.

**Correction:** authenticate hidden presence/length, or verify the fetched bytes against
the authenticated manifest descriptor before parsing. Add a negative test that removes
the hidden section from a valid publishing blob while retaining its public ciphertext.

## PL2 — P1 — Construction: Resolve staging without published-revision filtering

**Plan:** 867–875, 987–1001, 1962, 2002.

`current_thread` calls `thread_of(MAX)`, whose revision limit comes from published rows.
The pass clones the old thread part and assigns new messages at revision `r + 1`, adding
the new publication row only after commit. Thus `assign` cannot see its own earlier
assignments correctly. If published A is in thread 1, staged D replies to A, and staged N
references D alone, `current_thread(D)` returns singleton D because D's assignment is
newer than `R(MAX)`. N joins D instead of thread 1. The incorrect assignment persists
when the batch publishes.

**Correction:** distinguish current/staging resolution from historical resolution.
Staging must follow the assignments and aliases already present in the overlay regardless
of publication rows. Test multiple dependent inserts in one pass with no future row
installed, including a merge followed by an insert referencing a losing member.

## PL3 — P1 — Construction: Acquire one validated content/map/cache view

**Plan:** 2134–2135, 2205, 2278–2280, 2326; reload at 1524–1528, swap at 2002.

The prescribed acquisition order permits three failures:

1. `ZaContentGuard` holds an index read guard before `za_snapshot` runs. If another node
   publishes meanwhile, snapshot validation calls reload, which awaits `index.write()`.
   The query itself still owns the read guard, so it deadlocks.
2. Snapshot steps 4–5 validate a temporary index read and the cache's state, then step 6
   acquires the guard returned to the handler. A local pass can swap a newer publication
   in between. The handler returns newly mapped IDs under the older cache/state.
3. Content freshness is checked against the first cache, which Task 11 then replaces with
   `snapshot.cache`. A matching message delivered between those operations advances that
   cache without changing the index manifest. Snapshot validation succeeds, but the query
   omits the new match while reporting the newer state.

The [cache builder](../../../crates/email/src/cache/mod.rs), lines 449–483, associates
each immutable cache with its own change state. The response cannot freely substitute a
different cache after validating content or substitute a different index after validation.

**Correction:** coordinate content loading, catch-up and snapshot validation in one retry
procedure. Release read guards before any reload/indexing write. Validate the exact guard
returned to the handler, and check content coverage against that handler's cache. Add
interleaving tests at all three boundaries above; require bounded completion and no
result/state mismatch.

## PL4 — P1 — Construction: Do not shortcut partial checkpoints or content cursors

**Plan:** 1488–1508, 1970, 2002, 2673, 2686.

Loads seed `scanned_to` from `indexed_to.change_id` even when that record is partial;
swaps also raise it to that value unconditionally. The shortcut trusts `scanned_to`.
For a grouped record c whose first cap-sized prefix is unreadable and whose suffix is
readable, a pass writes a partial manifest-only checkpoint without a new log entry.
It sets `scanned_to = c`; the cache is also at c, so every later pass returns `Current`
and never reads the suffix. Reloading preserves the error. Real grouped ingestion uses
one batch: [group.rs](../../../crates/email/src/message/ingest/group.rs), lines 130–144.

The shortcut also omits `content_to.cursor`. A content-only reset preserves the ordinary
checkpoint and installs that cursor, so an idle account's content rebuild never runs.

**Correction:** track only fully consumed records in `scanned_to`, and explicitly refuse
the shortcut with any partial checkpoint or pending cursor. Cover the skipped-prefix
case before and after restart, and content-only recovery without subsequent delivery.

## PL5 — P1 — Construction: Preserve thread links during content-only cleanup

**Plan:** 2673, 2685.

A broken content chain sends content-only cleanup to `za_principal_blob_links`, defined
as every Principal document-0 link. That includes the thread blobs the unchanged manifest
still names. With thread blob T and content chain C3→C2→C1, losing C2 makes the scan unlink
T too. Purge deletes T, and a later load turns content-only loss into a full identity
rebuild and epoch reset. [Blob liveness](../../../crates/store/src/write/blob.rs),
lines 455–457, depends on those permanent links; purge deletes unlinked blobs at 206–224.

**Correction:** subtract the thread hashes retained by the asserted manifest from the
content cleanup scan. Test a missing middle content blob, then purge and restart without
destroying the account; thread IDs, epoch and thread blobs must survive unchanged.

## PL6 — P1 — Construction: Persist reconciliation progress for skipped items

**Plan:** E4 at 100; 2675–2679, 2686.

The difference is declared to be its own progress record, but a loss-skipped message
never enters the index. After log truncation, a present message with an unreadable blob
and no retained insert remains in `cache - index` forever. Each pass retries it, the
difference never empties, and content search stays unavailable. A cap-sized unreadable
prefix also starves later gap items. Retained-record processing restarts at
`first_retained` on every reconciliation pass, so loss-skipped retained inserts recur too.

This is a reachable recovery path:
[log truncation](../../../crates/common/src/storage/transaction.rs),
lines 228–244, removes the earlier records and leaves an empty marker.

**Correction:** persist progress that includes skipped gap items, and resume retained
processing from its complete/partial checkpoint. Attribution may inspect the retained log
without re-consuming completed work. Test unreadable gap and retained inserts with
eviction between passes; reconciliation must finish and later messages must progress.

## PL7 — P1 — Construction: Order expired folds within a publication

**Plan:** 889–892, 987–989, 1962, 2723; current fixture at 2736.

Sorting aliases only by revision does not guarantee chain order: every insert in one
pass uses the same revision, and aliases are stored in an unordered `AHashMap`.
Start with same-subject singleton roots 10, 20 and 30. Message 40 references roots 30 and
20, producing `30→20`; message 41 references message 40 and root 10, producing `20→10`.
Both edges share revision r, using the specified lowest-ID tie-break.

If `20→10` folds first, then `30→20`, root 30's record ends at 20 after both edges are
removed. Compaction changes its thread from 10 to 20. This is precisely the incomplete
expired-prefix fold that the revision 18 review excluded from the approved construction.

**Correction:** preserve dependency order within equal revisions, or walk each record's
whole expired prefix before deleting edges. Add a same-revision chain with reversed
iteration order; the existing distinct-revision fixture cannot catch this.

## PL8 — P1 — Construction: Keep alias counts out of production traces

**Plan:** 2702, versus the trace schema at 169 and privacy invariant at 181.

The rebuild case requires each `ZA_SEARCH_PASS` trace to report its alias count. That
count directly reveals whether a chosen bridge merged threads, bypassing the padding and
fixed-checkpoint protections. It is absent from the plan's production trace schema.
The test's `K - 1` bound is also per admitted item, not per publishing pass: a pass may
index several independent bridges.

**Correction:** observe alias counts through test-only in-process instrumentation or the
keyed model, without adding them to operator-visible traces. Assert the per-item bound
or an appropriate admitted-item aggregate, and assert that production traces omit it.

## PL9 — P2 — Construction: Map the query fast paths as well as residual filters

**Plan:** 2326, 2413, 2448.

The substitution inventory misses active paths in the current source:

- Top-level `inThread:T` is absorbed by `RowFilter::absorb` before the modified residual
  arm. [query/filter.rs](../../../crates/jmap/src/email/query/filter.rs), lines 84–86,
  126–128 and 233–253, still compares singleton IDs. Root A plus reply B therefore returns
  only A when queried by their mapped thread.
- `Page::count` still counts singleton threads when finishing `calculateTotal` after a
  limit: [query.rs](../../../crates/jmap/src/email/query.rs), lines 499–505, 522–530 and
  618–620. `collapseThreads:true, limit:1, calculateTotal:true` can overstate the total.
- Own-account thread-keyword sorts bypass `restrict_thread_keywords`' replacement because
  its guard requires `visible(cache)` to be `Some` (query.rs:325–332), while owner access
  returns `None` (query/filter.rs:410–417). Attachment-keyword sorts also remain cache
  comparators: Task 10 only substitutes From/To/Subject/SentAt. These paths retain
  singleton grouping or the stub's missing attachment flags.

**Correction:** extend the inventory to row absorption, every counting path, and owner/
attachment keyword comparators. Add direct regressions for these query shapes rather
than relying on residual-filter or ordinary pagination cases.

## PL10 — P2 — Construction: Use upstream's effective date

**Plan:** 1058, 2139, 2144.

`None` is ranked after every sent timestamp and matches no date predicate. Upstream's
[sent_at_offset](../../../crates/email/src/message/messagedata.rs), lines 330–335,
uses zero for missing Date and clamps supplied offsets;
[sorting](../../../crates/email/src/message/sortkeys.rs),
lines 396–408, uses `received_at + offset`. A no-Date message received at 100 sorts before
a Date-200 message upstream, but after it under the plan. Extreme dates differ too.

**Correction:** derive the same effective timestamp from received time and the upstream
offset rule for both sorts and SENT/date filters. Test absent, unparsable and out-of-range
Date values against the ordinary account.

## PL11 — P2 — Construction: Charge memory until its final owner releases it

**Plan:** B6 at 54–56; 1474–1480.

Removing an entry from the map decrements accounting while readers can still retain its
`Arc`. Repeated account invalidation and replacement loads can leave arbitrarily many
old generations alive and uncharged. A fixed 3× load factor cannot bound that number.
The invalidation paths at [invalidate.rs](../../../crates/common/src/cache/invalidate.rs),
lines 609–615 and 634–642, can run independently of readers.

**Correction:** attach reservations to live allocations until their final owners drop,
as [MemoryReservation](../../../crates/common/src/network/memory.rs), line 178, does.
Test overlapping invalidated generations, ensuring replacement content loads still
account for bytes retained by earlier readers.

## PL12 — P2 — Construction: Match the manifest count to accepted configuration

**Plan:** 544, 774, 1279.

The manifest stores `n_thread` as u8, but `ZA_SEARCH_SEGMENTS` accepts arbitrary positive
usize values. At 257, the 256-entry manifest is published before compaction triggers and
cannot be encoded by the specified format. This is not the 256 threshold, which could
compact before encoding its 256th entry.

**Correction:** widen the length or enforce a compatible configuration ceiling and test
both sides of the boundary. Encoding must not silently truncate or panic on an accepted
configuration.

## PL13 — P2 — Construction/contract: Positions are not merely a format choice

**Plan:** 25, 515; contrary to the spec's same-phrase-rules contract at 206–210.

The benchmark branch drops positions above 128 MiB and tells Task 4 to use co-occurrence.
Then a phrase such as `alpha gamma` matches `alpha beta gamma`, contradicting the approved
semantics and the plan's own phrase test at 1115–1116. The call is explicitly pending Jay,
so this review does not claim the deviation has already been authorized; it does change
a spec decision despite the header's assertion that none of the plan calls does.

**Correction:** retain positions for the approved implementation. Keep the position-free
measurement as a comparison, or present dropping positions as a separate semantic change
requiring explicit approval and corresponding spec/test amendments.

## PL14 — P2 — Construction (validation): Make the Move fixture actually merge

**Plan:** 2836.

The replacement chooses the lowest surviving type-1 message as `older`. The first
`Move(0,3)` therefore references message 0 twice. Later source messages retain different
`test_{id}` subjects: [za_variants.rs](../../../tests/src/jmap/za_variants.rs), lines
863–867 and 897–925. [Thread matching](../../../crates/email/src/message/ingest.rs),
lines 1098–1145, only searches the same normalized-subject bucket. Neither construction
creates the claimed losing-thread transition, yet bookkeeping records the old ID as
destroyed.

**Correction:** use two distinct existing threads with the same normalized subject and
assert that the losing member's prefix actually changes before checking queryChanges.

## PL15 — P2 — Construction (validation): Normalize the G5 size comparison

**Plan:** 2915, 3004.

Changing the twenty-first message's vocabulary can change its own compressed length.
Per-record compression guarantees that the unchanged twenty records do not shrink; it
does not guarantee equal total blob sizes. The spec explicitly permits that sum of
content-dependent sizes, and [codec.rs](../../../crates/store/src/search/codec.rs),
lines 89–102, produces variable-length per-document output. Correct code can fail the
specified assertion.

**Correction:** compare the unchanged records, or subtract each neighbor's independently
encoded contribution from the total. Show that cross-record compression changes that
normalized measurement. Duplicate suppression is not an additional issue for key accounts.

## PL16 — P2 — Construction (validation): Let the benchmark recognize current state

**Plan:** 2948 and its reuse by 2954; pass behavior at 1962, 1977–2002.

The pass-rate benchmark waits for the persisted `indexed_to` to equal the account's
current change ID. The final publishing pass itself adds a later witness update. The next
update-only pass advances only in-memory `scanned_to` below the 1,000-record checkpoint
threshold, then later passes shortcut. On an idle account the manifest equality remains
false indefinitely, so the benchmark never completes despite a current index.

**Correction:** use the real current/coverage predicate, including checkpoint completeness
and cursor state, or explicitly request a terminal manifest checkpoint for measurement.
The benchmark must finish after the final delivery without generating extra mail changes.

## PL17 — P2 — Construction (validation): Disable attachment indexing in its control

**Plan:** 486, 498.

The generated attachments are text/plain, but the attachment-free control merely zeros
`ExtractLimits`. [index/search.rs](../../../crates/email/src/message/index/search.rs),
lines 75–89 and 168–185, sends these attachments directly through text indexing; binary
extraction limits do not disable them. Both measurements therefore still index attachment
text and cannot establish the claimed rate difference.

**Correction:** disable the Attachment index field for that control and assert that an
attachment-only term disappears while body terms remain.

## PL18 — P2 — Construction (validation): Preserve the compile check's failure status

**Plan:** 132–136.

The failure group ends with `false`, but the next warning-filter command still runs in
a shell without errexit. Its `|| echo clean` succeeds, so the whole block can finish
successfully and print `clean` after compilation failed. The comment promises the
opposite, and this is the per-task gate reused by final verification.

**Correction:** use an explicit conditional that exits nonzero on build failure, or chain
the filter only after a successful build. Do not rely on an executing agent's ambient
shell options.

## PL19 — P2 — Wording: Correct Task 1's tokenizer target and stop threshold

**Plan:** 456, 515.

The accessor instructions name `IndexDocument::build_batch` tokenization, but tokenization
lives in [AccountIndexer::index_document](../../../crates/store/src/search/term.rs),
line 100; `AccountIndexer::build_batch` at 276 emits postings. Point the extraction at
the actual shared tokenizer and specify how its raw encoder output is exposed.

The same task accepts a derived `R = 304` but orders a stop whenever the measured maximum
exceeds 296. A valid maximal record would halt execution with a false derivation error.
Use the derived constant consistently. Both are local instruction corrections.

## Review method and limits

Three read-only review scopes covered Tasks 1–5, persistence/lifecycle Tasks 6–9 and 14–16,
and protocol/validation Tasks 10–13 and 17–18. The primary checked the global instructions,
Task 19, cross-task dependencies and the reported failure cases against source, and
consolidated overlapping findings. Existing parent-model routes were inherited; no claim
about a stronger model tier is made, and the plan's execution tiers were not assessed.

No implementation, build, test, benchmark, runtime probe or network action ran. Only this
review file was written. The plan and approved spec remain unchanged.
