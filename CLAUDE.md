# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

This is a fork of the Stalwart mail server (Rust, edition 2024, Cargo workspace, base 0.16.25). Jay is building a zero-access (encrypted-at-rest) calendar product on branch `zero-access`. Read `docs/superpowers/plans/README-dev.md` for developer notes and `docs/superpowers/plans/2026-10-06-zero-access-plan1-outcome.md` before touching fork code.

## Commands

Every shell command that uses cargo must start with `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"` (Homebrew's keg-only rustup; Claude Code's shell is non-interactive, so the zshrc PATH does not apply).

- Product build (shipping configuration, never the `enterprise` feature): `cargo build --release -p stalwart --no-default-features --features rocks`. Add other store backends by name (`sqlite`, `postgres`, `mysql`, ...). `-p http` is ambiguous in this workspace; use `-p http@0.16.25`.
- Unit tests: `cargo test -p vault`, `cargo test -p common`, `cargo test -p http@0.16.25 --features test_mode`. `cargo test -p common --features test_mode` without `enterprise` fails to compile upstream (telemetry test data); the `tests` crate enables both.
- Integration tests live in the `tests` crate and need `STORE=RocksDb RUST_MIN_STACK=16777216`. Each suite is ONE test function that runs its sub-modules in order against one server; you select the suite by filter and cannot select a sub-module:
  - `cargo test -p tests za_tests -- --nocapture`: zero-access account API, caches, CORS, registry refusals.
  - `webdav_tests`: upstream CalDAV/CardDAV suite. `ZA_KEY_ACCOUNTS=1` runs it with every user provisioned as a key account.
  - Others: `system_tests`, `imap_tests`, `jmap_tests`, `smtp_tests`, etc.
  - Upstream's `cal_itip` sub-test of `webdav_tests` is timing-flaky (DTSTAMP index mismatch, `tests/src/webdav/cal_itip.rs`); rerun once before treating a failure there as a regression.
- rustfmt: `cargo fmt -p <crate> -- --check`; for the http crate cargo rejects `-p http@0.16.25` for fmt, so use `cargo fmt --manifest-path crates/http/Cargo.toml -- --check`. Workspace lints are strict; build output must stay warning-free for new code.
- Local manual run: scripts in the git-ignored `.run/` (`env.sh` shared settings). First boot is `start-recovery.sh` then `provision.sh`; afterwards `start.sh`. `--config` takes a JSON data-store file (`{"@type":"RocksDb","path":...}`); a missing file means web bootstrap mode. `STALWART_RECOVERY_ADMIN=user:pass` sets a fallback admin.

## Repository rules (AGENTS.md, CONTRIBUTING.md)

- Never open GitHub issues, pull requests or security reports against upstream `stalwartlabs/stalwart`, and do not draft them for the user. `origin` is Jay's fork (`jaylaney/stalwart`); `upstream` is Stalwart Labs; never push to upstream. Upstream bugs and questions go to support.stalw.art, by the human. Do not request CVEs or advisories.
- Upstream does not accept AI-generated code or pull requests from non-vouched contributors. This fork publishes under AGPL-3.0 and does not contribute back.
- Code under `cfg(feature = "enterprise")` and the whole `scim` crate are licensed only under the Stalwart Enterprise License and are excluded from the product build. The `tests` crate enables `enterprise` on `store`, `directory`, `coordinator` (and others) for upstream's own tests; leave that.

## Architecture

Crates (under `crates/`):
- `main`: the `stalwart` binary; `types`: shared ids and `PrincipalField`; `utils`: helpers and proc macros.
- `common`: the shared core (`Server`, `Inner`, `Core`, `Data`, `Caches`), config parsing, boot, authentication routing, cache invalidation.
- `store`: storage backends and `RegistryStore`; `registry`: registry object schema (generated) and pickling; `directory`: internal/external directories and `Credentials`; `coordinator`: cluster pub/sub.
- `http` (+ `http-proto`): hyper listener, routing, `/api/*` management API, auth layer; `jmap` (+ `jmap-proto`): JMAP and the registry mapping layer; `dav` (+ `dav-proto`): CalDAV/CardDAV/WebDAV handlers; `groupware`: calendar/contact/file resources, iTIP scheduling.
- `email`, `imap` (+ `imap-proto`), `pop3`, `managesieve`, `smtp`, `spam-filter`, `nlp`: mail stack. `services`: background tasks and broadcast subscriber. `migration`: data migrations (run before services start). `trc`: tracing events and metrics.

Startup: `main` calls `BootManager::init` (`common/src/manager/boot.rs`): config JSON path (`--config` or `CONFIG_PATH`) -> `RegistryStore::init` -> `Bootstrap` (inserts safe defaults) -> `Listeners` -> `Storage`/`Telemetry` -> `Core`, `Data`, `Caches` -> `Arc<Inner>`. `Server` is a cheap per-use view built with `inner.build_server()`. Then migrate, `start_services`, `start_queue_manager`, and `init.servers.spawn` starts one session manager per listener protocol (SMTP/LMTP, HTTP, IMAP, POP3, ManageSieve), plus the broadcast subscriber.

HTTP request flow: `crates/http/src/request.rs` routes on the first path segment: `/jmap`, `/dav`, `/.well-known`, `/auth` (OAuth), `/api`, `/scim`, `/form`, etc. Each arm authenticates itself via `authenticate_headers` (`http/src/auth/authenticate.rs`, which holds the fingerprinted auth cache) -> `Server::authenticate` -> `common::auth::authentication::route_auth_request` (shared by every protocol, including IMAP/POP3/SMTP/ManageSieve) -> `AccessToken`. Handlers then call `jmap`/`dav`/`groupware`/`email` -> `store`. `crates/http/src/api/mod.rs` dispatches `/api/*` (`auth`, `account`, `token`, `discover`, `schema`, `vault`, ...).

