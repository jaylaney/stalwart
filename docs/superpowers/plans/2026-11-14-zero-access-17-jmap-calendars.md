# Zero-access plan 17: JMAP calendars and browser sign-in (release 6)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A key account's calendar works from an unmodified JMAP calendar client in a browser: an OAuth token issued through the authorization-code flow opens the account's keys on every HTTP request that presents it, and the JMAP calendar methods read and write through the same key-account pipeline as CalDAV.

**Architecture:** The fork's key-account calendar steps move from the `dav` crate into `groupware::calendar::pipeline` with no change of behaviour (group A, proved by the existing CalDAV tests), gain the paths only JMAP uses (group B), and are called by JMAP key readers and writers that sit as child modules of upstream's handlers and call upstream's own validation in upstream's order (group D). Sign-in (group C): every token string of a key account's grant carries a 32-byte secret after a `~`, a per-account token record (principal field 153) holds the master key wrapped under keys derived from those secrets, and the HTTP layer's Bearer arm opens it. Group E closes the release: the differential oracle, upstream's JMAP calendar tests in key mode, leak coverage, the gates, the mutation sample, the documentation, the final test matrix and one whole-branch review.

**Tech Stack:** Rust 2024 on upstream Stalwart v1.0.3 with release 7 (fork `0.4.0-beta.1`; internal crates `1.0.3`, `vault` `0.16.25`); `vault::keys` (HKDF sub-keys, `wrap_key`, XChaCha20-Poly1305) and rkyv for the token record; upstream's OAuth code (`seal_token`, `rotate_oauth_grant`, the in-memory store's code entries); upstream's JSCalendar converter in `calcard` and its JMAP calendar handlers; release 7's in-memory content part (`store::za_search::content::ContentPart`); the `tests` crate harness (`TestServer`, `DummyWebDavClient`, the JMAP helpers of `tests/src/utils/jmap.rs`, the SMTP sink, `HookParked`, `DrainOn`).

**Spec:** [`docs/superpowers/specs/2026-11-13-zero-access-jmap-calendars-design.md`](../specs/2026-11-13-zero-access-jmap-calendars-design.md), revision 6 (decisions D1 to D29, invariants 57 to 76). Executors read the spec with the plan: a task names the sections it implements, and where the plan and the spec disagree the spec wins. Every name a task uses is fixed by the interface sheet; a task cites its sections and copies no signature. A name that turns out wrong is reported, not renamed.

**Plan files:** the plan is this file, the interface sheet [`2026-11-14-zero-access-17-jmap-calendars-interfaces.md`](2026-11-14-zero-access-17-jmap-calendars-interfaces.md) ("the sheet", cited as I §n) and six task files `2026-11-14-zero-access-17-jmap-calendars-tasks-{a,b,c,d1,d2,e}.md`, all beside it in `docs/superpowers/plans/`. [`-tasks-a`](2026-11-14-zero-access-17-jmap-calendars-tasks-a.md) holds Tasks 1 to 9, [`-tasks-b`](2026-11-14-zero-access-17-jmap-calendars-tasks-b.md) 10 to 12, [`-tasks-c`](2026-11-14-zero-access-17-jmap-calendars-tasks-c.md) 13 to 19, [`-tasks-d1`](2026-11-14-zero-access-17-jmap-calendars-tasks-d1.md) 20 to 27, [`-tasks-d2`](2026-11-14-zero-access-17-jmap-calendars-tasks-d2.md) 28 to 37, [`-tasks-e`](2026-11-14-zero-access-17-jmap-calendars-tasks-e.md) 38 to 44.

## Global Constraints

### Commands

Every shell command that uses cargo starts with `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`. One cargo command runs at a time, and none while a manually started local server is under test. A za suite run starts only while the host's one-minute load is below 20 (`uptime`). A task names a command by its label.

