# Plan 17, group A: the move (Tasks 1 to 9)

These tasks belong to [zero-access plan 17: JMAP calendars and browser sign-in](2026-11-14-zero-access-17-jmap-calendars.md), whose header holds the commands, the constraints and the task index. "I §n" is section n of the [interface sheet](2026-11-14-zero-access-17-jmap-calendars-interfaces.md).

### Task 1: the pipeline module and its neutral error

**Spec:** 5.1 (the protocol-neutral error), 8 (the error table), 11 (the
`test_mode` wiring; the rule that follows `failed_precondition`).

**Files:**
- Create: `crates/groupware/src/calendar/pipeline/mod.rs`,
  `crates/groupware/src/calendar/pipeline/error.rs`.
- Modify: `crates/groupware/src/calendar/mod.rs:24-25` (`pub mod pipeline;`
  after `pinning`, in rustfmt order); `crates/dav/src/calendar/mod.rs`
  (the `From` impl after `impl ItipPrecondition for ItipError`,
  `:736-759`; a test module `pipeline_error_tests` at the end of the
  file); `crates/dav/Cargo.toml:30`; `crates/jmap/Cargo.toml:54`.
- Test: `crates/groupware/src/calendar/pipeline/error.rs` and
  `crates/dav/src/calendar/mod.rs` (unit).

**Interfaces:**
- Consumes: `ZaCalendarError` and its `into_store_error`
  (`crates/groupware/src/calendar/sharing/resolve.rs:36-59`); `ItipError`
  (`crates/groupware/src/scheduling/mod.rs:124-156`, 30 variants, `Debug`
  only); `ItipPrecondition::failed_precondition`
  (`crates/dav/src/calendar/mod.rs:732-759`).
- Produces: `ZaScheduling`, `ZaWritten`, `ZaDeleted` (I §2.1);
  `ZaPipelineError`, `ZaRefusal`, `ZaBusy`, `ZaPipelineError::from_calendar`,
  `za_read_refusal`, `za_itip_fails_write` and
  `impl From<ZaPipelineError> for DavError` by the CalDAV table (I §2.2);
  the two feature lines (I §2.12).

**Constraints:**
- `za_itip_fails_write` is a `match` that names every `ItipError` variant
  with no `_` arm, so a variant upstream adds stops the build. True for
  these 17: `MultipleOrganizer`, `OrganizerIsLocalAddress`,
  `SenderIsNotParticipant(_)`, `OrganizerMismatch`,
  `CannotModifyProperty(_)`, `CannotModifyInstance`, `CannotModifyAddress`,
  `MissingUid`, `MultipleUid`, `MultipleObjectTypes`,
  `MultipleObjectInstances`, `MissingMethod`, `InvalidComponentType`,
  `OutOfSequence`, `UnknownParticipant(_)`, `UnsupportedMethod(_)`,
  `TooManyRecipients`. False for these 13: `NoSchedulingInfo`,
  `OtherSchedulingAgent`, `NotOrganizer`, `NotOrganizerNorAttendee`,
  `NothingToSend`, `SenderIsNotOrganizerNorAttendee`,
  `SenderIsNotOrganizer`, `ICalendarParseError`, `EventNotFound`,
  `EventTooLarge`, `QuotaExceeded`, `NoDefaultCalendar`, `AutoAddDisabled`.
- In the CalDAV mapping, `Scheduling(err)` whose `failed_precondition()`
  is `None` maps to `Code(PRECONDITION_FAILED)`; it is unreachable, and
  the second dav test below proves it. `TooManyRecipients` carries no
  details; `Scheduling(err)` carries `err.to_string()`.
- No `impl From<SealError> for ZaPipelineError` and no
  `impl From<ZaCalendarError> for ZaPipelineError`: a seal error goes
  through `seal_error(err, account, document)`, a resolver error through
  `from_calendar` with its ids.
- `from_calendar` is the body of dav's `za_calendar_error`
  (`crates/dav/src/calendar/mod.rs:762-778`) with the neutral variants;
  `za_read_refusal` is the body of dav's `calendar_item_error`
  (`crates/dav/src/common/za.rs:369-385`). Both dav functions stay as
  they are in this task.
- `ZaWritten` and `ZaDeleted` derive nothing.
- `ItipPrecondition` and every handler body are untouched.

**Tests.**

`calendar_errors_keep_their_scopes` (`groupware`, in `error.rs`), owner 1
and document 2 throughout:
- Write scope, `ZaPipelineError::from_calendar`: `Forbidden` gives
  `Forbidden(ZaRefusal::Context)`; `LinkedEvent` gives `LinkedEvent`;
  `Store(StoreEvent::NotFound)` gives `NotFound`;
  `Store(StoreEvent::DataCorruption)` gives `Store(e)` where `e` matches
  `StoreEvent::DataCorruption`; `Seal(SealError::Aead)` gives `Store(_)`.
- Read scope, `za_read_refusal`: `Forbidden` gives
  `Err(Forbidden(ZaRefusal::Context))`; `LinkedEvent` gives
  `Err(LinkedEvent)`; `Store(NotFound)` gives `Err(NotFound)`;
  `Seal(SealError::Aead)` gives `Ok(_)`; `Store(DataCorruption)` gives
  `Ok(e)` where `e` matches `StoreEvent::DataCorruption`.

`pipeline_errors_map_to_todays_answers` (`dav`, in
`pipeline_error_tests`), each through `DavError::from`:
- `Forbidden(r)` for each of `NoKeys`, `Context`, `SharedCopy`,
  `LinkedMove`: `Code(FORBIDDEN)`.
- `LinkedEvent`: `Code(CONFLICT)`. `NotFound`: `Code(NOT_FOUND)`.
- `Busy(b)` for `LeaseBusy` and `CreateLost`:
  `Code(SERVICE_UNAVAILABLE)`.
- `UidConflict { href: Some("/dav/cal/a/b/c.ics") }`: a condition with
  code `FORBIDDEN`, condition `CalCondition::NoUidConflict` of that href,
  and no details. `UidConflict { href: None }`: `Code(FORBIDDEN)`.
- `TooManyRecipients`: a condition with code `PRECONDITION_FAILED`,
  `CalCondition::MaxAttendeesPerInstance`, no details.
- `Scheduling(ItipError::MultipleOrganizer)`: code `PRECONDITION_FAILED`,
  `CalCondition::SameOrganizerInAllComponents`, details
  `Some(ItipError::MultipleOrganizer.to_string())`.
- `SchedulingDenied("denied")`: `Code(FORBIDDEN)`.
- `Store(StoreEvent::AssertValueFailed)`: `Internal(e)` with
  `e.is_assertion_failure()`. `Store(SecurityEvent::Unauthorized)`:
  `Internal(e)` matching that event. The dispatcher decides a store
  error, so it must arrive unchanged.

`itip_precondition_classes_agree` (`dav`, same module): an array of all
30 variants, the payload ones built as
`CannotModifyProperty(ICalendarProperty::Summary)`,
`SenderIsNotParticipant(String::new())`,
`UnknownParticipant(String::new())` and
`UnsupportedMethod(ICalendarMethod::Publish)`. For each,
`za_itip_fails_write(e) == e.failed_precondition().is_some()`; and
exactly 17 of them fail a write.

Steps:

- [ ] Write the three tests.
- [ ] Run U-groupware `calendar_errors_keep_their_scopes` and U-ws
  `pipeline_error_tests`. Expected: neither compiles (`pipeline` is not a
  module of `groupware::calendar`).
- [ ] Create `pipeline/mod.rs` with `pub mod error;` and the three types
  of I §2.1; declare `pub mod pipeline;`.
- [ ] Fill `pipeline/error.rs` (I §2.2).
- [ ] Add `impl From<ZaPipelineError> for DavError` to
  `crates/dav/src/calendar/mod.rs` by the CalDAV table of I §2.2.
- [ ] Wire the feature. `crates/dav/Cargo.toml:30` becomes
  `test_mode = ["common/test_mode", "groupware/test_mode"]`;
  `crates/jmap/Cargo.toml:54` becomes
  `test_mode = ["utils/test_mode", "common/test_mode", "groupware/test_mode"]`.
  Without it a hook that later tasks move into `groupware` compiles out
  of a `dav` or `jmap` test-mode build.
- [ ] Run both commands again. Expected: one test passes in `groupware`,
  two in `dav`.
- [ ] The closing checks. No suite runs: no handler changed.
- [ ] Commit: `Release 6: the pipeline module and its neutral error`.

### Task 2: admission, roles, contexts and read helpers

**Spec:** 5.1 (the admission and role helpers, the context helpers, the
listing of the calendars a grantee may see), 6.8 (D15, the predicate
only).

**Files:**
- Create: `crates/groupware/src/calendar/pipeline/admission.rs`,
  `crates/groupware/src/calendar/pipeline/context.rs`.
- Modify: `crates/groupware/src/calendar/pipeline/mod.rs` (declare both);
  `crates/groupware/src/calendar/sharing/resolve.rs:72-77` (an `impl`
  after `ZaSyncBoundary`) and its test module (`:537`);
  `crates/dav/src/common/za.rs:96-115,201-216,324-436,499-529`;
  `crates/dav/src/calendar/mod.rs:761-889`;
  `crates/dav/src/common/propfind/za.rs:456-491`;
  `crates/dav/src/common/propfind/get.rs:156-166`.
- Test: `crates/groupware/src/calendar/sharing/resolve.rs` (unit).

