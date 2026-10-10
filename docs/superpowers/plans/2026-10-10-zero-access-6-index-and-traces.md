# Zero-access plan 6: sealed archives out of the index, expansion traces Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close issues #8 and #9: the calendar index builder never indexes a sealed event, and rule-expansion traces carry no calcard error text for key accounts and name the account and document on the query path.

**Architecture:** Two independent fixes in existing code, each with one integration test in the za suite. Task 1 adds an archived-event seal check next to `is_sealed` in `groupware` and calls it from `build_calendar_document`. Task 2 gives `CalendarEventData` a constructor that takes a key-account flag (the DAV PUT handler passes it) and adds account and document ids to the `CalendarQueryHandler` trace.

**Tech Stack:** Rust 2024 workspace (Stalwart 0.16.25 fork), calcard 0.3.14, rkyv archives, `trc` tracing, the `tests` crate integration harness.

**Spec:** `docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md` (revision 7). Sections that bind this plan: 7.1 (an event is sealed when the VCALENDAR root's last entry is `X-ZA-KEY`), 9 (nothing of a key account's calendar is indexed), 10 "Traces" (no calendar content in traces), 12 (invariants). Source of the two findings: `docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md`, "Deferred findings", the R8 fall-through bullet and the `RuleExpansionError` bullet.

## Global Constraints