| Label | Command | Passing |
|---|---|---|
| SHIP | the block below | prints `clean`, exit 0 |
| TM | the block below with `cargo check -p tests --tests`, the log `za-test.log` and `\|tests/src/` added to the second pattern | prints `clean` |
| FMT | `cargo fmt -p <each crate the task touched> -- --check`; for the http crate `cargo fmt --manifest-path crates/http/Cargo.toml -- --check` | no output |
| U-vault, U-groupware | `cargo test -p vault -p groupware --features vault/test_mode <name> -- --nocapture` | the named tests pass |
| U-common | `cargo test -p common --lib <name>` | the same |
| U-http | `cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise <name>` | the same |
| U-ws | `cargo test --workspace --exclude tests --exclude scim --exclude scim-proto --lib --bins --features "$UNIT_TEST_FEATURES" -- <name>`, with `UNIT_TEST_FEATURES` set to the value at `.github/workflows/test.yml:51-67`. For `jmap` and `dav` unit tests. | the same |
| ZA | `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture` | `test result: ok. 1 passed` |
| ZA-FS | ZA with `BLOB_STORE=FileSystem` | the same |
| SWAP | `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::cache_swap::cache_swap_tests -- --exact --nocapture`, alone and never with `BLOB_STORE` | the same |
| WP, WK | `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav::webdav_tests -- --nocapture`; WK adds `ZA_KEY_ACCOUNTS=1` | the same |
| JP, JK | the same for `jmap::jmap_tests` | `test jmap::jmap_tests ... ok` |
| IP, IK | the same for `imap::imap_tests`; IK adds `ZA_KEY_ACCOUNTS=1` | `test result: ok. 1 passed` |
| SYS | `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests system_tests -- --nocapture` | the same |
| PRODUCT | `CARGO_PROFILE_RELEASE_LTO=thin CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo build --release -p stalwart --no-default-features --features rocks`. A local test binary; never the `enterprise` feature. Task 44 builds with the full profile and `--locked`. | exit 0, no warning in a fork file |
| FRESH | `python3 -B tests/product/fresh_store.py --binary target/release/stalwart` | exit 0 after nine readback checks |
| CALDAV | the second block below, with `--mode key` or `--mode plain`, on the binary PRODUCT built. The client is fetched at its pinned commit into a new, empty directory outside the repository. | the harness's unit suite passes; the runner exits 0 with the mode's recorded entries and nothing else (key mode: 1 skipped, 2 xfailed; plain mode: its five records, `docs/zero-access/client-caldav.md`) |

```bash
log="${TMPDIR:-/tmp}/za-check.log"
if cargo check -p stalwart --no-default-features --features rocks > "$log" 2>&1; then
  grep -E "^(warning|error)" -A3 "$log" | grep -E "crates/(groupware|dav|jmap|vault|types|services)/|crates/common/src/(za|auth)/|crates/http/src/auth/" || echo clean
else
  tail -40 "$log"; echo BUILD FAILED; false
fi
```

```bash
work="$(mktemp -d)"
git clone --no-checkout https://github.com/python-caldav/caldav.git "$work/python-caldav"
git -C "$work/python-caldav" checkout --detach 111ae14b875f40d100d5259820f10210f00e20f9
python3.12 -m venv "$work/venv"
"$work/venv/bin/python" -m pip install -r tests/caldav_client/requirements.txt
"$work/venv/bin/python" -m pip install --no-deps --no-build-isolation "$work/python-caldav"
"$work/venv/bin/python" -m unittest discover -s tests/caldav_client -p test_runner.py -v
"$work/venv/bin/python" tests/caldav_client/runner.py --binary target/release/stalwart --caldav-source "$work/python-caldav" --mode key
```

SHIP, TM and FMT run at the end of every task that changes a file. A task does not repeat them as steps; its last step before the commit is "the closing checks".

### Code rules

- Code under `cfg(feature = "enterprise")` and the `scim` crate are excluded from the product; an enterprise-licensed snippet is never moved or copied into another file.
- No stored struct changes layout; the token record is a new rkyv record of the fork's own, and the sealed personal bundle keeps its layout (D14).
- `crates/registry/src/schema/*` and `crates/trc/src/event/enums.rs` are generated and never edited; the release adds no setting, permission or event variant.
- An ordinary account takes upstream's code path unchanged, with two exceptions that apply to every account: `decode_access_token` strips a `~` suffix from every token that begins with the server's token header before it opens it (spec 4.2, D26), and a successful answer of the token endpoint is untraced (D24).
- Key material, a token secret, a hand-off secret, an authorization code, a secret's tag and a token string never reach a log, a trace or an error; traces of the new paths carry account, grant and document ids, counts and fixed reasons only (spec 8).
- No type that holds a wrap, a secret, MK or a plaintext tree derives `Debug`. A set error or a trace carries ids, counts and fixed texts.
- A fork insertion in an upstream file is one call or one short branch. In an upstream handler it is one `mod` line and one branch at the handler's head, or one call. A key writer is a child module that calls upstream's private helpers in upstream's order.
- An upstream validation step is neither copied into `groupware` nor moved out of its handler; a fork step of a key account's calendar write exists once, in the pipeline (invariant 68).
- For a key account no JMAP path calls upstream's event or calendar writers, `ItipMessages::queue` or `notify_direct_change` (invariant 57).
- A moved function keeps its body, its trace strings, its hook order, its lease releases and its mail-guard settlements. The only changes are the ones its task lists. The reviewer compares it with the base line by line.
- A mapping from one error type to another emits no trace. Producers trace where they do today.
- A hook compiles only under `test_mode` and changes nothing while unset.
- A view is plaintext in process memory for one request. It is never written back, never an update's current value, never an assertion, never cached past the request.
- A pipeline call is awaited to its end: no timeout and no `select!` around a call that holds a mail guard or a UID lease.
- In a batch every assertion precedes every set and clear.
- `groupware` gains no dependency.
- A test-only task edits no product code. A case that fails is a finding for the task that owns the path.
- A kill check edits product code, runs the named test, and is reverted with `git checkout`; `git status --short crates/` then prints nothing.
- Every source file starts with the SPDX header, copied from a neighbour.
- Build output stays warning-free in every file the plan creates or touches.
- Each task compiles and passes alone.

