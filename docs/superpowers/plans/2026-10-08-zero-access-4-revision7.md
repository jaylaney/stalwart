# Zero-access plan 4: spec revision 7 follow-ups

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the five spec-revision-7 candidates from the plan 3 outcome
note and the three findings of the 2026-10-08 review of PR #5: five small
code changes with tests, then spec revision 7 and the record of the
decisions.

**Architecture:** Five edits at existing sites (the generic alarm email in
`crates/services/src/task_manager/alarm.rs`, the setup stray-data check and
the app-password cleanup in `crates/http/src/api/vault.rs`, the HTTP body
traces in `crates/http-proto` and the DAV handler, and the operator CORS
loop in `crates/http/src/request.rs`), each pinned by a test in the existing
key-mode suites; one new test module (`tests/src/za/tracing.rs`). Then one documentation commit: spec
revision 7, the "Decided" paragraph in the plan 3 outcome note, and the
status line in `CLAUDE.md`. No new modules, no stored-struct changes, no
template changes.

**Tech Stack:** Rust (edition 2024), rkyv archives, the `tests` crate
(`STORE=RocksDb RUST_MIN_STACK=16777216`).

**Spec:** `docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md`
(revision 6; Task 3 makes it revision 7). Decisions being implemented:
`docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md`, section
"Decisions that are Jay's to confirm or reverse", candidates 1 to 5, accepted
on 2026-10-08 as recommended with one refinement (the generic alarm email
keeps its link; see Task 1).

**Read first:** the plan 3 outcome note's "Where things stand" and "Gate
inventory" paragraphs. Every cargo command needs
`export PATH="/opt/homebrew/opt/rustup/bin:$PATH"` first.

## Global Constraints

- No stored struct changes layout (spec invariant 1). `Calendar`,
  `CalendarPreferences` and `CalendarEvent` are read, never redefined.
- Non-key accounts take unchanged upstream code paths (invariant 9). Both
  code tasks change behaviour only inside an `is_key_account()` branch or a
  `/api/vault/*` handler.
- Key material never appears in logs, traces or errors (invariant 3). No
  task touches keys.
- Fork diff stays narrow (invariant 10): edit the existing sites; add no
  files except where a task names one.
- Every source file starts with the SPDX header; copy it from a neighbour.
- Build output must stay warning-free; run `cargo fmt -p <crate> -- --check`
  before each commit (for the http crate:
  `cargo fmt --manifest-path crates/http/Cargo.toml -- --check`).
- Commit messages: subject line, blank line, then the two trailers the
  session's attribution reminder specifies.
- Product build never enables the `enterprise` feature.
- Upstream's `cal_itip` sub-test of `webdav_tests` is timing-flaky; rerun once
  before treating a failure there as a regression.

## Review Focus

1. A key account whose event carries an ORGANIZER with a display name: the
   generic email must show neither the name nor the address (Task 1 test
   asserts the account address is absent from the HTML body; the canary list
   already covers the attendee).
2. A non-key account's alarm email must still carry the organizer row with
   the account name fallback (Task 1 keeps upstream's `cal_alarm` sub-test
   green in plain mode; that test runs the same template path).
3. Setup for an account holding a default calendar whose display name the
   user changed, or that carries an ACL, a colour or a description: must
   still be refused with 409 (Task 2 test plants a renamed default calendar).
4. Setup for an account holding the default calendar plus one event: refused
   (Task 2 test plants an event alongside the untouched default calendar).
5. Setup when the server has no default calendar configured
   (`default_calendar_name` is `None`): any calendar document is user data
   and is refused (Task 2 code handles `None` by refusing; no test, the test
   server always configures one).
6. A key account's DAV REPORT whose response is the unsealed calendar must
   not appear in the response trace, and its Basic credential must not
   appear in any request trace (Task 3 test, both sides, with a plain
   account as positive control).
7. A creation parked after the registry write, revoked, and replaced under
   the same id: the replacement keeps working after the parked creation
   fails (Task 4 test).
8. Permissive CORS enabled while `ZA_ACCOUNT_PAGE_ORIGIN` is unset: vault
   routes carry no `Access-Control-*` header at all (Task 5 test).

---

### Task 1: The generic alarm email drops the organizer row

**Files:**
- Modify: `crates/services/src/task_manager/alarm.rs:610-618` (the
  `organizer` string) and `:673-676` (the organizer row of `EventDetails`)
- Test: `tests/src/webdav/za_variants.rs:239-336` (`alarm`)

**Interfaces:**
- Consumes: `generic: bool` (already computed at `alarm.rs:473` as
  `account_info.account().is_key_account()`), `organizer:
  Option<(Option<&str>, Option<&str>)>` (already cleared to `None` in the
  `if generic` block at `:538-546`).
- Produces: nothing other tasks use.

Context: spec 9 says the generic email has "no ... organizer". Today the
`organizer` variable is cleared for key accounts, but the fallback at
`:617` (`unwrap_or_else(|| account_info.name().to_string())`) puts the
account name back and the `EventDetails` block always emits the row. The
link (`webcal_uri`) stays: spec revision 7 (Task 3) records collection path
names as visible by structure, and without the link a generic reminder
cannot be traced to its event.

- [ ] **Step 1: Add the failing assertion to the key-mode alarm test**

In `tests/src/webdav/za_variants.rs`, inside the `for message in
messages.emails.items.iter()` loop of `alarm`, after the `assert!(text.contains(&start), ...)` line, add:

```rust
        // Spec 9: no organizer row. The account address appears only in the
        // headers; the link carries it percent-encoded, which is why the
        // plain form is a usable canary for the organizer row.
        assert!(
            !html.contains("john@example.com") && !text.contains("john@example.com"),
            "organizer row present in the generic alarm email: {text}"
        );
```

- [ ] **Step 2: Run the key-mode suite to verify it fails**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav::webdav_tests -- --nocapture`
Expected: FAIL in "Running key-account alarm tests..." with "organizer row present".

- [ ] **Step 3: Make the organizer row conditional**

In `crates/services/src/task_manager/alarm.rs` replace the `organizer`
binding at `:610-618`:

```rust
    let organizer = if generic {
        // Spec 9: the generic email names no organizer, not even the
        // account itself.
        None
    } else {
        Some(
            organizer
                .map(|(email, name)| match (email, name) {
                    (Some(email), Some(name)) => format!("{} <{}>", name, email),
                    (Some(email), None) => email.to_string(),
                    (None, Some(name)) => name.to_string(),
                    _ => unreachable!(),
                })
                .unwrap_or_else(|| account_info.name().to_string()),
        )
    };
```

and replace the organizer entry of the `EventDetails` block at `:673-676`:

```rust
            organizer.as_deref().map(|organizer| {
                vec![
                    (CalendarTemplateVariable::Key, locale.calendar_organizer),
                    (CalendarTemplateVariable::Value, organizer),
                ]
            }),
```

Non-key accounts produce exactly the row they produced before.

- [ ] **Step 4: Run both modes of the CalDAV suite**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav::webdav_tests -- --nocapture`
Expected: PASS.
Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav::webdav_tests -- --nocapture`
Expected: PASS (plain mode runs upstream's `cal_alarm`, which checks the
organizer row for a non-key account).

- [ ] **Step 5: Format and commit**

```bash
cargo fmt -p services -- --check && cargo fmt -p tests -- --check
git add crates/services/src/task_manager/alarm.rs tests/src/webdav/za_variants.rs
git commit -m "Drop the organizer row from the generic alarm email"
```

(Add the trailers from the attribution reminder.)

---

### Task 2: Setup tolerates a sole untouched default calendar

**Files:**
- Modify: `crates/http/src/api/vault.rs:746-763` (`za_assert_no_calendar_data`)
- Test: `tests/src/za/setup.rs:383-463` (`test_data_check`)

**Interfaces:**
- Consumes: `Server::za_has_documents(account_id, Collection) ->
  trc::Result<bool>` (`crates/common/src/auth/vault.rs:196`);
  `server.core.groupware.default_calendar_name: Option<String>` and
  `default_calendar_display_name: Option<String>`; the `Calendar` and
  `CalendarPreferences` structs (`crates/groupware/src/calendar/mod.rs:25`
  and `:64`); `CALENDAR_SUBSCRIBED` (same module); the pattern for reading
  every archive of a collection, `server.archives(account_id,
  Collection::Calendar, &(), |document_id, archive| ...)` as used in
  `crates/groupware/src/cache/calcard.rs`; the account name through
  `server.account_info(account_id).await?` (`AccountCache::name()`), the
  accessor `test_data_check` already uses.
- Produces: `za_assert_no_calendar_data` keeps its signature
  `(server: &Server, account_id: u32) -> trc::Result<Option<HttpResponse>>`.

Context: upstream creates a default calendar the first time anything builds
an account's calendar resource cache (`create_default_calendar`,
`crates/groupware/src/cache/mod.rs:381`). It writes one `Calendar` with
`name = default_calendar_name`, one `CalendarPreferences` entry whose `name`
is `"{default_calendar_display_name or name} ({account name})"` and whose
`flags` are `CALENDAR_SUBSCRIBED`, every other field default. If that
happens to a pending account, the stray-data check refuses setup with 409
forever, and no operator path removes the calendar. That calendar holds no
user data, so the check ignores it. It is left in place: it passes through
unseal as plaintext and is sealed by its first write (spec 7.3).

Definition used by code and tests, "untouched default calendar": a
`Calendar` document with `name == default_calendar_name`, empty `acls`,
empty `dead_properties`, and `preferences` either empty or exactly one entry
with `name == "{display} ({account name})"` (display =
`default_calendar_display_name` or, when `None`, `default_calendar_name`),
`description == None`, `color == None`, `default_alerts` empty. `sort_order`,
`flags` and `time_zone` are not compared. When `default_calendar_name` is
`None`, no calendar document is ever untouched.

- [ ] **Step 1: Extend `test_data_check` with three planted cases**

In `tests/src/za/setup.rs`, inside `test_data_check`, after the block that
removes the stray calendar (the one ending with `test.server.commit_batch(batch).await.unwrap();` before `let mut key6 = key6;`), and before `key6.recovery_key = Some(za_setup(...))`, insert:

```rust
    // A server-created default calendar with untouched preferences is not
    // user data: setup must not be blocked by it (plan 3 outcome, candidate 4).
    let default_name = test
        .server
        .core
        .groupware
        .default_calendar_name
        .clone()
        .expect("test server configures a default calendar");
    let default_display = format!(
        "{} ({})",
        test.server
            .core
            .groupware
            .default_calendar_display_name
            .as_deref()
            .unwrap_or(default_name.as_str()),
        "key6@example.com"
    );
    let plant_calendar = |name: String, display: String| {
        use groupware::calendar::{CALENDAR_SUBSCRIBED, Calendar, CalendarPreferences};
        Calendar {
            name,
            preferences: vec![CalendarPreferences {
                account_id: key6_id,
                name: display,
                flags: CALENDAR_SUBSCRIBED,
                ..Default::default()
            }],
            ..Default::default()
        }
    };
    let insert_calendar = |test: &TestServer, calendar: groupware::calendar::Calendar, id: u32| async move {
        let account_info = test.server.account_info(key6_id).await.unwrap();
        let mut batch = store::write::BatchBuilder::new();
        calendar
            .insert(account_info.account_tenant_ids(), key6_id, id, &mut batch)
            .unwrap();
        test.server.commit_batch(batch).await.unwrap();
    };
    let remove_calendar = |test: &TestServer, id: u32| async move {
        use groupware::{DestroyArchive, calendar::Calendar};
        use store::{
            ValueKey,
            write::{AlignedBytes, Archive},
        };
        use types::collection::Collection;
        let account_info = test.server.account_info(key6_id).await.unwrap();
        let archive = test
            .server
            .store()
            .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                key6_id,
                Collection::Calendar,
                id,
            ))
            .await
            .unwrap()
            .expect("calendar archive");
        let mut batch = store::write::BatchBuilder::new();
        DestroyArchive(archive.to_unarchived::<Calendar>().unwrap())
            .delete(account_info.account_tenant_ids(), key6_id, id, None, &mut batch)
            .unwrap();
        test.server.commit_batch(batch).await.unwrap();
    };

    // 1. Renamed default calendar: user data, refused.
    insert_calendar(
        test,
        plant_calendar(default_name.clone(), "My calendar".into()),
        1,
    )
    .await;
    let reply = za_post("setup", &body).await.expect(409);
    assert_eq!(reply["error"], "account already holds calendar data");
    remove_calendar(test, 1).await;

    // 2. Untouched default calendar plus one event: refused.
    insert_calendar(
        test,
        plant_calendar(default_name.clone(), default_display.clone()),
        2,
    )
    .await;
    {
        use groupware::calendar::{CalendarEvent, CalendarEventData};
        let account_info = test.server.account_info(key6_id).await.unwrap();
        let mut batch = store::write::BatchBuilder::new();
        CalendarEvent {
            names: vec![groupware::calendar::CalendarEventName {
                name: "stray.ics".into(),
                parent_id: 2,
            }],
            data: CalendarEventData::default(),
            ..Default::default()
        }
        .insert(account_info.account_tenant_ids(), key6_id, 0, None, &mut batch)
        .unwrap();
        test.server.commit_batch(batch).await.unwrap();
    }
    let reply = za_post("setup", &body).await.expect(409);
    assert_eq!(reply["error"], "account already holds calendar data");
    // Remove the event (same pattern as the calendar removal, with
    // Collection::CalendarEvent and CalendarEvent).
    {
        use groupware::{DestroyArchive, calendar::CalendarEvent};
        use store::{
            ValueKey,
            write::{AlignedBytes, Archive},
        };
        use types::collection::Collection;
        let account_info = test.server.account_info(key6_id).await.unwrap();
        let archive = test
            .server
            .store()
            .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                key6_id,
                Collection::CalendarEvent,
                0,
            ))
            .await
            .unwrap()
            .expect("event archive");
        let mut batch = store::write::BatchBuilder::new();
        DestroyArchive(archive.to_unarchived::<CalendarEvent>().unwrap())
            .delete(account_info.account_tenant_ids(), key6_id, 0, None, &mut batch)
            .unwrap();
        test.server.commit_batch(batch).await.unwrap();
    }

    // 3. Sole untouched default calendar: setup succeeds (checked by the
    //    za_setup call below, which expects 200).
