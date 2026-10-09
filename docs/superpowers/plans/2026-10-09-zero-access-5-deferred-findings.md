# Zero-access plan 5: plan 3 deferred findings

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the plan 3 deferred findings that spec revision 7 left open
and that are worth closing now: one shared key-account lookup for every
gate, OPTIONS authentication errors reported, no trace on every key-account
PUT, a CI retry for the known flake, and three missing tests. Then record
what is closed and what stays deferred.

**Architecture:** One new method, `Server::za_is_key_account`, in the fork
module `crates/common/src/auth/vault.rs`; every key-account gate calls it
instead of its own inline lookup. Two one-line behaviour changes at existing
sites (`crates/http/src/request.rs` OPTIONS helper,
`crates/dav/src/calendar/update.rs` scheduling trace). Tests extend
`tests/src/za/tracing.rs`, `tests/src/za/leak.rs` and add what Task 3 names.
One workflow edit and one documentation commit.

**Tech Stack:** Rust (edition 2024), the `tests` crate
(`STORE=RocksDb RUST_MIN_STACK=16777216`), GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md`
(revision 7). Findings being closed:
`docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md`, sections
"Deferred findings (reviewed, not fixed)" and "Tests worth adding". Scope
approved by Jay on 2026-10-09.

**Read first:** CLAUDE.md at the repo root, then the plan 3 outcome note's
"Gate inventory" and "Deferred findings". Every cargo command needs
`export PATH="/opt/homebrew/opt/rustup/bin:$PATH"` first.

## Global Constraints

- No stored struct changes layout (spec invariant 1).
- Non-key accounts take unchanged upstream code paths (invariant 9). The one
  deliberate all-account change is Task 2's OPTIONS error report, which spec
  section 8 already covers ("a wrong credential there counts as a failed
  attempt").
- Key material and credentials never appear in logs, traces or errors
  (invariant 3, spec section 10).
- Fork diff stays narrow (invariant 10): edit existing sites; add no files
  except where a task names one.
- Every source file starts with the SPDX header; copy it from a neighbour.
- Build output must stay warning-free; run `cargo fmt -p <crate> -- --check`
  for each touched crate before each commit (for the http crate:
  `cargo fmt --manifest-path crates/http/Cargo.toml -- --check`).
- Commit messages: subject line, blank line, then exactly these two
  trailers:

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01LUr2fGPHVLze3HcjTtCFCC
  ```
- Product build never enables the `enterprise` feature.
- Upstream's `cal_itip` sub-test of `webdav_tests` is timing-flaky; rerun once
  before treating a failure there as a regression.
- Only one test server runs at a time (port 8899): never run two suites
  concurrently.

## Review Focus

1. A wrong credential on DAV OPTIONS: the response is still 200 with
   upstream's header (spec 8), the failure is reported as `auth.failed`, and
   the wrong secret never reaches a trace (Task 2 test).
2. A plain account's scheduling status still traces its reason after the
   key-account skip, on both the create and the update PUT path; only
   `KeyAccount` is silenced (Task 2 test: a plain account's event in the
   past takes the same `reason()` branch the fix edits, on both paths).
3. An index task naming a destroyed key account must leave the queue instead
   of retrying forever (Task 3 test A, with a timeout so a regression fails
   instead of hanging the suite).
4. The leak scanner must flag a calendar search-index entry of a key account
   and nothing else: not an email entry of that account, not a calendar
   entry of another account (Task 3 test B).
5. Both rule-expansion traces, for key and plain accounts, carry the UID and
   no iCalendar text (Task 4; the query-time trace is only reachable through
   a corrupted stored archive, which the test plants).

Not covered by a test, by ruling: an unknown account id at the two former
`account()` gates in Task 1. It is reachable: the ACL handler loads the
calendar archive before the account lookup, so an account deleted between
the two reaches the gate with an unknown id. Plan 3 ruling R3 accepts the
fail-open outcome there (upstream's path runs on data it cannot unseal, so
the cost is a confusing error, not a leak). No test opens that interleaving
deterministically; it stays unverified.

---

### Task 1: One key-account lookup for every gate

Closes the deferred finding "Account lookups: `acl.rs` and `scheduling.rs`
use `account()` where every other gate uses `try_account`; four inline gates
in JMAP and three in groupware could reuse dav's private `is_key_account`
helper."