### Spec values, verbatim

- The token separator is `~`; a secret is 32 random bytes written as 43 characters of URL-safe base64 without padding; a code or consent token is upstream's 40 characters followed by the 43 of its secret, with no separator.
- A Bearer value that does not begin with the server's token header is not split, whatever characters it holds (D26). A token the fork refuses is traced as an authentication failure and is not counted for banning (D27).
- `PrincipalField::ZeroAccessTokens = 153`; the record holds at most 32 grants, and per grant at most two refresh entries and four access entries.
- HKDF labels: `za/v1/token/wrap`, `za/v1/token/tag`, `za/v1/token/next`, `za/v1/handoff`; a tag is 8 bytes.
- The keyless calendar description is `Calendars in this account are encrypted at rest and are not available to this session.`
- A store error on one object of a `/set` answers that object and every later one with the set error `forbidden` and `The server could not complete this change, try again.`; the method does not fail (D28).
- A write of the token record that stores a wrap asserts the record's previous bytes and the vault record's bytes; a write that only removes entries asserts the record alone; a writer retries a lost assertion at most three times and never under a changed binding.

### Suite rules

- An integration suite is one test function, selected by filter. A sub-module cannot be selected: the whole suite must pass, and a failing run stops at the first panic after the module's banner line.
- A rerun that does not count as a failure: `webdav_tests` once for upstream's `cal_itip`; plain `jmap_tests` once for `calendar::notification`; a za run that fails only on a helper timeout while the load was above 20, once, both results recorded.
- `DummyWebDavClient` times out at 500 ms. A request that waits on a pause or that mails goes through an in-process call or a five-second client.
- The za suite skips every drain; a case that needs one holds `DrainOn` for its own body. `wait_for_queue` after a write that mails, never `wait_for_delivery`.
- New za modules, in call order after `sharing_races::test` and before `sweep::test`: `tokens`, then `jmap_calendar::{pipeline, events, calendars, query, scheduling, sharing, refusals, oracle}`. `leak::test_jmap_calendars` runs after `leak::test_imip`.
- Fixtures, all at `example.com`, password `STRONG`: the families `token-*` and `jmapcal-*`. A `jmap_calendar` module's accounts are `jmapcal-<module>` and `jmapcal-<module>-2`, both key accounts; a third key account takes a role suffix (`-reader`, `-noperm`); an ordinary account takes `-plain`. Outside parties are sink addresses `jmapcal-<module>-<role>@remote.test`. OAuth client ids are `za-token-*` and `za-jmapcal-*`. Deliberate authentication failures use `X-Forwarded-For` addresses in `10.0.60.x`.
- `jmapcal-scheduling-nobody@example.com` is a local address with no entry, not an account.
- A za module deletes what it creates (events, calendars, inbox items, grants, OAuth clients, ordinary accounts), clears each hook at the end of the step that set it, leaves no UID lease and ends with `test.wait_for_tasks()`. Its key accounts are registered with `test.insert_account`; the suite destroys them.
- The inventory gate retains these functions by file and name. Each is edited in place, never renamed, moved or deleted, and still runs in its step: `za_variants::{calendars, principal_get, assert_keyless, assert_keyless_result}` (key-mode JMAP); `gating::{test, test_scheduling, method_error, dav_header, members}`, `websocket::{test, calendar_get, first_response}`, `app_password::test`, `leak::{scan, token_hits, check_event, check_calendar}` (the za suite).
- An existing assertion changes only where a task lists it, with a comment that cites the spec section.
- The CalDAV proof set is: the `dav` and `groupware` unit tests; ZA and ZA-FS; WK; the CalDAV client job in key mode; the fresh-store script, once, in Task 9.

### Repository rules

- No issue, pull request or report is opened or drafted against upstream `stalwartlabs/stalwart`.
- The plan, its commits and the outcome note are public: they carry no hostname, credential or private detail, and they describe inherited upstream behaviour as inherited, never as a bug.
- A commit message is the subject the task gives, a blank line, and the trailer lines of the executing session.

