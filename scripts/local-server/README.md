# Local manual-test server

Scripts for running the fork's server on a developer machine and checking the zero-access behaviour by hand (Apple Calendar, `curl`, a data-store scan). They are tracked here so they travel with the repository.

The folder splits into two parts:

- **Tracked scripts**, in this folder (`scripts/local-server/`).
- **Per-machine state**, in the state directory `$ZA_RUN_DIR`, which defaults to `.run-1.0/` in the repository and is ignored by git: the RocksDB data store (`rocks.db`), `datastore.json`, `local.env` (secrets), `tls/` (certificate and key) and any logs you keep.

Nothing in the state directory is meant to be copied to another machine. The registry stores the absolute paths of the certificate files, so a state directory is tied to its location; on a new machine, start a fresh one with the steps below.

## Moving from 0.16 to 1.0

The fork is now based on Stalwart 1.0.1. A 1.0 server cannot open a data store that 0.16 wrote, and there is no conversion: its schema check stops the startup ("The data store was created by a Stalwart version prior to 1.0"). So 1.0 needs a fresh state directory, and the scripts now default to `.run-1.0/` instead of 0.16's `.run/`.

- Leave an existing `.run/` alone. It holds 0.16 state that only a 0.16 binary can use. Don't point `ZA_RUN_DIR` at it with a 1.0 binary: the schema check runs only after the boot has opened the store.
- Run every step below again on the new directory: a new certificate, a new `local.env`, recovery boot, provisioning, setup flows. Trust the new certificate as well (step 2).
- 1.0 also stamps each store with its edition. The shipping build (no `enterprise` feature) writes the open-source edition's schema version, `0x80000007` (version 7 with the OSS flag, bit 31); a build with `enterprise`, such as the Rust test harness's, writes plain `7`. Neither edition opens the other's store ("created by the Stalwart ... edition and cannot be opened by this edition"), so use one build per state directory.

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