- Every cargo command starts with `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`.
- Never build or test with the `enterprise` feature for the product; the `tests` crate enables it for upstream's own tests, leave that.
- Integration tests: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture` (one function, sub-modules in order, port 8899; never run two suites at once). Key-mode CalDAV: `ZA_KEY_ACCOUNTS=1 STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture`; plain mode is the same without `ZA_KEY_ACCOUNTS`. Upstream's `cal_itip` sub-test is timing-flaky: rerun once before treating a failure there as a regression.
- Unit tests: `cargo test -p groupware`.
- Formatting: `cargo fmt -p groupware -- --check`, `cargo fmt -p services -- --check`, `cargo fmt -p dav -- --check`, `cargo fmt -p tests -- --check`. Build output stays warning-free.
- Do not edit generated code: `crates/registry/src/schema/*`, `crates/trc/src/event/enums.rs`. Use existing `trc` keys only (`AccountId`, `DocumentId`, `Reason`, `Details`, `Limit`).
- No stored struct changes layout.
- Non-key accounts keep upstream behaviour, including byte-identical trace text.
- Keep the fork diff narrow: new functions plus one-line call changes at existing sites.
- New source files start with the SPDX header copied from a neighbour.
- Commit messages: subject, blank line, then exactly:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01LUr2fGPHVLze3HcjTtCFCC
  ```
- Work on branch `plan6-index-traces` from `main`. Do not push.

## Review Focus

1. A plain account's event whose VCALENDAR root ends with a client-written `X-ZA-KEY` property is now left out of the search index. Expected and accepted: no ordinary client writes that name. Task 1's test plants a sealed archive under a plain account, which exercises exactly this branch.
2. A key account's event with several failing components gets one static reason per error. The `Reason` array must hold only the five static strings, never calcard text. Task 2's test asserts the key reason exactly.
3. Plain accounts' rule-expansion trace text must stay byte-identical to upstream. Task 2's test asserts the plain reason still carries calcard's `Until date` text.
4. The free-busy path (`freebusy.rs`) passes ids to the same trace as the REPORT path but no test drives a broken time range through free-busy. Reviewers check the ids passed there by reading the call site.
5. The existing key-account gate in `build_calendar_document` still runs first; the new check only covers the window after the account is gone. `index_task::test` (plan 5) still has to pass unchanged.

---

### Task 1: The index builder skips sealed archives (issue #8)

**Files:**
- Modify: `crates/groupware/src/calendar/seal/event.rs` (new `archived_event_is_sealed` after `is_sealed` at line 69; unit test in the existing `mod tests`)
- Modify: `crates/groupware/src/calendar/seal/mod.rs:17` (re-export)
- Modify: `crates/services/src/task_manager/index.rs:484-517` (`build_calendar_document`)
- Modify: `tests/src/za/index_task.rs` (new `pub async fn test_sealed_archive`)
- Modify: `tests/src/za/mod.rs:106` (run it after `index_task::test`)
- Modify: `docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md` (mark the R8 fall-through bullet)

**Interfaces:**
- Consumes: `KEY_PROP` (`seal/tree.rs:23`, `"X-ZA-KEY"`); `is_sealed` (`seal/event.rs:69`) as the model; `BuildResult::NotIndexed`; `leak::scan(test, account_id) -> Scan` with `violations: Vec<String>` (`tests/src/za/leak.rs:297`), whose calendar-index violations end with `"calendar search index entry of the account"`.
- Produces: `pub fn archived_event_is_sealed(event: &ArchivedCalendarEvent) -> bool`, re-exported from `groupware::calendar::seal`.

- [ ] **Step 1: Write the failing unit test**

In `seal/event.rs`'s `mod tests`, using the existing helpers `event()`, `keys()` and `archive()`:

```rust
#[test]
fn archived_seal_check_matches_is_sealed() {
    let plain = event();
    let stored = archive(&plain);
    assert!(!archived_event_is_sealed(
        stored.unarchive::<CalendarEvent>().unwrap()
    ));

    let mut sealed = event();
    seal_event(&mut sealed, &keys(), 9).unwrap();
    assert!(is_sealed(&sealed));
    let stored = archive(&sealed);
    assert!(archived_event_is_sealed(
        stored.unarchive::<CalendarEvent>().unwrap()
    ));
}
```

Adjust the `unarchive` call to whatever the neighbouring tests in this module use to get an `&ArchivedCalendarEvent` from `Archive<AlignedBytes>`; keep the assertions.

- [ ] **Step 2: Run it and see it fail to compile**

Run: `cargo test -p groupware archived_seal_check_matches_is_sealed`
Expected: FAIL, `archived_event_is_sealed` not found.

- [ ] **Step 3: Implement the check**

After `is_sealed` in `seal/event.rs`:

```rust
/// `is_sealed` on the stored archive, for readers that never deserialize
/// it (the index builder).
pub fn archived_event_is_sealed(event: &ArchivedCalendarEvent) -> bool {
    event
        .data
        .event
        .components
        .first()
        .and_then(|root| root.entries.last())
        .is_some_and(|e| {
            matches!(&e.name, ArchivedICalendarProperty::Other(n)
                if n.as_str().eq_ignore_ascii_case(KEY_PROP))
        })
}
```

Import `ArchivedCalendarEvent` and `calcard::icalendar::ArchivedICalendarProperty` (fix the paths to where they live). Add `archived_event_is_sealed` to the `pub use event::{...}` line in `seal/mod.rs`.

- [ ] **Step 4: Run the unit test**

Run: `cargo test -p groupware archived_seal_check_matches_is_sealed`
Expected: PASS.

- [ ] **Step 5: Write the failing integration test**

Append to `tests/src/za/index_task.rs`:

```rust
/// Plan 3's R8 fall-through: an index task that reaches a sealed archive
/// after the key-account check (the account was destroyed in between)
/// must not index it. The window cannot be opened on demand, so the test
/// plants a key account's sealed archive under a plain account, which the
/// key-account check lets through, and schedules its index task.
pub async fn test_sealed_archive(test: &mut TestServer) {
    println!("Running zero-access sealed-archive index test...");
    const KEY: &str = "key10@example.com";
    const PLAIN: &str = "plain10@example.com";
    const PLANTED: u32 = u32::MAX - 3;
    let admin = test.account("admin@example.com").clone();
    let key = admin
        .create_key_user_account(KEY, STRONG, "Key Ten", &[], user_permissions())
        .await;
    let plain = admin
        .create_user_account(PLAIN, STRONG, "Plain Ten", &[], user_permissions())
        .await;
    let key_id = key.id().document_id();
    let plain_id = plain.id().document_id();

    // A sealed archive, written by the real PUT path.
    DummyWebDavClient::new(key_id, KEY, STRONG, KEY)
        .request_with_headers(
            "PUT",
            "/dav/cal/key10%40example.com/default/sealed.ics",
            [CONTENT_TYPE],
            EVENT,
        )
        .await
        .with_status(StatusCode::CREATED);
    test.wait_for_tasks().await;
    let document_id = test
        .server
        .fetch_dav_resources(key_id, key_id, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("default/sealed.ics")
        .unwrap()
        .document_id();
    let sealed = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            key_id,
            Collection::CalendarEvent,
            document_id,
        ))
        .await
        .unwrap()
        .unwrap();

    // Plant it under the plain account, which the key-account check lets
    // through, and index it.
    let class = ValueKey::archive(plain_id, Collection::CalendarEvent, PLANTED).class;
    let mut batch = BatchBuilder::new();
    batch.set(class.clone(), sealed.as_bytes().to_vec());
    batch.schedule_task(Task::IndexDocument(TaskIndexDocument {
        account_id: Id::from(plain_id),
        document_id: Id::from(PLANTED),
        document_type: IndexDocumentType::Calendar,
        status: TaskStatus::now(),
    }));
    test.server.store().write(batch.build_all()).await.unwrap();
    test.server.notify_task_queue();
    tokio::time::timeout(Duration::from_secs(60), test.wait_for_tasks())
        .await
        .expect("the planted index task must drain");

    let calendar_entries = super::leak::scan(test, plain_id)
        .await
        .violations
        .into_iter()
        .filter(|v| v.ends_with("calendar search index entry of the account"))
        .collect::<Vec<_>>();
    assert!(calendar_entries.is_empty(), "{calendar_entries:?}");

    let mut batch = BatchBuilder::new();
    batch.clear(class);
    test.server.store().write(batch.build_all()).await.unwrap();
    admin.destroy_account(key).await;
    admin.destroy_account(plain).await;
    test.wait_for_tasks().await;
}
```

Add the imports this needs (`store::{ValueKey, write::{AlignedBytes, Archive}}`, `types::collection::Collection`); `ValueKey::archive(..).class` and `BatchBuilder::set`/`clear` with a `ValueClass` follow `tests/src/za/expansion.rs` and `leak::test_index_control`, so if a name differs (for example the archive bytes accessor), use the one those files use. Wire it in `tests/src/za/mod.rs` directly after `index_task::test(&mut test).await;`:

```rust
    index_task::test_sealed_archive(&mut test).await;
```

- [ ] **Step 6: Red run of the integration test**

The builder check is not written yet. Run: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture`
Expected: FAIL at the step 5 assertion, with at least one calendar search index entry of `plain10`. If it passes instead, the test does not reach the builder: stop and report rather than changing the assertion.

- [ ] **Step 7: Implement the builder check**

In `build_calendar_document`, replace the `Some(metadata_) => Ok(BuildResult::Document(...))` arm with:

```rust
        Some(metadata_) => {
            let event = metadata_
                .unarchive::<CalendarEvent>()
                .caused_by(trc::location!())?;
            // Spec 9: a sealed archive has nothing to index. Reached when the
            // account was destroyed after the key-account check (plan 3 R8).
            if archived_event_is_sealed(event) {
                return Ok(BuildResult::NotIndexed);
            }
            Ok(BuildResult::Document(event.index_document(
                account_id,
                document_id,
                index_fields,
                server.core.email.default_language,
            )))
        }
```

Import `groupware::calendar::seal::archived_event_is_sealed`.

- [ ] **Step 8: Run the suites**

Run, one at a time: `cargo test -p groupware`; the za suite; key-mode `webdav_tests`; plain-mode `webdav_tests`.
Expected: all PASS.

- [ ] **Step 9: Mark the finding closed**

In the plan 3 outcome note, prefix the bullet that starts `- R8's fall-through can index a destroyed key account's sealed archive` with `**Closed (plan 6)**` (the same style as the `**Closed (plan 5)**` bullets), and append to that section's opening paragraph: `Plan 6 (`docs/superpowers/plans/2026-10-10-zero-access-6-index-and-traces.md`, 2026-10-10) closed the items marked **Closed (plan 6)**.`

- [ ] **Step 10: fmt and commit**

Run the fmt checks for `groupware`, `services` and `tests`. Commit all Task 1 files with subject `Skip sealed calendar archives in the index builder` and the trailers from Global Constraints.

---

### Task 2: Rule-expansion traces carry no calcard text for key accounts, and ids on the query path (issue #9)

**Files:**
- Modify: `crates/groupware/src/calendar/dates.rs:25-31` (`new` delegates to new `new_for`), `:176-186` (the trace), plus a new `za_expansion_reason` and a unit test
- Modify: `crates/dav/src/calendar/update.rs:231` and `:418` (call `new_for`)
- Modify: `crates/dav/src/calendar/query.rs:216-246` (`CalendarQueryHandler::new` takes ids; trace carries them)
- Modify: `crates/dav/src/calendar/freebusy.rs:228`, `crates/dav/src/common/propfind.rs:517` and `:1030` (pass ids)
- Modify: `tests/src/za/expansion.rs` (stronger assertions)
- Modify: `docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md`, `CLAUDE.md`

**Interfaces:**
- Consumes: `calcard::icalendar::dates::CalendarErrorType` (variants `MissingDtStart`, `InvalidDtStart`, `InvalidDtEnd`, `InvalidDuration`, `RRule(_)`); `za_keys: Option<_>` in scope at both `update.rs` sites (lines 80, 332, 491).
- Produces: `CalendarEventData::new_for(ical: ICalendar, default_tz: Tz, max_expansions: usize, next_email_alarm: &mut Option<CalendarAlarm>, key_account: bool) -> Self`; `CalendarQueryHandler::new(event: &ArchivedCalendarEvent, max_time_range: Option<TimeRange>, default_tz: Tz, account_id: u32, document_id: u32) -> Self`.

- [ ] **Step 1: Write the failing unit test**

In `dates.rs` (add a `#[cfg(test)] mod tests` if the file has none):

```rust
#[test]
fn za_expansion_reason_is_static() {
    use calcard::icalendar::dates::CalendarErrorType;
    assert_eq!(za_expansion_reason(&CalendarErrorType::MissingDtStart), "Missing DTSTART property");
    assert_eq!(za_expansion_reason(&CalendarErrorType::InvalidDtStart), "Invalid DTSTART property");
    assert_eq!(za_expansion_reason(&CalendarErrorType::InvalidDtEnd), "Invalid DTEND property");
    assert_eq!(za_expansion_reason(&CalendarErrorType::InvalidDuration), "Invalid DURATION property");
    assert_eq!(
        za_expansion_reason(&CalendarErrorType::RRule(calcard::datecalc::error::RRuleError::IterError(
            "UNTIL=20200101T000000Z".into()
        ))),
        "RRule error"
    );
}
```

Fix the `RRuleError` path to wherever calcard exports it.

- [ ] **Step 2: Run it and see it fail to compile**

Run: `cargo test -p groupware za_expansion_reason_is_static`
Expected: FAIL, `za_expansion_reason` not found.

- [ ] **Step 3: Implement the constructor and the reason**

In `dates.rs`, turn the existing `new` into `new_for` with the extra parameter, and put a delegating `new` above it:

```rust
    pub fn new(
        ical: ICalendar,
        default_tz: Tz,
        max_expansions: usize,
        next_email_alarm: &mut Option<CalendarAlarm>,
    ) -> Self {
        Self::new_for(ical, default_tz, max_expansions, next_email_alarm, false)
    }

    /// `new`, except that for a key account the rule-expansion trace names
    /// each error's kind instead of calcard's text, which quotes RRULE values
    /// (spec 10, "Traces").
    pub fn new_for(
        ical: ICalendar,
        default_tz: Tz,
        max_expansions: usize,
        next_email_alarm: &mut Option<CalendarAlarm>,
        key_account: bool,
    ) -> Self {
        // ... the existing body of `new`, unchanged except the trace below ...
    }
```

Change only the `Reason` expression of the trace:

```rust
                Reason = expanded
                    .errors
                    .into_iter()
                    .map(|e| {
                        if key_account {
                            za_expansion_reason(&e.error).to_compact_string()
                        } else {
                            e.error.to_compact_string()
                        }
                    })
                    .collect::<Vec<_>>(),
```

And add, outside the `impl`:

```rust
/// The kind of a rule-expansion error without calcard's values; the same
/// words calcard prints, minus the RRULE detail.
fn za_expansion_reason(error: &CalendarErrorType) -> &'static str {
    match error {
        CalendarErrorType::MissingDtStart => "Missing DTSTART property",
        CalendarErrorType::InvalidDtStart => "Invalid DTSTART property",
        CalendarErrorType::InvalidDtEnd => "Invalid DTEND property",
        CalendarErrorType::InvalidDuration => "Invalid DURATION property",
        CalendarErrorType::RRule(_) => "RRule error",
    }
}
```

- [ ] **Step 4: Run the unit test**

Run: `cargo test -p groupware za_expansion_reason_is_static`
Expected: PASS.

- [ ] **Step 5: Use it from the DAV PUT handler**

At `update.rs:231` and `:418`, change `CalendarEventData::new(` to `CalendarEventData::new_for(` and add `za_keys.is_some(),` as the last argument. No other caller changes: JMAP calendar methods and iTIP are refused for key accounts (spec 9).

- [ ] **Step 6: Add ids to the query trace**

In `query.rs`, add `account_id: u32, document_id: u32` as the last two parameters of `CalendarQueryHandler::new` and add `AccountId = account_id, DocumentId = document_id,` to its `trc::event!` before `Details`. Pass the ids at the three callers: `freebusy.rs:228` (`account_id`, `document_id` of the loop at line 182), `propfind.rs:517` and `:1030` (`account_id`, `document_id` of the `for item in paths` loop, from `item`).

- [ ] **Step 7: Tighten the integration test**

In `tests/src/za/expansion.rs`:
- Capture `Key::AccountId` and `Key::DocumentId` values per event alongside `details` and `reason` (they are `trc::Value::UInt`; record them as `Option<u64>`).
- Keep each `broken-{who}` document id from the loop (it is already computed there) and, for the `za-chrono-key` and `za-chrono-plain` traces, assert the captured account id equals that client's account id and the captured document id equals that event's document id.
- Replace the `za-rrule-` branch with:

```rust
        } else if details == "za-rrule-key" {
            assert_eq!(reason, &["RRule error".to_string()], "key reason: {dump}");
        } else if details == "za-rrule-plain" {
            assert!(
                reason.iter().any(|s| s.contains("Until date")),
                "plain reason must stay upstream's text: {dump}"
            );
        }
```

- [ ] **Step 8: Red run, then green run**

First run the za suite with step 5 reverted (plain `new` at both `update.rs` sites): expected FAIL at `key reason`. Restore step 5, then run, one at a time: `cargo test -p groupware`; the za suite; key-mode `webdav_tests`; plain-mode `webdav_tests`.
Expected: all PASS.

- [ ] **Step 9: Docs**

- Plan 3 outcome note: prefix the bullet starting `` - `RuleExpansionError` reasons still carry calcard error strings `` with `**Closed (plan 6)**`.
- `CLAUDE.md`, the Status paragraph: change "all five plans are done" to "all six plans are done" and, after the plan 5 entry, add `, plan 6 (sealed archives out of the index, expansion traces, \`docs/superpowers/plans/2026-10-10-zero-access-6-index-and-traces.md\`)`. The merge hash is added after the merge, not here.

- [ ] **Step 10: fmt and commit**

Run the fmt checks for `groupware`, `dav` and `tests`. Commit all Task 2 files with subject `Keep calcard text out of key-account expansion traces, add ids to the query trace` and the trailers from Global Constraints.
