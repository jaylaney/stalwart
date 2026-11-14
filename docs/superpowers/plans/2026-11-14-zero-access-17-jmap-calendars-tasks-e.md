# Plan 17, group E: closing (Tasks 38 to 44)

These tasks belong to [zero-access plan 17: JMAP calendars and browser sign-in](2026-11-14-zero-access-17-jmap-calendars.md), whose header holds the commands, the constraints and the task index. "I §n" is section n of the [interface sheet](2026-11-14-zero-access-17-jmap-calendars-interfaces.md).

### Task 38: The differential oracle

**Spec:** 3.2, 3.5, 3.6, 9.3; review focus 4.

**Files:**
- Create: `tests/src/za/jmap_calendar/oracle.rs`
- Modify: `tests/src/za/jmap_calendar/mod.rs` (`pub mod oracle;`)
- Modify: `tests/src/za/mod.rs` (the call `jmap_calendar::oracle::test(&mut test).await;` after `jmap_calendar::refusals::test`, before `sweep::test` at `:191`)
- Modify: `tests/src/za/imip/oracle.rs`, visibility only (I §10): `Kind` (`:91`), `without_schedule_status` (`:153`), `Tally`, its three fields and `Tally::same` (`:581-610`), `mail_key` (`:772`), `mail_parts` (`:788`), `raw_header` (`:816`) become `pub(crate)`
- Modify: `tests/src/utils/za.rs` (two functions appended)
- Test: ZA

**Interfaces:**
- Consumes:
  - `ZaScheduling` (I §2.1) as the JMAP key writer maps it (Tasks 30, 31, 33): `sendSchedulingMessages: true` is `Required`; false or absent is `Off`.
  - The drain hook (I §7.4, Task 34): a keyed `Calendar/get` runs the drain.
  - `key_account`, `make_calendar`, `finish` (I §10).
  - In `tests/src/utils/za.rs`: `copy_href`, `new_inbox_items`, `open_pending_item_as`, `item_method`, `item_sequence`, `schedule_statuses`, `trigger_drain`, `wait_for_queue`, `za_unlock`, `pin_record`, `wait_for_index_queue`.
  - In `tests/src/za/`: `fanout::{Party, stored_pairs, uid_copies, NO_REPLY}`, `uid_lease::put`, `drain::{DrainOn, body, set_partstat}`, `imip::{deliver, itip_mail}`, `pinning::withheld`; `smtp_sink::sink`.
  - `Account::jmap_method_calls`, which signs in with HTTP Basic.
- Produces: `jscalendar_of`, `normalize_jscal` (I §10); `jmap_calendar::oracle::test`.

