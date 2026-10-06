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
