# Zero-access search and threads: plan 16 outcome

Evidence record as of 2026-11-11 UTC for [Plan 16](2026-11-07-zero-access-16-search.md)
and the [search design](../specs/2026-11-04-zero-access-search-design.md).
This evidence records plan revision 27 and spec revision 33. The subsequent
[PR 84 follow-up](2026-11-12-zero-access-search-pr84-fixes-outcome.md) records spec
revision 34 and its separate verification; the results below do not qualify those later changes. Task 1 measured
the format at spec revision 20. Revision 21 records the subsequent memory
default ruling. The task record below distinguishes accepted source and
controls from pending full qualification.

Links under `.superpowers/sdd/2026-11-07-zero-access-16-search/` refer to
checkout-local ignored evidence files. Those files are absent in ordinary
clones. The measurements and dispositions are recorded here.

## Where things stand

Branch `release7-search`, fork point `3649daf00bc5d63f662938eace4dd19d82ac88f2`. The execution checkout is the repository-local `.worktrees/release7-search`. The accepted implementation ends at `53062c367ac0b85596017fed6ebb50edd8a66567`. The separate review record is committed at `4dde13ade2be92411d76eb7906b14a0423359dc9` with identical code and test inputs. Execution began at plan 13/spec 20; current accepted authority is plan 27/spec 33.

| Task | Accepted commit and associated authority |
|---|---|
| 1 | `a733b0a1e` format benchmark, followed by `b777a30d4` provisional defaults |
| 2 | `0981aca03` fixed Publishing padding contract, `723776a35` key, format constants, boot configuration and hooks |
| 3 | `77ded51a9` thread model |
| 4 | `872620191` content model, followed by `282cfd461` validated post-mutation view contract |
| 5 | `aace1dd6b` blob format and manifest |
| 6 | `f247a57ad` cache and loader |
| 7 | `a0dcb3f62` record reader and openers, followed by `4870f31a6` pinned initial paging/retention contract |
| 8 | `42ee6e3dd` bounded passes and atomic publication |
| 16a, scheduled early | `d1f84c222` reset fences and `bcb3cd97f` repeated destroy cleanup authority, then `fa86c0780` destroy primitive and Principal-link scan |
| 9 | `f463fdf34` keyed triggers/wait, followed by `250a6c023` validated content views, `8fd2dfbb6` single queryChanges view and `d0bfb8f62` exact-base recovery/empty-account epochs |
| 10 | `e5d3e749e` search serving, followed by `15adf53b3` compaction overflow contract |
| 11 | `df9ffaf44` mapped identities and `4b01268fa` stale-session identity gates and test coverage |
| 12 | `247cbaceb` thread-valued reads |
| 13 | `6a84de9c3` changes |
| 14 | `092414a26` rebuild and reconciliation, including plan 25/spec 32 corrections |
| 15 | `5af0a0ed7` compaction |
| 16b | `8db38f50f` operator reindex, lifecycle and recode integration |
| Warm admission | `b51a5e193` bounded warm-update memory correction |
| 17 | `070c1d3a4` key protocol suite coverage |
| 18 | `b62932691` privacy and parity coverage; full canonical ZA 1664.38 s, exit 0, after scoped review. |
| 19, intermediate | `ea67be7f3` fixture controls, `28f84bdc6` system benchmark and controls, `f7c02e5bc` failure diagnostics, `fff5e6c10` benchmark client ceiling, and `f4eb2ecc` M30 test opener. Focused independent reviews accepted these changes. |
| 19, consolidated correction | `53062c367ac0b85596017fed6ebb50edd8a66567` addresses WB1–WB3. [Implementation review](2026-11-07-zero-access-16-search-implementation-review.md) committed separately at `4dde13ade2be92411d76eb7906b14a0423359dc9`. |

A later test-only follow-up integrates the independently reviewed T13 Content
caller-floor regression from the [remaining-checks report](2026-11-07-zero-access-16-search-remaining-checks-review.md).
Its full-suite qualification passed with both default and FileSystem blob storage.
The production implementation and accepted defaults are unchanged.

