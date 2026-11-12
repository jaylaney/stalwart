# Building and testing the zero-access fork

See the [release policy](releases.md) for independent versioning, tags, upstream
provenance, and the release checklist. Record fork release changes in the
[fork changelog](../CHANGELOG-fork.md); the root `CHANGELOG.md` remains upstream history.

- Base: upstream Stalwart `v1.0.3` (commit `8b32686532d34623e9ce8e1237cf2bed9554af38`). The fork's own version is independent of it: `fork-release.toml` records both, the shipping `stalwart` package carries the fork version, and the internal crates keep upstream's `1.0.3` (the fork-owned `vault` crate keeps `0.16.25`). `stalwart --version` prints the fork version, the upstream tag and commit, and the source commit.
- Fresh stores only. A 1.0-based build cannot open a store written by upstream 0.16 or by the fork's 0.16-based releases (`0.1.x`), and there is no conversion: the server stamps a new store with its schema version and edition and refuses any other. Old stores are unsupported; move an installation by provisioning a fresh store (new setup tokens, new enrolment), keeping the old store and binary for rollback. The local-server scripts default to `.run-1.0/` for this reason.
- Package names: `-p http` is ambiguous (the dependency graph also holds the crates.io `http` crate, through hyper, reqwest, axum and others); select the fork's with `-p http@1.0.3`. `cargo fmt` rejects that form, so format it with `cargo fmt --manifest-path crates/http/Cargo.toml`.
- Toolchain: stable Rust via rustup. Xcode command line tools for RocksDB.
- On this machine rustup is installed via Homebrew (keg-only): `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"` before using cargo. The curl installer in the plan is not needed.
- Build (tests): `STORE=RocksDb RUST_MIN_STACK=16777216 cargo build -p tests`
- Build (product, no enterprise code): `cargo build --release -p stalwart --no-default-features --features rocks`
- CalDAV suite (upstream, non-key accounts): `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests`
- CalDAV suite against key accounts (added in plan 1, task 11): `STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav_tests`
- Vault unit tests: `cargo test -p vault`
- Leak regression test (plan 3): runs inside `za::za_tests` (`tests/src/za/leak.rs`); there is no separate test to select.
- Upstream's `cal_itip` sub-test is timing-sensitive (DTSTAMP index mismatch when a second boundary falls between two iTIP operations); rerun once before treating a failure there as a regression.
- The za suite (`za::za_tests`) now ends with the leak regression test (`tests/src/za/leak.rs`). On failure it panics with a list of violation lines, each `<where>: <what>`: `subspace 'X' key [...]: raw value contains <canary>` (a plaintext canary string found in a store key, raw value or decoded blob), `missing <prop>` or sealed-tree complaints (an unsealed or wrongly shaped iCalendar tree), and `display_name`/`dead_properties`/event-preferences not empty (plaintext fields on a sealed row). The subspace letter and key locate the record.
- Full verification: `cargo test -p vault -p groupware -p common`, then `za::za_tests`, then `webdav::webdav_tests` in both modes.
- Manual client checklist: `docs/zero-access/manual-checklist.md`.
- Known limits (spec 2): the running server holds plaintext while serving a request and receives the password on every CalDAV request; the authentication cache and the key cache are process-local and bounded (15 minutes idle, 60 minutes hard cap).
- rustfmt: `cargo fmt -p <crate> -- --check` works for most crates; for http use `cargo fmt --manifest-path crates/http/Cargo.toml -- --check` (cargo rejects `-p http@1.0.3` for fmt).
- Never open upstream issues or PRs from this fork (see AGENTS.md).
- Zero-access account API tests: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests`
- CalDAV suite against key accounts: `STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav_tests`. In that mode the `acl`, `cal_alarm`, `cal_scheduling` and `copy_move` modules run the four key-mode variants in `tests/src/webdav/za_variants.rs` (`acl`, `alarm`, `scheduling`, `copy_move`) instead of skipping.
- Key cache tuning (environment): `ZA_KEY_IDLE_SECS` (900), `ZA_KEY_MAX_AGE_SECS` (3600), `ZA_KEY_MAX_ENTRIES` (10000).
- Setup tokens expire after 7 days. The admin permission for `setup-token` is `sysAccountUpdate`.
- Account page origin (environment): `ZA_ACCOUNT_PAGE_ORIGIN`, e.g. `https://account.example.com`. Read once at startup; it is the only origin CORS allows on `/api/vault/*`. Unset, empty or not a valid header value leaves the vault API without CORS headers. Other responses keep the operator's own `Access-Control-Allow-Origin` (or permissive CORS) unchanged.

