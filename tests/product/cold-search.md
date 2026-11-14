# Shipping cold search qualification

Run this gate with a clean, optimized OSS shipping build and the matching
Rust benchmark export. The full gate requires seed 1 and 30,000 messages.
Use an explicit five-message smoke export first to check fixture setup.

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo build --release --locked -p stalwart --no-default-features --features rocks
shasum -a 256 "$SHIPPING_BINARY"
"$SHIPPING_BINARY" --version

SEARCH_BENCH_EXPORT_DIR="$CORPUS_DIR" \
  cargo test --release --locked -p tests za::search_bench::export_corpus \
  -- --exact --ignored --nocapture

python3 -B tests/product/cold_search.py \
  --binary "$SHIPPING_BINARY" --binary-sha256 "$SHIPPING_SHA256" \
  --source-commit "$SHIPPING_SOURCE_COMMIT" --corpus "$CORPUS_DIR" \
  --work-dir "$FRESH_WORK_DIR"
```

Use `SEARCH_BENCH_EXPORT_COUNT=5` for a separate export and pass `--smoke` to
the Python runner for that export. Use separate fresh work directories for
smoke and full runs. Supply a new work directory with an existing nonsymlink
parent, such as a path under `/private/tmp` on macOS. The full run retains its
RocksDB store and needs space for both the export and the store.

Keep the Cargo build command, source, build output and binary digest with the
run receipt. The runner matches the actual binary digest and clean source in
`--version` against those inputs. Use the recorded build invocation to establish
the feature set and optimization profile. Build the Rust test binary separately
and give the shipping run its own store.

## Fixture and delivery

The runner verifies the version, seed, count, ordered filenames, lengths, every
message digest and the concatenated corpus digest. It verifies each message
again before sending the exact bytes through a single LMTP BDAT transaction.

The shared fixture bootstrap briefly uses Stalwart's recovery HTTP listener,
which binds the IPv6 wildcard address with IPv4 fallback. The runner connects
through loopback using newly generated recovery credentials. Recovery provisions
the loopback HTTP and LMTP listeners, disables application downloads, spam
filtering and ASN lookups, then stops cleanly. The synthetic key account and
mail exist only after normal startup. Before LMTP delivery, the runner opens
a keyed JMAP session and calls `Mailbox/get` to initialize system folders.
It validates the response and requires exactly one Inbox.

LMTP accepts only the local recipient. The runner sends a null envelope sender,
disables authentication, relaying, sender DNS checks, inbound throttles and
delivery header additions in the private fixture, and caps each connection at
50 submissions. The account has an email quota of the corpus count plus 100 and
a disk quota of twice the raw MIME size plus 1 GiB. The server environment
clears inherited Stalwart and zero-access settings. The runner checks startup
for all fourteen original search defaults.

The LMTP reply with a queue ID acknowledges a committed queue write. The gate
checks the SMTP queue, ordinary index queues and due tasks through the
recovery administrator after delivery. Bounded setup searches then drive indexing.
The before-restart public census compares the full unfiltered mailbox ID set
with the full body-canary result set. Every generated body starts with
`bodycanary`. Each set must have the expected total, distinct IDs and one stable
query state across pages. The limit-1 identity must match the first filtered
census ID at the same query state. The runner records that identity and state
as the baseline.

## Measured pair and evidence

The runner stops and restarts the same shipping binary on the same store.
Readiness uses only the plain recovery administrator's session endpoint.
Each measured request opens a new HTTP connection and sends
`Email/query` with `{"filter":{"text":"bodycanary"},"limit":1}` and the
fixture account ID. The repeat follows immediately, including after a first
JMAP refusal or transport error. There are no additional requests between the
pair. The client allows up to 600 seconds per request while the original
30-second Content admission interval remains in effect.

Both results must match the baseline ID and query state. After the pair, the
runner checks full public coverage and compares its count, stable ID-set digest,
first ID and query state with the before-restart census. The evidence covers public
delivery and correct search through a process restart. The separate Rust
diagnostic measures internal generation coverage, indexing writes and sealed
reads. This run retains the host's filesystem caches.

`receipt.json` records the command, binary version and checksum, corpus digest
and counts, fixture settings, phase, both structured outcomes, and elapsed
times. The `mailbox_initialization` receipt records the setup method, response
validation, mailbox and Inbox counts, Inbox role and mailbox state. The private
work directory and sanitized server log are retained on failure. The runner sends termination only to its own child.

Run the mocked checks without listeners or a server.

```sh
python3 -B -m unittest discover -s tests/product -p test_cold_search.py -v
```
