# Plan 17, tasks 20 to 27 (group D1)

These tasks belong to [zero-access plan 17: JMAP calendars and browser sign-in](2026-11-14-zero-access-17-jmap-calendars.md), whose header holds the commands, the constraints and the task index. "I §n" is section n of the [interface sheet](2026-11-14-zero-access-17-jmap-calendars-interfaces.md).

The reads: the calendar refusal text and the admission table, `/get`, `/changes`, `ParticipantIdentity`, `parse`, the event text index, `CalendarEvent/query`, and colour on a shared calendar. Commands are the header's labels. "I §n" is a section of the interface sheet.

### Task 20: the calendar refusal text and the admission table

**Spec:** 6.1 (admission, D25), 6.2 (the table, with nothing served yet), 8.

**Files:**
- Create: `crates/common/src/za/calendar.rs`; `crates/jmap/src/api/za_calendar.rs`.
- Modify:
  - `crates/common/src/za/mod.rs:7-18` (one sentence in the module doc; `pub mod calendar;` before `pub mod imip;`) and after `ZA_MAIL_KEYLESS` (`:27-30`), the constant.
  - `crates/jmap/src/api/mod.rs:23-32` (`pub mod za_calendar;`).
  - `crates/jmap/src/api/request.rs:536,543,552,561,652,659,668,776,785,794,803,891,892,943` (fourteen calls) and `:994-1002` (the old function and its comment, removed).
  - `crates/jmap/src/changes/get.rs:8-13` (imports), `:203,209,215,228`, and the comment at `:225-227`.
  - `crates/jmap/src/changes/query.rs:24-31` (one import) and `:308-313` (two lines).
- Test: unit tests in the two new files. Unchanged, and they must pass: za `gating`, `websocket`, `mail::fence` (`tests/src/za/mail/fence.rs:3502-3533`); key-mode `jmap_tests`.

**Interfaces:**
- Consumes: `za_calendar_admission` (I §2.3), `ZaPipelineError` (I §2.2).
- Produces: `ZA_CALENDAR_KEYLESS`, `za_calendar_keyless_refusal` (I §6); `ZaCalendarMethod`, `za_admit_calendar_method`, `za_calendar_method`, `is_calendar_method` (I §7.1).

The rule of `za_admit_calendar_method`, with `owner = account_id.document_id()`:

1. `server.za_is_key_account(owner).await?` is false: `Ok(())`. This is the cached classification the old function reads; an unknown id is not a key account.
2. `Refused`: `Err(trc::JmapEvent::AccountNotSupportedByMethod.into_err())`, as today.
3. `Served`: `za_calendar_admission(server, access_token, owner)`. `Ok` is `Ok`. `Err(ZaPipelineError::Store(error))` is `Err(error)`. Any other `Err` is `Err(za_calendar_keyless_refusal().account_id(owner))`. The groupware function has traced the refusal already.
4. `OwnerOnly`: `Ok` when `access_token.za_keys_for(owner).is_some()`. Otherwise it emits the trace `za_calendar_admission` emits (`Security(Unauthorized)`, `AccountId = owner`, `Details = "zero-access: calendar access without session keys"`) and returns the keyless refusal. It does not call `za_calendar_admission`, which admits a grantee.

`za_calendar_keyless_refusal` is `trc::JmapEvent::Forbidden.into_err().details(ZA_CALENDAR_KEYLESS)`, with no `code`: no IMAP path answers it.

**Constraints:**
- Nothing changes for any account in this task: `za_calendar_method` returns `Refused` for every pair.
- For an ordinary account the admission costs one cached classification read, at every call site.
- The admission runs after the method's own access check at every site, as the old call does.
- The five `CalendarEventNotification` rows and `CalendarEvent/copy` stay `Refused` in every task (invariant 66, spec 6.11).
- `Served`, `OwnerOnly` and `is_calendar_method` have no user yet. Each carries `#[allow(dead_code)]` with a one-line reason. Task 21 removes the first, Task 24 the second, Task 34 the last.
- Do not touch a handler. `crates/jmap/src/calendar_event/set.rs:1060,1527,1886` hold comments that name the removed function; Task 30 rewords them.

- [ ] **Step 1: write the failing unit test of the refusal**, `common`, in `za/calendar.rs`: `calendar_keyless_refusal_is_forbidden_with_the_fixed_text`.
  - `ZA_CALENDAR_KEYLESS` equals "Calendars in this account are encrypted at rest and are not available to this session."
  - the error matches `EventType::Jmap(JmapEvent::Forbidden)`;
  - its `Key::Details` is `ZA_CALENDAR_KEYLESS`;
  - `za::mail::za_is_keyless_refusal(&error)` is false: it is not mail's refusal, which `/jmap/download` answers with a 403 of its own.
- [ ] **Step 2: run it.** U-common `za::calendar`. Expected: fails to compile, `cannot find function za_calendar_keyless_refusal`.
- [ ] **Step 3: implement** the constant, the module and the function.
- [ ] **Step 4: run it again.** U-common `za::calendar`. Expected: 1 passed.
- [ ] **Step 5: write the failing unit test of the table**, `jmap`, in `api/za_calendar.rs`: `calendar_method_classes`. It asserts `za_calendar_method(object, function)` for every row. In this task every row is `Refused`. The task named in the last column changes the row and the function together.

  | Object | Function | Class at the end | Task |
  |---|---|---|---|
  | `Calendar` | `Get` | `Served` | 21 |
  | `Calendar` | `Query` | `Served` | 21 |
  | `Calendar` | `Changes` | `Served` | 22 |
  | `Calendar` | `Set` | `Served` | 28 |
  | `CalendarEvent` | `Get` | `Served` | 21 |
  | `CalendarEvent` | `Changes` | `Served` | 22 |
  | `CalendarEvent` | `Parse` | `OwnerOnly` | 24 |
  | `CalendarEvent` | `Query` | `Served` | 26 |
  | `CalendarEvent` | `QueryChanges` | `Served` | 26 |
  | `CalendarEvent` | `Set` | `Served` | 30 |
  | `CalendarEvent` | `Copy` | `Refused` | none |
  | `ParticipantIdentity` | `Get` | `Served` | 23 |
  | `ParticipantIdentity` | `Changes` | `Served` | 23 |
  | `ParticipantIdentity` | `Set` | `Served` | 23 |
  | `CalendarEventNotification` | `Get` | `Refused` | none |
  | `CalendarEventNotification` | `Query` | `Refused` | none |
  | `CalendarEventNotification` | `Set` | `Refused` | none |
  | `CalendarEventNotification` | `Changes` | `Refused` | none |
  | `CalendarEventNotification` | `QueryChanges` | `Refused` | none |

  Also: `(CalendarEvent, Import)` and `(Email, Get)` are `Refused` (a pair outside the table); `is_calendar_method` is true for `Calendar`, `CalendarEvent` and `ParticipantIdentity`, and false for `CalendarEventNotification`, `Email`, `Principal` and `ShareNotification`.
- [ ] **Step 6: run it.** U-ws `calendar_method_classes`. Expected: fails to compile, `file not found for module za_calendar`.
- [ ] **Step 7: implement `za_calendar.rs`** with the rule above and declare the module.
- [ ] **Step 8: replace the call sites.** Each `za_assert_calendar_allowed(self, X).await?` becomes `za_admit_calendar_method(self, access_token, X, za_calendar_method(OBJ, FUNCTION)).await?`.
  - `request.rs`: `OBJ, FUNCTION` is `method_name.obj, method_name.fnc` at all fourteen calls. Both calls of the copy arm (`:891-892`) pass the same class. Delete the old function (`:994-1002`).
  - `changes/get.rs`: `OBJ` is the arm's `MethodObject`, `FUNCTION` is `MethodFunction::Changes`. The comment at `:225-227` says the class decides.
  - `changes/query.rs`, in the `QueryChangesRequestMethod::CalendarEvent` arm, after `resolve_account_id(..)?;` (`:308-313`) and before `self.changes(..)`. The dispatcher passes `/queryChanges` through ungated, and `changes()` knows the object but not the function, so without this call nothing reads the `QueryChanges` row.

```rust
// Release 6 spec 6.1, 6.2: `/queryChanges` has its own row. `changes()` below admits `/changes`.
// The access check is the one `changes()` runs first, repeated so that its answer still comes first.
access_token.assert_has_access(request.account_id, Collection::CalendarEvent)?;
za_admit_calendar_method(self, access_token, request.account_id,
    za_calendar_method(MethodObject::CalendarEvent, MethodFunction::QueryChanges)).await?;
```