## Moving a refusal row

Tasks 21 to 24, 26, 28 and 30 each serve methods that existing tests assert as refused. The same three edits, each with a comment citing spec 6.2:

- `crates/jmap/src/api/za_calendar.rs`: the method's arm in `za_calendar_method` and its row in the unit test `calendar_method_classes` change together.
- `tests/src/za/gating.rs:361-432`: one loop over eighteen `(method, args)` rows asserts `accountNotSupportedByMethod`. Task 21 splits it in two loops inside `gating::test`. The first keeps the rows still refused. The second holds the served rows and asserts `!matches!(method_error(&response), Some("accountNotSupportedByMethod" | "forbidden"))`. A later task moves its rows from the first loop to the second and changes nothing else.
- `tests/src/jmap/za_variants.rs:3519-3558`: the same kind of loop inside `za_variants::calendars`. A task takes its rows out of the loop and asserts them served in a block before the loop, leaving the store empty for the closing `assert_is_empty`. Task 39 replaces the block and the loop.

`tests/src/za/websocket.rs:76-81` changes once, in Task 21, to the keyless text.

## Base and rebasing

- The plan is committed on `za-jmap-calendars-design`, which sits on `origin/main` at `cc8b50a9b`. Release 7 (pull request 84) merged into `main` on 2026-11-14 at `82b8984aa`, the commit the plan's line numbers were read at. The work branch `release6-jmap-calendars` is cut from the plan's branch.
- Line numbers in the plan and the sheet are at the commit that adds the plan. Between `82b8984aa` and `cc8b50a9b` one file differs, `tests/caldav_client/requirements.txt`, which no task cites by line.
- `main` is the base, and other work lands on it. The work branch is rebased onto `origin/main` at five points and nowhere else: before Task 1, after Task 9, after Task 19, after Task 37 and before Task 44. Never during a task, and never between a task's implementation and its review.
- A rebase is `git fetch origin main` then `git rebase origin/main`. If the base did not move, nothing is done. After a rebase that changed anything: SHIP, TM and one ZA run pass before the next task starts, and the ledger records the new base commit and the new commit of every finished task.
- Files that release 7 changed and this plan changes too, where a later change on `main` is most likely to conflict or to change meaning silently: `crates/jmap/src/api/request.rs`, `crates/jmap/src/changes/{get,mod}.rs`, `crates/jmap/Cargo.toml`, `crates/types/src/field.rs`, `crates/vault/src/keys.rs`, `crates/common/src/za/{mod,test_hooks}.rs`, `crates/common/src/auth/vault.rs`, `tests/src/za/{mod,leak,websocket}.rs`, `tests/src/utils/{za,cleanup,storage}.rs`, `tests/src/jmap/za_variants.rs`, `tests/qualification/dispositions.json`, `.github/workflows/test.yml`, `scripts/tests/ci_steps.py`, `CLAUDE.md`, `docs/README-dev.md` and `docs/zero-access/manual-checklist.md`.
- A change on the base to `crates/store/src/za_search/content.rs` or `crates/store/src/search/leaf.rs` needs a re-read of Task 25, which calls them without changing them.
- A conflict in a file of the list is resolved by the agent that owns the task whose lines conflict, and reviewed as that task's diff.
- The release's pull request targets `main`.

## Execution

- Subagent-driven development. The controller briefs, adjudicates and integrates; it writes no code.
- Every task, including tests, documentation and review fixes, is implemented by Sol (`gpt-6.1-sol`, `high`) and reviewed by a separate Astra agent (`gpt-6-astra`) that reads the diff against the task and the spec. Astra uses `high` for focused task reviews and `xhigh` for reviews of identity, authorization, persistence, ordering or concurrency contracts, following the spec's [implementation and review routing](../specs/2026-11-13-zero-access-jmap-calendars-design.md#implementation-and-review-routing).
- Each dispatch sets the model and effort explicitly. If a required route is unavailable, report the limitation; do not silently substitute another model or lower the effort.
- A reviewer of a round whose implementer ran the suites reads only: it runs no cargo command.
- An implementer is briefed with three things: this header, its task from the group file, and the sheet sections the task cites. The sheet is committed beside the plan. The implementer is also told where the spec and the other task files are, and reads them when its task names a section or a task. The brief adds one line, "deliver at the scope of the brief; don't widen it", and no instruction to verify or double-check.
- A reviewer gets the same three things, the diff and the spec sections the task names. Its brief carries no severity filter: every finding comes back with a confidence and a severity, and the controller filters.
- A task that finds the sheet wrong stops and reports. The controller rules (`Ruling:` in the ledger) and corrects the sheet in a commit of its own before the task goes on.
- Tasks run in number order.
- Group A changes no file under `tests/`; each of Tasks 2 to 8 ends with `git status --short tests/` printing nothing.
- The plan ends with one whole-branch review by a fresh Astra agent (`gpt-6-astra`, `xhigh`) with a clean context (Task 44), which the project owner authorized for this plan on 2026-11-14. A second whole-branch pass runs only on the project owner's word; this does not restrict the focused Astra reviews of tasks and fixes.
- The ledger is `.superpowers/sdd/2026-11-14-zero-access-17-jmap-calendars/progress.md`, git-ignored: one line per task with its commit, its review's verdict and each `Ruling:` the controller made.
- A mutation executor runs one test row per background command and waits for it; it never batches rows.