```

The existing `key6.recovery_key = Some(za_setup("key6@example.com", &token, STRONG).await);` that follows is case 3: it now runs with the untouched default calendar (document 2) still present. Leave it; `admin.destroy_account(key6)` removes the calendar with the account.

The exact `CalendarEvent` insert signature and the `CalendarEventName` /
`CalendarEventData` names must be taken from `crates/groupware/src/calendar/mod.rs`
and `storage.rs`; if `insert` for events needs different arguments, use the
same call `tests/src/za/leak.rs` uses to plant its negative control event.
If planting a bare event is impractical, plant a `CalendarEventNotification`
document instead (the check treats both collections the same); say which in
the report.

- [ ] **Step 2: Run the za suite to verify it fails**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`
Expected: FAIL in "Running zero-access setup data check tests..." at the
`za_setup` call (409 instead of 200) because the untouched default calendar
is still refused. Cases 1 and 2 already pass (any calendar is refused today).

- [ ] **Step 3: Teach the check to ignore an untouched default calendar**

Replace `za_assert_no_calendar_data` in `crates/http/src/api/vault.rs`:

```rust
/// Refuses an account that already holds calendar data: setup would
/// otherwise convert plaintext into a key account. A sole default calendar
/// created by the server (`create_default_calendar`) with untouched
/// preferences is not user data and is ignored; it is sealed by its first
/// write (spec 7.3).
async fn za_assert_no_calendar_data(
    server: &Server,
    account_id: u32,
) -> trc::Result<Option<HttpResponse>> {
    for collection in [
        Collection::CalendarEvent,
        Collection::CalendarEventNotification,
    ] {
        if server.za_has_documents(account_id, collection).await? {
            return Ok(Some(conflict("account already holds calendar data")));
        }
    }
    if za_has_user_calendars(server, account_id).await? {
        return Ok(Some(conflict("account already holds calendar data")));
    }
    Ok(None)
}

/// True when the account holds any calendar document other than a sole
/// untouched default calendar.
async fn za_has_user_calendars(server: &Server, account_id: u32) -> trc::Result<bool> {
    let Some(default_name) = server.core.groupware.default_calendar_name.as_deref() else {
        return server.za_has_documents(account_id, Collection::Calendar).await;
    };
    let Some(account_info) = server.account_info(account_id).await? else {
        return server.za_has_documents(account_id, Collection::Calendar).await;
    };
    let expected_display = format!(
        "{} ({})",
        server
            .core
            .groupware
            .default_calendar_display_name
            .as_deref()
            .unwrap_or(default_name),
        account_info.name()
    );
    let mut count = 0u32;
    let mut user_data = false;
    server
        .archives(account_id, Collection::Calendar, &(), |_, archive| {
            count += 1;
            let calendar = archive.unarchive::<Calendar>()?;
            let untouched = calendar.name == default_name
                && calendar.acls.is_empty()
                && calendar.dead_properties.0.is_empty()
                && match calendar.preferences.as_slice() {
                    [] => true,
                    [prefs] => {
                        prefs.name == expected_display
                            && prefs.description.is_none()
                            && prefs.color.is_none()
                            && prefs.default_alerts.is_empty()
                    }
                    _ => false,
                };
            if !untouched {
                user_data = true;
            }
            Ok(count < 2 && !user_data)
        })
        .await
        .caused_by(trc::location!())?;
    Ok(user_data || count > 1)
}
```

