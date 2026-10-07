# Building and testing the zero-access fork

- Toolchain: stable Rust via rustup. Xcode command line tools for RocksDB.
- On this machine rustup is installed via Homebrew (keg-only): `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"` before using cargo. The curl installer in the plan is not needed.
- Build (tests): `STORE=RocksDb RUST_MIN_STACK=16777216 cargo build -p tests`
- Build (product, no enterprise code): `cargo build --release -p stalwart --no-default-features --features rocks`
- CalDAV suite (upstream, non-key accounts): `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests`
- CalDAV suite against key accounts (added in plan 1, task 11): `STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav_tests`
- Vault unit tests: `cargo test -p vault`
- Leak regression test (plan 3): `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests zero_access_leak`
- Upstream's `cal_itip` sub-test is timing-sensitive (DTSTAMP index mismatch when a second boundary falls between two iTIP operations); rerun once before treating a failure there as a regression.
- The za suite (`za::za_tests`) now ends with the leak regression test (`tests/src/za/leak.rs`). On failure it panics with a list of violation lines, each `<where>: <what>`: `subspace 'X' key [...]: raw value contains <canary>` (a plaintext canary string found in a store key, raw value or decoded blob), `missing <prop>` or sealed-tree complaints (an unsealed or wrongly shaped iCalendar tree), and `display_name`/`dead_properties`/event-preferences not empty (plaintext fields on a sealed row). The subspace letter and key locate the record.
- Full verification: `cargo test -p vault -p groupware -p common`, then `za::za_tests`, then `webdav::webdav_tests` in both modes.
- Manual client checklist: `docs/zero-access/manual-checklist.md`.
- Known limits (spec 2): the running server holds plaintext while serving a request and receives the password on every CalDAV request; the authentication cache and the key cache are process-local and bounded (15 minutes idle, 60 minutes hard cap).
- rustfmt: `cargo fmt -p <crate> -- --check` works for most crates; for http use `cargo fmt --manifest-path crates/http/Cargo.toml -- --check` (cargo rejects `-p http@0.16.25` for fmt).
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
- Key cache: unlocked keys are held per node in process memory only, never shared across a cluster or written to disk. Each node applies `ZA_KEY_IDLE_SECS` (sliding idle timeout, 900), `ZA_KEY_MAX_AGE_SECS` (hard cap from login, 3600) and `ZA_KEY_MAX_ENTRIES` (LRU bound, 10000), swept every 30 seconds; a node logs the effective values at startup. Only HTTP requests carry keys; IMAP, POP3, ManageSieve, WebSocket and EventSource sessions never do.
- Test builds (`test_mode` feature) use weakened Argon2 parameters for new passwords and log a warning at startup; never deploy them.

## Merging upstream

The fork tracks Stalwart's tagged releases (base 0.16.25). Merge a release
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
  the merge did not take either.
- Visibility policy: upstream migrations that recompute derived data (time
  ranges, alarms, the UID index) read only properties the policy keeps
  visible; a merge that adds a new derived value must be checked against
  `crates/groupware/src/calendar/seal/policy.rs`.
- Generated files (`crates/registry/src/schema/*`, `crates/trc/src/event/
  enums.rs`) carry no fork edits; take upstream's version outright.

Regression net after a merge, in this order: `cargo test -p vault`,
`-p groupware`, `-p common`, `-p http@0.16.25 --features test_mode`;
`za_tests`; `webdav_tests` in baseline and `ZA_KEY_ACCOUNTS=1` mode; the
product build warning-free in `vault`, `groupware`, `dav` and `http`. The
key-account mode of the upstream suite is the real guard: it runs upstream's
own tests against sealed data.