## The session rule

Decided by the project owner on 2026-11-14 (spec D19; section 12, decision 4): a key account's OAuth sessions end only when a credential changes, that is on a password change, a recovery or a second-factor change. They survive every other vault write: the first pin's rewrap, the self-check's repair, a new recovery key, and an app password's creation, publication, withdrawal or revocation.

- `za_session_binding(record)` (`crates/vault/src/record.rs`, I §5.1) is a 64-bit fingerprint of the record's `salt`, Argon2 parameters, `verifier_hash` and `totp_url`, never 0. Every use of the rule goes through it.
- The binding is not the authentication generation. The key cache's generation stays the vault revision: a vault write that leaves the binding unchanged still evicts resident keys, and the next request opens them again from the token record.
- D23 stands: a Bearer request whose keys were opened at a generation that is no longer current answers 401. After a vault write that left the binding unchanged, the client's refresh succeeds.

| Where | What it does | Tested by |
|---|---|---|
| `za_session_binding` (Task 13) | the fingerprint | `binding_changes_only_with_credentials` |
| A key account's credential version (`crates/common/src/auth/access_token.rs:172-184`, Task 14) | is the binding | `tokens::credential_changes_end_sessions`, `tokens::other_vault_writes_keep_sessions` |
| `vault_batch_write` (`crates/common/src/auth/vault.rs:197-239`, Task 14) | clears field 153 only when the binding changes | `credential_change_clears_the_token_record`, `record_write_loses_to_vault_write`, `tokens::pin_rewrap_keeps_session` |
| `za_token_issue`, `za_token_open_access` (Task 15) | compare bindings; a lost vault assertion under an unchanged binding is retried | `tokens::issue_refuses_changed_binding`, `tokens::open_access_needs_entry_and_binding` |
| The Bearer arm (Task 16) | D23 | `tokens::bearer_crossed_by_vault_write_is_refused` |
| The code exchange (Task 17) | a crossing credential change refuses; any other crossing vault write is retried | `tokens::exchange_crossed_by_password_change`, `tokens::exchange_crossed_by_recovery`, `tokens::exchange_crossed_by_other_vault_write` |
| The refresh (Task 18) | the same | `tokens::refresh_crossed_by_credential_change`, `tokens::refresh_crossed_by_other_vault_write` |
| A browser's first invitation (Task 31, review focus 1) | the session survives the pin's rewrap | `scheduling::first_invitation_and_the_session` |
| The mutation sample (Task 42) | rows J16 and J16b | |
| The documentation and the checklist row (Task 43) | state the rule | |

## Review Focus

Five conditions no task's tests would otherwise exercise, most likely to hurt a user first. Each is owned by the named test.

1. **The first invitation keeps the browser signed in.** Condition: a key account's first invitation to another key account, sent from a browser session. The invitation pins the invitee's key, and the pin's rewrap is a vault write. A person expects the invitation to be delivered and the browser to stay signed in: the same token reads the event, and its refresh is granted. Task 31, `scheduling::first_invitation_and_the_session`.
2. **A native client's dead properties across a browser move.** Input: an event a CalDAV client annotated with dead properties, moved in the browser from an unshared to a shared calendar. A person expects the native client to read its properties afterwards. Task 11, `pipeline::placement_reseals_event_container`.
3. **A grantee's default reminders land on everyone.** Input: `useDefaultAlerts: true` on a new event in a shared calendar, written by a grantee. A person expects to know what is stored and who is reminded: the grantee's own defaults by value, an email reminder naming the grantee, and, for a grantee that set none, the defaults its entry took from the owner's at the grant. Task 36, `sharing::grantee_default_alerts`.
4. **An edit in the browser drops what a native client wrote.** Input: an event with an `X-` property and alarm details, written over CalDAV and edited over JMAP. A person expects to lose no more than the same steps lose on an ordinary account, and to keep the token pairs, the display name and the withheld-instance record. Task 38, `oracle::round_trip_keeps_what_an_ordinary_account_keeps`.
5. **Search on a large calendar.** Input: one unexpanded text query without a time range over 300 events. A person typing in a search box expects an answer within the JMAP test client's five seconds. Task 26, `query::text_query_cost`, which records the count of opened events and the time for the outcome note.

