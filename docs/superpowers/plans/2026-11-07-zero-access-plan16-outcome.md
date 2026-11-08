# Zero-access search and threads: plan 16 outcome

2026-11-08. [Plan 16](2026-11-07-zero-access-16-search.md) and
[search design](../specs/2026-11-04-zero-access-search-design.md), spec revision 20 for the measured format; revision 21 records the subsequent memory-default ruling.

## Format benchmark (Task 1)

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
### Task 1 validation

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
`crates/store/src/dispatch/blob.rs`; that file is unchanged. The linked test
artifact also carried Darwin's `__eh_frame` size warning. These runs are
passing, but their output is not warning-free. No new warning remained in
Task 1's changed source.

Task 1 used `gpt-6.1-sol` at medium reasoning for implementation and independent
`gpt-6-astra` at xhigh for review. The reviewer approved the code, measured
evidence and provisional-default documentation with no P0–P2 findings;
the existing warnings above remain disclosed. This is task acceptance, with
whole-branch and loaded-memory qualification still pending.