`Server::account()` (`crates/common/src/cache/principals.rs:340`) errors on an
unknown account id and synthesises an account for the recovery admin;
`try_account()` (`:373`) returns `None` for both. Every gate except two uses
the `try_account` form, which is fail-open on unknown ids (plan 3 ruling R3:
upstream's outcome stands for an id that is not a key account). This task
adds one method with that exact behaviour and routes every gate through it.
Behaviour is unchanged except at the two `account()` sites, where an unknown
id now reaches upstream's path instead of an error.

**Files:**
- Modify: `crates/common/src/auth/vault.rs` (add the method to the first
  `impl Server` block, line 162)
- Modify: `crates/dav/src/common/za.rs:182-188` (helper body delegates)
- Modify: `crates/dav/src/common/acl.rs:130-138`
- Modify: `crates/dav/src/calendar/scheduling.rs:376-382`
- Modify: `crates/jmap/src/principal/availability.rs:110-117`
- Modify: `crates/jmap/src/blob/download.rs:117-124`
- Modify: `crates/jmap/src/api/request.rs:735-760` (`za_assert_calendar_allowed`, `za_jmap_untraced`)
- Modify: `crates/groupware/src/calendar/itip.rs:383-390` and `:668-675`
- Modify: `crates/services/src/task_manager/index.rs:493-500`
- Modify: `crates/http/src/request.rs:1047-1077` (`za_is_key_account_request`, `za_jmap_untraced`)

**Interfaces:**
- Produces: `Server::za_is_key_account(&self, account_id: u32) -> trc::Result<bool>`
  (in `common`), used by Task 2.
- Keeps: `crate::common::za::is_key_account(server: &Server, account_id: u32) -> crate::Result<bool>`
  in dav, same signature, now a delegate.

- [ ] **Step 1: Add the method.** In `crates/common/src/auth/vault.rs`, inside
  the `impl Server` block that starts at line 162, add:

```rust
    /// Spec 3: whether `account_id` is a key account. An unknown account id
    /// is not a key account, so upstream's outcome stands for it (plan 3
    /// ruling R3); a lookup error propagates. A caller that must fail closed
    /// maps the error to `true`.
    pub async fn za_is_key_account(&self, account_id: u32) -> trc::Result<bool> {
        Ok(self
            .try_account(account_id)
            .await
            .caused_by(trc::location!())?
            .is_some_and(|account| account.is_key_account()))
    }
```

  Add `trc::AddContext` to the imports if the file does not have it.

- [ ] **Step 2: Route every gate through it.** Replace each inline lookup with
  the call shown; keep surrounding comments and control flow as they are.

  | Site | Replace the lookup expression with |
  |---|---|
  | dav `common/za.rs` `is_key_account` body | `Ok(server.za_is_key_account(account_id).await.caused_by(trc::location!())?)` |
  | dav `common/acl.rs` (the `self.account(account_id)...is_key_account()` operand) | `crate::common::za::is_key_account(self, account_id).await?` |
  | dav `calendar/scheduling.rs` (the `self.account(account_id)...is_key_account()` condition) | `crate::common::za::is_key_account(self, account_id).await?` |
  | jmap `principal/availability.rs` | `self.za_is_key_account(account_id).await.caused_by(trc::location!())?` |
  | jmap `blob/download.rs` (the `try_account` operand) | `self.za_is_key_account(*account_id).await.caused_by(trc::location!())?` |
  | jmap `api/request.rs` `za_assert_calendar_allowed` | `server.za_is_key_account(account_id.document_id()).await?` |
  | jmap `api/request.rs` `za_jmap_untraced` body | `server.za_is_key_account(account_id).await.unwrap_or(true)` |
  | groupware `calendar/itip.rs:383` | `self.za_is_key_account(rsvp.account_id).await.caused_by(trc::location!())?` |
  | groupware `calendar/itip.rs:668` | `server.za_is_key_account(account_id).await.caused_by(trc::location!())?` |
  | services `task_manager/index.rs:494` | `server.za_is_key_account(account_id).await?` |
  | http `request.rs` `za_is_key_account_request`, the final lookup | `server.za_is_key_account(access_token.account_id()).await.unwrap_or(false)` |
  | http `request.rs` `za_jmap_untraced` body | `server.za_is_key_account(account_id).await.unwrap_or(true)` |

  Leave three `account()` sites alone: they fail closed deliberately and
  must keep doing so. They are `crates/http/src/auth/authenticate.rs:164`
  (only non-key accounts enter the auth cache), `:181` (the cache entry is
  withdrawn on any failure) and `crates/common/src/auth/authentication.rs:624`
  (`za_refuse_directory_login`). Leave
  `crates/jmap/src/api/session.rs`, `crates/jmap/src/principal/get.rs`,
  `crates/dav/src/principal/propfind.rs`, `crates/dav/src/calendar/delete.rs`,
  `crates/email/src/message/ingest.rs` and `itip.rs:1077` alone: they test an
  account object they already hold. Remove imports the edits leave unused.

- [ ] **Step 3: Check nothing else does an inline lookup.** Run:

```bash
grep -rn "is_some_and(|account| account.is_key_account())" crates
grep -rn -A3 "\.account(" crates --include=*.rs | grep "\.is_key_account()"
```

  Expected: the first command prints one line, the new method in
  `crates/common/src/auth/vault.rs`. The second prints only lines from the
  three deliberate sites above (`authenticate.rs` twice,
  `authentication.rs` once) plus object-in-hand lines that happen to sit
  within three lines of an `.account(` call (for example
  `crates/jmap/src/principal/get.rs`, `crates/groupware/src/calendar/itip.rs:1077`,
  `crates/dav/src/calendar/delete.rs`, `crates/services/src/task_manager/alarm.rs`);
  read each such line and confirm it tests an account it already holds. A
  lookup-by-id gate outside the three deliberate sites is a missed site.

- [ ] **Step 4: Build and format.**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo build --release -p stalwart --no-default-features --features rocks 2>&1 | grep -E "^(warning|error)" | sort | uniq -c
cargo fmt -p common -p dav -p jmap -p groupware -p services -- --check
cargo fmt --manifest-path crates/http/Cargo.toml -- --check
```

  Expected: no new warnings in the touched crates (the product build shows
  the same pre-existing upstream warnings it showed before; compare against
  `git stash` if unsure), no fmt diff.

- [ ] **Step 5: Run the suites that cover the gates.**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo test -p groupware
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture
ZA_KEY_ACCOUNTS=1 STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture
```

  Expected: all pass. This task adds no test. The only behaviour change is
  an unknown id at the two former `account()` sites, reachable only when an
  account is deleted concurrently with an ACL or free-busy request; plan 3
  ruling R3 accepts the fail-open outcome, and the interleaving is recorded
  as unverified (see Review Focus). The gates' existing tests (`za::gating`,
  `za::dav_gate`, the key-mode variants) pin every converted site.