## Task index

Group files: A [`-tasks-a.md`](2026-11-14-zero-access-17-jmap-calendars-tasks-a.md), B [`-tasks-b.md`](2026-11-14-zero-access-17-jmap-calendars-tasks-b.md), C [`-tasks-c.md`](2026-11-14-zero-access-17-jmap-calendars-tasks-c.md), D1 [`-tasks-d1.md`](2026-11-14-zero-access-17-jmap-calendars-tasks-d1.md), D2 [`-tasks-d2.md`](2026-11-14-zero-access-17-jmap-calendars-tasks-d2.md), E [`-tasks-e.md`](2026-11-14-zero-access-17-jmap-calendars-tasks-e.md). "After" names the tasks whose names or files a task uses, for a rebase or a re-run.

| # | Group | Task | Spec | After |
|---|---|---|---|---|
| 1 | A | The pipeline module and its neutral error | 5.1, 8, 11 | |
| 2 | A | Admission, roles, contexts, read helpers; `ZaSyncBoundary::refuses` | 5.1, 6.8 | 1 |
| 3 | A | The UID lease functions | 5.1, 5.2 | 1 |
| 4 | A | The scheduling assembly | 5.1, 5.4 | 2 |
| 5 | A | The create commit | 5.1, 5.3 | 3, 4 |
| 6 | A | Placement contexts, refusals, the event container's reseal | 5.1, 5.2, 3.1 | 2 |
| 7 | A | The update target and commit | 5.1, 5.4 | 4, 6 |
| 8 | A | The deletes | 5.1, 6.7 | 2 |
| 9 | A | The move's proof run, with the fresh-store script | 5.1 | 8 |
| 10 | B | A create in several calendars; the `jmap_calendar` test helpers | 5.2, 9.2 | 5 |
| 11 | B | Placement in an update | 5.2, 9.1, 9.2 | 7, 10 |
| 12 | B | The destroy of a whole event; `za_delete_send_itip` | 5.2, 5.4, 6.5 | 8, 11 |
| 13 | C | Vault primitives: secrets, the record, the hand-off value | 4.2 to 4.5, 9.1 | |
| 14 | C | Field 153, storage, `za_token_read`, the vault write's deletion | 4.3, 4.7 | 13, 10 |
| 15 | C | Issue, open, forget; the grant hook for key accounts | 4.3, 4.4, 4.6, 4.7 | 14 |
| 16 | C | The suffix strip, untraced answers, the Bearer arm | 4.2, 4.6, 4.8, 8 | 15 |
| 17 | C | The hand-off, the code exchange, the refresh precheck and its interim refusal | 4.4, 4.5 (D22), 4.9 | 16 |
| 18 | C | Refresh | 4.5 | 17 |
| 19 | C | The sweep; account destruction | 4.7 | 18 |
| 20 | D1 | The calendar refusal text and the admission table | 6.1, 6.2 | 2 |
| 21 | D1 | `CalendarEvent/get`, `Calendar/get`, `Calendar/query` | 6.1 to 6.3, 6.7 | 10, 16, 20 |
| 22 | D1 | `/changes` and the sync boundary | 6.8 | 21 |
| 23 | D1 | `ParticipantIdentity` | 6.9, 6.11 | 21 |
| 24 | D1 | `CalendarEvent/parse` | 6.6 | 23 |
| 25 | D1 | The event text index | 6.4, 9.1 | 1 |
| 26 | D1 | `CalendarEvent/query` and `/queryChanges` | 6.4, 6.8, 9.2 | 22, 25 |
| 27 | D1 | Colour on a shared calendar | 6.7 (D14) | |
| 28 | D2 | `Calendar/set` | 6.7, 5.3, 6.11, 8 | 12, 21, 27 |
| 29 | D2 | Default alerts by value | 6.5 (D11) | |
| 30 | D2 | `CalendarEvent/set` create | 3.1, 5.3, 5.4, 6.5, 8 | 10, 21, 29 |
| 31 | D2 | `CalendarEvent/set` update and scheduling | 3.1, 5.4, 6.5, 8 | 7, 17, 26, 30 |
| 32 | D2 | `CalendarEvent/set` placement | 5.2, 6.5 | 11, 28, 31 |
| 33 | D2 | `CalendarEvent/set` destroy | 6.5, 5.4 | 12, 31 |
| 34 | D2 | The drain hook | 5.5 | 31 |
| 35 | D2 | The session and principal objects | 6.10 | 20 |
| 36 | D2 | The `jmap_calendar::sharing` module | 6.1, 6.5, 6.7, 9.2 | 22, 28, 33, 35 |
| 37 | D2 | The `jmap_calendar::refusals` module | 6.1, 6.11, 9.2 | 24, 26, 28, 30 |
| 38 | E | The differential oracle | 3.5, 3.6, 9.3 | 34 |
| 39 | E | Upstream's JMAP calendar tests in key mode | 9.4 | 35 |
| 40 | E | Leak coverage | 9.5 | 19, 33 |
| 41 | E | The other harnesses and the gates | 9.6 | 40 |
| 42 | E | The mutation sample | 9.7 | 41 |
| 43 | E | Documentation | 7, 9.8, 10 to 12 | 42 |
| 44 | E | The final test matrix and the whole-branch review | 3.6, 9, 10 | 43 |