3. Create `.run-1.0/local.env` (the directory exists after step 2) with the fallback admin, and keep it private (`chmod 600`):

   ```
   export STALWART_RECOVERY_ADMIN=admin:<password>
   ```

   Optional lines for the later steps: `export ZA_HOSTNAME=...` (the default hostname `provision.sh` sets in step 5; default `mail.example.com`; the server wants a dotted name and refuses `localhost`), `export PLAIN_PASS=...` (`provision.sh` creates the control account with it in step 5, and `gatecheck3.sh` reads it), `export ALICE_PASS=...` (alice's password after her setup flow; `gatecheck3.sh`), `export SMOKE_PASS=...` (`smoke.sh`). `local.env` is sourced by `env.sh`, so it wins over the environment.

4. Start the server in recovery mode (first boot only). It stays in the foreground and listens on plain HTTP at `127.0.0.1:8080`:

   ```
   scripts/local-server/start-recovery.sh
   ```

5. In another terminal, provision it:

   ```
   scripts/local-server/provision.sh
   ```

   It creates the listeners, the domain, the certificate and the settings update (default certificate, default domain and default hostname, which a fresh store needs), then the accounts listed below. Each result is printed.

6. Stop the recovery server (Ctrl-C in its terminal).

7. Start the server in normal mode, in the foreground:

   ```
   scripts/local-server/start.sh
   ```

   The server logs nothing in normal mode until a stdout tracer is configured in the registry. To keep a log, pipe the output, for example `scripts/local-server/start.sh 2>&1 | tee .run-1.0/server.log`.

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
| imap  | 1143 | plain, with STARTTLS (the self-signed certificate) |

Apple Calendar refuses Basic authentication over plain HTTP: add the account with the TLS listener (`https://127.0.0.1:8443`, SSL on) and trust the certificate first (step 2).

IMAP refuses a login on a connection without TLS (it advertises `LOGINDISABLED`, since the `Imap` settings' `allowPlainTextAuth` stays off), so a client issues `STARTTLS` first. Before the 1.0 scripts the listener had no TLS at all, and no login was possible over it.

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
  - Reads: `ZA_RUN_DIR` (default `.run-1.0/` in the repository), `STALWART_BIN` (default `target/release/stalwart`), `BASE` (default `https://127.0.0.1:8443`), `ZA_ACCOUNT_PAGE_ORIGIN`, `STALWART_RECOVERY_ADMIN` (required).
- `tls-cert.sh [--force]`: writes a self-signed certificate and key to `$ZA_RUN_DIR/tls/` (`cert.pem`, `key.pem`, mode 0600) for `localhost`, `127.0.0.1`, `::1` and any extra names in `ZA_TLS_EXTRA_SAN` (comma-separated). Refuses to overwrite without `--force`, then prints the command that trusts the certificate. Does not load `env.sh`, so it works before `local.env` exists.
  - Reads: `ZA_RUN_DIR`, `ZA_TLS_EXTRA_SAN`.
- `start-recovery.sh`: first boot in recovery mode, one plain-HTTP listener on 8080, no listeners written. Runs from `$ZA_RUN_DIR`.
  - Reads: the `env.sh` variables.
- `provision.sh`: against the recovery server, through the registry JMAP API as the fallback admin, creates, in this order, the `http`, `imap` (STARTTLS) and `https` listeners, the domain `example.com`, a `Certificate` from `$ZA_RUN_DIR/tls/`, one update of the settings singleton (default certificate, default domain `example.com`, default hostname), the user `alice` with no credentials, and, only when `PLAIN_PASS` is set, the user `plain` with that password. The settings update is skipped with a message if the certificate or the domain could not be created. Refuses to run if the certificate files are missing.
  - Reads: the `env.sh` variables, `ZA_HOSTNAME` (optional; the default hostname, default `mail.example.com`), `PLAIN_PASS` (optional).
- `start.sh`: normal mode, listeners from the registry. Runs from `$ZA_RUN_DIR`.
  - Reads: the `env.sh` variables.
- `smoke.sh [localpart] [password]`: end-to-end zero-access flow for a fresh user (setup token, setup, CalDAV, app password, password change, recovery key, refusal of registry credential edits). The password is the second argument, else `SMOKE_PASS`, else a random one. Passwords and recovery keys are never printed.
  - Reads: the `env.sh` variables, `SMOKE_PASS` (optional).
- `gatecheck.sh [address]`: tries to reach a key account's calendar as the admin and through the master-user path; expects 403/405 (default `alice@example.com`).
  - Reads: the `env.sh` variables.
- `gatecheck3.sh`: the release 1 (plan 3) gating checks over `curl`. Its expectations are historical until the script is updated: plan 10 reopened the scheduling gates, so key accounts now get the auto-schedule `OPTIONS` header and the scheduling URLs on their principal, and checks 1 and 6 differ from the script's expectations by design; the invitation checks 8 and 9 predate release 2's invitations. Check 6's propstat parsing is a text heuristic.
  - Reads: the `env.sh` variables, `ALICE_PASS` and `PLAIN_PASS` (both required).
- `leakcheck.sh <word>...`: runs two checks over `$ZA_RUN_DIR/rocks.db`: the historical physical scan counts matching files in raw bytes and decoded RocksDB blocks/frames, while the logical scan counts current matching blob values after decoding. Sealed content must be absent; the `X-ZA-KEY` marker and an ordinary account's event title are positive controls. Stop the local server before scanning. The logical pass exits nonzero if a blob is encrypted or undecodable. It needs a compatible `ldb` executable and Python 3.14 or later (`compression.zstd`).
  - If no blob values are decoded, the script warns that column family `t` may be empty or the server may use external blob storage. The physical scan still reports its results.
  - Reads: the `env.sh` variables.

## Notes

- Registry objects are edited with JMAP `x:<Type>/set` calls as the admin; the settings singletons are addressed by the id string `singleton`.
- 1.0 renamed some registry fields that hand-written provisioning may use: `NetworkListener.tlsImplicit` is now `implicitTls`, and `Domain.isEnabled` is now `enable`. The old names are refused ("Invalid key for object").
- Stop the local server before running `leakcheck.sh`. Its physical scan includes recent writes in the RocksDB write-ahead log (`*.log`); the logical pass counts current decoded blob values.
- Rebuild the release binary after pulling code changes; the running server does not pick them up.