- [ ] **Step 6: Commit.**

```bash
git add -A crates
git commit -F - <<'MSG'
Route every key-account gate through one lookup

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01LUr2fGPHVLze3HcjTtCFCC
MSG
```

### Task 2: Report OPTIONS authentication errors; stop tracing the key-account scheduling refusal

Closes two deferred findings: "Authentication errors on OPTIONS are
swallowed" and "`ItipMessageError` is logged on every key-account PUT".

**OPTIONS.** `za_is_key_account_request` (`crates/http/src/request.rs`)
authenticates an OPTIONS request that carries `Authorization` and, on any
error, falls back to upstream's header (spec section 8, kept). The error is
dropped, so a wrong credential is counted by fail2ban inside
`authentication_failure` but never reaches the trace as `auth.failed` (or
`security.authentication-ban`), unlike every other HTTP request, whose error
the session loop reports with `trc::error!(err.span_id(session.session_id))`
(`request.rs:942`). Report it the same way and keep the response.

**Scheduling trace.** Every DAV PUT resolves `ItipSendStatus`
(`ItipSendStatus::resolve`, `crates/groupware/src/calendar/itip.rs:1066`,
checks in order: scheduling disabled, no calendar address, key account, no
permission, event in the past); for a key
account it is always `KeyAccount`, whose `reason()` is a fixed string, and
both PUT paths in `crates/dav/src/calendar/update.rs` (`:312` update, `:468`
create) trace it as `calendar.itip-message-error`. That logs an expected
state as an error on every key-account write. Skip the trace for
`KeyAccount` only; every other status keeps upstream's trace. The JMAP sites
(`crates/jmap/src/calendar_event/set.rs:525,650,879`) are unreachable for key
accounts (JMAP calendar methods are refused) and stay as they are.

**Files:**
- Modify: `crates/http/src/request.rs` (`za_is_key_account_request` and its doc comment)
- Modify: `crates/dav/src/calendar/update.rs:312` and `:468`
- Test: `tests/src/za/tracing.rs`

**Interfaces:**
- Consumes: `Server::za_is_key_account` from Task 1.

- [ ] **Step 1: Write the failing tests.** In `tests/src/za/tracing.rs`:

  Add the imports `AuthEvent` and `CalendarEvent` to the `trc::{...}` use
  list. Add a constant next to the other canaries:

```rust
const OPTIONS_WRONG_SECRET: &str = "trace-canary-options-wrong-3e9a";
/// A login name no account has and no other module fails with: the probe
/// must not push a real account toward a fail2ban ban, which would turn the
/// reported event into `security.authentication-ban`.
const OPTIONS_PROBE_USER: &str = "options-probe@example.com";
```

  Add a helper next to `event()`. An event in the past makes a plain
  account's scheduling status `EventInPast`, whose reason is traced through
  the same `else if let Some(reason) = itip_status.reason()` branch that
  Step 4 edits; the key account's status is `KeyAccount` whatever the date:

```rust
fn past_event(uid: &str, summary: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20000101T000000Z\r\nDTSTART:20000102T090000Z\r\nDTEND:20000102T100000Z\r\nSUMMARY:{summary}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

/// PUT a past event, then PUT it again with a changed SUMMARY: the first
/// takes the create path of the DAV PUT handler, the second the update path.
async fn put_past_twice(client: &DummyWebDavClient, collection: &str) {
    let path = format!("{collection}past.ics");
    client
        .request_with_headers("PUT", &path, [CONTENT_TYPE], past_event("trace-past", "first"))
        .await
        .with_status(StatusCode::CREATED);
    client
        .request_with_headers("PUT", &path, [CONTENT_TYPE], past_event("trace-past", "second"))
        .await
        .with_status(StatusCode::NO_CONTENT);
}
```

  (If the update answers a different 2xx status, use the one the server
  returns and say so in the report.)

  Extend `strings` with an arm that descends into nested errors, so the
  secret check covers a reported error's `caused_by` chain:

```rust
        trc::Value::Event(err) => err.keys().iter().for_each(|(_, v)| strings(v, out)),
```

  Add a helper next to `http_client()`:

```rust
/// DAV OPTIONS with a wrong Basic credential, from its own forwarded address
/// (the suite enables `use_x_forwarded`) so the failed attempt is not charged
/// to the suite's other clients.
async fn options_wrong_password(user: &str) -> reqwest::Response {
    http_client()
        .request(reqwest::Method::OPTIONS, format!("{SERVER_URL}/dav/cal/"))
        .basic_auth(user, Some(OPTIONS_WRONG_SECRET))
        .header("x-forwarded-for", "10.77.0.1")
        .send()
        .await
        .unwrap()
}
```

  Add both event types to the `traced` array:

```rust
        EventType::Auth(AuthEvent::Failed),
        EventType::Calendar(CalendarEvent::ItipMessageError),
```

  After the `/api/auth` login request and before the 500 ms sleep, add:

```rust
    // Scheduling traces on both PUT paths, for both accounts.
    put_past_twice(&key_client, key_cal).await;
    put_past_twice(&plain_client, plain_cal).await;
```

  and then:

```rust
    // Spec 8: OPTIONS with a wrong credential keeps upstream's header and
    // is reported like any other authentication failure.
    // The secret check below relies on this error's text: "Authentication
    // failed" carries no "basic ". A malformed header would instead report
    // "Failed to decode Basic auth request." and trip the credential-header
    // assertion further down.
    let response = options_wrong_password(OPTIONS_PROBE_USER).await;
    assert_eq!(response.status().as_u16(), 200);
    let dav = response
        .headers()
        .get("dav")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(dav.contains("calendar-auto-schedule"), "{dav}");
```

  Change the `traced_as` closure to take an `EventType` instead of a
  `JmapEvent` (filter on `*typ == event_type`) and update its three existing
  calls to `traced_as(EventType::Jmap(JmapEvent::...))`. Then, after the
  existing assertions and before `test.wait_for_tasks()`, add:

```rust
    // OPTIONS: the failed credential is reported, never its secret.
    assert!(
        typed
            .iter()
            .any(|(typ, _)| *typ == EventType::Auth(AuthEvent::Failed)),
        "OPTIONS authentication failure not reported: {all}"
    );
    assert!(
        !all.contains(OPTIONS_WRONG_SECRET),
        "OPTIONS credential traced: {all}"
    );
    // Scheduling: the plain account's past event still traces its reason on
    // the create and the update path; the key account's fixed refusal is
    // traced on neither (four key-account PUTs reach it in this module).
    let itip = traced_as(EventType::Calendar(CalendarEvent::ItipMessageError));
    assert_eq!(
        itip.matches("lies in the past").count(),
        2,
        "plain-account scheduling reason not traced on both PUT paths: {itip}"
    );
    assert!(
        !itip.contains("zero-access accounts"),
        "key-account scheduling refusal traced: {itip}"
    );
```

- [ ] **Step 2: Run the suite and confirm both new assertions fail.**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture 2>&1 | tail -40
```

  Expected: FAIL at "OPTIONS authentication failure not reported". Then
  temporarily comment out that assertion and rerun: FAIL at "key-account
  scheduling refusal traced". Restore the assertion. Record both red
  messages in the report. After Step 4, also prove the update path is pinned
  on its own: temporarily revert only the `:312` (update) site, rerun, and
  expect FAIL at "key-account scheduling refusal traced"; restore it.

  Cleanup: in the module's final DELETEs, also delete
  `{key_cal}past.ics` (expect 204). The plain collection is deleted whole,
  as today.

- [ ] **Step 3: Report the OPTIONS error.** Replace the body of
  `za_is_key_account_request` in `crates/http/src/request.rs` with:

```rust
    if !req.headers().contains_key(header::AUTHORIZATION) {
        return false;
    }
    match server.authenticate_headers(req, session).await {
        Ok((_in_flight, access_token)) => server
            .za_is_key_account(access_token.account_id())
            .await
            .unwrap_or(false),
        Err(err) => {
            trc::error!(err.span_id(session.session_id));
            false
        }
    }
