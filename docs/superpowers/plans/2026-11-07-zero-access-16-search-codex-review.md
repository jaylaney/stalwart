# Zero-access plan 16: Codex review

## Current review — plan revision 13, 2026-11-08

**Verdict: APPROVE under the stopping rule. No P1 construction defect remains in scope.**

Reviewed revision 12's follow-ups at `fc1911eab`, including the confirmed shared account-cap
requirement, then independently reviewed the requested routing-only revision 13 edit to the
[plan](2026-11-07-zero-access-16-search.md). This pass covers the changed requirements and
their direct interactions; earlier accepted design decisions remain outside its scope.

- Technical baseline: `fc1911eab7184af71f96f874e9208265febb454b`, branch `za-search-design`.
- Worktree: `/Users/jay/Development/stalwart/.worktrees/za-search-design`.
- Baseline plan SHA-256:
  `c50462ce59365b33792942e73d906056f28829ad428148369761cea38ebed388`.
- Plan SHA-256 after the routing-only edit:
  `955597d598ac0f8eedc0e44fe30c26ab0660fdad5a38dcb2775ecbe5da6f63de`.
- Spec revision 20 SHA-256:
  `6f2d9a2e3b9f5b28687c1e9ccbe94b41dd9fc3b7fbcd8b009e7aa6aae4dad0d4`.

Plan line references below use the revision 13 text, including the routing edit.

### Disposition and routing

| Finding | Disposition |
|---|---|
| PL20 | Closed: seven currency conditions include the content cursor; fixture added (2428, 2444). |
| PL21 | Closed: both raised-cap variants keep the segment override through backfill (3106). |
| PL22 | Closed: the completion fixture explicitly requires `ManifestOnly` (2300–2302). |

FX1 clears stale content before either reload path when the manifest has no head. FX2
reloads after a content-loss start or assertion loss before re-entering the wait loop.
FX3 checks the source head before installing a load and drops a resident part behind the
pass's base head instead of extending it. These close the reported stale-content cases;
the new loader retry has the bounded-wait follow-up below. The shared cluster account cap
is recorded as the accepted operator requirement and in Task 19's documentation changes.

All active implementation assignments now select Sol (`gpt-6.1-sol`): `high` for the
kernel tasks and `medium` for the other tasks. Every independent reviewer uses Astra
(`gpt-6-astra`, `xhigh`), including a fresh review of the integrated branch after Task 19.
An Astra primary dispatches implementers and reviewers as siblings, preserving the
capability ceiling and reviewer independence. Required checks and the Task 1 checkpoint
remain in place. Historical Opus/Sonnet references are explicitly superseded.

