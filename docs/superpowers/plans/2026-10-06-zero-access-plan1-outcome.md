# Zero-access calendar: plan 1 outcome and handoff to plans 2 and 3

Written 2026-10-06 at the end of the plan 1 execution session. Read this after
the handoff notes and before starting plan 2. Everything here is a decision
made during execution, a reviewer finding that was deliberately deferred, or
a fact the next plans need; none of it is derivable from the code alone.

## Where things stand

- Plan 1 (11 tasks) is complete on branch `zero-access` at `98883a42`
  (32 commits on top of the handoff commit `adcbcc7c`; 50 files,
  about 7.5k lines). Each task had an implementer, a task review, scoped
  re-reviews, and a whole-branch review with one fix wave; all clean.
- Suites green at the end: `cargo test -p vault` (25), `-p common` (101),
  `-p http@0.16.25 --features test_mode` (6), `za_tests` (integration suite
  for the account API and caches), `webdav_tests` in baseline and in
  `ZA_KEY_ACCOUNTS=1` mode, `system_tests`.
- Developer and operator notes: `README-dev.md` in this directory.
- Nothing has been pushed; the branch exists only on this machine.

## Deviations from the plan that later plans must know

- `za_vault_write(account_id, &record, previous: Option<&VaultRead>)`
  replaces the plan's `expected_cas: Option<u64>`; a write must strictly
  increase `revision`. Endpoints never call it directly: every write goes
  through `za_commit(server, account_id, &mut record, previous,
  expected_revision, now)` in `crates/http/src/api/vault.rs`, which fences
  on the generation verified by `za_verify_primary`, prunes orphan wraps,
  bumps the revision, does the CAS write (409 on a lost race) and
  invalidates caches.
- The stored vault value is one version byte followed by the rkyv archive;
  the reader rejects an unknown version before unarchiving. The property id
  is `PrincipalField::ZeroAccessVault = 150` (46 was reused by upstream
  for `IdentityAddresses` in 0.16.18).
- `Server::authenticate` returns keyless tokens on every protocol.
  `SessionKeys` are attached only by the HTTP layer
  (`authenticate_uncached` → `authenticate_with_keys`) and by
  `za_verify_primary`; the JMAP WebSocket upgrade and EventSource handlers
  strip them. Plan 2 must take keys from the per-request `AccessToken` on
  DAV paths only; plan 3 must fail closed on WebSocket/EventSource calendar
  access for key accounts.
- `http_auth` fingerprint entries live exactly as long as their key-cache
  entry: sweep, LRU eviction, expiry on lookup, `remove`, `remove_account`
  and `clear` all drop the matching fingerprint. The sweep runs every 30 s.
- Cache loaders (`try_account`, the access-token loader) are fenced by a
  global `Caches::account_epoch` because quick_cache 0.7's `remove` is a
  no-op on a pending placeholder (an in-flight load could republish a stale
  classification after an invalidation).
- The `Account` invalidation arm also drops cached HTTP authentication and
  resident keys for that account (so does `AccessToken`).
- Argon2 derivation runs behind a process-wide semaphore
  (`available_parallelism` permits).
- Argon2 parameters for new passwords come from `za_argon2_params()` in
  `common`; under `common`'s `test_mode` feature they are 1 MiB / 1 pass and
  a startup warning is logged. Production builds use 64 MiB / 3 passes.
- `setup-token` is authenticated by the standard header path (Basic or
  Bearer) with `Permission::SysAccountUpdate` and tenant scoping (a
  tenant-bound caller can only target its own tenant; cross-tenant → 404).
- `setup` on an active account returns 409 but still passes through the
  authentication-failure delay and fail2ban accounting.
- `/api/vault/*` request bodies are read without the `RequestBody` trace
  event and responses are binary bodies, so passwords, tokens, recovery
  keys and app passwords never reach traces.
- TOTP: enrolment or replacement requires `confirm` (a current code from
  the new secret); `otp_auth` absent → 400, `null` → removal, string →
  enrol/replace, longer than 1024 bytes → 400; `recover` also removes TOTP
  and reports `totp_removed`.
- `app-password/revoke` of an id with no wrap deletes a dangling registry
  credential and returns 200; 409 only when neither exists. Creation
  returns 409 if any wrap already exists under the next credential id
  (after pruning); ids are reused after the highest credential is deleted.
- Passwords that parse as app passwords are refused on `setup`, `password`
  and `recover`.
- CORS: `ZA_ACCOUNT_PAGE_ORIGIN` (unset = no CORS headers); the vault
  origin wins over permissive `*` on `/api/vault/*` responses only.
- Test-mode pause hooks (`crates/http/src/auth/authenticate.rs::za_test`):
  `set`/`Pause::new` (HTTP uncached login, before the cache insert),
  `set_endpoint` (inside `za_reread_verified`), `set_publish` (between
  app-password steps 1 and 2). All account-keyed and one-shot; harness
  helpers `park_login`, `park_endpoint`, `park_creation` in
  `tests/src/utils/za.rs`.
