# Zero-access search cold-start qualification

2026-11-10. The optimized Rust run met the measured 30,000-message
search-cache eviction gate on clean source
`9ea951df965af634611fb6311afa987736f61242`. The run logged a Tokio worker
panic during teardown after the measurements. The optimized OSS product
from that source also met the 30,000-message process-restart gate using
the reviewed mailbox-initialized fixture. The two results close the local
cold-search qualification for this corpus and configuration. Task 19
remains incomplete.

## Optimized Rust measurement

Stable Cargo and Rustc 1.99.0 linked the test executable with
`cargo test --release --locked --offline -p tests --lib --no-run -j2`.
The release profile used opt-level 3, fat LTO and one codegen unit. The
existing development feature graph included `test_mode` and `enterprise`.
Cargo exited 0 after 27m 50s. It relayed an Xcode SDK-discovery warning
about FSEvents and the developer cache path before the successful link.
The primary accepted the Mach-O arm64 artifact after reviewing the
diagnostic and checking the source and artifact hashes. The executable
SHA-256 was
`bfcfba02ff135d2b53f90a8bf054fd368b196ee6d886594ad35bd32f9aa1aecf`.
[Build and controls](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/controls/report.md)
[Warning ruling](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/controls/build-warning-adjudication.json)

The focused observation-scope and first-failure retention tests each passed.
The five-message plumbing control passed with 116 filtered. Its cold query
and repeat took 5.78725 ms and 235.333 µs, with two and zero verified sealed
blob reads respectively. The corpus exporter produced separate five- and
30,000-message seed-1 fixtures. Verification checked every file's order,
length and SHA-256, plus the concatenated MIME digest. The full corpus
contained 9,000 attachments and 2,436,029,084 raw MIME bytes. Its SHA-256
was `7c6a492dae21db8d2538595b5cf0f8ed659db7abacf4e9c969298f9e61f15987`.
[Control and export receipts](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/controls/report.md)

The linked binary ran `za::search_bench::cold_start --exact --ignored --nocapture`
with RocksDB and all fourteen default search settings. It delivered the
seed-1 corpus, then used serial keyed `Email/get` requests to finish normal
indexing without milestone Content loads. The durable checkpoint had epoch 0
and revision 625, ten Thread segments and ten Content segments totaling
452,374,469 bytes. The harness evicted both search-cache parts and checked
for zero charges before sending a limit-1 body-canary `Email/query` over
loopback HTTPS. A passive cache snapshot separated it from the immediate
repeat. The server stayed in the same process and retained the host's
filesystem caches. The configured 30 s Content admission interval and
600 s client ceiling both applied.
[Full run report](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/rust-full/report.md)
[Instrumentation review](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/reviews/instrumentation-review.md)

| Query | Wire time | Public response | Verified sealed blob reads |
| --- | ---: | --- | ---: |
| First after eviction | 26.469407209 s | Expected single ID and query state | 20 |
| Immediate repeat | 507.458 µs | Same ID and query state | 0 |

Both requests acquired Content admission. The cold admission span was
26.415587791 s, with 3.584411792 s remaining at admission. Postings was
the largest measured loader substage at 23.91180525 s. Stage durations
include nested work and cannot be added together. The census after both requests
found every delivered ID in exact-head current Content and live Thread.
The authenticated durable manifest and Email state were unchanged. The
indexing writer emitted zero commit events during the pair. That counter
covers commits through the existing search indexing writer. The census
and durable-state comparison supply separate evidence.
[Measurements](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/rust-full/measured-evidence.json)
[Raw test log](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/rust-full/full/test.log)

Libtest selected one test and reported one passed, zero failed and 116
filtered in 254.72 s. The raw exit was 0 after 254.82083 s wall time.
The raw log records a Tokio worker panic after the pair, census and
durable-state reports and before libtest reported success. A timer was
polled after its driver stopped. The run has no backtrace, and the
panicking task and cause remain unidentified. The fixture follows an
existing shutdown sequence that does not wait for background tasks to
finish. Independent source triage found the search assertions precede
cleanup and accepted the measured result with the teardown anomaly open.
[Run result](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/rust-full/full/result.json)
[Teardown triage](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/reviews/teardown-triage.md)

One follow-up ran the exact five-message plumbing control with the same
linked binary, a fresh temporary directory and `RUST_BACKTRACE=full`.
It exited 0 with one passed and 116 filtered in 0.98 s test time and
1.026 s wall time. The panic did not recur at this smaller size and
timing. The full-run teardown anomaly remains unresolved.
[Five-message teardown diagnostic](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/teardown-diagnostic/receipt.json)

## Shipping restart qualification

The focused harness review and 12 mocked checks support running the
shipping gate. The separate runtime gate calls for a clean optimized OSS
binary without `test_mode`, a five-message smoke and a full 30,000-message
run on its own RocksDB store. The public checks compare the mailbox and
body-canary ID sets and query state before and after restart. The two
measured requests follow readiness and use the same limit-1 query and
expected baseline result. Internal generation, read and writer-commit
evidence comes from the Rust run above. The shipping gate keeps the
host's filesystem caches.
[Shipping procedure](../../../tests/product/cold-search.md)
[Focused harness review](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/reviews/shipping-fix-review.md)