## File structure

New files are marked (new). A pipeline file holds functions moved out of the `dav` file named with it.

The plan, in `docs/superpowers/plans/`, each name after the prefix `2026-11-14-zero-access-17-jmap-calendars`:
- `.md` (new): this header and the task index.
- [`-interfaces.md`](2026-11-14-zero-access-17-jmap-calendars-interfaces.md) (new): the interface sheet, the one place a name or signature is fixed.
- [`-tasks-a.md`](2026-11-14-zero-access-17-jmap-calendars-tasks-a.md) (new): group A, the move, proved by the existing CalDAV tests.
- [`-tasks-b.md`](2026-11-14-zero-access-17-jmap-calendars-tasks-b.md) (new): group B, the pipeline paths only JMAP uses.
- [`-tasks-c.md`](2026-11-14-zero-access-17-jmap-calendars-tasks-c.md) (new): group C, tokens.
- [`-tasks-d1.md`](2026-11-14-zero-access-17-jmap-calendars-tasks-d1.md) (new): group D, the JMAP reads, queries and the admission table.
- [`-tasks-d2.md`](2026-11-14-zero-access-17-jmap-calendars-tasks-d2.md) (new): group D, the JMAP writes, scheduling, sharing and refusals.
- [`-tasks-e.md`](2026-11-14-zero-access-17-jmap-calendars-tasks-e.md) (new): group E, closing.

`crates/vault`:
- `src/tokens.rs` (new): the token record and its entries, the suffix split, reconciliation, eviction, refresh classification, the hand-off value.
- `src/keys.rs`: the token and hand-off key derivations and their associated data.
- `src/record.rs`: `za_session_binding`, the fingerprint of the credentials.
- `src/lib.rs`: the module declaration.
- `Cargo.toml`, and the workspace's `Cargo.lock`: the base64 crate.

`crates/types`:
- `src/field.rs`: `PrincipalField::ZeroAccessTokens = 153`.

`crates/common`:
- `src/za/tokens.rs` (new): the record's storage, issue, refresh, open, forget, the sweep, the hand-off's store entry, `za_split_bearer` (the one splitter of a Bearer value) and the refresh precheck.
- `src/za/calendar.rs` (new): the keyless calendar refusal.
- `src/za/mod.rs`: `ZA_CALENDAR_KEYLESS`, the module declarations.
- `src/za/test_hooks.rs`: the five token hooks; the table of hooks that now fire from `groupware`.
- `src/auth/vault.rs`: the record's deletion in `vault_batch_write`.
- `src/auth/access_token.rs`: a key account's credential version through `za_session_binding`.
- `src/auth/oauth/token.rs`: the suffix strip in `decode_access_token`.
- `src/auth/oauth/token_restored_tests.rs`: the unit tests of the strip.
- `src/auth/oauth/grant.rs`: the entry removal in `delete_oauth_grant`, for key accounts.

`crates/groupware`:
- `src/calendar/pipeline/mod.rs` (new): `ZaScheduling`, `ZaWritten`, `ZaDeleted`.
- `src/calendar/pipeline/error.rs` (new): `ZaPipelineError` and the rule for a scheduling error that fails a write.
- `src/calendar/pipeline/admission.rs` (new; from `dav/src/common/za.rs` and `dav/src/calendar/mod.rs`): the admission gate, the writer's keys, the roles.
- `src/calendar/pipeline/context.rs` (new; from `dav/src/common/za.rs`, `dav/src/calendar/mod.rs`, `dav/src/common/propfind/za.rs`): the read and write contexts and views.
- `src/calendar/pipeline/uid.rs` (new; from `dav/src/calendar/mod.rs`): the UID lease, its uniqueness check, the lost-create fence, the create's second commit.
- `src/calendar/pipeline/schedule.rs` (new; from `dav/src/calendar/update.rs`): the scheduling status and assembly, the second run, the delete's send condition.
- `src/calendar/pipeline/create.rs` (new; from `dav/src/calendar/update.rs`): the create commit.
- `src/calendar/pipeline/update.rs` (new; from `dav/src/calendar/update.rs`): the update target and commit, with placement.
- `src/calendar/pipeline/placement.rs` (new; from `dav/src/calendar/copy_move.rs`): the placement plan, its contexts and refusals, the event container's reseal.
- `src/calendar/pipeline/delete.rs` (new; from `dav/src/calendar/delete.rs`): the event delete, the whole-event destroy, the calendar delete.
- `src/calendar/za_alerts.rs` (new): default alerts applied by value (D11).
- `src/calendar/za_text.rs` (new): the in-memory text index of events (D12).
- `src/calendar/sharing/resolve.rs`: `ZaSyncBoundary::refuses` (D15).
- `src/calendar/seal/collection.rs`, `src/calendar/sharing/convert.rs`: colour in the personal bundle (D14).