## Operating zero-access accounts

- Provisioning: an administrator with `sysAccountUpdate` calls `POST /api/vault/setup-token` with `{ "account": "<address>" }` for a user account that has no password credential and holds no data (the 409 reasons are in `CLAUDE.md`'s "Where release 3 lives"; at 1.0 they include "account already holds object metadata"); the response carries a one-time token (7 days). The user then calls `POST /api/vault/setup` with `{ "username", "token", "password" }` and receives the recovery key, shown once. No administrator ever chooses or sees the password.
- Marker: a key account's registry password credential holds the literal `$za$` instead of a hash. It classifies the account; the real verifier and key wraps live in the vault record. Credential edits through the registry API are refused for key accounts; password, recovery key, app passwords and TOTP are managed only through `/api/vault/*`.
- Corrupt or missing vault record: a key account whose marker has no usable record cannot log in (the refusal names `zero-access vault record missing` in the log). This only arises from data loss; recovery is operator intervention (restore the record from backup). There is no in-band reset: the server cannot recreate the keys.
- Key cache: unlocked keys are held per node in process memory only, never shared across a cluster or written to disk. Each node applies `ZA_KEY_IDLE_SECS` (sliding idle timeout, 900), `ZA_KEY_MAX_AGE_SECS` (hard cap from login, 3600) and `ZA_KEY_MAX_ENTRIES` (LRU bound, 10000), swept every 30 seconds; a node logs the effective values at startup. `SessionKeys` are attached by the HTTP layer for one request, and by the IMAP and POP3 login handlers for the life of the authenticated connection (mail spec 3.3; mail logins do not use the key cache: each derives afresh and the keys live in the connection's own state); WebSocket, EventSource, ManageSieve and SMTP sessions never hold them.
- Test builds (`test_mode` feature) use weakened Argon2 parameters for new passwords and log a warning at startup; never deploy them.

## Searching key accounts

Release 7 builds a per-account search and thread index inside requests holding
that account's keys. The server derives the search key from the account's master
key and seals the index into blobs linked to the principal document. Field 152
on the principal holds the clear, keyed-tagged manifest. Each server process
loads the index into memory while the account is in use.

The server reads these environment variables at startup and logs the effective
configuration. Unset, malformed and zero values use the defaults. The memory
and account sizing defaults remain provisional after the unoptimized system
benchmarks on `f4eb2ecc` and `4dde13ade`. The final-head post-H7 wire query
served and internal model coverage was verified for all 30,000 delivered
IDs. Its cold query refused and its linked repeat was skipped. The
final-head format repeat measured all 15,000 default messages and retained
the existing positions and compression decisions. The later
[cold-start qualification](superpowers/plans/2026-11-10-zero-access-search-cold-start-outcome.md)
records a successful optimized Rust cache-eviction pair at 30,000 messages
and the subsequent teardown panic. The separate optimized OSS run passed
the 30,000-message process-restart gate with matching public censuses.
Those qualification runs retained all fourteen original settings. The PR 84
correction removes the transient skip threshold; the sizing defaults stay unchanged.

| Variable | Default | Meaning |
| --- | ---: | --- |
| `ZA_SEARCH_MEM_MB` | 1536 MiB | Per-process budget for loaded content parts and their transient allocations. |
| `ZA_SEARCH_THREAD_MEM_MB` | 256 MiB | Separate soft per-process budget for thread parts and their transient allocations. |
| `ZA_SEARCH_ACCOUNT_MAX_MB` | 512 MiB | Per-account cap for the sealed content chain. Applies to loads and extensions. |
| `ZA_SEARCH_IDLE_SECS` | 900 s | Idle time after which a loaded index can be evicted. |
| `ZA_SEARCH_PASS_CAP` | 48 items | Admitted-item limit checked at the next insert boundary. |
| `ZA_SEARCH_PASS_MB` | 32 MiB | Opened message bytes limit checked at the next insert boundary. |
| `ZA_SEARCH_PASS_HIDDEN_KB` | 32 KiB | Reserved hidden thread bytes limit checked at the next insert boundary. |
| `ZA_SEARCH_PASS_MS` | 2000 ms | Elapsed-time limit checked at the next insert boundary. |
| `ZA_SEARCH_WAIT_SECS` | 30 s | How long a content request keeps completing passes before refusing. |
| `ZA_SEARCH_SEGMENTS` | 16 | Thread-blob and content-chain compaction threshold, also the tiered content merge fan-in. |
| `ZA_SEARCH_CHECKPOINT` | 1000 records | Update-only records that trigger a manifest-only checkpoint. |
| `ZA_SEARCH_ABSENT_RETRY_SECS` | 10 s | Retry window for an absent message or index blob before absence is treated as loss. |
| `ZA_SEARCH_PAD_MIN` | 65536 bytes | Exact publishing target for the hidden thread section and minimum thread compaction padding class. |

An owned pass finishes its compaction and transaction before the content wait
deadline is checked again. Request latency can therefore exceed both the pass
admission interval and the content wait duration.

`K = 16` is a format constant. A change requires a format bump and rebuild.
The startup refusals below use the encoded record bound `R = 304` bytes and
alias length `a = 15` bytes. The server prints the actual values in place of
the placeholders.

```text
ZA_SEARCH_PAD_MIN={value} is below {min_pad} = ZA_SEARCH_PASS_HIDDEN_KB·1024 + R + (K − 1)·a + 4; refusing to start
ZA_SEARCH_PASS_CAP={value} is above {max_pass_cap} = floor(ZA_SEARCH_PASS_HIDDEN_KB·1024 / (R + (K − 1)·a)); refusing to start
ZA_SEARCH_ACCOUNT_MAX_MB={value} is above {max_account_mb} = floor(ZA_SEARCH_MEM_MB / 3); refusing to start
ZA_SEARCH_SEGMENTS={value} is outside 2..=255; refusing to start
```

Size the per-process content and thread budgets separately. Content requests
can be refused when content will not fit. A thread load evicts eligible
idle parts and may exceed its soft budget, with a trace for the overrun.
Set the same `ZA_SEARCH_ACCOUNT_MAX_MB` on every node of a cluster. The
suspended flag lives in the shared manifest, so nodes with different caps can
undo each other's suspension and repeatedly rewrite the manifest on requests.

The fixed `ZA_SEARCH_*` trace details identify passes, rebuilds, suspension,
refusal, eviction and errors using account or document ids, counts, sizes,
versions and reason classes. A content request that cannot obtain a current
content part returns
`Search for this account is not available right now; try again shortly.`

Loads are single-flight per account and part. Map refresh
reads the authenticated manifest and required Thread blobs. The Content
flight prepares the optional model outside the shared index guard and
publishes it only against the exact target. An optional Content failure
can refuse the Content request while a validated Map remains available.

A missing message blob within its retry
window aborts the pass and keeps the document pending. The pass preserves
the matching attempt count, using zero when there is no previous count.
Non-absent transient failures also stay pending, regardless of the persisted
attempt count. The old `ZA_SEARCH_SKIP_AFTER` environment variable is ignored.
A persistent transport or configuration fault may hold Content freshness until
it is repaired; recovery retries the pending message without a rebuild.
Eviction can forget the first absence time and extend the effective window.
The existing 30-second key-sweep event also sweeps idle search entries, without
requiring another keyed request; active readers, loads and passes remain protected.

A lost or corrupt thread index rebuilds on the next keyed request and publishes
a new identity epoch. Clients with sync states that cross that boundary need to
resync. Isolated content loss rebuilds content without changing thread identity.
If a blob store incorrectly answered “not found,” repair it and then run the
upstream per-account reindex task (`Task::AccountMaintenance`, type `Reindex`).
For a key account, the task resets the search index through its destroying
marker so the next keyed request can rebuild.

Run the ignored benchmarks by name in separate processes. Each creates its own test server. The
[plan 16 outcome note](superpowers/plans/2026-11-07-zero-access-plan16-outcome.md)
records the benchmark figures and qualification status. The final matrix
includes four carried rows, completed Phases A and B, and a warning-free
shipping release build on the source-identical review-record head. The
first full SMTP attempt exited 101. The permitted exact retry passed
once, with the cause still unknown. Docker automation and directory
verification await CI. Both final-head benchmarks completed, bringing the
matrix to 39 of 41 main rows accounted for locally. Remaining mutation checks,
native clients, the separate fresh-store gate, networked-store timing
and physical multi-node verification remain pending.

```sh
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::search_bench::format -- --exact --ignored --nocapture
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::search_bench::system -- --exact --ignored --nocapture
```

The thread, index, differential and search leak modules run inside
`za::za_tests`. Run the integration suites one at a time.

```sh
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests
STORE=RocksDb BLOB_STORE=FileSystem RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests
STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests imap::imap_tests
STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests jmap::jmap_tests
```

## Merging upstream

The current integrated upstream base is Stalwart `v1.0.3`, commit
`8b32686532d34623e9ce8e1237cf2bed9554af38`, on fresh stores only; the fork version is recorded in `fork-release.toml`. The fork moved from `v0.16.27` through v1.0.1 by the plan
[`2026-10-27-stalwart-1-upgrade.md`](superpowers/plans/2026-10-27-stalwart-1-upgrade.md); its
[outcome note](superpowers/plans/2026-10-27-stalwart-1-upgrade-outcome.md) records the merge, the hook
inventory, the conflict ledger and the qualification. The 0.16-based line (`0.1.x`) is not
maintained, and its stores do not open on 1.0.

Plan each future upstream integration separately. Merge a verified release tag, never the
tip of `main`, in an isolated branch/worktree; never rebase the published branch or add an
`upstream` remote. For example, when integrating the next 1.0 release:

```sh
git fetch https://github.com/stalwartlabs/stalwart.git tag v1.0.4 --no-tags
git rev-parse 'v1.0.4^{commit}'
# Verify the commit against upstream's published release before merging.
git merge --no-ff v1.0.4
```

Then, in the same branch:

- Update `upstream_tag` and `upstream_commit` in `fork-release.toml` and add the same tag
  and full commit to `PINNED_UPSTREAM_TAGS` in `scripts/release/verify_product.py` on the
  merge branch. The verifier checks that the tag names the pinned commit, the commit is
  an ancestor of the source, no newer known release is in the source's history, and the
  version matches `crates/common/Cargo.toml`. Release checkouts use the table when they
  have no upstream tags. Run
  `python3 -B scripts/tests/test_release_metadata.py` and
  `python3 -B -m unittest discover -s scripts/release -p 'test_*.py' -v`.
- Re-read the merge hotspots and silent dependencies that `CLAUDE.md` lists, with the
  upstream changes to each. The exhaustive matches (the IMAP and JMAP filter and sort
  classifications, `Server::za_has_tasks` over `Task`) stop the build on a new variant on
  purpose; classify it.
- Run the gates. `python3 -I scripts/tests/ci_steps.py` (the workflow check) and
  `python3 -I scripts/tests/test_gates.py` run in CI's Check Style job. Dispatch
  `test.yml` on the branch: its Test Retention Gates job (weekly and dispatch only) runs the coverage
  net and the inventory and coverage gates against
  `tests/qualification/baseline-0.16.27.json`, and the other jobs run every suite, the
  independent CalDAV client and the fresh-store restart gate.

Qualification uses public upstream and fork tests, with no private upstream suite dependency
(upstream's `stalwart-tests` slots compile only under `--cfg stalwart_tests`, which no build
here sets). Record exact candidate commands and results. A merge that changes the store's
schema version needs a fresh store again; there is no conversion. Back up the database, blob
store and configuration and verify restoration before testing existing data; replacing only
the binary is not a rollback guarantee.

Merge hotspots: `CLAUDE.md` lists them per release, and the upgrade outcome note's hook
inventory gives each fork hook in an upstream file with its contract and guarding tests.
The two oldest remain the most conflict-prone: `crates/common/src/auth/authentication.rs`
(the marker diversion in `route_auth_request`) and `crates/http/src/auth/authenticate.rs`
(the resident-key lookup, the generation fence and the impersonation marker inside
upstream's auth cache).

Storage checks at every merge (preserve stored struct layouts and verify upstream
migration effects explicitly; ciphertext rides in existing fields and moves with them):

- calcard: sealed bundles embed calcard's rkyv layout for `ICalendarEntry` and
  `ICalendarParameter`. At 1.0 calcard is a workspace crate (`crates/calcard`), so it changes
  with upstream merges. If a merge changes that layout, old bundles need a versioned reader
  keyed on the bundle format byte before the binary ships; the user's key is available at
  login, so lazy re-sealing is possible. At 1.0 dead properties no longer ride in the
  `X-ZA-EXTRA` bundle: they are object-metadata containers (`seal::container`), so a change
  to `types::metadata` needs the same check.
- Numeric collisions: three values sit in gaps upstream has not used; confirm the merge did
  not take any of them. `PrincipalField::ZeroAccessVault = 150` and
  `PrincipalField::ZeroAccessPins = 151` are in `crates/types/src/field.rs`;
  `ACCOUNT_IS_KEY_ACCOUNT = 1 << 9` is in `crates/common/src/auth/mod.rs`. `VanishedCollection::CalendarIdentity = 250`
  (`crates/types/src/collection.rs`) separately reserves a log-collection byte; preserve
  existing 251–254 and check this reservation at the same merge gate.
- Visibility policy: upstream code that derives data (time ranges, alarms, the UID index,
  the privacy flags from `CLASS` under field policy 2) reads only properties the policy
  keeps visible; a merge that adds a new derived value must be checked against
  `crates/groupware/src/calendar/seal/policy.rs`.
- Generated files (`crates/registry/src/schema/*`, `crates/trc/src/event/
  enums.rs`) carry no fork edits; take upstream's version outright.

Regression net after a merge, in this order: `cargo test -p vault`, `-p groupware`,
`-p common`, `-p http@1.0.3 --lib --bins --features test_mode,enterprise`; `za_tests` and
`za::cache_swap::cache_swap_tests`; `webdav_tests`, `imap_tests` and `jmap_tests` in
baseline and `ZA_KEY_ACCOUNTS=1` mode; the product build warning-free; then the CI dispatch
above. The key-account mode of the upstream suites is the real guard: it runs upstream's
own tests against sealed data.

## Sharing key calendars

Two setup-complete key accounts can share an owner calendar at `view` or `edit` through
`POST /api/vault/shares`, `/api/vault/shares/grant` and `/api/vault/shares/revoke`.
See [the sharing API](zero-access/calendar-sharing-api.md) for request bodies, pinning,
exact refusals, measurement conditions and the privacy boundary. The owner pins the grantee's
current public key before granting; a first-use pin is accepted, with out-of-band comparison
recommended when confirming it. Revoke by the stable grant id because aliases can move.
The first grant converts all historical events; later grants use the converted ring.
Revocation rotates future-write keys and does not reseal existing ciphertext.
Apple assisted discovery is on by default; Thunderbird can use a second home-set href with
it off. Pubsub must reach every node for prompt cache invalidation. Missed broadcasts can
leave resident keys until the existing hard cap, although current ACL/ring checks reject
fresh revoked access. Use disposable accounts for the manual checklist.

Release qualification runs the following commands sequentially; suites share listener ports.
The benchmark and native checklist are separate gates. The key-mode JMAP gate must actually
reach `za_variants::calendars`, including its seven calendar-method refusals and empty
availability assertion. `RUST_TEST_NOCAPTURE=1` can expose its existing progress marker while
retaining the exact Cargo arguments below.

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt --all --check
cargo test -p vault -p groupware -p common
cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav::webdav_tests
STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav::webdav_tests
STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests jmap::jmap_tests
cargo build --release -p stalwart --no-default-features --features rocks
```

The [plan 14 outcome](superpowers/plans/2026-10-23-zero-access-plan14-outcome.md) records actual automated
results, temporary measurement instrumentation, limitations and pending native/review gates.
No passing automated suite establishes native client behavior, SQL or multi-node execution.

Release-4 merge dependencies: preserve the strict carrier/domain codecs and complete ring
identity checks before resolver/cache use; every event/link physical write must retain its
fresh calendar/vault/Account/event fences before conversion or grant publication. Conversion
keeps O(N) ids and at most 100 payload snapshots, commits each event separately and counts
only acknowledged work. Endpoint invalidation/boundary completion follows acknowledged writes.
Recheck `crates/common/src/cache/principals.rs` quiet catch-all propagation;
`crates/groupware/src/scheduling/event_cancel.rs`'s `itip_cancel_from_snapshot` seam;
`calendar/delegation.rs`'s pre-finalize owner-only second run and `za_finalize`;
`crates/dav/src/calendar/update.rs`'s two finalize branches and retained writer/owner identity;
DAV URI admission, discovery, sync/rename boundaries and all event writers; and
`crates/http/src/api/{vault,shares}.rs` dispatch, authentication, guards and mutation ordering.
Upstream changes to snapshot/finalize, cancel payloads or raw NothingToSend/empty distinctions
need the owner/delegated lifecycle differential suite and transparent mutation qualification.