The separate shipping build used
`cargo build --release --locked --offline -p stalwart --no-default-features --features rocks -j2`
at clean source `9ea951df965af634611fb6311afa987736f61242`. Cargo exited 0
without warnings in 1,591.402 s. The feature receipt shows RocksDB without
`enterprise` or `test_mode`. The 85,405,200-byte binary reported the clean
source and fork version `0.4.0-beta.1` on upstream `v1.0.3`. Its SHA-256
was `7f224cdd527776fdb28a18934c3dd78e4e4a62c41165169c9e049b6e86b12dbf`.
[Shipping build receipt](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/shipping/original/build/outer-receipt.json)

The first five-message smoke queued all five messages and completed the
before-restart census. The limit-1 baseline was ID `eaaaaab` at query state
`say` (Exact 6). After a clean restart, both measured requests returned
that ID at state `sa2` (Exact 7), in 97.236 ms and 0.534 ms. The runner
exited 1 on the strict baseline state comparison before the after-restart
census. Source analysis identified lazy default-mailbox creation and cache
hydration as a possible explanation for the state transition. The source
and receipt leave the origin of the extra revision unconfirmed.
[Initial smoke receipt](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/shipping/original/smoke/store-run/receipt.json)
[Initial smoke exit](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/shipping/original/smoke/outer-receipt.json)
[Independent state analysis](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/reviews/shipping-state-review.md)

The reviewed fixture change issues a public `Mailbox/get` before LMTP
delivery and requires exactly one Inbox. It moves mailbox creation ahead
of corpus indexing and keeps the two measured requests, the strict
baseline state comparison and the complete before/after census checks.
The candidate runner had SHA-256
`9c3871fe128ce07738c31444d58bdee352093f80051e4b036bcd72bde89e268c`
and passed 14 mocked checks. The five-message follow-up used that exact
copied runner with the same shipping binary and a separate fresh store.
The runner exited 0. Both measured queries matched the baseline ID and
state `sa2` in 104.183 ms and 0.866 ms. Both public censuses found five
messages with the same ID-set digest and query state. This supports the
setup-order explanation while leaving the original record 7 unattributed.
[Reviewed fixture proposal](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/shipping/mailbox-initialized/mailbox-proposal-review.md)
[Follow-up smoke receipt](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/shipping/mailbox-initialized/mailbox-smoke/store-run/receipt.json)
[Follow-up smoke exit](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/shipping/mailbox-initialized/mailbox-smoke/outer-receipt.json)

### Full 30,000-message restart

The full run used the same clean `9ea951df` shipping binary and exact
copied runner with SHA-256 `9c3871fe128ce07738c31444d58bdee352093f80051e4b036bcd72bde89e268c`.
The checkout remained clean throughout execution. The copied runner is a
Python harness change separate from the build provenance of the binary.
The run used a fresh RocksDB store and the verified seed-1 corpus, queued
all 30,000 messages and checked the original fourteen settings. It
recorded seven setup search refusals before the first complete public
census. The measured requests both succeeded.

| Query | Wire time | Public response |
| --- | ---: | --- |
| First after process restart | 23.053401084 s | ID `eaaaaab`, state `sulxqc` |
| Immediate repeat | 1.026083 ms | Same ID and query state |

The before- and after-restart censuses each verified 30,000 distinct
IDs with matching mailbox and body-canary result sets, first ID and query
state. Both censuses recorded ID-set SHA-256
`39824dd71976dc094d62c46c557013a1e9253e49e0c634a1667ce657fb0c013c`.
The product stopped cleanly. The runner and wrapper both exited 0 in
288.757 s and 289.061 s respectively.
[Full shipping receipt](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/shipping/mailbox-initialized/mailbox-full/store-run/receipt.json)
[Full shipping exit and provenance](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/shipping/mailbox-initialized/mailbox-full/outer-receipt.json)
[Sanitized server log](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/shipping/mailbox-initialized/mailbox-full/store-run/server.sanitized.log)

## Relation to Plan 16

The earlier unoptimized system run at `4dde13ade` returned
`unsupportedFilter` with reason `not_current` after 188.973701 s
and skipped its linked repeat. That result still applies to that run.
The optimized experiment also used a different fixture setup and produced
a slightly different sealed chain, so the two timings do not isolate the
effect of optimization. All fourteen search settings are unchanged. Memory
and account sizing defaults remain provisional. The new run is separate
from the historical 39 of 41 main Step 5 matrix rows accounted for locally.
[Historical system benchmark](2026-11-07-zero-access-plan16-outcome.md#final-head-system-benchmark)

Task 19 remains incomplete. Behavioral execution of the 55 prepared
drafts and the rest of the mutation campaign, Docker automation and
directory verification, native clients, the separate broader fresh-store
restart gate, networked-store timing and physical multi-node verification
remain pending. The shipping cold-search run covers its own search
fixture. The broader fresh-store gate also exercises ordinary and key
accounts, attachments, calendars, recurrences, pins and a share.

The evidence links point to checkout-local ignored files under
`.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/`.
Ordinary clones do not include these files. The retained corpus manifests
record the digests of the raw MIME exports, which remain in private temporary
storage and can be regenerated by the seed-1 exporter. The shipping
receipts and sanitized logs are retained here. The private shipping stores
remain in their original locations. The pre-shipping
checksum inventory covers the original 89-file checkpoint with relative paths.
Original inventories retain the absolute paths of the runs.
[Final retained evidence inventory](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/retained-evidence.sha256)
[Earlier 89-file checkpoint](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/pre-shipping.sha256)
[Full corpus manifest](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/corpus-manifests/30000.json)
[Original full-run inventory](../../../.superpowers/sdd/2026-11-07-zero-access-16-search/cold-start-qualification/rust-full/artifacts.sha256)
