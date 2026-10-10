# Zero-access plan 7: test hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close issues #12–#16. Add the gate tests that are missing, replace the scheduling checks that cannot fail, add the sealed-event cases left over from plan 2 and a WebSocket conversion test, make the leak scanner read blobs from a filesystem blob store, and record a mutation run for every gate the key-mode variants cover.

**Architecture:** Only tests and docs change; no product code changes. Two new helpers go in `tests/src/utils/za.rs`:
- `plant_event` writes an unsealed event straight into the store, which is how a legacy plaintext event looks. It lets tests reach gates that sealing otherwise hides: the alarm recipient override, the DELETE `send_itip` gate and the RSVP attendee-copy gate.
- `wait_for_delivery` waits until the SMTP queue is empty instead of sleeping for a fixed time.

Each new test proves it can fail through a red run against a temporary edit of the gate it guards. The edit is reverted at once. The last task runs every mutation and records the results in the plan 3 outcome note.

**Tech Stack:** Rust 2024 workspace (Stalwart 0.16.25 fork), the `tests` crate integration harness (`TestServer`, `DummyWebDavClient`), calcard 0.3.14, rkyv archives, `trc` subscribers, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md` (revision 7). These sections bind this plan:
- 4.2: Bearer, OAuth and API-key logins are refused on calendar paths.
- 7: plaintext events stay readable and are sealed by the next write.
- 9: the scheduling, alarm, index and JMAP gates.
- 11: the test list.
- 12: the invariants.

Where the findings come from:
- `docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md`: "Deferred findings" (Tests), "Tests worth adding", R7 and R15.
- `docs/superpowers/plans/2026-10-06-zero-access-plan2-outcome.md:244-255`: "Tests worth adding".

## Global Constraints

- Every cargo command starts with `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`.
- The plan is test-only. No commit changes anything under `crates/`.
  - Mutation edits under `crates/` are temporary. Revert each one with `git checkout -- <file>` right after its run.
  - At the end of every task, `git diff main -- crates/` prints nothing.
- Never build the product with the `enterprise` feature. The `tests` crate enables it for upstream's own tests; leave that.
- Suites (each is one test function on port 8899; never run two at once, and never in parallel with a mutation build of another task):
  - `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture`
  - `ZA_KEY_ACCOUNTS=1 STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture` (key mode)
  - `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav_tests -- --nocapture` (plain mode)
  - `BLOB_STORE=FileSystem STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za_tests -- --nocapture` (filesystem blob store, Task 6 onward)
  - Upstream's `cal_itip` sub-test of `webdav_tests` is timing-flaky. Rerun once before treating a failure there as a regression.
- Formatting: `cargo fmt -p tests -- --check`. Build output stays warning-free.
- Do not edit generated code (`crates/registry/src/schema/*`, `crates/trc/src/event/enums.rs`).
- Never weaken an existing assertion to make a test pass.
  - Some expected values in this plan were read from the code and never run; they are marked "(from code reading)".
  - If a run disagrees with one of them, assert what the run shows only when the plan names an oracle for it, such as the plain-account control. Write the disagreement in the task report as a ruling.
  - Otherwise stop and report.
- New source files start with the SPDX header copied from a neighbour.
- Keep the diff narrow. Change only the tests named in each task. `leak.rs` keeps its own planting block; do not refactor it onto `plant_event`.
- Commit messages: subject, blank line, then exactly:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01LUr2fGPHVLze3HcjTtCFCC
  ```
- Work on branch `plan7-test-hardening` from `main`. Do not push.

## Review Focus

1. **A message stuck in the SMTP queue.** `wait_for_delivery` panics after ten seconds if a message is still queued, for example one for an external recipient that is retrying. Only the outbound queue can stall it.
   - Every sub-module ends with `assert_is_empty`, which scans the queue, so at each sub-module start the queue is empty.
   - Reviewers check that no new test queues mail to an external domain on a green run.
2. **Stale DAV resource cache after a planted event.** A planted event is invisible to the DAV handlers until the DAV resource cache is invalidated, and DELETE then answers 404. `plant_event` invalidates the cache. A test that writes the store any other way must do the same.
3. **The whole za suite under a filesystem blob store.** The CI step added in Task 6 runs all of `za_tests` with `BLOB_STORE=FileSystem`, not only the scanner. The blob purge in `assert_is_empty` and the mail fetches must also work against it.
4. **Conditional requests compared with a plain account.** The cases in Task 3 (304, `If-Match`, `If-None-Match: *`) and the timezone REPORT in Task 4 compare the key account's result with the same request against a plain account. That comparison is the oracle. A sealed event must answer exactly as an ordinary one, even where both differ from what the code reading predicted.
5. **Mutation reverts.** Mutation runs edit product code. A missed revert would ship a disabled gate. Every task ends with `git diff main -- crates/` empty, and the final review checks it again.

---

### Task 1: Delivery barrier, planted events, and scheduling checks that can fail (issue #14)

**Files:**
- Modify: `tests/src/utils/za.rs` (new helpers `plant_event`, `mail_count`, `queued_recipients`, `wait_for_delivery`)
- Modify: `tests/src/za/gating.rs` (`test_scheduling`, `:416-656`; remove the local `wait_for_delivery` at `:422-430`)
- Modify: `tests/src/webdav/za_variants.rs` (`scheduling`, `:340-393`)

**Interfaces:**
- Consumes:
  - `raw_event(test, account_id, "calendar/name.ics") -> (Archive<AlignedBytes>, u32)` (`tests/src/za/dav_seal.rs:39`).
  - `TestServer::read_queued_messages() -> Vec<MessageWrapper>` (`tests/src/smtp/inbound/mod.rs:213`). `MessageWrapper.message.recipients[i].address` is a `Box<str>`.
  - `CalendarEvent::insert(tenant_ids, account_id, document_id, next_alarm: Option<CalendarAlarm>, batch)` (`crates/groupware/src/calendar/storage.rs:200`).
  - `CalendarEventData::new(ical, Tz::Floating, 100, &mut next_alarm)`.
- Produces (Tasks 2–5 use these):
  - `pub async fn plant_event(test: &TestServer, account_id: u32, calendar: &str, name: &str, ical: &str, schedule_tag: Option<u32>) -> u32` (returns the document id).
  - `pub async fn mail_count(test: &TestServer, account_id: u32) -> usize`
  - `pub async fn queued_recipients(test: &TestServer) -> Vec<String>`
  - `pub async fn wait_for_delivery(test: &TestServer)`

Background:
- A task that sends mail (iMIP in `crates/services/src/task_manager/imip.rs:221-300`) awaits the local SMTP session that accepts the message. So once the task queue is drained, every message is in the SMTP queue. Local delivery ingests a message and then removes it from that queue.
- A CANCEL on DELETE needs three things (`delete_all`, `crates/groupware/src/calendar/storage.rs:478-481`):
  1. `send_itip`, which is false for key accounts (`crates/dav/src/calendar/delete.rs:81`).
  2. A stored schedule tag, which key accounts never get (`ItipSendStatus::KeyAccount`).
  3. Visible attendees, which sealing hides.

  So the existing post-DELETE checks cannot fail. A planted plaintext event with a schedule tag removes conditions 2 and 3 and leaves the DELETE gate as the only thing in the way.
- The RSVP attendee-copy gate (`crates/groupware/src/calendar/itip.rs:666-672`) cannot be observed with a sealed copy either: a sealed copy shows no ATTENDEE, so the sync never matches it. A planted plaintext copy does match.

- [ ] **Step 1: Add the helpers to `tests/src/utils/za.rs`**

Append the following. Add the imports to the existing `use` block at the top of the file: `common::ipc::CacheInvalidation`, `email::cache::MessageCacheFetch`, `groupware::{cache::GroupwareCache, calendar::{CalendarEvent, CalendarEventData}}`, `store::write::BatchBuilder`, `types::collection::{Collection, SyncCollection}`, `calcard::common::timezone::Tz`. `Duration` is already imported.

```rust
/// Writes `ical` into the store as the event `name` in the account's
/// calendar `calendar` (a slug such as "default"), bypassing the DAV
/// handlers and so the seal. Such an event is what a legacy plaintext event
/// (written before the account held keys) looks like; tests use it to reach
/// code that sealing otherwise hides. Schedules the event's next email alarm
/// the way a PUT does. Returns the document id.
pub async fn plant_event(
    test: &TestServer,
    account_id: u32,
    calendar: &str,
    name: &str,
    ical: &str,
    schedule_tag: Option<u32>,
) -> u32 {
    let calendar_id = test
        .server
        .fetch_dav_resources(account_id, account_id, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path(calendar)
        .unwrap_or_else(|| panic!("calendar {calendar} not found"))
        .document_id();
    let size = ical.len() as u32;
    let ical = match calcard::Parser::new(ical).entry() {
        calcard::Entry::ICalendar(ical) => ical,
        other => panic!("{other:?}"),
    };
    let mut next_alarm = None;
    let event = CalendarEvent {
        names: vec![common::DavName {
            name: name.into(),
            parent_id: calendar_id,
        }],
        data: CalendarEventData::new(ical, Tz::Floating, 100, &mut next_alarm),
        size,
        schedule_tag,
        ..Default::default()
    };
    let account_info = test.server.account_info(account_id).await.unwrap();
    let document_id = test
        .server
        .store()
        .assign_document_ids(account_id, Collection::CalendarEvent, 1)
        .await
        .unwrap();
    let mut batch = BatchBuilder::new();
    event
        .insert(
            account_info.account_tenant_ids(),
            account_id,
            document_id,
            next_alarm,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    // A direct store write neither wakes the task manager nor refreshes the
    // DAV resource cache the way the DAV handlers do.
    test.server.notify_task_queue();
    test.server
        .invalidate_local_caches(&[CacheInvalidation::DavResources(account_id)])
        .await;
    document_id
}

/// Emails in the account's mailboxes.
pub async fn mail_count(test: &TestServer, account_id: u32) -> usize {
    test.server
        .get_cached_messages(account_id)
        .await
        .unwrap()
        .emails
        .items
        .len()
}

/// Recipients of every message still in the SMTP queue.
pub async fn queued_recipients(test: &TestServer) -> Vec<String> {
    test.read_queued_messages()
        .await
        .iter()
        .flat_map(|m| m.message.recipients.iter().map(|r| r.address.to_string()))
        .collect()
}

/// Waits until everything the server has queued is delivered. A task that
/// sends mail (iMIP, alarms) waits for the local SMTP session to accept the
/// message, so once the task queue is empty every such message is in the
/// SMTP queue; local delivery ingests it and then removes it. Panics after
/// ten seconds, naming what is still queued.
pub async fn wait_for_delivery(test: &TestServer) {
    test.wait_for_tasks().await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let queued = queued_recipients(test).await;
        if queued.is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "mail still queued for {queued:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Ingest can queue index tasks of its own.
    test.wait_for_tasks().await;
}
```

- [ ] **Step 2: Rewrite the scheduling checks in `gating::test_scheduling`**

Delete the local `wait_for_delivery` (`gating.rs:422-430`) and import the shared helpers:
- `use crate::utils::za::{mail_count, plant_event, wait_for_delivery};`
- `use super::dav_seal::raw_event;`
- `use groupware::calendar::CalendarEvent;` (merge it into the existing `groupware` import)

Then make these changes inside `test_scheduling`.

(a) Right after `plain_inbox` is defined (`:462`), add:

```rust
    let plain_id = plain.id().document_id();
    // Mail counts before anything is sent. iMIP email is delivered to the
    // recipient's mailbox (mail is not sealed in this release), so a count
    // that does not move shows the sender sent nothing.
    let plain_mail = mail_count(test, plain_id).await;
    let key_mail = mail_count(test, key1_id).await;
```

Remove the later `let plain_id = plain.id().document_id();` (`:580`).

(b) Replace `:479-483` (after the schedule-tag assertion) with:

```rust
    wait_for_delivery(test).await;
    // Sender side: nothing left the key organizer.
    assert_eq!(mail_count(test, plain_id).await, plain_mail);
    assert_eq!(
        members(&plain_client, plain_inbox).await,
        Vec::<String>::new()
    );
```

(c) Replace the block "Deleting the key organizer's event sends no CANCEL." (`:533-542`) with:

```rust
    // A CANCEL on DELETE needs a stored schedule tag (`delete_all` in
    // groupware's calendar storage); a key account's events never get one,
    // so deleting this one cannot send anything whatever the DELETE gate does.
    let (archive, _) = raw_event(test, key1_id, "default/invite.ics").await;
    assert!(
        archive
            .unarchive::<CalendarEvent>()
            .unwrap()
            .schedule_tag
            .is_none()
    );
    key_client
        .request("DELETE", &format!("{key_cal}invite.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);

    // The DELETE gate itself (`send_itip` off for key accounts): a legacy
    // plaintext event with a schedule tag and a visible attendee is what a
    // CANCEL can be built from, so only the gate stops one here.
    plant_event(
        test,
        key1_id,
        "default",
        "planted-invite.ics",
        &KEY_INVITE.replace("za-invite-1", "za-invite-planted"),
        Some(1),
    )
    .await;
    key_client
        .request("DELETE", &format!("{key_cal}planted-invite.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    wait_for_delivery(test).await;
    assert_eq!(
        mail_count(test, plain_id).await,
        plain_mail,
        "a CANCEL left the key organizer"
    );
    assert_eq!(
        members(&plain_client, plain_inbox).await,
        Vec::<String>::new()
    );
```

(d) In the plain-organizer block, replace `wait_for_delivery(test).await;` at `:564` with:

```rust
    wait_for_delivery(test).await;
    // The invitation email itself was delivered, so the empty inbox and
    // calendar below are the ingest gate's doing, not timing.
    assert_eq!(mail_count(test, key1_id).await, key_mail + 1);
```

(e) Replace the RSVP-copy setup (`:568-579`, from the comment through the `let etag = ...` statement) with:

```rust
    // RSVP from the key attendee on the non-key organizer's page: the
    // organizer's copy records it, and the key attendee's own copy is never
    // rewritten. A copy written through DAV is sealed and shows no ATTENDEE,
    // so the attendee-copy sync could not match it anyway; a legacy
    // plaintext copy does match, which makes the attendee-copy gate the only
    // thing keeping it unchanged.
    let copy = format!("{key_cal}copy.ics");
    plant_event(test, key1_id, "default", "copy.ics", PLAIN_INVITE, None).await;
    let etag = key_client
        .request("GET", &copy, "")
        .await
        .with_status(StatusCode::OK)
        .etag()
        .to_string();
```

After the `organizer_copy` assertion, replace the ETag comparison (`:628-632`) with:

```rust
    let response = key_client
        .request("GET", &copy, "")
        .await
        .with_status(StatusCode::OK);
    assert_eq!(response.etag(), etag);
    let (archive, _) = raw_event(test, key1_id, "default/copy.ics").await;
    let stored = archive
        .unarchive::<CalendarEvent>()
        .unwrap()
        .data
        .event
        .to_string();
    assert!(
        stored.contains("PARTSTAT=NEEDS-ACTION") && !stored.contains("PARTSTAT=ACCEPTED"),
        "the key attendee's copy was rewritten: {stored}"
    );
```

(f) Replace the cleanup loop (`:637-643`) with:

```rust
    // The RSVP reply reaches the organizer's scheduling inbox directly, not
    // through the mail queue: exactly one notification.
    let replies = members(&plain_client, plain_inbox).await;
    assert_eq!(replies.len(), 1, "{replies:?}");
    for member in replies {
        plain_client
            .request("DELETE", &member, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
```

(g) After the plain organizer's DELETE (`:645-649`), keep `wait_for_delivery(test).await;`, now the shared helper. Then add before the two `members` assertions:

```rust
    // The CANCEL email was delivered too; the ingest gate dropped it.
    assert_eq!(mail_count(test, key1_id).await, key_mail + 2);
```

- [ ] **Step 3: Rewrite the checks in `za_variants::scheduling`**

Add the imports `use crate::utils::za::{mail_count, wait_for_delivery};` and `use groupware::calendar::CalendarEvent;`. Then replace the body from the PUT's `assert!(response.headers.get("schedule-tag").is_none());` (`:354`) to the end of the function with:

```rust
    assert!(response.headers.get("schedule-tag").is_none());
    wait_for_delivery(test).await;
    // Sender side: no iMIP email reached jane's mailbox. Her scheduling inbox
    // would stay empty even if john's send gate failed, because jane is a key
    // account whose own ingest gate drops invitations; the mailbox is what
    // shows john sent nothing.
    assert_eq!(mail_count(test, jane.account_id).await, jane_mail);
    let inbox = jane
        .request_with_headers(
            "PROPFIND",
            "/dav/itip/jane@example.com/inbox/",
            [("depth", "1")],
            "",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    // hrefs() includes the collection itself, so one href is an empty inbox
    // (jane's own ingest gate).
    assert_eq!(inbox.hrefs().len(), 1, "{:?}", inbox.hrefs());
    // The event is readable by its owner with attendees intact.
    let body = john
        .request("GET", "/dav/cal/john@example.com/default/s.ics", "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(body.contains("mailto:jane@example.com") && !body.contains("X-ZA-"));
    // A CANCEL on DELETE needs a stored schedule tag (`delete_all`); key
    // accounts never get one. The DELETE gate itself is tested with a
    // planted event in `za::gating::test_scheduling`.
    let (archive, _) =
        crate::za::dav_seal::raw_event(test, john.account_id, "default/s.ics").await;
    assert!(
        archive
            .unarchive::<CalendarEvent>()
            .unwrap()
            .schedule_tag
            .is_none()
    );
    john.request("DELETE", "/dav/cal/john@example.com/default/s.ics", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    wait_for_delivery(test).await;
    assert_eq!(mail_count(test, jane.account_id).await, jane_mail);
    john.delete_default_containers().await;
    jane.delete_default_containers().await;
    test.assert_is_empty().await;
}
```

Add `let jane_mail = mail_count(test, jane.account_id).await;` right after `let jane = ...` (`:343`).

- [ ] **Step 4: Run both suites green**

Run `za_tests` and then key-mode `webdav_tests` (commands in Global Constraints). Expected: both PASS. If plain-mode `webdav_tests` is affected (it is not, since `za_variants` runs only in key mode), say so.

Fallbacks:
- **Mail counts differ from (from code reading).** This covers `key_mail + 1`, `key_mail + 2`, and a sender check that never moves because iMIP mail is not stored in the mailbox.
  - Use a `trc` subscriber for `CalendarEvent::ItipMessageSent` instead. Follow the pattern in `tests/src/za/expansion.rs:108-124, 165, 197`; the events carry `AccountId`.
  - Assert that no event names the key account. As the positive control, assert that the plain organizer's invitation produced one.
  - Record this as a ruling.
- **The reply count in (f) is not 1.** Record the observed count and the reason in the report, assert the observed count, and fix the comment to say what is true.

- [ ] **Step 5: Red run for the DELETE gate**

In `crates/dav/src/calendar/delete.rs:81`, delete the line `&& !account_info.account().is_key_account()`. Run `za_tests`.

Expected: FAIL at "a CANCEL left the key organizer".

Revert with `git checkout -- crates/dav/src/calendar/delete.rs`. Write the failure line into the report.

- [ ] **Step 6: Red run for the RSVP attendee-copy gate**

In `crates/groupware/src/calendar/itip.rs:666`, change `if server` to `if false && server` in the block commented "A key account's copy is sealed". Run `za_tests`.

Expected: FAIL at "the key attendee's copy was rewritten".

Revert with `git checkout -- crates/groupware/src/calendar/itip.rs`. Write the failure line into the report.

- [ ] **Step 7: Format, check and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p tests -- --check
git diff main -- crates/   # must print nothing
git add tests/src/utils/za.rs tests/src/za/gating.rs tests/src/webdav/za_variants.rs
git commit -m "Scheduling tests: delivery barrier and checks that can fail"   # plus the trailers
```

---

### Task 2: Missing gate tests, Bearer, `CalendarEvent/copy` source and alarm recipient override (issue #13)

**Files:**
- Modify: `tests/src/za/dav_gate.rs` (Bearer case after the admin controls at `:96`)
- Modify: `tests/src/za/gating.rs` (`CalendarEvent/copy` source case after `:108`)
- Modify: `tests/src/webdav/za_variants.rs` (new `pub async fn alarm_override`)
- Modify: `tests/src/webdav/mod.rs:229-233` (run it after `za_variants::alarm` in key mode)

**Interfaces:**
- Consumes:
  - `plant_event`, `mail_count`, `queued_recipients` and `wait_for_delivery` from Task 1.
  - `Server::encode_access_token(GrantType, account_id, name, expiry_secs, claims: Option<&str>, credential_version: Option<u64>) -> trc::Result<String>` (async; used in `gating.rs:496-507`).
  - `DummyWebDavClient.credentials: String` (a pub field holding the full `Authorization` value).
- Produces: `za_variants::alarm_override(test: &TestServer)`.

Background:
- The DAV layer handles a Bearer request through the ordinary auth path. A key account's Bearer token never carries keys, and the auth cache never stores the keyless result (`crates/http/src/auth/authenticate.rs:157-189`).
- The URI gate then answers 403 (`crates/dav/src/common/za.rs:55-75`).
- No OAuth helper exists in the tests crate. Minting the token in-process gives the same token the server issues at the end of an OAuth flow.
- `CalendarEvent/copy` gates both `accountId` and `fromAccountId` (`crates/jmap/src/api/request.rs:647-648`). The existing case passes key1 for both, so the second gate is never the one that answers.

- [ ] **Step 1: Bearer test in `dav_gate.rs`**

Add `use common::auth::oauth::GrantType;`. Insert after the "Address book and principal paths are not gated." block (`:88-96`):

```rust
    // OAuth Bearer for the key account itself: the token is valid (the
    // address book answers) but carries no keys, so calendar paths refuse
    // it (spec 4.2). Minted in-process: it is the token the server issues at
    // the end of an OAuth flow, and the tests crate has no OAuth client.
    let token = test
        .server
        .encode_access_token(
            GrantType::AccessToken,
            key1_id,
            "key1@example.com",
            3600,
            None,
            None,
        )
        .await
        .unwrap();
    let mut bearer_client =
        DummyWebDavClient::new(key1_id, "key1@example.com", STRONG, "key1@example.com");
    bearer_client.credentials = format!("Bearer {token}");
    bearer_client
        .request("PROPFIND", "/dav/card/key1@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    for path in [
        "/dav/cal/key1@example.com/",
        "/dav/cal/key1@example.com/default/",
        "/dav/itip/key1@example.com/",
    ] {
        bearer_client
            .request("PROPFIND", path, "")
            .await
            .with_status(StatusCode::FORBIDDEN);
    }
    bearer_client
        .request_with_headers(
            "REPORT",
            "/dav/cal/key1@example.com/default/",
            [("depth", "1")],
            CALENDAR_QUERY,
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
    // Control: a Bearer token of the non-key account reads its own calendars.
    let plain_id = plain.id().document_id();
    let token = test
        .server
        .encode_access_token(GrantType::AccessToken, plain_id, plain.name(), 3600, None, None)
        .await
        .unwrap();
    let mut plain_bearer = DummyWebDavClient::new(
        plain_id,
        plain.name(),
        plain.secret(),
        plain.name(),
    );
    plain_bearer.credentials = format!("Bearer {token}");
    plain_bearer
        .request("PROPFIND", "/dav/cal/plain@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
```

- [ ] **Step 2: `CalendarEvent/copy` source gate in `gating.rs`**

Insert right after key1's PROPFIND of `plain_cal` (`:105-108`), while the read grant is in place:

```rust
    // `CalendarEvent/copy` gates both of its accounts. Here the target is
    // plain's account, which key1 reaches through the grant above, so only
    // the source gate (`fromAccountId` is the key account) can refuse it.
    let response = key1
        .jmap_method_call(
            "CalendarEvent/copy",
            json!({
                "accountId": plain.id_string(),
                "fromAccountId": key1.id_string(),
                "create": {}
            }),
        )
        .await;
    assert_eq!(
        method_error(&response),
        Some("accountNotSupportedByMethod"),
        "{:?}",
        response.0
    );
```

- [ ] **Step 3: `alarm_override` in `za_variants.rs`**

Add these imports: `use crate::utils::za::{mail_count, plant_event, queued_recipients, wait_for_delivery};` and `use std::time::{Duration, Instant};`. Then add:

```rust
/// A legacy plaintext event whose VALARM names an external recipient.
const R15_ALARM: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:za-r15\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:$START\r\nDURATION:PT1H\r\nSUMMARY:r15-canary\r\nBEGIN:VALARM\r\nTRIGGER:-P2S\r\nACTION:EMAIL\r\nATTENDEE:mailto:r15-external@unknown.com\r\nSUMMARY:r15-canary\r\nDESCRIPTION:r15-canary\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

/// R15: a key account's alarm email goes to the account's own address even
/// when the VALARM names an external ATTENDEE and the server allows external
/// alarm recipients (key mode sets `allow_external_rcpts`). Sealing hides a
/// VALARM's ATTENDEE in every event written with keys, so only a legacy
/// plaintext event, planted here, reaches the override in the alarm task.
pub async fn alarm_override(test: &TestServer) {
    println!("Running key-account alarm recipient override test...");
    let account = test.account("john@example.com");
    let client = account.webdav_client();
    let id = client.account_id;
    let cal = "/dav/cal/john%40example.com/r15/";
    client
        .request(
            "MKCALENDAR",
            cal,
            "<?xml version=\"1.0\" encoding=\"utf-8\" ?><A:mkcalendar xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"/>",
        )
        .await
        .with_status(StatusCode::CREATED);
    let start = DateTime::from_timestamp(now() as i64 + 5)
        .to_rfc3339()
        .replace(['-', ':'], "");
    plant_event(
        test,
        id,
        "r15",
        "r15.ics",
        &R15_ALARM.replace("$START", &start),
        None,
    )
    .await;

    // The alarm fires two seconds before the start.
    let deadline = Instant::now() + Duration::from_secs(15);
    while mail_count(test, id).await == 0 {
        assert!(
            Instant::now() < deadline,
            "no alarm email reached the account; queued for {:?}",
            queued_recipients(test).await
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    wait_for_delivery(test).await;
    let messages = test.server.get_cached_messages(id).await.unwrap();
    assert_eq!(messages.emails.items.len(), 1);
    let contents = test
        .fetch_email(id, messages.emails.items[0].document_id)
        .await;
    let message = MessageParser::new().parse(&contents).unwrap();
    let to = message
        .to()
        .and_then(|t| t.first())
        .and_then(|a| a.address())
        .unwrap_or_default();
    assert_eq!(to, "john@example.com", "recipient is the account address");
    assert!(
        !String::from_utf8_lossy(&contents).contains("r15-external"),
        "the external alarm attendee appears in the email"
    );

    test.wait_for_tasks().await;
    client
        .request("DELETE", &format!("{cal}r15.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    client
        .request("DELETE", cal, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    test.destroy_all_mailboxes(account).await;
    test.assert_is_empty().await
}
```

In `tests/src/webdav/mod.rs`, add `za_variants::alarm_override(&test).await;` in the key-mode branch, right after `za_variants::alarm(&test).await;`.

- [ ] **Step 4: Run both suites green**

Run `za_tests`, then key-mode `webdav_tests`. Expected: both PASS.

Fallback for Step 2: if the copy call answers `forbidden` or another access error, key1 does not reach plain's `CalendarEvent` collection through the DAV grant.
- Use the administrator instead, an impersonating member of every account (ruling R4 in the plan 3 outcome): `admin.jmap_method_call(...)` with `"accountId": admin.id_string()` and `"fromAccountId": key1.id_string()`.
- Record this as a ruling.

- [ ] **Step 5: Red runs**

Do these one at a time, reverting each with `git checkout -- <file>` before the next. Write each failure line into the report.
1. **Bearer.** In `crates/dav/src/common/za.rs` `za_session_keys`, make the `None =>` arm return `Ok(None)` instead of the `FORBIDDEN` error. That mutation first breaks the master-user and admin PROPFIND assertions earlier in `dav_gate.rs` (`:64-77`), so for this run only, comment those out too. Run `za_tests`. Expected: FAIL in `dav_gate.rs` at the first Bearer PROPFIND on `/dav/cal/key1@example.com/` (got 207). Revert both files.
2. **Copy source.** Delete `crates/jmap/src/api/request.rs:648` (`za_assert_calendar_allowed(self, req.from_account_id).await?;`). Run `za_tests`. Expected: FAIL at the new copy assertion (a method result instead of the error).
3. **Alarm override.** In `crates/services/src/task_manager/alarm.rs`, in the `if generic { ... }` clearing block, delete `rcpt_to = None;`. Run key-mode `webdav_tests`. Expected: FAIL with "no alarm email reached the account; queued for [\"r15-external@unknown.com\"]".

- [ ] **Step 6: Format, check and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p tests -- --check
git diff main -- crates/   # must print nothing
git add tests/src/za/dav_gate.rs tests/src/za/gating.rs tests/src/webdav/za_variants.rs tests/src/webdav/mod.rs
git commit -m "Gate tests: OAuth Bearer, CalendarEvent/copy source, alarm recipient override"   # plus the trailers
```

---

### Task 3: Sealed-event cases: tampered records, conditional requests, legacy events, revocation (issue #15, part 1)

**Files:**
- Modify: `tests/src/za/dav_seal.rs` (`test_reports` tamper block at `:392-413`; new `pub async fn test_conditional` and `pub async fn test_legacy`)
- Modify: `tests/src/za/dav_gate.rs` (revocation assertions after the grant is dropped at `:266-273`)
- Modify: `tests/src/za/mod.rs` (run `dav_seal::test_conditional` and `dav_seal::test_legacy` after `dav_seal::test_collections`)

**Interfaces:**
- Consumes:
  - `plant_event` from Task 1.
  - `raw_event`, `EVENT`, `CANARIES`, `CONTENT_TYPE` and `parse` (`dav_seal.rs`).
  - `groupware::calendar::seal::archived_event_is_sealed(&ArchivedCalendarEvent) -> bool`.
- Produces: `dav_seal::test_conditional(test: &mut TestServer)` and `dav_seal::test_legacy(test: &mut TestServer)`.

Background:
- GET and HEAD share one handler. That handler and a PUT over an existing event both unseal the stored record before `validate_headers` runs (`crates/dav/src/calendar/get.rs:95-108`, `crates/dav/src/calendar/update.rs:160-167`). So a tampered record answers 500 whatever the conditional headers say.
- Conditional outcomes come from upstream's `validate_headers` (`crates/dav/src/common/lock.rs:339-616`). The plain account is the oracle for them.
- A PUT whose body equals the stored plaintext takes the no-change shortcut (`update.rs:199-202`) and does not seal. Only a changed body seals.

- [ ] **Step 1: Tampered HEAD and PUT in `test_reports`**

Right after the tampered GET's 500 (`:410-413`), insert:

```rust
    // HEAD shares GET's handler, and a PUT over an existing event unseals it
    // to compare: both fail the same way, before any conditional header is
    // looked at, and the failed PUT leaves the record as it was.
    let (tampered, _) = raw_event(test, id, "default/report-1.ics").await;
    client
        .request("HEAD", path, "")
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    client
        .request_with_headers(
            "PUT",
            path,
            [CONTENT_TYPE],
            EVENT.replace("summary-canary", "tampered-put-canary"),
        )
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    let (after, _) = raw_event(test, id, "default/report-1.ics").await;
    assert_eq!(after.as_bytes(), tampered.as_bytes(), "a failed PUT wrote");
```

- [ ] **Step 2: Conditional requests, compared with a plain account**

Add to `dav_seal.rs`:

```rust
/// What the conditional cases answer, from code reading of upstream's
/// `validate_headers`; the plain account is the oracle.
const CONDITIONAL_EXPECTED: [(&str, StatusCode); 8] = [
    ("GET If-None-Match current", StatusCode::NOT_MODIFIED),
    ("GET If-None-Match *", StatusCode::NOT_MODIFIED),
    ("GET If-Match current", StatusCode::OK),
    ("GET If-Match stale", StatusCode::PRECONDITION_FAILED),
    ("PUT If-None-Match * existing", StatusCode::PRECONDITION_FAILED),
    ("PUT If-Match stale", StatusCode::PRECONDITION_FAILED),
    ("PUT If-Match current", StatusCode::NO_CONTENT),
    ("PUT If-None-Match * new", StatusCode::CREATED),
];

/// Runs the conditional cases against `path` (which holds `EVENT`) and a
/// not-yet-existing `fresh` path; returns each case's status.
async fn conditional_statuses(
    client: &DummyWebDavClient,
    path: &str,
    fresh: &str,
) -> Vec<(&'static str, StatusCode)> {
    let etag = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .etag()
        .to_string();
    let stale = "\"stale-etag\"";
    let changed = EVENT.replace("summary-canary", "conditional-canary");
    let mut out = Vec::new();
    for (case, header, value) in [
        ("GET If-None-Match current", "if-none-match", etag.as_str()),
        ("GET If-None-Match *", "if-none-match", "*"),
        ("GET If-Match current", "if-match", etag.as_str()),
        ("GET If-Match stale", "if-match", stale),
    ] {
        let status = client
            .request_with_headers("GET", path, [(header, value)], "")
            .await
            .status;
        out.push((case, status));
    }
    for (case, header, value) in [
        ("PUT If-None-Match * existing", "if-none-match", "*"),
        ("PUT If-Match stale", "if-match", stale),
        ("PUT If-Match current", "if-match", etag.as_str()),
    ] {
        let status = client
            .request_with_headers("PUT", path, [CONTENT_TYPE, (header, value)], changed.clone())
            .await
            .status;
        out.push((case, status));
    }
    // A UID of its own: a second event with `EVENT`'s UID in the same
    // calendar is refused (412 no-uid-conflict) before the condition is
    // looked at.
    let status = client
        .request_with_headers(
            "PUT",
            fresh,
            [CONTENT_TYPE, ("if-none-match", "*")],
            EVENT.replace("za-event-1", "za-cond-new"),
        )
        .await
        .status;
    out.push(("PUT If-None-Match * new", status));
    out
}

/// 304, If-Match and If-None-Match on sealed events answer exactly as on
/// ordinary ones: they compare against the stored record's ETag.
pub async fn test_conditional(test: &mut TestServer) {
    println!("Running zero-access conditional request tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let key_client = DummyWebDavClient::new(id, name, STRONG, name);
    let plain = test.account("plain@example.com").clone();
    let plain_client = DummyWebDavClient::new(
        plain.id().document_id(),
        plain.name(),
        plain.secret(),
        plain.name(),
    );
    let mut results = Vec::new();
    for (client, base) in [
        (&key_client, "/dav/cal/key1@example.com/default/"),
        (&plain_client, "/dav/cal/plain@example.com/default/"),
    ] {
        let path = format!("{base}cond.ics");
        client
            .request_with_headers("PUT", &path, [CONTENT_TYPE], EVENT)
            .await
            .with_status(StatusCode::CREATED);
        results.push(conditional_statuses(client, &path, &format!("{base}cond-new.ics")).await);
    }
    assert_eq!(results[0], results[1], "sealed events answer differently");
    assert_eq!(results[1], CONDITIONAL_EXPECTED.to_vec(), "plain oracle");

    // The If-Match write was a real write: sealed, with a fresh envelope.
    let (archive, _) = raw_event(test, id, "default/cond.ics").await;
    assert!(groupware::calendar::seal::archived_event_is_sealed(
        archive.unarchive::<CalendarEvent>().unwrap()
    ));
    assert!(!String::from_utf8_lossy(archive.as_bytes()).contains("conditional-canary"));

    test.wait_for_tasks().await;
    for (client, base) in [
        (&key_client, "/dav/cal/key1@example.com/default/"),
        (&plain_client, "/dav/cal/plain@example.com/default/"),
    ] {
        for name in ["cond.ics", "cond-new.ics"] {
            client
                .request("DELETE", &format!("{base}{name}"), "")
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
    }
    // `plain` outlives this module: drop the calendar its PUT created.
    plain_client
        .request("DELETE", "/dav/cal/plain@example.com/default", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}
```

The `PUT If-None-Match * new` case must be 201 for both accounts; that value is not open to the oracle fallback, since a 412 there means the case tested a UID conflict instead.

If any other entry of `results[1]` differs from `CONDITIONAL_EXPECTED`, the code reading was wrong. Replace the expected values with the plain account's statuses and record a ruling. `results[0] == results[1]` must hold either way.

- [ ] **Step 3: Legacy plaintext event**

Add to `dav_seal.rs`. Import `crate::utils::za::plant_event`.

```rust
/// Spec 7: an event stored before the account held keys is read as it is,
/// and the next write that changes it seals it.
pub async fn test_legacy(test: &mut TestServer) {
    println!("Running zero-access legacy event tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let path = "/dav/cal/key1@example.com/default/legacy.ics";
    let legacy = EVENT.replace("za-event-1", "za-legacy-1");
    plant_event(test, id, "default", "legacy.ics", &legacy, None).await;
    let is_sealed = |archive: &Archive<AlignedBytes>| {
        groupware::calendar::seal::archived_event_is_sealed(
            archive.unarchive::<CalendarEvent>().unwrap(),
        )
    };
    let (archive, _) = raw_event(test, id, "default/legacy.ics").await;
    assert!(!is_sealed(&archive));

    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(!body.contains("X-ZA-"), "{body}");
    assert_eq!(parse(&body), parse(&legacy), "legacy event read as stored");

    // An unchanged PUT is the no-change shortcut: no write, still plaintext.
    client
        .request_with_headers("PUT", path, [CONTENT_TYPE], legacy.clone())
        .await
        .with_status(StatusCode::NO_CONTENT);
    let (archive, _) = raw_event(test, id, "default/legacy.ics").await;
    assert!(!is_sealed(&archive));

    // A changed PUT seals it.
    let changed = legacy.replace("summary-canary", "legacy-rewritten-canary");
    client
        .request_with_headers("PUT", path, [CONTENT_TYPE], changed)
        .await
        .with_status(StatusCode::NO_CONTENT);
    let (archive, _) = raw_event(test, id, "default/legacy.ics").await;
    assert!(is_sealed(&archive));
    let raw = String::from_utf8_lossy(archive.as_bytes()).to_string();
    for canary in CANARIES.iter().chain(&["legacy-rewritten-canary"]) {
        assert!(!raw.contains(canary), "{canary} left in the clear");
    }
    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(body.contains("legacy-rewritten-canary") && !body.contains("X-ZA-"));

    test.wait_for_tasks().await;
    client
        .request("DELETE", path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}
```

- [ ] **Step 4: Revocation in `dav_gate.rs`**

Insert right after the `acl(..., [])` call that drops the grant (`:266-273`):

```rust
    // The revocation takes effect at once: key1 can no longer read or write
    // plain's calendar (403, as for a user who never had a grant).
    key_client
        .request("GET", "/dav/cal/plain@example.com/default/x.ics", "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    key_client
        .request_with_headers(
            "PUT",
            "/dav/cal/plain@example.com/default/revoked.ics",
            [("content-type", "text/calendar")],
            TEST_ICAL_2,
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
```

The 403 is from code reading. If the run shows 404 for both, a denial upstream also gives in that case, assert 404 and record a ruling. Any 2xx is a failure to report.

- [ ] **Step 5: Wire up and run**

In `tests/src/za/mod.rs`, add after `dav_seal::test_collections(&mut test).await;`:

```rust
    dav_seal::test_conditional(&mut test).await;
    dav_seal::test_legacy(&mut test).await;
```

Run `za_tests`. Expected: PASS.

- [ ] **Step 6: Red run for revocation**

Comment out the `acl(..., [])` revocation call (`dav_gate.rs:266-273`) and run `za_tests`. Expected: FAIL at the new GET (`Expected 403 Forbidden but got 200 OK`). Restore the call and write the failure line into the report.

The tampered, conditional and legacy cases pin existing behaviour and have no gate to remove. For those, the oracle is the plain account and the stored-record checks, and the report says so.

- [ ] **Step 7: Format, check and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p tests -- --check
git diff main -- crates/   # must print nothing
git add tests/src/za/dav_seal.rs tests/src/za/dav_gate.rs tests/src/za/mod.rs
git commit -m "Sealed-event tests: tampered HEAD and PUT, conditional requests, legacy events, revocation"   # plus the trailers
```

---

### Task 4: Sealed collection and PROPPATCH cases, and the custom timezone in a time-range REPORT (issue #15, part 2)

**Files:**
- Modify: `tests/src/za/dav_seal.rs` (`test_collections`, `:468-784`; new helpers `time_range_query` and `reports_floating`)

**Interfaces:**
- Consumes: `raw_calendar`, `raw_event`, `CANARIES`, `EVENT` and `mkcalendar_body` (`dav_seal.rs`); `archived_event_is_sealed`; the `tz` string that `test_collections` builds at `:532-543` (US-Eastern custom timezone with canary names).
- Produces: none for other tasks.

Background:
- PROPPATCH removes a description by clearing a preference. It removes a colour as a dead property. Then it reseals the whole bundle (`crates/dav/src/calendar/proppatch.rs:558-585`, `crates/groupware/src/calendar/seal/collection.rs:101-125`).
- A creationdate-only PROPPATCH still runs `seal_event`/`seal_calendar`.
- PROPPATCH never touches `size`; only PUT sets it.
- A calendar-query uses the calendar's timezone from the DAV resource cache. The cache is built from the stored (sealed) record, whose VTIMEZONE keeps its calculation rules visible.

- [ ] **Step 1: Description and colour removal**

After the "Clearing the last ordinary value keeps the timezone readable." block (`:588-604`), insert:

```rust
    // The cleared values are gone, and the record stays sealed.
    let gone = client
        .propfind(cal, ["A:calendar-description", "C:calendar-color"])
        .await;
    gone.properties(cal)
        .get("A:calendar-description")
        .with_status(StatusCode::NOT_FOUND);
    gone.properties(cal)
        .get("calendar-color")
        .with_status(StatusCode::NOT_FOUND);
    let archive = raw_calendar(test, id, "work").await;
    let raw = String::from_utf8_lossy(archive.as_bytes()).to_string();
    assert!(raw.contains("$za$"), "no collection marker");
    for canary in collection_canaries {
        assert!(!raw.contains(canary), "{canary} in the stored collection");
    }
```

The 404 statuses are from code reading. If the run reports the cleared properties some other way, run the same MKCALENDAR, PROPPATCH and PROPFIND sequence on a plain calendar (`/dav/cal/plain%40example.com/colclear/`, with `plain_client` from Step 4) and assert the key result equals the plain one. Record that as a ruling.

- [ ] **Step 2: Creationdate-only PROPPATCH on the collection**

Right after Step 1's block:

```rust
    // A PROPPATCH that sets only creationdate still writes a sealed bundle.
    client
        .proppatch(cal, [("D:creationdate", "2000-01-01T00:00:00Z")], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let archive = raw_calendar(test, id, "work").await;
    let raw = String::from_utf8_lossy(archive.as_bytes()).to_string();
    for canary in collection_canaries {
        assert!(!raw.contains(canary), "{canary} in the stored collection");
    }
    let stored = archive.unarchive::<Calendar>().unwrap();
    assert!(stored.preferences(id).name.starts_with("$za$"));
    assert_eq!(stored.created.to_native(), 946684800);
    client
        .propfind(cal, ["D:displayname"])
        .await
        .properties(cal)
        .get("D:displayname")
        .with_values(["Work displayname-canary"]);
```

- [ ] **Step 3: Event PROPPATCH keeps `size`; creationdate-only PROPPATCH on the event**

In the event PROPPATCH block, right after `let stored = archive.unarchive::<CalendarEvent>().unwrap();` (`:671`), add:

```rust
    assert_eq!(
        stored.size.to_native() as usize,
        EVENT.len(),
        "PROPPATCH keeps the stored size"
    );
```

After that block's GET assertion (`:681-692`), add:

```rust
    // A PROPPATCH that sets only creationdate still seals the event, and the
    // extra properties written above survive it.
    client
        .proppatch(path, [("D:creationdate", "2000-01-01T00:00:00Z")], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let (archive, _) = raw_event(test, id, "work/evt.ics").await;
    let raw = String::from_utf8_lossy(archive.as_bytes()).to_string();
    for canary in CANARIES.iter().chain(&["evtname-canary", "dead-canary"]) {
        assert!(!raw.contains(canary), "{canary} leaked into the stored event");
    }
    let stored = archive.unarchive::<CalendarEvent>().unwrap();
    assert!(groupware::calendar::seal::archived_event_is_sealed(stored));
    assert_eq!(stored.created.to_native(), 946684800);
    assert_eq!(stored.size.to_native() as usize, EVENT.len());
    client
        .propfind(path, ["D:displayname"])
        .await
        .properties(path)
        .get("D:displayname")
        .with_values(["evtname-canary"]);
```

- [ ] **Step 4: Custom timezone in a time-range REPORT, compared with a plain account**

Add these helpers to `dav_seal.rs`:

```rust
fn time_range_query(start: &str, end: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\" ?><C:calendar-query xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:prop><D:getetag/></D:prop><C:filter><C:comp-filter name=\"VCALENDAR\"><C:comp-filter name=\"VEVENT\"><C:time-range start=\"{start}\" end=\"{end}\"/></C:comp-filter></C:comp-filter></C:filter></C:calendar-query>"
    )
}

/// Whether a depth-1 calendar-query on `cal` returns `floating.ics`.
async fn reports_floating(client: &DummyWebDavClient, cal: &str, query: &str) -> bool {
    let response = client
        .request_with_headers("REPORT", cal, [("depth", "1")], query)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    response
        .hrefs()
        .iter()
        .any(|href| href.ends_with("/floating.ics"))
}

/// A floating event: 23:00 local on 10 January 2099.
const FLOATING: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:za-floating-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990110T230000\r\nDTEND:20990110T233000\r\nSUMMARY:floating\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
```

In `test_collections`, after Step 2's block (`work` still has the custom timezone from `:545`), insert:

```rust
    // Time-range REPORT in a calendar with a sealed custom timezone: the
    // query reads the timezone's rules from the stored record, so a floating
    // event lands where it does on an ordinary calendar with the same
    // timezone. 23:00 floating is 04:00Z the next day in US-Eastern (UTC-5
    // in January) and 23:00Z if the timezone were lost.
    //
    // The query first drops events by the range cached at write time, which
    // reads floating times as UTC (23:00Z), before any timezone is applied
    // (`is_resource_in_time_range`). So the positive window spans both the
    // cached 23:00Z interval and the Eastern 04:00Z one; the narrow UTC
    // window passes that prefilter and is then decided by the timezone. A
    // lost timezone would give (true, true).
    let plain = test.account("plain@example.com").clone();
    let plain_client = DummyWebDavClient::new(
        plain.id().document_id(),
        plain.name(),
        plain.secret(),
        plain.name(),
    );
    let plain_cal = "/dav/cal/plain%40example.com/tz/";
    plain_client
        .request("MKCALENDAR", plain_cal, mkcalendar_body(&[]))
        .await
        .with_status(StatusCode::CREATED);
    plain_client
        .proppatch(plain_cal, [("A:calendar-timezone", tz.as_str())], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    for (client, cal) in [(&client, cal), (&plain_client, plain_cal)] {
        client
            .request_with_headers("PUT", &format!("{cal}floating.ics"), [CONTENT_TYPE], FLOATING)
            .await
            .with_status(StatusCode::CREATED);
    }
    let eastern = time_range_query("20990110T223000Z", "20990111T043000Z");
    let utc = time_range_query("20990110T223000Z", "20990110T233000Z");
    let key = (
        reports_floating(&client, cal, &eastern).await,
        reports_floating(&client, cal, &utc).await,
    );
    let ordinary = (
        reports_floating(&plain_client, plain_cal, &eastern).await,
        reports_floating(&plain_client, plain_cal, &utc).await,
    );
    assert_eq!(key, ordinary, "the sealed timezone changes time-range results");
    assert_eq!(ordinary, (true, false), "the calendar timezone places the event");
    test.wait_for_tasks().await;
    for (client, cal) in [(&client, cal), (&plain_client, plain_cal)] {
        client
            .request("DELETE", &format!("{cal}floating.ics"), "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    plain_client
        .request("DELETE", plain_cal, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
```

If `mkcalendar_body(&[])` does not build an empty MKCALENDAR, use the literal body from `gating.rs:551`.

If `ordinary` is not `(true, false)`, report it with both windows and the plain calendar's REPORT body. Do not move the windows at run time: a pair that agrees, such as `(false, false)` or `(true, true)`, means the case no longer shows the timezone being applied. `key == ordinary` must hold either way.

- [ ] **Step 5: Run and commit**

Run `za_tests`. Expected: PASS.

These cases pin existing behaviour and have no gate to remove. The oracles are the stored-record checks and the plain-account comparison, and the report says so.

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p tests -- --check
git diff main -- crates/   # must print nothing
git add tests/src/za/dav_seal.rs
git commit -m "Sealed collection tests: cleared properties, creationdate, size, timezone REPORT"   # plus the trailers
```

---

### Task 5: WebSocket opened before its account becomes a key account (issue #15, last bullet)

**Files:**
- Modify: `tests/src/za/tracing.rs` (split `ws_exchange` at `:277-339` into `ws_connect`, `ws_call` and `ws_close`, all `pub(super)`; `ws_exchange` stays and calls them)
- Create: `tests/src/za/websocket.rs`
- Modify: `tests/src/za/mod.rs` (add `pub mod websocket;`, and run `websocket::test` after `gating::test_scheduling`)

**Interfaces:**
- Consumes:
  - `za_setup_token(admin, name) -> String` and `za_setup(name, token, password) -> String` (`tests/src/utils/za.rs:146,153`).
  - `Account::create_passwordless_user_account(name, secret, description, aliases, permissions)`.
  - `super::user_permissions()`.
  - The master-user login form `"<account>%<admin name>"` with the admin's secret (`dav_gate.rs:54-55`).
- Produces:
  - `pub(super) type WsStream`
  - `pub(super) async fn ws_connect(user: &str, secret: &str) -> WsStream`
  - `pub(super) async fn ws_call(stream: &mut WsStream, message: &str) -> String`
  - `pub(super) async fn ws_close(stream: WsStream)`

Background: issue #15 says the key-account check is made per connection. The code does not do that:
- The upgrade drops session keys and keeps the token for the socket's lifetime (`crates/http/src/request.rs:288-305`).
- Every calendar method looks the account up live (`za_assert_calendar_allowed`, `crates/jmap/src/api/request.rs:735-743`).

So this test pins per-call behaviour: the open socket survives the conversion, and its next calendar call is refused. A passwordless account is the only kind that can be converted (setup-token answers 409 for an account with credentials), so the socket is opened through a master-user login.

- [ ] **Step 1: Split the WebSocket client in `tracing.rs`**

Replace `ws_exchange` (`:277-339`) with:

```rust
pub(super) type WsStream =
    BufReader<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>;

/// Open a JMAP WebSocket with Basic credentials.
pub(super) async fn ws_connect(user: &str, secret: &str) -> WsStream {
    // body: the current `ws_exchange` lines from building the rustls config
    // through skipping the response headers, unchanged; then `stream`
}

/// Send `message` as one text frame and return the server's text reply,
/// answering pings meanwhile.
pub(super) async fn ws_call(stream: &mut WsStream, message: &str) -> String {
    // body: the current `ws_send(&mut stream, 0x1, ...)` and reply loop,
    // unchanged apart from `stream` already being `&mut`
}

/// Close handshake: wait for the server's close frame, then hang up.
pub(super) async fn ws_close(mut stream: WsStream) {
    // body: the current close-handshake lines, unchanged
}

/// Open a JMAP WebSocket, send `message` as one text frame, return the
/// server's text reply and close the socket. A minimal client, since
/// `jmap_client` cannot send a malformed message.
async fn ws_exchange(user: &str, secret: &str, message: &str) -> String {
    let mut stream = ws_connect(user, secret).await;
    let reply = ws_call(&mut stream, message).await;
    ws_close(stream).await;
    reply
}
```

Each `// body:` line names lines that move verbatim from the current `ws_exchange`. Move them without editing them; the code already reviewed there stays as it is.

- [ ] **Step 2: Write `tests/src/za/websocket.rs`**

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! A WebSocket opened before its account became a key account. The socket
//! stays open; the key-account check runs on every method call, so the next
//! calendar call on it is refused as it would be over HTTP (spec 9).

use super::{
    STRONG,
    tracing::{ws_call, ws_close, ws_connect},
    user_permissions,
};
use crate::utils::{
    server::TestServer,
    za::{za_setup, za_setup_token},
};
use serde_json::{Value, json};

fn calendar_get(account_id: &str) -> String {
    json!({
        "@type": "Request",
        "id": "1",
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:calendars"],
        "methodCalls": [["Calendar/get", { "accountId": account_id }, "c0"]]
    })
    .to_string()
}

/// The first method response's name and arguments.
fn first_response(reply: &str) -> (String, Value) {
    let reply: Value = serde_json::from_str(reply).unwrap();
    let call = &reply["methodResponses"][0];
    (
        call[0].as_str().unwrap_or_default().to_string(),
        call[1].clone(),
    )
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access WebSocket conversion test...");
    let admin = test.account("admin@example.com").clone();
    let mut account = admin
        .create_passwordless_user_account(
            "ws1@example.com",
            STRONG,
            "WebSocket User",
            &[],
            user_permissions(),
        )
        .await;
    let account_id = account.id_string();
    // A passwordless account has no login of its own before setup: open
    // the socket as the administrator acting as the account.
    let master = format!("ws1@example.com%{}", admin.name());
    let mut socket = ws_connect(&master, admin.secret()).await;

    let (name, args) = first_response(&ws_call(&mut socket, &calendar_get(&account_id)).await);
    assert_eq!(name, "Calendar/get", "{args}");

    // `setup-token` makes the account a key account at once.
    let token = za_setup_token(&admin, "ws1@example.com").await;
    assert!(
        test.server
            .account(account.id().document_id())
            .await
            .unwrap()
            .is_key_account()
    );

    // Same socket, same token: the calendar call is now refused.
    let (name, args) = first_response(&ws_call(&mut socket, &calendar_get(&account_id)).await);
    assert_eq!(
        (name.as_str(), args["type"].as_str()),
        ("error", Some("accountNotSupportedByMethod")),
        "{args}"
    );
    ws_close(socket).await;

    // Finish setup so the suite's key-account teardown destroys it.
    account.recovery_key = Some(za_setup("ws1@example.com", &token, STRONG).await);
    test.insert_account(account);
}
```

Wire it up in `mod.rs`: add `pub mod websocket;` in alphabetical order, and add `websocket::test(&mut test).await;` right after `gating::test_scheduling(&mut test).await;`.

Expected outcomes, from code reading:
- If the master-user upgrade answers anything but `101`, report BLOCKED with the status line. Do not switch to a different account kind.
- If `za_setup` answers 409 because the first `Calendar/get` created calendar data beyond the tolerated default calendar, report the response.

- [ ] **Step 3: Run green, then red**

Run `za_tests`. Expected: PASS, including the unchanged tracing WebSocket cases.

For the red run, make `za_assert_calendar_allowed` (`crates/jmap/src/api/request.rs:737-743`) always return `Ok(())`. Run `za_tests`.

Expected: the first failure is in `gating::test`, which runs earlier. To reach this module, also comment out the `gating::test(&mut test).await;` line in `mod.rs`. Expected then: FAIL at the conversion assertion in `websocket.rs` (`Calendar/get` answered).

Revert both edits (`git checkout -- crates/jmap/src/api/request.rs`, and restore the `mod.rs` line). Write the failure line into the report.

- [ ] **Step 4: Format, check and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p tests -- --check
git diff main -- crates/   # must print nothing
git add tests/src/za/tracing.rs tests/src/za/websocket.rs tests/src/za/mod.rs
git commit -m "WebSocket test: an account converted to a key account while its socket is open"   # plus the trailers
```

---

### Task 6: Leak scanner reads blobs through the configured blob store (issue #16)

**Files:**
- Modify: `tests/src/za/leak.rs` (`scan`, `:295-345`; the guard comment at `:656-659`)
- Modify: `.github/workflows/test.yml` (job `zero-access`: a step that runs `za_tests` with `BLOB_STORE: FileSystem`)

**Interfaces:**
- Consumes:
  - `test.server.blob_store() -> &BlobStore` (`crates/common/src/storage/mod.rs:47`) and `BlobStore::get_blob(&[u8], Range) -> trc::Result<Option<Vec<u8>>>`.
  - `SUBSPACE_BLOBS` and `SUBSPACE_BLOB_LINK` (already in `leak.rs`'s `SUBSPACES`).
  - `types::blob_hash::BLOB_HASH_LEN` (32).
- Produces: nothing new. `Scan.blobs` keeps its meaning: the number of decoded blobs.

Background:
- `scan` decodes blobs only from the data store's blob subspace, through a hard-coded `BlobStore::Store(store)` (`leak.rs:299`). With a filesystem or S3 blob store that subspace is empty.
- Every blob a document or upload holds is named by a link record in `SUBSPACE_BLOB_LINK`, whose key starts with the 32-byte blob hash (`crates/store/src/write/key.rs:284-297`).
- Reading each linked hash through the configured blob store covers the data store, the filesystem and S3 the same way.
- Orphan blobs waiting for purge on a filesystem store have no link and stay unscanned. The outcome note records that gap.
- `BLOB_STORE=FileSystem` is already supported by the test builder (`tests/src/utils/storage.rs:36-57`, `:118-148`). Blob files then live under the suite's temp directory, which the server wipes at start.

- [ ] **Step 1: Red run**

Run the filesystem command from Global Constraints (`BLOB_STORE=FileSystem ... za_tests`) on the unchanged scanner.

Expected: FAIL at `leak.rs:662` with `no blob decoded`. Any earlier failure is a filesystem-store problem outside the scanner; report it as BLOCKED with the failure.

- [ ] **Step 2: Decode linked blobs through the configured store**

In `scan`:
- Delete `let blob_store = BlobStore::Store(store.clone());`.
- Add `let mut blob_hashes: Vec<Vec<u8>> = Vec::new();` before the subspace loop.
- Replace the per-record decode block ("Blobs are stored compressed; decode them the way readers do.", `:329-339`) with:

```rust
            // Blob contents are decoded after the walk, through the server's
            // configured blob store: the data store's own blob subspace, or a
            // filesystem or S3 store whose blobs only the link records name
            // (a link key starts with the blob hash).
            if subspace == SUBSPACE_BLOBS {
                blob_hashes.push(key.clone());
            } else if subspace == SUBSPACE_BLOB_LINK && key.len() >= BLOB_HASH_LEN {
                blob_hashes.push(key[..BLOB_HASH_LEN].to_vec());
            }
```

Right after the `for &subspace in SUBSPACES { ... }` loop ends, add:

```rust
    // Blobs are stored compressed; decode them the way readers do.
    blob_hashes.sort_unstable();
    blob_hashes.dedup();
    for hash in blob_hashes {
        if let Some(blob) = test
            .server
            .blob_store()
            .get_blob(&hash, 0..usize::MAX)
            .await
            .ok()
            .flatten()
        {
            scan.blobs += 1;
            let what = format!("blob {hash:?}");
            find_canaries(&blob, &what, "decoded blob", &mut scan.violations);
        }
    }
```

Remove the now-unused `BlobStore` import if nothing else uses it, and import `types::blob_hash::BLOB_HASH_LEN`. In the guard comment at `:656-659`, replace the parenthetical about a split blob store with "(read through the configured blob store, so a filesystem store is covered too)".

- [ ] **Step 3: Run green in both modes**

Run the filesystem command, then the plain `za_tests` command. Expected: both PASS. The scanner's printed summary line shows a non-zero blob count in both.

- [ ] **Step 4: CI step**

In `.github/workflows/test.yml`, job `zero-access`, add this step right after "Zero-access Tests":

```yaml
      - name: Zero-access Tests (filesystem blob store)
        # The leak scanner reads blobs through the configured blob store;
        # this run keeps blobs outside the data store. The test server wipes
        # its data directory, blob files included, at start.
        run: cargo test -p tests za::za_tests -- --nocapture
        env:
          BLOB_STORE: FileSystem
```

- [ ] **Step 5: Format, check and commit**

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p tests -- --check
git diff main -- crates/   # must print nothing
git add tests/src/za/leak.rs .github/workflows/test.yml
git commit -m "Leak scanner: decode blobs through the configured blob store, run za_tests on a filesystem store in CI"   # plus the trailers
```

---

### Task 7: Mutation runs and outcome notes (issue #12, and the notes for #13–#16)

**Files:**
- Modify (temporarily, reverted after each run): the product files named in the table below.
- Modify: `docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md` (new section "Mutation runs (plan 7)"; mark items closed)
- Modify: `docs/superpowers/plans/2026-10-06-zero-access-plan2-outcome.md` (mark the "Tests worth adding" items at `:244-255`)
- Modify: `CLAUDE.md` (the za suite description and the CI description; leave the plan status line, which is updated at merge)
- Possibly modify: test files, only to add a test for a mutation that no suite catches (Step 3)

**Interfaces:**
- Consumes: every test from Tasks 1–6.
- Produces: the mutation table in the plan 3 outcome note.

How a run works:
1. Apply exactly the edit in the table.
2. Run `za_tests`, then key-mode `webdav_tests`, never at the same time.
3. For each suite, record the first failure as `file:line` plus the panic message, or "passed".
4. Revert with `git checkout -- <file>` and confirm `git diff main -- crates/` is empty before the next mutation.

A suite stops at its first panic, so record only that first failure. A `cal_itip` failure in `webdav_tests` gets one rerun before it counts.

| Id | Gate | Edit | Expected first failure (from code reading) |
|----|------|------|-------------------------------------------|
| M1 | Cross-account COPY/MOVE | `crates/dav/src/common/za.rs`, `za_refuse_cross_account`: prefix the condition with `false &&` | za: `dav_gate.rs` COPY 403; key webdav: `za_variants::copy_move`, a jane direction |
| M2 | ACL on key calendars | `crates/dav/src/common/acl.rs:130`: `if false && collection == ...` | za: `gating.rs:96-99`; key webdav: `za_variants::acl`, first ACL assertion |
| M3 | DAV URI gate | `crates/dav/src/common/uri.rs:105-115`: remove the `za_session_keys` call in the calendar arm | za: `dav_gate.rs` master-user PROPFIND; key webdav: `za_variants::acl` jane PROPFIND, or passed (ordinary permissions answer 403 too) |
| M4 | Keyless token refused | `crates/dav/src/common/za.rs`, `za_session_keys`: the `None =>` arm returns `Ok(None)` | za: `dav_gate.rs` master-user PROPFIND |
| M5 | DELETE `send_itip` | `crates/dav/src/calendar/delete.rs:81`: delete the `is_key_account` line | za: planted DELETE, "a CANCEL left the key organizer"; key webdav: passed (no schedule tag) |
| M6 | `ItipSendStatus::KeyAccount` | `crates/groupware/src/calendar/itip.rs:1075`: `} else if false && account_info...` | za: `gating.rs` schedule-tag assertion; key webdav: `za_variants::scheduling` schedule-tag assertion |
| M7 | RSVP attendee copy | `crates/groupware/src/calendar/itip.rs:666`: `if false && server` | za: "the key attendee's copy was rewritten" |
| M8 | iMIP ingest | `crates/email/src/message/ingest.rs:367`: delete `&& !account.is_key_account()` | za: `gating.rs` key inbox after plain's invitation; key webdav: passed (john sends nothing) |
| M9 | Generic alarm | `crates/services/src/task_manager/alarm.rs:473`: `let generic = false;` | key webdav: `za_variants::alarm` organizer row or subject |
| M10 | Alarm recipient | `alarm.rs`, clearing block: delete `rcpt_to = None;` | key webdav: `alarm_override`, "no alarm email reached the account" |
| M11 | Schedule URLs hidden | `crates/dav/src/principal/propfind.rs:310`: `if false && account.is_key_account()` | za: `gating.rs:110-141`; key webdav: `principals.rs:135-153` |
| M12 | OPTIONS header | `crates/http/src/request.rs:341`: `if false && za_is_key_account_request(...)` | za: `gating.rs:147`; key webdav: `basic.rs:21-33` |
| M13 | Copy source | `crates/jmap/src/api/request.rs:648`: delete the line | za: the copy-source assertion |
| M14 | Bearer on key calendars | same edit as M4 | za: first failure is M4's; record that the Bearer case fails too by commenting out the master-user and admin PROPFINDs in `dav_gate.rs` for this run only |

- [ ] **Step 1: Run M1–M14**

Run the mutations in order. Keep raw notes in the task report as you go: the edit, each suite's first failure, and the revert confirmation.

- [ ] **Step 2: Write the table into the plan 3 outcome note**

Add a section "## Mutation runs (plan 7)" after "Deferred findings". It holds:
- One table row per mutation: Id, gate (with `file:line`), edit, `za_tests` first failure, key-mode `webdav_tests` first failure.
- One short paragraph noting that earlier recorded red runs (outbox, availability, OPTIONS, `Principal/get`, the first JMAP gate run) are not repeated.

- [ ] **Step 3: Close every gap**

For each mutation where both suites passed, decide which of these applies:
- **(a) A request can still reach the gate.** Write a test in the module that owns that gate's other tests, red-run it with the mutation, revert, and update the row. Keep such a test under about forty lines. If it needs more, record it as an open finding with the reason instead.
- **(b) No request can reach it, because an earlier gate stops every input.** Name the earlier gate and the line that stops it. Mark the row "layered behind <gate>". M3 against the variants and M5 against the variants may land here.

- [ ] **Step 4: Mark the notes**

Plan 3 outcome note:
- Mark each item this plan closes with "**Closed (plan 7)**" and a pointer to the test:
  - From "Tests worth adding": the variant mutation runs, the OAuth Bearer case, the R15 red test, `from_account_id`.
  - From the deferred Tests findings: the post-DELETE CANCEL checks, the fixed sleeps, the cleanup loop, the ETag comment, the inbox `len() == 1` comment, queue capture, and leak blob decoding.
- Queue capture is closed differently from the original ask: `wait_for_delivery` reads the persisted SMTP queue. `capture_queue()` is unusable in these suites because it stops local delivery. Say so in the note.
- Leak blob decoding: say that S3 stays covered only by the code path, since no CI step runs it, and that filesystem orphans are unscanned.
- Record that `ParticipantIdentity/changes` and the secondary-account session filter stay untested, unchanged, with their existing reasons.
- Correct the WebSocket wording: the key-account check runs per method call, not per connection (`crates/jmap/src/api/request.rs:735-743`), and `tests/src/za/websocket.rs` pins it.

Plan 2 outcome note, items at `:244-255`:
- Mark each case "**Closed (plan 7)**" with its test.
- Mark the outbox free-busy `Withheld` path "**Unreachable, no test (plan 7)**". The outbox refuses a key attendee with 3.7 before free-busy is built (`crates/dav/src/calendar/scheduling.rs:375-385`). A free-busy REPORT without the account's keys is refused by the URI gate (`crates/dav/src/common/uri.rs:105-115`). `ZaFreeBusy::Withheld` stays as defence in depth.

`CLAUDE.md`:
- In the za suite sentence, add the conditional, legacy and WebSocket tests and the shared helpers `plant_event` and `wait_for_delivery` (`tests/src/utils/za.rs`).
- In the CI sentence, add the filesystem blob-store run of `za_tests`.

- [ ] **Step 5: Final runs and commit**

Run `za_tests`, the filesystem `za_tests`, key-mode `webdav_tests` and plain-mode `webdav_tests`. Expected: all PASS.

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt -p tests -- --check
git diff main -- crates/   # must print nothing
git add docs/superpowers/plans/2026-10-06-zero-access-plan3-outcome.md docs/superpowers/plans/2026-10-06-zero-access-plan2-outcome.md CLAUDE.md
# plus any test file Step 3 touched
git commit -m "Record plan 7 mutation runs and close the test findings"   # plus the trailers
```
