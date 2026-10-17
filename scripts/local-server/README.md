# Local manual-test server

Scripts for running the fork's server on a developer machine and checking the zero-access behaviour by hand (Apple Calendar, `curl`, a data-store scan). They are tracked here so they travel with the repository.

The folder splits into two parts:

- **Tracked scripts**, in this folder (`scripts/local-server/`).
- **Per-machine state**, in the state directory `$ZA_RUN_DIR`, which defaults to `.run/` in the repository and is ignored by git: the RocksDB data store (`rocks.db`), `datastore.json`, `local.env` (secrets), `tls/` (certificate and key) and any logs you keep.

Nothing in the state directory is meant to be copied to another machine. The registry stores the absolute paths of the certificate files, so a state directory is tied to its location; on a new machine, start a fresh one with the steps below.

## First run on a new machine

All paths below are relative to the repository root.

1. Build the server binary, either the shipping build:

   ```
   cargo build --release -p stalwart --no-default-features --features rocks
   ```

   or the faster local build (thin LTO and 16 codegen units parallelise the release profile's single-threaded tail; the binary is slightly larger and slower, and switching between the two rebuilds from scratch):

   ```
   CARGO_PROFILE_RELEASE_LTO=thin CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo build --release -p stalwart --no-default-features --features rocks
   ```

   The scripts expect the binary at `target/release/stalwart` (override with `STALWART_BIN`).

2. Generate the self-signed TLS certificate, then trust it:

   ```
   scripts/local-server/tls-cert.sh
   ```

   Add `ZA_TLS_EXTRA_SAN="my-host.local,192.168.1.20"` to the command to put more names or addresses in the certificate (for a phone or another computer on the network). The script prints the trust command at the end; run it yourself:

   ```
   security add-trusted-cert -r trustRoot -k ~/Library/Keychains/login.keychain-db <path to cert.pem>
   ```

   The certificate is valid for 800 days; rerun `tls-cert.sh --force` (and trust the new one) when it expires.

3. Create `.run/local.env` (the directory exists after step 2) with the fallback admin, and keep it private (`chmod 600`):

   ```
   export STALWART_RECOVERY_ADMIN=admin:<password>
   ```

   Optional lines for the later steps: `export PLAIN_PASS=...` (`provision.sh` creates the control account with it in step 5, and `gatecheck3.sh` reads it), `export ALICE_PASS=...` (alice's password after her setup flow; `gatecheck3.sh`), `export SMOKE_PASS=...` (`smoke.sh`). `local.env` is sourced by `env.sh`, so it wins over the environment.

4. Start the server in recovery mode (first boot only). It stays in the foreground and listens on plain HTTP at `127.0.0.1:8080`:

   ```
   scripts/local-server/start-recovery.sh
   ```

5. In another terminal, provision it:

   ```
   scripts/local-server/provision.sh
   ```

   It creates the listeners, registers the certificate, and creates the domain and accounts listed below. Each result is printed.

6. Stop the recovery server (Ctrl-C in its terminal).

7. Start the server in normal mode, in the foreground:

   ```
   scripts/local-server/start.sh
   ```

   The server logs nothing in normal mode until a stdout tracer is configured in the registry. To keep a log, pipe the output, for example `scripts/local-server/start.sh 2>&1 | tee .run/server.log`.

8. In another terminal, run the end-to-end check:

   ```
   scripts/local-server/smoke.sh
   ```

Later runs need only `start.sh`.

## Listeners

| Name  | Port | Transport |
| ----- | ---- | --------- |
| http  | 8080 | plain     |
| https | 8443 | TLS (the self-signed certificate) |
| imap  | 1143 | plain     |

Apple Calendar refuses Basic authentication over plain HTTP: add the account with the TLS listener (`https://127.0.0.1:8443`, SSL on) and trust the certificate first (step 2).

If another program already holds `127.0.0.1:1143`, the IMAP listener on `[::]:1143` is still reachable through `::1`.

## Accounts

By address only; no passwords are kept in this folder.

- The fallback admin: the credentials in `STALWART_RECOVERY_ADMIN` in `local.env`.
- `alice@example.com`: created with no password, so it becomes a key (zero-access) account through the setup flow.
- `plain@example.com`: optional ordinary account, created only when `PLAIN_PASS` is set while `provision.sh` runs. It is the control account of the gating and leak checks.
- `smoke.sh` users: `bob@example.com` by default, or the local part given as its first argument. Each is created fresh, so a name can be used once per data store.

## Scripts

All scripts load `env.sh` first.

- `env.sh`: sourced by the others, not run. Resolves the repository root from its own location and sets the defaults below. It creates `$ZA_RUN_DIR` and, if missing, `datastore.json` (a RocksDB store at `$ZA_RUN_DIR/rocks.db`; an existing file is never touched). It sources `$ZA_RUN_DIR/local.env` when present and stops with a message if `STALWART_RECOVERY_ADMIN` is unset. It exports `STALWART_RECOVERY_ADMIN`, `ZA_ACCOUNT_PAGE_ORIGIN` (default `http://localhost:5173`, used only when unset) and `CURL_CA_BUNDLE` (`$ZA_RUN_DIR/tls/cert.pem`).
  - Reads: `ZA_RUN_DIR` (default `.run/` in the repository), `STALWART_BIN` (default `target/release/stalwart`), `BASE` (default `https://127.0.0.1:8443`), `ZA_ACCOUNT_PAGE_ORIGIN`, `STALWART_RECOVERY_ADMIN` (required).
- `tls-cert.sh [--force]`: writes a self-signed certificate and key to `$ZA_RUN_DIR/tls/` (`cert.pem`, `key.pem`, mode 0600) for `localhost`, `127.0.0.1`, `::1` and any extra names in `ZA_TLS_EXTRA_SAN` (comma-separated). Refuses to overwrite without `--force`, then prints the command that trusts the certificate. Does not load `env.sh`, so it works before `local.env` exists.
  - Reads: `ZA_RUN_DIR`, `ZA_TLS_EXTRA_SAN`.
- `start-recovery.sh`: first boot in recovery mode, one plain-HTTP listener on 8080, no listeners written. Runs from `$ZA_RUN_DIR`.
  - Reads: the `env.sh` variables.
- `provision.sh`: against the recovery server, through the registry JMAP API as the fallback admin, creates the `http`, `imap` and `https` listeners, a `Certificate` from `$ZA_RUN_DIR/tls/` set as the default certificate, the domain `example.com`, the user `alice` with no credentials, and, only when `PLAIN_PASS` is set, the user `plain` with that password. Refuses to run if the certificate files are missing.
  - Reads: the `env.sh` variables, `PLAIN_PASS` (optional).
- `start.sh`: normal mode, listeners from the registry. Runs from `$ZA_RUN_DIR`.
  - Reads: the `env.sh` variables.
- `smoke.sh [localpart] [password]`: end-to-end zero-access flow for a fresh user (setup token, setup, CalDAV, app password, password change, recovery key, refusal of registry credential edits). The password is the second argument, else `SMOKE_PASS`, else a random one. Passwords and recovery keys are never printed.
  - Reads: the `env.sh` variables, `SMOKE_PASS` (optional).
- `gatecheck.sh [address]`: tries to reach a key account's calendar as the admin and through the master-user path; expects 403/405 (default `alice@example.com`).
  - Reads: the `env.sh` variables.
- `gatecheck3.sh`: the plan 3 gating checks over `curl` (scheduling and sharing are off for key accounts, no JMAP calendars capability, invitations between a key and an ordinary account leave the inboxes empty). Check 6's propstat parsing is a text heuristic.
  - Reads: the `env.sh` variables, `ALICE_PASS` and `PLAIN_PASS` (both required).
- `leakcheck.sh <word>...`: greps `$ZA_RUN_DIR/rocks.db` for words that must not be at rest and reports how many data files contain each. Sealed content: 0. The `X-ZA-KEY` marker must appear once a key account has written, and a non-key account's event title (the positive control) must show up.
  - Reads: the `env.sh` variables.

## Notes

- Registry objects are edited with JMAP `x:<Type>/set` calls as the admin; the settings singletons are addressed by the id string `singleton`.
- RocksDB scans (`leakcheck.sh`) see only flushed files: wait a couple of seconds after a write before scanning.
- Rebuild the release binary after pulling code changes; the running server does not pick them up.
