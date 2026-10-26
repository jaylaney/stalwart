# Building and testing the zero-access fork

See the [release policy](../../releases.md) for independent versioning, tags, upstream
provenance, and the release checklist. Record fork release changes in the
[fork changelog](../../../CHANGELOG-fork.md); the root `CHANGELOG.md` remains upstream history.

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
- rustfmt: `cargo fmt -p <crate> -- --check` works for most crates; for http use `cargo fmt --manifest-path crates/http/Cargo.toml -- --check` (cargo rejects `-p http@0.16.26` for fmt).
- Never open upstream issues or PRs from this fork (see AGENTS.md).
- Zero-access account API tests: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests`
- CalDAV suite against key accounts: `STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav_tests`. In that mode the `acl`, `cal_alarm`, `cal_scheduling` and `copy_move` modules run the four key-mode variants in `tests/src/webdav/za_variants.rs` (`acl`, `alarm`, `scheduling`, `copy_move`) instead of skipping.
- Key cache tuning (environment): `ZA_KEY_IDLE_SECS` (900), `ZA_KEY_MAX_AGE_SECS` (3600), `ZA_KEY_MAX_ENTRIES` (10000).
- Setup tokens expire after 7 days. The admin permission for `setup-token` is `sysAccountUpdate`.
- Account page origin (environment): `ZA_ACCOUNT_PAGE_ORIGIN`, e.g. `https://account.example.com`. Read once at startup; it is the only origin CORS allows on `/api/vault/*`. Unset, empty or not a valid header value leaves the vault API without CORS headers. Other responses keep the operator's own `Access-Control-Allow-Origin` (or permissive CORS) unchanged.

## Operating zero-access accounts

- Provisioning: an administrator with `sysAccountUpdate` calls `POST /api/vault/setup-token` with `{ "account": "<address>" }` for a user account that has no password credential and no calendar data; the response carries a one-time token (7 days). The user then calls `POST /api/vault/setup` with `{ "username", "token", "password" }` and receives the recovery key, shown once. No administrator ever chooses or sees the password.
- Marker: a key account's registry password credential holds the literal `$za$` instead of a hash. It classifies the account; the real verifier and key wraps live in the vault record. Credential edits through the registry API are refused for key accounts; password, recovery key, app passwords and TOTP are managed only through `/api/vault/*`.
- Corrupt or missing vault record: a key account whose marker has no usable record cannot log in (the refusal names `zero-access vault record missing` in the log). This only arises from data loss; recovery is operator intervention (restore the record from backup). There is no in-band reset: the server cannot recreate the keys.
- Key cache: unlocked keys are held per node in process memory only, never shared across a cluster or written to disk. Each node applies `ZA_KEY_IDLE_SECS` (sliding idle timeout, 900), `ZA_KEY_MAX_AGE_SECS` (hard cap from login, 3600) and `ZA_KEY_MAX_ENTRIES` (LRU bound, 10000), swept every 30 seconds; a node logs the effective values at startup. `SessionKeys` are attached by the HTTP layer for one request, and by the IMAP and POP3 login handlers for the life of the authenticated connection (mail spec 3.3; mail logins do not use the key cache: each derives afresh and the keys live in the connection's own state); WebSocket, EventSource, ManageSieve and SMTP sessions never hold them.
- Test builds (`test_mode` feature) use weakened Argon2 parameters for new passwords and log a warning at startup; never deploy them.

## Merging upstream

The fork tracks Stalwart's tagged releases (base 0.16.26). Merge a release
tag, not the tip of `main`, about once per upstream minor version, with
`git merge upstream/vX.Y.Z`; never rebase the published branch. Treat each
merge as a small plan: scan what upstream changed in the hotspot files, merge,
run the full net below, and have the conflict resolutions reviewed.

Merge hotspots (fork insertions inside upstream files; everything else lives
in fork-owned modules and merges cleanly):

- `crates/common/src/auth/authentication.rs` (marker diversion in
  `route_auth_request`) and `crates/http/src/auth/authenticate.rs`
  (keyed-fingerprint auth cache, restructured).
- `crates/dav/src/common/uri.rs` (gate call), `crates/dav/src/request.rs`
  (calendar REPORT prefix check), `crates/dav/src/common/propfind.rs` (the
  loader's `za_archive_view` call), `crates/dav/src/calendar/{update,get,
  freebusy,mkcol,proppatch,copy_move}.rs` (seal/unseal insertions).
- `crates/jmap/src/registry/mapping/{principal,account}.rs` (credential
  edit refusals).
- The 24 non-enterprise warnings in `store`, `common`, `jmap` and `services`
  are upstream's; silence them when those files are touched by a merge, not
  before.

Storage checks at every merge (the fork never changes a stored struct's
layout, so upstream migrations keep working; ciphertext rides in existing
fields and moves with them):

- calcard version: sealed bundles embed calcard's rkyv layout for
  `ICalendarEntry` and `ICalendarParameter`, and `X-ZA-EXTRA` embeds
  `types::DeadProperty`. If a merge bumps calcard (pinned at 0.3.x in
  `crates/groupware/Cargo.toml`) or changes `DeadProperty`, old bundles need a
  versioned reader keyed on the bundle format byte before the binary ships;
  the user's key is available at login, so lazy re-sealing is possible.
- Numeric collisions: `PrincipalField::ZeroAccessVault = 150`
  (`crates/types/src/field.rs`) and `ACCOUNT_IS_KEY_ACCOUNT = 1 << 9`
  (`crates/common/src/auth/mod.rs`) sit in gaps upstream has not used; confirm
  the merge did not take either. `VanishedCollection::CalendarIdentity = 250`
  (`crates/types/src/collection.rs`) separately reserves a log-collection byte;
  preserve existing 251–254 and check this reservation at the same merge gate.
- Visibility policy: upstream migrations that recompute derived data (time
  ranges, alarms, the UID index) read only properties the policy keeps
  visible; a merge that adds a new derived value must be checked against
  `crates/groupware/src/calendar/seal/policy.rs`.
- Generated files (`crates/registry/src/schema/*`, `crates/trc/src/event/
  enums.rs`) carry no fork edits; take upstream's version outright.

Regression net after a merge, in this order: `cargo test -p vault`,
`-p groupware`, `-p common`, `-p http@0.16.26 --features test_mode`;
`za_tests`; `webdav_tests` in baseline and `ZA_KEY_ACCOUNTS=1` mode; the
product build warning-free in `vault`, `groupware`, `dav` and `http`. The
key-account mode of the upstream suite is the real guard: it runs upstream's
own tests against sealed data.

## Sharing key calendars

Two setup-complete key accounts can share an owner calendar at `view` or `edit` through
`POST /api/vault/shares`, `/api/vault/shares/grant` and `/api/vault/shares/revoke`.
See [the sharing API](../../zero-access/calendar-sharing-api.md) for request bodies, pinning,
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
cargo test -p http@0.16.26 --features test_mode
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav::webdav_tests
STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav::webdav_tests
STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests jmap::jmap_tests
cargo build --release -p stalwart --no-default-features --features rocks
```

The [plan 14 outcome](2026-10-23-zero-access-plan14-outcome.md) records actual automated
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