Adjust to the real shapes: `archive.unarchive::<Calendar>()` yields an
`ArchivedCalendar` whose string fields compare with `==` against `&str` and
whose `Option` fields are `ArchivedOption` (use `.is_none()`);
`dead_properties` is a newtype over a `Vec`, so use whatever emptiness
accessor `crates/dav-proto` gives (`is_empty()` if present). If
`server.archives` is not reachable from the `http` crate, iterate with
`server.store().iterate(IterateParams::new(ValueKey::archive(account_id,
Collection::Calendar, 0), ValueKey::archive(account_id, Collection::Calendar,
u32::MAX)), |key, value| ...)` and unarchive each value as
`crates/common/src/auth/vault.rs:196` and the groupware cache do. Whichever
accessor returns the account (`account_info`, `account`, `try_account`): a
missing account refuses (treats every calendar as user data).

- [ ] **Step 4: Run the za suite to verify it passes**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`
Expected: PASS through all sixteen sub-modules including the leak scan.

- [ ] **Step 5: Format and commit**

```bash
cargo fmt --manifest-path crates/http/Cargo.toml -- --check && cargo fmt -p tests -- --check
git add crates/http/src/api/vault.rs tests/src/za/setup.rs
git commit -m "Let setup proceed past a sole untouched default calendar"
```

(Add the trailers from the attribution reminder.)

---

### Task 3: Keep key-account DAV traffic and credentials out of HTTP body traces

PR #5 review finding P1 (2026-10-08). With `http.request-body` and
`http.response-body` enabled on a Trace-level tracer, `fetch_body`
(`crates/http-proto/src/request.rs:21-95`) records every request header,
including the reversible Basic `Authorization` value, plus the full body; and
`crates/http/src/request.rs:879-892` records every text response body. For a
key account that is the plaintext calendar and a credential that unwraps its
keys after logout. The vault API already dodges both (`fetch_body_untraced`,
binary JSON bodies). Nothing else does.

**Files:**
- Modify: `crates/http-proto/src/request.rs:21-95` (header redaction in
  `fetch_body_inner`)
- Modify: `crates/http-proto/src/lib.rs:40-44` and
  `crates/http-proto/src/response.rs:21-30, 263-275` (`HttpResponse` gains an
  `untraced` flag)
- Modify: `crates/http/src/request.rs:879-892` (honour the flag)
- Modify: `crates/dav/src/request.rs:601-625` (`handle_dav_request`: untraced
  body fetch and flagged response for key accounts) and
  `crates/dav/src/common/za.rs:182` (`is_key_account` becomes `pub(crate)`)
- Modify: `crates/http/src/api/vault.rs:194-200` (`json_with_status` also
  sets the flag; belt and braces)
- Create: `tests/src/za/tracing.rs`; register it in `tests/src/za/mod.rs`
  after `dav_seal::test_collections` and before `gating::test`

**Interfaces:**
- Consumes: `trc::ipc::subscriber::SubscriberBuilder` (`new(id)`,
  `set_interests(iter)`, `with_lossy(false)`, `register() -> (Sender,
  Receiver<EventBatch>)`), `trc::EventType::Http(trc::HttpEvent::RequestBody
  | ResponseBody)`, `AccessToken::primary_id()`, `Server::try_account`.
- Produces: `HttpResponse::with_untraced_body(self) -> Self` and
  `HttpResponse::is_untraced(&self) -> bool`; `pub fn fetch_body_untraced`
  unchanged.

Rulings carried into this task:
- Credential header values (`authorization`, `proxy-authorization`,
  `cookie`) are redacted in the `RequestBody` trace for every request, key
  account or not. A key account's Basic credential travels on JMAP, upload
  and OAuth requests too, where no account is known at fetch time. This
  masks one field of one Trace-level event and changes no code path, so
  invariant 9 is kept; Task 6 records it in spec section 10.
- Key-account DAV requests use `fetch_body_untraced` and their responses are
  flagged `untraced`, so the trace shows `[redacted]`. Non-key DAV traffic is
  traced exactly as upstream traces it.
- When the key-account lookup itself fails, the request is treated as a key
  account's (fail closed: no trace).

- [ ] **Step 1: Write the failing test**

Create `tests/src/za/tracing.rs` (SPDX header from a neighbour):

```rust
//! Spec invariant 7: nothing a key account sends or receives over DAV, and
//! no credential header from any request, reaches the HTTP body traces.