Registry: configuration and accounts are registry objects. The Rust types in `crates/registry/src/schema/*` and the event enums in `crates/trc/src/event/enums.rs` are GENERATED by a tool that is not in this repo ("auto-generated, do not edit"). Do not hand-edit them and do not add settings, permissions or event variants; the fork uses environment variables instead (`ZA_KEY_IDLE_SECS`, `ZA_KEY_MAX_AGE_SECS`, `ZA_KEY_MAX_ENTRIES`, `ZA_ACCOUNT_PAGE_ORIGIN`). Registry objects are edited through JMAP `x:<Type>/set` calls (see `tests/src/utils/registry.rs`).

Storage: `store` abstracts RocksDB, SQLite, FoundationDB, PostgreSQL and MySQL. Stored structs are rkyv archives read with unchecked zero-copy access (`rkyv::access_unchecked` in `store/src/write/serialize.rs`), so stored struct layouts must never change. Conditional writes use `AssertValue` (`store/src/write/assert.rs`). Per-account Principal properties are keyed by `PrincipalField` (`types/src/field.rs`).

Caches (`common::Caches`): account and access-token caches, `http_auth`, the zero-access key cache `za_keys`, DAV resource caches. `CacheInvalidation` (`common/src/ipc.rs`) is broadcast across cluster nodes. Cache loaders are fenced by `Caches::account_epoch`.

Test harness: `TestServer`/`TestServerBuilder` (`tests/src/utils/server.rs`), one server per suite on port 8899 with self-signed TLS. `Account` helpers in `tests/src/utils/account.rs` (`create_key_user_account` makes a zero-access account); fork helpers in `tests/src/utils/za.rs`. Every sub-module ends with `assert_is_empty`, which scans every store subspace (`tests/src/utils/cleanup.rs`); vault records are exempted there only while their account still exists, and the key-account suites destroy their key accounts at the end.

## Zero-access fork

Spec: `docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md` (revision 6, approved; binding for plans 2 and 3). Trust model: the operator cannot read data at rest or while the user is logged out; native CalDAV clients stay unmodified; the running server holds plaintext during a session.

Where the code lives:
- `crates/vault`: key primitives (`keys`), the vault record (`record`), recovery key, `SessionKeys` (`session`), the key cache (`cache`). `ZA_MARKER = "$za$"`.
- `crates/common/src/auth/vault.rs`: vault storage and verification (`PrincipalField::ZeroAccessVault = 150`; 46 is taken upstream). `crates/common/src/auth/authentication.rs`: the marker-credential diversion inside `route_auth_request`.
- `crates/http/src/auth/authenticate.rs`: keyed-fingerprint auth cache, generation fence, test-mode pause hooks (`za_test`).
- `crates/http/src/api/vault.rs`: the eight `/api/vault/*` endpoints (`setup-token`, `setup`, `password`, `recover`, `recovery-key`, `app-password`, `app-password/revoke`, `totp`) plus CORS helpers. Every vault write goes through `za_commit`; never call `za_vault_write` directly.
- `crates/jmap/src/registry/mapping/{principal,account}.rs`: refusals of registry credential edits for key accounts.
- Tests: `tests/src/za/`, helpers in `tests/src/utils/za.rs`.

Invariants (spec section 12):
- No stored struct changes layout; ciphertext rides in existing fields.
- The vault record is the sole authority for login. Every write is conditional on its revision (the authentication generation), verification reads it once, and no registry-only edit may change login outcome.
- Key material lives in `Zeroizing` buffers with a silent `Debug`, only in process memory, and never in logs, traces or errors. No cache retains a credential or a reversible form of one.
- Non-key accounts take unchanged upstream code paths.
- `Server::authenticate` returns keyless tokens on every protocol. `SessionKeys` are attached only by the HTTP layer and dropped with the request; WebSocket and EventSource handlers strip them. Background code never needs a key; if a path would, gate it.
- Sealing never adds, removes or reorders iCalendar components; visibility is an allowlist; the index builder only sees the stored sealed archive.
- Keep the fork diff narrow: new modules plus one-line call insertions at existing sites. Merge hotspots with upstream are `http/src/auth/authenticate.rs` and `common/src/auth/authentication.rs`.

Status: plan 1 (accounts) is done at `98883a42` and plan 2 (sealing) at `ac0bcdca`; read `docs/superpowers/plans/2026-10-06-zero-access-plan2-outcome.md` before plan 3 (gating, `...-3-gating.md`), which is next. The outcome notes list deferred findings, decisions awaiting Jay's confirmation (spec revision 6 candidates), and facts for plan 3. The key-account web page (product name Circulo) lives in a separate repository, `~/Development/circulo-account` (`jaylaney/circulo-account`); its endpoint contract is `docs/api.md` there.

## Working conventions

- Every source file starts with the SPDX header; copy it from a neighbour.
- Commit messages: subject, blank line, then the trailers the session's attribution reminder specifies.
- Superpowers plans and specs live under `docs/superpowers/`; execution ledgers under the git-ignored `.superpowers/sdd/`.
- Jay's global instructions require implementation to be delegated to subagents, with the main loop briefing and reviewing. Jay prefers to run system installs himself unless he says otherwise (he approved the Homebrew rustup install on 2026-10-06).