**Interfaces:**
- Consumes: `ZaPipelineError`, `ZaRefusal`, `from_calendar`,
  `za_read_refusal`, the CalDAV mapping (I §2.2).
- Produces: `za_calendar_admission`, `za_writer_keys`, `ZaRoles`,
  `za_roles` (I §2.3); `ZaReadMemo`, `za_mutation_context`,
  `za_read_context`, `za_event_view_in`, `za_read_event_context`,
  `za_read_event_view`, `za_read_calendar_view`, `za_write_contexts`,
  `za_refresh_event`, `za_foreign_calendar_contexts`, and
  `ZaSyncBoundary::refuses` (I §2.4).

**What moves, and what stays behind in `dav`:**

| Old item (`dav`) | New item (`groupware::calendar::pipeline`) | What `dav` keeps |
|---|---|---|
| `ZeroAccessGate::za_calendar_gate` body, `common/za.rs:96-115` | `admission::za_calendar_admission` | the trait method, calling it and mapping the error with `?` |
| `za_event_view_in`, `common/za.rs:201-216` | `context::za_event_view_in` | `pub(crate) use` of it in `common/za.rs` |
| `ZaReadMemo`, `:324-325` | `context::ZaReadMemo` | `pub(crate) use` |
| `za_mutation_context`, `:327-344` | `context::za_mutation_context` | `pub(crate) use` |
| `za_read_context`, `:346-365` | `context::za_read_context` | `pub(crate) use` |
| `calendar_item_error`, `:367-385` | `error::za_read_refusal` (Task 1) | a private fn of the same name and signature: `za_read_refusal(..).map_err(DavError::from)` |
| `za_read_event_context`, `:387-410` | `context::za_read_event_context` | a `pub(crate)` fn of the same name and `crate::Result` shape, calling it with `?` |
| `za_read_event_view`, `:412-436` | `context::za_read_event_view` | the same kind of wrapper |
| `za_read_calendar_view`, `:499-529` | `context::za_read_calendar_view` | the same kind of wrapper |
| `za_calendar_error`, `calendar/mod.rs:761-778` | `ZaPipelineError::from_calendar` (Task 1) | a fn of the same name and signature: `ZaPipelineError::from_calendar(error, owner_id, document_id).into()` |
| `za_write_contexts`, `:780-805` | `context::za_write_contexts` | a wrapper of the same name and `crate::Result` shape |
| `za_refresh_event`, `:807-829` | `context::za_refresh_event` | a wrapper of the same name and shape (Task 7 removes it) |
| `za_writer_keys`, `:831-847` | `admission::za_writer_keys` | a wrapper |
| `ZaRoles`, `:849-857` | `admission::ZaRoles`, public fields | `pub(crate) use` |
| `za_roles`, `:859-889` | `admission::za_roles` | a wrapper |
| `foreign_calendar_contexts` body, `common/propfind/za.rs:456-491` | `context::za_foreign_calendar_contexts`, without the hook | a `pub(super)` fn of the same name and signature: it calls the new fn with `resources.document_ids(true)` and `requested`, then runs `propfind_admitted_pause_point(token.account_id())` under `#[cfg(feature = "test_mode")]`, then returns the map |
| the inline predicate, `common/propfind/get.rs:158-162` | `ZaSyncBoundary::refuses` | `context.boundary.is_some_and(\|boundary\| boundary.refuses(id))` |

Everything else in `crates/dav/src/common/za.rs` stays: `ZaFreeBusy`,
`za_uri_gate`, `za_session_keys` (with its own copy of the trace),
`za_freebusy_access`, `za_refuse_cross_account`, `za_calendar_report_uri`,
`za_item_view`, `za_item_hidden`, `za_drain_gave_up`,
`za_cached_item_hidden`, the two pending-bit helpers, `za_freebusy_views`
(it now calls the wrapper), `is_key_account`, `za_dead_fenced`,
`za_dead_fence`, `za_ordinary_fence`, `ZaDeadSeal`, `za_open_container`,
`za_reseal_container`, `za_container_buf` and both unit-test modules.

**Constraints:**
- The trace of `za_calendar_admission` is
  `Security(trc::SecurityEvent::Unauthorized)`, `AccountId = owner_id`,
  `Details = "zero-access: calendar access without session keys"`:
  `tests/src/za/sharing.rs:8600` matches the text and the account id.
- `za_writer_keys` emits no trace, as today.
- `za_foreign_calendar_contexts` resolves with `za_calendar_context`
  (the read-only resolver, as today), never with `za_mutation_context`.
  `Ok(None)`, `Err(ZaCalendarError::Forbidden)` and `Store(NotFound)`
  leave the calendar out; any other error is
  `ZaPipelineError::from_calendar(error, owner_id, id)`.
- `PROPFIND_ADMITTED_PAUSE` stays in `dav` and fires only there, after a
  successful admission. JMAP must not reach it.
- The two error scopes stay apart: a write-scope resolver error is
  `from_calendar`; a read-scope one is `za_read_refusal`, where a seal or
  store error fails one item and only a refusal fails the request.
- `za_refresh_event` keeps passing the addressed calendar id as the
  document id of `seal_error`.
- `za_mutation_context` still returns `Ok(None)` for an ordinary owner; a
  plain collection DELETE and a plain PROPPATCH call it.
- The dav unit test `read_errors_keep_their_scope`
  (`crates/dav/src/common/za.rs:753-776`) does not change at all: it
  calls the wrapper `calendar_item_error`.
- Untouched: `za_uri_gate`, `za_session_keys`, the free-busy and inbox
  helpers, the fences, the place of the PROPFIND hook.