Task 19 qualification remains incomplete. Both final-head benchmarks
completed on `4dde13ade`. The post-H7 query served a match with a wire
limit of 1, and separate internal assertions verified Content membership
and Thread record existence for all 30,000 delivered IDs. The full-size
cold query refused and its linked warm repeat was skipped.
[System benchmark](#final-head-system-benchmark) and
[format benchmark](#final-head-format-benchmark).

The later [cold-start qualification](2026-11-10-zero-access-search-cold-start-outcome.md)
records a 30,000-message optimized Rust pair after in-process cache
eviction. Both queries returned the expected result in 26.469 s and
507.458 µs, with exact-head internal coverage and unchanged durable state.
The run logged a teardown panic after measurement. The separate optimized
OSS restart pair served in 23.053 s and 1.026 ms, with matching public
censuses across restart. Local 30,000-message cold-search qualification
is complete. All fourteen search settings are unchanged. Memory and
account sizing defaults remain provisional.

The three [mutation campaigns](#mutation-runs-plan-16) now account for all 79
rows at their recorded heads. M16, M23 and M48 survived the executed campaigns;
the later test-only follow-up passed static review, all 18 planner tests and the
full canonical RocksDB ZA suite, without new deliberate-fault runs. M29b remains
unexecuted because it requires a codec change rather than a bounded fault. These
are explicit draft-PR qualification exceptions, not passed or equivalent mutants.
Alternative-catcher and unobserved-branch findings remain recorded below.

Locally accounted for are 39 of 41 main Step 5 rows on `4dde13ade` and
source-identical implementation `53062c367`. Four source-identical rows
were carried, 33 passed, one SMTP row qualified with its one permitted
exact retry, and one system harness completed with the serving limits
recorded for that historical run. The retry is separate from the main-row
count. Two Docker rows remain for CI. [Matrix ledger](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-progress.json)

| Step 5 group | Status |
|---|---|
| Formatting and builds | Workspace formatting and the warning-free shipping check carried. HTTP formatting and test-target check passed in Phase A. The Phase C shipping release build passed without warnings. |
| Units | JMAP ZA carried. The remaining unit rows passed in Phase A, including Email ZA and paired arrival. |
| Fork integration | Canonical RocksDB ZA carried. FileSystem blob ZA, isolated cache swap, and plain/key WebDAV, IMAP and JMAP passed in Phase B. |
| SMTP and upstream | Initial full SMTP failed and its permitted exact retry passed. All other local upstream selections in Phase B passed. Docker CLI was absent, so the automation and directory rows remain for CI. |
| CI and measurements | Both local CI scripts passed in Phase B, with two skipped Python tests. Both final-head benchmarks completed in Phase C, with system serving limits recorded separately. |

See [final matrix evidence](#final-matrix-evidence) for the completed rows.

Both independent whole-branch Astra source reviews of `f4eb2ecc` are complete. Both
requested changes. They independently found the same three issues: early
permanent skipping inside the absence window, optional Content controlling Map
availability, and stale JMAP unit-test calls. Primary accepts these as WB1 (P1),
WB2 (P2), and WB3 (P2). The privacy reviewer graded WB1 P2. Both subsequent scoped
Astra reviews passed requirements and correctness for the accepted correction,
addressed all three findings and reported zero new findings. The tracked
[implementation review](2026-11-07-zero-access-16-search-implementation-review.md)
records the dispositions.
See [fix validation](#whole-branch-fix-validation).

## What changed and why

**Task 1, §§1.3 and 9.6.** Measured the format half of the benchmark using the upstream stored-document encoder via a test-mode accessor. The measured positions and compression comparisons inform the fixed positions and Term dictionary decisions and the provisional memory sizing. The format corpus uses text attachments, which bounds the reported open/index rate.

**Task 2, §§4.2, 4.4 and 8.** Derived a zeroizing search key under the separate `za/v1/search` label, reserved Principal field 152, defined the public bounds and startup configuration, and added account-scoped test controls and fixed traces. Publishing uses an exact minimum class even for non-power-of-two values. Startup refuses unsupported knob combinations before serving.

**Task 3, §§5.1 and 6.1–6.3.** Added the thread part, with subject-bucket candidate counting, deterministic winner and staged aliases visible within a pass. Revision-indexed resolution preserves the identity before and after publication, including the singleton fallback and bounded alias traversal.

**Task 4, §§4.2 and 7.1–7.4.** Added content records with upstream terms/positions and comparators, exact/prefix postings, and phrase resolution. Resolve content leaves before using the existing query merge; an undecodable phrase record refuses the query. Positions and per-record Term frames carry forward.

**Task 5, §4.2 and §5.7.** Added independently authenticated sections and predecessor identities, fixed Publishing padding, compaction padding from public counts, a keyed manifest tag calculated after assigned IDs are resolved, and a structurally checked keyless destroy inventory. Readers use the stored framing.

**Task 6, §§4.3, 5.2 and 5.6.** Added single-flight loading, now per account and independently loaded part after WB2, along with invalidation, soft thread and hard content accounting, access-driven idle sweep, and typed load outcomes. Charges follow the final holder, including a reader of an evicted generation. Retrying a moving content head retains the request's deadline and drops staging on refusal.

**Task 7, §§4.2, 5.1 and 6.1.** Added physical change-record reads with an inclusive partial checkpoint and an exclusive complete checkpoint, retained-bound probes, and production row/blob openers. Capture the bounded threading fields during the normal open and apply the missing-blob retry and loss classes.

**Task 8, §§4.2, 5.1 and 6.3.** Added the bounded planner and atomic writer. The asserted manifest batch turns upload holds into Principal links and publishes the fixed keyless witness entries. Setup writes the empty manifest with the vault. The request swaps its installed generation after commit. The initial task used temporary teardown unlinking, removed after Task 16a supplied service cleanup.

**Task 9, §§5.2 and 7.4.** Connected owner-keyed JMAP and IMAP dispatch and bounded Content waiting. JMAP coordinates the post-adding pass at request scope. Keyed IMAP preserves one pre-pass per command even where ordinary dispatch groups commands. Busy/pass waiting is checked before loading, with one original deadline. Principal links enter the recode inventory.

**Task 10, §§6.2 and 7.1–7.4.** Served JMAP search/snippets and IMAP SEARCH/SORT using one admitted Map or Content view as required. Resolved content leaves join local predicates and existing sorting, while missing current Content refuses the whole request. Fixed saved-search capture before holding a keyed view when the operation may await a producer.

**Task 11, §6.2.** Projected mapped identities through a validated cache/manifest snapshot at the wire boundary. Persisted rows stay singleton. Applied the keyless dispatcher table and gated stale pre-provisioning owner IMAP identities against the selected account while retaining ordinary share and metadata controls.

**Task 12, §§6.1–6.2 and 7.** Applied the admitted thread map to members, counters, query predicates and rankings, submission identities and IMAP grouping. Access, visibility and original ordinary-account algorithms remain the inputs. Shared fixture cleanup preserves the Email identity namespace and history needed by the retained generation.

**Task 13, §6.3.** Rendered Email and Thread changes over unfolded lifecycle records and sealed identity history. Exact, Initial and continued pages use pinned endpoints with retention/epoch validation. Keyed queryChanges borrows one view and uses historical presence and thread identities with current readable rows and nonidentity sort values.

**Task 14, §5.4 and §5.8.** Rebuilt thread identities on proved loss from an exact asserted base, writing a boundary even without a mail witness. Reconciled retention gaps, and kept content loss separate from identity loss. Cap-raised and behind cases keep existing content and backfill missing records. The canonical integration correction changed one fixture's checkpoint expectation.

**Task 15, §5.5.** Folded thread history with a floor revision and kept deleted bridge records and their tombstones while the assigning publication is retained. Thread rebuild applies deletions only after its chronological cursor is exhausted. For content compaction, any pending rebuild cursor, content cursor or reconciliation marker limits a due merge to the tiered path. Added bounded merge planning, final-only upload of same-pass outputs, typed tier overflow, atomic link replacement and charge ownership through cold decode and merge. The initial review identified a separate warm-write admission path corrected later.

**Task 16a, §5.7.** Moved the destroy prerequisite ahead of dependent recovery. The production destroy marker fences passes; exact Principal/document-zero scans recover incomplete inventory. Each destructive batch carries the applicable captured fence. Resets also assert live Account. With absent manifest and vault, permanent destroy scans without installing a marker.

**Task 16b, §§4.2, 5.4 and 5.7.** Connected account Reindex to keyless marker/unlink/clear and kept ordinary reindex behavior. Real queued work exercised nonempty and zero-row reset, content recovery before destroy, a modeled stale-reset overlap, and adaptive BlobRecode selection and byte identity.

**Warm admission, §§4.3 and 5.5.** Reserved actual retained staging capacities and bounded scratch and projected resident growth before touching warm content. This closed the Task 15 warm-writer finding. Its canonical RocksDB result was 829.79 s, exit 0. The final 30,000-message loaded budget remains a separate measurement.

**Task 17, §§6.2, 7.5 and 9.3.** Reused ordinary IMAP search/thread bodies in both modes, narrowed retained keyed variants and restored the equivalent JMAP outcomes using explicit content-current fixture boundaries. Reused upstream received-order behavior for keyed Thread/get and bound keyed EventSource import assertions to the real durable insert and latest published state. Functional bulk setup must stay outside timed benchmark backlog preparation.

**Task 18, §§7.5, 9.4 and 9.5.** Extended the leak scanner to manifests and Principal-linked index blobs, added the privacy/oracle controls, and ran ordinary/key parity on 3,258 matched raw pairs. Chronology is controlled through settled paired delivery and ordinary thread work, with shared arrival inputs verified from stored metadata. Three headerless records explicitly refuse LMTP on both accounts and use byte-exact paired IMAP APPEND. The exact K split, 183 served forms, G5 and focused teardown passed. The final canonical RocksDB suite passed in 1664.38 s after the R16 ordinary control was corrected to preserve the same insert-before-delete order as the key account.

**Task 19, §§8 and 9.6.** Added fixture controls for the production timeout
classifier, the shorter Thread retention marker, precommit map visibility,
a post-unlink writer, and the creator path with an absent manifest. The
system harness uses the byte-compatible lazy generator, separates normal,
concurrent and recovery workloads, and records committed write work and
sampled memory. The canonical setup asserts the authenticated empty
manifest created during key activation. A separate loaded-growth fixture
clears the pristine manifest to exercise serial tiered rebuild and labels
its milestone warming. Diagnostic1 streams settled rows and records typed,
redacted client failure information with the last settled request. The
subsequent harness commit gives long owned compactions more measurement
headroom. The separate M30 correction completes the declared fixture input.

## Historical benchmark scope

The Benchmarks and Defaults sections immediately below record Task 1
and the system attempts through `f4eb2ecc`. Their qualification statements
describe those runs. See the [final-head system benchmark](#final-head-system-benchmark)
for current system evidence and default assessments, and the
[final-head format benchmark](#final-head-format-benchmark) for the
format repeat.

## Benchmarks

### Format benchmark (Task 1)

Apple M1 Max with 64 GiB of RAM, macOS 27.2, and a fresh local RocksDB store.
Rust 1.99.0 ran the unoptimized test profile (`opt-level = 0`, `debug = 1`)
with `test_mode`. The server used the default test listeners and the normal
key-account setup and SMTP ingest paths. Throughput figures describe this
test build.

The seed-1 corpus generated 15,000 messages and 1,217,729,696 raw MIME bytes
(81.2 KB per message, including MIME encoding and framing). It kept the
specified decoded body and attachment size ranges. It included 9,000 replies,
1,500 forwards, 4,500 roots, 1,500 subjects with non-ASCII words and 300
repeated Message-IDs. There were 3,750 medium and 750 large attachments.
References reached 40 entries, and 720 messages (4.80%) had more than 16
entries in the References header.

For each stored message, the benchmark opened the metadata row and blob and
ran the upstream forward-index builder. The attachment control removed the
Attachment search field. Its encoded term table retained the body canary and
excluded the attachment canary. The benchmark alternated which field set opened
each message first. Each indexing timer includes the row and blob reads and
opens, tokenization, and both raw forward-index encodings. Thread statistics,
compression and decompression ran outside those timers.

In this test-mode build, unknown-language fields use the server's configured
default language. Production uses the most frequently detected language when
one is available. This can affect the token and position measurements.
[Upstream language selection](../../../crates/email/src/message/index/search.rs#L141-L151)

The size table uses raw forward-index payloads before any storage marker or
compression. Each per-record comparison uses the named codec option and each
aggregate comparison uses one no-dictionary frame. Decompression uses the
per-record Term frames and one reused scratch buffer.

The ingest loop took 219.742472 s. The full benchmark completed in
1,211.24 s.

| Figure | With positions | Without positions |
|---|---|---|
| Content term bytes, total / mean / p95 per message | 436,627,716 / 29,108.514 / 89,038 | 265,473,368 / 17,698.225 / 55,513 |
| Per-record zstd, no dictionary, total | 233,862,464 | 87,764,043 |
| Per-record zstd, `Dictionary::Term`, total | 224,811,653 | 79,756,036 |
| Per-record zstd, `Dictionary::Email`, total | 235,790,801 | 90,248,987 |
| One frame over all records, total | 208,247,722 | 58,955,211 |
| Projected content part at 30,000 messages (mean × 30,000) | 873,255,432 | 530,946,736 |
| Decompression of every per-record frame, seconds | 3.371493 | 2.425538 |

For G5, dividing the per-record Email row by the one-frame row gives 1.1323×
with positions and 1.5308× without. Using the production Term dictionary in
that comparison gives 1.0795× and 1.3528×. With no dictionary on either side,
the per-record-to-one-frame ratios are 1.1230× and 1.4887×.

The proposed thread record layout, `min(16, distinct hashes) × 16 + 48`
bytes per message, averaged 88.160 bytes and reached the derived bound of
304 bytes. It projects to 2.52 MiB of thread records at 30,000 messages.
750 of 15,000 messages (5.00%) had more than 16 distinct hashes.
The open-and-index rate was 27.879 documents/s with attachment text and
56.793 documents/s with the attachment field disabled.

1. **Positions stay.** Phrase matching needs them. The measured Term frames
   with positions total 214.40 MiB at 15,000 messages. At 30,000 messages
   the with-positions projection is 832.80 MiB of raw term payloads or
   428.79 MiB (449,623,306 bytes) of Term frames. The without-positions
   comparison is 506.35 MiB raw or 152.12 MiB (159,512,072 bytes) of Term
   frames. These projections cover term payloads. Record fields and
   postings are additional. Actual allocation overhead and transient
   memory remain unmeasured.
2. **Use the upstream Term dictionary.** Term frames were 51.49% of raw with
   positions and 30.04% without. No-dictionary frames were 53.56% and
   33.06% of raw. Email frames were 54.00% and 34.00% of raw, or 4.88% and
   13.16% larger than Term.
3. **Use provisional 512 / 1,536 MiB limits.** On 2026-11-08 Jay approved
   `ZA_SEARCH_ACCOUNT_MAX_MB = 512` MiB and `ZA_SEARCH_MEM_MB = 1536` MiB.
   Every cluster node uses the same account cap, and the account cap remains
   at most one-third of the process content budget. These replace the prior
   128 / 512 MiB defaults. Task 19 must confirm them against the completed
   model's loaded size; the payload projections do not establish that size.
   Other section 4.4 defaults stay. The estimated thread record maximum is
   within the 304-byte bound.

#### Task 1 validation

The accessor test first failed with the expected missing-method errors, then
passed; all seven store search tests passed. The ignored format benchmark
passed with 2,000 messages and with 15,000 messages. The initial sandboxed
2,000-message attempt stopped at listener/DNS setup; its approved rerun used
the same synthetic workload and completed successfully.

The following checks passed, using the repository's prescribed Rust PATH:

- `cargo test -p store --lib --features test_mode za_bench_term_bytes_counts_positions`
- `cargo test -p store --lib --features test_mode search::`
- `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::search_bench::format -- --exact --ignored --nocapture`
- `cargo check -p stalwart --no-default-features --features rocks`
- `cargo check -p tests --tests`
- `cargo fmt -p store -p tests -- --check`

The store builds repeated the baseline unused `key` warning in
`crates/store/src/dispatch/blob.rs`; Task 1 left that file unchanged. The linked test
artifact also carried Darwin's `__eh_frame` size warning. These runs are
passing, but their output is not warning-free. No new warning remained in
Task 1's changed source.

Task 1 used `gpt-6.1-sol` at medium reasoning for implementation and independent
`gpt-6-astra` at xhigh for review. The reviewer approved the code, measured
evidence and provisional-default documentation with no P0–P2 findings;
the existing warnings above remain disclosed. This is task acceptance, with
whole-branch and loaded-memory qualification still pending.

### System benchmark (Task 19)

The first two full attempts failed. The extended-client attempt completed
all workload phases with exit 0. Its full-size cold and post-H7 queries
returned unavailable.
The full system command is
`STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::search_bench::system -- --exact --ignored --nocapture`.
The runs use local RocksDB with fresh temporary state, Apple M1 Max
(10 cores, 64 GiB, arm64), APFS on a 3,996,329,328,640-byte SSD, and
Homebrew stable Rust/Cargo 1.99.0 in the test profile. The fourteen
search overrides, `BLOB_STORE`, `ZA_KEY_ACCOUNTS`, `NO_INSERT` and
`SEARCH_BENCH_MESSAGES` are unset. Wire times, process samples and local validation reads describe the
unoptimized test build. Production latency remains unmeasured.

| Attempt | Head and host | Compile, test and wall time | Outcome |
|---|---|---|---|
| Initial, 2026-11-09 18:38:34–19:07:58 UTC | `28f84bdc6`, macOS 27.2 build 26B123, available space 42,500,588 → 30,802,296 KiB | 48.82 s, 1,706.95 s, 1,763.49 s | **Failed**, exit 101. Email/get send error before response headers after the 10,018-document milestone. The original error category, request elapsed and server outcome are unknown. |
| Diagnostic1, 19:47:02–20:20:06 UTC | `f7c02e5bc`, post-reboot macOS 27.2.1 build 26B131, available space 22,547,292 → 27,169,616 KiB | Cargo displayed 3m 12s, test 1,782.35 s, wall 1,984.51 s | **Failed**, exit 101. Normal-index request 256 hit the 60 s client send deadline before headers. The last settled request had indexed 11,649 of 30,000. |
| Extended client, 2026-11-09 20:41:14–23:36:01 UTC | `f4eb2ecc`, macOS 27.2.1 build 26B131, available space 26,009,792 → 32,254,008 KiB | Cargo displayed 5m 04s, test 10,174.17 s, wall 10,487.26 s | **Harness completed**, exit 0, one selected and passed, 111 filtered. Client 600 s, direct milestone load 60 s, phase 4 h, whole benchmark 12 h. Cold and post-H7 Content serving remain unqualified. |

Filesystem figures describe host-wide available space. The first two
runs reported the existing Darwin compact-unwind linker warning and
neither phase nor whole-run watchdog fired. The extended-client compile
logged the same linker warning. Its run record notes that only the compiler
incremental cache was cleared before launch. The extended-client change
adjusts the benchmark observation window. Product deadlines, the
workloads, assertion predicates and search defaults are unchanged.

#### Initial retained rows

| Figure | Observed |
| --- | --- |
| Attachment-free pass rate | 2,000 delivered/indexed, 0 attachments, 10,251,442 MIME bytes, 13.15 s delivery. 42 commits over 20.316943249 s of committed-request time, 98.44 messages/s. 43 requests, 20.328060375 s phase wall, 42 beyond 100 ms cadence, 1 final noncommit. Currentness reached, no abandoned attempts. |
| Mixed pass rate | 2,000 delivered/indexed, 600 attachments, 165,466,687 MIME bytes, 34.90 s delivery. 43 commits over 94.407127921 s of committed-request time, 21.18 messages/s. 44 requests, 94.417970375 s phase wall, 43 beyond 100 ms cadence, 1 final noncommit. Currentness reached, no abandoned attempts. |
| Validation | The run advanced past the 500 keyed, 500 plain and 500 raw local reads and their assertions. Latency values existed only in the unprinted final table. PostgreSQL remains pending. |
| Mailbox property changes | The run advanced past the 10- and 100-mailbox assertions. Durations were not emitted before the later failure. |
| First search after 200 deliveries | 200 messages, 60 attachments, 17,298,000 MIME bytes in 8.985063458 s. The method completed before the design fixture, but its unprinted row can represent either a match or an expected refusal. Neither result, refusal count nor latency is established from the retained log. |
| Design size | 30,000 delivered, 9,000 attachments, 2,436,029,084 MIME bytes in 524.467284125 s. The source then checked setup bytes and mailbox census before the observed serial growth. Default-size indexing stopped before the 15k milestone. |
| Thread/full content compaction timing | The design serial phase did not finish or emit its spans/compaction rows. The mixed request spans contain outliers, but no actual per-request compaction markers were emitted for that cohort. |
| Cold 30,000 serving, H7/R32, D11 | Not reached. No cold admission, full ID coverage, rebuild/first-serve/refusal figures, exact committed rewrite totals or R32 bounds are established. |
| Separate serial tiered compaction/loaded growth; 1 MiB cap lifecycle | Not reached. No physical 512 MiB crossing was observed in this partial run. |

| Normal design milestone | Actually indexed | Sealed bytes / chain length | Modeled and charged thread bytes | Resident content | Sampler reading |
| --- | ---: | ---: | ---: | --- | --- |
| 2,000 | 2,039 | 31,327,025 / 14 | 265,176 | absent | 98.798 s, content ledger 0, thread ledger 256,816, RSS 680,050,688 B |
| 5,000 | 5,036 | 75,858,974 / 5 | 650,448 | absent | 315.911 s, content ledger 0, thread ledger 641,280, RSS 927,154,176 B |
| 10,000 | 10,018 | 151,255,090 / 10 | 1,288,304 | absent | 807.237 s, content ledger 0, thread ledger 1,282,688, RSS 2,388,951,040 B |

The first run retained no completed final table. Its terminal small-cohort
Email states were 2,043 and 2,044, with persisted coverage one change
behind and no resident Content. The sampled delivery peak RSS was
885,194,752 B with zero search ledger. The last verified partial sealed
Content was 144.25 MiB, leaving 367.75 MiB to the 512 MiB cap at that
point. The failure was at the Email/get send site. The retained Display
message gives no cause or duration. Six cap-sized mixed requests
exceeded 2 s end to end (2.104, 5.637, 8.953, 2.035, 2.057 and 2.126 s).
The source and the commit cadence support compaction as a possible
explanation for the two longest spans, with per-request attribution absent.

#### Diagnostic1 retained rows

| Figure | Observed |
| --- | --- |
| Attachment-free pass rate | 99.48 messages/s. 2,000 delivered/indexed, 0 attachments, 10,251,442 MIME bytes, delivery 15.4357 s. 42 commits over 20.105000542 s committed-request time. 43 requests, 20.117670375 s phase wall, 42 over 100 ms, one final noncommit. |
| Mixed pass rate | 21.07 messages/s. 2,000 delivered/indexed, 600 attachments, 165,466,687 MIME bytes, delivery 34.2302 s. 43 commits over 94.938602628 s committed-request time. 44 requests, 94.950618292 s phase wall, 43 over 100 ms, one final noncommit. |
| Validation cost | RocksDB keyed 500-read median/p99 0.387/0.608 ms, plain 500-read 0.279/0.477 ms, raw field 152 500-read 0.022/0.041 ms. PostgreSQL remains pending. |
| 10 mailboxes | 10 updated. Publishing get 19.661 ms, Mailbox/changes 2.802 ms. |
| 100 mailboxes | 100 updated. Publishing get 19.642 ms, Mailbox/changes 2.609 ms. |
| First search after 200 deliveries | 20.137505 s and five committed passes. Last delivery matched. 200 delivered, 60 attachments, 17,298,000 MIME bytes, 9.1545 s delivery. |
| Canonical design size | 30,000 delivered, 9,000 attachments, 2,436,029,084 MIME bytes, 520.338104709 s delivery. Setup and mailbox census checks reached normal indexing. 255 settled committed requests at 11,649 indexed. Not content-current or fully loaded. |
| Partial normal compaction timing | 16 settled joint thread/full-content merges, 1,429,008,776 cumulative encoded rewrite bytes. Latest was #241 at 11,006 indexed, 59.639704083 s and 166,394,883 rewritten B. These are normal full-merge results, not H7/R32. |
| Canonical 30,000 cold coverage, H7/R32, D11, serial tiered recovery, controlled 1 MiB cap lifecycle | Not reached. No full-size cold admission, complete 30,000 ID coverage, tiered request timing, rebuild or R32 qualification. |

| Normal design milestone | Indexed | Sealed bytes / chain length | Modeled and charged thread bytes | Resident content | Process sample |
| --- | ---: | ---: | ---: | --- | --- |
| 2,000 | 2,003 | 30,550,383 / 13 | 257,384 | absent | 96.096 s, content ledger 0, thread ledger 252,256 B, RSS 947,863,552 B |
| 5,000 | 5,036 | 75,858,749 / 4 | 650,432 | absent | 311.085 s, content ledger 0, thread ledger 641,264 B, RSS 1,508,245,504 B |
| 10,000 | 10,021 | 151,274,347 / 9 | 1,288,600 | absent | 803.120 s, content ledger 0, thread ledger 1,282,152 B, RSS 2,815,967,232 B |

| Settled request | Indexed | End-to-end seconds | Encoded rewrite bytes |
| --- | ---: | ---: | ---: |
| 31 | 1,445 | 8.723186 | 21,972,960 |
| 106 | 4,896 | 26.864623 | 74,037,726 |
| 166 | 7,636 | 40.785546 | 115,084,990 |
| 196 | 8,986 | 48.971355 | 135,754,799 |
| 211 | 9,679 | 52.097062 | 146,076,273 |
| 226 | 10,345 | 55.677921 | 156,116,322 |
| 241 | 11,006 | 59.639704 | 166,394,883 |

Both small cohorts reached currentness without abandoned attempts.
Streaming happens after the measured response and its settled observations.
Printing is outside wire timing and inside phase wall time, where it can
influence missed-tick cadence. The sampled delivery peak RSS was
912,261,120 B with zero search ledger. Last settled sealed size was
167.69 MiB, leaving 344.31 MiB to the 512 MiB cap at that point.

Request 256 began at 984.432256083 s into normal indexing and failed
during send after 60.00179075 s. Flags were `timeout=true`,
`request=true`, and `connect/dns/body/decode=false`, with an
unclassified source chain. Request 255 settled at 11,649 indexed and
scanned, Email state 30,256, epoch/revision 0/255, complete indexing
and coverage checkpoints at 11,649, 175,832,683 sealed bytes in 15
segments, and no cursor or suspension. The last completed full merge
was request 241 at 59.639704083 s. Prior settled compactions and the
15-segment chain make a full merge due if request 256 staged Content.
Its actual server stage and outcome are unknown. The initial failure
also remains unclassified.

Seven diagnostic mixed 48-item requests exceeded 2 s end to end, two
with full merges and five without. The settled design requests included
50 such spans (11 full and 39 without a full merge), with non-full
median/max wire time of 2.028088459/3.006312625 s. The 2 s planner guard
limits item admission. Those wire durations include later work and
transport. The measurement policy now permits observation of longer
requests and supplies no evidence of a latency improvement.

In both failed runs, sampling could precede the nearby guarded model
reading. RSS describes the whole process, separately from model-owned
bytes and search ledger charges. The observed RSS maxima are lower
bounds and neither failed run emitted its final serial memory summary.
Exact committed rewrite work uses encoded output counters, separately
from sampled memory and I/O.

#### Extended-client measurements at the frozen benchmark head

The [completed benchmark report](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-system-long-request-report.md) and
[primary verification record](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-system-long-request-primary.json)
bind these measurements to `f4eb2ecccb073fea34fefa116743644732f9dedc`.
Primary verified that all seven benchmark source hashes matched before
and after the run and that the two earlier failed logs were unchanged.
The [benchmark log](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-system-long-request.log#L25-L849) records the
phases through D11. The 30 s Content deadline and the 600 s wire watchdog
apply to different parts of the call.

| Figure | Observed |
|---|---|
| Attachment-free pass rate | 2,000 delivered/indexed, zero attachments, 10,251,442 MIME bytes, delivery 13.559554458 s. 99.60 messages/s, 20,080.934 ms of committed-request time, 42 commits, 43 requests, 20.093933709 s phase wall. 42 over cadence, one final noncommit, zero abandoned attempts. |
| Mixed pass rate | 2,000 delivered/indexed, 600 attachments, 165,466,687 MIME bytes, delivery 35.965393875 s. 20.78 messages/s, 96,240.315 ms of committed-request time, 44 commits, 45 requests, 96.253327125 s phase wall. 44 over cadence, one final noncommit, zero abandoned attempts. |
| Validation cost | RocksDB median/p99 over 500 reads per case, keyed 0.386/0.584 ms, plain 0.270/0.427 ms, raw field 152 0.025/0.053 ms. Networked PostgreSQL remains pending. |
| 10 mailboxes | 10 updated. Publishing get 27.780 ms, Mailbox/changes 3.494 ms. |
| 100 mailboxes | 100 updated. Publishing get 10,471.885 ms, Mailbox/changes 3.290 ms. The publishing span has no separate server-stage marker. |
| First search after 200 deliveries | 9,560.716 ms after five committed passes, last delivery matched. Delivery took 9.297955666 s for 200 messages, 60 attachments and 17,298,000 MIME bytes. |
| Normal design-size serial indexing | 30,000 delivered, 9,000 attachments, 2,436,029,084 MIME bytes, delivery 519.758299333 s. 656 GETs, 655 commits and 30,000 Map records over 4,593.904822667 s phase wall, including 4,592.021462210 s of committed-request wire time. 655 requests over cadence and zero abandoned Content attempts. The final noncommitting GET brought scanned and Email state to 30,656, with complete durable checkpoint and coverage at 30,655 and epoch/revision 0/655. Resident Content was absent at the final indexed milestone. |
| Normal sealed cap | 452,374,836 B of sealed Content in 10 segments, leaving 84,496,076 B (80.58 MiB) to the default 512 MiB cap. This run did not reach a 512 MiB sealed-size crossing in this phase. |
| Normal Thread/full Content compaction GETs | 43 requests logged both compactions, with 9,853,440,397 B of encoded Content rewrites summed from the settled rows. Joint minimum/median/maximum wire spans were 5.500881791/77.850464167/156.899334959 s. The last, request 646 at 29,597 indexed, rewrote 446,315,948 B. Both benchmark timing rows report 156,899.335 ms from that same GET. These spans do not separate server stages or the cost of the two compactions. 124 cap-sized normal publishing requests exceeded 2 s end to end. |
| Default cold wire query | Unavailable after 180,225.239 ms, 20 counted blob reads, zero commits and one `not_current` refusal. The refusal bypassed guarded head, cardinality, per-document coverage, result-match and warm-repeat checks. |
| H7/R32 rebuild and query attempts | Cursor exhaustion at 1,790.006009083 s. 896 GETs, 51 queries and 51 `not_current` refusals. No successful Content query. 658 commits and Content writes, 43 Thread compactions, max Content chain 31 against derived bound 46. Tier 1 rewrote 451,460,981 B in 41 merges and tier 2 rewrote 351,166,654 B in two merges. Tiered total 802,627,635 B against 452,446,568 B original (1.774×), with 0 B full rewrites. Uploaded and linked each totaled 1,205,038,256 B. No attempted-but-unlinked Content uploads or busy/absent progress samples were logged. |
| D11 retained-state changes | Email/changes 36.090 ms and Thread/changes 14.793 ms end to end, one creation each. |

| Normal design milestone | Indexed | Sealed B / segments | Guarded Thread estimate and charge, B | Nearby sampled Content charge / Thread charge / RSS, B |
|---|---:|---:|---:|---|
| 2,000 | 2,007 | 30,710,658 / 13 | 258,088 | 0 / 252,568 / 1,065,680,896 |
| 5,000 | 5,036 | 75,859,196 / 4 | 650,432 | 0 / 641,264 / 1,820,360,704 |
| 10,000 | 10,036 | 151,583,378 / 8 | 1,292,976 | 0 / 1,284,288 / 3,133,030,400 |
| 15,000 | 15,023 | 226,453,788 / 12 | 1,931,800 | 0 / 1,924,920 / 3,002,712,064 |
| 20,000 | 20,010 | 300,681,434 / 15 | 2,572,032 | 0 / 2,565,936 / 3,643,981,824 |
| 25,000 | 25,018 | 377,402,249 / 6 | 3,215,968 | 0 / 3,209,472 / 2,307,194,880 |
| 30,000 | 30,000 | 452,374,836 / 10 | 3,855,424 | 0 / 3,852,928 / 1,734,148,096 |

Resident Content was absent at each of those guarded milestones. Normal indexing completed 27 requests beyond the earlier 60 s ceiling.
Normal request 256 took 60.785053125 s with one Thread compaction and
one full Content compaction, 177,935,034 B rewritten, and 11,794 indexed.
This separate request cannot establish the server outcome of diagnostic1
request 256. The final Thread inventory had 17,376,490 B sealed, with 16,777,216 B as the oldest
retained hidden class. Nearby sampler observations can precede the guarded
read. The completed delivery phase's sampled RSS peak was 860,930,048 B
with both search ledgers at zero.

| Completed phase | Peak sampled Content charge, B | Peak sampled Thread charge, B | Peak sampled process RSS, B |
|---|---:|---:|---:|
| Attachment-free serial indexing | 10,393,224 | 2,813,024 | 283,115,520 |
| Mixed serial indexing | 66,749,940 | 2,720,468 | 687,177,728 |
| Normal design-size serial indexing | 1,340,386,997 | 53,129,828 | 5,563,662,336 |
| Default cold query | 1,357,124,508 | 52,129,470 | 3,579,084,800 |
| H7 rebuild and serving attempts | 1,357,124,919 | 67,239,699 | 4,741,955,584 |
| Separate warmed growth | 1,357,798,124 | 53,092,580 | 6,124,175,360 |

The independently sampled peaks are lower bounds and need not coincide.
The largest Content charge sample was 1,294.90 MiB, 241.10 MiB below the
1,536 MiB budget at that sample. The largest Thread charge sample was
64.12 MiB. A ledger tracks budget
charges, including transient reservations. The model estimates describe
modeled ownership, physical allocation was unmeasured, and RSS covers the
whole process. The cold peak equals the initial `3 × sealed bytes`
reservation. The last cold sampled Content charge was 577,304,880 B and
persisted into the first H7 sample. That supports a retained completed load,
with no verified cold endpoint or guarded loaded-model estimate. It does
not establish the load's head, count or coverage.

H7 settled at epoch/revision 30657/659 with 30,000 live Map records,
scanned and Email state both 31,315, and complete checkpoint and coverage
at 30,656. The settled manifest had no cursors or reconciliation, 452,374,973 B
in 13 Content segments, a 3,855,472 B Thread estimate and charge, and no
resident Content. The settled snapshot does not print the suspension bit.
The GET cadence target was 2 s, with three durations over it. The query
cadence target was 5 s, with all 51 durations over it. Median/max wire spans
were 1.268 ms/3.003387 s for GET and 33.987572/96.521751 s for query.
Three refusals were received after observed cursor
exhaustion. The first of those requests started at 1,775.310557625 s and
straddled the boundary. Only the requests started at 1,839.240056375 s
and 1,888.905759542 s began afterward. The three wire spans were
63.929255875 s, 49.665534584 s and 48.157741916 s in the same order. Map
settlement and a matching
scanned/Email state do not qualify Content serving. The settled 13-segment
chain remained below the 16-segment full-merge trigger. Zero full-rewrite
bytes record no completed full merge, and the cursor-clearing write did
not exercise an eligible final full merge.

The [source-only refusal assessment](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-cold-serving-investigation-report.md)
traces the awaited pre-method pass, the subsequent absolute Content
deadline, and expiry paths in ensure, load and final view validation. The
2 s pass guard bounds item admission. The 30 s Content deadline governs
cooperative waits and checks inside the method. Owned work and synchronous
decode or assembly can run across those checks. `not_current` identifies
the deadline family without identifying the expired stage. The cold
ledger is consistent with a load that survived refusal. H7 only shows
Content absent at settlement. The observations leave both exact causes
and any common mechanism unresolved. The separate accepted WB2 Map refresh
coupling remains source evidence without attribution to these refusals.
The links below show current source locations. The linked assessment
retains the historical loader anchors for the measured head.
See the [request hook](../../../crates/jmap/src/api/request.rs#L168-L178),
[Content deadline](../../../crates/jmap/src/email/za_search.rs#L80-L98),
[ensure](../../../crates/email/src/message/za/search/ensure.rs#L293-L345),
[view validation](../../../crates/email/src/message/za/search/view.rs#L103-L173),
[candidate Content loader](../../../crates/common/src/za/search/load.rs#L889-L1069) and
[compaction selection](../../../crates/email/src/message/za/search/write.rs#L284-L327).

#### Separate warmed growth and controlled cap

A second 30,000-message fixture delivered 9,000 attachments and
2,436,029,084 MIME bytes in 505.938719667 s. It cleared the pristine
manifest and warmed the model through direct milestone loads. The phase
recorded 656 commits in 657 GETs, 2,014.307582879 s committed wire time,
2,210.757434792 s phase wall, one final noncommit and zero abandoned
attempts. It performed 43 Thread compactions, 794,450,921 B of encoded
tiered rewrites and 452,373,758 B of full rewrite. The last of the 40 tiered
GET samples was request 640, at 29,299 indexed, with tier [1] and
11,183,466 B rewritten in 8.903114375 s. The slowest was request 512,
at 23,440 indexed, with tiers [1, 2] and 186,434,425 B rewritten in
64.479348084 s. The final publishing GET, request 656, closed its cursor
with 25 prior segments and performed an eligible full merge at 30,000
indexed. It took 157.518231084 s, rewrote 452,373,758 B and had no Thread
compaction. These results belong to the separate warmed fixture.

| Target | Indexed and resident count | Sealed B / segments | Resident model estimate and charge, B | Thread model estimate and charge, B |
|---:|---:|---:|---:|---:|
| 2,000 | 2,007 | 30,710,574 / 13 | 42,538,820 | 258,088 |
| 5,000 | 5,015 | 75,654,618 / 19 | 103,646,994 | 645,416 |
| 10,000 | 10,035 | 151,551,991 / 24 | 205,406,121 | 1,292,888 |
| 15,000 | 15,023 | 226,453,859 / 13 | 304,515,257 | 1,931,816 |
| 20,000 | 20,010 | 300,681,140 / 16 | 397,756,981 | 2,572,048 |
| 25,000 | 25,017 | 377,401,152 / 7 | 490,104,377 | 3,215,880 |
| 30,000 | 30,000 | 452,373,758 / 1 | 577,304,801 | 3,855,440 |

The final guarded model reading preceded the final noncommit, which
advanced scanned to the Email state of 30,658. No direct milestone-loader
refusal was logged. The approximately 183.23 s gap between request 656's
wire completion and request 657's start included settled observation,
direct loading, milestone inventory and scheduling. No isolated loader
duration or every-ID census follows from that interval or the resident count.
Diagnostic printing was outside wire timing and inside phase wall and cadence.

The small 1 MiB semantic cap fixture used 255 segments as its compaction
threshold. Sealed Content crossed from 980,019 to 1,305,899 B with Content
resident at crossing. Warm and cold wire refusals took 0.884 and 49.289 ms.
The Map and epoch survived the cap raise, and restored serving and
coverage assertions passed on this fixture. The default-sized normal
fixture stayed below its 512 MiB sealed cap.

Successful full-size cold or post-H7 serving, a guarded cold matching
head and count, every-ID coverage, an eligible final full-merge
observation on canonical H7, exact refusal stages and production latency
remain unestablished. Final-head system and format repeats and networked
PostgreSQL timing remain pending.

## Defaults

Task 1 approved the provisional 512 MiB account and 1,536 MiB process
Content limits. The 256 MiB Thread budget remains provisional with them.
Cluster nodes use one shared account cap, at most one-third of the
Content budget. All three system runs logged the fourteen effective values.
The completed extended-client run supports sealed-size headroom and warmed
residency on this fixture. One unoptimized host run supplies no basis for
a specific production knob change. The approved provisional memory settings
stay in place.

| Setting | Effective value | Assessment at the benchmark head |
|---|---:|---|
| `ZA_SEARCH_MEM_MB` | 1536 | Provisional. Warmed 30,000-record part modeled and charged at 550.56 MiB. Largest sampled Content charge 1,294.90 MiB. No observed budget refusal. Full-size wire admission remains unqualified. |
| `ZA_SEARCH_THREAD_MEM_MB` | 256 | Provisional. Final Thread model about 3.68 MiB. Largest sampled Thread charge 64.12 MiB, separate from whole-process RSS. |
| `ZA_SEARCH_ACCOUNT_MAX_MB` | 512 | Provisional. Normal 30,000 sealed size 431.42 MiB with 80.58 MiB headroom. Small 1 MiB lifecycle assertions passed. The default sealed-size threshold was not crossed. |
| `ZA_SEARCH_IDLE_SECS` | 900 | Effective. No isolated idle eviction timing. |
| `ZA_SEARCH_PASS_CAP` | 48 | Effective. 655 normal and 658 H7 committed writes. Of 124 cap-sized normal spans over 2 s, 29 included full compaction and 95 had no observed Thread or Content compaction. No alternate pass cap was measured. |
| `ZA_SEARCH_PASS_MB` | 32 | Effective. No isolated byte-admission threshold measurement. |
| `ZA_SEARCH_PASS_HIDDEN_KB` | 32 | Effective. No isolated hidden reservation threshold measurement. |
| `ZA_SEARCH_PASS_MS` | 2000 | Item-admission interval. Normal noncompacting committed spans had median/max 1.980117104/2.543886916 s. Normal joint full-compaction wire time reached 156.899334959 s. The run supplies no replacement interval. |
| `ZA_SEARCH_WAIT_SECS` | 30 | First search after 200 matched in 9,560.716 ms. The full-size cold query and all 51 H7 queries refused. End-to-end spans include other work, and the run supplies no replacement wait. |
| `ZA_SEARCH_SEGMENTS` | 16 | Normal full merges returned the chain to one. H7 peaked at 31 against bound 46 and settled below threshold at 13. The separate warmed cursor-closing write qualified and merged. The controlled cap alone used 255. |
| `ZA_SEARCH_CHECKPOINT` | 1000 | Both 2,000 cohorts and both serial 30,000 phases settled after a final noncommitting request. Cold resident per-document coverage remains unverified. |
| `ZA_SEARCH_SKIP_AFTER` | 8 | Effective. No isolated unreadable-item skip threshold measurement. |
| `ZA_SEARCH_ABSENT_RETRY_SECS` | 10 | Effective. The manifest-loss rebuild did not exercise the referenced-blob absence retry window. |
| `ZA_SEARCH_PAD_MIN` | 65536 | Effective. H7 Thread estimate 3,855,472 B against the oldest retained hidden class of 16,777,216 B. No isolated minimum-padding threshold sweep. |

## Final-head system benchmark

Measured 2026-11-10 on `4dde13ade2be92411d76eb7906b14a0423359dc9`
with source-identical implementation `53062c367ac0b85596017fed6ebb50edd8a66567`.
The host was Apple M1 Max with 10 cores, 64 GiB and macOS 27.2.1 build
26B131, using stable Rust 1.99.0. The shipping release build and this
unoptimized benchmark are separate rows.

`STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::search_bench::system -- --exact --ignored --nocapture` selected the single ignored system benchmark, using the tests crate's RocksDB/SQLite default and development features in the unoptimized debug profile. The runner used stable Rust, the plan-local Cargo target, and fresh `/private/tmp/za-final-epzwbxor`. It cleared all fourteen search overrides, `SPAM_RULES_URL`, `BLOB_STORE`, `ZA_KEY_ACCOUNTS`, `NO_INSERT` and `SEARCH_BENCH_MESSAGES`. Startup reported a 600 s client wire ceiling, 60 s direct milestone loader watchdog, 14,400 s phase watchdog, and 43,200 s whole-test watchdog. The separate product query wait was 30 s.

| Figure | Observed |
| --- | --- |
| Attachment-free pass rate | 98.62 messages/s. 2,000 delivered and indexed, zero attachments, 10,251,442 MIME bytes, 13.202018 s delivery. 42 commits over 20.280709248 s committed wire time, 43 GETs, one noncommitting, and 20.293748042 s phase wall. All 42 publishing requests exceeded the 100 ms cadence. |
| Mixed pass rate | 21.14 messages/s. 2,000 delivered and indexed, 600 attachments, 165,466,687 MIME bytes, 35.044287 s delivery. 43 commits over 94.626169002 s committed wire time, 44 GETs, one noncommitting, and 94.639294583 s phase wall. All 43 publishing requests exceeded the 100 ms cadence. |
| Local validation cost | Median/p99 in ms, 500 each. Keyed 0.381/0.672, plain 0.275/0.438, raw field 152 0.025/0.070. Networked PostgreSQL remains pending. |
| 10 mailbox properties | 10 updated, publishing GET 20.839 ms, Mailbox/changes 3.595 ms. |
| 100 mailbox properties | 100 updated, publishing GET 19.946 ms, Mailbox/changes 2.971 ms. |
| First search after 200 | 19.090253 s after five committed passes, last delivery matched. Delivered 200 with 60 attachments and 17,298,000 MIME bytes in 8.910085 s. |
| Normal design size | 30,000 delivered and indexed, 9,000 attachments, 2,436,029,084 MIME bytes, delivery 494.980857458 s. 655 commits over 4,558.263030035 s committed wire time, 656 GETs, one noncommitting, and 4,560.135944 s phase wall. All 655 publishing requests exceeded 100 ms, zero abandoned content attempts. |
| Normal thread and full-content compaction | 43 joint requests. Last and slowest #646 took 155.732645125 s at 29,607 indexed with 446,526,831 encoded rewrite bytes. This measures the joint request. |
| Default sealed cap | 452,374,470 B (431.42 MiB) sealed and 84,496,442 B (80.58 MiB) headroom. Physical 512 MiB crossing unobserved. |
| Full-size cold wire | `unsupportedFilter`, reason `not_current`, 188.973701 s, 20 blob reads, zero committed passes. The positive cold wire, head/count and every-ID checks were unqualified, as was the subsequent warm-repeat wire query on this fixture. |
| Canonical H7/R32 | Cursor exhausted at 1,896.005562333 s, first successful Content query at 2,065.773897375 s. The positive currentness, matching resident head and full delivered-ID assertions ran. 653 commits, 47 refusals, 43 thread compactions, maximum committed chain 31, 1.765× tiered ratio and 452,372,814 B of committed full rewrite. |
| D11 retained-state changes | Email/changes 36.407 ms, Thread/changes 14.767 ms, one creation each. Both figures are end-to-end. |
| Separate warmed tiered compaction | 40 committed tiered request samples with direct milestone warming. Last #640 was 8.939308 s at 29,352 indexed, tier [1], 11,306,906 rewrite B. Slowest #256 was 63.782716834 s at 11,862 indexed, tiers [1, 2], 189,770,186 rewrite B and a thread compaction on the same GET. |
| Separate warmed full merge | Final publishing GET #654 at 30,000 indexed took 154.838316208 s, 452,372,731 encoded rewrite B and no thread compaction. |

### Normal indexing and cold query

Normal 30,000 indexing advanced from scanned/checkpoint 30,655 at the final
milestone to scanned 30,656 equal to Email state on the final noncommitting
GET. That milestone had a ten-segment content head and no resident Content
part. The Thread model and charge were 3,855,424 B, total sealed Thread
17,376,490 B and oldest retained hidden class 16,777,216 B. The 43 full
merges encoded a cumulative 9,849,809,559 B. There were 27 completed
requests over 60 s. Request #256 succeeded in 61.763694375 s at 11,781
indexed, with full-content and thread compaction and 177,836,458 encoded
rewrite B. This observation belongs to the final head and its 600 s client
ceiling.

The cold request followed explicit cache eviction and zero charge checks.
The refusal returned before the positive head, loaded count, every-ID and
warm-repeat assertions. Its last sampled ledger was 577,304,514 B Content
and 3,855,424 B Thread. The ledger measures charges and cannot establish the
resident inventory or wire admission. The observed reason was
`not_current`. The request duration includes all work inside the wire
span and has no stage breakdown or observed budget/over-cap reason.

### Canonical H7

Two clients targeted one GET per 2 s and one query per 5 s. They completed
948 GETs (two over cadence) and 48 queries (47 over cadence). Median and max
GET wire times were 1.329 ms and 5.197014 s. Median and max query wire times
were 34.941954 s and 176.881854 s. All 47 refusals were `not_current`.
Only the final refusal arrived after cursor exhaustion. Its request
started at 1,894.452884666 s, before the cursor observation. The successful query started at 2,065.560994041 s,
with 0.212675708 s wire time. There were no busy or absent progress
samples. The two concurrent streams and grouped successful commits do not
provide per-request commit attribution.

The positive branch matched a delivered message through a wire query with
limit 1. The harness separately asserted currentness, the matching resident
head segment and Content cardinality. For each of the 30,000 delivered IDs,
it verified Content membership and the existence of its Thread record
before printing the settled snapshot.
[Internal coverage assertions](../../../tests/src/za/search_bench/system.rs#L491-L508) It observed 30,000 indexed and resident, 577,303,857 B of
modeled and charged Content, and matching one-segment head of
452,372,814 B. Epoch/revision was 30,657/654, checkpoint and coverage
checkpoint 30,656, Email state and scanned both 31,310, and cursors and
reconciliation absent. Thread estimate and charge were 3,855,392 B versus
a 16,777,216 B oldest retained hidden class (ratio about 0.230).

All 653 successful writer commits wrote content. Original input totaled
452,445,186 B. Tier 1 encoded 40 outputs totaling 443,853,247 B and tier 2
encoded two totaling 354,343,524 B. Total tiered rewrite of 798,196,771 B
divided by the pre-full original of 452,131,368 B was 1.765×, within the
3× bound. A single full merge wrote 452,372,814 B. Uploaded and linked bytes
were both 1,652,725,111, with zero attempted unlinked upload bytes.
Maximum committed chain 31 was below the derived bound of 46. The last
pending-cursor sample had a 22-segment chain and the settled head had one.
These are settled observations, with no isolated H7 full-merge request
latency inferred.

### Separate warmed growth and controlled cap

The separate fixture delivered 30,000 messages with the same 9,000
attachments and 2,436,029,084 MIME bytes in 484.974442916 s. The harness
cleared the pristine manifest and directly warmed at its settled milestones.
It issued 655 GETs with 654 commits, one final noncommit and no abandoned
content attempts, taking 1,994.059148077 s of committed wire time and
2,180.287706 s of phase wall. All 654 publishing requests exceeded 100 ms.
It recorded 43 thread compactions, 796,125,612 encoded tiered rewrite bytes
and 452,372,731 full rewrite bytes, 1,248,498,343 combined. Three
completed GETs exceeded 60 s. The final noncommitting GET advanced
scanned from 30,001 to Email state 30,656. No direct milestone loader refusal
was logged. Approximately 173.05 s between #654 wire completion and #655
start includes the loader, settled observation, inventory and scheduling.
This interval does not isolate loader time.

| Target | Indexed and resident count | Sealed B / segments | Content modeled and charged B | Thread modeled and charged B |
| --- | ---: | ---: | ---: | ---: |
| 2,000 | 2,007 | 30,710,692 / 13 | 42,538,938 | 258,088 |
| 5,000 | 5,040 | 75,896,246 / 18 | 104,001,755 | 650,832 |
| 10,000 | 10,032 | 151,398,618 / 21 | 205,227,088 | 1,292,288 |
| 15,000 | 15,028 | 226,482,200 / 12 | 304,556,073 | 1,933,520 |
| 20,000 | 20,016 | 300,718,709 / 16 | 397,807,365 | 2,573,200 |
| 25,000 | 25,027 | 377,431,861 / 6 | 490,146,257 | 3,218,296 |
| 30,000 | 30,000 | 452,372,731 / 1 | 577,303,774 | 3,855,408 |

The final row is a resident count with modeled and charged bytes. Full-size
warm-repeat wire serving on the normal cold fixture stayed unqualified, and
this directly warmed fixture did not run a separate query or ID census.

The small cap fixture used 1 MiB and 255 segments, with sealed Content
crossing from 980,013 to 1,305,898 B while resident. Warm and cold requests
refused with `over_cap` in 0.772 and 46.799 ms respectively. The fixture
verified map availability, stable epoch/head while suspended, and restoration
of query matches and full fixture-ID coverage after raising the cap. This
verified the semantic transition at 1 MiB. Physical crossing of the 512 MiB
default remains unobserved.

### Memory samples and effective defaults

| Phase | Peak sampled Content ledger B | Peak sampled Thread ledger B | Peak sampled process RSS B |
| --- | ---: | ---: | ---: |
| Attachment-free | 10,393,485 | 2,813,024 | 275,431,424 |
| Mixed | 67,157,824 | 2,734,228 | 677,314,560 |
| Normal design delivery | 0 | 0 | 905,052,160 |
| Normal design indexing | 1,340,730,331 | 53,146,740 | 5,783,158,784 |
| Cold probe | 1,357,123,410 | 52,129,470 | 4,205,428,736 |
| Canonical H7 | 1,357,789,961 | 53,306,372 | 7,094,255,616 |
| Separate warmed growth | 1,358,080,103 | 66,297,691 | 9,701,801,984 |

Ledger sampling targeted 50 ms and process RSS 1 s, with zero logged RSS
misses. The largest sampled Content charge was 1,295.17 MiB, 240.83 MiB
below the 1,536 MiB setting at that reading. The largest sampled Thread
charge was 63.23 MiB. Process RSS was a separate measurement that includes
allocations outside those ledgers. Peaks are sampled lower bounds and
individual table columns can peak at different times. Serial diagnostic
printing is outside wire timing and inside phase wall and cadence.

| Knob | Effective | Assessment |
| --- | ---: | --- |
| ZA_SEARCH_MEM_MB | 1536 | Retain provisional. H7 and separate warmed part measured approximately 550.56 MiB resident Content each. Peak sampled Content charge 1,295.17 MiB and no full-size budget refusal observed. Cold wire remains unqualified. |
| ZA_SEARCH_THREAD_MEM_MB | 256 | Retain provisional. Approximately 3.68 MiB steady model/charge at 30,000, with 63.23 MiB highest sampled ledger. RSS is separate. |
| ZA_SEARCH_ACCOUNT_MAX_MB | 512 | Retain provisional. Normal sealed size 431.42 MiB, headroom 80.58 MiB. Small cap lifecycle passed. No physical 512 MiB transition observed. |
| ZA_SEARCH_IDLE_SECS | 900 | Effective. No isolated idle eviction timing. |
| ZA_SEARCH_PASS_CAP | 48 | Effective. Normal design needed 655 commits and H7 653. Normal design had 135 cap-sized GETs over 2 s, 33 with joint full/thread compaction and 102 without observed compaction. No alternate cap measured. |
| ZA_SEARCH_PASS_MB | 32 | Effective. No isolated byte-admission cap result. |
| ZA_SEARCH_PASS_HIDDEN_KB | 32 | Effective. No isolated hidden reservation cap result. |
| ZA_SEARCH_PASS_MS | 2000 | Effective planner interval. Normal noncompacting committed wire median/max 1.963963563/2.535389542 s, versus 155.732645 s for the longest full-compacting GET. Wire spans alone cannot identify planner stage timing. |
| ZA_SEARCH_WAIT_SECS | 30 | First search after 200 succeeded in 19.090 s. Full cold refused after 188.974 s. H7 succeeded at 2,065.774 s overall with 0.213 s on its final query. End-to-end spans do not isolate the configured wait. |
| ZA_SEARCH_SEGMENTS | 16 | Normal full compaction observed. Canonical H7 committed one full merge and max chain 31 within the derived bound of 46. The 255 threshold was isolated to the small cap fixture. |
| ZA_SEARCH_CHECKPOINT | 1000 | Both 2,000 cohorts, normal design and separate design rebuild each completed with one final noncommitting GET. |
| ZA_SEARCH_SKIP_AFTER | 8 | Effective. No isolated unreadable-item skip threshold measurement. |
| ZA_SEARCH_ABSENT_RETRY_SECS | 10 | Effective. Manifest-loss rebuild is distinct from referenced-blob absence retry. |
| ZA_SEARCH_PAD_MIN | 65536 | Effective. Observed oldest retained H7 Thread hidden class 16,777,216 B versus a 3,855,392 B Thread estimate. No minimum-padding sweep. |

The completed default fixture supports sealed-size headroom, full-size
resident growth with direct milestone warming, and successful full-size
serving after H7. Full-size cold and the linked warm-repeat wire query
were unqualified on this run. This unoptimized benchmark gives no isolated
stage timing or alternate-setting comparison to support a specific
production default change. Retain the approved 512 MiB account, 1,536 MiB Content and 256 MiB Thread
defaults provisionally. The optimized shipping binary was built
separately. Observed differences from the earlier run establish no
controlled causal or release-profile performance attribution.

### System command evidence

Raw and runner exits were 0. One test passed, 111 were filtered, Cargo
reported a 4m 08s build and libtest elapsed 10,201.56 s. Runner wall was
10,459.858 s, from 2026-11-10 02:50:19 to 05:44:39 UTC. The existing
Darwin linker warning identified the compact-unwind table limit, with
one Cargo warning summary and no additional warning or error headings.
Available space was 34,421,544 KiB at entry and 28,480,000 KiB on the
post-row snapshot. Runner postflight verified the same clean HEAD.

Checkout-local ignored evidence, absent in ordinary clones.
[Raw system log](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/bench-system/attempt-at0vcaty/command.log),
[frozen system report](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-release-system-report.md) and
[primary verification](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-system-primary.json).

- `argv.txt` `780cc04ccca7ba327895e22cee9406b6d786502f29d4e5be3d8ebc5b7d56e7d1`
- `attempt.json` `6f83d1f591fd2ce3143cb10587d942b4cc1f53b62a05380805b8ffd311e09883`
- `command.log` `6ad64011e2eac61430958db9a46f4fd7d36ba8919ccd9dcb7f7e1af17ec9c2c4`

## Final-head format benchmark

Measured 2026-11-10 UTC on `4dde13ade2be92411d76eb7906b14a0423359dc9`
with source-identical implementation `53062c367ac0b85596017fed6ebb50edd8a66567`.

`STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::search_bench::format -- --exact --ignored --nocapture`
selected the single ignored format benchmark in the unoptimized test
profile on Apple M1 Max with 10 cores, 64 GiB and macOS 27.2.1 build
26B131. The runner used stable Rust 1.99.0, the plan-local Cargo target
and fresh `/private/tmp/za-final-aqafngoc`. The runner cleared all fourteen
search overrides, `SPAM_RULES_URL`, `BLOB_STORE`, `ZA_KEY_ACCOUNTS`,
`NO_INSERT` and `SEARCH_BENCH_MESSAGES`. The unset message override
selected the default 15,000.

All 15,000 generated seed-1 messages were ingested in 207.425365 s and
all 15,000 were measured. The corpus contained 1,217,729,696 raw MIME bytes,
4,500 attachments (750 large), and 720 messages with more than 16
References, with a maximum of 40 References. The first attachment-bearing
record checked the attachment canary with extraction enabled and its
absence in the attachment-disabled control, and the body canary in both
positions-bearing encodings. The test asserted the delivered ID count,
the complete attachment count and the 304 B maximum Thread record bound.
[Corpus and assertions](../../../tests/src/za/search_bench.rs#L419-L549)
Thread record size was 88.160 B mean and 304 B max. There were 750 records
with more than 16 hashes (5.00%). Open-and-index throughput was 28.778
documents/s with attachment text and 59.343 without. These are format
and per-document measurements. The projected 30,000 row extrapolates
the 15,000 means and is distinct from the system's measured sealed part.
The positions-bearing per-record Term frames project to 449,622,688 B
at 30,000 messages. The raw term and compressed-frame projections
describe format payloads. System sealed size, model charges and RSS
were measured separately.

| Figure | With positions | Without positions |
| --- | ---: | ---: |
| Content term bytes, total / mean / p95 per message | 436,627,716 / 29,108.514 / 89,038 | 265,473,368 / 17,698.225 / 55,513 |
| Per-record zstd, no dictionary, total B | 233,861,859 | 87,763,848 |
| Per-record zstd, Dictionary::Term, total B | 224,811,344 | 79,755,932 |
| Per-record zstd, Dictionary::Email, total B | 235,790,919 | 90,249,593 |
| One frame over all records, total B | 208,245,428 | 58,957,049 |
| Projected raw term bytes at 30,000, mean × 30,000 | 873,255,432 | 530,946,736 |
| Decompression of every per-record Term frame, seconds | 3.136853 | 2.272307 |

Positions remain part of the format for phrase matching. The
without-positions and single-frame comparisons supply measurements.
The format mechanisms and provisional defaults remain as accepted.

Raw and runner exits were 0. One test passed, 111 were filtered,
Cargo displayed 3m 03s and libtest elapsed 1,171.82 s. Runner wall
was 1,366.234 s, from 2026-11-10 05:45:04 to 06:07:50 UTC. The
only warning was the same Darwin compact-unwind linker diagnostic.
Available space was 28,496,620 KiB at preflight and 26,240,000 KiB
at the post-row snapshot. Runner postflight verified the same clean HEAD.

Checkout-local ignored evidence, absent in ordinary clones.
[Raw format log](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/bench-format/attempt-mitqys85/command.log),
[attempt metadata](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/bench-format/attempt-mitqys85/attempt.json),
[completed report](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-release-bench-report.md) and
[primary verification](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-format-primary.json).

- `argv.txt` `0af44e2e0b6db8136c0fc21ffa9bd5d1a4bbe65d246ea633bffd977102f520ee`
- `attempt.json` `072c3c12979d500f273b17d6bbc488caaab293315453c8d337d1f257b716c18a`
- `command.log` `9da4cd445c006cfc36822ef0631cde0ce5c0afcdcbdddc9b90771258c4909177`

## Whole-branch fix validation

The accepted eight-file correction `53062c367ac0b85596017fed6ebb50edd8a66567`
matches the reviewed binary diff SHA-256
`34a3594aabb133a096a2631858ba081f22ce1b16449c98eba14b3912e1227d66`
against base `f4eb2ecccb073fea34fefa116743644732f9dedc`. The correction amends
plan 27 and spec 33 with their headers retained. The historical full system
run describes the base. The final-head system repeat is recorded separately. The tracked
[implementation review](2026-11-07-zero-access-16-search-implementation-review.md)
is recorded at `4dde13ade2be92411d76eb7906b14a0423359dc9`.

- **WB1.** An absent message within its H6 window aborts without advancing
  input or Content coverage. It preserves the matching attempt count or
  records zero for a new failure. Only a current non-absent transient
  increments the count and can skip at the separate limit. The canonical RED
  restored the removed synthetic blob, then failed at attempts 1 versus 0
  (exit 101, 876 s command, 768 s suite). Its later skip assertion was
  unreached. [Pass admission](../../../crates/email/src/message/za/search/pass.rs#L296-L324)
- **WB2.** Thread and Content have separate flights per account. The Map
  reload verifies the manifest and Thread. It can defer a charged Content
  suffix for an explicit Content request. Posting preparation and tombstone
  pruning run outside the shared writer. The final install checks raw hash,
  epoch and exact head. The second-wait correction checks epoch and exact
  Content head under the suffix-restoration writer before returning a fetch
  error. It retries a moved target and uses the raw hash for restoration.
  The moved-head unit exercises stale-fetch retry. Closure of the narrower
  read-to-writer wait gap is source-verified.
  [Map reload](../../../crates/common/src/za/search/load.rs#L1125-L1171),
  [error recheck](../../../crates/common/src/za/search/load.rs#L1002-L1043),
  [moved-head control](../../../crates/common/src/za/search/load_tests.rs#L1412)
- **WB3.** The original JMAP unit target reproduced the stale-interface
  compile error E0432 (exit 101, 223 s). The corrected test calls supply
  the visibility callback and retain the assertions.
  [JMAP fixture](../../../crates/jmap/src/email/za_search.rs#L367-L390)

| Validation | Observed |
|---|---|
| Focused behavioral RED | Common 49 passed and five expected failures, exit 101, 89 s command and 2.02 s tests. Email 16 passed and one expected in-window absence failure, exit 101, 36 s command. |
| Initial canonical GREEN | Intentionally interrupted with SIGINT before the second-wait correction. Cargo exited 101 for the signal. No completed suite result or reported assertion failure. |
| Final focused units | Common 59/59, Email 17/17 and JMAP 15/15, all exit 0. Command times 13/18/34 s. These code bytes are unchanged by the account isolation. |
| Canonical GREEN after the second-wait correction | Failed in WB2 fixture setup at SMTP 452 after WB1 passed (exit 101, 846 s command, 780.20 s suite). The evidence does not identify whether root or reply delivery filled the counter. WB2 Map and Content assertions were unreached. |
| Isolated canonical retry | RocksDB `za::za_tests` passed 1/1, exit 0, 1,794 s command and 1,628.27 s suite. Both new served cases and 183 paired comparisons across 3,258 messages passed, with the explicit K deviation. Fresh `searchfix-fixture` account, cleanup between cases and at teardown, byte-identical case bodies and unchanged production limits. The existing Darwin compact-unwind linker warning was emitted. |
| Primary shipping check | `cargo check -p stalwart --no-default-features --features rocks` passed, exit 0, 232.491 s, with no warnings or errors. |
| Independent scoped fix reviews | Both Astra reviewers passed requirements and correctness and reported zero new findings. |

The [primary acceptance record](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/whole-branch-fix-primary-integration.json)
binds the two review hashes and the shipping verification to the accepted
code. The fixture account and call sites are in
[index setup](../../../tests/src/za/mail/index.rs#L207-L212).

## Spec questions and plan calls

- **A1–A4.** Keep the format/system benchmark split. Use existing Term compression and upstream tokenization via the test accessor. Record text-only attachment coverage. The final-head system harness completed. Its post-H7 Content query served, and its full-size cold query refused.
- **A5–A7 and B1.** Use main's post-schema boot refusal. Task 2 owns format constants including derived R=304, a=15, reservation 529 B and K=16. Refuse segment limits outside 2–255. With H=32 KiB, 48 items are within the 61-reservation bound.
- **B2–B9.** Keep upstream Term/positions, exact allowance and typed refusal, corrupt phrase refusal, access-driven sweep, last-holder accounting, the test-only AssignedIds constructor, cycle-bounded alias resolution and upstream's 16-byte verbatim prefix rule. Thread allowance declines optional compaction within the final inventory bound; typed content-tier overflow refuses the proposed pass.
- **C1–C8.** Retry-window start is resident and the non-absent attempt count durable. Cursor work uses a separate pass from log work. Manifest hash is recomputed from commit-assigned bytes. Setup uses the existing vault batch extension. `scanned_to` proves complete records only. JMAP triggers once before qualifying own-account mail methods and coordinates the bounded post-pass. Restart controls evict state in-process. H6 governs absent-blob loss after the window. Only a current non-absent transient can reach the attempt bound and skip as Parse.
- **D1–D6.** Respect complete-record currentness, reuse the existing bitmap accessor and the one configuration override, exercise skipped swap and view pause in test mode, use a fixed StoreEvent route to JMAP serverUnavailable and IMAP Unavailable, and gate EmailSubmission/changes with the table.
- **D7–D13.** Key-touching copy refuses before prefix lookup. Task 13 closed the interim raw changes output. Use the maximum of both retained markers. Keep adapter additions with their producing tasks. D11 measured retained-state Email/changes and Thread/changes end to end at 36.090 ms and 14.793 ms on f4eb2ecc. The final-head repeat measured 36.407 ms and 14.767 ms. Neither run isolated the scan cost. Task 12 tightens the interim UID comparison. Thread state comes from the admitted cache's Email-group counter (Jay's D13 decision).
- **E1–E10.** Keep the retained revision floor row (Jay's E1 decision), use marker/unlink/clear for reindex, use explicit cursor endpoints and progress through gap inserts, decline over-allowance optional compaction without enlarging pads, suspend chain extension and merge above the cap, run permanent destroy for every account under the amended exact fences, reuse test configuration/restart controls, and backfill rather than replace a healthy chain after cap raise or lag. Jay requires one shared account cap per cluster.
- **F1–F6.** IMAP search/thread variants removed; keyed FETCH retained, STORE delegated. Keyed synthetic Move aliases fixture IDs. Stem-sensitive rows run in key mode. Integration helpers stay in producing tasks. Separate leak_search oracle entry follows test_mail. The corpus is mailbox.gz plus eighteen generated records with ordered work, paired timestamps, exactly three headerless transport exceptions and one pinned K deviation.
- **F7–F10.** Networked point-read figure depends on a local PostgreSQL run and is pending. Jay decides any final default changes. Class fixtures check the class; folding/drop evidence belongs to real Task 15 controls and M50–M52. CI topology stays as prescribed. The local CI step-table comparison had zero differences, and the Python gate passed 41 of 43 selected tests with two skipped.

## Rulings

- PL23 shares the original Content deadline through loader retries and frees staging and charges. PL24 compares cold-load reads with the pre-request inventory, retaining progress and zero Content reads. Preflight kept producing-module ownership, omitted the tautological class assertion, applied exit-status-preserving checks, exercised the real compaction procedure, and bounded JMAP passes request-wide. Task 1 was a reporting checkpoint with the 304 B hard record bound. Independent reviews and primary acceptance replace implementer self-review. The primary owns staging and commits. Push, PR, merge and deployment require separate authority.
- Retain Task 1's baseline unused store parameter and Darwin linker diagnostics as provenance. The old per-task literal warning gate was superseded by explicit baseline comparison plus no changed-file warnings. Task 19 benchmark code includes the narrow unused-parameter tidy and passed a warning-free shipping check. Step 5 carries the accepted fix's source-identical, warning-free shipping check. The separate shipping release build also passed without warnings.
- Resume an empty Destroying inventory with the exact Principal/doc-zero scan, since the encoding cannot distinguish true empty from fallback. Topology/parse/absence invokes the scan; I/O/key errors propagate. Every true reset mutation asserts live Account and the captured marker where applicable. The earlier unconditional absent-field marker rule is superseded: with raw vault present assert its hash; with both fields absent leave no marker and assert both absences for any unlink. Preserve the separately scoped stale setup-token issuance residual.
- Task 7 standalone email units explicitly enable the dependency test feature, without a Cargo manifest change. Task 8 retains the preceding complete checkpoint before any insert of a fresh record, checks a conservative hold expiry after the pre-commit pause, preserves both tombstones and backfill in mixed stages, and resolves the Email change ID by account for Principal manifest writes. The hold check covers known expiry at batch entry. Task 16a ran directly after Task 8 and removed its temporary fixture-only unlinking.
- Task 10 ownership includes the established JMAP/IMAP evaluator and fixtures. Task 9 takes the raw installed entry and tries the pass lock before loading; its Running waiter spends the original deadline. Keyed IMAP preserves individual triggers; ordinary command grouping stays in place. Task 10 enters via ensure_current before a blocking loaded read; later Content acquisition waits share that deadline. Admitted store writes retain their existing ownership. Task 17's converted-socket Mailbox/set control subscribes to Mailbox state; the flag stimulus subscribes to Email.
- Keyed queryChanges keeps current readable rows and nonidentity sort values, with pinned historical presence/identity before dependent query operations. This is the primary's implementation interpretation. Exact historical values were left for separate design. Recovery uses one ordinary empty Email log record when both witness sets are empty, consuming one retention slot, and requires loss proved for the asserted base. A readable old manifest continues its revision; unreadable or unsupported older format starts at one. Newer unsupported format follows the no-map boundary.
- Full compaction tier saturates at 255; checked tiered overflow atomically refuses the proposed pass. Thread allowance and the thread inventory count are separate. Task 15 uses charged bounded preflight and actual selected inputs, then may perform nonblocking exact-base content eviction. Real allowance controls distinguish 254→255, refusal at 255, and recovery to one. Corrected reservation covers cold decode/load transitions as well as merges. The separate warm scratch/admission finding was mandatory before benchmarking and closed by the later warm admission commit.
- Task 11 corrects stale pre-provisioning identity paths under selected-account key classification. Task 12 preserves only the needed retained namespace/history in cleanup. Task 13 whitelists two fixed keyed changes error reasons and records its failed keyed gate for Task 17. Task 14 corrects both the R16/R20 fixture partition and exact reconciliation progress oracle without changing the production algorithm.
- Record actual sealed content bytes uploaded as trace Size and cumulative encoded merge output work as Used, including intermediate encoded outputs. Compare G17/H7/R32 rewrite work against original logical delta input volume. Sampled memory and I/O counters remain distinct from exact write accounting.
- Task 16b follows adaptive outer recoding and checks rebuild trace `Source=scan` separately from loss Reason. Task 17 qualifies keyed import delivery across a legitimate split notification window and retains strict durable-state and missing/duplicate checks. Bulk pre-drain is for functional setup only.
- Task 18 temporarily raises only the two identified delivery throttle counts with exact restoration, controls Delivered-To only inside membership, preserves all corpus records with the three transport exceptions, and serializes paired fixture work. The scanner and differential gates are independent. The longest first-part word preserves the selected MIME part. Paired arrival controls comparator inputs and reads them back for every ordinal. The earlier all-LMTP and input-byte-only parity assumptions are superseded by plan 26/spec 33 candidate wording.

- Task 19 takes independent review and an intermediate benchmark commit before
  committed-head mutations, with verified restoration. The entry-refresh ruling
  permits a narrow test-only Committed observer under independent review and a
  controlled validation. Fresh mutation execution follows the default-assessment
  checkpoint.
- After diagnostic1, extend only the benchmark wire ceiling to 600 s and print
  the watchdog policy at startup. Spec §§5.1 and 6.2 distinguish admitting
  another item from completing owned work. Keep direct milestone load at 60 s
  and phase and whole-run limits at 4 h and 12 h. Preserve both failed runs.
- H6 controls the C1/C8 conflict exposed by WB1: an unexpired absence
  classification cannot become permanent loss through attempt exhaustion.
  The persistent non-absent fault bound stays. The accepted plan and spec
  amendments reflect that division.
- G6 is single-flight per account and independently loaded part. WB2 keeps
  required Thread refresh independent of optional Content I/O. The approved
  second-wait correction shrinks the charge outside the writer and checks
  the exact target after acquiring the writer. It preserves the raw-hash
  restoration guard, off-lock disposal and the caller's deadline.
- After the canonical SMTP 452 setup failure, isolate both new real-delivery
  cases on one fresh key account. Cleanup between cases and at final teardown
  preserves the case bodies and avoids adding their traffic to keymail46's
  existing bucket. The production and suite-wide rate settings stay in place.
- Bring both independent whole-branch source reviews forward onto the frozen
  implementation while the benchmark runs. Later source changes require scoped
  review and relevant revalidation; runtime qualification remains required.
- Correct the M30 test opener before the next measured head. The input already
  declares 39 documents. The correction supplies all 39 and leaves the
  correct-code assertions and product defaults intact.
- Carry workspace formatting, the shipping check, JMAP ZA units and
  canonical RocksDB ZA across the review-only commit after verifying
  identical code, tests, Cargo manifests, scripts and resources. Phase A
  covers compilation and units. Phase B follows with compatibility and
  local gates. Phase C runs the shipping release build alone, then the
  full system and format repeats serially. Stop on an unexpected failure
  for primary adjudication. The named SMTP rule permitted one isolated
  retry of the sole matching failure before Phase B resumed.
- Between Phases A and B the primary reclaimed only the ignored
  incremental compiler cache at `W/target/debug/incremental` while runtime
  was idle. The record shows 23,437,948 KiB removed and 31,491,944,448 B
  free afterward. Logs, fixtures, binaries and dependencies were retained.
  [Cache cleanup record](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-between-phases-cache-cleanup.json)

## Mutation runs (plan 16)

### Step 2 status after three campaigns (2026-11-11)

Three independent campaigns account for the plan's Step 2 table at their
recorded source heads. The final two used `bae62db6f`; the first used `bcc81d177`
before the T13 test-only follow-up. Every row except one variant has a behavioural run.

| Campaign | Rows | Report |
|---|---|---|
| 19 regressions (2026-11-10) | M18, M26, M35, M55, M57, M58, M59, M68–M74, plus three non-plan targets (T10 floor and cap, T13) | [Remaining checks](2026-11-07-zero-access-16-search-remaining-checks-review.md) |
| 55 drafts (2026-11-10/11) | 49 plan rows across 54 labels, plus the Task 3 map-order companion (55 labels total) | [55 drafts](2026-11-07-zero-access-16-search-mutations55-review.md) |
| Remaining rows and follow-ups (2026-11-11) | the 16 rows neither earlier campaign ran, the masked 55-draft follow-ups and an M16 probe | [Remaining rows](2026-11-07-zero-access-16-search-mutations-final-review.md) |

Executed faults used focused selections within `za::za_tests`, or a unit test by
name, with passing correct-code baselines and restored controls. Failed baseline
attempts remain recorded separately. The final campaign's follow-ups removed whole
dispatch calls without changing assertions. The first campaign's M26 follow-up
temporarily relaxed two masking assertions to reach the later catcher, as recorded
in its report. No fault was committed.

| Outcome | Rows |
|---|---|
| Survives every test | M16 (reachable only through an untested race), M23, M48 |
| Named test doesn't demonstrate it, but other tests catch it | M14 (M50's units, identical patch), M19 (two earlier tests), M22 (two other tests), M31 (unit and R22), M43a (unit and two wire tests), M44 (units), M49 (unit and G12) |
| Not executed | M29b (needs a hidden-section codec change) |
| Demonstrated at a named test | every other row and label |

| Row | Result |
|---|---|
| M1 | Demonstrated (unit). The R11 reload can't see a symmetric AAD change |
| M2, M3 | Demonstrated |
| M4 | Demonstrated at the backlog case, in a focused round |
| M5 | Demonstrated, both variants (R2 cross-node) |
| M6 | Demonstrated (unit) |
| M7 | Demonstrated (unit), and at the backlog case in a focused round |
| M8 | Demonstrated at R12 paging, in a focused round |
| M9 | Demonstrated (unit); the front-door control passed |
| M10–M12 | Demonstrated |
| M13 | Demonstrated (membership) |
| M14 | R3 passes; the identical patch is caught by M50's units |
| M15 | Demonstrated at R3, in a focused round |
| M16 | **Survives**. The probe shows no fixture reaches its state; a race can |
| M17, M18, M20 | Demonstrated |
| M19 | Caught by `case_lost_assertion` and `case_expired_hold_does_not_publish`; R7 not reached |
| M21 | Demonstrated (unit), and at R14 with the masking tests removed |
| M22 | Caught by the retention and thread-page cases; named F10 passes |
| M23 | **Survives**: no test puts an insert and a delete in one record |
| M24, M25 | Demonstrated (R16/R20, R17) |
| M26 | Demonstrated |
| M27 | Demonstrated at F3, with the masking test removed |
| M28 | Demonstrated: a at the F4 block (focused round), b at R22 |
| M29 | a: demonstrated (unit); wire F1 unobserved behind a debug assertion. **b: not executed.** c: demonstrated |
| M30 | Demonstrated (unit); R21 doesn't discriminate |
| M31 | Caught by its unit and R22's held-set check; R16/R20 not reached |
| M32 | Demonstrated |
| M33 | Demonstrated (unit), and at R26 with PL4 removed |
| M34–M39 | Demonstrated, including both variants of M34 and M38 |
| M40 | Demonstrated (unit H9); R3 injects no bad tag |
| M41, M42 | Demonstrated, including both variants of M42 |
| M43 | a: demonstrated (unit) and caught by two wire tests; named case not reached. b: demonstrated |
| M44 | Demonstrated (two units); R24/G2 passes when reached |
| M45, M46, M47 | Demonstrated |
| M48 | **Survives**: no test re-presents an already-held insert |
| M49 | Demonstrated (unit; G12); R7 can't discriminate |
| M50–M63 | Demonstrated |
| M64 | Demonstrated; the injected-error branch is not reached |
| M65–M79 | Demonstrated. M66's second branch passes, as expected |

Open decisions for Jay: M14, M16, M23, M44, M48, M22's F10, and M29b. Task 19 remains
incomplete for the reasons under [Where things stand](#where-things-stand).

### Correct-code regression follow-up (2026-11-11)

The [M16/M23/M48 follow-up](2026-11-11-zero-access-search-regression-followup.md)
adds a real-pass pending-cursor regression, strengthens the mixed-record planner
ordering assertion, and directly tests live and retained-deleted held inserts.
Independent Astra review approved the tests. All 18 planner tests and the full
canonical RocksDB ZA suite passed; the latter includes M16 in normal recovery
order and complete differential checks and cleanup. Production behavior is unchanged.

These are correct-code results. No deliberate faulty variant ran in this follow-up,
so the historical survivor rows above are not changed to mutation kills. The M16
fixture directly establishes the resident state rather than scheduling the full
cross-node race. M23 covers the planner contract; a single `Email/set` create and
destroy commits separately and does not establish a mixed log record. M29b's
exclusion and the other campaign findings remain as recorded.

### First campaign: 19 regressions (2026-11-10)

**At that point the fresh Step 2 campaign was partly executed.** At the earlier local cutoff,
automatic screening had stopped preparation of 19 checks, and a separate
forward/reverse verification request for 55 rebased drafts had stopped before
dispatch. Jay subsequently commissioned an independent validation task on
`bcc81d177`, whose production and test source matched `53062c367`. Its
[remaining-checks report](2026-11-07-zero-access-16-search-remaining-checks-review.md)
records focused, baselined fault runs for the 19 targets, exact restoration,
and preparation-only verification of all 55 drafts. The earlier stopped
attempts did not execute; the later evidence belongs to the independent task.

Eighteen targets detected their fault at the named catcher. T13 Caller floor
was partly demonstrated: standalone changes and Map queryChanges caught the
missing minimum, but the Content route did not. Its existing zero-second wait
refused before exercising the caller floor. The separate test-only proposal
uses a real flag update and a held cache to isolate that floor from the
publication and Content-checkpoint floors. Its correct-code focused baseline
passed; removing the Content caller argument then failed at the expected
error-class assertion. It observed `CannotCalculateChanges` instead of the
required Content-unavailable error, not a successful stale response.

The proposal has been integrated byte-for-byte in
`tests/src/za/mail/threads/task13/minimum.rs` after independent Astra review.
It changes no production code. The full ZA suite passed with both
default and FileSystem blob storage on this integrated test. Those correct-code
passes qualify integration; they are separate from the earlier focused fault
run. Task 19 and the complete Step 2 campaign remain incomplete.

| Fresh preparation category | Status |
|---|---|
| 55 offline drafts | Independent preparation verification passed for all 55: source binding, forward application and exact reversal. These drafts were not behaviorally executed, and nonempty selection of their catchers remains unverified. |
| Two race targets | M18 and M26 detected their faults in focused, baselined runs. M26's original masking assertion and the narrower follow-up are both preserved. M18's later after-failed-CAS assertion was not reached. |
| Eight paging/view targets | M35, M55, M57, M58, M59, T10-content-floor and T10-final-cap detected their faults. T13's standalone/Map routes detected theirs; the existing Content route survived. The added Content fixture passed its focused baseline and caught that fault; full-suite integration qualification passed with both default and FileSystem blob storage. |
| Nine epoch/recovery targets | M68–M73 and M74a/b/c detected their faults in focused, baselined runs; each production edit was restored exactly. |
| M30 opener | Correct-code unit passed with 39 opened documents. The wrong-reservation outcome remains unexecuted. R21 covers fixed padding, with distinct reservation-only coverage pending. |
| Remaining preparation and restored checks | Pending release and execution. |

The later campaigns above superseded the 55-draft and remaining-preparation rows of this table.

The report preserves the exact selections, commands, first failing assertions,
restoration hashes and deviations. These were focused module selections within
`za::za_tests`, not fault runs under the full suite or FileSystem blob variant.
M35's secondary admission-equality fixture, M18's after-failed-CAS assertion and
M58's recovery variants after Thread remain unobserved in this campaign. The
55 drafts and the other Step 2 rows were not executed by this follow-up.

### T13 Content caller-floor integration, 2026-11-10

The integrated test file has SHA-256
`125438ccea0d0e3223b38c1dd594d4b7244f2c4d55749368dbea472a7d220c94`.
It is the exact reviewed 88-line addition to `bcc81d177`, not a test present in
that original commit. Production sources and the full-suite dispatcher were
unchanged. Independent Astra review accepted the proposal and the evidence
calibration; the primary verified the applied bytes and both raw full-suite
results. The external report's unapplied/focused-only cutoff remains preserved.

Both runs used stable Rust 1.99.0, the normal unoptimized test profile,
`STORE=RocksDb RUST_MIN_STACK=16777216`, a private temporary directory, and no
`ZA_SEARCH_*` environment overrides. The second set `BLOB_STORE=FileSystem`;
the first left it unset. They ran serially, once each, using:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo test --locked --offline -p tests za::za_tests -- --exact --nocapture
```

| Blob storage | Selection and result | Cargo exit | Test / command wall time | Evidence |
|---|---|---|---|---|
| Default RocksDB | 1 passed, 0 failed, 0 ignored, 111 filtered | 0 | 1640.65 s / 1873.980 s | [Default log](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-t13c-integration-7sev4in8/default/cargo-test.log) |
| FileSystem, with RocksDB data store | 1 passed, 0 failed, 0 ignored, 111 filtered | 0 | 1739.49 s / 1741.346 s | [FileSystem log](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-t13c-integration-7sev4in8/filesystem/cargo-test.log) |

The new case's banner appeared once in each full run. Both reached the paired
corpus comparisons and final cleanup, with no panic or watchdog expiry.
The tested source hashes and diff were unchanged before and after each run.
Formatting and whitespace checks passed. Both Cargo outputs retained the known
Apple compact-unwind linker warning and its summary, with no new source warning.
A supplementary listener-preflight tool warned about an unrelated mounted volume;
the successful kernel snapshots showed all required ports vacant. No test had
started during those preflight diagnostics, so they are not runtime retries.

Default log SHA-256:
`0566591f8539d5832679e52f9406e940f61bd328574d9b76dbb8993fd0792307`.
FileSystem log SHA-256:
`37a1a80758d0b877a16d03a138aa41fff850e4d03a85ab234f60f303b9c0a3b2`.
Commands, environment, source identities, receipts and the retained proposal are
in the [validation receipt](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-t13c-integration-7sev4in8/final.txt)
and [evidence inventory](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-t13c-integration-7sev4in8/evidence.sha256).
These runs close integration of this coverage fix, not the remaining mutation,
cold-serving or external qualification gaps.

### Historical task kill checks

The earlier task-scoped results are separate from the fresh campaign.

| Task | Recorded control and observed failure |
|---|---|
| 10 | One byte over cap served instead of refusing. Removed floor served stale matches. Unbounded entry acquisition reached the outer timeout with held locks. |
| 13 | Standalone and query Map-caller minima returned the wrong failure class. Reacquiring a Map timed out. Current collapse grouping retained the wrong survivors. M35/M56/M57/M59 rejected a valid continuation or served the wrong historical candidates. |
| 14 | M68 left manifest absent; M69 left durable Email key behind epoch; M70 encoded `[]` for the empty boundary; M71–M74 altered healthy successors after stale diagnoses, including older/tag/parse subcases. |
| 15 | One-at-a-time folding stranded a member at the intermediate thread. Trimming retirement reservation early gave 884 bytes versus required 156,532 at the 256-node commit pause. |
| 16a | Removing individual live/marker/absence guards exercised separately isolated reset/permanent-destroy paths. M64–M67 caught recreated markers, missing raw-vault authority and unlinks after one captured absence changed. |
| 16b | Missing marker assertion removed replacement links. Reset inside Email loop missed zero-row account. Discarded scan results left old links. Missing Principal selector failed the codec key checks. |
| 17 | Dispatcher omission let a fresh keyless WebSocket serve Email/query instead of refusing. |

## Final matrix evidence

These results are for `4dde13ade2be92411d76eb7906b14a0423359dc9`, with
source-identical implementation `53062c367ac0b85596017fed6ebb50edd8a66567`.
The local host was Apple M1 Max with 10 cores, 64 GiB and macOS 27.2.1
build 26B131. The runner used stable Rust/Cargo 1.99, the Homebrew rustup
path, `RUSTUP_TOOLCHAIN=stable`, `CARGO_TERM_COLOR=never`, and the plan-local
`W/target` as `CARGO_TARGET_DIR`. Exact PATH, TMPDIR and the set/unset
environment are in each attempt's adjacent `attempt.json`. Each command
ran in the release7-search execution checkout.

| Environment group | Rows | Row-specific effective environment |
|---|---|---|
| Build, unit and Python | All Phase A, `gate-ci-steps`, `gate-tests` | Store and test stack overrides unset. |
| RocksDB/FileSystem blobs | `za-fs-blob` | `STORE=RocksDb RUST_MIN_STACK=16777216 BLOB_STORE=FileSystem` and fresh TMPDIR. |
| RocksDB/plain or upstream | `za-cache-swap`, plain protocol rows, all `upstream-*` rows in Phase B | `STORE=RocksDb RUST_MIN_STACK=16777216` and fresh TMPDIR. |
| RocksDB/keyed | Key WebDAV, IMAP and JMAP rows | `STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1` and fresh TMPDIR. |
| RocksDB/SMTP | `smtp-full` and its exact retry | `STORE=RocksDb RUST_MIN_STACK=16777216`, fresh TMPDIR and `SPAM_RULES_URL=https://github.com/stalwartlabs/spam-filter/releases/download/v3.0.4/spam-filter-rules.json.gz`. |

`BLOB_STORE`, `ZA_KEY_ACCOUNTS` and `SPAM_RULES_URL` are unset outside the
listed overrides. `NO_INSERT`, `SEARCH_BENCH_MESSAGES` and all fourteen
`ZA_SEARCH_*` overrides are unset throughout.

### Four source-identical carried rows

The review-record commit changed only documentation. The release record
verified equal code, tests, Cargo manifests, scripts and resources. These
earlier command exits are direct checks, with no matrix runner exit.

| Row | Command | Result | Evidence |
|---|---|---|---|
| fmt-workspace | `cargo fmt --all --check` | Exit 0, 5 s | [Status](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/whole-branch-fix-green-fixture-fmt.status) |
| check-shipping | `cargo check -p stalwart --no-default-features --features rocks` | Exit 0, 232.491 s, warning-free | [Shipping record](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/whole-branch-fix-primary-shipping.json) |
| unit-jmap | `cargo test -p jmap --lib --features test_mode,enterprise za` | Exit 0, 15 passed, 34 s wall | [Status](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/whole-branch-fix-green-final-jmap.status) |
| za-rocks | `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --exact --nocapture` | Exit 0, 1/1, 1,794 s wall, 1,628.27 s suite | [Status](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/whole-branch-fix-green-fixture-integration.status) |

### Phase A completed

All 14 rows passed on their first attempt, totaling 318 test passes.
The test-target warning gate passed. The sole warning class was the
known Darwin compact-unwind linker warning on the archive test's
integration binary. The complete logs contained no source-file warnings.
[Phase A report](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-phase-a-report.md) and
[primary verification](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-phase-a-primary.json).

| Row | Literal command | Selection and result | Raw / runner exit | Wall / test time | Evidence |
|---|---|---|---|---|---|
| fmt-http | `cargo fmt --manifest-path crates/http/Cargo.toml -- --check` | n/a | 0 / 0 | 0.25 s / n/a | [attempt-lz2wgaa1](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/fmt-http/attempt-lz2wgaa1/command.log) |
| check-tests | `cargo check -p tests --tests` | n/a | 0 / 0 | 247.573 s / n/a | [attempt-qvirsned](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/check-tests/attempt-qvirsned/command.log) |
| unit-vault | `cargo test -p vault --features test_mode` | 47 selected, 47 passed, 0 failed, 0 ignored, 0 filtered; 0 selected, 0 passed, 0 failed, 0 ignored, 0 filtered | 0 / 0 | 22.264 s / 0.02 s + 0.00 s | [attempt-xuiwj8jj](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/unit-vault/attempt-xuiwj8jj/command.log) |
| unit-store-search | `cargo test -p store --lib --features test_mode za_search` | 75 selected, 75 passed, 0 failed, 0 ignored, 25 filtered | 0 / 0 | 150.454 s / 1.32 s | [attempt-hmnkunca](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/unit-store-search/attempt-hmnkunca/command.log) |
| unit-store-hooks | `cargo test -p store --lib --features test_mode za_test` | 20 selected, 20 passed, 0 failed, 0 ignored, 80 filtered | 0 / 0 | 0.4 s / 0.05 s | [attempt-o2q7ocpt](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/unit-store-hooks/attempt-o2q7ocpt/command.log) |
| unit-common | `cargo test -p common --lib --features test_mode za::` | 75 selected, 75 passed, 0 failed, 0 ignored, 117 filtered | 0 / 0 | 94.804 s / 1.06 s | [attempt-gl2ipz3j](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/unit-common/attempt-gl2ipz3j/command.log) |
| unit-email | `cargo test -p email --lib --features test_mode,enterprise,common/test_mode za::` | 67 selected, 67 passed, 0 failed, 0 ignored, 4 filtered | 0 / 0 | 72.139 s / 0.03 s | [attempt-zojjbku3](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/unit-email/attempt-zojjbku3/command.log) |
| unit-email-paired-arrival | `cargo test -p email --lib --features test_mode,enterprise,common/test_mode message::delivery::test_received_at::tests::paired_received_at_scope_and_restoration -- --exact` | 1 selected, 1 passed, 0 failed, 0 ignored, 70 filtered | 0 / 0 | 0.428 s / 0.00 s | [attempt-hc62y3qj](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/unit-email-paired-arrival/attempt-hc62y3qj/command.log) |
| unit-imap | `cargo test -p imap --lib za` | 3 selected, 3 passed, 0 failed, 0 ignored, 8 filtered | 0 / 0 | 60.866 s / 0.00 s | [attempt-dncigb6m](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/unit-imap/attempt-dncigb6m/command.log) |
| unit-services | `cargo test -p services --lib --features test_mode,enterprise za` | 6 selected, 6 passed, 0 failed, 0 ignored, 3 filtered | 0 / 0 | 52.556 s / 0.01 s | [attempt-0f3nc0ar](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/unit-services/attempt-0f3nc0ar/command.log) |
| unit-jmap-proto | `cargo test -p jmap_proto --lib error::method::tests::only_fixed_change_history_reasons_reach_the_client -- --exact` | 1 selected, 1 passed, 0 failed, 0 ignored, 16 filtered | 0 / 0 | 43.475 s / 0.00 s | [attempt-ttj20anf](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/unit-jmap-proto/attempt-ttj20anf/command.log) |
| unit-http | `cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise` | 6 selected, 6 passed, 0 failed, 0 ignored, 0 filtered | 0 / 0 | 172.349 s / 0.00 s | [attempt-9c_i86pt](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/unit-http/attempt-9c_i86pt/command.log) |
| unit-smtp | `cargo test -p smtp --lib --bins --features test_mode` | 16 selected, 16 passed, 0 failed, 0 ignored, 0 filtered | 0 / 0 | 63.438 s / 0.00 s | [attempt-j4p34ai9](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/unit-smtp/attempt-j4p34ai9/command.log) |
| unit-archive-short-values | `cargo test -p tests za::leak::tests::try_archive_short_values_are_not_archives -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 206.95 s / 0.00 s | [attempt-rki58tit](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/unit-archive-short-values/attempt-rki58tit/command.log) |

### Phase B completed

All 18 main rows ran. Seventeen passed on their first attempt. The sole
initial SMTP failure was `smtp::inbound::dkim2::dkim2_dsn_is_signed`.
The wait timed out at [the queue-event helper](../../../tests/src/smtp/inbound/mod.rs#L73)
at column 27 with `No matching queue event received, pending events: []`.
There is no backtrace to identify the exact calling wait. The cause
remains unconfirmed. The same-source exact retry passed once in a fresh
TMPDIR and satisfied the named conditional rule. The original full-run
failure remains recorded. The retry establishes that the selected test
passed in isolation. All nine later Phase B rows passed on their first
attempt. Integration binaries replayed only the known Darwin linker
warning, and the Python scripts emitted no warnings.
[Phase B report](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-phase-b-report.md),
[primary verification](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-phase-b-primary.json) and
[retry release](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-smtp-retry-release.json).

| Row | Literal command | Selection and result | Raw / runner exit | Wall / test time | Evidence |
|---|---|---|---|---|---|
| za-fs-blob | `cargo test -p tests za::za_tests -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 1678.307 s / 1676.85 s | [attempt-i5kmsgh1](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/za-fs-blob/attempt-i5kmsgh1/command.log) |
| za-cache-swap | `cargo test -p tests za::cache_swap::cache_swap_tests -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 19.961 s / 18.73 s | [attempt-1abqrxa3](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/za-cache-swap/attempt-1abqrxa3/command.log) |
| webdav-plain | `cargo test -p tests webdav::webdav_tests -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 21.115 s / 20.56 s | [attempt-gp4nqv78](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/webdav-plain/attempt-gp4nqv78/command.log) |
| webdav-key | `cargo test -p tests webdav::webdav_tests -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 23.986 s / 23.43 s | [attempt-irmdir48](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/webdav-key/attempt-irmdir48/command.log) |
| imap-plain | `cargo test -p tests imap::imap_tests -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 12.083 s / 11.51 s | [attempt-i2xsmrt2](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/imap-plain/attempt-i2xsmrt2/command.log) |
| imap-key | `cargo test -p tests imap::imap_tests -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 14.214 s / 13.66 s | [attempt-a6qbi3tb](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/imap-key/attempt-a6qbi3tb/command.log) |
| jmap-plain | `cargo test -p tests jmap::jmap_tests -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 129.821 s / 129.25 s | [attempt-ibj9vj1q](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/jmap-plain/attempt-ibj9vj1q/command.log) |
| jmap-key | `cargo test -p tests jmap::jmap_tests -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 166.808 s / 166.25 s | [attempt-6wo1w0mk](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/jmap-key/attempt-6wo1w0mk/command.log) |
| smtp-full | `cargo test -p tests smtp -- --nocapture` | 79 selected, 75 passed, 1 failed, 3 ignored, 33 filtered | 101 / 101 | 50.947 s / 50.37 s | [attempt-59xxqm_q](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/smtp-full/attempt-59xxqm_q/command.log) |
| smtp-rerun-dkim2-dsn | `cargo test -p tests smtp::inbound::dkim2::dkim2_dsn_is_signed -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 2.924 s / 2.34 s | [attempt-b83z1_cq](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/smtp-rerun-dkim2-dsn/attempt-b83z1_cq/command.log) |
| upstream-caldav-query | `cargo test -p tests webdav::cal_query:: -- --nocapture` | 3 selected, 2 passed, 0 failed, 1 ignored, 109 filtered | 0 / 0 | 0.602 s / 0.01 s | [attempt-0k2o8b7v](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/upstream-caldav-query/attempt-0k2o8b7v/command.log) |
| upstream-store | `cargo test -p tests store::store_tests -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 8.841 s / 8.27 s | [attempt-kpizezfy](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/upstream-store/attempt-kpizezfy/command.log) |
| upstream-blob | `cargo test -p tests store::blob::blob_tests -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 8.441 s / 7.87 s | [attempt-4q8nd8ae](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/upstream-blob/attempt-4q8nd8ae/command.log) |
| upstream-lookup | `cargo test -p tests store::lookup::lookup_tests -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 13.444 s / 12.88 s | [attempt-vpo2wen9](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/upstream-lookup/attempt-vpo2wen9/command.log) |
| upstream-search | `cargo test -p tests store::search_tests -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 249.141 s / 248.56 s | [attempt-9cyih8pn](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/upstream-search/attempt-9cyih8pn/command.log) |
| upstream-telemetry | `cargo test -p tests telemetry::telemetry_tests -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 4.034 s / 3.46 s | [attempt-ltm92p72](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/upstream-telemetry/attempt-ltm92p72/command.log) |
| upstream-system | `cargo test -p tests system::system_tests -- --exact --nocapture` | 1 selected, 1 passed, 0 failed, 0 ignored, 111 filtered | 0 / 0 | 66.335 s / 65.76 s | [attempt-8dn204s3](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/upstream-system/attempt-8dn204s3/command.log) |
| gate-ci-steps | `python3 -I scripts/tests/ci_steps.py` | Workflow comparison passed, 0 differences | 0 / 0 | 0.165 s / n/a | [attempt-_vi9qkow](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/gate-ci-steps/attempt-_vi9qkow/command.log) |
| gate-tests | `python3 -I scripts/tests/test_gates.py` | 43 selected, 41 passed, 0 failed, 2 skipped | 0 / 0 | 7.802 s / 7.533 s (unittest) | [attempt-icw07gxe](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/gate-tests/attempt-icw07gxe/command.log) |

### Phase C completed release and benchmark rows

The shipping release build ran alone on `4dde13ade2be92411d76eb7906b14a0423359dc9`
with implementation `53062c367ac0b85596017fed6ebb50edd8a66567`. It used
`cargo build --release -p stalwart --no-default-features --features rocks`.
Raw and runner exits were 0/0, with 1,207.655 s wall time and no warnings.
The binary was 85,405,200 bytes with SHA-256
`0e00efd5f27f557d5725f7ee8b94f55de12ad1adab1735b3b1f3a8d418f374a4`.
The [release build log](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-matrix-prep/results/4dde13ade2be92411d76eb7906b14a0423359dc9/release/attempt-0c9is9fw/command.log)
has SHA-256 `800eda1418a67f421eb9d714b8d931253b5591d3ba34df3184d1978116567202`.
[Primary release verification](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/task19-final-release-primary.json).

The [final-head system repeat](#final-head-system-benchmark) completed
with raw and runner exits 0/0, one test passed, 10,201.56 s test time
and 10,459.858 s wall time. The post-H7 query served. Cold wire serving
and its linked warm repeat remain unqualified. The
[final-head format repeat](#final-head-format-benchmark) completed with
raw and runner exits 0/0, one test passed, 1,171.82 s test time and
1,366.234 s wall time. Docker automation and directory remain for CI.
Native clients, the separate fresh-store gate, networked-store timing
and physical multi-node behavior remain pending.

## Other harnesses

Earlier task runs cover keyed and plain protocol paths and canonical ZA
backend paths at their recorded heads. Task 17's final scoped JMAP, IMAP
and RocksDB results and Task 18's final canonical RocksDB pass are
functional results. Step 5 carries the accepted fix's canonical RocksDB ZA
pass. Phase B completed the FileSystem blob and isolated cache suites,
both modes of WebDAV, IMAP and JMAP, the selected local upstream suites
and the CI script checks. The full SMTP attempt and exact rerun are
recorded separately in the matrix evidence. The Phase C shipping release
build passed without warnings and both benchmark repeats completed.
Native clients, the separate fresh-store gate, networked PostgreSQL and
physical multi-node evidence remain pending. The Docker-backed automation and directory rows were
left to CI because the Docker CLI was absent.

The five-message RocksDB observer passed after the benchmark's initial
2.59 s failure on the empty-manifest assumption. The corrected runtime
control took 9.48 s and covered normal/full and cursor/tiered recovery,
exact cold coverage, controlled budget refusal/restoration and committed
accounting. The independently compiled legacy generator's seed-1 digest
matched all 1,002 MIME/metadata records. The comparison passed in 6.13 s
after the legacy control in 4.33 s. The benchmark-code shipping check
passed warning-free in 40.53 s. The sampler label now says first sampled
reading because work can already be in flight.

Diagnostic1's small observer passed in 10.38 s, and its test-target check
passed warning-free in 127.71 s. The reviewed 600 s client change passed
the same small observer in 8.97 s and its test-target check in 137.46 s.
That observer did not reach system startup logging or a request beyond
60 s. The M30 exact correct-code unit passed 1/1, test 0.00 s and wall
119.06 s. These are focused controls at the respective source candidates.

## Adjusted assertions

- **Tasks 9–10.** Task 9 repaired the test teardown and narrowed the exact push-state assertion to the ingest event with its durable row. Keyed IMAP preserves wire order and distinguishes witness updates from a real flags mutation. The first Task 9 key JMAP run failed at the ordinary Sales/admin thread partition. Later diagnostic and final runs passed. Its original cause remains unexplained. Task 10 turned the mail search refusal tables into served results and parity, including depth-20 BODY, while retaining depth-33 BAD, unsupported/mixed controls, whole-request unavailable and keyless refusal. Mail reads now expect keyed snippets. The trace fixture expects tagged OK for FROM and retains the original redaction and malformed controls.
- **Tasks 11–12.** The gates table expects exact keyless refusals for the mapped methods, with served mailbox and Core controls; the shared helper moved. Read fixtures retain persisted singleton rows but assert mapped wire IDs and collapse. Task 12 replaces interim UID-set-only THREAD with grouping, and asserts mapped members and derived counts while keeping actual independent singleton controls. Shared keyed cleanup had erased durable log and allocators while keeping the manifest. Its diagnostic isolated that mismatch and its correction preserves the retained Email history/namespace, retaining ordinary resets. The key JMAP run still failed at the old 100 actual vs 978 expected singleton-thread assertion.
- **Task 13.** Exact old/new mapped IDs, Initial/continuation presence, both retained markers and changed epoch rules replace raw singleton rendering. F15 observed an idle FLAGS/UID notification, unchanged explicit FETCH MODSEQ for that UID and omission from CHANGEDSINCE, with advancing HIGHESTMODSEQ. At Task 13 acceptance, the caller-only Content queryChanges floor was source-verified and the shared minimum/wait/deadline had runtime evidence. The later independent remaining-checks task exposed the masked Content assertion and supplied the focused baseline/fault-tested case described under Mutation runs; full-suite integration qualification passed with both default and FileSystem blob storage. Initial key JMAP failed in 9.81 s at 799 of 978 mapped IDs retrieved by the old chunk loader, before Task 12's old singleton assertion. Advancing prefixes across Get batches is a source-supported explanation. Task 17 replaced the loader with content currentness and exact ID coverage and passed canonical key JMAP.
- **Task 14.** The first canonical RocksDB run exited 101 in 531.79 s at the earlier Initial-after-truncation test. A 23.52 s reproduction showed marker 7, retained row 8 and legitimate checkpoint movement 6→8 during reconciliation, with zero publication, unchanged epoch 0 and newest row 3@7, equal inventories and retained Email records, and no later Thread suffix. The one-file fix requires exact complete checkpoint 8, cleared marker and all preserved identity and wire checks. Focused rerun 86.84 s and corrected full RocksDB 722.31 s both exited 0. R16/R20 asserts keyed `{A},{N,N2}` versus ordinary `{A,N,N2}` with D absent.
- **Task 15.** The 737.88 s canonical run failed the warm-eviction assertion after the fixture's requested budget override missed the actual cache budget. A targeted actual-cache override tests the effective budget. Subsequent scoped corrections covered real allowance cases, cold decode/merge accounting and linked retirement ownership; accepted full RocksDB 766.93 s exit 0. The separate warm-write finding closed at `b51a5e193`.
- **Task 16.** The first 16a FileSystem attempt stopped before suite execution at a log-directory barrier. The amended freeze ran on both backends. Task 16b's I6 RED saw field 152 still present before Reindex integration. The final recode oracle accepts adaptive compression for sealed blobs, still requiring exact decoded bytes and selected key; the compressible ordinary control requires Zstd. The modeled stale-reset overlap is a single-manager fixture.
- **Task 17.** IMAP SEARCH/THREAD now use upstream bodies; keyed FETCH retains sealed attachment flags and tests the saved FROM path after content currentness, and STORE delegates to upstream. JMAP query, snippet, thread get/merge and queryChanges assert real outcomes. The artificial Move in key queryChanges aliases fixture IDs and real merge parity lives in Task 13. Synthetic raw log changes runs on a verified ordinary account even in the key suite. WebSocket and Bearer gates preserve exact refusals plus served mailbox controls, the converted socket, push shape and state/trace checks. Initial failures include removed helper/required import compile errors, leftover OAuthGrant, a 5 s helper transport deadline (reproduced and fixed locally), an actual 30.618 s `not_current` refusal after 59 passes/2,832 of 3,240 documents, and independent submission and EventSource fixture boundaries. Bulk setup uses real keyed work to cover a captured boundary before the unchanged check. The submission correction uses a real colliding numeric ID and preserves Email effects. The keyed EventSource import can legitimately split delivery across commits; it requires exact durable insert-bound EmailDelivery, the latest committed state triple and following ping. The ordinary fixture stays intact. Final scoped canonical JMAP key/plain were 172.39/138.89 s, IMAP key/plain 16.24/14.44 s, ZA RocksDB 799.95 s, all exit 0.
- **Task 18.** The scanner reports linked search blobs and manifests. The mail fixture requires one key manifest and at least one linked index blob while retaining two stubs, two emails, positive canaries and the empty leak/undelivered sets. Separate exact link and planted controls distinguish Principal document zero and ordinary accounts. Initial membership failed full link-size equality because recipient-specific Delivered-To is indexed; the fixture scopes/restores that setting and uses case-only reference variants to keep terms equal and memberships distinct. The first differential stopped at SMTP 452 before parity (4.15 s). Two named inbound throttles are temporarily raised in each disposable delivery scope and restored/reloaded exactly. Initial differential partition attempts failed (998 keyed vs 1,001 expected after the pinned split in the compatibility run; the passive report observed 998 vs 1,002 expected with four keyed unions). The ordered counterfactual passed partition and cleanup, establishing a controlled fixture schedule rather than the causes of the previous interleavings. The first controlled scanner run passed membership and F1 measurements then failed message cleanup (12.32 s). The first full differential passed pair/readiness/partition and failed the first-part six-letter assumption (631.75 s). Message-only cleanup and a longest first-part word got past these; the next runs failed the sealed-vs-loaded public row comparison (96 vs 108, 66.26 s) and array-only snippet `notFound: null` handling (618.56 s). Exact exclusion of the unique manifest-overlay row and accepting the typed null representation preserve the strict oracles. The scanner path then passed in 91.26 s. The differential then failed a From-labelled order at ordinals 2697/2696 after 647.47 s; their actual timestamps were unmeasured. A separate passive run failed at the original exact assertion in 642.23 s and demonstrated equal From ranks and different receive timestamps for 878/873. The scoped fixture guard supplies identical arrival before normal ingest and reads back every stored timestamp, with shipping `received_at: None`. The first guard unit command omitted `common/test_mode` and stopped before tests; the corrected invocation passed. The final focused differential passed in 662.74 s with 183 forms, G5 and teardown. These times describe functional runs.

- **Task 18 canonical correction.** The first canonical run failed at R16
  in 693.42 s. The ordinary control inserted its late reply after deleting
  D, while the key account inserted before deletion. The correction aligns
  the ordinary order and positively checks the A/N survivor grouping, with
  key assertions unchanged. Focused threads passed in 63.36 s, scoped
  independent review passed, and the full canonical suite passed in
  1664.38 s with cleanup.
- **Task 19.** The first focused fixture run failed at the timeout read-count
  oracle (196.16 s, exit 101). Mailbox/get made a second loader entry.
  Mailbox/query retains the production one-attempt pre-pass and exact
  2/4 reads, no rebuild, unchanged state and retry assertions. The corrected
  threads/index run reached all five controls and teardown in 276.95 s,
  exit 0. The benchmark setup correction separately recognizes the
  authenticated empty manifest. The M30 fixture now opens all 39 declared
  records and retains its two complete(48) correct-code expectations.

## Accepted residuals

- Keyed freshness starts with a keyed request, and cross-node visibility carries the specified one-response bound. Content can refuse unavailable, and an unvalidated or unavailable map gives no map. Keyless methods follow the documented gates. Keyless Email/set update/destroy can accept a stale prefix.
- Thread windows can be a superset. The K nearest references are bounded; when all K are absent and an older referenced ancestor is present, the key account can stay singleton where upstream joins.
- Rebuilds invalidate pre-epoch sync where the method log provides the relevant evidence, with the stated Initial/continuation rules. Rebuild ordering can choose a different surviving thread identity. IMAP EMAILID and THREADID can then change without a CHANGEDSINCE signal.
- IMAP CHANGEDSINCE omits identity-only reassignment and the item's modseq stays fixed. A publishing pass still emits the Email witness update and can send unsolicited FLAGS for that witness, with an advancing HIGHESTMODSEQ. Mailbox/changes reports every mailbox after publication.
- §5.8 retains two missing-evidence cases. A message deleted before any keyed request indexes it, or before the rebuild cursor reaches it, contributes no evidence to a later message referencing only it. Ordered parity requires the same settled ordinary delivery, deletion and thread-merge history; concurrent work can expose different candidates.
- EmailSubmission/get projects the current thread rather than a stored submission-time prefix. Initial renders present objects as created even if upstream calls them updated or loses the insert. After truncation a gap insert can thread with a message the gap also deleted. Upstream would have threaded that way only when the insert came first.
- Duration and the checkpoint where the time cap stops reveal admitted processing cost (§2, R30/H4). Delivery still keeps off the features enumerated in §6.4.
- The permanent-destroy fence covers already admitted creators against the captured vault/manifest evidence. Later issuance from an already authorized stale setup token and potential later rebuild remains the narrow §5.7 residual. This is an authority/lifetime limitation, separate from duplicate-worker cleanup and the tested reset fences.
- The PostgreSQL point-read figure, native clients, the separate broader
  fresh-store gate and Docker-only evidence remain pending. The
  [cold-start qualification](2026-11-10-zero-access-search-cold-start-outcome.md)
  records the completed local Rust and shipping cold-search measurements
  and the unresolved Rust teardown anomaly.

## Deferred findings

- Task 3 construction reversal alone did not force both hash iteration orders. The deterministic Task 15 companion forces the downstream-first map traversal. The 55-draft campaign's `actual-map-order` companion detected the fault at its chain-order assertion; this former mutation gap is closed.
- Task 9 first Sales failure remains unexplained. Later passes record non-recurrence.
- Task 10 source review deferred the unchanged ordinary/mixed SAVE construction that may wait on its own saved-search result. No runtime reproduction was claimed.
- Task 13 caller-only Content floor now has a focused correct-code pass and a named fault failure in the independent remaining-checks report. The exact test-only proposal is integrated; the full ZA suite passed with both default and FileSystem blob storage. Current-row values in queryChanges are the accepted scope. A revision-indexed performance shortcut depends on Task 19 measurement.
- Task 15 warm admission finding closed separately. The benchmark now records 30,000-message sealed headroom and warmed residency. Final production limit qualification remains open.
- Task 17 IMAP helper uses a 1.5 s per-line read deadline. Its warm fixture passed. A cold-backlog IMAP wait-budget result remains absent; the 5 s JMAP cutoff was separately reproduced and corrected.
- Task 18 original ordering timestamps and exact earlier partition schedules remain unobserved. The corpus includes bounded synthetic controls and real mailbox records. Broader attachment-format/client coverage is open.

## Merge hotspots

The prepared file-and-line notes cover §11 and the branch additions below.
Reconcile these anchors and the frozen-head refusal assessment's source
anchors against the final source head during Step 3. Rebase links to
ignored evidence when moving this candidate into the tracked plan folder.

| Seam | Source anchors |
|---|---|
| Request ownership and key gates | [JMAP request dispatch](../../../crates/jmap/src/api/request.rs#L99), [IMAP client dispatch](../../../crates/imap/src/core/client.rs#L137) |
| Admitted views, waiting and history | [View snapshot](../../../crates/email/src/message/za/search/view.rs#L135), [Recovery and wait](../../../crates/email/src/message/za/search/ensure.rs#L72), [Shorter retained log](../../../crates/email/src/message/za/search/changes.rs#L22) |
| Compaction and bounded writes | [Compaction choice](../../../crates/email/src/message/za/search/write.rs#L321), [Thread assignment retention](../../../crates/store/src/za_search/thread.rs#L256) |
| Keyless lifecycle and recovery | [Reindex task](../../../crates/services/src/index_queue/reindex.rs#L152), [Reset fence](../../../crates/common/src/za/search/destroy.rs#L182), [Content-only recovery](../../../crates/email/src/message/za/search/rebuild.rs#L505) |
| Sizing, admission and instrumentation | [Search defaults](../../../crates/common/src/za/search/config.rs#L31), [Boot observation](../../../crates/common/src/manager/boot.rs#L211), [Loader admission](../../../crates/common/src/za/search/load.rs#L780), [Test-only paired delivery](../../../crates/email/src/message/delivery.rs) |

Keep the query/sort/snippet/change adapters, cached row lookup and mapped
IDs, state counters and IMAP CONDSTORE/scheduling, store log and batch
assignment/serialization, vault setup and key derivation, Principal
fields/links, hold/release/purge and BlobRecode, cache invalidation and
swap exclusions, and upstream tokenizer/leaf/rank/thread behavior in the
final merge notes.

## Open questions for Jay

Jay approved the provisional limits, both final reviewers and proceeding to a
draft PR with the remaining qualification listed. M29b is an explicit unexecuted
campaign exception. The three strengthened tests have correct-code results only;
new mutation sensitivity and the recorded alternate-catcher findings remain
unestablished where indicated above. Docker CI, native clients, the separate
fresh-store gate, networked-store timing and physical multi-node verification
remain open; opening a draft does not waive them or declare Task 19 complete.
Ask about a default only if completed evidence supports a change.