**Constraints:**
- The oracle is the release's stopping rule (3.6): a difference it finds is fixed by routing the JMAP path through the pipeline, never by a new normalization.
- `normalize_jscal` removes `id`, `baseEventId`, the keys of `calendarIds`, and `created` and `updated` at any depth. It removes nothing else.
- **[RULE]** Both sides sign in with HTTP Basic. The first invitation pins the invitee's key, which is a vault write, and a Bearer session would end at that step.
- Fixtures. Key accounts, each through `key_account`: `jmapcal-oracle-cd` and `jmapcal-oracle-jm` (the two sides), `jmapcal-oracle-cd-inv` and `jmapcal-oracle-jm-inv` (each side's invitee). Ordinary: `jmapcal-oracle-plain`, Step 5 only. Outside parties: `jmapcal-oracle-cd-out@remote.test` and `jmapcal-oracle-jm-out@remote.test`. The names of the two sides have equal length, so the name mapping moves no fold point.
- The name mapping `jmapcal-oracle-cd` to `jmapcal-oracle-jm` is applied to the CalDAV side's values only.
- Each step starts with `sink().clear()`. The module holds `DrainOn` for its run and ends with `finish(test, "oracle")`.
- The `imip/oracle.rs` items keep file, name and body.

- [ ] **Step 1: Write `jscalendar_of`, `normalize_jscal` and `self_check`**

`self_check()` runs first in `test` and holds no server. `base` is `jscalendar_of` of step M6's invitation on the JMAP side.

- Each of these edits alone, and all five together, leaves `normalize_jscal` of the edited object equal to `normalize_jscal(base)`: `id = "zz"`; `baseEventId = "zz"`; `calendarIds = { "zz": true }`; `created = "2001-01-01T00:00:00Z"`; `updated = "2001-01-01T00:00:00Z"`.
- Each of these makes it unequal: `title = "another title"`; the invitee's `participationStatus = "accepted"`; the invitee's `calendarAddress = "mailto:other@example.com"`; a `locations` entry named `added`; `duration` removed.
- The mapping turns `mailto:jmapcal-oracle-cd-inv@example.com` into `mailto:jmapcal-oracle-jm-inv@example.com` and `jmapcal-oracle-cd-out@remote.test` into `jmapcal-oracle-jm-out@remote.test`.
- Without the mapping, the two sides' M6 objects are unequal.

- [ ] **Step 2: Write the setup**

- The four key accounts.
- The precondition of 9.3: `pin_record(test, id, STRONG)` is `None` for all four.
- The calendar `oracle-b` on both sides with `make_calendar`. Setup is not a compared step.
- Each side's JMAP calendar ids of `default` and `oracle-b`, read with `Calendar/get`.

- [ ] **Step 3: Write the script**

One step is performed on both sides, then compared. Each side's CalDAV input is the step's iCalendar text for that side. Each side's JMAP input is `jscalendar_of` of the same text:

- a create sends the whole object with `calendarIds`;
- an update sends the top-level difference between `jscalendar_of(previous text)` and `jscalendar_of(step text)`: each added or changed property with its new value, each removed one as `null`;
- the flag is `sendSchedulingMessages: true` on every step marked S, and absent otherwise.

Three exchanges, in this order. Times are UTC; a weekly series starts `20990302T100000Z`.

Quiet exchange, UID `jmapcal-oracle-quiet`. It holds 3.2's one input that is off on both protocols.

| n | Step | CalDAV | JMAP | Expect |
|---|---|---|---|---|
| Q1 | create with the invitee and the outside address as attendees | `PUT` | create, S | present; item `REQUEST` at the invitee; sink `(outside, REQUEST)` |
| Q2 | quiet delete | `DELETE` with `Schedule-Reply: F` | destroy with `sendSchedulingMessages: false` | absent; no item; no sink message |

Main exchange, UID `jmapcal-oracle-main`.

| n | Step | CalDAV | JMAP | Expect |
|---|---|---|---|---|
| M1 | create in `default`: `SUMMARY:oracle one`, `DTSTART:20990301T100000Z`, `DTEND:20990301T110000Z` | `PUT` | create | present in `default`; contains `SUMMARY:oracle one` |
| M2 | update: `SUMMARY:oracle two`, `DTSTART:20990302T100000Z`, `DTEND:20990302T110000Z` | `PUT` | update | contains `SUMMARY:oracle two`, `DTSTART:20990302T100000Z` |
| M3 | placement: `default` to `oracle-b` | `MOVE` | update of `calendarIds` to `oracle-b` alone | present in `oracle-b`, absent from `default` |
| M4 | recurrence: `RRULE:FREQ=WEEKLY;COUNT=4` | `PUT` | update | contains `RRULE:FREQ=WEEKLY;COUNT=4` |
| M5 | override of the second instance: `RECURRENCE-ID:20990309T100000Z`, `SUMMARY:oracle two moved`, `DTSTART:20990309T120000Z` | `PUT` | update | contains `SUMMARY:oracle two moved` |
| M6 | invite: `ORGANIZER` the account; `ATTENDEE` the invitee and the outside address, each `RSVP=TRUE;PARTSTAT=NEEDS-ACTION` | `PUT` | update, S | item `REQUEST` at the invitee; sink `(outside, REQUEST)`; the outside address's status is `1.1`; the invitee has a status; the stored pairs name the invitee alone |
| M7 | update: `LOCATION:room seven` on the master | `PUT` | update, S | item `REQUEST`; sink `(outside, REQUEST)` |
| M8 | the invitee accepts in its own copy over CalDAV (`set_partstat`), on both sides; the account then drains | `trigger_drain` | a keyed `Calendar/get` | item `REPLY` at the account, drained; the invitee's `PARTSTAT` is `ACCEPTED` |
| M9 | the outside address's emailed `REPLY`, `ACCEPTED`, delivered over LMTP to each account; the account then drains as in M8 | `trigger_drain` | a keyed `Calendar/get` | the outside address's `PARTSTAT` is `ACCEPTED` |
| M10 | exclusion of the third instance: `EXDATE:20990316T100000Z` | `PUT` | update, S | item `CANCEL` at the invitee; sink `(outside, CANCEL)` |
| M11 | delete | `DELETE` | destroy, S | absent; item `CANCEL`; sink `(outside, CANCEL)` |

Inbound exchange, UID `jmapcal-oracle-in`. The invitee organizes through CalDAV on both sides; the account attends.

| n | Step | CalDAV | JMAP | Expect |
|---|---|---|---|---|
| I1 | the invitee's invitation arrives; the account drains | `trigger_drain` | a keyed `Calendar/get` | present; item `REQUEST` at the account, drained; the account's `PARTSTAT` is `NEEDS-ACTION` |
| I2 | the account accepts | `PUT` with `PARTSTAT=ACCEPTED` | update, S | item `REPLY` at the invitee; the account's `PARTSTAT` is `ACCEPTED` |
| I3 | the account deletes its copy | `DELETE` | destroy, S | absent; item `REPLY` at the invitee |

`Expect` is asserted on each side before the sides are compared, so two sides that both did nothing cannot pass. Its fields: whether the event is present; the collection that holds it; substrings of the unfolded CalDAV GET; one party's `PARTSTAT`; the new inbox items as `(whose inbox, METHOD)` in order; the sink messages as `(recipient, METHOD)`; an attendee's `SCHEDULE-STATUS` in the organizer's copy, as a value or as "any, not empty". A party is the account, the invitee or the outside address.

- [ ] **Step 4: Write the comparison**

After each step: `wait_for_queue`, the step's drains, then, under the label `"<exchange> step <n> (first CalDAV, second JMAP)"`:

1. `Expect`, on each side.
2. 3.5's first comparison: `normalize_jscal(jscalendar_of(the CalDAV GET of the event))` is equal on both sides, after the mapping.
3. The same event read with `CalendarEvent/get` on both sides is equal after `normalize_jscal` and the mapping.
4. 3.5's second comparison, items: the sorted lists of `(recipient, method, UID, SEQUENCE, recurrence ids)` of the new inbox items are equal. Then each pair of item trees goes through `Tally::same` with `Kind::Ics`.
5. The same for sink mail: the sorted lists of `mail_key`, extended with the UID, SEQUENCE and recurrence ids of `calendar_part()`, are equal. Then each pair goes part by part through `mail_parts` and `Tally::same`.
6. 3.5's third comparison: each attendee's `SCHEDULE-STATUS` (`schedule_statuses`) is equal, and the recipients of the stored token pairs (`stored_pairs`) are equal after the mapping.

`Tally::same` names its first value "plain" and its second "key" in a failure, so the CalDAV side is passed first. A failure prints both projections, which is what 9.3 asks of the first step that differs.

- [ ] **Step 5: Write `round_trip_keeps_what_an_ordinary_account_keeps`**

Called from `test` after the three exchanges.

Part A, what the conversion keeps. On `jmapcal-oracle-jm` and on `jmapcal-oracle-plain`, a CalDAV `PUT` of:

```
BEGIN:VEVENT
UID:jmapcal-oracle-rt
DTSTART:20990401T100000Z
DTEND:20990401T110000Z
SUMMARY:round trip
X-JMAPCAL-KEEP:native value
BEGIN:VALARM
ACTION:DISPLAY
TRIGGER:-PT10M
DESCRIPTION:alarm text
X-JMAPCAL-ALARM:native alarm value
END:VALARM
END:VEVENT
```

Then on both a JMAP update `{ "title": "round trip edited" }`, with no scheduling flag. With `lines(account)` the set of unfolded lines of the CalDAV GET, less `DTSTAMP`, `CREATED` and `LAST-MODIFIED`, names mapped:

- `lines(key)` holds `SUMMARY:round trip edited`, and so does `lines(plain)`;
- `lines(key) == lines(plain)`.

The step prints which of `X-JMAPCAL-KEEP`, `DESCRIPTION:alarm text` and `X-JMAPCAL-ALARM` survived, for the outcome note.

Part B, the fork's own state, on `jmapcal-oracle-jm` alone. The event `jmapcal-oracle-rt2` names two attendees: `jmapcal-oracle-jm-inv`, and `jmapcal-oracle-cd-inv` whose pin in the organizer's pin record was first replaced by another key, as `pinning::case_02_refusal` arranges a refusal (`tests/src/za/pinning.rs:189-249`). A `PROPPATCH` sets the event's `D:displayname` to `rt2 name`.

- Preconditions: the withheld attendee's `SCHEDULE-STATUS` is `3.8`; `withheld(test, jm.id, cd-inv)` is not empty.
- After a JMAP update `{ "description": "edited in the browser" }` with no scheduling flag: the pairs' recipients, the display name and the withheld entries' `refusal_id`s are what they were, and the status is still `3.8`.
- After a JMAP update of `locations` with `sendSchedulingMessages: true`: the pairs' recipients and the display name are what they were, and `withheld(..)` is not empty.

Cleanup: both events deleted with `Schedule-Reply: F`; the invitees' items deleted with `Party::delete_items_since`; `wait_for_index_queue` for `jmapcal-oracle-plain`, then `admin.destroy_account`.

- [ ] **Step 6: Run ZA**

Expected: passes; the output holds `Running zero-access JMAP calendar oracle tests...` and one tally line per step.

- [ ] **Step 7: Kill-check the oracle**

In `crates/jmap/src/calendar_event/set/za.rs`, the destroy path maps `sendSchedulingMessages: false` to `ZaScheduling::On`. Run ZA. Expected first failure: `quiet step Q2`, a `CANCEL` on the JMAP side that `Expect` forbids. If an earlier module fails first, record it, skip that module's call with a tests-only edit and run again. Both edits are reverted with `git checkout -- crates/ tests/`. Record the failing assertion in the ledger.

- [ ] **Step 8: The closing checks**

- [ ] **Step 9: Commit**: `Release 6: the differential oracle of CalDAV against JMAP`

### Task 39: Upstream's JMAP calendar tests in key mode

**Spec:** 9.4; 3.3, 6.2, 6.3, 6.5, 6.7, 6.9, 6.11, D10, D11.

**Files:**
- Modify: `tests/src/jmap/za_variants.rs` (`calendars`, `:3510-3574`, rewritten in place; five private functions after it; the module doc, `:7-11`)
- Modify: `tests/src/jmap/mod.rs:247-249` (a comment only; the two calls stay as they are)
- Test: JK and JP

**Interfaces:**
- Consumes: upstream's bodies `calendar::{calendars, instance, alarm}::test` under `tests/src/jmap/calendar/`, read-only; `test_jscalendar_1` to `test_jscalendar_4` and `assert_eq_ignoring_updated` in `tests/src/jmap/calendar/event.rs`; `za_post` in `tests/src/utils/za.rs`, for `contacts/pin`, `shares/grant` and `shares/revoke`; `Server::za_served_public_key`; the value Task 30 pins in `events::create_use_default_alerts`.
- Produces: `za_variants::calendars`, the key-mode entry of the calendar block. Private to the file: `calendar_event`, `calendar_identity`, `calendar_notification`, `calendar_acl`, `calendar_availability`, each taking `&TestServer`.

**Constraints:**
- Upstream's test bodies under `tests/src/jmap/calendar/` and `tests/src/jmap/principal/` are not edited.
- The calls at `tests/src/jmap/mod.rs:248-249` do not change: the inventory gate matches them.
- Rule R44: a variant's doc comment names the upstream body it mirrors and each ruling that changes an assertion. Where it mirrors, it mirrors line for line.
- An assertion of an upstream body that fails in key mode and that no ruling of this task's tables explains is a finding. It is not answered with a variant.
- Each variant ends as its upstream body ends: its calendars destroyed, `test.assert_is_empty()`.
- `principal_get` is Task 35's and is not touched.

- [ ] **Step 1: Make `calendars` the entry**

`calendars` is rewritten whole. Its new body replaces the served block and the refusal loop that Tasks 21 to 30 left in it, with their CalDAV delete of the default calendar and their clear of the identities record. It prints `Running key-account Calendar tests...` and runs, in upstream's order: `calendar::calendars::test`, `calendar_event`, `calendar::instance::test`, `calendar_notification`, `calendar::alarm::test`, `calendar_identity`, `calendar_acl`, `calendar_availability`.

Run JK once the five functions exist as empty bodies.
Expected: `Running Calendar tests...`, `Running Calendar Event instance tests...` and `Running Calendar Alarm tests...` print, and the three bodies pass for key accounts.

- [ ] **Step 2: Write `calendar_event`, a mirror of `calendar::event::test`**

Every line is upstream's except these:

| Upstream (`event.rs`) | Key mode | Ruling |
|---|---|---|
| `:85-90`, the create of event 1 with `isDraft`, `mayInviteSelf`, `mayInviteOthers`, `hideAttendees` true | First a create with the four flags: `not_created(0).typ() == "invalidProperties"`. Then the body's create without them. | 6.5 |
| `:154-160`, the full read of event 1 | the four flags and `isDraft` read `false` | 6.3 |
| `:169` and `:175-187`, event 2 | `useDefaultAlerts` reads `false`; `alerts` is the value `events::create_use_default_alerts` pins for the same default alert | D11 |
| `:237-240`, `:250` | event 1's three flags read `false`; event 2's `useDefaultAlerts` reads `false` | 6.3, D11 |
| `:350-370`, a second event with UID `00959BC664CA650E933C892C@example.com` created in calendar 1 while event 2 holds it in calendar 2 | it is created; the variant destroys it at once. Then the same create in calendar 2: `not_created(0).typ() == "invalidProperties"` and `description() == "An event with UID 00959BC664CA650E933C892C@example.com already exists."` | 3.3, UID rule |
| `:467` | event 1's `useDefaultAlerts` reads `false` after the patch that set it | D11 |

The CalDAV read at `:802-807` stays as upstream wrote it: D11 writes the alarm as upstream's view shows it, with `JSID:abc` and the event's title as its description.

- [ ] **Step 3: Write `calendar_identity`, a mirror of `calendar::identity::test`**

| Upstream (`identity.rs`) | Key mode | Ruling |
|---|---|---|
| `:48-55`, destroy of `b` | `not_destroyed("b").typ() == "forbidden"` | 6.9 |
| `:56-75`, the list after it | both identities, `a` the default | 6.9 |
| `:78-101`, two creates with an address that is not the account's | each `not_created(n).typ() == "forbidden"` | 6.9 |
| `:104-114`, a create with `onSuccessSetIsDefault: "#i0"` | `not_created(0).typ() == "forbidden"` | 6.9 |
| `:115-140` | A `/set` that holds only `onSuccessSetIsDefault: "b"`. Then the list: `a` with `isDefault: false`, `b` with `isDefault: true` and the name `John Doe`. | 6.9 |

The cleanup at `:143-150` is upstream's.

- [ ] **Step 4: Write `calendar_notification` and `calendar_availability`**

`calendar_notification`, for `jdoe`: each of `CalendarEventNotification/get`, `/set`, `/changes`, `/query` and `/queryChanges` answers the method error `accountNotSupportedByMethod` (6.11).

`calendar_availability` mirrors `principal::availability::test` with these changes:

| Upstream (`availability.rs`) | Key mode | Ruling |
|---|---|---|
| `:94-109`, the `shareWith` grant | `not_updated(&calendar1_id).typ() == "forbidden"` | 6.11 |
| every `Principal/getAvailability` of John after it | `list_array()` equals `json!([])` | 6.2 |

- [ ] **Step 5: Write `calendar_acl`**

It follows `calendar::acl::test` with the grant made through the account API. The calendar's path name, which the share endpoints take, is read from `fetch_groupware_resources` as `event.rs:773-790` reads a path. A grant is `contacts/pin` then `shares/grant`, as `tests/src/webdav/za_variants.rs:209-222` makes one.

| Upstream (`acl.rs`) | Key mode | Ruling |
|---|---|---|
| `:146-161`, John's `shareWith` | `not_updated(&john_calendar_id).typ() == "forbidden"`; then the grant at level `view` through the endpoints | 6.11 |
| `:162-190`, John's read of `shareWith` | unchanged: `mayReadItems` true, the other eight rights false | 6.7 |
| Jane's reads of John's calendar | `id` and `myRights` asserted; the name is Jane's own entry and is not asserted here | 6.7 |
| `:232-289`, `:399-455`, `:652-707`, the share notifications | `jane.jmap_changes(ShareNotification, state)` shows no change: the share endpoints write none | 6.11 |
| `:302-313`, Jane's `shareWith: {}` | `not_updated(..).typ() == "forbidden"` | 6.11 |
| added after `:313`, while the level is `view`: Jane's update of the calendar's name | `not_updated(..).typ() == "forbidden"` | 6.7 |
| `:314-325`, Jane's destroy of the calendar | unchanged: `"You are not allowed to delete this calendar."` | 6.7 |
| `:330-353`, Jane's event update and destroy at `view` | `not_updated(..).typ() == "forbidden"`; `not_destroyed(..).typ() == "forbidden"` | section 8 |
| `:357-369`, the write grant | the grant at level `edit` through the endpoints | 6.11 |
| `:372-394`, Jane's `myRights` | `mayReadItems`, `mayWriteAll`, `mayWriteOwn`, `mayUpdatePrivate`, `mayRSVP` true; `mayDelete`, `mayShare`, `mayReadFreeBusy`, `maySetMetadata` false | 6.7: the `edit` level holds no `Delete` |
| `:474-525`, `CalendarEvent/copy` in both directions | the method error `accountNotSupportedByMethod` | 6.11 |
| `:606-617`, the revocation | `shares/revoke` through the endpoint, then `test.wait_for_tasks()` | 6.11 |
| `:710-736`, the grant of `mayDelete` and Jane's destroy | a grant at `edit`; Jane's destroy answers `not_destroyed(..)` with `"You are not allowed to delete this calendar."`; John destroys the calendar | 6.7 |
| `:738-744`, the cleanup | `destroy_all_calendars` for both; no `destroy_all_event_notifications`, which a key account refuses; `test.assert_is_empty()` | 6.11 |

The level in force at `:456-471` and `:528-603` is `edit`, granted at the `:357-369` row. Those lines are upstream's, unchanged: a calendar created in a shared account is refused by upstream's check; Jane's event edit is served; Jane's own name and description of John's calendar are served, and John does not see them. At `view` the same update of the name is refused, which the added row asserts (6.7). Jane's reads before the first grant and after the revocation keep upstream's `accountNotFound`.

- [ ] **Step 6: Run JK three times and JP once**

Expected: each passes. Record each JK run's result and time: Task 41 decides the CI rerun from them.

- [ ] **Step 7: The closing checks**

- [ ] **Step 8: Commit**: `Release 6: upstream's JMAP calendar tests run for key accounts`

The task report lists every assertion of the tables above with its upstream line and ruling, for the outcome note's "Adjusted assertions".

### Task 40: Leak coverage

**Spec:** 9.5; 4.3, 4.4, 8, invariants 58, 59, 61, 62, 70; D11, D14.

**Files:**
- Modify: `tests/src/za/leak.rs`: `EXTRA_CANARIES` (`:177-240`); `Scan` (`:344-381`); the `Property` arm of `scan()`, after the field-152 arm (`:1361-1379`); `secret_hits` beside `token_hits` (`:1719`); `test_jmap_calendars` after `test_imip` (`:3199`)
- Modify: `tests/src/za/mod.rs` (the call `leak::test_jmap_calendars(&mut test).await;` after `leak::test_imip` at `:207`)
- Test: ZA

**Interfaces:**
- Consumes: `ZaTokenRecord`, `TOKEN_RECORD_VERSION`, `MAX_REFRESH_ENTRIES`, `MAX_ACCESS_ENTRIES`, `MAX_KEYED_GRANTS` (I §5.3); `Server::za_token_read`, `ZaTokenRead` (I §5.4); `za_oauth_client`, `za_drop_oauth_client`, `za_authorize`, `za_exchange`, `za_refresh`, `za_revoke_token`, `za_split_token`, `za_bearer_jmap`, `CALENDAR_JMAP_USING`, `raw_token_record`, `set_raw_token_record`, `handoff_exists`, `pair`, `make_calendar`, `share`, `finish` (I §10). `vault::keys::WRAP_LEN`. In `tests/src/utils/za.rs`: `za_unlock`, `hold_task_consumer`, `itip_tasks_for`, `wait_for_queue`. In `tests/src/za/`: `leak::{scan, sink_and_purge_held}`, `drain::DrainOn`, `mail::Traced`. `LoginResponseTest::unwrap_code` in `tests/src/system/oidc.rs`.
- Produces: `Scan.token_records`, `leak::secret_hits` (I §10); `leak::test_jmap_calendars`.

**Constraints:**
- Existing rules of the scan do not change, and `token_hits` keeps its callers.
- The field-153 rule lives in `scan()`, so it applies to every scan of every module.
- No codec of the token record is public: the scan reads through `Server::za_token_read`. The cap rule therefore has no negative control; Task 13's `record_round_trips_and_caps` covers the caps at the codec.
- A token secret is searched as its 32 bytes and as its 43 characters. A code is searched as its 40 characters.
- The hand-off entry is in the in-memory store, which no scan reads: it is checked by its exact key with `handoff_exists`.
- **[RULE]** Calendar writes of this module sign in with HTTP Basic, and its Bearer sessions are opened after its share and its first invitation.
- Fixtures: `pair(test, "leak")`; the OAuth client id `za-jmapcal-leak`, dropped at the end; the outside attendee `jmapcal-leak-addr-canary@remote.test`.
- Every canary is in `EXTRA_CANARIES`, and the test asserts that each was listed and sent, as `test_imip` does (`:3210-3220`).

- [ ] **Step 1: Add the field-153 rule to `scan()`**

The arm takes a `Property` key of the scanned account, of length `U32_LEN + 2 + U32_LEN`, whose collection is `Principal` and whose field is `ZeroAccessTokens`. It counts `scan.token_records`, runs `check_canaries` on the value as private data under the name "token record", and reports:

| Violation | When |
|---|---|
| `{what}: field 153 is outside principal document 0` | the key's document is not 0 |
| `{what}: field 153 does not parse as a token record` | `za_token_read(account_id)` gives no record, or its `raw` is not the scanned value |
| `{what}: field 153 has version {v}` | `v` is not `TOKEN_RECORD_VERSION` |
| `{what}: field 153 holds {n} grants` | `n` is above `MAX_KEYED_GRANTS` (32) |
| `{what}: field 153 grant {id} holds {n} refresh entries` | `n` is above `MAX_REFRESH_ENTRIES` (2) |
| `{what}: field 153 grant {id} holds {n} access entries` | `n` is above `MAX_ACCESS_ENTRIES` (4) |
| `{what}: field 153 grant {id} holds a wrap of {n} bytes` | an entry's wrap is not `WRAP_LEN` (72) bytes |

- [ ] **Step 2: Write `secret_hits`**

It is `token_hits`' search over byte strings of any length, with these forms per secret: the bytes; base64url without padding; standard base64, with and without padding, of the secret preceded by 0, 1 and 2 bytes, keeping the characters that do not depend on the prefix; lower and upper hex. `token_hits` is not rewritten.

- [ ] **Step 3: Write the negative controls**

- Junk in field 153 is a violation: after `set_raw_token_record(test, leak_id, Some(b"jmapleak-junk"))`, a scan reports a violation that ends `field 153 does not parse as a token record`. The field is then cleared.
- A record that holds a secret in the clear is found. After the first sign-in of Step 4, the record's raw bytes with the access secret's 32 bytes appended are planted; `secret_hits(test, &[access secret])` then holds a hit in `Subspace::Property`. The original bytes are restored.

- [ ] **Step 4: Write `test_jmap_calendars`**

It prints `Running zero-access leak test of JMAP calendars and tokens...`.

Canaries: `jmapleak-title-canary`, `jmapleak-desc-canary`, `jmapleak-loc-canary`, `jmapleak-part-canary` (a participant's name), `jmapcal-leak-addr-canary` (the outside participant's address), `jmapleak-kw-canary` (a keyword), `jmapleak-ovr-canary` (an override's title), `jmapleak-task-canary` (a task's title), `jmapleak-grantee-canary` (a title the grantee writes), `jmapleak-calname-canary`, `jmapleak-caldesc-canary`, `jmapleak-ownname-canary` (the grantee's own name of the shared calendar). The colours `#a1b2c3` (the owner's) and `#c3b2a1` (the grantee's) are searched with `secret_hits`.

The writes, under `DrainOn`, with the task consumer held (`hold_task_consumer`) and inside one trace capture, `Traced::start(EventType::variants())`:

1. `Calendar/set` create by the owner: name, description, colour `#a1b2c3`, one default alert with time.
2. The share at level `edit`; the grantee's `Calendar/set` update of its own entry: name, colour `#c3b2a1` (D14).
3. `CalendarEvent/set` create by the owner in an unshared calendar, with `sendSchedulingMessages: true`: title, description, locations, keywords, colour, `useDefaultAlerts: true` (D11), a recurrence rule with one override, and the participants `jmapcal-leak-2` and the outside address.
4. `CalendarEvent/set` update of the override's title; a task create.
5. The grantee's `CalendarEvent/set` create in the shared calendar, with `useDefaultAlerts: true`.
6. `CalendarEvent/set` destroy of the task, with `sendSchedulingMessages: true`.

Then, in this order:

- `itip_tasks_for` is 0 for both accounts. The consumer is released; `sink_and_purge_held`.
- Positive control: a sink message's calendar part holds `jmapleak-title-canary`.
- The sessions, for both accounts: `za_authorize` with consent given; the code is 40 characters and the 43 of its hand-off secret; `za_exchange` is granted; `handoff_exists(code)` is false; `za_refresh` is granted; one keyed `Calendar/get` per access token through `za_bearer_jmap` answers 200.
- The trace capture ends here. No captured event's text holds a canary, either colour, or the 43 characters of a token secret or hand-off secret. One exemption, as `imip::traces` has it: the outside address in the events of the SMTP, queue and delivery layers, which carry the envelope recipient.
- `secret_hits` is empty for: every access and refresh secret of both accounts, first and refreshed, as bytes and as text; both codes' 40 characters and both hand-off secrets; both accounts' MK (`za_unlock(..).mk()`).
- `secret_hits(test, &[b"#a1b2c3", b"#c3b2a1"])` is empty.
- For each account, `scan` reports no violation, `token_records == 1`, and at least one event and one calendar.
- The sessions end: `za_revoke_token` answers 200 for `jmapcal-leak-2`'s refresh token; `jmapcal-leak` changes its password through `POST /api/vault/password`, as `tests/src/za/password.rs` does. A password change ends the session under either rule.
- For each account `raw_token_record` is `None` and a scan's `token_records` is 0. `secret_hits` over the same secrets is empty.

The scan's existing rules give the rest of 9.5 for both accounts: sealed archives only, no event preferences (`check_event`), no Calendar search entry (`check_search`), each calendar record sealed (`check_calendar`).

Cleanup: the events and the two calendars destroyed with `sendSchedulingMessages: false`; the grant revoked through `shares/revoke`; the invitee's items deleted; the OAuth client dropped; `sink().clear()`; `finish(test, "leak")`.

- [ ] **Step 5: Run ZA**

Expected: passes. Every earlier scan passes the field-153 rule.

- [ ] **Step 6: The closing checks**

- [ ] **Step 7: Commit**: `Release 6: leak rules for the token record, token secrets and JMAP calendar writes`

### Task 41: The other harnesses and the gates

**Spec:** 9.6; 5.1 (the move's proof, on the head).

**Files:**
- Modify: `.github/workflows/test.yml:426-428` (the comment of "JMAP Tests (key accounts)")
- Modify: `tests/qualification/dispositions.json` (`cohort.additions`)
- Modify, only under Step 3's condition: `.github/workflows/test.yml:424-431` and `scripts/tests/ci_steps.py:186-188`
- Test: the commands of the steps below

**Interfaces:**
- Consumes: Tasks 1 to 40.
- Produces: for the outcome note, one record per harness, the list of cohort additions with the reason, and the rerun decision with its four run results.

**Constraints:**
- `tests/qualification/baseline-0.16.27.json` never changes.
- A change to a gated step's command, environment, `if` or rerun changes its row in `STEPS` in the same commit, or "Check Gates" fails.
- The inventory gate itself runs in CI's `gates` job (Task 44). Step 1 checks only that each retained function is still defined.
- A harness written in Python is not changed by this plan. A finding there is recorded and put to the project owner.
- Nothing is installed on the host without the project owner's word.

- [ ] **Step 1: Check that every retained test function is still defined**

Run:

```bash
python3 -I - <<'EOF'
import json, re, subprocess
base = json.load(open("tests/qualification/baseline-0.16.27.json"))
changed = set(subprocess.run(["git", "diff", "--name-only", "origin/release7-search...HEAD", "--", "tests/src"],
                             capture_output=True, text=True, check=True).stdout.split())
missing = [(f["file"], f["name"], sorted(f["executed"])) for f in base["helpers"]["functions"]
           if f["file"] in changed and f.get("executed")
           and not re.search(r"\bfn\s+%s\b" % re.escape(f["name"]), open(f["file"]).read())]
print("\n".join(map(str, missing)) or "every retained function is still defined")
EOF
```

Expected: `every retained function is still defined`. A function the list reports is restored in place by the task that removed it. After pull request 84 merges, the base in the command is `origin/main`.

- [ ] **Step 2: Run WK, IK and JK**

Expected: each passes. WK proves the move on the head: no assertion under `tests/src/webdav/` changed in this release (`git diff origin/release7-search...HEAD --stat -- tests/src/webdav` prints nothing).

- [ ] **Step 3: Decide the CI rerun of key-mode JMAP**

The step has no rerun today, and from this release it runs upstream's timing-sensitive `calendar::alarm` body.

- If Task 39's three JK runs and Step 2's run all passed: no rerun is added.
- If `calendar::alarm` failed in one of them and passed when repeated: the workflow step gains the rerun form of "JMAP Tests (plain accounts)" (`test.yml:402-408`), and its `STEPS` row gains `rerun="same"`, in one commit.
- Any other failure is a regression and is not answered with a rerun.

- [ ] **Step 4: Rewrite the workflow comment**

`test.yml:426-428` becomes: "The same for the JMAP suite (tests/src/jmap/za_variants.rs). Key mode runs upstream's calendar bodies and the calendar variants; the notification variant is a refusal, so upstream's timing-flaky calendar::notification sub-test does not run here."

- [ ] **Step 5: Run the fresh-store script and the CalDAV client against the product build**

Run, one at a time:
- PRODUCT;
- `python3 -B -m unittest discover -s tests/product -p test_fresh_store.py -v`;
- FRESH;
- CALDAV in key mode, then in plain mode.

Expected: FRESH passes. The CalDAV client's key mode is 1 skipped and 2 xfailed; plain mode carries its five records (`docs/zero-access/client-caldav.md`). If the virtual environment's packages are not on the host, the client is recorded as left to CI's `caldav-client` job.

- [ ] **Step 6: Add the release's runtime files to the coverage cohort**

Every new fork runtime file of the release joins `cohort.additions`, each with `"runtime_excluded": false` and an `evidence` string of the form `Plan 17 (release 6) Task <n> (commit <sha>): <what the file holds>. A fork runtime file added by release 6 (git log --diff-filter=A -- <file>).`

The 23 files: the ten of `crates/groupware/src/calendar/pipeline/`; `crates/groupware/src/calendar/{za_alerts,za_text}.rs`; `crates/vault/src/tokens.rs`; `crates/common/src/za/{tokens,calendar}.rs`; `crates/http/src/auth/za_bearer.rs`; `crates/http/src/auth/oauth/za_token.rs`; `crates/jmap/src/api/za_calendar.rs`; `crates/jmap/src/calendar_event/{za_read.rs,query/za.rs,set/za.rs}`; `crates/jmap/src/calendar/set/za.rs`; `crates/jmap/src/changes/za_calendar.rs`.

The reason, recorded in the outcome note: `crates/dav/src/common/za.rs` is a file of the baseline cohort, and the move takes its lines to `pipeline/admission.rs` and `pipeline/context.rs`. Left out, those lines and the rest of the release's fork code would leave the measured cohort. The gate's figure is read in Task 44; a shortfall there is answered with tests, as release 5 answered its own.

- [ ] **Step 7: Run the gates' checks**

Run: `python3 -I scripts/tests/ci_steps.py` and `python3 -I scripts/tests/test_gates.py`.
Expected: both exit 0.

- [ ] **Step 8: The closing checks**

- [ ] **Step 9: Commit**: `Release 6: the coverage cohort's additions and the key-mode JMAP step's description`

### Task 42: The mutation sample

**Spec:** 9.7.

**Files:**
- Create: none in the repository. Logs `t42-mut-<id>.log` and diffs `t42-mut-<id>.diff` go to `.superpowers/sdd/2026-11-14-zero-access-17-jmap-calendars/mutations/`.
- Test: the catcher each row names

**Interfaces:**
- Consumes: the committed head after Task 41, and every test the table names.
- Produces: for the outcome note, the table below with one more column, "First failure observed" (`file:line`, message, left and right), and the follow-up runs.

**Constraints:**
- One row at a time, against the committed head. The edit is one exact replacement that matches once; the executor writes the replaced text into the row's diff file.
- After each row: `git checkout -- crates/ tests/`, then `git status --short crates/ tests/` prints nothing. Nothing is committed.
- The first failing assertion is recorded whether or not it is the named catcher.
- A row that no test fails is a survivor and a finding for the controller. An equivalent mutant is argued in the outcome note.
- The reviewer of this task reads the logs and diffs and runs nothing.

Commands: a row names ZA, U-vault, U-groupware or U-common with the catcher's name. One more form exists here only:

- **ZA-short**, a follow-up only: ZA with a tests-only edit that removes the calls from `setup::test` (`tests/src/za/mod.rs:140`) through `sharing_races::test` (`:190`), so the suite goes from its setup to `tokens::test`. It is used when ZA's first failure is in a module before `tokens::test`, to reach the named catcher. The edit is reverted with the mutation.

25 of the 27 rows run ZA.

- [ ] **Step 1: Run the rows**

| Id | Fault (9.7) | Edit | Named catcher | Command |
|---|---|---|---|---|
| J1 | the JMAP create skips the lease | `crates/jmap/src/calendar_event/set/za.rs`, create: `za_take_uid_lease` is called with `&[]` as its calendars, so the UID is not checked under the lease. The lease itself cannot be left out: `ZaEventCreate.lease` is not optional (I §2.7). | `events::create_uid_rule`: the second event of the UID in one calendar is created | ZA |
| J1b | the lease check covers one calendar (5.2) | `pipeline/uid.rs`, `za_take_uid_lease`: only `calendar_ids[0]` is checked | `pipeline::create_uid_taken_in_second_calendar` | ZA |
| J2 | the JMAP create skips sealing | `crates/jmap/src/calendar_event/set.rs`: the key branch after `:189` is not taken, so upstream's writer runs for a key account | `events::create_reads_back_over_caldav`: the create is not made, because upstream's batch carries the ordinary fence (`set.rs:249`) | ZA |
| J2f | the same, without the fence | J2's edit, and the fence call at `set.rs:249` removed | `events::create_stores_no_preferences_and_no_index_document`, or an earlier case of `events` that reads the stored record; then `leak::test_jmap_calendars` | ZA |
| J3 | the JMAP create calls `ItipMessages::queue` | `set/za.rs`, create: when `za_schedule_create` returned messages, the same tree is also given to upstream's builder as `set.rs:1902-1919` uses it, and `ItipMessages::new(messages).queue(&mut batch)` is committed before `za_create_event` | `scheduling::no_itip_task_ever` | ZA |
| J4 | an update drops the token pairs | `pipeline/update.rs`, `za_commit_update`: the stored pairs are not carried into the resealed event | `events::update_keeps_pairs_and_display_name`. The function is CalDAV's too, so ZA is expected to fail first in a module of Task 7's set (`reply` or `drain::test_reply`); record it, then ZA-short. | ZA, ZA-short |
| J5 | an update drops the display name | `set/za.rs`, update: `ZaEventUpdate.display_name` is `None` | `events::update_keeps_pairs_and_display_name` | ZA |
| J6 | an update skips the SEQUENCE floor | `set/za.rs`, update: the `za_sequence_floor` call is removed | `events::update_applies_sequence_floor` | ZA |
| J7 | a text filter reads the stored tree | `crates/jmap/src/calendar_event/query.rs`, the expanded loop (`:483-498`): `InstanceTextFilter` is given the stored content for a key account | `query::text_matches_the_view_not_the_tree` | ZA |
| J8 | the unexpanded matcher reads one component at a time | `crates/groupware/src/calendar/za_text.rs`, `ZaEventTextIndex::insert`: the document holds the values of the view's first component only | `text_index_cases` (the words split between master and override); then `query::two_accounts_same_answers` | U-groupware, ZA |
| J9 | a token-record write omits the vault assertion | `crates/common/src/za/tokens.rs`, `token_write_store`: no vault assertion is added when `vault_cas` is `Some` | `record_write_loses_to_vault_write`; then `tokens::exchange_crossed_by_password_change` | U-common, ZA |
| J10 | a superseded refresh generation deletes the grant | `crates/http/src/auth/oauth/za_token.rs`, `za_refresh_keyed`: `Superseded` deletes the grant as `Lost` does | `tokens::retry_of_g_while_g1_completes`, the `TOKEN_REFRESH_READ_PAUSE` run: the grant is gone | ZA |
| J11 | a lost record race puts back a pruned entry | `tokens.rs`, `za_token_refresh`: after an assertion loss under an unchanged binding, the retry writes the first attempt's entries without classifying again | `tokens::retry_of_g_while_g1_completes`, the `TOKEN_REFRESH_WRITE_PAUSE` run: generation `g` is in the record | ZA |
| J12 | the sweep keeps a refresh entry below its grant's stored generation | `tokens.rs`, `za_token_sweep`: each grant's state is given to `reconcile` with `generation: 0` | `tokens::interrupted_refresh_cleared_by_sweep` | ZA |
| J13 | the drain hook is removed | `crates/jmap/src/api/request.rs`: the `za_drain` insertion before `:194` is removed | `scheduling::drain_runs_from_jmap` | ZA |
| J14 | a notification destroy is let through | `crates/jmap/src/api/za_calendar.rs`, `za_calendar_method`: `CalendarEventNotification/set` is `Served` | `refusals::every_row_of_6_11`; then `refusals::notification_destroy_leaves_the_item` | ZA |
| J15 | the wrap's associated data omits the grant id | `crates/vault/src/keys.rs`, `token_aad`: the label is built without `grant_id` | `token_entry_binds_account_grant_kind_generation` | U-vault |
| J16 | the vault commit leaves the token record **[RULE]** | `crates/common/src/auth/vault.rs`, `vault_batch_write`: the `batch.clear(PrincipalField::ZeroAccessTokens)` is removed | `vault_write_clears_the_token_record`; then `tokens::vault_writes_end_sessions` | U-common, ZA |
| J17 | a superseded refresh entry is kept | `crates/vault/src/tokens.rs`, `ZaTokenRecord::reconcile`: the rule that drops a refresh entry below the grant's stored generation is removed | `reconcile_cases`; then `tokens::interrupted_refresh_cleared_by_another_grants_write` | U-vault, ZA |
| J18 | a token with an unopenable secret is served keyless | `crates/http/src/auth/za_bearer.rs`, `za_bearer_keys`: an entry that is missing or does not open returns `Ok(None)` | `tokens::tampered_or_foreign_secret_is_401` | ZA |
| J19 | the hand-off is not deleted on redemption | `tokens.rs`, `za_handoff_take`: the entry is read and not deleted | `tokens::handoff_gone_after_redemption_and_expiry`. Upstream's single-use code refuses a replayed exchange, so the exact-key check is the only catcher. | ZA |
| J20 | the cap's eviction leaves the evicted grant | `tokens.rs`, `za_token_issue`: the evicted grant is not deleted | `tokens::cap_evicts_least_recently_used`; then `tokens::thirty_third_sign_in_ends_the_oldest` | ZA |
| J21 | a refresh without the secret of a keyed grant rotates it | `tokens.rs`, `za_token_refresh_precheck`: a missing secret for a grant with entries returns `Keyless` | `tokens::refresh_without_secret_of_keyed_grant_refused` (it passes from Task 17 on; its line "the refresh with its secret is granted" arrives in Task 18) | ZA |
| J22 | a Bearer request at a stale generation is served | `crates/http/src/auth/authenticate.rs`, the keyed arm (`:262-277`): keys from a token that fail the generation fence are served uncached, as a password login's are | `tokens::bearer_crossed_by_vault_write_is_refused` | ZA |
| J23 | a successful token answer is traced | `crates/http/src/auth/oauth/token.rs`, `token_response` (`:652-668`): `with_untraced_body()` is removed | `tokens::granted_answer_is_untraced` | ZA |
| J25 | the unexpanded matcher's document is built from the stored tree | `crates/jmap/src/calendar_event/query/za.rs`, `za_resolve_text_leaves`: the stored content is inserted into the index in place of the reader's view | `query::text_matches_the_view_not_the_tree` | ZA |
| J26 | a placement change asks for a right CalDAV's `MOVE` does not | `pipeline/placement.rs`, `ZaPlacementPlan::entered_rights`: `ModifyItems` is added | `caldav_plans_keep_todays_rule`; then `jmap_plans` | U-groupware |

There is no J24. D21's "asserts the record alone" has no row: a removal reads no vault record, so the fault cannot be made without adding one. `removal_write_asserts_the_record_alone` and `forget_grant_asserts_the_record_alone` test it.

Task 38's kill check is the table's last row in the outcome note, with the line it recorded.

D28 has no fault in spec 9.7's list. Its tests are `calendar_set_error_table` and `calendars::store_failure_stops_the_call` (Task 28), `event_set_error_table` (Task 30) and the two mail-guard cases (Tasks 31 and 33). D21 has none either: a removal reads no vault record (spec 9.7).

- [ ] **Step 2: Hand the table to the controller**

The ledger gets one line per row: its id, killed or survived, the first failure. The controller rules on each survivor (`Ruling:`): a test is strengthened by the task that owns it and the row is run again, or the mutant is argued equivalent.

### Task 43: Documentation

**Spec:** 7, 9.8, 10 (the amendments), 11, 12.

**Files:**
- Create: `docs/zero-access/client-jmap.md`
- Create: `docs/superpowers/plans/2026-11-14-zero-access-plan17-outcome.md`
- Modify: `CLAUDE.md`, `docs/README-dev.md`, `docs/zero-access/manual-checklist.md`, `docs/zero-access/calendar-sharing-api.md` (`:153-154`), `README.md` (`:29-31`), `CHANGELOG-fork.md` (`:7`)

**Interfaces:**
- Consumes: every task's report (adjusted assertions, kill lines, rulings), Task 41's records, Task 42's table, the two printed lines of `query::text_query_cost` (review focus 5), the fixed texts (I §5.6, §7.5).
- Produces: the text every later session reads.

**Constraints:**
- Documentation only: no code or test file changes, so the closing checks have nothing to check.
- Each passage that states when browser sessions end is written for the plan's rule and listed in the outcome note among the **[RULE]** passages, so that a later decision finds them all.
- Line references in `CLAUDE.md` are checked against the head before they are written.
- No release 6 status paragraph is added to `CLAUDE.md`: the controller adds it at merge.

- [ ] **Step 1: `CLAUDE.md`**

| Place (line at this checkout) | What it must now say |
|---|---|
| Commands, `:18` | Key-mode `jmap_tests` runs upstream's `calendar::{calendars, instance, alarm}` bodies and the variants `calendar_event`, `calendar_identity`, `calendar_notification`, `calendar_acl`, `calendar_availability`, entered through `za_variants::calendars`. "The calendar refusals" goes. |
| Commands, `:21` | Key mode does not run `calendar::notification` because its variant is the refusal of 6.11, not because calendars are refused. |
| Commands, `:28` | The zero-access job's description, if Task 41 added a rerun. |
| HTTP request flow, `:57` | Keys are attached in `authenticate_uncached` from HTTP Basic or from a Bearer token that carries a secret (`za_bearer_keys`). |
| Caches, `:70` | The key cache also holds entries of Bearer tokens, keyed by the fingerprint of the token as presented, suffix included. |
| Test harness, `:72` | Field 153 among the records exempt while their account exists. The five token hooks: `TOKEN_EXCHANGE_PAUSE`, `TOKEN_REFRESH_READ_PAUSE`, `TOKEN_REFRESH_WRITE_PAUSE`, `TOKEN_REFRESH_WRITE_FAIL_ONCE`, `TOKEN_FORGET_FAIL_ONCE`. `COMMIT_PAUSE`, `COMMIT_FAIL_ONCE`, the key `UPDATE_COMMIT_PAUSE`, `UPDATE_COMMIT_FAIL_ONCE`, `DELETE_COMMIT_PAUSE` and `DELETE_COMMIT_FAIL_ONCE` now fire from `groupware`, on CalDAV and JMAP writes; `PROPFIND_ADMITTED_PAUSE` stays in `dav` and does not fire on JMAP. The helpers of `tests/src/utils/oauth.rs` and of `tests/src/za/jmap_calendar/mod.rs`, `Keyless::call_using`, `jscalendar_of`, `normalize_jscal`, `leak::secret_hits`. |
| Where the code lives, after the release 7 bullet (`:107-129`) | A release 6 bullet naming the spec, the plan's files and the outcome note, and every file of the header's "File structure" with its one-line role. |
| Invariants, `:137` | `SessionKeys` are attached by the HTTP layer for one request, from Basic or from a token with a secret; a token without a secret is keyless. |
| Invariants, after the release 7 bullet (`:144-178`) | One bullet, "JMAP calendars and browser sign-in (release 6 spec section 10, 57-76)", restating each invariant in a clause with its number, then the amendments of section 10. |
| Invariant 9 exceptions (`:179-197`) | Two more. `decode_access_token` strips a `~` suffix from every token that begins with the server's token header before it opens it (spec 4.2, D26). A successful answer of the token endpoint is untraced for every account (D24). With them, the three costs section 10 names for ordinary accounts. |
| Where release 2 lives, `:286` | The drain is also called from `handle_jmap_request`, for a request whose token holds its own account's keys and that holds a calendar method. |
| Merge hotspots, after the release 7 paragraphs (`:304-411`) | A paragraph "Release 6 added merge hotspots" with every insertion of spec 11's first list, and one "Release 6 silent dependencies" with every item of its second list. Among them, or added to them: the key writer of `CalendarEvent/set` repeats the head and the update grouping of upstream's handler; `za_calendar_set` repeats the head of `calendar/set.rs`; `za_itip_fails_write` names every `ItipError` variant; the split of a token relies on `TOKEN_HEADER`; D15's paged form relies on the fields of an intermediate state (Task 22); the recipient rule of `za_apply_default_alerts_by_value` repeats `alerts.rs:472-489`; the query branch relies on no key account's event carrying `EVENT_HIDE_ATTENDEES`; the place of each **[RULE]** use of `za_session_binding` (the header's table). |
| The za suite paragraph, `:419` | The modules `tokens` and `jmap_calendar::{pipeline, events, calendars, query, scheduling, sharing, refusals, oracle}` after `sharing_races::test`; `leak::test_jmap_calendars` after `leak::test_imip`; the fixture rule of the header's suite rules, and the `token-*` and `jmapcal-*` allocations, by module, as the tasks made them. |
| Release 4 calendar sharing, `:426-434` | A grantee reads and writes a shared key calendar through JMAP under the rules CalDAV applies; `shareWith` stays refused; a shared calendar's colour is in each principal's sealed bundle, and a new grantee's entry starts as a copy of the owner's (D14). |

- [ ] **Step 2: `docs/README-dev.md`**

| Place | What it must now say |
|---|---|
| The command list, `:14-26` | Key-mode `jmap_tests` runs the calendar block. |
| "Operating zero-access accounts", `:31-37` | Browser sign-in: an OAuth token of the authorization-code flow carries the account's keys; a keyed token is about 45 characters longer; the device-code flow and the mail protocols' bearer logins stay keyless (D9); a token whose entry is gone answers 401, which is not counted for banning (D7). **[RULE]** Every vault write ends every browser session of the account, among them the first pin's rewrap. The token record (field 153) and the daily sweep that bounds an expired, superseded or registry-destroyed entry (D8). At most 32 browser sign-ins of an account hold keys at once (D20). How long a browser stays signed in is upstream's refresh-token lifetime, thirty days without use by default; the release adds no setting. |
| The key-cache bullet, `:36` | The cache also holds Bearer entries, under the same idle and age limits. |
| A new section "## Browser sign-in and JMAP calendars", before "## Merging upstream" (`:186`) | What is served and what stays refused (spec 6.2 and 6.11 as two short lists); that the release ships no client; the pointer to `docs/zero-access/client-jmap.md`; that text search of events is matched in memory per request and what `query::text_query_cost` measured; that a store error on one object of a `/set` leaves the earlier objects committed (D3). |
| "Sharing key calendars", `:287-289` | The sentence about "seven calendar-method refusals" goes. Key-mode JMAP must reach `za_variants::calendars`, which runs upstream's calendar bodies and the variants. |

- [ ] **Step 3: `docs/zero-access/client-jmap.md`**

In the form of `client-caldav.md`. Sections:

- **What this is**: the manual gate of spec 9.8 for the reference client, Bulwark Webmail 1.21.2, static edition. It qualifies the server; the release ships, hosts and configures no client.
- **Setup**: serving the static edition from a local server; the OAuth client and Application registration it needs and the server's CORS setting, as steps on a local machine; the accounts: a key account without a second factor, one with, a second key account as grantee and invitee, an outside address at a local SMTP sink.
- **Recording**: the date, the client's version, the server's commit, and one result per row.
- **Checks**: every row of spec section 7's table, in its order, each with the action and the expected result. The row "after the first invitation to a key account" expects one sign-out and a working second sign-in **[RULE]**.
- **Assumptions to confirm on a running instance**: the five facts of spec 1.3, each with how to observe it.

- [ ] **Step 4: The smaller files**

- `docs/zero-access/manual-checklist.md`: a section "## JMAP calendars and browser sign-in (release 6)" after "## Search and threads (release 7)" (`:98`), naming the spec and `client-jmap.md`. Native status: pending.
- `docs/zero-access/calendar-sharing-api.md:153-154`: "JMAP calendars are served to an owner and to a grantee; `shareWith` stays refused for key accounts." A sentence on D14: each principal's colour of a shared calendar is sealed in its own entry.
- `README.md:29-31`: JMAP calendars leave the list of what is deferred or refused. Granting a share through JMAP joins it.
- `CHANGELOG-fork.md`, "## Unreleased": browser sign-in carries keys; JMAP calendars for key accounts, with what stays refused; a keyed token whose entry is gone is refused (D7); default alerts are applied by value (D11); destroying a calendar through JMAP sends cancellations (D16); at most 32 keyed sign-ins (D20); the token endpoint's successful answers leave the traces for every account (D24); **[RULE]** sessions end on every vault write.

- [ ] **Step 5: The outcome note**

`docs/superpowers/plans/2026-11-14-zero-access-plan17-outcome.md`, in the shape of the plan 15 note:

- **Where things stand**: the branch, its base, every commit by task. Task 44 adds each suite's result and time.
- **What changed and why**: one paragraph per task group, naming the spec sections.
- **The pending decision**: the rule the code implements, and the list of **[RULE]** places in code, tests and documentation.
- **Rulings**: every `Ruling:` line of the ledger.
- **Review focus**: the result of each of the five tests; for the fifth, the count of opened events and the time.
- **Mutation runs (plan 17)**: Task 42's table with the observed lines, the follow-up runs, the survivors or "No mutation survived", the kill checks, and why D21 has no row.
- **Other harnesses**: Task 41's records, the rerun decision, the cohort additions with their reason.
- **Adjusted assertions**: every existing assertion a task changed, with its file, line and spec section: `gating.rs`, `websocket.rs`, `za_variants::calendars` and `principal_get`, `sharing.rs`'s colour case, `app_password.rs`'s comment, and Task 39's tables.
- **Accepted residuals**:
  - spec 3.3 in substance, and 3.4's four limitations, of which only the round trip has a test;
  - 4.5's interrupted refresh; D22's residual; D7 at a cache hit; section 2's at-rest and client statements;
  - the three costs on ordinary accounts (spec section 10);
  - a retried refresh's next entry can outlive its token by the retry's delay (4.3);
  - a grantee's JMAP update stamps `updated`, so most JMAP moves ask `ModifyItems` as well (5.2);
  - coverage that stays partial: the refresh steps of 4.5 have a unit test for the classification only and run otherwise in the za suite; the pass-through of a foreign Bearer value has a unit test only.
- **Deferred findings**, by task.
- **Merge hotspots**: spec 11 and Step 1's additions.
- **Open questions for the project owner**: decision 4; whatever is still open of D15 on a paged sync, the memory of an unranged text query and the `/set` answer after D11; and whatever execution found.

- [ ] **Step 6: Commit**: `Plan 17 outcome note, the reference client's checklist, CLAUDE.md and the release 6 documentation`

### Task 44: The final test matrix and the whole-branch review

**Implementer:** opus. **Reviewer:** the whole-branch review of Step 3, at the top tier.

**Spec:** the release's qualification: 3.6, 9, 10.

**Files:**
- Modify: `docs/superpowers/plans/2026-11-14-zero-access-plan17-outcome.md` (the matrix's results; the review's findings and rulings)
- Modify: whatever an adjudicated fix names

**Interfaces:**
- Consumes: the head after Task 43, rebased as "Base and rebasing" says.
- Produces: the record on which the project owner decides the pull request.

**Constraints:**
- Each row is recorded with its command, result and wall time.
- A rerun the header's suite rules allow is taken once. Any other failure stops the matrix and is a finding.
- The whole-branch review runs once, at the top tier, as the project owner authorized on 2026-11-14. It reads only. A second pass needs the project owner's word.
- Nothing is pushed and no workflow is dispatched without the project owner's word.

- [ ] **Step 1: Run the matrix on the head**

In this order:

1. `cargo fmt --all --check`.
2. `python3 -I scripts/tests/ci_steps.py`, then `python3 -I scripts/tests/test_gates.py`.
3. SHIP and TM.
4. U-vault and U-groupware, with no name.
5. U-ws, with no name.
6. `cargo test -p smtp --lib --bins --features test_mode`.
7. U-http, with no name.
8. `cargo test -p tests za::leak::tests::try_archive_short_values_are_not_archives -- --exact --nocapture`.
9. ZA.
10. ZA-FS.
11. SWAP.
12. WP, then WK.
13. IP, then IK.
14. JP, then JK.
15. `SPAM_RULES_URL=https://github.com/stalwartlabs/spam-filter/releases/download/v3.0.4/spam-filter-rules.json.gz cargo test -p tests smtp -- --nocapture`, the six known flakes repeated by name.
16. The upstream suites: `'webdav::cal_query::'`, `store::store_tests`, `store::blob::blob_tests`, `store::lookup::lookup_tests`, `store::search_tests`, `telemetry::telemetry_tests`, SYS. `automation::automation_tests` and `directory` run if Docker is up; otherwise they are recorded as left to CI.
17. `python3 -B -m unittest discover -s scripts/release -p 'test_*.py' -v`; the product with the full profile, `cargo build --release --locked -p stalwart --no-default-features --features rocks`, with no other cargo command beside it; the verifier as `.github/workflows/test.yml:462-468` runs it, without `--inspect-elf` on a host that builds no ELF.
18. FRESH with its failure tests, and CALDAV in both modes, as Task 41 Step 5 ran them, on row 17's binary.

Rows 15 and 16 carry `STORE=RocksDb RUST_MIN_STACK=16777216`.
Expected: every row passes, or passes on its one allowed rerun. The product build prints no warning in a file of the fork.

- [ ] **Step 2: Record the matrix**

The outcome note's "Where things stand" gets one line per row: command, result, time, and the head it ran on. The move's proof is rows 4, 9, 10, the key half of 12 and the key half of 18.

- [ ] **Step 3: Dispatch the whole-branch review**

One reviewer at the top tier, with a clean context and this brief:

- **Read**: the spec, revision 5; the plan's header, the interface sheet and the six task files; `CLAUDE.md`; the outcome note (rulings, adjusted assertions, mutation table, residuals); and `git diff <base>...HEAD`, where the base is the branch's merge base with `origin/release7-search`, or with `origin/main` once pull request 84 has merged.
- **Run nothing.** The matrix of Step 1 ran on this head; its results are in the outcome note.
- **Look for**:
  1. a place where JMAP and CalDAV differ for a key account in anything spec 3.2 does not list as an input, and a JMAP write that stores plaintext CalDAV would have sealed (3.6);
  2. each invariant 57 to 76 against the code that carries it (74 to 76 are revision 5's: D26, D28 and D29), with an input that would break it;
  3. the refresh steps of 4.5 against `rotate_oauth_grant`, and every write of the token record against the two assertions of 4.3 and D21;
  4. each function moved in group A against the lines it replaced, and every hook, trace string and unsettled mail guard that had to move unchanged;
  5. every fork insertion in an upstream file: more than one call or one short branch, a copied upstream check, and a path an ordinary account now takes differently beyond what section 10 records: the strip of a `~` suffix from a token that begins with the server's token header, the untraced successful answer of the token endpoint, and the three costs it names;
  6. a `/set` call for a key account in which a store error fails the method, changes an earlier object's answer or lets a later object run (5.3, D3);
  7. a trace, log or error that can carry a token, a secret, a code, a tag, event content or an address;
  8. a test that would pass on wrong behaviour, an assertion changed without its spec section, a retained test function that no longer runs in its step;
  9. a use of the session rule that does not go through `za_session_binding`.
- **Report every finding**, each with its file and line, what happens, the input or interleaving that shows it, a confidence (high, medium or low) and a severity (critical, important or minor). Do not filter, merge or rank findings away, and do not fix anything.
- **Not findings**: what spec 3.3 and 3.4 record as accepted, and the residuals the outcome note lists. Inherited upstream behaviour is reported as such, in one line.

- [ ] **Step 4: Adjudicate and fix**

The controller rules on every finding (`Ruling:` in the ledger): fixed, recorded as a residual, deferred, or rejected with its reason. A fix goes to an Opus implementer with a brief of its own and is reviewed by a separate Opus agent.

- [ ] **Step 5: Run again what the fixes touch**

After the last fix: rows 1 to 3 always; every unit row whose crate changed; rows 9 and 10 for any change under `crates/` or `tests/src/za/`; the row of each suite whose protocol or test files changed; row 17 for any change under `crates/`. Expected: each passes. The outcome note records the second results beside the first.

- [ ] **Step 6: Commit**: `Plan 17: the final test matrix, the whole-branch review and its rulings`

The controller then puts to the project owner, for authorization: pushing the branch; dispatching CI's `gates` job on it (the inventory gate and the coverage gate with Task 41's cohort additions); the reference client's checklist of `docs/zero-access/client-jmap.md`; and the pull request. A gate that fails is answered with tests or a precise disposition, never with a change to the baseline.