```

  and its doc comment with:

```rust
/// Spec 8: a key account is not offered `calendar-auto-schedule` in the DAV
/// OPTIONS header. Upstream answers OPTIONS without authenticating;
/// credentials are checked only when present. An authentication error is
/// reported like any request's, and the response falls back to upstream's
/// header; a failed account lookup does too.
```

- [ ] **Step 4: Skip the key-account scheduling trace.** At both sites in
  `crates/dav/src/calendar/update.rs` change

```rust
            } else if let Some(reason) = itip_status.reason() {
```

  to

```rust
            } else if itip_status != ItipSendStatus::KeyAccount
                && let Some(reason) = itip_status.reason()
            {
```

  with the comment `// Spec 9: a key account's refusal is its normal state,
  not an error.` as the first line inside each block, above the
  `trc::event!`. (Let chains are stable in edition 2024; rustfmt decides the
  final layout.)

- [ ] **Step 5: Run and format.**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture 2>&1 | tail -5
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture 2>&1 | tail -5
cargo fmt -p dav -p tests -- --check
cargo fmt --manifest-path crates/http/Cargo.toml -- --check
```

  Expected: both suites pass, no fmt diff.

- [ ] **Step 6: Commit** with subject "Report OPTIONS authentication errors and stop tracing the key-account scheduling refusal" and the two trailers from Global Constraints.

### Task 3: Index tests: destroyed account's index task, calendar index control in the leak scan

Adds two bullets of "Tests worth adding": "A destroyed account's pending
index task (R8)" and "A positive control that plants a calendar search-index
entry for a key account".

Facts the tests rely on (verified against the code on 2026-10-09):

- No test hook pauses the task manager mid-suite, and a PUT's own index task
  runs within milliseconds. So test A plants the task itself, after the
  account is destroyed: the state R8 handles (task queued, account gone).
- `build_calendar_document` (`crates/services/src/task_manager/index.rs:484`)
  first returns `NotIndexed` if calendar indexing is off (it is on by
  default; if it were off, test A's red run would pass vacuously), then asks
  the key-account gate (after Task 1, `za_is_key_account`, which is
  `try_account`-based). For a destroyed account that is `false`, the
  archive lookup finds nothing, the task is retried three times five seconds
  apart (`MISSING_DOCUMENT_MAX_ATTEMPTS`, `MISSING_DOCUMENT_RETRY_DELAY`) and
  then ignored: it drains in about 15 seconds. With the old `account()` form
  the lookup errors, the task is a temporary failure, and `IndexDocument` is
  in the perpetual-retry list, so it never drains.
- `wait_for_tasks` (`tests/src/utils/storage.rs:202`) has no timeout. Wrap it.
- The leak scanner's index check (`tests/src/za/leak.rs:349-373`) reads the
  first key byte of each `SUBSPACE_SEARCH_INDEX` record as
  `SearchIndex | type << 6` and flags `SearchIndex::Calendar` entries of the
  scanned account. Today only Email entries of key2 prove the layout
  (`search_account_records > 0`, leak.rs:664-667); nothing proves the Calendar
  branch fires.

**Files:**
- Create: `tests/src/za/index_task.rs`
- Modify: `tests/src/za/leak.rs` (add `pub async fn test_index_control`)
- Modify: `tests/src/za/mod.rs` (`pub mod index_task;` and two calls)

**Interfaces:**
- Consumes: `Server::za_is_key_account` (Task 1) through the index builder;
  `leak::scan(test: &TestServer, account_id: u32) -> Scan` (existing, leak.rs:296).

- [ ] **Step 1: Write test B, the scanner control.** Add to `tests/src/za/leak.rs`:

```rust
/// Positive control for the scanner's calendar search-index check: entries
/// planted with the store's own key serializer are flagged exactly when they
/// are Calendar entries of the scanned account.
pub async fn test_index_control(test: &mut TestServer) {
    use store::write::{BatchBuilder, SearchIndexClass, SearchIndexId, SearchIndexType, ValueClass};
    println!("Running zero-access leak scanner index control...");
    // `leak::test` ends by deleting key2's calendars and mailboxes without
    // waiting: their unindex tasks may still be running. Drain them so the
    // baseline below is stable.
    test.wait_for_tasks().await;
    let id = test.account("key2@example.com").id().document_id();
    let plain_id = test.account("plain@example.com").id().document_id();
    let entry = |index, account_id, typ| {
        ValueClass::SearchIndex(SearchIndexClass {
            index,
            id: SearchIndexId::Account {
                account_id,
                document_id: u32::MAX - 1,
            },
            typ,
        })
    };
    let term = || SearchIndexType::Term {
        field: 0,
        hash: utils::cheeky_hash::CheekyHash::new(b"za-control"),
    };
    let planted = [
        // Flagged: Calendar entries of the scanned account (document and term layouts).
        entry(SearchIndex::Calendar, id, SearchIndexType::Document),
        entry(SearchIndex::Calendar, id, term()),
        // Not flagged: an Email entry of the account, a Calendar entry of another.
        entry(SearchIndex::Email, id, SearchIndexType::Document),
        entry(SearchIndex::Calendar, plain_id, SearchIndexType::Document),
    ];
    let before = scan(test, id).await;
    assert!(before.violations.is_empty(), "{:?}", before.violations);
    let mut batch = BatchBuilder::new();
    for class in &planted {
        batch.set(class.clone(), Vec::<u8>::new());
    }
    test.server.store().write(batch.build_all()).await.unwrap();

    let result = scan(test, id).await;
    let flagged = result
        .violations
        .iter()
        .filter(|v| v.ends_with("calendar search index entry of the account"))
        .count();
    assert_eq!(flagged, 2, "{:?}", result.violations);
    assert_eq!(
        result.violations.len(),
        2,
        "unexpected violations: {:?}",
        result.violations
    );
    assert_eq!(
        result.search_account_records,
        before.search_account_records + 3,
        "the three entries of the account were not all counted"
    );

    let mut batch = BatchBuilder::new();
    for class in planted {
        batch.clear(class);
    }
    test.server.store().write(batch.build_all()).await.unwrap();
    let after = scan(test, id).await;
    assert!(after.violations.is_empty(), "{:?}", after.violations);
}
```

  Adjust imports to what the file already has (`SearchIndex` is imported at
  leak.rs:38). If the `Scan` struct lacks `Debug`, drop it from the messages
  rather than adding derives elsewhere.

- [ ] **Step 2: Write test A.** Create `tests/src/za/index_task.rs` (SPDX
  header copied from a neighbour):

```rust
//! Plan 3 ruling R8: an index task naming a destroyed key account leaves the
//! queue instead of retrying forever.

use super::{STRONG, user_permissions};
use crate::utils::{server::TestServer, webdav::DummyWebDavClient};
use groupware::cache::GroupwareCache;
use hyper::StatusCode;
use registry::schema::{
    enums::IndexDocumentType,
    structs::{Task, TaskIndexDocument, TaskStatus},
};
use std::time::Duration;
use store::write::BatchBuilder;
use types::{collection::SyncCollection, id::Id};

const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");
const NAME: &str = "key9@example.com";
const EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:r8-event\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:r8-canary\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access destroyed-account index task test...");
    let admin = test.account("admin@example.com").clone();
    let account = admin
        .create_key_user_account(NAME, STRONG, "Key Nine", &[], user_permissions())
        .await;
    let id = account.id().document_id();
    let client = DummyWebDavClient::new(id, NAME, STRONG, NAME);
    client
        .request_with_headers(
            "PUT",
            "/dav/cal/key9%40example.com/default/r8.ics",
            [CONTENT_TYPE],
            EVENT,
        )
        .await
        .with_status(StatusCode::CREATED);
    test.wait_for_tasks().await;
    let document_id = test
        .server
        .fetch_dav_resources(id, id, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("default/r8.ics")
        .unwrap()
        .document_id();

    // The registry entry, the data and the search index of the account go.
    admin.destroy_account(account).await;
    test.wait_for_tasks().await;
    assert!(test.server.try_account(id).await.unwrap().is_none());

    // R8's state: an index task naming the destroyed account.
    let mut batch = BatchBuilder::new();
    batch.schedule_task(Task::IndexDocument(TaskIndexDocument {
        account_id: Id::from(id),
        document_id: Id::from(document_id),
        document_type: IndexDocumentType::Calendar,
        status: TaskStatus::now(),
    }));
    test.server.store().write(batch.build_all()).await.unwrap();
    test.server.notify_task_queue();

    // About 15 s: three missing-document retries five seconds apart.
    tokio::time::timeout(Duration::from_secs(60), test.wait_for_tasks())
        .await
        .expect("an index task for a destroyed account must drain");
}
```

  The account is never passed to `test.insert_account`, so
  `destroy_key_accounts` does not try to destroy it again.

  The green path takes about 15 seconds. Keep `TaskStatus::now()`: it is the
  realistic state. (Planting `TaskStatus::Retry` with `attempt_number: 3`
  would make it instant, but tests a status no real task reaches first.)

- [ ] **Step 3: Wire both into the suite.** In `tests/src/za/mod.rs` add
  `pub mod index_task;` to the module list and, directly after
  `leak::test(&mut test).await;`:

```rust
    leak::test_index_control(&mut test).await;
    index_task::test(&mut test).await;
```

  (`index_task` runs last so no account is created after the destroy and the
  destroyed id cannot be reused while the planted task is pending.)

- [ ] **Step 4: Prove each test can fail.**
  - Test B: in `leak.rs`'s index check, temporarily change
    `index == SearchIndex::Calendar` to `index == SearchIndex::Contacts`; run
    `za_tests`; expect `test_index_control` to fail at `flagged == 2`. Restore.
  - Test A: in `crates/services/src/task_manager/index.rs`, temporarily
    replace the gate's `server.za_is_key_account(account_id).await?` with
    `server.account(account_id).await?.is_key_account()`; run `za_tests`;
    expect the 60 s timeout panic "an index task for a destroyed account must
    drain". Restore.
  Record both red messages in the report.

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture 2>&1 | tail -30
```

- [ ] **Step 5: Run green and format.** Same command; expected PASS,
  including the final `assert_is_empty`. Then
  `cargo fmt -p tests -- --check`.

- [ ] **Step 6: Commit** with subject "Test a destroyed account's index task and the leak scan's calendar index check" and the two trailers from Global Constraints.

### Task 4: Rule-expansion traces carry the UID and no iCalendar text

Adds the "Tests worth adding" bullet "A trace event with a store tracer
that asserts the `RuleExpansionError` traces carry UIDs and no iCalendar
text". Two sites emit `calendar.rule-expansion-error`; plan 3 changed both
from the whole iCalendar to the first UID:

- `crates/groupware/src/calendar/dates.rs:176-187`, at the end of
  `CalendarEventData::new`: fires when calcard's expansion reports errors.
  An RRULE whose UNTIL is before DTSTART does it reliably
  (`DTSTART:20240102T090000Z` with `RRULE:FREQ=DAILY;UNTIL=20200101T000000Z`).
  It runs on the plaintext tree in the DAV PUT path before sealing, for key
  and plain accounts alike. Its `Reason` carries calcard's message, which
  includes UNTIL and DTSTART values; recurrence rules are visible metadata
  (spec section 2), so do not assert their absence.
- `crates/dav/src/calendar/query.rs:221-247`: fires when
  `ArchivedCalendarEventData::expand` returns `None` during a time-range
  REPORT. No client input produces that; an archive whose first time range
  has empty `instances` does. The test plants that corruption with a direct
  store write. For a key account the REPORT unseals first and keeps
  `time_ranges` as stored, so the corruption reaches the handler.

The event is level Debug, so it is emitted only once a subscriber declares
interest (same pattern as `tests/src/za/tracing.rs`).

**Files:**
- Create: `tests/src/za/expansion.rs`
- Modify: `tests/src/za/mod.rs` (`pub mod expansion;` and one call)

- [ ] **Step 1: Write the test.** Create `tests/src/za/expansion.rs` (SPDX
  header copied from a neighbour). Structure:

```rust
//! Spec section 9: rule-expansion traces carry the event's UID, never its
//! iCalendar text, for key and plain accounts.

use super::STRONG;
use crate::utils::{server::TestServer, webdav::DummyWebDavClient};
use groupware::{cache::GroupwareCache, calendar::CalendarEvent};
use hyper::StatusCode;
use std::time::Duration;
use store::{
    ValueKey,
    write::{AlignedBytes, Archive, BatchBuilder},
};
use trc::{
    CalendarEvent as TraceCalendarEvent, Collector, EventType, Key,
    ipc::subscriber::{Interests, SubscriberBuilder},
};
use types::collection::{Collection, SyncCollection};

const SUBSCRIBER_ID: &str = "za-expansion-test";
const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");
const SUMMARY_CANARY: &str = "expansion-canary-summary-5b1e";
const DESCRIPTION_CANARY: &str = "expansion-canary-description-8c3f";

/// An RRULE that ends before it starts: calcard reports an expansion error.
fn bad_rrule(uid: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20240102T090000Z\r\nRRULE:FREQ=DAILY;UNTIL=20200101T000000Z\r\nSUMMARY:{SUMMARY_CANARY}\r\nDESCRIPTION:{DESCRIPTION_CANARY}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

/// A valid event; its stored time ranges are broken afterwards.
fn valid(uid: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:{SUMMARY_CANARY}\r\nDESCRIPTION:{DESCRIPTION_CANARY}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

const TIME_RANGE_QUERY: &str = "<?xml version=\"1.0\" encoding=\"utf-8\" ?><C:calendar-query xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:prop><D:getetag/></D:prop><C:filter><C:comp-filter name=\"VCALENDAR\"><C:comp-filter name=\"VEVENT\"><C:time-range start=\"20990101T000000Z\" end=\"20990103T000000Z\"/></C:comp-filter></C:comp-filter></C:filter></C:calendar-query>";

/// Every string anywhere in an event's values, flattened (as in tracing.rs).
fn strings(value: &trc::Value, out: &mut Vec<String>) {
    match value {
        trc::Value::String(s) => out.push(s.to_string()),
        trc::Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
        _ => {}
    }
}

/// Empties the first stored time range's instances, keeping everything else
/// (including the seal): `expand` then returns `None` at query time.
async fn break_time_ranges(test: &TestServer, account_id: u32, document_id: u32) {
    let stored = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEvent,
            document_id,
        ))
        .await
        .unwrap()
        .unwrap();
    let mut event = stored.deserialize::<CalendarEvent>().unwrap();
    let mut ranges = event.data.time_ranges.into_vec();
    assert!(!ranges.is_empty());
    ranges[0].instances = Box::default();
    event.data.time_ranges = ranges.into_boxed_slice();
    let account_info = test.server.account_info(account_id).await.unwrap();
    let mut batch = BatchBuilder::new();
    event
        .update(
            account_info.account_tenant_ids(),
            stored.to_unarchived::<CalendarEvent>().unwrap(),
            account_id,
            document_id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    test.server.notify_task_queue();
}
```

  Then `pub async fn test(test: &mut TestServer)`, in this order:

  1. Clients: key1 at `/dav/cal/key1%40example.com/default/` with
     `DummyWebDavClient::new(key1_id, "key1@example.com", STRONG, "key1@example.com")`;
     plain as in `tracing.rs:403-409` (plain has no email address) with its
     own collection `/dav/cal/plain%40example.com/expansion/` created by
     MKCALENDAR (copy tracing.rs's MKCALENDAR call).
  2. Register the subscriber for
     `EventType::Calendar(TraceCalendarEvent::RuleExpansionError)` exactly as
     tracing.rs does (`Interests`, `SubscriberBuilder::new(SUBSCRIBER_ID.into())`,
     `.with_lossy(false)`, `Collector::union_interests`, `Collector::reload`).
  3. For each account, PUT `bad-rrule-<who>.ics` with `bad_rrule("za-rrule-<who>")`;
     expect 201.
  4. For each account, PUT `broken-<who>.ics` with `valid("za-chrono-<who>")`;
     expect 201; `test.wait_for_tasks()`; look up its document id with
     `test.server.fetch_dav_resources(id, id, SyncCollection::Calendar)`
     `.await.unwrap().by_path("<collection name>/broken-<who>.ics").unwrap().document_id()`
     (collection name `default` for key1, `expansion` for plain);
     `break_time_ranges(test, id, document_id)`; then REPORT
     `TIME_RANGE_QUERY` on the collection with `[("depth", "1")]`; expect 207.
  5. Sleep 500 ms, drain the receiver into `(details, all_strings)` pairs
     (`details` = the `Key::Details` value as a string, `all_strings` = every
     string of the event joined by newlines), remove the subscriber and
     `Collector::reload()`.
  6. Assert:
     - each of `za-rrule-key`, `za-rrule-plain`, `za-chrono-key`,
       `za-chrono-plain` is the `details` of at least one captured event
       (positive control: both sites, both accounts);
     - every event whose details is `za-chrono-*` has the string
       `chrono error` among its strings; every `za-rrule-*` event has a
       `Key::Reason` whose flattened strings are non-empty (the dates.rs
       reason is a `Value::Array` of calcard messages, so flatten it with
       `strings` rather than matching a single `Value::String`);
     - no captured string contains `SUMMARY_CANARY`, `DESCRIPTION_CANARY`,
       `BEGIN:VCALENDAR`, `SUMMARY:` or `X-ZA-`.
  7. Cleanup: `test.wait_for_tasks()`, DELETE the two key1 events (expect
     204) and the plain collection (expect 204), as tracing.rs:624-631 does.

- [ ] **Step 2: Wire it in.** In `tests/src/za/mod.rs` add `pub mod expansion;`
  and call `expansion::test(&mut test).await;` directly after
  `tracing::test(&mut test).await;`.

- [ ] **Step 3: Prove it can fail.** Temporarily change `Details` at
  `crates/groupware/src/calendar/dates.rs` to `ical.to_string()`; run
  `za_tests`; expect the canary assertion to fail. Restore, then do the same
  with `Details = event.data.event.to_string()` at `crates/dav/src/calendar/query.rs`.
  Restore. Record both red messages in the report.

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture 2>&1 | tail -30
```

- [ ] **Step 4: Run green and format.** Same command, expected PASS
  (including the leak scan and `assert_is_empty`); `cargo fmt -p tests -- --check`.

- [ ] **Step 5: Commit** with subject "Test that rule-expansion traces carry the UID and no iCalendar text" and the two trailers from Global Constraints.

### Task 5: CI retry for the `cal_itip` flake, and the record

Closes the deferred finding "The `zero-access` CI job's plain-mode
`webdav_tests` step has no retry for the known `cal_itip` flake", then
records what plan 5 closed. `cal_itip::test()` runs in both modes
(`tests/src/webdav/mod.rs:234`, before the key/plain split), so both steps
get the retry.

**Files:**
- Modify: `.github/workflows/test.yml` (the two "CalDAV Tests" steps)
- Modify: `docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md`
- Modify: `CLAUDE.md` (the "Status:" paragraph)

- [ ] **Step 1: Retry both CalDAV steps once.** Replace the plain step

```yaml
      - name: CalDAV Tests (plain accounts)
        run: cargo test -p tests webdav::webdav_tests -- --nocapture
```

  with

```yaml
      - name: CalDAV Tests (plain accounts)
        # Upstream's cal_itip sub-test is timing-flaky (DTSTAMP index
        # mismatch); one rerun keeps it from failing the job. A real
        # regression fails both runs. The test server wipes its data
        # directory at start, so the rerun starts clean.
        run: |
          cargo test -p tests webdav::webdav_tests -- --nocapture || {
            echo "::warning title=webdav_tests::plain-mode run failed; retrying once (known cal_itip flake)"
            cargo test -p tests webdav::webdav_tests -- --nocapture
          }
```

  and give the key-mode step ("CalDAV Tests (key accounts)", which keeps its
  `env: ZA_KEY_ACCOUNTS: "1"`) the same comment and the same `run: |` block,
  with "key-mode" in place of "plain-mode" in the warning.

- [ ] **Step 2: Record the closed findings.** In the plan 3 outcome note,
  directly under the heading "## Deferred findings (reviewed, not fixed), by
  area", add one paragraph:

  > Plan 5 (`docs/superpowers/plans/2026-10-09-zero-access-5-deferred-findings.md`,
  > 2026-10-09) closed the items marked **Closed (plan 5)** below. Spec
  > revision 7 had already decided the OPTIONS fail-open, the anonymous rate
  > limit on an unparseable OPTIONS header, the sole default calendar and the
  > check-then-commit window; they are marked **Decided (revision 7)**.

  Then prefix each matching bullet (keep its text) with the bold marker:
  - **Decided (revision 7)**: "OPTIONS fail-open", "`za_is_key_account_request`
    checks for the presence...", "The check-then-commit window...", and the
    "Task 2 gates key off the authenticated account" bullet.
  - **Closed (plan 5)**: "Authentication errors on OPTIONS are swallowed",
    "`ItipMessageError` is logged on every key-account PUT", "Account
    lookups: ..." and "The `zero-access` CI job's plain-mode ... no retry".
  - Two test bullets hold several findings each; mark only the clause plan 5
    closed, by inserting "(**closed, plan 5**)" right after it and leaving
    the other clauses as they are: in "The leak scanner: ...", after the
    first clause ("no positive control plants a calendar search-index entry,
    so the index check proves the key layout through mail entries only");
    in "Final review \"can stay\": ...", after "no trace-event test for the
    removed trace content".
  - Leave unmarked (still deferred): the pending-account operator path, blob
    decode in the leak scanner, `RuleExpansionError` reasons carrying RRULE
    text, and every other test bullet or clause.

  Add one new bullet at the end of "Security and robustness", found while
  planning plan 5:

  > - R8's fall-through can index a destroyed key account's sealed archive:
  >   if a calendar index task runs after the registry delete but before
  >   `DestroyAccount` removes the data, the key-account gate sees no account
  >   and the builder indexes the sealed tree, which exposes only visible
  >   metadata. `DestroyAccount` unindexes calendars before destroying the
  >   data, so a write landing in between leaves an orphan search entry. The
  >   window is milliseconds after a PUT in the product build; no test can
  >   open it deterministically. A fix would skip sealed archives in
  >   `build_calendar_document`.

  In "## Tests worth adding", prefix the three bullets Tasks 3 and 4 closed with
  **Added (plan 5)**.

- [ ] **Step 3: Update the CLAUDE.md status line.** In the "Status:"
  paragraph, change "all three plans are done" to "all five plans are done",
  append ", plan 5 (deferred findings, `docs/superpowers/plans/2026-10-09-zero-access-5-deferred-findings.md`)"
  after the plan 4 entry, and add one sentence: "Key-account gates that look
  an account up by id call `Server::za_is_key_account`
  (`crates/common/src/auth/vault.rs`, fail-open on an unknown id); the
  auth-cache and directory-login checks use `account()` deliberately to fail
  closed." In the
  Commands section, the `cal_itip` bullet gains: "CI reruns each CalDAV
  step once."

- [ ] **Step 4: Commit** with subject "Retry the CalDAV CI steps once and record plan 5" and the two trailers from Global Constraints. The CI change is verified by the pull request's own run.