- [ ] **Step 9: run the unit test again.** U-ws `calendar_method_classes`. Expected: 1 passed.
- [ ] **Step 10: run ZA.** Expected: pass, with no test file changed. Proved by `gating` (the eighteen refusals at `gating.rs:361-432`, the copy at `:123-138`), `websocket` (`websocket.rs:76-81`) and `mail::fence` (an ordinary account whose classification is cached still reaches upstream's calendar writers).
- [ ] **Step 11: run JK.** Expected: pass, with no test file changed (`za_variants::calendars`).
- [ ] **Step 12: the closing checks.**
- [ ] **Step 13: commit.** `Release 6: the calendar refusal text and the JMAP admission table`

---

### Task 21: `CalendarEvent/get`, `Calendar/get` and `Calendar/query`

**Spec:** 6.1 (the keyless answer, a grantee's admission, the access check), 6.2 (three rows), 6.3, 6.7 (`Calendar/get`, `Calendar/query`).

**Files:**
- Create: `crates/jmap/src/calendar_event/za_read.rs`; `tests/src/za/jmap_calendar/events.rs`; `tests/src/za/jmap_calendar/calendars.rs`.
- Modify:
  - `crates/jmap/src/calendar_event/mod.rs:36-45` (`pub mod za_read;`).
  - `crates/jmap/src/calendar_event/get.rs:184-188` (the reader, beside the projector) and `:251-270` (the content handed to the projector; the expression at `:258-261`).
  - `crates/jmap/src/calendar/get.rs:98-102` (a grantee's calendars) and `:159-175` (the stored archive becomes the view).
  - `crates/jmap/src/api/za_calendar.rs` (three rows; the allow on `Served` goes).
  - `tests/src/za/jmap_calendar/mod.rs` (declares `events` and `calendars`; two helpers); `tests/src/za/mod.rs` (two calls, after `jmap_calendar::pipeline::test(&mut test).await;`).
  - `tests/src/za/gating.rs:44` (`method_error` becomes `pub(crate)`, visibility only) and `:361-432`; `tests/src/za/websocket.rs:74-81`; `tests/src/jmap/za_variants.rs:3510-3574`.

**Interfaces:**
- Consumes: `ZaReadMemo`, `za_read_event_view`, `za_read_calendar_view`, `za_foreign_calendar_contexts` (I §2.4); `ZaPipelineError` (I §2.2); `ZaCalendarMethod::Served` (I §7.1). Tests: `pair`, `make_calendar`, `share`, `event_ics`, `put_event`, `finish`, `za_bearer_jmap`, `CALENDAR_JMAP_USING` (I §10); existing `za_grant_token`, `za_revoke_grant`, `plant_event`, `stored_event`, `write_stored_event`, `delete_event_records` (`tests/src/utils/za.rs`).
- Produces: `ZaEventReader` (I §7.2); `mark_boundary_pending`, `owner_boundary` (I §10); `gating::method_error` as `pub(crate)` (I §10).

The rule of `ZaEventReader::view`. A memoised view of `document_id` is returned at once. Otherwise, for each id of `calendar_ids` in order, it calls `za_read_event_view`:

| Result | `view` does |
|---|---|
| `Ok(Ok(Some(view)))` | memoises it and returns it |
| `Ok(Ok(None))` | returns `Ok(None)` |
| `Ok(Err(error))`, a `StoreEvent::DataCorruption` | `trc::error!(error)`, returns `Ok(None)`. The error holds the account id, the document id and a fixed reason. |
| `Ok(Err(error))`, anything else | returns `Err(error)` |
| `Err(ZaPipelineError::Store(error))` | returns `Err(error)` |
| any other `Err` (a refusal) | tries the next calendar; after the last, `Ok(None)` |

`mark_boundary_pending` follows `tests/src/za/sharing.rs:7127-7135` and `:86-105`; I §10 says what each of the two helpers does.

**Constraints:**
- Each handler classifies the account once (`Server::za_is_key_account`); for an ordinary account that read is the whole cost.
- Upstream's projector, its privacy and hidden-participant rules and its shared-id sets are not touched; they run on the view. `isDraft`, `mayInviteSelf`, `mayInviteOthers`, `hideAttendees` and `useDefaultAlerts` are then false without code.
- No owner session is manufactured: a grantee's read goes through the grantee's own contexts.
- A `CalendarEvent/get` that asks for no property needing the stored content is answered from the cache, without a read context, as `/query` and `/changes` are (spec 6.4, 6.8). No code.
- `Calendar/query` lists ids from the cache, as upstream does, and opens nothing. It needs its row only.
- `view` leaves the row read to its caller, so the branch at `get.rs:258-261` is longer than a call. Keep all of it inside that one expression and add no other line there.
- `calendar_event_get` charges each opened view to the method's memory budget.

- [ ] **Step 1: write the failing za cases.** Declare the two modules, add the two helpers, make `gating::method_error` `pub(crate)` and import it, register `jmap_calendar::events::test` then `jmap_calendar::calendars::test`.

  `events.rs`. Banner: `Running zero-access JMAP calendar event tests...`. Set-up: `pair(test, "events")`. The first case runs before any grant. Then `make_calendar(.., "jmapcal-shared")` and `share(.., "jmapcal-shared", "view")` to `key2`. End: every event deleted (CalDAV `DELETE` with `fanout::NO_REPLY`; `delete_event_records` for the tampered one), the shared calendar deleted, `finish(test, "events")`. No event of the module has an attendee or an alarm.

  - `no_access_gets_upstreams_answer`. `key2`, with no grant, calls `CalendarEvent/get` and `Calendar/get` with the owner's account id. Each is the method error `accountNotFound`, not `forbidden`.
  - `get_serves_the_caldav_tree`. An event PUT into `default`: `event_ics("jmapcal-events-get", "jmapcal title canary")` with the lines `DESCRIPTION:jmapcal description canary` and `LOCATION:jmapcal location canary` added. The owner's `CalendarEvent/get` of its id:
    - `uid` is "jmapcal-events-get"; `title` is "jmapcal title canary"; `description` is "jmapcal description canary"; `duration` is "PT1H";
    - `start` begins with "2099-03-01T10:00:00";
    - `locations`, as text, contains "jmapcal location canary";
    - `isDraft`, `mayInviteSelf`, `mayInviteOthers`, `hideAttendees` and `useDefaultAlerts` are each `false`;
    - `stored_event(..).bytes()` does not contain "canary": the answer came from the view;
    - with `ids: null` the list holds an object with that `uid`.
  - `get_keyless_is_refused`. A token of `za_grant_token(&test.server, <key's name>, OAuthScopes::ALL)`; it carries no secret. `za_bearer_jmap(token, CALENDAR_JMAP_USING, ..)` with three calls, `CalendarEvent/get` (`ids: null`, "c0"), `Calendar/get` (`ids: null`, "c1"), `Calendar/query` ("c2"):
    - HTTP 200;
    - response `n` is `["error", { "type": "forbidden", "description": ZA_CALENDAR_KEYLESS }, "c<n>"]` for each of the three;
    - `ZA_CALENDAR_KEYLESS` equals "Calendars in this account are encrypted at rest and are not available to this session.";
    - then `za_revoke_grant`.
  - `get_legacy_event`. `plant_event(test, key_id, "default", "jmapcal-legacy.ics", &event_ics("jmapcal-events-legacy", "jmapcal legacy title"), None)`. The owner's `CalendarEvent/get` of it: `title` is "jmapcal legacy title".
  - `get_unreadable_event_is_not_found`. Two events are PUT. One is tampered: read with `stored_event`, one character changed in the `X-ZA-KEY` value (`groupware::calendar::seal::tree::KEY_PROP`) of the stored tree's root component, written back with `write_stored_event`. One `CalendarEvent/get` asks for both ids:
    - no method error;
    - `notFound` is exactly the tampered id;
    - `list` holds one object, the intact one.
  - `get_completes_pending_boundary`. `put_event` of "jmapcal-events-boundary", "jmapcal shared title", into `jmapcal-shared`. In order:
    - `mark_boundary_pending`; `owner_boundary(..).pending` is true;
    - `key2`'s `CalendarEvent/get` of the event, with the owner's account id: `title` is "jmapcal shared title";
    - `owner_boundary(..).pending` is still true: a grantee repairs nothing;
    - the owner's `CalendarEvent/get` of the event;
    - `owner_boundary(..).pending` is false (spec 6.3).

  `calendars.rs`. Banner: `Running zero-access JMAP calendar tests...`. Set-up: `pair(test, "calendars")`. End: the calendar deleted, `finish(test, "calendars")`.

  - `get_shows_own_entry`. `make_calendar(.., "jmapcal-work")`; the owner sets its display name over CalDAV (PROPPATCH of `D:displayname`) to "jmapcal owner name"; `share(.., "jmapcal-work", "edit")`; `key2` sets its own display name on the shared calendar the same way to "jmapcal reader name".
    - the owner's `Calendar/get` of the calendar: `name` is "jmapcal owner name";
    - `key2`'s `Calendar/get` with the owner's account id and `ids: null`: the list has one object, not the owner's default calendar; its `id` is the shared calendar's; its `name` is "jmapcal reader name"; `myRights.mayReadItems` is true;
    - `key2`'s `Calendar/get` of the owner's default calendar: `notFound` is exactly that id;
    - the stored calendar archive's bytes, as text, contain neither name;
    - after `shares/revoke`, `key2`'s `Calendar/get` with the owner's account id is the method error `accountNotFound`.
- [ ] **Step 2: change the three existing tests** (header, "Moving a refusal row"), each with a comment citing spec 6.1 or 6.2.
  - `gating.rs:361-432`: split the loop. Served rows of this task: `Calendar/get` (`:364`), `Calendar/query` (`:365`), `CalendarEvent/get` (`:374`). Rewrite the comment at `:361`.
  - `websocket.rs:76-81`: the socket's token is the impersonating master login. The assertion becomes: `(name, args["type"], args["description"])` is `("error", "forbidden", ZA_CALENDAR_KEYLESS)`.
  - `za_variants.rs:3519-3558`: take out the rows `Calendar/get` (`:3520`) and `CalendarEvent/get` (`:3525`). Before the loop: `Calendar/get` with `ids: null` has no error and a list of one, the default calendar; `CalendarEvent/get` with `ids: null` has no error and an empty list. The first served call creates the account's default calendar, as for an ordinary account, and `Calendar/set` is still refused. So before `test.assert_is_empty()` (`:3573`): `DELETE /dav/cal/jdoe%40example.com/default` through `account.webdav_client()`, status 204. Rewrite the doc comment at `:3510-3514`.
- [ ] **Step 3: run ZA.** Expected: fails in `jmap_calendar::events`, at `get_serves_the_caldav_tree`, on `accountNotSupportedByMethod`. Every module before it passes, and so does `no_access_gets_upstreams_answer`: upstream's access check already answers before the fork's.
- [ ] **Step 4: implement `ZaEventReader`** with the rule above. `new` returns `None` when `server.za_is_key_account(account_id)` is false.
- [ ] **Step 5: insert the reader in `calendar_event/get.rs`.**
  - beside the projector (`:184-188`): `let mut za_reader = ZaEventReader::new(self, access_token, account_id).await?;`
  - at the content expression (`:258-261`): when `za_reader` is `Some` and the archive was read, read the stored row (`ValueKey::archive(account_id, Collection::CalendarEvent, document_id)`), call `view` with the parents of `resource.child_names()`, charge the view (`budget.add_archive(view.content.as_bytes().len(), 1)?`), and hand `view.content.unarchive::<CalendarEventContent>()` to the projector. A missing row, or `Ok(None)` from `view`, is `results.push_not_found(group); continue;`. Otherwise the expression is upstream's.
- [ ] **Step 6: insert the views in `calendar/get.rs`.**
  - after `calendar_ids` (`:98-102`): a `ZaReadMemo` and `let za_key = self.za_is_key_account(account_id).await?;`. When `za_key && !is_owner`, `calendar_ids` becomes the keys of `za_foreign_calendar_contexts(self, access_token, account_id, calendar_ids.iter(), None)`, and each returned context goes into the memo under `(account_id, calendar_id)`. `Err(ZaPipelineError::Store(error))` is `Err(error)`; any other `Err` leaves the set empty.
  - at `:159-175`, when `za_key`: the archive upstream just read is replaced by the view of `za_read_calendar_view(.., &mut memo)`. A `DataCorruption` item error is traced and the id is not found. Any other item error, and an outer `Store(error)`, fail the method. Any other outer error is `response.push_not_found(id); continue;`.
- [ ] **Step 7: flip the rows** `(Calendar, Get)`, `(Calendar, Query)` and `(CalendarEvent, Get)` to `Served`, in the function and in `calendar_method_classes`, and remove the allow on `Served`. Run U-ws `calendar_method_classes`. Expected: 1 passed.
- [ ] **Step 8: run ZA.** Expected: pass. Proved by `jmap_calendar::events`, `jmap_calendar::calendars`, `gating` and `websocket`.
- [ ] **Step 9: run JK.** Expected: pass, with the store empty at the end of `za_variants::calendars`.
- [ ] **Step 10: the closing checks.**
- [ ] **Step 11: commit.** `Release 6: CalendarEvent/get, Calendar/get and Calendar/query for key accounts`

---

### Task 22: `/changes` and the sync boundary

**Spec:** 6.8, D15, D29; 6.2 (two rows); invariant 76.

**Files:**
- Create: `crates/jmap/src/changes/za_calendar.rs`; `tests/src/za/jmap_calendar/sharing.rs`.
- Modify:
  - `crates/jmap/src/changes/mod.rs:7-11` (`pub mod za_calendar;`).
  - `crates/jmap/src/changes/get.rs:245-254`: one arm in the fork's `match object` block, which runs after admission and before the log is read.
  - `crates/jmap/src/api/za_calendar.rs` (two rows).
  - `tests/src/za/jmap_calendar/events.rs` (one case); `tests/src/za/jmap_calendar/mod.rs` (`pub mod sharing;`); `tests/src/za/mod.rs` (the call `jmap_calendar::sharing::test`, after `jmap_calendar::calendars::test`; Tasks 26 and 31 put `query` and `scheduling` between them).
  - `tests/src/za/gating.rs` (two rows).

**Interfaces:**
- Consumes: `za_foreign_calendar_contexts` (I §2.4); `ZaSyncBoundary::refuses`, `ZaCalendarContext.boundary` (I §2.4). Tests: `pair`, `key_account`, `make_calendar`, `share`, `put_event`, `finish`, `mark_boundary_pending`, `owner_boundary` (I §10).
- Produces: `za_calendar_sync_boundary` (I §7.2), and the private `since_change_id` that Step 1 decides.

**The rule (spec 6.8, D15 and D29).** The predicate is applied to the state the sync started from, with the same comparison on every page. A page of a sync that started from an initial state is never refused. The owner is never checked.

**What the code holds.**
- An intermediate state is `JMAPIntermediateState { from_id, to_id, items_sent }` (`crates/jmap-proto/src/types/state.rs:19-23`). `LogBounds::new_state` builds it (`crates/jmap/src/changes/page.rs:278-279`) from the first and last change id of the range the first page read (`:204-228`).
- It does not record whether the sync began at `Initial` or at `Exact(s)`.
- An initial sync reads the log from its first record (`Query::All`, `crates/jmap/src/changes/get.rs:324-333`), so its `from_id` is the first retained change id of the log. An exact start gives the first change after `s`.
- A boundary's change ids are taken from the calendar log: `crates/groupware/src/calendar/sharing/boundary.rs:158-170` and `sharing/mutations.rs:176-183,443-447`.

**The candidate for a later page.** The sync started from an initial state exactly when `from_id` is the first retained change id of the calendar log; then the page is served. Otherwise the start is some `s < from_id`, and the page is checked as `refuses(from_id - 1)`. It costs one log read per later page, and only for a grantee's sync of a key account.

`za_calendar_sync_boundary`, in this order:

1. `State::Initial`: `Ok`.
2. `access_token.account_id() == account_id`: `Ok`. This is CalDAV's own test for "not a sharer's calendars" (`crates/dav/src/common/propfind/get.rs:84-89`).
3. `!server.za_is_key_account(account_id).await?`: `Ok`.
4. The id to check: `Exact(id)` is `id`. `Intermediate` follows Step 1's decision; under the candidate it reads the log's first retained change id, returns `Ok` when `from_id` equals it, and otherwise checks `from_id - 1`.
5. The calendars are `document_ids(true)` of `fetch_groupware_resources(access_token.account_id(), account_id, SyncCollection::Calendar)`, passed to `za_foreign_calendar_contexts(.., None)`. `Err(ZaPipelineError::Store(error))` is `Err(error)`; any other `Err` is `Err(trc::JmapEvent::CannotCalculateChanges.into_err())`.
6. If any context has `boundary.is_some_and(|b| b.refuses(id))`: `Err(trc::JmapEvent::CannotCalculateChanges.into_err())`, with no details.

**Constraints:**
- The change log is keyless and is served as upstream serves it, with upstream's filtering for a grantee. Do not touch the log reader or another object's arm.
- One predicate decides both protocols: call `ZaSyncBoundary::refuses` and write no comparison of a boundary's change ids in `jmap`.
- For an ordinary account the arm costs at most one cached classification read, and none when the caller is the owner or the state is initial.
- `/queryChanges` inherits the check through `self.changes(..)` (`crates/jmap/src/changes/query.rs:314-322`); add nothing there.
- The insertion in `changes/get.rs` is one arm with one call.

- [ ] **Step 1: confirm the reading, before any test.** Read the four places under "What the code holds" and decide how a later page tells the two starts apart. Check these and write each answer in the task report:
  - that `from_id` of an initial sync is the log's first retained change id, also when the log's head was trimmed (an empty record marks truncation, `crates/store/src/query/log.rs:160-168`);
  - how a later page reads the first retained change id with one read;
  - that a boundary above `s` is at or above `from_id`, because a boundary's ids are ids of the calendar log;
  - what an exact start below the first retained record gives: the same `from_id` as an initial sync. Say whether upstream refuses that start itself.

  If the code does not let the two starts be told apart, stop and report to the controller. Do not choose a side.
- [ ] **Step 2: write the failing unit test**, `jmap`, in `changes/za_calendar.rs`: `sync_boundary_since_ids`. Under the candidate, `since_change_id(since: &State, first_retained: Option<u64>) -> Option<u64>` returns the id to check, or `None` for a state that is never checked. If Step 1 decided another form, the rows follow it.

  | `since` | First retained | Result |
  |---|---|---|
  | `Initial` | any | `None` |
  | `Exact(41)` | any | `Some(41)` |
  | `Intermediate { from_id: 42, to_id: 50, items_sent: 3 }` | `Some(42)` | `None`: the sync started from an initial state |
  | the same | `Some(7)` | `Some(41)` |
  | the same | `None` | `Some(41)` |
  | `Intermediate { from_id: 1, to_id: 9, items_sent: 1 }` | `Some(1)` | `None` |
- [ ] **Step 3: run it.** U-ws `sync_boundary_since_ids`. Expected: fails to compile, `file not found for module za_calendar`.
- [ ] **Step 4: write the failing za cases.**

  `events::changes_as_upstream`, called from `events::test`. Over CalDAV: PUT `a` and `b`; change `a`'s SUMMARY; DELETE `b`. A state is read from `CalendarEvent/get`'s `state` before the first step (`s0`) and after each (`s1`, `s2`, `s3`).
  - `CalendarEvent/changes` since `s0`, after the two PUTs: `created` is `{a, b}`, `updated` and `destroyed` are empty, `newState` is `s1`;
  - since `s1`, after the change: `updated` is `[a]`, the other two empty;
  - since `s2`, after the delete: `destroyed` is `[b]`;
  - `Calendar/changes` since the calendar state read before a `make_calendar`: `created` is the new calendar's id;
  - `CalendarEvent/changes` since `"n"` has no method error.

  `sharing.rs`. Banner: `Running zero-access JMAP calendar sharing tests...`. Set-up: `pair(test, "sharing")` (the owner, and `key2`, which is never granted) and `key_account(test, "sharing-reader", user_permissions())`, the grantee. This is the first call of `key_account`: remove its `#[allow(dead_code)]` in `jmap_calendar/mod.rs` (I §10.1). `make_calendar(.., "jmapcal-shared")`, two events with `put_event`, `share(.., "view")` to the grantee. End: the events and the calendar deleted, `finish(test, "sharing")`. Task 36 adds cases.

  `sharing::sync_boundary_matches_caldav`. Three helpers of the case:
  - `dav(id)`: the grantee's REPORT `sync-collection` on `/dav/cal/jmapcal-sharing%40example.com/jmapcal-shared/` with the token `urn:stalwart:davsync:{id:x}`. Refused means status 403 and a body that holds "valid-sync-token" (`tests/src/za/sharing_tombstones.rs:31-53`).
  - `jmap(method, since)`: the grantee's `method` with the owner's account id and that `sinceState`. Refused means the method error `cannotCalculateChanges`.
  - `exact(id)`: `State::new_exact(id).to_string()`.

  The assertions, in order:
  1. `mark_boundary_pending`. For `id` in `1` and `u64::MAX - 1`: `dav(id)`, `jmap("CalendarEvent/changes", exact(id))` and `jmap("Calendar/changes", exact(id))` are all refused. A pending boundary refuses every incremental state on both protocols.
  2. (a) Still pending: the grantee's `CalendarEvent/changes` from `"n"` with `maxChanges: 1`. The first answer has no error, `hasMoreChanges: true` and a `newState` that begins with `r`. Each next request sends the last `newState`. Every page has no error, until `hasMoreChanges` is false.
  3. The owner's `Calendar/get` with `ids: null`. Then `owner_boundary(..).pending` is false. `b` is its `rename_change_id`: the later of grant and rename, here the completion's id.
  4. (b) The paged sync of (a) again, after completion: every page is served to its end.
  5. (c) For `id` in `1`, `b - 1`, `b`: `dav(id)` and both `jmap` calls with `exact(id)` are refused, on the first request.
  6. Three further events are PUT into the shared calendar, one at a time. `t0`, `t1` and `t2` are the owner's `CalendarEvent/get` `state` after each; `State::parse` gives each as `Exact(id)` with `id > b`. For each of the three: `dav(id)` and both `jmap` calls are served. Since `t1` the grantee's `created` has one id.
  7. (d) The grantee's `CalendarEvent/changes` from `t0` with `maxChanges: 1` (two changes follow it): every page is served to its end.
  8. (e) Two requests with `maxChanges: 1` each return a first page and an intermediate state: one from `t0` (state `p`), one from `"n"` (state `q`). Then `mark_boundary_pending`. The second page with `p` is refused. The second page with `q` is served. Then the owner's `Calendar/get` completes the boundary again.
  9. The owner's `CalendarEvent/changes` since `exact(b)` is not `cannotCalculateChanges`: the owner is never checked.
  10. The case deletes its three events. The calendar keeps the two of the set-up and a completed boundary, which Task 36's cases start from.
- [ ] **Step 5: move the gating rows** `Calendar/changes` (`gating.rs:370-373`) and `CalendarEvent/changes` (`:388-391`) to the served loop.
- [ ] **Step 6: run ZA.** Expected: fails in `jmap_calendar::events`, at `changes_as_upstream`, on `accountNotSupportedByMethod`.
- [ ] **Step 7: implement** `za_calendar.rs` and the arm at `changes/get.rs:245-254`:

```rust
MethodObject::Calendar | MethodObject::CalendarEvent => {
    za_calendar_sync_boundary(self, access_token, request.account_id.document_id(), &request.since_state).await?;
}
```

- [ ] **Step 8: flip the rows** `(Calendar, Changes)` and `(CalendarEvent, Changes)` to `Served`, in the function and in `calendar_method_classes`. Run U-ws `sync_boundary_since_ids`, then `calendar_method_classes`. Expected: 1 passed each.
- [ ] **Step 9: run ZA.** Expected: pass. Proved by `events::changes_as_upstream` and `sharing::sync_boundary_matches_caldav`. `gating` still shows `CalendarEvent/queryChanges` refused: Task 20's call in `changes/query.rs` keeps it so until Task 26.
- [ ] **Step 10: run JK.** Expected: pass, unchanged.
- [ ] **Step 11: the closing checks.**
- [ ] **Step 12: commit.** `Release 6: calendar /changes and the sync boundary for a grantee`

---

### Task 23: `ParticipantIdentity`

**Spec:** 6.9; 6.2 (three rows); 6.11 (the row on identities).

**Files:**
- Create: `tests/src/za/jmap_calendar/refusals.rs`.
- Modify:
  - `crates/jmap/src/participant_identity/set.rs`: one branch after the state check (`:77`, before `let account_info` at `:79`), and a private function and constant at the end of the file, beside `identities_not_shared` (`:421-424`).
  - `crates/jmap/src/api/za_calendar.rs` (three rows).
  - `tests/src/za/jmap_calendar/events.rs` (one case); `tests/src/za/jmap_calendar/mod.rs` (`pub mod refusals;`); `tests/src/za/mod.rs` (the call `jmap_calendar::refusals::test`, after `jmap_calendar::sharing::test`).
  - `tests/src/za/gating.rs` (two rows); `tests/src/jmap/za_variants.rs` (two rows and a cleanup).
- Must not touch: `participant_identity/get.rs`, `changes.rs`.

**Interfaces:**
- Consumes: `Server::za_is_key_account` (`crates/common/src/auth/vault.rs:463`); upstream's `SetRequest::unwrap_create` and `unwrap_update`, each of which takes its map and leaves it empty, and the `will_destroy` the handler collects at `set.rs:53`. Tests: `key_account`, `finish` (I §10).
- Produces: `ZA_IDENTITIES_FIXED`, `za_refuse_identity_edits` (I §7.4; the text is I §7.5's).

The fixed text: "Participant identities in this account cannot be created, changed or destroyed; only the default identity can be switched."

The branch: for a key account `za_refuse_identity_edits` moves every create, update and destroy of the request to its `not_*` map with the set error `forbidden` and the fixed text, and `will_destroy` becomes empty. Upstream's loops then run over nothing and its tail applies `onSuccessSetIsDefault`. Upstream applies the default only when all three `not_*` maps are empty (`set.rs:267-272`), so a request that mixes a refused edit with a new default leaves the default unchanged. That is upstream's rule and stays.

**Constraints:**
- `/get` and `/changes` are served by upstream's code as it stands. The first read writes the identities record: the account's own addresses and its registry name, which the registry already holds in the clear.
- Nothing of a refused request is stored.
- For an ordinary account the handler costs one cached classification read.
- Another account's request already gets upstream's `reject_identity_changes` (`set.rs:54-56`), before the fork's branch.

- [ ] **Step 1: write the failing za cases.**

  `refusals.rs`. Banner: `Running zero-access JMAP calendar refusal tests...`. Set-up, without `pair`, because the first account needs an alias: `jmapcal-refusals@example.com` through `create_key_user_account` with the description `jmapcal-refusals`, the alias `jmapcal-refusals-alias@example.com` and `user_permissions()`, registered with `test.insert_account`; and `key_account(test, "refusals-2", user_permissions())`. End: `finish(test, "refusals")`. Tasks 24 and 37 add cases.

  `refusals::identity_set_only_switches_default`. The account has two addresses, so two identities: `a` (default) and `b`.
  - One `ParticipantIdentity/set` with a create `c` (`name` "jmapcal identity canary", `calendarAddress` "mailto:jmapcal-refusals-alias@example.com"), an update of `a` (`name` "jmapcal identity canary") and a destroy of `b`: `notCreated.c`, `notUpdated.a` and `notDestroyed.b` each have `type` "forbidden" and `description` equal to the fixed text.
  - `ParticipantIdentity/get` then lists exactly `{ id: "a", name: "jmapcal-refusals", calendarAddress: "mailto:jmapcal-refusals@example.com", isDefault: true }` and `{ id: "b", name: "jmapcal-refusals", calendarAddress: "mailto:jmapcal-refusals-alias@example.com", isDefault: false }`.
  - The stored identities record (`PrincipalField::ParticipantIdentities` of document 0, read as an archive; its bytes after decompression, as text) does not contain "canary".
  - `ParticipantIdentity/set` with only `onSuccessSetIsDefault: "b"`: no method error; then `a` has `isDefault: false` and `b` `true`.
  - `ParticipantIdentity/set` with a destroy of `b` and `onSuccessSetIsDefault: "a"`: `b` is still the default.

  `events::identity_get_and_changes`, called from `events::test`.
  - The owner's `ParticipantIdentity/get` with `ids: null` lists exactly `{ id: "a", name: "jmapcal-events", calendarAddress: "mailto:jmapcal-events@example.com", isDefault: true }`.
  - `ParticipantIdentity/changes` since that answer's `state`: no method error; `created`, `updated` and `destroyed` are each empty.
  - `key2`, a grantee of the owner's shared calendar, calls `ParticipantIdentity/get` with the owner's account id: the first object's `calendarAddress` is "mailto:jmapcal-events-2@example.com". A caller gets its own identities, as upstream answers.
- [ ] **Step 2: change the two existing tests**, each with a comment citing spec 6.2 or 6.9.
  - `gating.rs`: `ParticipantIdentity/get` (`:396-399`) and `ParticipantIdentity/set` (`:400-403`, its `create: {}`) go to the served loop.
  - `za_variants.rs`: take out `ParticipantIdentity/get` (`:3541-3544`) and `ParticipantIdentity/changes` (`:3545-3550`). In the served block each asserts that `error_type_at(0)` is neither `accountNotSupportedByMethod` nor `forbidden`. The get writes the account's identities record, so before the CalDAV delete of Task 21 clear it as upstream's own test does (`tests/src/jmap/calendar/identity.rs:142-149`): one batch that clears `PrincipalField::ParticipantIdentities` of document 0 in `Collection::Principal`.
- [ ] **Step 3: run ZA.** Expected: fails in `jmap_calendar::events`, at `identity_get_and_changes`, on `accountNotSupportedByMethod`.
- [ ] **Step 4: implement** the branch, the function and the constant.
- [ ] **Step 5: flip the rows** `(ParticipantIdentity, Get)`, `(.., Changes)` and `(.., Set)` to `Served`, in the function and in `calendar_method_classes`. Run U-ws `calendar_method_classes`. Expected: 1 passed.
- [ ] **Step 6: run ZA.** Expected: pass. Proved by `refusals::identity_set_only_switches_default` and `events::identity_get_and_changes`.
- [ ] **Step 7: run JK.** Expected: pass, with the store empty at the end of `za_variants::calendars`.
- [ ] **Step 8: the closing checks.**
- [ ] **Step 9: commit.** `Release 6: ParticipantIdentity for key accounts, default switch only`

---

### Task 24: `CalendarEvent/parse`

**Spec:** 6.6; 6.1 (`parse` admits the owner only); 6.2 (one row).

**Files:**
- Modify: `crates/jmap/src/api/za_calendar.rs` (one row; the allow on `OwnerOnly` goes); `tests/src/za/jmap_calendar/events.rs` (two cases); `tests/src/za/jmap_calendar/refusals.rs` (one case); `tests/src/za/gating.rs` (one row).
- Must not touch: `crates/jmap/src/calendar_event/parse.rs`, `crates/jmap/src/blob/download.rs`.

**Interfaces:**
- Consumes: `ZaCalendarMethod::OwnerOnly` and rule 4 of `za_admit_calendar_method` (I §7.1); upstream's `blob_download`, which the handler calls at `parse.rs:55` and which already reads a key account's upload and message part through the unsealing reader. Tests: `make_calendar`, `share`, `za_bearer_jmap`, `CALENDAR_JMAP_USING` (I §10); `Account::jmap_client().await.upload(None, bytes, None)` (a sealed upload, `tests/src/za/mail/uploads.rs:244-252`); `imip::deliver` (`tests/src/za/imip/mod.rs:130`); `wait_for_queue`; `za_grant_token`, `za_revoke_grant`.
- Produces: nothing new.

**Constraints:**
- Only admission changes. A sealed source is never read raw: the handler's one reader is `blob_download`.
- The method writes nothing.
- A grantee cannot parse a blob in the sharer's account, and a keyless token cannot parse at all. Both get the method error `forbidden` with `ZA_CALENDAR_KEYLESS`.

- [ ] **Step 1: write the failing za cases.** The ICS of all three is one VEVENT with no `METHOD` property and the SUMMARY "jmapcal parse canary".

  `events::parse_sealed_upload`. The owner uploads the ICS with `jmap_client().upload`. `CalendarEvent/parse` of the blob id: no method error; `parsed[<blob id>][0].title` is "jmapcal parse canary".

  `events::parse_message_part`. A message delivered over LMTP (`imip::deliver`, then `wait_for_queue`): `multipart/mixed` with a `text/plain` part and the ICS as `Content-Type: text/calendar; name="event.ics"`, `Content-Disposition: attachment`. The part's blob id is `attachments[0].blobId` of `Email/get`.
  - `CalendarEvent/parse` of it: `parsed[<blob id>][0].title` is "jmapcal parse canary";
  - `inbox_ids(test, key_id)` is empty: without a `METHOD` it is no invitation;
  - the case destroys the message with `Email/set`.

  `refusals::parse_by_grantee_and_keyless`. The owner makes `jmapcal-shared` and shares it with the second account at `view`, so upstream's access check passes for the grantee. `blob` is the owner's sealed upload of the ICS.
  - the grantee's `CalendarEvent/parse` of `blob` with the owner's account id is `["error", { "type": "forbidden", "description": ZA_CALENDAR_KEYLESS }, "0"]`;
  - the owner's keyless token (`za_grant_token`, `OAuthScopes::ALL`), through `za_bearer_jmap` with the same call: HTTP 200 and the same error; then `za_revoke_grant`;
  - control: the keyed owner's `CalendarEvent/parse` of `blob` has no method error;
  - control: the grantee's `CalendarEvent/parse` of an upload of its own, in its own account, has no method error;
  - the case deletes the shared calendar.
- [ ] **Step 2: move the gating row** `CalendarEvent/parse` (`gating.rs:384-387`) to the served loop, with its comment.
- [ ] **Step 3: run ZA.** Expected: fails in `jmap_calendar::events`, at `parse_sealed_upload`, on `accountNotSupportedByMethod`.
- [ ] **Step 4: flip the row** `(CalendarEvent, Parse)` to `OwnerOnly`, in the function and in `calendar_method_classes`, and remove the allow on `OwnerOnly`. Run U-ws `calendar_method_classes`. Expected: 1 passed.
- [ ] **Step 5: run ZA.** Expected: pass. Proved by the three cases.
- [ ] **Step 6: run JK.** Expected: pass, unchanged.
- [ ] **Step 7: the closing checks.**
- [ ] **Step 8: commit.** `Release 6: CalendarEvent/parse for the keyed owner`

---

### Task 25: the event text index

**Spec:** 6.4 (D12, the unexpanded form: the mechanism); 9.1; invariant 60.

**Files:**
- Create: `crates/groupware/src/calendar/za_text.rs`.
- Modify: `crates/groupware/src/calendar/mod.rs:6-34` (`pub mod za_text;`, after `pub mod user;`).
- Must not touch: `crates/store/src/search/`, `crates/store/src/za_search/`.

**Interfaces:**
- Consumes, each existing and public: `ArchivedCalendarEventContent::index_document` (`crates/groupware/src/calendar/index.rs:742-834`), the function the index builder itself calls; `SortKeys`, `ContentRecord::new`, `ContentPart::{insert, resolve}`, `ResolveError` (`crates/store/src/za_search/content.rs:30-82,246-289,442-459`); the settings `server.core.email.index_fields` and `server.core.email.default_language`.
- Produces: `ZaEventTextIndex` with `new`, `insert`, `resolve` and the private `with_fields` (I §3.2).

`with_fields(index_fields: Option<AHashSet<SearchField>>, default_language: Language) -> Self` is what `new` calls and what the unit tests build from. `None` means the setting has no Calendar entry: `insert` then adds nothing and every text condition matches nothing.

`resolve` maps `ResolveError::TooLarge(error)` to `Err(error)`: the store's own `LimitEvent::SizeRequest`, "Too many search terms in one query". The content part answers `Unresolvable` for an `Integer` leaf, so the caller passes none (Task 26 calls before the account-id leaf is added), and `Corrupt` cannot arise from an index built in the same call. Both map to `trc::StoreEvent::UnexpectedError.into_err().details("zero-access: calendar text condition not resolved").caused_by(trc::location!())`.

**Constraints:**
- Nothing is copied by hand. The values, their order, their joining, the language choice, the terms and the planning of a text condition are the index's own code: `index_document`, `ContentRecord::new` and `ContentPart::resolve`. Write no tokenizer call, no stemmer call and no matching rule in this file.
- Text of a key account's event is matched only in process memory, over the unsealed view, in a request that holds the keys (invariant 60). The struct holds plaintext terms: no `Clone`, no `Serialize`, and nothing here stores or caches it.
- Nothing is indexed: no write to the search store, no index-queue entry.
- The event is matched as the one document the index would hold for it: `insert` takes the whole view once, never one component at a time.

- [ ] **Step 1: write the failing unit tests**, in `za_text.rs`. The archived content of an event is built as the neighbouring tests build it (`crates/groupware/src/calendar/seal/event.rs:673-721`). All text is lower case. `index_of(events, fields, language)` is `with_fields`, then `insert(9, id, view)` for each event. The three events:
  - **1**, one VEVENT: SUMMARY "quarterly budget review"; DESCRIPTION "the committee is reviewing the quarterly budgets for the marketing department this afternoon"; LOCATION "room alpha"; `ORGANIZER;CN=alice organizer:mailto:alice@example.com`; `ATTENDEE;CN=bob guest:mailto:bob@example.com`; `ATTENDEE;CN=carol guest:mailto:carol@example.com`.
  - **2**, a daily series: the master has SUMMARY "team sync" and DESCRIPTION "the weekly meeting of the whole engineering team to talk about progress"; one override (RECURRENCE-ID) has SUMMARY "offsite planning".
  - **3**, one VEVENT: SUMMARY "dentist appointment"; DESCRIPTION "a routine visit to the dentist for the yearly cleaning and a check of the teeth"; LOCATION "clinic beta".

  `text_index_cases`. The index is `index_of(all three, Some(empty set), English)`. For each row, `resolve(&[SearchFilter::has_text(field, text, language)])` is one `DocumentSet` of exactly the listed ids.

  | Field | Text | Language | Ids | What it shows |
  |---|---|---|---|---|
  | Title | `budget` | English | 1 | a single word |
  | Title | `budget review` | English | 1 | several words: each must match |
  | Title | `budget dentist` | English | none | |
  | Title | `team offsite` | English | 2 | one word in the master, one in the override |
  | Title | `"budget review"` | English | 1 | a phrase |
  | Title | `"review budget"` | English | none | in order only |
  | Title | `"sync offsite"` | English | 2 | the field is one string: master, then override |
  | Title | `quart*` | English | 1 | a prefix |
  | Title | `udget*` | English | none | |
  | Title | `appointments` | English | 3 | a stem |
  | Description | `review` | English | 1 | "reviewing" by its stem |
  | Description | `marketing department` | English | 1 | |
  | Description | `engineering` | English | 2 | |
  | Location | `alpha` | none | 1 | |
  | Location | `room alpha` | none | 1 | |
  | Location | `clinic beta` | none | 3 | |
  | Owner | `alice@example.com` | none | 1 | |
  | Owner | `organizer` | none | 1 | the CN parameter is indexed |
  | Attendee | `bob@example.com` | none | 1 | |
  | Attendee | `carol@example.com` | none | 1 | a second value of one field |
  | Attendee | `bob@example.com carol@example.com` | none | 1 | |
  | Attendee | `alice@example.com` | none | none | the organizer is not an attendee |

  Also: `resolve` of `[Or, Title "budget", Title "dentist", End, DocumentSet {7}]` is `[Or, DocumentSet {1}, DocumentSet {3}, End, DocumentSet {7}]`. Operators, ends and sets pass through, and each text leaf is replaced in place.

  No row asserts a phrase on a field indexed without positions (location, owner, attendee). That case is decided by Task 26's `two_accounts_same_answers` against a stored index, not by either reading of the store's code. A row that disagrees with the stored index is corrected in the test, never in the index's code, and named in the task report.

  `no_index_fields_matches_nothing`.
  - `index_of([1], None, English)`: Title "budget" resolves to an empty `DocumentSet`.
  - `index_of([1], Some({Calendar(Title)}), English)`: Title "budget" resolves to `{1}`; Description "review" resolves to the empty set.

  `oversized_query_is_the_stores_error`. With `index_of([1], Some(empty set), English)`:
  - `store::search::MAX_QUERY_LEAVES` (1,024) text leaves resolve `Ok`;
  - one more is an `Err` that matches `EventType::Limit(LimitEvent::SizeRequest)`.
- [ ] **Step 2: run them.** U-groupware `za_text`. Expected: fails to compile, `file not found for module za_text`.
- [ ] **Step 3: implement** `za_text.rs` and declare the module.
- [ ] **Step 4: run them again.** U-groupware `za_text`. Expected: 3 passed.
- [ ] **Step 5: the closing checks.**
- [ ] **Step 6: commit.** `Release 6: the in-memory text index of a key account's events`

---

### Task 26: `CalendarEvent/query` and `/queryChanges`

**Spec:** 6.4; 6.8 (`/queryChanges`); 6.2 (two rows); 9.2 (`jmap_calendar::query`); review focus 5.

**Files:**
- Create: `crates/jmap/src/calendar_event/query/za.rs`; `tests/src/za/jmap_calendar/query.rs`.
- Modify:
  - `crates/jmap/src/calendar_event/query.rs`: `mod za;`; point 1 between the end of the filter loop (`:373`) and the search-store call (`:417-425`), directly before `let results = self.search_store()`; point 2 at the stored content read of the expanded loop (`:483-497`).
  - `crates/jmap/src/api/za_calendar.rs` (two rows).
  - `tests/src/za/jmap_calendar/mod.rs` (`pub mod query;`); `tests/src/za/mod.rs` (the call `jmap_calendar::query::test`, after `jmap_calendar::calendars::test` and before `jmap_calendar::sharing::test`).
  - `tests/src/za/gating.rs` (two rows); `tests/src/jmap/za_variants.rs` (one row).
- Must not touch: upstream's filter building (`query.rs:241-373`), its sorting, its expansion; `changes/query.rs`.

**Interfaces:**
- Consumes: `ZaEventReader` (I §7.2); `ZaEventTextIndex` (I §3.2). Tests: `pair`, `make_calendar`, `share`, `put_event`, `finish`, `mark_boundary_pending`, `owner_boundary` (I §10); `gating::method_error`.
- Produces: `za_resolve_text_leaves` and the private `za_text_candidates` (I §7.2).

What the task relies on, as the code stands: the filter list's top level is an AND (`crates/store/src/search/local.rs:85-88`); with set-only leaves the search store combines them and reads nothing (`crates/store/src/dispatch/search.rs:42-135`); a `metadata` condition of a key account already answers `unsupportedFilter` with `jmap::api::metadata::ZA_METADATA_REFUSED` (`crates/jmap/src/api/metadata/za.rs:69-73`).

`za_text_candidates(mask, filters)` is `mask` intersected with every `DocumentSet` leaf whose enclosing groups are all `And`. The result of the whole filter is a subset of every such set, and whether an event matches depends on that event alone, so a text leaf resolved over the candidates gives the same result as one resolved over the mask. This is what makes a text query with a time range open only the events in range.

`za_resolve_text_leaves`:

1. No `SearchFilter::Text` in `filters`: return. Nothing is opened.
2. The candidates are `za_text_candidates(mask, filters)`.
3. For each candidate: its resource from `cache.item_by_id`, its stored row (`ValueKey::archive(account_id, Collection::CalendarEvent, id)`) and its stored content (`CalendarEventField::Content`), then `reader.view` with the parents of `child_names()`. `Some(view)`: `index.insert(account_id, id, view.content.unarchive::<CalendarEventContent>()?)`. A missing resource, row or content, or `None`: the event matches no text condition.
4. `*filters = index.resolve(filters)?;`

The two points in `query.rs`:

- **Point 1:** `let mut za_reader = ZaEventReader::new(self, access_token, account_id).await?;` and, when it is `Some`, the call of `za_resolve_text_leaves` with `&cache`, `&mask` and `&mut filters`. Upstream's search-store call follows unchanged. It adds the account-id leaf itself (`with_account_id`), after this point: the text index refuses an `Integer` leaf, so the call must stay before it.
- **Point 2:** when `za_reader` is `Some` and `instance_filter` is not empty, the stored content the loop reads is replaced by the view's: read the stored row, call `reader.view` (a hit of the memo point 1 filled), use `view.content.unarchive::<CalendarEventContent>()`; `None` is `continue`. Otherwise the loop is upstream's: without a text condition it reads structure only, which is visible.

**Constraints:**
- Text of a key account's event is matched only in process memory, over the unsealed view, in a request that holds the keys (invariant 60). No text leaf of a key account reaches the search store: after point 1 the list holds sets only. Nothing is indexed.
- `inCalendar`, `uid`, `after`, `before`, every sort and the expansion read the cache and the stored tree's visible fields, as upstream does. Do not route them through the view.
- The expanded form keeps upstream's matcher (`InstanceTextFilter`) on the view, and upstream's refusal of `NOT`, `OR` and a nested `AND` when expanding. Its phrase and prefix rules differ from the index's for every account; that is not this task's to change.
- Privacy for a grantee is upstream's: the wrappers (`And(leaf, public ids)`) stay around the replaced leaves.
- The branch relies on no key account's event carrying `EVENT_HIDE_ATTENDEES` (6.5 refuses the flag; CalDAV has none). Upstream's check for such events (`query.rs:895-1034`) sends an attendee leaf to the search store.
- `CalendarEvent/queryChanges` calls the query function directly, so the branch sits inside that function. D15 reaches `/queryChanges` through `self.changes(..)`.
- For an ordinary account the function costs one cached classification read (`ZaEventReader::new`).
- The insertions in `query.rs` stay at the two points; everything else is in `query/za.rs`. This path charges no memory budget for the views it opens.

- [ ] **Step 1: write the failing unit test**, `jmap`, in `query/za.rs`: `text_candidates_narrow_only_under_and`. The mask is `{1..=9}`; `T` is a text leaf on the title.

  | Filters | Candidates |
  |---|---|
  | `[T]` | 1 to 9 |
  | `[Set{1,2,30}, T]` | 1, 2: the top level is an AND |
  | `[And, Set{1,2}, T, End, Set{2,3}]` | 2 |
  | `[Or, Set{1}, T, End]` | 1 to 9 |
  | `[Not, Set{1}, End, T]` | 1 to 9 |
  | `[Or, And, Set{1}, T, End, End]` | 1 to 9: an AND under an OR does not narrow |
  | `[And, T, Set{4,5}, End]` | 4, 5: a grantee's wrapper narrows to the public events |
- [ ] **Step 2: run it.** U-ws `text_candidates_narrow_only_under_and`. Expected: fails to compile, `file not found for module za`.
- [ ] **Step 3: write the failing za cases** in `query.rs`. Banner: `Running zero-access JMAP calendar query tests...`. Set-up: `pair(test, "query")` (the owner, and `key2`, the grantee) and the ordinary account `jmapcal-query-plain@example.com` (`create_user_account`), which the module destroys itself at its end, after `wait_for_index_queue(test, plain_id, Duration::from_secs(30))`. Every event is written over CalDAV. No event names an account of the module as organizer or attendee, so nothing is scheduled or mailed. End: `finish(test, "query")`. The cases run in the order below.

  `two_accounts_same_answers`. The same three events are in the key account and in the ordinary account, and nothing else is in either: the case runs first and deletes its events at its end. Their text is that of Task 25's three events, except that the meeting has `ORGANIZER;CN=olive organizer:mailto:jmapcal-query-olive@remote.test` and one `ATTENDEE;CN=arthur guest:mailto:jmapcal-query-arthur@remote.test`. If the server refuses a PUT whose organizer and attendee are both outside addresses, stop and report: the owner conditions need another fixture. The ordinary account's index is awaited first (`wait_for_index_queue`). The conditions:
  1. `{ "title": "budget" }`
  2. `{ "text": "marketing" }`
  3. `{ "description": "review" }`
  4. `{ "location": "alpha" }`
  5. `{ "participant": "jmapcal-query-arthur@remote.test" }`
  6. `{ "owner": "jmapcal-query-olive@remote.test" }`
  7. `{ "title": "budget review" }`
  8. `{ "title": "team offsite" }`
  9. `{ "title": "\"budget review\"" }`
  10. `{ "title": "\"review budget\"" }`
  11. `{ "title": "\"sync offsite\"" }`
  12. `{ "location": "\"room alpha\"" }`
  13. `{ "location": "\"alpha room\"" }`
  14. `{ "owner": "\"olive organizer\"" }`
  15. `{ "owner": "\"organizer olive\"" }`
  16. `{ "participant": "\"arthur guest\"" }`
  17. `{ "participant": "\"guest arthur\"" }`

  Conditions 12 to 17 are a phrase in each word order on a field the index holds without positions. The assertions:
  - for each condition, and for `{ "operator": "NOT", "conditions": [<it>] }`: the UIDs of `CalendarEvent/query`'s ids on the key account equal those on the ordinary account;
  - conditions 1, 7 and 8 match at least one event on the ordinary account, so the comparison is not vacuous;
  - expanded (`expandRecurrences: true`, `after` and `before` around the series), on the key account: `{ "title": "offsite" }` gives 1 result, the overridden occurrence; `{ "title": "sync" }` gives 4;
  - expanded, for "offsite", "sync" and "team offsite": the starts of the results on the key account equal those on the ordinary account;
  - expanded with `{ "operator": "NOT", "conditions": [{ "title": "sync" }] }`: the method error `invalidArguments`, on both accounts.

  `filters_and_sorts`. No text condition, so no event is opened. Each result is compared with the ids the test wrote:
  - `inCalendar`: only the events of that calendar;
  - `uid`: the one event;
  - `after` and `before`: the events in the window;
  - sort by `start`, `uid`, `created` and `updated`, ascending and descending: the order of the stored values;
  - `expandRecurrences` with `after` and `before` over a daily series of five: five results with synthetic ids.

  `text_matches_the_view_not_the_tree`. An event with SUMMARY "jmapcal veiled word".
  - `{ "title": "veiled" }` gives exactly its id;
  - `stored_event(..).bytes()` does not contain "veiled";
  - `{ "title": "absentword" }` gives nothing;
  - `search_keys(test, key_id, SearchIndex::Calendar).is_empty()`.

  `grantee_privacy`. `make_calendar(.., "jmapcal-shared")` and `share(.., "view")` to `key2`; the calendar stays until the module's end. One event with `CLASS:PRIVATE` and SUMMARY "jmapcal private word", one public with SUMMARY "jmapcal public word".
  - `key2`, with the owner's account id: `{ "title": "public" }` gives exactly the public event; `{ "title": "private" }` gives nothing;
  - the owner: `{ "title": "private" }` gives exactly the private event.

  `query_changes`.
  - `s` is `CalendarEvent/query`'s `queryState` with the filter `{ "title": "veiled" }`. A second event with that word is written. `CalendarEvent/queryChanges` with that filter and `sinceQueryState: s`: no method error; `added` holds exactly the second event's id.
  - On the calendar `grantee_privacy` shared: `mark_boundary_pending`, then the owner's `Calendar/get` completes it. `b` is `owner_boundary(..).rename_change_id`. With `sinceQueryState: State::new_exact(b)`: `key2`'s `CalendarEvent/queryChanges` is `cannotCalculateChanges`; the owner's is not.

  `metadata_condition_refused`. `Account::jmap_request` with the `using` list `urn:ietf:params:jmap:core`, `urn:ietf:params:jmap:calendars`, `urn:ietf:params:jmap:metadata`, and `CalendarEvent/query` with `filter: { "metadataExists": "example.com/note" }`: the method error `unsupportedFilter`; its `description` is `ZA_METADATA_REFUSED`.

  `text_query_cost` (review focus 5). 300 events in a calendar of their own, each with SUMMARY "jmapcal bulk <n>"; one also holds "needleword". Event `n` is one hour long and starts at 10:00 UTC on day `n % 30` of a 30-day span, ten a day. They are written with `uid_lease::put`, one after another.
  - `{ "title": "needleword" }` gives exactly the needle's id, within the JMAP test client's five seconds; print `text_query_cost: 300 events opened, one text query without a time range, <ms> ms`;
  - the same condition with `after` and `before` around the needle's day gives the same id; print `text_query_cost: 10 events opened, the same query within one day, <ms> ms`;
  - the counts are by construction; no counter is added;
  - cleanup: `delete_event_records` for each event, then the empty calendar over CalDAV, then `wait_for_index_queue(test, key_id, Duration::from_secs(60))`.
- [ ] **Step 4: change the two existing tests**, each with a comment citing spec 6.2.
  - `gating.rs`: `CalendarEvent/query` (`:375`) and `CalendarEvent/queryChanges` (`:392-395`) go to the served loop.
  - `za_variants.rs`: take out `CalendarEvent/query` (`:3526`); in the served block it has no error and an empty `ids` list.
- [ ] **Step 5: run ZA.** Expected: fails in `jmap_calendar::query`, at `two_accounts_same_answers`, on `accountNotSupportedByMethod` for the key account's first query.
- [ ] **Step 6: implement** `query/za.rs` and the two points in `query.rs`.
- [ ] **Step 7: flip the rows** `(CalendarEvent, Query)` and `(CalendarEvent, QueryChanges)` to `Served`, in the function and in `calendar_method_classes`. Run U-ws `text_candidates_narrow_only_under_and`, then `calendar_method_classes`. Expected: 1 passed each.
- [ ] **Step 8: run ZA.** Expected: pass. Proved by the seven cases of `jmap_calendar::query`. Copy the two `text_query_cost` lines into the ledger; Task 43 puts them in the outcome note.
- [ ] **Step 9: run JK.** Expected: pass.
- [ ] **Step 10: the closing checks.**
- [ ] **Step 11: commit.** `Release 6: CalendarEvent/query and queryChanges for key accounts`

Mutation catchers for Task 42: "a text filter reads the stored tree instead of the view" and "the unexpanded matcher's document is built from the stored tree" are caught by `text_matches_the_view_not_the_tree`; "the unexpanded matcher reads one component at a time" by condition 8 of `two_accounts_same_answers` and by Task 25's rows `team offsite` and `"sync offsite"`.

---

### Task 27: colour on a shared calendar

**Spec:** D14 (in 6.7); 11 (the silent dependency on the bundle's layout).

**Files:**
- Modify:
  - `crates/groupware/src/calendar/seal/collection.rs:205-235` (`seal_personal_entry`, the bundle's colour at `:215`), `:300-327` (`open_personal`: the refusal at `:318-320`, the assignment at `:326`), `:394-480` (`seal_calendar_changes`: the reseal condition at `:454-457`, the refusal at `:474-476`), the module doc at `:7-20`, and the test at `:687`.
  - `crates/groupware/src/calendar/sharing/convert.rs`: its `tests` module only (`:967-1034`).
  - `tests/src/za/sharing.rs:1906-1909`.
- Must not touch: the struct `Bundle` (`collection.rs:42-48`) or any rkyv layout; the rings; `open_shared` (`:329-337`), which keeps refusing a colour in the shared bundle; `za_replace_calendar_entries` (`crates/groupware/src/calendar/sharing/format.rs:585-601`), which keeps refusing a stored entry whose `color` field is set.

**Interfaces:** no new name and no signature change (I §8). The task changes the behaviour of `seal_personal_entry`, `open_personal` and `seal_calendar_changes`, and reads through `calendar_view_in_domain` and `convert.rs`'s private `initialize` (`:365-416`).

The three changes:

1. `seal_personal_entry` writes `entry.color` into the bundle. The returned entry's `color` field stays `None`.
2. `open_personal` returns the bundle's colour in `entry.color`. A bundle that holds none reads as `None`.
3. `seal_calendar_changes` reseals the caller's entry when its colour differs from the opened one, as it does for the name, the description and the time zone, and no longer refuses a colour.

`convert.rs` needs no production change. `initialize` already hands the owner's opened entry to `seal_personal_entry`, and the restore at the end of `rollback` (`:528-531`) already reseals the opened view with `seal_calendar`, whose bundle holds the colour.

From this task on a new grantee's entry starts with the owner's colour, as with the owner's name and description, because a grant copies the owner's opened entry (`sharing/mutations.rs:314-324`). Task 28's `colour_before_and_after_sharing` pins it.

**Constraints:**
- No stored struct changes layout. The bundle already has the field; an entry written before this release holds `None` there and reads as no colour.
- A colour never sits in the clear on a sealed calendar: the stored entry's `color` field is `None` for every principal, and the value is only inside that principal's sealed bundle.
- Each principal's bundle is its own: a grantee's write changes its own entry and leaves every other entry byte for byte.
- A write that changes only visible preferences (sort order, flags, default alerts) still reseals nothing.
- The CalDAV colour is a dead property in the calendar's container and is not this colour; do not touch `seal/container.rs`.

- [ ] **Step 1: write the failing unit tests.** `view(calendar, domain, id)` is `calendar_view_in_domain` of the archived calendar, deserialized.

  In `collection.rs`, with its fixture `shared_fixture()` (`:553-638`: the owner, id 4, sealed from `calendar()`, whose colour is `#ff0000`; a grantee, id 8, without a colour).

  `personal_bundle_carries_colour`, in order:
  - the owner's view of the fixture: `preferences[0].color` is `#ff0000`;
  - in the stored fixture every entry's `color` is `None` and no entry's `name` contains "ff0000";
  - the grantee's view with `preferences[1].color = #00ff00`, through `seal_calendar_changes(stored, edited, grantee, 8)`: `Ok`;
  - the grantee's view of the result: `preferences[1].color` is `#00ff00`;
  - in the result every entry's `color` is `None` and no `name` contains "00ff00";
  - the result's `preferences[0]` equals the stored one: the owner's entry is untouched;
  - the result's `preferences[1].name` differs from the stored one: a colour change reseals the bundle;
  - the owner's view of the result: `#ff0000`;
  - the owner's view with `preferences[0].color = None`, through `seal_calendar_changes(.., owner, 4)`: the owner's view of that result has no colour; its `preferences[1]` equals the previous result's; the grantee's view still reads `#00ff00`.

  `bundle_without_colour_reads_none`. The fixture's grantee bundle is what every earlier release wrote.
  - the grantee's view: `preferences[1].color` is `None`;
  - the same view with `sort_order = 9`, through `seal_calendar_changes`: `preferences[1].name` equals the stored one (nothing resealed), `sort_order` is 9, and the grantee's view of the result has no colour.

  In `convert.rs`: `conversion_keeps_owner_colour`. A calendar with one entry (account 4, name "Work", description "desc", colour `#ff0000`), sealed with `seal_calendar` under fresh keys of account 4. A `ZaCalendarContext` for it: `owner_id` 4, `calendar_id` 42, `personal_id` 4, `domain` `ZaKeyDomain::owner(&keys)`, `boundary` `None`.
  - `initialize(&context, &keys)` is `Ok((converted, domain))`;
  - `converted.preferences[0].color` is `None` and its `name` does not contain "ff0000";
  - the view of `converted` in `domain` for account 4: `color` is `#ff0000`, `name` is "Work";
  - that view through `seal_calendar` and then `unseal_calendar`: `color` is `#ff0000` (the restore path).
- [ ] **Step 2: run them.** U-groupware `colour`. Expected: two fail and one passes. `personal_bundle_carries_colour` and `conversion_keeps_owner_colour` fail on `left: None, right: Some("#ff0000")`. `bundle_without_colour_reads_none` passes already and guards the old bundle through the change.
- [ ] **Step 3: implement** the three changes. The module doc says a shared collection's personal bundle carries the name, the description and the colour.
- [ ] **Step 4: change the two existing assertions**, each with a comment citing D14.
  - `collection.rs:687`, in `shared_collection_uses_two_three_and_five_part_forms`: `assert!(opened.preferences[0].color.is_none());` becomes `assert_eq!(opened.preferences[0].color.as_deref(), Some("#ff0000"));`.
  - `tests/src/za/sharing.rs:1906-1909`, inside the loop over `empty`: the assertion "shared format excludes JMAP personal color" becomes

```rust
// Release 6 spec 6.7, D14: the conversion carries the owner's colour in its sealed personal bundle.
assert_eq!(
    view.preferences[0].color.as_deref(),
    (!empty).then_some("legacy JMAP-only"),
    "the conversion carries the owner's colour"
);
```

- [ ] **Step 5: run the unit tests.** U-groupware `seal::collection`, then U-groupware `sharing::`. Expected: every test passes, the three new ones among them.
- [ ] **Step 6: run ZA.** Expected: pass. Proved by `sharing` (the changed assertion) and `dav_seal::test_collections` (an unshared calendar is as before).
- [ ] **Step 7: the closing checks.**
- [ ] **Step 8: commit.** `Release 6: a shared calendar's colour in the sealed personal bundle`