- The upstream directory sync (`synchronize_account`) never replaces a
  password credential holding the `$za$` marker; external-directory logins
  are refused for key accounts before the directory is contacted (Basic)
  or before the sync (Bearer).

## Decisions that are Jay's to confirm or reverse

1. `recover` clears TOTP (the recovery key is treated as the stronger
   factor). Alternative: keep TOTP and accept that a lost authenticator
   locks the account out of all management.
2. `recover` and `setup` do not go through `Server::authenticate`, so a
   disabled account or tenant can still rewrite its vault password.
3. Spec amendments to apply to revision 4 (the spec is the binding
   authority for plans 2 and 3): 4.1 `setup-token` auth is Basic or Bearer
   with `SysAccountUpdate`, no new permission; 10 add 402 for a missing
   TOTP code; 4.1 password-verified writes are additionally conditional on
   the verified generation (409 otherwise); 4.1 `revoke` is idempotent for
   dangling credentials; 4.1 the CORS origin is `ZA_ACCOUNT_PAGE_ORIGIN`;
   5 fingerprint residency is bound to the key entry; 5 the residual
   placeholder race (a waiter handed a stale account entry inside a
   pre-emption-sized window) is accepted for release 1; 4.1 `confirm` on
   `totp`; 4.1 `recover` removes TOTP (if decision 1 stands).
4. The account web page repository can start now: all eight endpoints
   exist with the request and response shapes in plan 1's Task 7 to 10
   text as amended above.

## Deferred findings (reviewed, not fixed), by area

Security/robustness, low impact:
- Timing oracle on vault state (PendingSetup/missing record skip Argon2;
  the 50 to 500 ms random delay blurs it).
- A key-account master user impersonating a non-key account caches a
  generation-0 entry under the master's password until it expires (plan 3
  should refuse or not cache master logins by key accounts).
- Username resolution differs between `setup`/`recover`
  (`account_id_from_email`) and the password endpoints (`authenticate`).
- Racing first `setup-token` issuance can return a dead token with 200
  (reissue recovers). A stale `PendingSetup` record after a lost marker
  race has no cleanup path. `setup` does not repeat the data check after up
  to seven days.
- App-password creation is blocked for up to an hour by a lingering fresh
  `Pending` wrap at the next id (liveness; the error text could say "retry
  later"). Error-after-write in step 3 can leave a Published wrap without a
  registry credential (cannot log in; revoke removes it).
- Responses produced before the `/api` arm (fail2ban 429, parse errors)
  carry no CORS headers. Permissive mode widens the vault preflight's
  Allow-Methods/Headers (origin stays pinned).
- `ZA_KEY_*` values are logged at startup but not clamped to the spec's
  bounds.
- Plain `String` copies of passwords, tokens and recovery keys in request
  and response structs are not zeroized (upstream does the same).

Tests worth adding later:
- A deterministic test for the cache-loader race (needs a test-mode pause
  inside `try_account`); a cluster "provisioning observed from a warmed
  node" test; tenant scoping; the active-409 fail2ban path; wrap-open
  ordering (wrong password on an unopenable wrap is Invalid, not NoRecord);
  `$za$` refusal over IMAP; several unit-test gaps listed in the plan 1
  ledger (vault key tests, cache boundaries, exemption failure path).

Structure:
- `crates/http/src/api/vault.rs` is about 1400 lines; split it into
  `api/vault/{mod,commit,setup,password,app_password,totp,cors}.rs` before
  plan 3 touches it. Four duplicate `trc::error!` blocks and two retry
  loops could share a helper.
- Merge hotspots with upstream: `crates/http/src/auth/authenticate.rs`
  (restructured) and `crates/common/src/auth/authentication.rs`.
  `ACCOUNT_IS_KEY_ACCOUNT = 1 << 9` will collide silently if upstream adds a
  flag.
- The plan 1 task text still says field id 46; it is historical.

## Facts for plans 2 and 3

- `AccessToken::za_keys_for(account_id)` returns the keys only when they
  belong to that account; `session_keys()` for the raw option.
- `KeyCacheConfig` has `DEFAULT_*` constants and `Default`;
  `KeyCache::contains_account` exists for tests.
- `vault::keys` exports the AAD purpose constants and `app_aad`.
- The za test suite enables `useXForwarded` and plain-text IMAP auth, uses
  dedicated forwarded IPs for deliberate failures (fail2ban counts per IP
  and per login name, 100/day), and a fresh key account per module.
- Upstream's `cal_itip` sub-test of `webdav_tests` is timing-flaky; rerun
  once before treating a failure there as a regression.
- Toolchain on this machine: Homebrew's keg-only rustup
  (`/opt/homebrew/opt/rustup/bin`), rustc 1.99 stable.
