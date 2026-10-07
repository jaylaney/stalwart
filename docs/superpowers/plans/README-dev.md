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
- Never open upstream issues or PRs from this fork (see AGENTS.md).
- Zero-access account API tests: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests`
- Key cache tuning (environment): `ZA_KEY_IDLE_SECS` (900), `ZA_KEY_MAX_AGE_SECS` (3600), `ZA_KEY_MAX_ENTRIES` (10000).
- Setup tokens expire after 7 days. The admin permission for `setup-token` is `sysAccountUpdate`.
- Account page origin (environment): `ZA_ACCOUNT_PAGE_ORIGIN`, e.g. `https://account.example.com`. Read once at startup; it is the only origin CORS allows on `/api/vault/*`. Unset, empty or not a valid header value leaves the vault API without CORS headers. Other responses keep the operator's own `Access-Control-Allow-Origin` (or permissive CORS) unchanged.

## Operating zero-access accounts

- Provisioning: an administrator with `sysAccountUpdate` calls `POST /api/vault/setup-token` with `{ "account": "<address>" }` for a user account that has no password credential and no calendar data; the response carries a one-time token (7 days). The user then calls `POST /api/vault/setup` with `{ "username", "token", "password" }` and receives the recovery key, shown once. No administrator ever chooses or sees the password.
- Marker: a key account's registry password credential holds the literal `$za$` instead of a hash. It classifies the account; the real verifier and key wraps live in the vault record. Credential edits through the registry API are refused for key accounts; password, recovery key, app passwords and TOTP are managed only through `/api/vault/*`.
- Corrupt or missing vault record: a key account whose marker has no usable record cannot log in (the refusal names `zero-access vault record missing` in the log). This only arises from data loss; recovery is operator intervention (restore the record from backup). There is no in-band reset: the server cannot recreate the keys.
- Key cache: unlocked keys are held per node in process memory only, never shared across a cluster or written to disk. Each node applies `ZA_KEY_IDLE_SECS` (sliding idle timeout, 900), `ZA_KEY_MAX_AGE_SECS` (hard cap from login, 3600) and `ZA_KEY_MAX_ENTRIES` (LRU bound, 10000), swept every 30 seconds; a node logs the effective values at startup. Only HTTP requests carry keys; IMAP, POP3, ManageSieve, WebSocket and EventSource sessions never do.
- Test builds (`test_mode` feature) use weakened Argon2 parameters for new passwords and log a warning at startup; never deploy them.