**Test.** `sync_boundary_refuses_as_caldav` (`groupware`, in
`resolve.rs`'s `tests`):
- `{ grant_change_id: 10, rename_change_id: 20, pending: true }` refuses
  0, 10, 20, 21 and `u64::MAX`.
- `{ 10, 20, pending: false }` refuses 0, 19 and 20, and does not refuse
  21.
- `{ 30, 20, pending: false }` refuses 30 and does not refuse 31.

Steps:

- [ ] Write the test.
- [ ] Run U-groupware `sync_boundary_refuses_as_caldav`. Expected: no
  method `refuses` on `ZaSyncBoundary`.
- [ ] Add `ZaSyncBoundary::refuses` and use it at
  `crates/dav/src/common/propfind/get.rs:158-162`.
- [ ] Create `pipeline/admission.rs` and move the three bodies and
  `ZaRoles` into it by the table; leave the dav wrappers.
- [ ] Create `pipeline/context.rs` and move the context and read helpers
  into it by the table; leave the dav wrappers and re-exports. Remove the
  imports the move leaves unused in the three dav files.
- [ ] Run U-groupware `sync_boundary_refuses_as_caldav` and U-ws
  `read_errors_keep_their_scope`. Expected: both pass.
- [ ] Run ZA, WK and WP. The modules that prove this task: `dav_gate`,
  `dav_seal::{test, test_reports, test_collections, test_conditional,
  test_legacy, test_dead_properties}`, `sharing_tombstones`,
  `sharing_races` (the `PROPFIND_ADMITTED_PAUSE` case still parks),
  `sharing` (the trace at `sharing.rs:8600`), `gating`, `expansion`,
  `pairs`.
- [ ] The closing checks. An unused wrapper or import in `dav` shows
  there as a warning.
- [ ] Commit: `Release 6: move calendar admission, roles and contexts to groupware`.

### Task 3: the UID lease functions

**Spec:** 5.1 (the UID lease functions of a create), 5.2 (a create checks
UID uniqueness under the lease in every calendar it names).

**Files:**
- Create: `crates/groupware/src/calendar/pipeline/uid.rs`.
- Modify: `crates/groupware/src/calendar/pipeline/mod.rs` (declare it);
  `crates/dav/src/calendar/mod.rs:483-730` (five functions and one enum
  leave); `crates/dav/src/calendar/update.rs:10` (imports), `:913-921`,
  `:1449-1458`, `:1556-1566`, `:1571-1580` (the call sites);
  `crates/common/src/za/test_hooks.rs:20-22,27-29` (the doc table).
- Test: none new.

**Interfaces:**
- Consumes: `ZaPipelineError`, `ZaBusy`, the CalDAV mapping (I §2.2);
  `ZaUidLease`, `ZaUidLeaseGuard` (`crates/common/src/za/lease.rs:159-306`).
- Produces: `za_take_uid_lease`, `za_assert_is_unique_uid`,
  `za_lost_create_fence`, `za_commit_create` (I §2.5).

**What moves, and what stays behind in `dav`:**

| Old item, `crates/dav/src/calendar/mod.rs` | New item, `pipeline/uid.rs` |
|---|---|
| `za_take_uid_lease`, `:483-523` | `za_take_uid_lease` |
| `za_assert_is_unique_uid`, `:525-598` | `za_assert_is_unique_uid` |
| `za_lost_create_fence`, `:600-628` | `za_lost_create_fence` |
| `CreateLoss`, `:630-633` | private `CreateLoss` |
| `za_finish_create_loss`, `:635-679` | private `za_finish_create_loss`, returning `Result<Infallible, ZaPipelineError>` |
| `za_commit_create`, `:681-730` | `za_commit_create` |

`dav` keeps no wrapper for these: its four call sites in
`crates/dav/src/calendar/update.rs` call the `groupware` functions with a
one-element slice. `assert_is_unique_uid` (`mod.rs:120-151`, the snapshot
rule the plain path and COPY/MOVE use), `validate_ical` and
`SupportedComponents` stay in `dav`, untouched.

**Differences from the old bodies, and no others:**
1. `calendar_id: u32` becomes `calendar_ids: &[u32]`.
   `za_take_uid_lease` runs `za_assert_is_unique_uid` for each calendar
   in slice order; the first error releases the lease
   (`lease.release().await`) and is returned.
2. `za_finish_create_loss` retakes with the whole slice.
3. `za_commit_create` returns
   `assigned.last_change_id(owner_id, SyncCollection::Calendar)` of its
   `commit_batch` instead of `()`.
4. Errors: a busy lease is `Busy(ZaBusy::LeaseBusy)`; the loss's final
   answer is `Busy(ZaBusy::CreateLost)`; a visible conflict is
   `UidConflict { href: Some(href) }` with the href built as today
   (`resources.format_item("{calendar path}/{name}")`, empty when the
   calendar is not in `resources`); a hidden one is
   `UidConflict { href: None }`.

**Constraints:**
- The traces move verbatim, each
  `Store(trc::StoreEvent::AssertValueFailed)` with `AccountId`:
  `"zero-access: UID creation lease busy"`,
  `"zero-access: UID creation lease lost before the commit"`,
  `"zero-access: create retry lost its assertion"`. The lost-fence trace
  is emitted before the retake.
- The hooks move with `za_commit_create`, in this order:
  `take_commit_fail(owner_id)` first (it releases the lease and returns
  the store error with details
  `"zero-access test hook: sender commit failure"`), then
  `commit_pause_point(owner_id).await`.
- The lease is released exactly where it is today: explicitly before a
  UID error returns, after a successful commit, after a non-assertion
  commit error; abandoned on a confirmed lease loss; released, result
  ignored, on an opaque second loss. Add no release and remove none.
- `za_finish_create_loss` keeps a return type that cannot be `Ok`, so a
  lost create cannot reach a mail submission (iMIP invariant 46).
- The hidden-conflict rule stays: a caller that is not a member of the
  owner account and whose only conflict is a secret event gets
  `UidConflict { href: None }`.

Steps:

- [ ] Create `pipeline/uid.rs` and move the six items into it with the
  four differences.
- [ ] Remove the six items from `crates/dav/src/calendar/mod.rs`, with
  the imports they alone used.
- [ ] Repoint the four call sites in `crates/dav/src/calendar/update.rs`.
  `za_take_uid_lease` (`:913`): pass `&[parent.document_id()]`; the `?`
  maps the error. Both `za_lost_create_fence` sites (`:1449`, `:1571`):
  pass `&[calendar_id]`, bind the result with `let Err(error) = .. .await;`
  (it cannot be `Ok`) and `return Err(error.into());`.
  `za_commit_create` (`:1556`): pass `&[calendar_id]`, map the error with
  `.map_err(DavError::from)` inside `za_mail.on_err(..)`, and discard the
  returned change id in this task.
- [ ] Update the doc table of `crates/common/src/za/test_hooks.rs`: the
  `COMMIT_PAUSE` entry (`:20-22`) and the `COMMIT_FAIL_ONCE` entry
  (`:27-29`) name `groupware::calendar::pipeline::uid::za_commit_create`.
- [ ] Run ZA, WK and WP. The modules that prove this task: `uid_lease`
  (`PUT_PAUSE`, `COMMIT_PAUSE`, the busy and lost-fence 503s, the 403 of
  a found copy), `fanout::test_failures` and `drain`
  (`COMMIT_FAIL_ONCE`), `tracing`, `imip::outbound`, `imip::traces`; in
  key-mode `webdav_tests`, `put_get` and `za_variants::copy_move`
  (`NoUidConflict`).
- [ ] The closing checks.
- [ ] Commit: `Release 6: move the UID lease functions to groupware`.

### Task 4: the scheduling assembly

**Spec:** 5.1 (the scheduling assembly and the status step), 5.4 (D4,
D18).

**Files:**
- Create: `crates/groupware/src/calendar/pipeline/schedule.rs`.
- Modify: `crates/groupware/src/calendar/pipeline/mod.rs` (declare it);
  `crates/dav/src/calendar/update.rs:494-543` (the update's scheduling
  head and key arm), `:1000-1038` (the create's), `:1227-1351`
  (`za_schedule`, `za_itip_precondition`, `za_second_run` leave),
  `:1604-1641` (the unit test's one call expression).
- Test: `crates/dav/src/calendar/update.rs` (the existing unit test).

**Interfaces:**
- Consumes: `ZaPipelineError`, `za_itip_fails_write` (I §2.2);
  `ZaScheduling` (I §2.1); `ZaRoles` (I §2.3); `ItipSendStatus`
  (`crates/groupware/src/calendar/itip.rs:1394-1461`); the delegation
  functions of `crates/groupware/src/calendar/delegation.rs`;
  `itip_create`, `itip_update`, `itip_set_unreachable_status`,
  `RecipientPolicy`.
- Produces: `ZaScheduled`, `za_schedule`, `za_second_run`,
  `za_schedule_create`, `za_schedule_update` (I §2.6).
  `za_delete_send_itip` is Task 12's.

**What moves, and what stays behind in `dav`:**

| Old item, `crates/dav/src/calendar/update.rs` | New item, `pipeline/schedule.rs` |
|---|---|
| `za_schedule`, `:1227-1324` | `za_schedule`, same logic |
| `za_itip_precondition`, `:1326-1334` | gone: its two callers test `za_itip_fails_write(&err)` and return `ZaPipelineError::Scheduling(err)` |
| `za_second_run`, `:1336-1351` | `za_second_run` |
| the status and key arm of the update, `:498-543`, with its `reason` trace, `:619-626` | `za_schedule_update` |
| the status and key arm of the create, `:1004-1038`, with its `reason` trace, `:1078-1084` | `za_schedule_create` |

`dav` keeps, untouched: upstream's `is_send()` arms (`:544-618`,
`:1039-1077`), the plain `reason` traces (`:619-626`, `:1078-1084`), the
`ItipPrecondition` trait and its uses there, and the unit test
`second_run_fails_only_on_a_failed_precondition`.

**The status step, the same in both functions.** Let `end` be
`content.data.event_range_end()` (the new content's):

1. `ZaScheduling::Off` gives `ItipSendStatus::NotRequested`. In
   `za_schedule_update`, `old == None` also gives `NotRequested`, whatever
   `schedule` is (today's `:498-499`, where "user data only" is exactly
   "no old tree").
2. `On` and `Required` give
   `ItipSendStatus::resolve(server, token, &roles.sender, end, true)`.
3. `Required` with `status.is_denied()` returns
   `Err(ZaPipelineError::SchedulingDenied(reason))`, where `reason` is
   `status.reason()`, before anything is mutated.
4. `SendSealed` runs, in today's order: `za_resolve_delegation`
   (`caused_by(trc::location!())` on its error), `za_schedule`,
   `itip_set_unreachable_status(&mut tree, roles.sender.addresses())`.
   - Create: operation `ZaSelfOperation::Create`, `old` `None`, policy
     `RecipientPolicy::new(&server.core.groupware, event.flags)`,
     `document_id` `None`.
   - Update: `is_update = new_event.schedule_tag.is_some() && old.is_some()`;
     operation `Update` when `is_update`, else `Create`;
     `za_resolve_delegation` gets `old` as given; `za_schedule` gets
     `old.filter(|_| is_update)` and `Some(document_id)`.
5. Any other status with a `reason()` is traced as today:
   `Calendar(trc::CalendarEvent::ItipMessageError)`, `AccountId = owner_id`,
   `Reason = reason`, and for an update also `DocumentId = document_id`.
   The result is then `ZaScheduled { messages: None, resolution: None }`.

**Constraints:**
- `za_schedule`'s logic is unchanged: the second run sees the
  pre-finalize trees; an error that `za_itip_fails_write` accepts returns
  `Scheduling(err)`; any other first-run error is traced
  (`ItipMessageError`, `AccountId`, `DocumentId`, `Reason`) and bumps the
  schedule tag once when `old` is `Some`; the recipient cap
  (`>= server.core.groupware.itip_outbound_max_recipients`) returns
  `TooManyRecipients`.
- The status is resolved with the sender's roles, never the target
  account's addresses, so a grantee's write carries `SENT-BY` (D18).
- In the create branch the handler holds the UID lease guard while
  `za_schedule_create` runs. Its error returns with `?`, and the guard's
  drop releases the lease, as the old arm's `?` returns did. Add no
  explicit release there.
- CalDAV passes `ZaScheduling::On` only. `Off` and `Required` are
  written here and get their tests in Task 31.
- Upstream's `is_send()` arms and the plain trace are untouched.

Steps:

- [ ] Change the unit test's call expression first. In `mod tests` of
  `crates/dav/src/calendar/update.rs`: import
  `groupware::calendar::pipeline::schedule::za_second_run` and
  `groupware::scheduling::ItipError` inside the module, and change only
  the first `match` head to
  `match za_second_run(Err(ItipError::TooManyRecipients)).map_err(DavError::from) {`.
  Every arm and every other line of the test stays.
- [ ] Run U-ws `second_run_fails_only_on_a_failed_precondition`.
  Expected: `unresolved import groupware::calendar::pipeline::schedule`.
- [ ] Create `pipeline/schedule.rs` with `ZaScheduled`, the moved
  `za_schedule` and `za_second_run`, and the two new functions by the
  status step.
- [ ] Replace the update's key arm (`update.rs:494-543`). Keep the three
  `let mut` declarations. The head and the first arm become:

  ```rust
  let itip_status = if is_user_data_only || za_roles.is_some() {
      // A key account's status is resolved in the pipeline (D18).
      ItipSendStatus::NotRequested
  } else {
      ItipSendStatus::resolve(
          self,
          access_token,
          &account_info,
          new_content.data.event_range_end(),
          false,
      )
  };
  if let Some(roles) = &za_roles {
      let scheduled = za_schedule_update(
          self,
          access_token,
          roles,
          ZaScheduling::On,
          old_ical.as_ref(),
          &mut new_event,
          &mut new_content,
          account_id,
          document_id,
      )
      .await?;
      za_messages = scheduled.messages;
      za_resolution = scheduled.resolution;
  } else if itip_status.is_send() {
  ```

  The `is_send()` arm and the `reason` arm after it stay byte for byte.
- [ ] Replace the create's key arm (`update.rs:1000-1038`) the same way:
  `itip_status` is `NotRequested` when `za_roles.is_some()`, else
  `ItipSendStatus::resolve(self, access_token, &account_info, content.data.event_range_end(), false)`;
  the first arm is `if let Some(roles) = &za_roles`, calling
  `za_schedule_create(self, access_token, roles, ZaScheduling::On, &mut event, &mut content, account_id).await?`
  and assigning `za_messages` and `za_resolution`.
- [ ] Delete `za_schedule`, `za_itip_precondition` and `za_second_run`
  from `update.rs`, with the imports they alone used. An item only the
  unit test names is imported inside `mod tests`.
- [ ] Run U-ws `second_run_fails_only_on_a_failed_precondition`.
  Expected: it passes.
- [ ] Run ZA, WK and WP. The modules that prove this task: `fanout`,
  `reply`, `pinning`, `sharing` (delegated scheduling, `SENT-BY`, ruling
  197's recipient limit on the second run), `imip::outbound` (the
  recipient sets); in key-mode `webdav_tests`,
  `za_variants::scheduling`. Plain mode proves the upstream arms did not
  move.
- [ ] The closing checks.
- [ ] Commit: `Release 6: move the scheduling assembly to groupware`.

### Task 5: the create commit

**Spec:** 5.1 (the commit of an event create, with its fan-out, sealing,
fences, bounded retry and mail guard), 5.3 (one commit per object).

**Files:**
- Create: `crates/groupware/src/calendar/pipeline/create.rs`.
- Modify: `crates/groupware/src/calendar/pipeline/mod.rs` (declare it);
  `crates/dav/src/calendar/update.rs:1097-1120` (the call), `:1353-1602`
  (`ZaCreate` and `za_create_event` leave);
  `crates/common/src/za/test_hooks.rs:20-22` (the `COMMIT_PAUSE` line).
- Test: none new.

**Interfaces:**
- Consumes: `ZaWritten` (I §2.1); `ZaRoles` (I §2.3);
  `za_refresh_event` (I §2.4); `za_lost_create_fence`, `za_commit_create`
  (I §2.5); `ZaScheduled` (I §2.6); `ZaFanOut::za_fan_out`,
  `ZaFanOutKind::Change`, `ZaSource`, `za_set_schedule_status`
  (`crates/groupware/src/calendar/fanout.rs`); `za_set_sequences`
  (`sequence_floor.rs`); `seal_event_in_domain`, `seal_error`, `ZaEvent`;
  `ZaImipPending` (`crates/common/src/za/imip.rs`).
- Produces: `ZaEventCreate`, `za_create_event` (I §2.7).

**What moves, and what stays behind in `dav`:**

| Old item, `crates/dav/src/calendar/update.rs` | New item, `pipeline/create.rs` |
|---|---|
| `ZaCreate`, `:1353-1367` | `ZaEventCreate` |
| `za_create_event`, `:1369-1602` | `za_create_event` |

`dav` keeps every step of the create handler before the call
(`:863-1095`): the ACL and privacy checks, `validate_user_data`,
`validate_headers`, `validate_ical`, the lease call, supported
components, the object quota, the build of the event and its data,
`za_write_contexts(.., Acl::AddItems)`, the next alarms, the scheduling
call, the enterprise-licensed tenant-quota snippet (`:1086-1095`, with
its SPDX snippet markers, on the raw request length), and the whole
plain create (`:1122-1181`). It also keeps `serves_ical` and the
response.

**Differences from the old private function, all neutral for CalDAV:**
1. `lease` is required. The `lease == None` branch (`:1589-1593`) is
   removed, and every `if let Some((lease, _))` becomes unconditional.
   `uid` is its own field.
2. `submitted_ical`, `is_owner` and `personal_id` leave the struct. The
   function clones the tree right after `za_set_sequences` and before
   sealing (where `:1473-1479` computed the old flag) and returns it as
   `ZaWritten.ical`.
3. `calendar_id` leaves the struct. The calendars of the lost-fence
   re-check and of `za_commit_create` are the parents of `event.names`,
   without duplicates, in name order; the retry's addressed calendar is
   `event.names[0].parent_id`. Read them before `event` moves into
   `ZaEvent::new`.
4. `ZaWritten.change_id` is
   `assigned.last_change_id(owner, SyncCollection::Calendar)` of the first
   commit, or the value `za_commit_create` returns for the retry; `etag`
   is the insert's (the retry's when it ran); `schedule_tag` is
   `event.schedule_tag` read before sealing; `document_id` is the
   reserved id.
5. An empty `contexts` (no caller passes one) releases the lease and
   returns `Forbidden(ZaRefusal::Context)`; the function never indexes an
   empty vector.

**Constraints:**
- The order is today's: the reservation-only batch (ruling 14: the id is
  assigned before the fan-out); the fan-out with the lease as its fence;
  statuses, sequences and pairs; sealing in `contexts[0].domain`; the
  insert batch (`limit_disk_quota`, the lease assertion, every context's
  assertion, `insert_at` with one timestamp for both attempts); the
  hooks; the commit; on an assertion loss
  `za_refresh_event(.., Acl::AddItems, ..)`, then either the rebuilt
  batch through `za_commit_create` or `za_lost_create_fence`; then
  `notify_task_queue`, then the mail.
- The lease is released exactly where it is today. Explicit
  `release().await`: after a failed reservation commit; after a fan-out
  error that is not an assertion loss; after a seal error; when the test
  hook fails the commit; after the first commit returns `Ok`; after a
  non-assertion commit error. A fan-out assertion loss, and a commit
  assertion loss without a calendar change, hand the guard to
  `za_lost_create_fence`. The retry hands it to `za_commit_create`. The
  two `?` returns that today drop the guard (`build(..)?` and
  `za_refresh_event(..).await?`) still drop it. Add no release.
- The mail guard is settled exactly where it is today:
  `za_mail.dropped()` when the hook fails the commit, before
  `za_lost_create_fence` at the commit, and on a non-assertion commit
  error; `za_mail.on_err(za_commit_create(..).await)?` on the retry;
  `za_mail.submit(server).await` only after the commit returned `Ok`,
  after `notify_task_queue`. The returns that today leave it unsettled
  (the seal error, `build(..)?`, `za_refresh_event(..).await?`) stay
  unsettled and trace `ZA_IMIP_ABANDONED`; add no `dropped()` there.
- The hooks move with the first commit, in this order:
  `commit_pause_point(owner).await`, then `take_commit_fail(owner)`,
  whose error has details `"zero-access test hook: sender commit failure"`.
- The plain create (`update.rs:1122-1181`) is untouched.

Steps:

- [ ] Create `pipeline/create.rs` with `ZaEventCreate` and the moved
  `za_create_event`, with the five differences.
- [ ] Replace the call in the handler (`update.rs:1097-1120`):

  ```rust
  if let (Some(keys), Some(roles), Some((lease, uid))) = (&za_keys, &za_roles, za_lease.take()) {
      let written = za_create_event(
          self,
          access_token,
          &resources,
          &account,
          keys,
          roles,
          ZaEventCreate {
              event,
              content,
              next_alarms: next_email_alarms,
              scheduled: ZaScheduled { messages: za_messages, resolution: za_resolution },
              lease,
              uid,
              contexts: std::mem::take(&mut contexts),
          },
      )
      .await?;
      // Today's expression (`update.rs:1473-1479` at the base): a key
      // account's content holds no preferences, so the view applies none.
      let serves_submitted_ical = if is_owner {
          serves_ical(&written.ical, &submitted_ical)
      } else {
          let mut view = written.ical.clone();
          view.apply_user_data(None, UserDataView::AlertsOnly);
          serves_ical(&view, &submitted_ical)
      };
      return Ok(HttpResponse::new(StatusCode::CREATED)
          .with_etag_opt(serves_submitted_ical.then_some(written.etag))
          .with_schedule_tag_opt(written.schedule_tag));
  }
  ```

- [ ] Delete `ZaCreate` and `za_create_event` from `update.rs`, with the
  imports they alone used.
- [ ] Update the doc table of `crates/common/src/za/test_hooks.rs`: the
  `COMMIT_PAUSE` entry says the pause sits in
  `groupware::calendar::pipeline::create::za_create_event`, immediately
  before the leased create commits, and again in `za_commit_create`.
- [ ] Run ZA, SWAP, WK and WP. The modules that prove this task:
  `uid_lease`, `fanout::{test_task_queue, test, test_failures,
  test_plain_owner, test_storage_guards}`, `reply`, `drain`,
  `drain_limits`, `pinning`, `imip::{inbound, outbound, oracle, traces}`,
  the sharing modules, `tracing`, `expansion`, the leak modules,
  `mail::fence` (the plain PUT's fence is unchanged).
- [ ] The closing checks.
- [ ] Commit: `Release 6: move the key create commit to groupware`.

Review note: the reviewer confirms that the removed `lease == None`
branch had no caller at the base. `za_create_event`'s one caller passed
`za_lease.take()`, which is `Some` exactly when `za_keys` is.

### Task 6: placement contexts, refusals and the event container's reseal

**Spec:** 5.1 (the contexts of a placement change, the two placement
refusals, the reseal of an event's dead-property container), 5.2 (the
placement rule and the rights it asks), 3.1 (D17).

**Files:**
- Create: `crates/groupware/src/calendar/pipeline/placement.rs`.
- Modify: `crates/groupware/src/calendar/pipeline/mod.rs` (declare it);
  `crates/dav/src/calendar/copy_move.rs:1898-1993` (`shared_copy_refusal`
  becomes a wrapper; `linked_move_refusal` and `za_move_contexts` leave),
  and four sites inside `za_copy_move_stored_event`: `:2159-2170`,
  `:2200-2210`, `:2272-2292`, `:2371-2382`.
- Test: `crates/groupware/src/calendar/pipeline/placement.rs` (unit).

**Interfaces:**
- Consumes: `ZaPipelineError`, `ZaRefusal`, `from_calendar` (I §2.2);
  `za_mutation_context` (I §2.4); `Server::metadata_container`
  (`crates/common/src/storage/metadata/read.rs:105`); `reseal_container`
  (`crates/groupware/src/calendar/seal/container.rs:310`);
  `ContainerChange::new` and `into_write`
  (`crates/common/src/storage/metadata/change.rs:17-82`);
  `MetadataWrite::build`.
- Produces (I §2.9): `ZaPlacementPlan` with `caldav`, `is_change`,
  `parent_rights`, `entered_rights`, `shared_refusal` and the private
  `steps`; `ZaPlacementContexts` with `unchanged`, `from_domain`,
  `to_domain`, `assert_in`, `into_vec`; `za_placement_contexts`;
  `za_shared_copy_refusal`; `za_linked_move_refusal`;
  `za_reseal_event_container`. `ZaPlacementPlan::jmap` is Task 11's.

**The rule**, which is today's for every CalDAV input:

- **`caldav`.** `parents`: the distinct parents of `event.names`,
  ascending. `names`: `event.names.len()`. `source`: `Some(source)`.
  `left`: `[source]` for a MOVE, empty for a COPY. `entered`: `[target]`.
  `content_changed`: false.
- **Rights.** `parent_rights` is `RemoveItems` when `left` is not empty,
  else `ReadItems`; plus `ModifyItems` when `content_changed`.
  `entered_rights` is `AddItems`; plus `RemoveItems` when `overwrite`.
- **Shared calendars.** Let `strict` be `left.is_empty() || collection`.
  A shared current calendar: `SharedCopy` when `strict`, else
  `LinkedMove` when `names != 1`, else none. A shared entered calendar:
  `SharedCopy` when `strict`, else `LinkedMove` when `names != 1` or
  `entered.len() != 1`, else none. A calendar is shared when its
  context's `domain.calendar_id()` is `Some`.
- **Order (`steps`).** The source, when the plan names one. Then every
  other parent in ascending id. Then the entered calendars in the plan's
  order; an entered calendar that is also a parent appears again.
- **`za_placement_contexts`.** For each step: resolve with
  `za_mutation_context` and the step's rights; a resolver error is
  `from_calendar(error, owner_id, calendar)`; no context is
  `Forbidden(ZaRefusal::Context)`. For the source only, right after it
  resolved: `NotFound` unless it is one of `parents`. Then the step's
  shared check, which returns `za_shared_copy_refusal(owner_id)` or
  `za_linked_move_refusal(owner_id)`.
- **`ZaPlacementContexts::assert_in`** asserts every parent, then every
  entered context.

**The reseal.** `za_reseal_event_container` returns `Ok(())` at once when
`kinds.is_empty()`, reads the container with
`server.metadata_container(account.id, Collection::CalendarEvent, document_id)`
and returns `Ok(())` when there is none. Otherwise it reseals the stored
bytes (`stored.view().as_bytes()`) with `reseal_container` under
`ZaContainerOf::Event(uid)` and `ZaContainerScope::Event(uid)`, maps a
seal error with `seal_error(err, account.id, document_id)`, and queues
`ContainerChange::new(Some(&stored), Some(resealed), MetadataEdit::Write)`
through `into_write(account, Collection::CalendarEvent, MetadataLog::None)`
and `build(document_id, batch)`. It applies no dead-property fence: a
sealed write of a key account has none.

**What moves, and what stays behind in `dav`:**

| Old item, `crates/dav/src/calendar/copy_move.rs` | New |
|---|---|
| `shared_copy_refusal`, `:1898-1905` | `za_shared_copy_refusal`; `dav` keeps `fn shared_copy_refusal(owner_id: u32) -> DavError` as a wrapper (`za_shared_copy_refusal(owner_id).into()`) for the collection paths |
| `linked_move_refusal`, `:1906-1913` | `za_linked_move_refusal`; leaves `dav` |
| `za_move_contexts`, `:1915-1993` | `ZaPlacementPlan::caldav` and `za_placement_contexts`; leaves `dav` |
| the inline reseal, `:2272-2292`, and its read, `:2200-2210` | `za_reseal_event_container` |

`dav` keeps everything else of COPY and MOVE: the handler (`:81-567`,
with the default-calendar refusal under
`cfg(not(any(feature = "dev_mode", feature = "test_mode")))` at `:108`),
`za_copy_move_event`, the body of `za_copy_move_stored_event`, the
overwrite fences, the snapshot UID check, the supported-components check,
`assert_parent_limit`, every collection copy, move and rename path, and
all of `crates/dav/src/common/dead/` (`ContainerWrites` and
`read_container` are upstream's code with enterprise-licensed snippets).

**Constraints:**
- The refusal traces move verbatim, each
  `Security(trc::SecurityEvent::Unauthorized)` with `AccountId = owner_id`:
  `"zero-access: copy into or out of a shared calendar"` and
  `"zero-access: event linked into a shared calendar"`. The function that
  builds the error emits the trace; the dav wrapper adds none.
- A CalDAV COPY or MOVE asks exactly the rights it asks today. A right
  CalDAV's `MOVE` does not ask is a regression (spec 9.7 names it).
- `za_copy_move_stored_event` changes at the four sites only. Its vector
  indexing (`contexts[0]` the source, `contexts.last()` the target) and
  its `calendars_changed(&contexts, &fresh)` stay.
- The container write of a domain-changing MOVE stores what it stores
  today: the previous value is the stored container, the next value the
  bytes resealed under a fresh data key in the target domain. The
  reviewer confirms that passing the resealed `EncodedMetadata` directly
  equals today's round trip through `za_container_buf` and
  `EncodedMetadata::from_view` (`crates/dav/src/common/za.rs:706-723`,
  `crates/dav/src/common/dead/containers.rs:136-151`).
- The hooks of `za_copy_move_stored_event`
  (`record_move_overwrite_message`, `copy_pause_point`) and of the
  collection paths (`share_rename_pause_point`, the move plans,
  `calendar_move_delete_pause_point`) stay where they are: they fire on
  paths JMAP does not have.
- `za_test_move_event` (`crates/dav/src/calendar/mod.rs:891-925`) has no
  caller and is left as it is; it still compiles, because
  `za_copy_move_event` stays in `dav`.
- Untouched: `crates/dav/src/common/dead/`, the overwrite fences, every
  collection path's own logic.

**Tests** (`groupware`, in `placement.rs`).

`caldav_plans_keep_todays_rule`. The expected values are
`za_move_contexts` at the base
(`crates/dav/src/calendar/copy_move.rs:1918-1993`), transcribed. "Names"
is the count of the event's names.

| `is_move` | `overwrite` | `collection` | names | rights on the source and every other parent | rights on the target | a shared source or target | a shared other parent |
|---|---|---|---|---|---|---|---|
| no | no | no | 1 | `ReadItems` | `AddItems` | `SharedCopy` | `SharedCopy` |
| no | no | no | several | `ReadItems` | `AddItems` | `SharedCopy` | `SharedCopy` |
| no | no | yes | 1 | `ReadItems` | `AddItems` | `SharedCopy` | `SharedCopy` |
| no | no | yes | several | `ReadItems` | `AddItems` | `SharedCopy` | `SharedCopy` |
| no | yes | no | 1 | `ReadItems` | `AddItems`, `RemoveItems` | `SharedCopy` | `SharedCopy` |
| no | yes | no | several | `ReadItems` | `AddItems`, `RemoveItems` | `SharedCopy` | `SharedCopy` |
| no | yes | yes | 1 | `ReadItems` | `AddItems`, `RemoveItems` | `SharedCopy` | `SharedCopy` |
| no | yes | yes | several | `ReadItems` | `AddItems`, `RemoveItems` | `SharedCopy` | `SharedCopy` |
| yes | no | no | 1 | `RemoveItems` | `AddItems` | none | `LinkedMove` |
| yes | no | no | several | `RemoveItems` | `AddItems` | `LinkedMove` | `LinkedMove` |
| yes | no | yes | 1 | `RemoveItems` | `AddItems` | `SharedCopy` | `SharedCopy` |
| yes | no | yes | several | `RemoveItems` | `AddItems` | `SharedCopy` | `SharedCopy` |
| yes | yes | no | 1 | `RemoveItems` | `AddItems`, `RemoveItems` | none | `LinkedMove` |
| yes | yes | no | several | `RemoveItems` | `AddItems`, `RemoveItems` | `LinkedMove` | `LinkedMove` |
| yes | yes | yes | 1 | `RemoveItems` | `AddItems`, `RemoveItems` | `SharedCopy` | `SharedCopy` |
| yes | yes | yes | several | `RemoveItems` | `AddItems`, `RemoveItems` | `SharedCopy` | `SharedCopy` |

The test runs every row with the name parents `[1]`, `[1, 1]`, `[1, 2]`
and `[1, 2, 3]` (the row's "names" is the list's length), source 1, and
each target of 1, 2 and 9. For each plan of
`ZaPlacementPlan::caldav(&event, 1, target, is_move, overwrite, collection)`:
- `parent_rights()` and `entered_rights()` equal the row's two rights;
- `shared_refusal(false)` and `shared_refusal(true)` both equal the
  "source or target" column;
- with more than one distinct parent, `shared_refusal(false)` also equals
  the "other parent" column;
- `parents` is the distinct parents ascending, `names` the list's
  length, `source` `Some(1)`, `left` `[1]` for a move and empty for a
  copy, `entered` `[target]`, `content_changed` false, `is_change()`
  true.

`caldav_plan_order`, for an event whose names' parents are `[7, 3, 5]`:
- `caldav(&event, 5, 9, true, false, false).steps()` is
  `[(5, false), (3, false), (7, false), (9, true)]`;
- source 5, target 5 (a rename, or a copy within one calendar):
  `[(5, false), (3, false), (7, false), (5, true)]`;
- source 5, target 3, a copy: `[(5, false), (3, false), (7, false), (3, true)]`;
- source 4, which is not a parent, target 9:
  `[(4, false), (3, false), (5, false), (7, false), (9, true)]`.

Steps:

- [ ] Write the two tests.
- [ ] Run U-groupware `caldav_plan`. Expected: a compile error
  (`placement` is not declared).
- [ ] Implement the plan, the contexts and the two refusals by the rule.
- [ ] Run U-groupware `caldav_plan`. Expected: both pass.
- [ ] Implement `za_reseal_event_container` by "The reseal".
- [ ] Edit `crates/dav/src/calendar/copy_move.rs`.
  1. `shared_copy_refusal` becomes the wrapper; delete
     `linked_move_refusal` and `za_move_contexts`.
  2. `:2159-2170` and `:2371-2382`: each call of `za_move_contexts`
     becomes
     `za_placement_contexts(server, token, owner, &ZaPlacementPlan::caldav(&event, source, target, is_move, overwrite.is_some(), is_collection)).await?.into_vec()`.
  3. `:2200-2210`: delete the `read_container` call and its `container`
     binding. Nothing else in this function read it.
  4. `:2272-2292`: the inline reseal becomes, under `if rewraps`: the
     owner's account, `server.account(owner).await.caused_by(trc::location!())?`,
     then
     `za_reseal_event_container(server, &owner_account, event_id, &event.uid, event.metadata_kinds(), from, to, &mut batch).await.caused_by(trc::location!())?`.
     The account is read only on this path, as today's write reads it
     inside `ContainerWrites`.

  Remove the imports these edits leave unused (`StoredContainer` among
  them).
- [ ] Run ZA, WK and WP. The modules that prove this task:
  `sharing_move_atomic`, `sharing_review` (`COPY_PAUSE`), `sharing_races`
  (`MOVE_OVERWRITE_OBSERVATION`), `sharing` (each COPY and MOVE refusal
  with its trace), `sharing_tombstones`, `dav_gate`, `dav_seal`
  (`test_dead_properties` moves an event with dead properties into a
  shared calendar and reads them back as owner and grantee: the reseal),
  `pinning`, `mail::fence` (the container races), the leak modules; in
  key-mode `webdav_tests`, `za_variants::copy_move`. Plain mode proves
  cards and files still copy their containers.
- [ ] The closing checks.
- [ ] Commit: `Release 6: one placement resolver for COPY, MOVE and JMAP`.

Review note: the reviewer compares `za_placement_contexts` with
`za_move_contexts` at the base line by line, for the order of the
resolver calls, of the `NotFound` check and of the shared checks.

### Task 7: the update target and commit

**Spec:** 5.1 (the commit of an event update, with its fan-out, sealing,
fences, bounded retry and mail guard), 5.4 (an update carries the stored
token pairs and display name).

**Files:**
- Create: `crates/groupware/src/calendar/pipeline/update.rs`.
- Modify: `crates/groupware/src/calendar/pipeline/mod.rs` (declare it);
  `crates/dav/src/calendar/update.rs:219-256` (the contexts and the
  view), `:700-828` (the key commit); `crates/dav/src/calendar/mod.rs`
  (the wrapper `za_refresh_event` loses its last caller and goes);
  `crates/common/src/za/test_hooks.rs:23-24,30-33` (the doc table).
- Test: none new.

**Interfaces:**
- Consumes: `ZaWritten` (I §2.1); `ZaRoles` (I §2.3);
  `za_write_contexts`, `za_refresh_event` (I §2.4); `ZaScheduled`
  (I §2.6); `ZaPlacementContexts`, `ZaPlacementPlan` (I §2.9);
  `event_view_in_domain`, `event_pairs_in_domain`, `seal_event_in_domain`,
  `seal_error`, `ZaEvent`, `ZaEventView`; `ZaFanOut::za_fan_out`,
  `ZaFanOutKind::Change`; `ZaImipPending`.
- Produces: `ZaUpdateTarget`, `za_update_target`, `ZaEventUpdate`,
  `za_commit_update` (I §2.8), with `placement: None` the only path
  written.

**What moves, and what stays behind in `dav`:**

| Old lines, `crates/dav/src/calendar/update.rs` | New |
|---|---|
| `:222-236`, the `ModifyItems` contexts, and `:237-249`, the view | `za_update_target` |
| `:701-828`, the key commit | `za_commit_update` |

`dav` keeps every step between the two (`:258-700`): `validate_headers`
and the 412 with a representation, the no-change 204, `validate_ical`,
the UID-equality check, supported components, `If-Schedule-Tag-Match`,
the previous alarms, the default-alert strip and restore, the user-data
checks, the grantee's change check, `za_sequence_floor`, the build of
the data, the next alarms, the scheduling call (Task 4), the
enterprise-licensed tenant-quota snippet (`:629-641`, with its SPDX
snippet markers), the plain batch with its ordinary fence, the
`notify_direct_change` call and `serves_submitted_ical`. It keeps the
plain commit (`:829-862`) whole, with its own
`update_commit_pause_point` and its `caused_by(trc::location!())`.

**`za_update_target`.**
`za_write_contexts(server, token, owner_id, event, addressed, Acl::ModifyItems)`,
then `event_view_in_domain(event, content, &contexts[0].domain)` with a
seal error mapped by `seal_error(err, owner_id, document_id)`. An empty
context list (an event row with no name; nothing writes one) returns
`Forbidden(ZaRefusal::Context)`.

**`za_commit_update`, in today's order.** The owner's account id is
`account.id`.
1. With `placement: Some(_)`: return `Err(ZaPipelineError::Store(..))`
   built from
   `trc::StoreEvent::UnexpectedError.into_err().details("zero-access: placement change not implemented").caused_by(trc::location!())`,
   before anything else. Task 11 replaces this arm; no caller passes
   `Some` before then.
2. The stored pairs: `event_pairs_in_domain` on the deserialized stored
   row and the stored content archive, in `contexts.from_domain()`.
3. With `scheduled.messages`: the fan-out (`roles.sender`,
   `roles.writer`, `roles.owner`, `keys`, the messages, the pairs, the
   new tree, `ZaSource { document_id, uid }` with the **stored** row's
   UID, `ZaFanOutKind::Change`, `internal_only` false,
   `RecipientPolicy::new(&server.core.groupware, new_event.flags)`, no
   lease fence, `scheduled.resolution.as_ref()`); then
   `za_set_schedule_status`, `za_set_sequences`, the returned pairs and
   the returned mail guard.
4. `new_event.display_name = display_name`; `schedule_tag` and a clone
   of the tree (for `ZaWritten`) are read here; then
   `seal_event_in_domain` in `contexts.to_domain()` with the pairs.
5. One timestamp (`now() as i64`), taken once before the loop. The
   stored `modified` is this timestamp, taken in the commit; the
   handler's own `now` (`update.rs:331`) is no longer the value stored
   (I §2.8). Then two attempts: a batch with the account id,
   `limit_disk_quota`, `contexts.assert_in`, `update_full_at` (the sealed
   parts, `token.changed_by()`, the stored row and stored content as the
   current values, no parent slot, the timestamp), `replace_event_alarms`
   with clones of both alarm sets and `AlarmTasks::None`, the hooks, the
   commit.
6. On `Ok`: `notify_task_queue`, the mail's `submit`, and `ZaWritten`
   (`change_id` from this commit's
   `last_change_id(owner, SyncCollection::Calendar)`, the `etag` of this
   attempt's `update_full_at`).
7. On an assertion loss at the first attempt:
   `za_refresh_event(server, token, owner, addressed, Acl::ModifyItems, &contexts.parents, &mut sealed)`;
   `Some(fresh)` replaces the contexts (`ZaPlacementContexts::unchanged`)
   and the loop runs again; `None` drops the mail and returns the error.
8. Any other commit error drops the mail and returns it with
   `caused_by(trc::location!())`.

**Constraints:**
- The plain commit stays in `crates/dav/src/calendar/update.rs`, with its
  `update_commit_pause_point` and its `caused_by(trc::location!())`:
  `tests/src/za/sharing_review.rs:738-755`
  (`plain_update_trace_exhausted`) requires a `CausedBy` that contains
  `crates/dav/src/calendar/update.rs:` for an ordinary account's
  exhausted update.
- `put_pause_point` (`update.rs:112-113`) fires for every PUT, plain
  ones included, and stays in the handler.
- The hooks move with the key commit, in this order, inside the loop:
  `update_commit_pause_point(owner).await` on every attempt, then, on the
  first attempt only, `take_update_commit_fail(owner)`, which drops the
  mail and returns a store error with details
  `"zero-access test hook: update commit failure"`.
- The mail guard is settled exactly where it is today: `dropped()` when
  the hook fails the commit, when the retry finds no calendar change, and
  on any other commit error; `submit(server)` only after a commit
  returned `Ok`, after `notify_task_queue`. The returns that today leave
  it unsettled stay unsettled and trace `ZA_IMIP_ABANDONED`: the seal
  error, a failed `update_full_at`, a failed `replace_event_alarms`, and
  a failed `za_refresh_event`. Add no `dropped()` there.
- Every rewrite carries the stored pairs (invitations spec 3.3) and the
  stored display name (ruling 15), with scheduling on or off.
- The stored archives are the update's current values. The commit never
  calls `update_full`, the unfenced writer.
- Untouched: `update.rs:258-700` apart from the bindings the two call
  sites rename, and the plain commit.

Steps:

- [ ] Create `pipeline/update.rs` with the two structs and the two
  functions as specified.
- [ ] Replace the first stage in the handler (`update.rs:222-256`):

  ```rust
  let za_target = match &za_keys {
      Some(_) => Some(
          za_update_target(
              self,
              access_token,
              account_id,
              document_id,
              parent_id,
              &event
                  .deserialize::<CalendarEvent>()
                  .caused_by(trc::location!())?,
              &content_,
          )
          .await?,
      ),
      None => None,
  };
  let content = match &za_target {
      Some(target) => target
          .view
          .content
          .to_unarchived::<CalendarEventContent>()
          .caused_by(trc::location!())?,
      None => stored_content.clone(),
  };
  ```

- [ ] Replace the key commit in the handler (`update.rs:701-828`):

  ```rust
  if let (Some(keys), Some(roles), Some(target)) = (&za_keys, &za_roles, za_target) {
      let written = za_commit_update(
          self,
          access_token,
          &resources,
          &account,
          keys,
          roles,
          ZaEventUpdate {
              document_id,
              addressed: parent_id,
              stored_event: &event_,
              stored_content: &content_,
              new_event,
              new_content,
              display_name: target.view.display_name.clone(),
              prev_alarms: prev_email_alarms,
              next_alarms: next_email_alarms,
              scheduled: ZaScheduled { messages: za_messages, resolution: za_resolution },
              contexts: ZaPlacementContexts::unchanged(target.contexts),
              placement: None,
          },
      )
      .await?;
      return Ok(HttpResponse::new(StatusCode::NO_CONTENT)
          .with_etag_opt(serves_submitted_ical.then_some(written.etag))
          .with_schedule_tag_opt(written.schedule_tag));
  }
  ```

  `serves_submitted_ical` is the handler's own value, computed before the
  fan-out as today; the handler does not read `written.ical`.
- [ ] Delete the dav wrapper `za_refresh_event` from
  `crates/dav/src/calendar/mod.rs` (its last caller left), and the
  imports the two replacements leave unused in `update.rs`.
- [ ] Update the doc table of `crates/common/src/za/test_hooks.rs`:
  `UPDATE_COMMIT_PAUSE` (`:23-24`) sits before the plain commit in
  `dav::calendar::update` and before the key commit in
  `groupware::calendar::pipeline::update::za_commit_update`;
  `UPDATE_COMMIT_FAIL_ONCE` (`:30-33`) names the latter.
- [ ] Run ZA, SWAP, WK and WP. The modules that prove this task: those
  of Task 5, and `sharing_review` (`UPDATE_COMMIT_PAUSE`;
  `plain_update_trace_exhausted` still finds the `dav` location),
  `imip::outbound` (`UPDATE_COMMIT_FAIL_ONCE`),
  `dav_seal::test_conditional` (If-Match on the view, the 412
  representation), `sharing_races`; in key-mode `webdav_tests`,
  `za_variants::{alarm, alarm_override}`.
- [ ] The closing checks.
- [ ] Commit: `Release 6: move the key update commit to groupware`.

### Task 8: the deletes

**Spec:** 5.1 (the commits of an event delete and a collection delete),
6.7 (D16, the CalDAV side: a collection delete sends cancellations).

**Files:**
- Create: `crates/groupware/src/calendar/pipeline/delete.rs`.
- Modify: `crates/groupware/src/calendar/pipeline/mod.rs` (declare it);
  `crates/dav/src/calendar/delete.rs:103-108` (the key-only locals),
  `:163-191`, `:228-270`, `:368-449`, `:451-602`;
  `crates/common/src/za/test_hooks.rs:25-26,34-37` (the doc table).
- Test: none new.

**Interfaces:**
- Consumes: `ZaDeleted` (I §2.1); `ZaPipelineError`, `from_calendar`
  (I §2.2); `ZaRoles` (I §2.3); `za_mutation_context` (I §2.4);
  `delete_za_at` and `delete_with_events_za`
  (`crates/groupware/src/calendar/storage.rs:743-772,488-640`);
  `za_resolve_delegation`, `za_self_cancel`, `za_combine_delegated`,
  `ZaSelfOperation::Delete`; `ZaFanOut::za_fan_out`,
  `ZaFanOutKind::Deletion`; `calendars_changed`; `ZaImipPending`.
- Produces: `za_delete_event`, `ZaCalendarDeleteTarget`,
  `za_calendar_delete_target`, `za_delete_calendar` (I §2.10).
  `za_destroy_event` is Task 12's.

**What moves, and what stays behind in `dav`:**

| Old lines, `crates/dav/src/calendar/delete.rs` | New |
|---|---|
| `:163-191`: the fresh context of a collection delete, the repaired archive, the grantees of a shared calendar | `za_calendar_delete_target` (for every account) |
| `:228-245`, the key arm `delete_with_events_za`; `:265-270`, the default-calendar clear, for a key account | `za_delete_calendar` |
| `:368-394`: the `RemoveItems` context, its assertion, the refusal without a domain, the cutoff and the retry's copy of the event; `:414-432`, the key arm `delete_za_at` | `za_delete_event` |
| `:451-600`, for a key account: the fan-out of each delete message, both hooks, the commit, the one retry, the grantees' invalidation, `notify_task_queue`, the mail guard | one private function in `pipeline/delete.rs` that both public functions call ("the shared commit") |

`dav` keeps: the handler's head (`:67-102`), with upstream's `send_itip`
expression (`:99-102`) untouched; the ordinary fence on the plain batch
(`:111-114`); the default-calendar refusal under
`cfg(not(any(feature = "dev_mode", feature = "test_mode")))` (`:117-129`),
which must not move because `groupware` has no `dev_mode` feature; the
ACL checks and the child listing; `validate_headers` for both branches
and the schedule-tag check; `preload_container_cleanup` (`:211-227`);
both plain arms (`delete_with_events` at `:247-262`, `delete` at
`:434-447`) with the plain default-calendar clear and
`notify_direct_change`; and a plain tail.

**The handler after this task.**
- Collection branch: where `:163-191` stood,

  ```rust
  let target =
      za_calendar_delete_target(self, access_token, account_id, document_id, calendar_).await?;
  let calendar = target
      .stored
      .to_unarchived::<Calendar>()
      .caused_by(trc::location!())?;
  ```

  then `validate_headers` and the cleanup preload as today, then

  ```rust
  if let (Some(keys), Some(roles)) = (&za_keys, &za_roles) {
      za_delete_calendar(
          self,
          access_token,
          &resources,
          keys,
          roles,
          account_id,
          document_id,
          target,
          children_ids,
          &cleanup,
          resources.format_resource(delete_resource).into(),
          send_itip,
      )
      .await?;
      return Ok(HttpResponse::new(StatusCode::NO_CONTENT));
  }
  ```

  then the plain arm and the plain default-calendar clear, unchanged.
- Event branch: right after the schedule-tag check (`:360-366`),

  ```rust
  if let (Some(keys), Some(roles)) = (&za_keys, &za_roles) {
      za_delete_event(
          self,
          access_token,
          &resources,
          keys,
          roles,
          account_id,
          document_id,
          calendar_id,
          event_,
          content_,
          resources.format_resource(delete_resource).into(),
          send_itip,
      )
      .await?;
      return Ok(HttpResponse::new(StatusCode::NO_CONTENT));
  }
  ```

  then `notify_direct_change` and the plain arm, unchanged. The
  handler's `notify_direct_change` call is no longer reached for a key
  account; it returns at once for one today
  (`crates/groupware/src/calendar/storage.rs:1601-1610`), so nothing
  changes.
- Tail (`:451-602`): what an ordinary account's delete runs today, and
  nothing else: `self.commit_batch(batch).await.caused_by(trc::location!())?;`
  then the 204. The locals `za_messages`, `retry_event`, `contexts`,
  `deleted_grantees` and `cutoff` go.

**The pipeline functions, in today's order.**

`za_calendar_delete_target`:
`za_mutation_context(server, token, owner_id, calendar_id, Delete | RemoveItems)`
with its error through `from_calendar(error, owner_id, calendar_id)`.
With a context: `stored` is a clone of the context's archive, and when
the context's `domain.calendar_id()` is `Some`, `deleted_grantees` is
every grant's account id of that archive except the owner's. Without
one: `stored` is the given archive and the list is empty.

`za_delete_event`:
1. `za_mutation_context(.., calendar_id, RemoveItems)`, error through
   `from_calendar(error, owner_id, calendar_id)`; no context is
   `from_calendar(ZaCalendarError::Forbidden, owner_id, calendar_id)`
   (ruling 176: a key account's delete needs its key domain).
2. A new batch; the context's assertion; `cutoff = now() as i64`; a
   clone of `event` kept for the retry.
3. `delete_za_at` with `roles.sender`, the given `content` and `href`,
   `send_itip`, the context's domain and the cutoff.
4. The shared commit, as an event delete.

`za_delete_calendar`:
1. A new batch;
   `DestroyArchive(target.stored unarchived).delete_with_events_za` with
   `token`, `roles.sender`, `children`,
   `&resources.event_ids_with_flags(EVENT_HAS_ALARMS)`, `cleanup`, `href`,
   `send_itip` and `Some(keys)`.
2. The default-calendar clear (`:265-270`).
3. The shared commit, as a collection delete, with
   `target.deleted_grantees`.

The shared commit:
1. For each delete message:
   `za_resolve_delegation(.., ZaSelfOperation::Delete, Some(&message.event), None)`;
   the first run is `message.messages.unwrap_or_default()`; with a
   resolution, `za_self_cancel(..).ok().flatten()` and
   `za_combine_delegated`, else each message as
   `ZaFanOutMessage::upstream`; then the fan-out with
   `ZaFanOutKind::Deletion`, the message's pairs, tree, document id, UID
   and policy, no lease fence; its mail is added to one guard with
   `extend`.
2. `delete_commit_pause_point(owner).await`, for an event delete only.
3. `take_delete_commit_fail(owner)`, for both: it drops the mail and
   returns a store error with details
   `"zero-access test hook: delete commit failure"`.
4. The commit. For an event delete only, an assertion loss resolves a
   fresh `RemoveItems` context; if `calendars_changed` is false the error
   returns; otherwise a new batch asserts the fresh context and rebuilds
   with `delete_za_at` on the kept event, `content` `None`, the same
   `href`, `send_itip` false, the fresh domain and the original cutoff,
   and commits once more.
5. Any error of step 4 drops the mail before it returns.
6. With grantees: one `CacheInvalidation::AccessToken(grantee)` each,
   through `invalidate_caches`.
7. `notify_task_queue` when the invalidation succeeded.
8. The mail's `submit`, whether or not the invalidation failed.
9. The invalidation's error, if any; else `ZaDeleted { change_id }` from
   the last successful commit's
   `change_id(owner, SyncCollection::Calendar).unwrap_or_default()`.
   A concurrent unlink can remove the addressed parent after DAV takes
   its resource snapshot but before it reads the event archive. If the
   event still has another name, `delete_from_calendar_za` adds no write
   or Calendar log entry; the assertion-only commit has no assigned
   change id. Preserve the existing 204 with a zero receipt in that
   case, instead of `last_change_id`'s debug assertion. Real mutations
   return their assigned change id; do not fetch a later account tip.

The invalidation builder contains only `AccessToken` entries. At the
plan's base, `invalidate_caches` can return an error only while expanding
`Role` entries (`crates/common/src/cache/invalidate.rs:471-489`); the
AccessToken-only path ends in `Ok(())` (:532-536). Keep that input scope.
Before adding a fallible postcommit operation, the pipeline result and
JMAP consumers must preserve the committed change id and successful
object answer under D3/D28. The existing precommit failure hook does not
cover such a future postcommit failure.

**Constraints:**
- `za_delete_event` passes the `content` archive it was given to
  `delete_za_at`. For a key account that is the stored, sealed archive
  (or `None`); do not unseal it and do not read it again.
- `DELETE_COMMIT_PAUSE` fires for an event delete only;
  `DELETE_COMMIT_FAIL_ONCE` for an event and a collection delete. Keep
  the asymmetry.
- The mail guard is settled exactly where it is today: `dropped()` when
  the hook fails the commit and on every error of the commit or its
  retry; `submit(server)` after the deletion committed, after
  `notify_task_queue`, and before the invalidation's error is returned.
  An error inside the fan-out loop (`za_resolve_delegation` or
  `za_fan_out`) still returns with the guard unsettled, as today; add no
  `dropped()` there.
- `za_self_cancel`'s error is still swallowed (`.ok().flatten()`).
- A grantee's keys on a collection delete are refused inside
  `delete_with_events_za` with a `Security` store error; it must arrive
  as `ZaPipelineError::Store` so the dispatcher answers 403 with its
  trace.
- The retry runs with scheduling off: its items already went out.
- The plain arms keep their statements; only the `if`/`else` wrappers
  around them go. Upstream's `send_itip` line and the default-calendar
  refusal are untouched.

Steps:

- [ ] Create `pipeline/delete.rs` with `ZaCalendarDeleteTarget`, the
  three public functions and the private shared commit, as specified.
- [ ] Edit the collection branch of the handler as above.
- [ ] Edit the event branch of the handler as above.
- [ ] Reduce the tail to the plain commit and the 204; remove the five
  key-only locals and the imports the edits leave unused
  (`za_calendar_error` among them, in this file only).
- [ ] Update the doc table of `crates/common/src/za/test_hooks.rs`:
  `DELETE_COMMIT_PAUSE` (`:25-26`) and `DELETE_COMMIT_FAIL_ONCE`
  (`:34-37`) name `groupware::calendar::pipeline::delete`.
- [ ] Run ZA, WK and WP. The modules that prove this task:
  `imip::outbound` and `imip::traces` (`DELETE_COMMIT_FAIL_ONCE`),
  `tracing` and `sharing_tombstones` (`DELETE_COMMIT_PAUSE`), `sharing`
  (a collection delete retiring grants and invalidating the grantees),
  `sharing_races`, `fanout`, `pinning`, `pairs`, `drain`, `reply`,
  `gating`, `inbox`. Plain-mode `webdav_tests` proves the plain arms.
- [ ] The closing checks.
- [ ] Commit: `Release 6: move the key deletes to groupware`.

### Task 9: the move's proof run, with the fresh-store script

**Spec:** 5.1 ("The move changes no behaviour. Its proof is that the
existing CalDAV tests pass without edits to their assertions").

**Files:** none committed. The ledger records each command, its exit
status and its result line.

**Interfaces:** consumes the head after Task 8; produces nothing.

**Constraints:**
- Nothing is edited. A failure is a finding for the task that owns the
  path; it is fixed there, in a new commit, and this task runs again
  from its first step.
- The CalDAV client suite is fetched at its pinned commit into a new,
  empty directory outside the repository.

Steps:

- [ ] `cargo fmt --all --check`, CI's style command. Expected: no
  output.
- [ ] PRODUCT. Expected: exit 0, no warning in a file of `groupware`,
  `dav`, `jmap` or `common`.
- [ ] U-vault and U-groupware with no name. Expected: no failure;
  `calendar_errors_keep_their_scopes`, `sync_boundary_refuses_as_caldav`,
  `caldav_plans_keep_todays_rule` and `caldav_plan_order` pass.
- [ ] U-ws with no name. Expected: no failure; in `dav`,
  `pipeline_errors_map_to_todays_answers`,
  `itip_precondition_classes_agree`, `read_errors_keep_their_scope`,
  `only_calendar_containers_are_fenced`,
  `second_run_fails_only_on_a_failed_precondition` and
  `tombstones_need_identity_coverage` pass.
- [ ] ZA.
- [ ] ZA-FS.
- [ ] SWAP.
- [ ] WK.
- [ ] WP.
- [ ] CALDAV in key mode, on the binary PRODUCT built. Expected: the
  harness's unit suite passes; the runner exits 0 with key mode's three
  recorded entries and nothing else (1 skipped, 2 xfailed).
- [ ] FRESH, on the same binary.
- [ ] No assertion changed in the tests crate:

  ```bash
  base="$(git log --reverse --format=%H -- crates/groupware/src/calendar/pipeline/mod.rs | head -1)^"
  git diff --stat "$base" HEAD -- tests/
  ```

  Expected: no output.
- [ ] No assertion changed in dav's unit tests:
  `git diff "$base" HEAD -- crates/dav/src/calendar/update.rs crates/dav/src/common/za.rs crates/dav/src/common/propfind/za.rs`,
  read inside each `#[cfg(test)]` module. Expected: one changed line in
  `second_run_fails_only_on_a_failed_precondition` (the `match` head,
  Task 4) and that module's imports; nothing else.
- [ ] Record the run in the ledger: the head's hash, each command, its
  exit status and its result line. No commit.