`crates/dav`:
- `src/common/za.rs`, `src/calendar/{mod,update,delete,copy_move}.rs`, `src/common/propfind/{za,get}.rs`: key branches become calls into the pipeline; wrappers keep their names; `From<ZaPipelineError> for DavError`.
- `Cargo.toml`: `test_mode` enables `groupware/test_mode`.

`crates/http`:
- `src/auth/za_bearer.rs` (new): keys for a Bearer token that carries a secret.
- `src/auth/oauth/za_token.rs` (new): the code's hand-off, the keyed issue, the keyed refresh.
- `src/auth/authenticate.rs`: the Bearer arm and its refusal at a stale generation.
- `src/auth/oauth/auth.rs`: the keys of the password step, the hand-off at the code and the consent token.
- `src/auth/oauth/token.rs`: the code exchange, the refresh grant, the untraced granted answer.

`crates/jmap`:
- `src/api/za_calendar.rs` (new): the admission of calendar methods, the method table, the fixed JMAP texts.
- `src/calendar_event/za_read.rs` (new): the per-request reader of unsealed event views.
- `src/calendar_event/query/za.rs` (new): the text leaves of a key account's query.
- `src/calendar_event/set/za.rs` (new): the key writer of `CalendarEvent/set`.
- `src/calendar/set/za.rs` (new): the key writer of `Calendar/set`.
- `src/changes/za_calendar.rs` (new): the sync boundary of `/changes` and `/queryChanges`.
- `src/api/{request,session,mod}.rs`, `src/changes/{get,mod}.rs`, `src/calendar_event/{get,query,set,mod}.rs`, `src/calendar/{get,set}.rs`, `src/participant_identity/set.rs`, `src/principal/get.rs`: one branch or call each.
- `src/changes/query.rs`: the `/queryChanges` admission.
- `src/api/metadata/za.rs`: one unit test.
- `Cargo.toml`: `test_mode` enables `groupware/test_mode`.

`crates/services`:
- `src/task_manager/tasks/maintenance.rs`: the token sweep after the lease sweep.

`tests`:
- `src/utils/oauth.rs` (new): the OAuth client, sign-in, Bearer, token-record and hand-off helpers.
- `src/utils/mod.rs`: the module declaration.
- `src/za/tokens.rs` (new): the sign-in, refresh, end-of-session and sweep cases.
- `src/za/jmap_calendar/{mod,pipeline,events,calendars,query,scheduling,sharing,refusals,oracle}.rs` (new): the JMAP calendar modules and their shared helpers.
- `src/utils/{za,cleanup,storage}.rs`: `Keyless::call_using`, `jscalendar_of`, `normalize_jscal`, the field-153 exemption.
- `src/za/{mod,leak,websocket,app_password,sharing}.rs`, `src/za/imip/oracle.rs`, `src/jmap/{mod,za_variants}.rs`: the calls, the leak rules and the assertions the release changes.
- `src/za/gating.rs`: the assertions the release changes; `method_error` becomes `pub(crate)`.
- `src/za/dav_seal.rs`: three helpers' visibility.
- `src/za/sweep.rs`: it calls the lifted `run_purge_maintenance`.
- `qualification/dispositions.json`: the coverage cohort's additions.

Documentation and CI:
- `docs/zero-access/client-jmap.md` (new): the reference client's manual checklist.
- `docs/superpowers/plans/2026-11-14-zero-access-plan17-outcome.md` (new): the outcome note.
- `CLAUDE.md`, `docs/README-dev.md`, `docs/zero-access/manual-checklist.md`, `docs/zero-access/calendar-sharing-api.md`, `README.md`, `CHANGELOG-fork.md`: the passages Task 43 lists.
- `.github/workflows/test.yml`, `scripts/tests/ci_steps.py`: one comment, and one rerun only on measured evidence (Task 41).