This routing is a reasonable choice for a detailed implementation plan with demanding
independent review. Official model guidance describes Sol for complex coding and Astra
for the most demanding reasoning and coding work:
[Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol),
[Astra](https://developers.openai.com/api/docs/models/gpt-6-astra).
This is a routing assessment, not measured performance on this repository.

### PL23 — P2 — Construction: Bound the loader's head-change retries by the request deadline

**Plan:** 1798–1804; callers at 2445; racing-load fixture at 1874.

The FX3 correction walks content outside the index write lock, then retries from the newer
head if a pass advanced it before installation. The loader takes no deadline or retry
bound, and both content-wait call sites await it directly. The outer deadline is checked
only after the loader returns.

A load of H0 can lose its install check to a swap publishing H1, then its H1 retry to H2,
and so on. Every individual read can succeed, yet the internal loop continues beyond
`ZA_SEARCH_WAIT_SECS` without returning control to the content wait. This violates the
bounded wait stated in Task 9 (2428) and spec 5.2, 1259–1263.

**Correction:** wrap the load in `timeout_at(deadline, ...)`, or pass a deadline/bounded
retry policy into it. On refusal or cancellation, release the temporary part and its
charge. Extend the racing-load case from one head advance to repeated advances, asserting
timely refusal and charge cleanup. The head validation itself is correct and should stay.

### PL24 — P2 — Wording: Compare blob reads with the pre-request thread inventory

**Plan:** 3106, the new FX9 portion of `case_f6_r23_over_the_cap_then_raised`.

After restart, the case delivers a message, resets the counters, and runs `Mailbox/get`.
That request loads N existing thread blobs and then indexes the new message, publishing
one more thread blob. With compaction disabled, the post-request manifest therefore has
N+1 thread blobs, but the read counter is N: writing the new blob is not a blob read.
Comparing the counter with the manifest's count after the request rejects correct behavior.

**Correction:** capture the thread-blob count before the request and compare with that
value, or assert the exact fetched thread hashes. Keep the separate assertions that the
new message was indexed and that no content blob was read.

### Review method and limits

Three read-only scopes checked reload/install races, content recovery/currency, and test
instructions/documentation. An independent reviewer also checked the routing edit. The
primary inspected the cited text and adjudicated the findings. This review reused the
existing parent-model routes because the originating capability ceiling was unresolved;
the explicit Sol/Astra routes are for the plan's future execution.

No builds, tests, benchmarks or runtime probes ran. Official OpenAI documentation was
consulted only for the requested routing assessment. The only edits were the plan's agent
routing and this review. The spec and Fable report were preserved; earlier reviews follow
unchanged except for renaming the previous current heading to historical.

## Historical review — plan revision 11, 2026-11-08

**Verdict: APPROVE under the stopping rule. No P1 construction defect remains in scope.**

Reviewed the [plan](2026-11-07-zero-access-16-search.md) at `c6a032796`, as requested after
revision 11 landed during the review. This pass checks the two PL11 residuals from revision
7, the intervening revisions 8–10, and revision 11's changes and direct interactions.
Earlier accepted design decisions and the plan's model tiers remain outside the scope.

- Reviewed commit: `c6a032796d4c4a9163209b5a348306fc8637b563`, branch `za-search-design`.
- Worktree: `/Users/jay/Development/stalwart/.worktrees/za-search-design`.
- Plan SHA-256: `19ad644a003a0c3f1c7726b6ce23953660baca805282e853eca5277d19d85043`.
- Spec revision 20 SHA-256:
  `6f2d9a2e3b9f5b28687c1e9ccbe94b41dd9fc3b7fbcd8b009e7aa6aae4dad0d4`.

### Disposition and construction checks

**PL11 is closed.** Admission and charge adjustment share the admission mutex through
`admit` and `grow`; loads, incremental reloads, pass swaps and compaction swaps use the
same protocol. Refused content growth drops the part and its charge while keeping committed
progress. The pass calls the search cache on the correct receiver, outside the content
borrow. The ownership and concurrent-growth fixtures cover the prior counterexamples.

**Revision 11's backfill separation is sound in this scope.** The pass no longer loads
content (2284–2285). An unavailable content source leaves its cursor pending and permits
thread/log and gap work to continue (2276), so a cold node above the content cap no longer
aborts every indexing pass. Content requests perform their own preload and handle cap and
budget refusal (2397). `content_runnable` and the optional source distinguish an unprepared
list from an empty one (2096, 3012). Reloading a manifest with no content head drops stale
resident content and its charge (1747–1752).

The reconciliation changes preserve insert-before-delete ordering across capped passes,
write a moved bound only to a surviving marker, and supply the bound and completion state
to the planner before it chooses whether to write. Abort preserves those facts while
discarding staged records and cursor progress. No additional construction issue was found
in these changes. Three P2 wording/test corrections remain below; none requires a new
mechanism or changes the verdict under **approve unless a P1 construction defect remains**.

### PL20 — P2 — Wording: Include the content cursor in the explicit currency predicate

**Plan:** 2380, 2396–2397; the existing pending-cursor rule is at 3027.

Task 14 already requires content requests to refuse while `content_to.cursor` is pending,
but Task 9's explicit six-condition `is_current` predicate omits that check. Checkpoint
equality alone is insufficient: a full rebuild does not advance `indexed_to` as it admits
its cursor items, so content skipped while suspended can coexist with equal checkpoints.

For example, both checkpoints are 100 during a rebuild: A has content, B was indexed while
suspended, and C remains on the rebuild cursor. `CapRaised` preserves checkpoint 100 and
starts the content cursor. If C is expunged at 103, a pass can exhaust the empty rebuild
cursor, leave the unprepared content source pending, consume the records through 103,
and carry both checkpoints to 103 (2290). All six listed conditions then pass despite
B's missing content.

**Correction:** add `content_to.cursor.is_none()` to `is_current` and update its references
to six conditions. Add a regression for equal checkpoints with a pending content cursor,
including the finishing-rebuild interaction. This makes the explicit predicate implement
the refusal rule the plan already states; the cursor and its completion mechanism exist.

### PL21 — P2 — Wording: Keep the compaction override through the backfill assertion

**Plan:** 3050; override lifetime at 2378; default segments at 247; compaction at 3085.

`case_f6_r23_over_the_cap_then_raised` sets `account_max_mb: 1, segments: 255` to suppress
compaction, then clears that combined override before checking that `content.length`
grew only by the backfill's blobs. Clearing it also restores the default segment threshold
of 16. A valid pre-suspension chain of 16 tier-0 nodes can therefore compact when the first
backfill blob is added, reducing its length despite correct backfill behavior.

**Correction:** keep `segments: 255` for the whole case while restoring only the account
cap, including the second raised-cap variant. For example, use an outer segment override
and an inner cap override. The chain-length assertion then measures the intended behavior.

### PL22 — P2 — Wording: Assert that the completion regression chooses a manifest write

**Plan:** 2253–2254; corrected planner decision ordering at 2276.

The new FW5 fixture says the decision must see `marker_bound` and `Finished`, but asserts
only those two fields. The original defect still passes that assertion: compute `Nothing`
first, then install the bound and completion on the returned staging value. The caller
would then omit the manifest write even though the test passed.

**Correction:** also assert `matches!(s3.decision, PassDecision::ManifestOnly)`, using no
indexed records and fewer update-only records than the checkpoint threshold. That fixture
requires the bound/completion itself to select the write and catches the stated regression.

### Review method and limits

Three read-only scopes checked memory admission and reloads, backfill/currency behavior,
and reconciliation/test instructions. The primary inspected the changed text, traced the
counterexamples, and adjudicated the findings. Existing parent-model routes were inherited
because the originating capability ceiling remained unresolved; Claude tiers were ignored.

No builds, tests, benchmarks, runtime probes or network checks ran. This approves the scoped
plan under the stated stopping rule, not an implementation or runtime result. Only this
review was edited. The plan, spec and Fable report were preserved. Earlier reviews follow
unchanged except for renaming the previous current heading to historical.

## Historical review — plan revision 7, 2026-11-08

**Verdict: APPROVE under the stopping rule. No P1 construction defect remains in scope.**

Re-reviewed the [plan](2026-11-07-zero-access-16-search.md) at `aa594dc09`, checking the
three revision 6 follow-ups, their direct interactions, and the D13 changes in
[spec revision 20](../specs/2026-11-04-zero-access-search-design.md). This is a scoped
re-review; earlier accepted decisions and model-tier assignments remain outside its scope.

- HEAD: `aa594dc098519b6068230298770a6cb6f72b9484`, branch `za-search-design`.
- Worktree: `/Users/jay/Development/stalwart/.worktrees/za-search-design`.
- Plan SHA-256: `7463124e004fb587597b350f737f26bb8609ecd7f487573dedcbd032416caac4`.
- Spec revision 20 SHA-256:
  `6f2d9a2e3b9f5b28687c1e9ccbe94b41dd9fc3b7fbcd8b009e7aa6aae4dad0d4`.

### Disposition of the revision 6 follow-ups

| Finding | Revision 7 disposition |
|---|---|
| PL11 | Sequential growth and size estimate fixed; two admission details remain below. |
| PL3 | Closed: the regression pauses after validation while retaining the view's guard. |
| PL19 | Closed: Task 1's Files list and staging command both include `codec.rs`. |
| D13 | Spec alignment closed: Thread state is the validated cache's group counter. |

PL11's swap now uses `ContentPart::loaded_bytes()` and drops resident content if growth
cannot be admitted, without losing committed progress (2219). The estimate is explicitly
shared by loads, reloads and swaps (1649), including postings (1239–1240). The new test
covers sequential cross-account pressure, refusal with other parts held, and checkpoint
preservation (1772). These answer the previous sequential counterexample.

PL3's `SEARCH_VIEW_PAUSE` is after all validation, with the returned cache and read guard
still held (2377). The revised regression (2875–2885) can now publish a merge while the old
view remains pinned: the competing pass pauses or skips before taking the swap's write
lock. Both same-node and remote forms retain the old grouping and its old state.
Spec 6.2 and 6.3 now agree with the plan's `ZaView::state()` (2520), `Thread/get` (2570)
and `Thread/changes` (2779). Task 1 includes its raw-buffer helper in both lists (511, 618).

Two P2 corrections remain within PL11: one construction issue and one wording issue.
Neither changes the verdict under **approve unless a P1 construction defect remains**.

### PL11 — P2 — Construction: Reserve admitted growth before another account can claim it

**Plan:** 2219; admission and charge interfaces at 1635–1648.

The new swap checks `make_room(Content, delta, account_id)` and only afterward calls
`content.charge.resize(grown)`. `make_room` returns a Boolean; its contract checks the
counter but reserves no bytes. The surrounding index write lock belongs to one account,
so it cannot serialize admission and charging across two accounts.

With a 1 MiB budget and 800 KiB already charged, let two accounts each grow by 150 KiB.
Both swaps can observe 800 KiB and admit their growth before either resizes its charge.
Both then retain their content, leaving 1,100 KiB charged. No eviction is triggered,
because each separate check saw room. This is the concurrent form of the budget failure;
the new sequential test does not exercise it.

**Correction:** admission must atomically reserve the delta, or a shared admission lock
must cover the check through the charge adjustment. Ensure load and swap admissions
participate in the same reservation protocol, without double charging. Existing source
shows an atomic reservation and rollback pattern in
[memory.rs](../../../crates/common/src/network/memory.rs), 88–104. Add a two-account test
that synchronizes the competing growth attempts and verifies that only admitted content
remains resident, with committed progress retained on refusal.

### PL11 — P2 — Wording: Call admission on the search cache

**Plan:** 2192, 2219.

`write_pass` binds `cache` to `server.get_cached_messages(account_id)`, an
`Arc<MessageStoreCache>` ([cache/mod.rs](../../../crates/email/src/cache/mod.rs), 59).
The new `cache.make_room(...)` call therefore names the wrong receiver: that method
belongs to `ZaSearchCache`. Qualify it as `server.inner.cache.za_search.make_room(...)`,
or bind a distinctly named search-cache reference, adapting the call to the atomic
admission correction above. The literal sketch otherwise cannot compile.

### Review method and limits

Three read-only scopes checked memory admission, view timing/spec alignment, and Task 1's
commit instructions. The primary inspected the changed passages and supporting source,
then adjudicated the findings. Existing parent-model routes were inherited because the
originating capability ceiling remained unresolved; the plan's Claude tiers were ignored.

No builds, tests, benchmarks, runtime probes or network checks ran. This is approval of
the scoped plan under the stated stopping rule, not implementation or runtime validation.
Only this review was edited. The plan, spec and Fable report were preserved, and earlier
reviews follow unchanged except for renaming the previous current heading to historical.

## Historical review — plan revision 6, 2026-11-08

**Verdict: APPROVE under the stopping rule. No P1 construction defect remains in scope.**

Re-reviewed the [plan](2026-11-07-zero-access-16-search.md) at `32c05fb78`, checking the
five outstanding items from the revision 2 re-review and their directly changed consumers.
The intervening revisions 3–5 supply part of those fixes. This is a scoped re-review,
not a fresh audit of every task. Model-tier assignments remain out of scope.

- HEAD: `32c05fb78c6c5afc860e27489d12d9badbccc0f7`, branch `za-search-design`.
- Worktree: `/Users/jay/Development/stalwart/.worktrees/za-search-design`.
- Plan SHA-256: `ea1559e07deb879e4828d9c7fa043bc832a40276f0fa783d57abc0d6f2995ed2`.
- Spec revision 19 SHA-256:
  `3d90536da3396c1af2d8c306dba9c7bac4fc575f2c3804eb5859b144e7a994db`.

### Disposition of the revision 2 re-review

| Finding | Revision 6 disposition |
|---|---|
| PL3 | P1 closed: Thread state comes from the validated view. Regression timing follow-up below. |
| PL11 | Lifetime test and resize calls fixed; content admission/accounting follow-up below. |
| PL19 | Raw-byte accessor fixed; Task 1's commit list needs the new codec file below. |
| PL1 | Closed: stale-hash corruption expects Hash; header mismatch uses its actual hash. |
| PL16 | Closed: the wait test uses currency and progress, without persisted-state equality. |

`ZaSnapshot::state()` now returns `cache.last_change_id` (2517). `Thread/get` (2567)
and `Thread/changes` (2776) use that value from the same view whose map and cache were
validated together (2368–2374). A later store publication cannot independently advance
the response's Thread state. This closes the remaining lost-sync construction.

Source supports that pairing: the cache retains its corresponding counter in
[cache/mod.rs](../../../crates/email/src/cache/mod.rs), 449–483, and Thread shares the
Email counter group in [collection.rs](../../../crates/types/src/collection.rs), 228–231.
D13 explicitly identifies the corresponding spec wording amendments; that synchronization
remains recorded in the plan rather than applied to the spec in this review.

Three P2 follow-ups remain: one construction/integration defect and two wording defects.
None changes the approval verdict under **approve unless a P1 construction defect remains**.

### PL11 — P2 — Construction: Admit resident content growth against the process budget

**Plan:** 2216; budget interfaces at 1632–1636; content size estimate at 1236–1237.

The swap now resizes both charges, and the two-generation test at 1769 releases all owners
correctly. However, the swap still retains content growth unconditionally. Resizing records
the usage; it does not check whether that usage fits or evict another content part.

For example, eight resident content parts can grow from 40 MiB each to 80 MiB each through
local passes. Every account stays below the default 128 MiB account cap, but their retained
total reaches 640 MiB against the 512 MiB process budget. Task 8's limits (2185) check the
account cap, and its blob-writing step (2191) does not perform process-budget admission.
The load/reload checks at 1699–1701 and 1807–1809 do not cover these resident local swaps.
Spec 4.3's soft-budget exception at 864–868 applies to thread parts.

**Correction:** apply the existing budget admission/eviction mechanism to retained content
growth. If room cannot be made, drop the resident content part while preserving the
committed index progress. Use the existing `ContentPart::loaded_bytes()` estimate, which
includes postings and bitmaps: the new `content.bytes += c.encoded_len()` only counts
encoded records and omits that growth. Keep the estimate consistent across loads and swaps.

Extend the growth test to cross the process budget with several accounts while remaining
below each account's cap, and include records that create new posting keys. The existing
mechanisms suffice; the remaining defect is their application in the swap.

### PL3 — P2 — Wording: Pause the regression after the view has been acquired

**Plan:** 2875–2880; the hook's actual placement is 2368–2369.

The new regression parks `Thread/get` at `SEARCH_SNAPSHOT_PAUSE`, after the cache read but
before the manifest read. It then commits a merge and requires the parked request to return
the old grouping and a state below that merge. Correct acquisition instead reads the new
manifest, detects the old loaded hash, reloads, and refreshes the cache through steps 5–6.
It returns the new grouping/state, or a bounded refusal, rather than the prescribed old pair.

Place this regression's pause after `za_view` successfully returns, retaining its read guard
before Thread rendering. Then commit the competing merge and resume the old view. Its
returned state must stay below the merge, and the next `Thread/changes` must include it.
Keep both the same-node commit waiting to swap and the remote-publication forms. This
corrects the test interleaving; the production state-pairing fix stands.

### PL19 — P2 — Wording: Include the raw-byte helper in Task 1's commit

**Plan:** 505–510, 562, 615.

The accessor now correctly obtains the writer's raw buffer through a test-only
`Writer::into_raw_bytes()` in `crates/store/src/search/codec.rs`. Task 1's Files list and
explicit `git add` command omit that file. Following the prescribed command leaves the
required helper uncommitted, so a fresh checkout cannot compile its `test_mode` caller.

Add `crates/store/src/search/codec.rs` to both lists. The underlying benchmark correction
is closed: it no longer feeds `into_document()`'s compressed, marked output into the
compression comparison.

### Review method and limits

Three read-only scopes checked state pairing, memory accounting, and the benchmark/test
corrections. The primary inspected the cited plan and source passages and adjudicated the
findings. Existing parent-model routes were inherited because the originating capability
ceiling was unresolved; the plan's Claude execution tiers were not evaluated.

No builds, tests, benchmarks, runtime probes or network checks ran. This approves the plan
under the stated scoped stopping rule, not an implementation or runtime result. Only this
review was edited; the plan, spec and Fable report were preserved. Earlier reviews follow
unchanged apart from changing the previous current heading to historical.

## Historical review — plan revision 2, 2026-11-07

**Verdict: REQUEST CHANGES. One P1 construction issue remains in PL3's Thread consumer.**

Re-reviewed the [plan](2026-11-07-zero-access-16-search.md) at the requested commit,
checking PL1–PL19 and interactions introduced by their corrections. Model tiers remain
out of scope. The stopping rule is **approve unless a P1 construction defect remains**.
There are four P2 wording/integration follow-ups in addition to the P1 below.

- HEAD: `584096fe562958a9a22439e9c2a7a4df4211deca`, branch `za-search-design`.
- Worktree: `/Users/jay/Development/stalwart/.worktrees/za-search-design`.
- Plan SHA-256: `51c083f68d64b2ecb7131ba2618cb8aa60c86d0bab53533f734ba7f93f4e4f44`.
- Spec revision 19 SHA-256:
  `3d90536da3396c1af2d8c306dba9c7bac4fc575f2c3804eb5859b144e7a994db`.

### Disposition of the first review

| Finding | Revision 2 disposition |
|---|---|
| PL1 | Production fix closed: descriptor hashes verified before parsing. Test follow-up below. |
| PL2 | Closed: staging resolves current assignments without published-row filtering. |
| PL3 | Original three cases closed by `za_view`; Thread's state still escapes it below. |
| PL4 | Closed: partial-safe `scanned_to`, complete checkpoint and all pending cursors checked. |
| PL5 | Closed: content cleanup excludes retained thread hashes; purge/restart case added. |
| PL6 | Closed: durable `gap_next` advances on skips; retained records resume from checkpoint. |
| PL7 | Closed: whole expired prefixes are walked before deletion, including same-revision edges. |
| PL8 | Closed: per-item alias counts are test-only; production traces explicitly omit them. |
| PL9 | Closed: row absorption, page counting and owner/attachment sorts are covered. |
| PL10 | Closed: production records carry upstream's effective sent time. |
| PL11 | Lifetime fixed; mutation accounting and test ownership need updates below. |
| PL12 | Closed: boot range and final publication guard cover the one-byte manifest count. |
| PL13 | Closed: positions remain mandatory; the size figure is reported for a decision. |
| PL14 | Closed: Move is an explicit no-op; Task 13 retains real merge-rendering coverage. |
| PL15 | Closed: the four measurements subtract each neighbor's standalone contribution. |
| PL16 | Benchmark loop fixed; one old currency assertion remains below. |
| PL17 | Closed: the control disables Attachment indexing and checks the resulting terms. |
| PL18 | Closed: the conditional preserves a failed build's nonzero status. |
| PL19 | Tokenizer call and R threshold fixed; the raw-byte accessor needs correction below. |

The chosen alternatives are sound for their stated purpose: a gap cursor records skipped
work, per-record prefix walks need no same-revision tie-break, and the segments ceiling
matches the codec's count width. The spec's B1/E1 amendments supply the derived byte
maxima and a revision floor row; neither reopens R35's alias-count proof.

### PL3 — P1 — Construction: Pin Thread's state inside the validated view

**Plan:** 2448, 2494, 2703–2706; `za_view` at 2297–2305.

The new acquisition closes the three reported Email/content interleavings. However,
`thread_state` still reads the live store counter after the view has frozen its map and
cache, checking only that the counter is not below the map's publication. `Thread/get`
and `Thread/changes` call that helper outside the acquisition loop.

A concrete lost-sync sequence:

1. `za_view` returns the map/cache at publication P1, holding its read guard.
2. A pass commits a merge at P2. A local swap may wait for that guard; another node also
   can commit without acquiring the reader's local lock.
3. `thread_state` reads P2. Its lower-bound check succeeds against the P1 map.
4. `Thread/get` returns the old grouping with state P2.
5. The client's next `Thread/changes` from P2 returns empty under the equality rule, so
   the client never receives that merge.

[get_state](../../../crates/jmap/src/changes/state.rs), lines 58–65, really reads the
current store counter; it is not a state attached to the view. Holding a read lock through
the response prevents a local swap, not the store commit preceding it.

**Correction:** make the Thread state part of the view's retry/validation procedure and
return that pinned value. It must correspond to the returned cache and map, not merely be
at least as new as the map. Capture and validate it with those inputs, or use a corresponding
snapshot counter where the method contract permits. Update the inherited I5 instruction
in spec 6.2 as well: its after-manifest read and lower-bound check alone are insufficient.
This is an overlooked consumer of PL3, not a failure of the new three regression cases.

**Regression:** pause after successful view acquisition, commit a merge, resume
`Thread/get`, then apply `Thread/changes` from the returned state. The resulting grouping
must agree with a fresh read. Cover a same-node commit waiting to swap as well as a remote
publication, and the corresponding current-end acquisition in `Thread/changes`.

### PL11 — P2 — Wording/integration: Apply charges to growth and release all test owners

**Plan:** 1585, 1706, 2147, 2896.

`BudgetCharge` fixes the original lifetime defect. The local pass swap still appends
thread/content records and updates only `thread_bytes`, without resizing either charge
or updating `content.bytes`. Compaction's replacement likewise needs explicit charge
adjustment. A small loaded account can therefore grow through local passes while remaining
charged at its old size.

Wire the already-defined byte estimates, budget admission and `BudgetCharge::resize` into
every local mutation/replacement, just as the plan does for incremental reload. Add a
loaded-small, grow-through-passes, then budget-pressure case. This uses the existing
mechanism; it does not require another cache design.

The new lifetime test also needs its owners released correctly: dropping the first reader
does not free its generation while the local `entry` Arc still owns it, and dropping the
replacement's local Arc does not free it while the map still owns it. Drop the old entry
handle before the last reader, and remove the replacement from the map before expecting
zero usage. Otherwise the corrected implementation fails the prescribed test.

### PL19 — P2 — Wording: Obtain raw encoder bytes before benchmarking compression

**Plan:** 510, 541.

The tokenizer call is now correct, but the accessor is still told to use
`Writer::into_document()` while promising raw, uncompressed output.
[codec.rs](../../../crates/store/src/search/codec.rs), lines 89–102, compresses eligible
documents under the Term dictionary and always appends a storage marker. Task 1 would
then recompress those stored bytes, invalidating its raw-size and dictionary comparisons.

Return the actual writer buffer through a test-only accessor, or decode the storage
wrapper before measuring. Check an above-watermark fixture so an already-compressed
result cannot accidentally satisfy the accessor test.

### PL1 — P2 — Wording: Expect hash failure before the old parse failure

**Plan:** 1660–1662, versus the new verifier at 1602–1606.

The existing chain test replaces node 1's bytes under its original hash and still expects
`Loss(Parse)`. Mandatory descriptor-hash verification now returns `Loss(Hash)` first.
Update that expectation; test header/descriptor mismatch separately using the replacement
bytes' actual hash. The production integrity correction itself is closed.

### PL16 — P2 — Wording: Remove the same invalid equality from the wait test

**Plan:** 2216; the corrected benchmark is at 3117.

`case_content_wait_budget` still expects persisted `indexed_to.change_id` to equal the
cache's latest change ID after `ensure_current` succeeds. The final publication's own
update lies above that checkpoint, and a short update-only tail advances `scanned_to`
without another manifest write. This is the same distinction the revised benchmark now
handles correctly.

Use `is_current` and the existing progress assertions here too. A successful wait must
not fail a test solely because its harmless update-only tail stayed in memory.

### Review method and limits

Three read-only scopes rechecked the assigned fixes and their consumers; the primary
verified the findings and the benchmark, trace and compile-gate corrections against the
current plan/source. Existing parent-model routes were inherited; the plan's execution
tiers were not assessed. Task 14's abbreviated shortcut reminder is read together with
Task 8's explicit complete predicate, not treated as a separate construction defect.

No builds, tests, benchmarks, runtime probes or network checks ran. Only this review was
edited. The plan and spec remain unchanged, and the first review below is preserved.

## Historical review — plan revision 1, 2026-11-07

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