use trc::{EventType, HttpEvent, ipc::subscriber::SubscriberBuilder};

use crate::{
    utils::za::{create_key_user_account, za_dav_client},
    TestServer,
};

const KEY_CANARY: &str = "trace-canary-key-9f3c";
const PLAIN_CANARY: &str = "trace-canary-plain-7a1d";

fn event(uid: &str, summary: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:{summary}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

/// Every string anywhere in an event's values, flattened.
fn strings(value: &trc::Value, out: &mut Vec<String>) {
    match value {
        trc::Value::String(s) => out.push(s.to_string()),
        trc::Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
        _ => {}
    }
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access HTTP trace tests...");
    let (_tx, mut rx) = SubscriberBuilder::new("za-trace-test".into())
        .set_interests([
            EventType::Http(HttpEvent::RequestBody),
            EventType::Http(HttpEvent::ResponseBody),
        ])
        .with_lossy(false)
        .register();

    // ... create one key account and one plain account with the suite's
    // helpers (see tests/src/za/dav_seal.rs for the exact calls), then:
    //   key:   PUT event("trace-key", KEY_CANARY), then REPORT calendar-query
    //          on the collection (returns the unsealed canary to the client)
    //   plain: PUT event("trace-plain", PLAIN_CANARY), then the same REPORT
    // Both with Basic authentication (the suite's DAV client does that).

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let mut seen = Vec::new();
    while let Ok(batch) = rx.try_recv() {
        for event in batch {
            let mut values = Vec::new();
            for (_, value) in event.keys.iter() {
                strings(value, &mut values);
            }
            seen.push(values.join("\n"));
        }
    }
    let all = seen.join("\n");
    // Positive control: the subscriber works and plain traffic is traced.
    assert!(all.contains(PLAIN_CANARY), "no plain-account trace captured: {all}");
    // Key-account traffic is absent on both sides.
    assert!(!all.contains(KEY_CANARY), "key-account body traced: {all}");
    // Credentials are never traced, for any account.
    assert!(
        !all.to_ascii_lowercase().contains("basic ") && !all.contains("Bearer "),
        "credential header traced: {all}"
    );
    assert!(all.contains("[redacted]"), "redaction marker missing: {all}");

    // ... destroy both accounts the way dav_seal.rs does and finish with
    // test.assert_is_empty().await
}
```

Fill the elided parts from `tests/src/za/dav_seal.rs` (account creation,
`za_dav_client`, PUT/REPORT helpers, teardown). If `event.keys` is not the
field name, use whatever `trc::Event<EventDetails>` exposes for its
key-value pairs. If no events arrive at all (positive control fails), the
collector thread is not running under the test server: look at how
`crates/common/src/manager/boot.rs` or the telemetry config starts it
(`Collector::spawn`, `init_tracing` or similar) and start it once in the
test; report what was needed.

- [ ] **Step 2: Run the za suite to verify it fails**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`
Expected: FAIL in "Running zero-access HTTP trace tests..." on
`key-account body traced` (or `credential header traced`).

- [ ] **Step 3: Redact credential headers in `fetch_body_inner`**

In `crates/http-proto/src/request.rs`, both `Details = req.headers()...`
expressions (oversize branch and normal branch) map each header through:

```rust
fn traced_header(k: &hyper::header::HeaderName, v: &hyper::header::HeaderValue) -> trc::Value {
    const REDACTED: &[&str] = &["authorization", "proxy-authorization", "cookie"];
    let value = if REDACTED.contains(&k.as_str()) {
        "[redacted]"
    } else {
        v.to_str().unwrap_or_default()
    };
    trc::Value::Array(vec![
        k.as_str().to_compact_string().into(),
        value.to_compact_string().into(),
    ])
}
```

so `Details = req.headers().iter().map(|(k, v)| traced_header(k, v)).collect::<Vec<_>>()`.

- [ ] **Step 4: Add the `untraced` flag to `HttpResponse`**

In `crates/http-proto/src/lib.rs` add `untraced: bool` to `HttpResponse`;
in `response.rs` initialise it `false` in `new` and `redirect`, and add:

```rust
    /// Marks a body that must not appear in the `HttpEvent::ResponseBody`
    /// trace (a key account's calendar data, or tokens).
    pub fn with_untraced_body(mut self) -> Self {
        self.untraced = true;
        self
    }

    pub fn is_untraced(&self) -> bool {
        self.untraced
    }
```

Check every place that constructs `HttpResponse { .. }` by struct literal
(grep `HttpResponse {` across `crates/`) and add the field.

In `crates/http/src/request.rs:879-892`, the `Contents` match becomes:

```rust
                        Contents = if response.is_untraced() {
                            trc::Value::String("[redacted]".into())
                        } else {
                            match response.body() {
                                HttpResponseBody::Text(value) =>
                                    trc::Value::String(value.as_str().into()),
                                HttpResponseBody::Binary(_) =>
                                    trc::Value::String("[binary data]".into()),
                                HttpResponseBody::Stream(_) =>
                                    trc::Value::String("[stream]".into()),
                                _ => trc::Value::None,
                            }
                        },
```

- [ ] **Step 5: Use both in the DAV handler and the vault API**

In `crates/dav/src/common/za.rs:182` make `is_key_account` `pub(crate)`.
In `crates/dav/src/request.rs` `handle_dav_request`, before the body fetch:

```rust
        // Spec invariant 7: a key account's DAV traffic never reaches the
        // HTTP body traces. A failed lookup is treated as a key account.
        let untraced = crate::common::za::is_key_account(self, access_token.primary_id())
            .await
            .unwrap_or(true);
```

Replace the `fetch_body(&mut request, max, session.session_id)` call with a
branch: `if untraced { fetch_body_untraced(&mut request, max).await } else {
fetch_body(&mut request, max, session.session_id).await }` (same `max`
expression as today; import `fetch_body_untraced` from `http_proto::request`).
At the end of `handle_dav_request`, where the response is returned, apply
`if untraced { response.with_untraced_body() } else { response }` to the
success response; the error-to-response conversion needs no flag (problem
bodies carry no content).

In `crates/http/src/api/vault.rs:194-200`, `json_with_status` chains
`.with_untraced_body()` after `.with_no_store()` and its doc comment says
the binary body and the flag both keep tokens out of the trace.

- [ ] **Step 6: Run the suites**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`
Expected: PASS.
Run: `cargo test -p http-proto` and `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav::webdav_tests -- --nocapture` (plain mode).
Expected: PASS (non-key DAV traced as before).

- [ ] **Step 7: Format and commit**

```bash
cargo fmt -p http-proto -- --check && cargo fmt --manifest-path crates/http/Cargo.toml -- --check && cargo fmt -p dav -- --check && cargo fmt -p tests -- --check
git add crates/http-proto crates/http/src/request.rs crates/http/src/api/vault.rs crates/dav/src/request.rs crates/dav/src/common/za.rs tests/src/za/tracing.rs tests/src/za/mod.rs
git commit -m "Keep key-account DAV bodies and credential headers out of HTTP traces"
```

(Add the trailers from the attribution reminder.)

---

### Task 4: App-password cleanup deletes only the credential it created

PR #5 review finding P2 (2026-10-08). `za_delete_registry_credential`
(`crates/http/src/api/vault.rs:389-408`) removes an app-password registry
credential by numeric id alone. Creation A can finish step (2), park before
step (3), be revoked, and creation B then reuses A's id (ids are reused
once the highest credential is deleted) and publishes. When A resumes,
`za_publish` correctly rejects (publication id differs) and
`za_withdraw_pending` leaves B's wrap, but A's cleanup deletes B's registry
credential, so B's freshly issued password stops authenticating. The retried
delete in `za_app_password_revoke` has the same identity gap.

**Files:**
- Modify: `crates/http/src/api/vault.rs:389-408` (`za_delete_registry_credential`),
  `:1219-1254` (`za_delete_registry_credential_logged`), `:1105-1112` (the
  cleanup call in `za_app_password`), `:1256-1328` (`za_app_password_revoke`)
- Test: `tests/src/za/app_password.rs` (after the "Rollback, entry gone"
  block at `:415-426`)

**Interfaces:**
- Consumes: `park_creation(test, id, description) -> (Parked, credential_id)`
  and `Parked::finish()`, `create(description)`, `revoke(credential_id)`,
  `caldav(app_password, expected_status)`, `registry_app_ids(test)`,
  `wrap_state(test, id, credential_id)`, all in `tests/src/za/app_password.rs`;
  `SecondaryCredential.secret` (the hashed secret string written at step 2).
- Produces: `za_delete_registry_credential(server, account_id,
  credential_id, secret_hash: &str) -> trc::Result<bool>` and the same extra
  parameter on `za_delete_registry_credential_logged`.

- [ ] **Step 1: Write the failing test**

In `tests/src/za/app_password.rs`, after the "Rollback, entry gone" block
(the one ending `assert!(!registry_app_ids(test).await.contains(&gone_id));`), add:

```rust
    // Rollback must not touch a replacement (PR #5 P2): creation A parks
    // after step (2), is revoked, and creation B reuses its id and lands.
    // A's cleanup must leave B's registry credential alone.
    let (parked, reused_id) = park_creation(test, id, "Parked").await;
    revoke(reused_id).await.expect(200);
    let (replacement, replacement_id) = create("Replacement").await;
    assert_eq!(replacement_id, reused_id, "the id is reused");
    caldav(&replacement, StatusCode::MULTI_STATUS).await;
    let reply = parked.finish().await;
    assert_eq!(
        reply.expect(409)["error"],
        "app password publication failed"
    );
    assert_eq!(
        wrap_state(test, id, replacement_id).await,
        Some(WrapState::Published)
    );
    assert!(registry_app_ids(test).await.contains(&replacement_id));
    test.server
        .invalidate_local_caches(&[CacheInvalidation::AccessToken(id)])
        .await;
    caldav(&replacement, StatusCode::MULTI_STATUS).await;
    revoke(replacement_id).await.expect(200);
```

- [ ] **Step 2: Run the za suite to verify it fails**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`
Expected: FAIL in "Running zero-access app-password tests..." at
`registry_app_ids(...).contains(&replacement_id)` or the second `caldav`
(401 after the cache invalidation).

- [ ] **Step 3: Carry the credential identity into the delete**

`za_delete_registry_credential` gains `secret_hash: &str` and retains with:

```rust
    credentials.retain(|c| {
        !(matches!(&c.value, Credential::AppPassword(app)
            if app.credential_id.document_id() == credential_id
                && app.secret == secret_hash))
    });
```

`za_delete_registry_credential_logged` gains the same parameter and passes
it through on every retry (each retry re-reads the registry, so a
replacement under the same id never matches).

In `za_app_password`, keep the hash: `let secret_hash_for_cleanup =
secret_hash.clone();` before the credential is pushed (or compute the
registry credential first and clone its `secret`), and pass
`&secret_hash_for_cleanup` to the cleanup call after the failed publish.

In `za_app_password_revoke`, read the registry credential's secret once,
before the vault commit:

```rust
    let registry_secret = za_registry_account(server, account_id)
        .await?
        .and_then(|reg| {
            reg.account.credentials.values().find_map(|c| match c {
                Credential::AppPassword(c)
                    if c.credential_id.document_id() == request.credential_id =>
                {
                    Some(c.secret.clone())
                }
                _ => None,
            })
        });
```

The `dangling` branch uses `registry_secret.is_some()` in place of its own
lookup and passes the secret to the delete; the main branch, after the
commit, deletes only when `registry_secret` is `Some` and passes it. The
`secret` field's exact type is whatever `SecondaryCredential` declares;
compare as strings.

- [ ] **Step 4: Run the za suite to verify it passes**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Format and commit**

```bash
cargo fmt --manifest-path crates/http/Cargo.toml -- --check && cargo fmt -p tests -- --check
git add crates/http/src/api/vault.rs tests/src/za/app_password.rs
git commit -m "Delete only the app-password registry credential a cleanup created"
```

(Add the trailers from the attribution reminder.)

---

### Task 5: No operator CORS headers on vault routes when the origin is unset

PR #5 review finding P2 (2026-10-08). `crates/http/src/request.rs:898-912`
skips the operator's `Access-Control-Allow-Origin` on vault paths only when
the vault response already carries one. With `ZA_ACCOUNT_PAGE_ORIGIN` unset,
`za_with_cors` adds nothing, so enabling upstream's `use_permissive_cors`
puts `Access-Control-Allow-Origin: *` on `/api/vault/*`, and every website
can read vault responses. Spec 4.1: unset means no CORS headers.

**Files:**
- Modify: `crates/http/src/request.rs:898-912`
- Test: `tests/src/za/cors.rs` (before the line
  `test.server.inner.cache.set_za_account_page_origin(...)` at `:78-82`)

**Interfaces:**
- Consumes: `Account::registry_update_setting`, `reload_settings`, the
  `send`, `assert_no_cors`, `header_of` helpers in `cors.rs`.
- Produces: nothing.

- [ ] **Step 1: Write the failing test**

In `tests/src/za/cors.rs`, after the two "Unset" assertions and before the
origin is set, add:

```rust
    // Unset plus permissive CORS: the operator's `*` reaches every other
    // route and never a vault route (spec 4.1).
    let admin = test.account("admin@example.com");
    admin
        .registry_update_setting(
            Http {
                use_permissive_cors: true,
                ..Default::default()
            },
            &[Property::UsePermissiveCors],
        )
        .await;
    admin.reload_settings().await;
    let response = send(Method::OPTIONS, "/api/auth", None).await;
    assert_eq!(
        header_of(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some("*")
    );
    for (method, path, body) in [
        (Method::OPTIONS, "/api/vault/password", None),
        (Method::POST, "/api/vault/recovery-key", Some(b"not json".to_vec())),
    ] {
        let response = send(method, path, body).await;
        assert_no_cors(&response);
    }
    admin
        .registry_update_setting(
            Http {
                use_permissive_cors: false,
                ..Default::default()
            },
            &[Property::UsePermissiveCors],
        )
        .await;
    admin.reload_settings().await;
```

`assert_no_cors` must check every `Access-Control-*` header, not only the
origin; extend it if it does not.

- [ ] **Step 2: Run the za suite to verify it fails**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`
Expected: FAIL in "Running zero-access CORS tests..." with `*` present on a
vault route.

- [ ] **Step 3: Exclude vault routes from operator CORS headers**

In `crates/http/src/request.rs`, the loop over
`server.core.network.http.response_headers` becomes:

```rust
                        for (header, value) in &server.core.network.http.response_headers {
                            // The vault API sets its own CORS headers from
                            // the account-page origin and none when that is
                            // unset (spec 4.1); operator CORS headers never
                            // apply there. Every other response takes the
                            // operator's value, as upstream.
                            if is_vault_path
                                && header.as_str().starts_with("access-control-")
                            {
                                continue;
                            }
                            headers.insert(header.clone(), value.clone());
                        }
```

(`HeaderName::as_str()` is lowercase.) The existing cors.rs assertions
that an operator origin never replaces the account page on vault routes
still hold.

- [ ] **Step 4: Run the za suite to verify it passes**

Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Format and commit**

```bash
cargo fmt --manifest-path crates/http/Cargo.toml -- --check && cargo fmt -p tests -- --check
git add crates/http/src/request.rs tests/src/za/cors.rs
git commit -m "Keep operator CORS headers off the vault API"
```

(Add the trailers from the attribution reminder.)

---

### Task 6: Spec revision 7 and the decision record

**Files:**
- Modify: `docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md`
  (header lines 3-20; section 2 "Visible metadata" paragraph at :95-104;
  section 4.1 setup wording near the state table at :232-260; section 8.2;
  section 9 table at :666-680; invariant 9 at :810-811)
- Modify: `docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md`
  (section "Decisions that are Jay's to confirm or reverse", :207)
- Modify: `CLAUDE.md:50` (status line) and the "Status:" paragraph below it

**Interfaces:**
- Consumes: Task 1's and Task 2's behaviour as committed.
- Produces: nothing.

Prose rules: wrap at 78 columns like the rest of the spec; keep the
existing voice (short declarative sentences, no marketing); never remove a
sentence that still holds; put each amendment where the subject already
lives rather than in a new section.

- [ ] **Step 1: Amend the spec**

Header (after the Revision 6 sentence, before the "The 4.1 item" sentence),
add:

```
Revision 7 (2026-10-08) records the plan 3 outcome, following the
decisions in `../plans/2026-10-06-zero-access-plan3-outcome.md`:
collection path names as visible by structure (section 2), the setup
refusal of pre-existing calendar data (section 4.1), the DAV OPTIONS
authentication as the second invariant 9 exception (section 8.2), and in
the section 9 table the `ParticipantIdentity/changes` answer, the mail
index of key accounts and the contents of the generic alarm email.
```

Change line 3's status to `revision 7, approved`.

Section 2, in the "Visible metadata" paragraph, after "filenames chosen by
clients (often UID-derived)," insert "the path names of calendar collections
(they route URLs and so are never sealed),".

Section 4.1, after the state table's description of `setup` (the row
`PendingSetup` + `setup` → `Active`, and the write-ordering paragraph that
follows), add a paragraph:

```
**Pre-existing data.** `setup-token` and `setup` refuse (409, `account
already holds calendar data`) an account that holds calendar events,
scheduling notifications, or any calendar collection other than a sole
default calendar created by the server with untouched preferences. That
one calendar holds only the server's default display name and the account
address; it is kept, passes through unseal as plaintext, and is sealed by
its first write (section 7.3). Converting an account with real calendar
data is not supported in release 1.
```

Section 8.2 (find the paragraph recording the REPORT prefix check as the
one all-account change), add after it:

```
The second all-account change is DAV `OPTIONS`: when the request carries an
`Authorization` header the server authenticates it, so a key account's
`DAV` header omits `calendar-auto-schedule` and clients never offer
invitations (verified with Apple Calendar on 2026-10-08). A wrong
credential there counts as a failed attempt, an unparseable header charges
the anonymous rate limit, and a transient authentication failure falls
back to upstream's header rather than failing the request. Non-key
accounts see the same header as before.
```

Section 9 table, three rows:

- "JMAP calendars capability and methods": append "; `ParticipantIdentity/changes` answers `cannotCalculateChanges` as upstream does for every account".
- "Full-text indexing": append "; mail is unsealed in release 1, so its index, including the generic alarm emails, is upstream's (accepted scope limit until a mail release)".
- "Alarm email": replace "and a link" with "and a link to the event (collection path name and filename, both visible by structure)"; keep "no title, description, location, organizer, guests or conference link".

Invariant 9: replace "with one recorded exception: the calendar REPORT
prefix check in section 8.2" with "with two recorded exceptions, both in
section 8.2: the calendar REPORT prefix check and DAV OPTIONS
authentication".


Section 10 (error handling), add a paragraph at the end:

```
**Traces.** HTTP body traces (`http.request-body`, `http.response-body`,
Trace level) never carry a key account's DAV request or response body
(shown as `[redacted]`), nor the value of an `Authorization`,
`Proxy-Authorization` or `Cookie` header on any request. Vault API bodies
are never traced. Non-key DAV traffic is traced as upstream traces it.
```

Section 4.1, in the app-password write-ordering text, add one sentence:
"Cleanup after a failed publication, and the registry delete after a
revocation, remove only the registry credential whose hashed secret
matches the one the operation read, so a replacement that reused the id
is never deleted." In the CORS paragraph, change "When the variable is unset
no CORS headers are sent." to "When the variable is unset no CORS headers
are sent on vault routes, including operator-configured ones (permissive
CORS or response headers)."

- [ ] **Step 2: Record the decisions in the plan 3 outcome note**

At the top of "Decisions that are Jay's to confirm or reverse" (before
"Candidates for spec revision 7:"), add:

```
Decided 2026-10-08: all five candidates were accepted as recommended and
are now spec revision 7, with one refinement to candidate 2: the generic
alarm email keeps its link, because collection path names are visible by
structure and without the link a reminder cannot be traced to its event;
only the organizer row was dropped (plan 4,
`2026-10-08-zero-access-4-revision7.md`). Candidate 4 was implemented as
proposed: a sole untouched default calendar no longer blocks setup. The
three non-spec items (R3, R5/R7/R15, R9) stand as recorded. The same plan
fixed the three findings of the 2026-10-08 review of PR #5: HTTP body
traces (key-account DAV bodies and credential headers), app-password
cleanup identity, and operator CORS headers on vault routes. The list below
is kept as the record of what was decided.
```

- [ ] **Step 3: Update CLAUDE.md**

Line 50: `(revision 6, approved; binding for plans 2 and 3)` becomes
`(revision 7, approved)`. In the "Status:" paragraph, after "plan 3 (gating)
at `fde504f3`", add ", plan 4 (revision 7 follow-ups) at `<commit of Task 5>`"
and keep the pointer to the plan 3 outcome note.

- [ ] **Step 4: Check line widths and commit**

Run: `awk 'length > 78 {print FILENAME": "FNR": "length}' docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md`
Expected: no lines reported beyond those already over 78 before the edit
(table rows are exempt; check with `git diff -U0 | grep '^+' | awk 'length > 79'` that no added non-table line exceeds 78).

```bash
git add docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md
git add -f CLAUDE.md
git commit -m "Spec revision 7: record the plan 3 decisions"
```

(Add the trailers from the attribution reminder. `CLAUDE.md` is git-ignored
and needs `-f`.)
