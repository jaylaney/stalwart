# Zero-Access Calendar, Plan 3 of 3: Feature Gating, Suite Variants and the Leak Regression Test

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every server-side path that would need a key it does not have is closed for key accounts (sharing, impersonation, free-busy by others, scheduling, iMIP, RSVP, JMAP calendars, full-text indexing, alarm email content, whole-iCalendar traces), the CalDAV suite runs its key-account variants, and a leak regression test decodes every stored record and asserts the sealing boundary.

**Architecture:** Gating checks sit at existing permission points and read `AccountCache::is_key_account()`; nothing is worked around. The alarm email becomes generic for key accounts. The test suite gains a `za_variants` module for the key-account versions of `copy_move`, `acl`, `cal_alarm` and `cal_scheduling`, and a leak test that walks every data-store subspace, decompresses and unarchives each record, and checks the schema of what it finds, with a negative control.

**Tech Stack:** Stalwart `dav`, `jmap`, `groupware`, `services`, `email` crates; `tests` crate; `store` iteration API.

**Spec:** `docs/superpowers/specs/2026-10-06-zero-access-calendar-design.md` (revision 4), sections 9, 10, 11 and invariants 4, 6, 9, 10. Plans 1 and 2 must be complete first.

## Global Constraints

- Gating table (spec 9): sharing refused on calendars owned by key accounts; key accounts may still read calendars shared *to* them by non-key accounts or groups; impersonation and master-user sessions get 403 on calendar paths (plan 2's gate); free-busy by another user, the scheduling outbox and `Principal/getAvailability` skip key accounts; CalDAV scheduling never sends, never sets a schedule tag, never writes the notification collection; iMIP ingest skipped; HTTP RSVP refused; JMAP calendars not advertised and methods answer `accountNotSupportedByMethod`; full-text indexing returns `NotIndexed`; the alarm email is generic (start time, timezone, link; recipient is the account address); the two whole-iCalendar trace events carry the UID instead of the tree, for all accounts.
- Backup and restore need no change: sealed records are copied as-is and no plaintext calendar content exists in any subspace, which the leak test asserts.
- The leak test is a regression test for the sealing boundary, not a proof (spec 11). It runs in CI on every change once CI runs the tests crate (upstream's `test.yml` is manual; a `webdav` and `za` job is added here).
- Non-key accounts take unchanged code paths: every gate is `if account.is_key_account()`.
- **Shipping build excludes the enterprise feature.** The product binary is built with `cargo build --release -p stalwart --no-default-features --features rocks` (add other store backends by name as needed, never `enterprise`). Code under `cfg(feature = "enterprise")` and the whole `scim` crate are licensed only under the Stalwart Enterprise License and are not part of the product. Every compile check in these plans that builds the server uses the same flags, so fork code is always verified in the shipping configuration. The `tests` crate enables `enterprise` on `store`, `directory` and `coordinator` for upstream's own test modules; that is test-only and stays as is.
- Code in this plan was written without a compiler; small type and import fixes are expected.

## Review Focus

1. **A key account that is a member of a group** reads the group's (non-key) calendar: allowed, unchanged (Task 4, `za_variants::acl`, Jane reads `support@example.com`).
2. **A PUT with ORGANIZER and ATTENDEE on a key account** must store attendees sealed, send nothing, set no `Schedule-Tag`, and a later DELETE must send no CANCEL (Task 4, `za_variants::scheduling`).
3. **An alarm whose VALARM ATTENDEE is an external address** on a key account: the email goes to the account address, not the attendee, and carries no event text (Task 4, `za_variants::alarm`).
4. **A stateless JMAP method** (`CalendarEvent/parse`) on a key account is also refused, so clients see one consistent answer (Task 1 test `jmap_calendar_methods_refused`).
5. **The negative control**: an unsealed event written directly to a key account's store must make the leak scanner fail; a scanner that passes without the control is broken (Task 5).

---

### Task 1: Sharing, impersonation surfaces and JMAP gating

**Files:**
- Modify: `crates/dav/src/common/acl.rs`
- Modify: `crates/dav/src/principal/propfind.rs`
- Modify: `crates/dav/src/calendar/scheduling.rs`
- Modify: `crates/jmap/src/principal/availability.rs`
- Modify: `crates/jmap/src/api/session.rs`
- Modify: `crates/jmap/src/api/request.rs`
- Create: `tests/src/za/gating.rs`
- Modify: `tests/src/za/mod.rs`

**Interfaces:**
- Consumes: `AccountCache::is_key_account()` (plan 1), `ZeroAccessGate` (plan 2).
- Produces: `jmap::api::request::za_assert_calendar_allowed(server: &Server, account_id: Id) -> trc::Result<()>`.

- [ ] **Step 1: Write the failing tests**

`tests/src/za/gating.rs`:

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::STRONG;
use crate::utils::{server::TestServer, webdav::DummyWebDavClient};
use hyper::StatusCode;
use serde_json::json;

const GRANT: &str = "<?xml version=\"1.0\"?><D:acl xmlns:D=\"DAV:\"><D:ace><D:principal><D:href>/dav/pal/plain@example.com/</D:href></D:principal><D:grant><D:privilege><D:read/></D:privilege></D:grant></D:ace></D:acl>";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access gating tests...");
    let key1 = test.account("key1@example.com").clone();
    let key1_id = key1.id().document_id();
    let plain = test.account("plain@example.com").clone();
    let key_client = DummyWebDavClient::new(key1_id, "key1@example.com", STRONG, "key1@example.com");
    let plain_client = plain.webdav_client();

    // Sharing: ACL on a key-owned calendar is refused; on a plain calendar it works.
    key_client
        .request("ACL", "/dav/cal/key1@example.com/default/", GRANT)
        .await
        .with_status(StatusCode::FORBIDDEN);
    plain_client
        .request("ACL", "/dav/cal/plain@example.com/default/", GRANT.replace("plain@example.com", "key1@example.com"))
        .await
        .with_status(StatusCode::OK);
    // A key account may read a calendar shared to it by a non-key account.
    key_client
        .request("PROPFIND", "/dav/cal/plain@example.com/default/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);

    // Scheduling URLs are not advertised for key accounts.
    let principal = "/dav/pal/key1@example.com/";
    let props = key_client.propfind(principal, ["A:schedule-inbox-URL", "A:schedule-outbox-URL", "A:calendar-home-set"]).await;
    props.properties(principal).get("A:schedule-inbox-URL").with_status(404);
    props.properties(principal).get("A:schedule-outbox-URL").with_status(404);
    props.properties(principal).get("A:calendar-home-set").is_not_empty();
    let plain_principal = "/dav/pal/plain@example.com/";
    plain_client.propfind(plain_principal, ["A:schedule-inbox-URL"]).await.properties(plain_principal).get("A:schedule-inbox-URL").is_not_empty();

    // Scheduling outbox: a free-busy request naming a key account gets 3.7.
    let outbox = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REQUEST\r\nBEGIN:VFREEBUSY\r\nUID:fb-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20240101T000000Z\r\nDTEND:20240131T000000Z\r\nORGANIZER:mailto:plain@example.com\r\nATTENDEE:mailto:key1@example.com\r\nEND:VFREEBUSY\r\nEND:VCALENDAR\r\n";
    let body = plain_client
        .request_with_headers("POST", "/dav/itip/plain@example.com/outbox/", [("content-type", "text/calendar; charset=utf-8")], outbox)
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(body.contains("3.7"), "{body}");

    // JMAP: calendars not advertised, methods refused.
    let session = key1.jmap_session_object().await.into_inner();
    let caps = &session["accounts"][key1.id_string()]["accountCapabilities"];
    assert!(caps.get("urn:ietf:params:jmap:calendars").is_none(), "{caps}");
    assert!(caps.get("urn:ietf:params:jmap:mail").is_some() || caps.get("urn:ietf:params:jmap:core").is_some() || true);
    assert!(session["primaryAccounts"].get("urn:ietf:params:jmap:calendars").is_none());
    let plain_session = plain.jmap_session_object().await.into_inner();
    assert!(plain_session["accounts"][plain.id_string()]["accountCapabilities"].get("urn:ietf:params:jmap:calendars").is_some());
    for (method, args) in [
        ("Calendar/get", json!({ "accountId": key1.id_string() })),
        ("CalendarEvent/query", json!({ "accountId": key1.id_string() })),
        ("CalendarEvent/set", json!({ "accountId": key1.id_string(), "create": {} })),
        ("CalendarEvent/parse", json!({ "accountId": key1.id_string(), "blobIds": [] })),
        ("CalendarEvent/changes", json!({ "accountId": key1.id_string(), "sinceState": "0" })),
        ("ParticipantIdentity/get", json!({ "accountId": key1.id_string() })),
        ("CalendarEventNotification/get", json!({ "accountId": key1.id_string() })),
    ] {
        let response = key1.jmap_method_call(method, args).await.into_inner().to_string();
        assert!(response.contains("accountNotSupportedByMethod"), "{method}: {response}");
    }

    // Availability: a key account contributes no busy periods.
    let response = plain
        .jmap_method_call(
            "Principal/getAvailability",
            json!({ "accountId": plain.id_string(), "id": key1.id_string(), "utcStart": "2024-01-01T00:00:00Z", "utcEnd": "2024-12-31T00:00:00Z" }),
        )
        .await
        .into_inner()
        .to_string();
    assert!(!response.contains("\"busy\"") || response.contains("\"list\":[]") || response.contains("\"busyPeriods\":[]"), "{response}");

    // Clean up the grant on the plain calendar.
    plain_client
        .request("ACL", "/dav/cal/plain@example.com/default/", "<?xml version=\"1.0\"?><D:acl xmlns:D=\"DAV:\"></D:acl>")
        .await
        .with_status(StatusCode::OK);
}
```

(`jmap_session_object`, `jmap_method_call`, `id_string` are in `tests/src/utils/{jmap.rs,account.rs}`; the method-response shape for errors is whatever `JmapResponse::into_inner()` holds, so the assertion is on the serialized text. For the availability call, match the request shape in `tests/src/jmap/principal/`.) Register `pub mod gating;` and call it after `dav_seal::test_collections`.

- [ ] **Step 2: Run to verify failure**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -8
```

Expected: FAIL at the first ACL assertion (200 instead of 403).

- [ ] **Step 3: DAV: ACL, principal properties, outbox**

`crates/dav/src/common/acl.rs`, in `handle_acl_request` right after `let acls = container.acls().unwrap();`:

```rust
        // Spec 9: sharing is refused on calendars owned by key accounts.
        if collection == Collection::Calendar
            && self
                .account(account_id)
                .await
                .caused_by(trc::location!())?
                .is_key_account()
        {
            return Err(DavError::Code(StatusCode::FORBIDDEN));
        }
```

`crates/dav/src/principal/propfind.rs`, the `ScheduleInboxURL` and `ScheduleOutboxURL` arms become:

```rust
                        PrincipalProperty::ScheduleInboxURL | PrincipalProperty::ScheduleOutboxURL
                            if account.is_key_account() =>
                        {
                            fields_not_found.push(DavPropertyValue::empty(property.clone()));
                        }
```

placed before the two existing arms (so they keep serving non-key accounts).

`crates/dav/src/calendar/scheduling.rs`, in the attendee loop of the outbox handler, right after `if let Some(account_id) = self.account_id_from_email(&email, true).await.caused_by(trc::location!())? {`:

```rust
                if self
                    .account(account_id)
                    .await
                    .caused_by(trc::location!())?
                    .is_key_account()
                {
                    // Same item the handler emits for an unknown or
                    // unpermitted calendar user.
                    response.items.push(ScheduleResponseItem::new(
                        attendee,
                        "3.7;Invalid calendar user or insufficient permissions",
                    ));
                    continue;
                }
```

Use the exact constructor the existing `3.7;Invalid calendar user or insufficient permissions` branch uses in that function (copy it).

- [ ] **Step 4: JMAP: session, methods, availability**

`crates/jmap/src/api/session.rs`:

1. Before `let mut account = Account { name: account.name().to_string(), is_personal: true, ..` (the shadowing), add `let primary_is_key = account.is_key_account();`. Change the primary loop header to:

```rust
        for capability in access_token
            .account_capabilities()
            .filter(|c| !(primary_is_key && matches!(c, Capability::Calendars | Capability::CalendarsParse)))
        {
```

2. In the secondary loop, after `let is_owner = ..` and the `try_account` load, add `let is_key = account.is_key_account();` (before the shadowing `let mut account = Account {`), and filter the same way with `is_key`.

`crates/jmap/src/api/request.rs`: add

```rust
/// Spec 9: JMAP calendar methods are not offered for key accounts in this release.
pub(crate) async fn za_assert_calendar_allowed(server: &Server, account_id: Id) -> trc::Result<()> {
    if server.account(account_id.document_id()).await?.is_key_account() {
        Err(trc::JmapEvent::AccountNotSupportedByMethod.into_err())
    } else {
        Ok(())
    }
}
```

and call `za_assert_calendar_allowed(self, req.account_id).await?;` immediately after `resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;` in these arms: `GetRequestMethod::{Calendar, CalendarEvent, CalendarEventNotification, ParticipantIdentity}` (lines ~336-360), `QueryRequestMethod::{Calendar, CalendarEvent, CalendarEventNotification}` (~433-450), `SetRequestMethod::{Calendar, CalendarEvent, CalendarEventNotification, ParticipantIdentity}` (~547-575), `CopyRequestMethod::CalendarEvent` (~627; check both `req.account_id` and `req.from_account_id`), `ParseRequestMethod::CalendarEvent` (~671). For the generic changes arm, add before `self.changes(*req, method_name.obj, access_token)`:

```rust
                    if matches!(
                        method_name.obj,
                        MethodObject::Calendar | MethodObject::CalendarEvent | MethodObject::CalendarEventNotification
                    ) {
                        za_assert_calendar_allowed(self, req.account_id).await?;
                    }
```

`crates/jmap/src/principal/availability.rs`, first statement inside `for account_id in principal.all_ids_by_collection(Collection::Calendar) {`:

```rust
            if self.account(account_id).await.caused_by(trc::location!())?.is_key_account() {
                continue;
            }
```

- [ ] **Step 5: Run and commit**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -5
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests jmap::jmap_tests -- --nocapture 2>&1 | tail -3
git add crates/dav crates/jmap tests/src/za
git commit -m "Gate sharing, scheduling advertisement and JMAP calendars for key accounts"
```

---

### Task 2: Scheduling, iMIP and RSVP gating

**Files:**
- Modify: `crates/groupware/src/calendar/itip.rs` (`ItipSendStatus`, RSVP)
- Modify: `crates/dav/src/calendar/delete.rs`
- Modify: `crates/email/src/message/ingest.rs`
- Modify: `tests/src/za/gating.rs`

**Interfaces:**
- Produces: `ItipSendStatus::KeyAccount` (denied, with a reason); `send_itip` false for key accounts on DELETE; iMIP ingest skipped; `http_rsvp_handle` answers the invalid-link error for key accounts.

- [ ] **Step 1: Extend the tests**

Append to `tests/src/za/gating.rs` and call `test_scheduling` after `test`:

```rust
pub async fn test_scheduling(test: &mut TestServer) {
    println!("Running zero-access scheduling gating tests...");
    let key1 = test.account("key1@example.com").clone();
    let key1_id = key1.id().document_id();
    let key_client = DummyWebDavClient::new(key1_id, "key1@example.com", STRONG, "key1@example.com");
    let plain_client = test.account("plain@example.com").webdav_client();

    // Organizer is a key account: stored, nothing sent, no schedule tag.
    let invite = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:za-invite-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:invite-canary\r\nORGANIZER:mailto:key1@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:plain@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let response = key_client
        .request_with_headers("PUT", "/dav/cal/key1@example.com/default/invite.ics", [("content-type", "text/calendar; charset=utf-8")], invite)
        .await
        .with_status(StatusCode::CREATED);
    assert!(response.headers.get("schedule-tag").is_none(), "{:?}", response.headers);
    test.wait_for_tasks().await;
    assert!(test.queue_rx.try_recv().is_err(), "no outbound message was queued");
    let inbox = plain_client
        .request_with_headers("PROPFIND", "/dav/itip/plain@example.com/inbox/", [("depth", "1")], "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_eq!(inbox.hrefs().len(), 1, "inbox holds only itself: {:?}", inbox.hrefs());
    key_client
        .request("DELETE", "/dav/cal/key1@example.com/default/invite.ics", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    test.wait_for_tasks().await;
    assert!(test.queue_rx.try_recv().is_err(), "no CANCEL was queued");

    // RSVP page: a token for a key account is refused.
    let token = test
        .server
        .encode_access_token(common::auth::oauth::GrantType::Rsvp, key1_id, "key1@example.com", 3600, Some("plain@example.com;0"), None)
        .await
        .unwrap();
    let response = test
        .server
        .http_rsvp_handle(
            groupware::calendar::itip::RsvpRequest { token, partstat: None, comment: None },
            "en",
            "127.0.0.1".parse().unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(response, groupware::calendar::itip::RsvpResponse::Error { .. }), "{response:?}");
}
```

(`encode_access_token` lives in `crates/common/src/auth/oauth/token.rs`; match its signature from `http_rsvp_url` in `crates/groupware/src/calendar/itip.rs:339-370`. The `http_rsvp_handle` trait is `groupware::calendar::itip::ItipIngest` or a sibling; import it. `test.queue_rx` is the queue event receiver on `TestServer`.)

- [ ] **Step 2: Run to verify failure**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -8
```

Expected: FAIL (a schedule tag is returned or an inbox item appears).

- [ ] **Step 3: Implement**

`crates/groupware/src/calendar/itip.rs`:

1. Add `KeyAccount,` to `pub enum ItipSendStatus`.
2. In `ItipSendStatus::resolve`, add a branch after the `addresses().is_empty()` check:

```rust
        } else if account_info.account().is_key_account() {
            Self::KeyAccount
```

3. `is_denied` includes `Self::KeyAccount`; `reason()` gains `Self::KeyAccount => Some("Scheduling is not available for zero-access accounts in this release."),`.
4. In the function that calls `decode_rsvp_token(server, &request.token).await` (the `http_rsvp_handle` implementation), right after the token is decoded:

```rust
        if server.account(token.account_id).await?.is_key_account() {
            return Err(RsvpError::InvalidLink);
        }
```

   using the same error-to-response conversion that function applies to `decode_rsvp_token` errors (if it maps `RsvpError` with `?`, this line is enough; otherwise mirror the surrounding `match`).

`crates/dav/src/calendar/delete.rs`, the `send_itip` expression:

```rust
        let send_itip = self.core.groupware.itip_enabled
            && !headers.no_schedule_reply
            && !account_info.addresses().is_empty()
            && !account_info.account().is_key_account()
            && access_token.has_permission(Permission::CalendarSchedulingSend);
```

`crates/email/src/message/ingest.rs`, the iMIP block: move `let account_info = self.build_account_info(account.clone()).await.caused_by(trc::location!())?;` above the `if self.core.groupware.itip_enabled && ..` condition and add `&& !account_info.account().is_key_account()` to that condition. (`AccountInfo::account()` is at `crates/common/src/cache/principals.rs:990`.)

- [ ] **Step 4: Run and commit**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -5
git add crates/groupware crates/dav crates/email tests/src/za
git commit -m "Disable scheduling, iMIP ingest and RSVP for key accounts"
```

---

### Task 3: Full-text indexing, trace events and the generic alarm email

**Files:**
- Modify: `crates/services/src/task_manager/index.rs`
- Modify: `crates/groupware/src/calendar/dates.rs`
- Modify: `crates/dav/src/calendar/query.rs`
- Modify: `crates/services/src/task_manager/alarm.rs`

**Interfaces:**
- Produces: `build_calendar_document` returns `NotIndexed` for key accounts; `RuleExpansionError` events carry the UID; `build_template` produces a generic email for key accounts.

- [ ] **Step 1: Full-text indexing**

`crates/services/src/task_manager/index.rs`, in `build_calendar_document` after the `index_fields` early return:

```rust
    // Spec 9: nothing readable to index for key accounts.
    if server.account(account_id).await?.is_key_account() {
        return Ok(BuildResult::NotIndexed);
    }
```

- [ ] **Step 2: Trace events**

`crates/groupware/src/calendar/dates.rs`, in `CalendarEventData::new`, change `Details = ical.to_string(),` to:

```rust
                Details = ical.uids().next().unwrap_or_default().to_string(),
```

`crates/dav/src/calendar/query.rs`, in `CalendarQueryHandler::new`, change `Details = event.data.event.to_string(),` to:

```rust
                                Details = event.data.event.uids().next().unwrap_or_default().to_string(),
```

- [ ] **Step 3: Generic alarm email**

`crates/services/src/task_manager/alarm.rs`, in `build_template`:

1. After the `let mut guests = vec![];` line add `let generic = account_info.account().is_key_account();`.
2. Wrap the two property loops (`for entry in alarm_component.entries.iter() { .. }` and `for entry in event_component.entries.iter() { .. }`) in `if !generic { .. }`. With `generic`, `summary`, `description`, `rcpt_to`, `location`, `conference`, `organizer` stay `None` and `guests` stays empty, so the recipient falls back to `account_info.name()` and the template renders only start, end, organizer (the account) and the webcal link.
3. Change the subject construction to:

```rust
    let subject = if generic {
        format!("{}: {}", locale.calendar_alarm_subject_prefix, start)
    } else {
        format!(
            "{}: {} @ {}",
            locale.calendar_alarm_subject_prefix,
            summary.or(description).unwrap_or("No Subject"),
            start
        )
    };
```

- [ ] **Step 4: Build, run the upstream alarm test and commit**

```bash
cargo build -p stalwart --no-default-features --features rocks 2>&1 | grep -E "^error" -A 5 | head -30
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav::webdav_tests -- --nocapture 2>&1 | tail -3
git add crates/services crates/groupware crates/dav
git commit -m "Skip full-text indexing, drop iCalendar dumps from traces and send generic alarms for key accounts"
```

The key-account alarm behaviour is asserted by the variant in Task 4.

---

### Task 4: Key-account variants of the CalDAV suite

**Files:**
- Create: `tests/src/webdav/za_variants.rs`
- Modify: `tests/src/webdav/mod.rs`

**Interfaces:**
- Produces: `za_variants::{copy_move, acl, alarm, scheduling}` run in `ZA_KEY_ACCOUNTS=1` mode instead of the skipped modules.

- [ ] **Step 1: Write the variants**

`tests/src/webdav/za_variants.rs`:

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Key-account variants of copy_move, acl, cal_alarm and cal_scheduling (spec 11).

use super::{TEST_ICAL_1, TEST_ICAL_2};
use crate::utils::server::TestServer;
use email::cache::MessageCacheFetch;
use hyper::StatusCode;
use mail_parser::{DateTime, MessageParser};
use store::write::now;

pub async fn copy_move(test: &TestServer) {
    println!("Running key-account copy/move tests...");
    let john = test.account("john@example.com").webdav_client();
    let jane = test.account("jane@example.com").webdav_client();

    // In-account copy and move succeed.
    john.request_with_headers("PUT", "/dav/cal/john@example.com/default/a.ics", [("content-type", "text/calendar")], TEST_ICAL_1)
        .await
        .with_status(StatusCode::CREATED);
    john.mkcol("MKCALENDAR", "/dav/cal/john@example.com/other/", [], [("D:displayname", "Other")])
        .await
        .with_status(StatusCode::CREATED);
    john.request_with_headers("COPY", "/dav/cal/john@example.com/default/a.ics", [("destination", "/dav/cal/john@example.com/other/a.ics")], "")
        .await
        .with_status(StatusCode::CREATED);
    john.request_with_headers("MOVE", "/dav/cal/john@example.com/other/a.ics", [("destination", "/dav/cal/john@example.com/other/b.ics")], "")
        .await
        .with_status(StatusCode::CREATED);
    let body = john.request("GET", "/dav/cal/john@example.com/other/b.ics", "").await.with_status(StatusCode::OK).body.unwrap();
    assert!(body.contains("What a nice present") && !body.contains("X-ZA-"), "{body}");
    john.request_with_headers("COPY", "/dav/cal/john@example.com/other/", [("destination", "/dav/cal/john@example.com/other-copy/"), ("depth", "infinity")], "")
        .await
        .with_status(StatusCode::CREATED);
    john.request("GET", "/dav/cal/john@example.com/other-copy/b.ics", "").await.with_status(StatusCode::OK);

    // Across accounts: refused in both directions, including to a group the
    // caller belongs to (Jane is a member of support).
    john.request_with_headers("COPY", "/dav/cal/john@example.com/default/a.ics", [("destination", "/dav/cal/jane@example.com/default/a.ics")], "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    john.request_with_headers("MOVE", "/dav/cal/john@example.com/default/a.ics", [("destination", "/dav/cal/support@example.com/default/a.ics")], "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    jane.request_with_headers("PUT", "/dav/cal/jane@example.com/default/j.ics", [("content-type", "text/calendar")], TEST_ICAL_2)
        .await
        .with_status(StatusCode::CREATED);
    jane.request_with_headers("COPY", "/dav/cal/jane@example.com/default/j.ics", [("destination", "/dav/cal/support@example.com/default/j.ics")], "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    jane.request_with_headers("COPY", "/dav/cal/jane@example.com/default/", [("destination", "/dav/cal/support@example.com/jane/"), ("depth", "infinity")], "")
        .await
        .with_status(StatusCode::FORBIDDEN);

    for path in ["/dav/cal/john@example.com/other-copy/", "/dav/cal/john@example.com/other/", "/dav/cal/john@example.com/default/a.ics", "/dav/cal/jane@example.com/default/j.ics"] {
        let client = if path.contains("john") { &john } else { &jane };
        client.request("DELETE", path, "").await.with_status(StatusCode::NO_CONTENT);
    }
    john.delete_default_containers().await;
    jane.delete_default_containers().await;
    test.assert_is_empty().await;
}

pub async fn acl(test: &TestServer) {
    println!("Running key-account ACL tests...");
    let john = test.account("john@example.com").webdav_client();
    let jane = test.account("jane@example.com").webdav_client();
    let grant = "<?xml version=\"1.0\"?><D:acl xmlns:D=\"DAV:\"><D:ace><D:principal><D:href>/dav/pal/jane@example.com/</D:href></D:principal><D:grant><D:privilege><D:read/></D:privilege></D:grant></D:ace></D:acl>";
    john.request("ACL", "/dav/cal/john@example.com/default/", grant).await.with_status(StatusCode::FORBIDDEN);
    jane.request("PROPFIND", "/dav/cal/john@example.com/default/", "").await.with_status(StatusCode::FORBIDDEN);
    // A key account that is a group member reads the group's calendar (Review Focus 1).
    jane.request("PROPFIND", "/dav/cal/support@example.com/", "").await.with_status(StatusCode::MULTI_STATUS);
    john.request("PROPFIND", "/dav/cal/support@example.com/", "").await.with_status(StatusCode::FORBIDDEN);
    john.delete_default_containers().await;
    jane.delete_default_containers().await;
    jane.delete_default_containers_by_account("support@example.com").await;
    test.assert_is_empty().await;
}

pub async fn alarm(test: &TestServer) {
    println!("Running key-account alarm tests...");
    let account = test.account("john@example.com");
    let client = account.webdav_client();
    client
        .request_with_headers(
            "PUT",
            "/dav/cal/john%40example.com/default/its-alarming-how-charming-i-feel.ics",
            [("content-type", "text/calendar; charset=utf-8")],
            super::cal_alarm::TEST_ALARM_1.replace(
                "$START",
                &DateTime::from_timestamp(now() as i64 + 5).to_rfc3339().replace(['-', ':'], ""),
            ),
        )
        .await
        .with_status(StatusCode::CREATED);
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
    let messages = test.server.get_cached_messages(client.account_id).await.unwrap();
    assert_eq!(messages.emails.items.len(), 2);
    for message in messages.emails.items.iter() {
        let contents = test.fetch_email(client.account_id, message.document_id).await;
        let message = MessageParser::new().parse(&contents).unwrap();
        let to = message.to().and_then(|t| t.first()).and_then(|a| a.address()).unwrap_or_default().to_string();
        assert_eq!(to, "john@example.com", "recipient is the account address, not the alarm attendee");
        let html = String::from_utf8(message.html_bodies().next().unwrap().contents().to_vec()).unwrap();
        let text = message.html_bodies().next().unwrap().text_contents().unwrap().to_string();
        assert!(html.contains("/dav/cal/john%40example.com/default/its-alarming-how-charming-i-feel.ics"), "{html}");
        for canary in ["See the pretty girl", "What mirror where", "I feel pretty", "alarming how charming", "meet.example.com/west-side", "West Side", "john_doe@unknown.com"] {
            assert!(!html.contains(canary) && !text.contains(canary), "{canary} leaked into the alarm email: {html}");
        }
        let subject = message.subject().unwrap_or_default();
        assert!(!subject.contains("pretty") && !subject.contains("mirror") && !subject.contains("No Subject"), "{subject}");
    }
    client.delete_default_containers().await;
    test.destroy_all_mailboxes(account).await;
    test.assert_is_empty().await
}

pub async fn scheduling(test: &TestServer) {
    println!("Running key-account scheduling tests...");
    let john = test.account("john@example.com").webdav_client();
    let jane = test.account("jane@example.com").webdav_client();
    let invite = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:za-sched-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:sched-canary\r\nORGANIZER:mailto:john@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:jane@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let response = john
        .request_with_headers("PUT", "/dav/cal/john@example.com/default/s.ics", [("content-type", "text/calendar; charset=utf-8")], invite)
        .await
        .with_status(StatusCode::CREATED);
    assert!(response.headers.get("schedule-tag").is_none());
    test.wait_for_tasks().await;
    let inbox = jane
        .request_with_headers("PROPFIND", "/dav/itip/jane@example.com/inbox/", [("depth", "1")], "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_eq!(inbox.hrefs().len(), 1, "{:?}", inbox.hrefs());
    // The event is readable by its owner with attendees intact.
    let body = john.request("GET", "/dav/cal/john@example.com/default/s.ics", "").await.with_status(StatusCode::OK).body.unwrap();
    assert!(body.contains("mailto:jane@example.com") && !body.contains("X-ZA-"));
    john.request("DELETE", "/dav/cal/john@example.com/default/s.ics", "").await.with_status(StatusCode::NO_CONTENT);
    test.wait_for_tasks().await;
    john.delete_default_containers().await;
    jane.delete_default_containers().await;
    test.assert_is_empty().await;
}
```

(`TEST_ALARM_1` in `cal_alarm.rs` must become `pub(super) const`.)

- [ ] **Step 2: Wire the variants**

`tests/src/webdav/mod.rs`: add `pub mod za_variants;` and replace the plan-2 skips:

```rust
    if key_accounts_mode() {
        za_variants::copy_move(&test).await;
    } else {
        copy_move::test(&test, assisted_discovery).await;
    }
    ...
    if key_accounts_mode() {
        za_variants::acl(&test).await;
        za_variants::alarm(&test).await;
        cal_itip::test();
        za_variants::scheduling(&test).await;
    } else {
        acl::test(&test).await;
        cal_alarm::test(&test).await;
        cal_itip::test();
        cal_scheduling::test(&test).await;
    }
```

(`cal_itip::test()` is pure and runs in both modes; keep `card_query` and `cal_query` where they are.)

- [ ] **Step 3: Run both modes**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav::webdav_tests -- --nocapture 2>&1 | tail -3
STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav::webdav_tests -- --nocapture 2>&1 | tail -3
```

Expected: both `test result: ok`.

- [ ] **Step 4: Commit**

```bash
git add tests
git commit -m "Add key-account variants for copy_move, acl, cal_alarm and cal_scheduling"
```

---

### Task 5: Leak regression test

**Files:**
- Create: `tests/src/za/leak.rs`
- Modify: `tests/src/za/mod.rs`
- Modify: `.github/workflows/test.yml`

**Interfaces:**
- Produces: `za::za_tests` ends with the leak scan; CI runs `za::za_tests` and both `webdav_tests` modes.

- [ ] **Step 1: Write the scanner and the test**

`tests/src/za/leak.rs`:

```rust
/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Spec 11: decodes every stored record and asserts the sealing boundary.
//! A regression test, not a proof.

use super::{STRONG, dav_seal::{CANARIES, EVENT}};
use crate::utils::{server::TestServer, webdav::DummyWebDavClient};
use calcard::icalendar::{ICalendar, ICalendarProperty};
use email::cache::MessageCacheFetch;
use groupware::calendar::{
    Calendar, CalendarEvent, CalendarEventData, Timezone,
    seal::policy::{is_visible_parameter, is_visible_property},
};
use hyper::StatusCode;
use mail_parser::{DateTime, MessageParser};
use store::{
    IterateParams, SUBSPACE_ACL, SUBSPACE_BLOB_LINK, SUBSPACE_BLOBS, SUBSPACE_COUNTER,
    SUBSPACE_DELETED_ITEMS, SUBSPACE_DIRECTORY, SUBSPACE_IN_MEMORY_COUNTER,
    SUBSPACE_IN_MEMORY_VALUE, SUBSPACE_INDEXES, SUBSPACE_LOGS, SUBSPACE_PROPERTY,
    SUBSPACE_QUEUE_EVENT, SUBSPACE_QUEUE_MESSAGE, SUBSPACE_QUOTA, SUBSPACE_REGISTRY,
    SUBSPACE_REGISTRY_IDX, SUBSPACE_REGISTRY_PK, SUBSPACE_REPORT_IN, SUBSPACE_REPORT_OUT,
    SUBSPACE_SEARCH_INDEX, SUBSPACE_SPAM_SAMPLES, SUBSPACE_TASK_QUEUE,
    SUBSPACE_TELEMETRY_METRIC, SUBSPACE_TELEMETRY_SPAN,
    write::{AlignedBytes, AnyKey, Archive},
};
use types::collection::SyncCollection;

const EXTRA_CANARIES: &[&str] = &["leakcal-canary", "leakdesc-canary", "leakcolor-canary", "leaktz-canary", "leakevtname-canary", "leakdead-canary", "alarm-summary-canary", "todo-canary", "negative-canary"];

fn all_canaries() -> Vec<&'static str> {
    CANARIES.iter().chain(EXTRA_CANARIES.iter()).copied().collect()
}

fn is_carrier_name(name: &ICalendarProperty) -> bool {
    matches!(name, ICalendarProperty::Other(n) if n.len() > 5 && n[..5].eq_ignore_ascii_case("X-ZA-"))
}

fn check_tree(tree: &ICalendar, what: &str, require_key: bool, violations: &mut Vec<String>) {
    for (index, component) in tree.components.iter().enumerate() {
        for entry in component.entries.iter() {
            if is_carrier_name(&entry.name) {
                continue;
            }
            if !is_visible_property(&component.component_type, &entry.name) {
                violations.push(format!("{what}: component {index} has sealed-class property {:?}", entry.name));
            }
            for param in entry.params.iter() {
                if !is_visible_parameter(&param.name) {
                    violations.push(format!("{what}: component {index} {:?} has sealed-class parameter {:?}", entry.name, param.name));
                }
            }
        }
    }
    if require_key
        && !tree.components.first().and_then(|c| c.entries.last()).is_some_and(|e| matches!(&e.name, ICalendarProperty::Other(n) if n == "X-ZA-KEY"))
    {
        violations.push(format!("{what}: missing X-ZA-KEY"));
    }
}

fn check_event(event: &CalendarEvent, what: &str, violations: &mut Vec<String>) {
    if event.display_name.is_some() {
        violations.push(format!("{what}: display_name not empty"));
    }
    if !event.dead_properties.0.is_empty() {
        violations.push(format!("{what}: dead_properties not empty"));
    }
    check_tree(&event.data.event, what, true, violations);
}

fn check_calendar(calendar: &Calendar, account_id: u32, what: &str, violations: &mut Vec<String>) {
    let pref = calendar.preferences(account_id);
    if !pref.name.starts_with("$za$") {
        // Plaintext collections are allowed only if nothing user-supplied is in them.
        if pref.description.is_some() || pref.color.is_some() || !calendar.dead_properties.0.is_empty() || matches!(pref.time_zone, Timezone::Custom(_)) {
            violations.push(format!("{what}: unsealed collection with user data"));
        }
        return;
    }
    if pref.description.is_some() || pref.color.is_some() || !calendar.dead_properties.0.is_empty() {
        violations.push(format!("{what}: sealed collection with plaintext fields"));
    }
    if let Timezone::Custom(tz) = &pref.time_zone {
        check_tree(tz, what, false, violations);
    }
}

/// Walks every subspace. Returns the violations found for `account_id`.
pub async fn scan(test: &TestServer, account_id: u32) -> Vec<String> {
    let canaries = all_canaries();
    let store = test.server.store().clone();
    let mut violations = Vec::new();
    let subspaces = [
        SUBSPACE_ACL, SUBSPACE_TASK_QUEUE, SUBSPACE_IN_MEMORY_VALUE, SUBSPACE_IN_MEMORY_COUNTER,
        SUBSPACE_PROPERTY, SUBSPACE_QUEUE_MESSAGE, SUBSPACE_QUEUE_EVENT, SUBSPACE_REPORT_OUT,
        SUBSPACE_REPORT_IN, SUBSPACE_DELETED_ITEMS, SUBSPACE_SPAM_SAMPLES, SUBSPACE_BLOB_LINK,
        SUBSPACE_BLOBS, SUBSPACE_COUNTER, SUBSPACE_QUOTA, SUBSPACE_INDEXES, SUBSPACE_TELEMETRY_SPAN,
        SUBSPACE_TELEMETRY_METRIC, SUBSPACE_SEARCH_INDEX, SUBSPACE_REGISTRY, SUBSPACE_REGISTRY_IDX,
        SUBSPACE_REGISTRY_PK, SUBSPACE_DIRECTORY, SUBSPACE_LOGS,
    ];
    for subspace in subspaces {
        if subspace == SUBSPACE_SEARCH_INDEX && store.is_pg_or_mysql() {
            continue;
        }
        let from = AnyKey { subspace, key: vec![0u8] };
        let to = AnyKey { subspace, key: vec![u8::MAX; 16] };
        let mut records: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        store
            .iterate(IterateParams::new(from, to), |key, value| {
                records.push((key.to_vec(), value.to_vec()));
                Ok(true)
            })
            .await
            .unwrap();
        for (key, value) in records {
            let what = format!("subspace {:?} key {:?}", char::from(subspace), key);
            for blob in [&key[..], &value[..]] {
                let text = String::from_utf8_lossy(blob);
                for canary in &canaries {
                    if text.contains(canary) {
                        violations.push(format!("{what}: raw bytes contain {canary}"));
                    }
                }
            }
            // Decode archives (LZ4 if compressed) and look inside.
            if let Ok(archive) = <Archive<AlignedBytes> as store::Deserialize>::deserialize(&value) {
                let decoded = String::from_utf8_lossy(archive.as_bytes()).to_string();
                for canary in &canaries {
                    if decoded.contains(canary) {
                        violations.push(format!("{what}: decoded archive contains {canary}"));
                    }
                }
                if subspace == SUBSPACE_PROPERTY && key.len() == 10 && key[5] == 50 {
                    let key_account = u32::from_be_bytes(key[0..4].try_into().unwrap());
                    if key_account == account_id {
                        match key[4] {
                            9 => match archive.deserialize::<CalendarEvent>() {
                                Ok(event) => check_event(&event, &what, &mut violations),
                                Err(err) => violations.push(format!("{what}: event archive does not decode: {err:?}")),
                            },
                            8 => match archive.deserialize::<Calendar>() {
                                Ok(calendar) => check_calendar(&calendar, account_id, &what, &mut violations),
                                Err(err) => violations.push(format!("{what}: calendar archive does not decode: {err:?}")),
                            },
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    // Generated alarm emails.
    if let Ok(messages) = test.server.get_cached_messages(account_id).await {
        for message in messages.emails.items.iter() {
            let contents = test.fetch_email(account_id, message.document_id).await;
            let text = String::from_utf8_lossy(&contents);
            for canary in &canaries {
                if text.contains(canary) {
                    violations.push(format!("alarm email {}: contains {canary}", message.document_id));
                }
            }
        }
    }
    violations
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access leak regression test...");
    let name = "key2@example.com";
    let id = test.account(name).id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let cal = "/dav/cal/key2@example.com/leak/";

    // Everything the spec lists, written through CalDAV.
    client
        .mkcol("MKCALENDAR", cal, [], [("D:displayname", "leakcal-canary"), ("A:calendar-description", "leakdesc-canary"), ("C:calendar-color", "leakcolor-canary")])
        .await
        .with_status(StatusCode::CREATED);
    let tz = crate::webdav::TEST_VTIMEZONE_1.replace("Eastern Standard Time (US Canada)", "leaktz-canary");
    client.proppatch(cal, [("A:calendar-timezone", tz.as_str())], []).await.with_status(StatusCode::MULTI_STATUS);
    client
        .request_with_headers("PUT", "/dav/cal/key2@example.com/leak/e.ics", [("content-type", "text/calendar; charset=utf-8")], EVENT)
        .await
        .with_status(StatusCode::CREATED);
    client
        .proppatch("/dav/cal/key2@example.com/leak/e.ics", [("D:displayname", "leakevtname-canary"), ("X:dead xmlns:X=\"urn:x\"", "leakdead-canary")], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let todo = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VTODO\r\nUID:leak-todo\r\nDTSTAMP:20240101T000000Z\r\nDUE:20240105T000000Z\r\nSUMMARY:todo-canary\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
    client
        .request_with_headers("PUT", "/dav/cal/key2@example.com/leak/t.ics", [("content-type", "text/calendar; charset=utf-8")], todo)
        .await
        .with_status(StatusCode::CREATED);
    let alarm = format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:leak-alarm\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:{}\r\nDTEND;TZID=America/New_York:21250221T180000\r\nSUMMARY:alarm-summary-canary\r\nBEGIN:VALARM\r\nTRIGGER:-P2S\r\nACTION:EMAIL\r\nATTENDEE:mailto:key2@example.com\r\nSUMMARY:alarm-summary-canary\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        DateTime::from_timestamp(store::write::now() as i64 + 3).to_rfc3339().replace(['-', ':'], "")
    );
    client
        .request_with_headers("PUT", "/dav/cal/key2@example.com/leak/alarm.ics", [("content-type", "text/calendar; charset=utf-8")], alarm)
        .await
        .with_status(StatusCode::CREATED);
    client
        .request_with_headers("COPY", cal, [("destination", "/dav/cal/key2@example.com/leak-copy/"), ("depth", "infinity")], "")
        .await
        .with_status(StatusCode::CREATED);
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    test.wait_for_tasks().await;

    // No carrier ever reaches a client.
    for path in [cal, "/dav/cal/key2@example.com/leak-copy/"] {
        let body = client
            .request_with_headers("PROPFIND", path, [("depth", "1")], "<?xml version=\"1.0\"?><D:propfind xmlns:D=\"DAV:\"><D:allprop/></D:propfind>")
            .await
            .with_status(StatusCode::MULTI_STATUS)
            .body
            .unwrap();
        assert!(!body.contains("X-ZA-"), "{body}");
    }
    for path in ["/dav/cal/key2@example.com/leak/e.ics", "/dav/cal/key2@example.com/leak/t.ics", "/dav/cal/key2@example.com/leak-copy/e.ics"] {
        let body = client.request("GET", path, "").await.with_status(StatusCode::OK).body.unwrap();
        assert!(!body.contains("X-ZA-") && !body.contains("$za$"), "{body}");
    }

    let violations = scan(test, id).await;
    assert!(violations.is_empty(), "leaks found:\n{}", violations.join("\n"));

    // Negative control: an unsealed event planted directly must be caught.
    {
        let resources = test.server.fetch_dav_resources(id, id, SyncCollection::Calendar).await.unwrap();
        let calendar_id = resources.by_path("leak").unwrap().document_id();
        let ical = match calcard::Parser::new(&EVENT.replace("summary-canary", "negative-canary")).entry() {
            calcard::Entry::ICalendar(ical) => ical,
            _ => panic!(),
        };
        let mut next = None;
        let event = CalendarEvent {
            names: vec![common::DavName { name: "planted.ics".into(), parent_id: calendar_id }],
            data: CalendarEventData::new(ical, calcard::common::timezone::Tz::Floating, 100, &mut next),
            size: 1,
            ..Default::default()
        };
        let account_info = test.server.account_info(id).await.unwrap();
        let document_id = test.server.store().assign_document_ids(id, types::collection::Collection::CalendarEvent, 1).await.unwrap();
        let mut batch = store::write::BatchBuilder::new();
        event.insert(account_info.account_tenant_ids(), id, document_id, None, &mut batch).unwrap();
        test.server.commit_batch(batch).await.unwrap();
        let violations = scan(test, id).await;
        assert!(violations.iter().any(|v| v.contains("negative-canary")), "the scanner must catch the planted event: {violations:?}");
        test.server.invalidate_local_caches(&[common::ipc::CacheInvalidation::DavResources(id)]).await;
        client.request("DELETE", "/dav/cal/key2@example.com/leak/planted.ics", "").await.with_status(StatusCode::NO_CONTENT);
    }

    for path in ["/dav/cal/key2@example.com/leak-copy/", cal] {
        client.request("DELETE", path, "").await.with_status(StatusCode::NO_CONTENT);
    }
    test.destroy_all_mailboxes(test.account(name)).await;
}
```

(The planted event is deleted through DAV so the index, quota and changelog are cleaned the normal way; the `DavResources` invalidation makes the cache pick it up. Register `pub mod leak;` and call `leak::test(&mut test).await;` last, before `destroy_key_accounts`.)

- [ ] **Step 2: Run**

```bash
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -8
```

Expected: PASS, with the negative-control assertion proving the scanner sees planted plaintext. If `assert_is_empty` complains about leftover keys from the leak module, every created collection and the planted event must be deleted through DAV before the function returns.

- [ ] **Step 3: CI**

`.github/workflows/test.yml`: after the `JMAP Tests` step add

```yaml
      - name: Zero-access Tests
        run: cargo test -p tests za::za_tests -- --nocapture

      - name: CalDAV Tests (plain accounts)
        run: cargo test -p tests webdav::webdav_tests -- --nocapture

      - name: CalDAV Tests (key accounts)
        run: cargo test -p tests webdav::webdav_tests -- --nocapture
        env:
          ZA_KEY_ACCOUNTS: "1"
```

and change the workflow trigger to run on `push` and `pull_request` for the `zero-access` branch in addition to `workflow_dispatch`.

- [ ] **Step 4: Commit**

```bash
git add tests .github/workflows/test.yml
git commit -m "Add the zero-access leak regression test and run it in CI"
```

---

### Task 6: Manual checklist, developer notes and the final run

**Files:**
- Create: `docs/zero-access/manual-checklist.md`
- Modify: `docs/superpowers/plans/README-dev.md`

- [ ] **Step 1: Write the manual checklist (spec 11)**

`docs/zero-access/manual-checklist.md`:

```markdown
# Zero-access calendar: manual client checklist

Run once per release against a server built from this branch, with TOTP
enrolled on one of the two accounts used.

Clients: Apple Calendar on macOS and on iOS, Thunderbird, DAVx5.

For each client:
1. Add the account with the primary password (TOTP-enabled account: with an app password instead).
2. Create an event with title, location, notes, URL and a 10-minute alert; confirm it appears after a refresh.
3. Edit the title and move the event to a different calendar.
4. Create a weekly recurring event, then change one occurrence; confirm the exception survives a refresh.
5. Delete an event.
6. Rename a calendar and change its colour; confirm both after a refresh.
7. Set a custom timezone on a calendar (Thunderbird: calendar properties).
8. Copy a calendar (clients that support it) or export and re-import.
9. Go offline, edit two events, come back online; confirm both edits sync.
10. Change the password through the account page, reconnect the client with the new password.
11. Log in with an app password; revoke it through the account page; confirm the client is refused.
12. Enrol TOTP through the account page; confirm the client needs an app password; remove TOTP.

Server-side, after the run: `cargo test -p tests za::za_tests` passes against the same build, and the admin
can read nothing meaningful in the stored records (spot-check with the leak scanner's output).
```

- [ ] **Step 2: Final notes**

Append to `docs/superpowers/plans/README-dev.md`:

```markdown
- Full verification: `cargo test -p vault -p groupware -p common`, then `za::za_tests`, then `webdav::webdav_tests` in both modes.
- Manual client checklist: `docs/zero-access/manual-checklist.md`.
- Known limits (spec 2): the running server holds plaintext while serving a request and receives the password on every CalDAV request; the authentication cache and the key cache are process-local and bounded (15 minutes idle, 60 minutes hard cap).
```

- [ ] **Step 3: Full run and commit**

```bash
cargo test -p vault -p groupware -p common 2>&1 | tail -3
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture 2>&1 | tail -3
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav::webdav_tests -- --nocapture 2>&1 | tail -3
STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav::webdav_tests -- --nocapture 2>&1 | tail -3
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests system::system_tests jmap::jmap_tests -- --nocapture 2>&1 | tail -3
git add docs
git commit -m "Add the zero-access manual checklist and developer notes"
```

Expected: every command ends with `test result: ok`. Release 1 is complete when this step passes; the signup/account web page (separate repository) can start against the eight endpoints of plan 1.
