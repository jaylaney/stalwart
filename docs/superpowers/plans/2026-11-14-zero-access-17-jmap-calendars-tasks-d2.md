# Plan 17, group D2: Tasks 28 to 37

These tasks belong to [zero-access plan 17: JMAP calendars and browser sign-in](2026-11-14-zero-access-17-jmap-calendars.md), whose header holds the commands, the constraints and the task index. "I §n" is section n of the [interface sheet](2026-11-14-zero-access-17-jmap-calendars-interfaces.md).

The JMAP writes, the drain hook, the session and principal objects, and two test modules. "I §n" is a section of the interface sheet; command labels (ZA, JK, U-ws, U-groupware, the closing checks) and "Moving a refusal row" are the header's. In the order lists, "U" is a step of upstream's handler at the line range given, "F" a step of the fork, "P" a call into the pipeline.

### Task 28: `Calendar/set`

**Spec:** 6.7 (D14, D16), 5.3 (D3, D28), 6.11, 8; 6.2; invariant 75.

**Files:**
- Create: `crates/jmap/src/calendar/set/za.rs`, with its `#[cfg(test)] mod tests`.
- Modify: `crates/jmap/src/calendar/set.rs`: a `mod za;` line after the `use` block (`:60`), and one branch after `:80` (`let account_id = request.account_id.document_id();`).
- Modify: `crates/jmap/src/api/za_calendar.rs`: the `Calendar/set` row (header, "Moving a refusal row"), and the items under Produces.
- Test: `tests/src/za/jmap_calendar/calendars.rs` (Task 21's file); `tests/src/za/gating.rs:366-369` and `tests/src/jmap/za_variants.rs:3521-3524` (the row).

**Interfaces:**
- Consumes: `ZaScheduling`, `ZaDeleted` (I §2.1); `ZaPipelineError`, `ZaPipelineError::from_calendar` (I §2.2); `za_writer_keys`, `za_roles`, `ZaRoles` (I §2.3); `za_mutation_context` (I §2.4); `za_delete_send_itip` (I §2.6); `za_calendar_delete_target`, `ZaCalendarDeleteTarget`, `za_delete_calendar` (I §2.10); `za_calendar_keyless_refusal` (I §6); `za_calendar_method` (I §7.1).
- Consumes, unchanged `groupware` primitives: `seal_calendar`, `calendar_view_in_domain`, `seal_calendar_changes` (`crates/groupware/src/calendar/seal/collection.rs:90`, `:374`, `:394`), `seal_error` (`seal/tree.rs:95`).
- Consumes, private items of upstream's `calendar/set.rs`, named through `super::`: `update_calendar` (`:991-1217`), `CalendarChanges` (`:879`), `RequestDefaultAlerts` (`:886`), `DefaultAlertValidator::assert_unique_default_alerts` (`:907`), `CalendarServerSet::server_set_values` (`:1297`).
- Produces: `za_calendar_set`, `za_calendar_set_error` (I §7.3); `ZA_CALENDAR_SET_INCOMPLETE`, `za_set_incomplete` and the `shareWith` text (I §7.5).

**Constraints:**
- The branch classifies with `Server::za_is_key_account`, the cached classification admission uses, so `tests/src/za/mail/fence.rs:3502-3533` (a vault planted behind the cached ordinary classification) keeps its 503.
- Invariant 57: for a key account the handler never calls `DestroyArchive::delete_with_cleanup`, `DestroyArchive::remove_all`, `CalendarEvent::update_meta`, `reap_calendar_notifications`, `notify_calendar_removal`, `notify_direct_change` or `ItipMessages::queue`. A calendar and its events are deleted only by `za_delete_calendar`.
- `Calendar::insert` and `Calendar::update` are called only on a value that `seal_calendar` or `seal_calendar_changes` returned, as MKCOL and PROPPATCH call them.
- Invariant 58: sort order, the subscription and visibility flags, default-alert offsets and an IANA time zone id are stored in the clear; name, description, colour and a custom time zone are sealed.
- 6.7: the handler never adds a preference entry. `update_calendar` adds one through `preferences_mut` and `subscribe_member`, so a caller with no stored entry is refused before it runs.
- 6.7: a grantee changes only its own entry, and only at the `edit` level. The context asks `Modify`, as PROPPATCH does; a grantee at `view` is refused. `seal_calendar_changes` keeps every other entry byte for byte.
- 6.11: `shareWith`, as a property or as a patch pointer, is refused before `update_calendar` sees the object.
- D3: each object is its own commit; `newState` is the change id of the last commit, or the old state when nothing commits.
- D16: a destroy schedules with `ZaScheduling::On`: cancellations and declines go through the fan-out, and a denied status never fails the destroy.
- Object metadata is refused where it is extracted (upgrade ruling 66), before the object's other checks.
- `reschedule_calendar_alarms` and the owner's `inherit_owner_preferences` and `sync_owner_preferences` stay in the update path, as PROPPATCH's key arm has them; they change nothing for a key account today.

**Store failures (I §7.5, spec 5.3 D28).** Each object's body returns `trc::Result<Result<answer, SetError>>`. An `Err` of the body, and the `Err` of `za_calendar_set_error`, is a store failure: the handler traces it once (`trc::error!`, with the account id and, when known, the document id), answers the object with `za_set_incomplete()`, and answers every later object of the call (the remaining creates, then updates, then destroys) with the same set error without attempting it. Objects already answered keep their answers, `newState` is the last commit's change id, and the method returns `Ok`. An error in the head fails the method: nothing was committed. A store error of the default write after the objects is traced; the default stays as it was and the method still answers.

**Order of the key handler.** U lines are at `crates/jmap/src/calendar/set.rs`.

Head, once per call:
1. U `:81-89`: `ObjectMetadata::new`, `viewer_change_id`, `fetch_groupware_resources(.., SyncCollection::Calendar)`.
2. U `:90-91`: `SetResponse::from_request(..)?.with_state(sampled.assert_state(cache.get_state(true), &request.if_in_state)?)`.
3. U `:92-95`, `:99`: `MetadataWriter::new`, `will_destroy`, `is_shared`, `personal_id`, `RequestDefaultAlerts::default()`.
4. U `:101-109`: the calendar-count quota.
5. F: `za_writer_keys(server, access_token, account_id)`. `Ok(Some(keys))` goes on. `Ok(None)` or `Err(_)` returns `Err(za_calendar_keyless_refusal().account_id(account_id))`, the method error of 6.1. When `will_destroy` is not empty, `za_roles(server, access_token, account_id)`.
6. Not taken from upstream: the batch and its provisioning fence (`:112-114`), `PendingCreates`, `reject_uncommitted`.

Create, per object, in request order:
1. U `:116-125`: no room in the quota: `overQuota`, upstream's text. The count of calendars created so far in this call replaces `created_slots.len()`.
2. U `:127-134`: `is_shared`: `forbidden`, "Cannot create calendars in a shared account."
3. F: the object names `shareWith`: `forbidden`, the `shareWith` text.
4. U `:136-148`: the new `Calendar` (random ten-character path name, the owner's entry).
5. U `:151-158`: `metadata_writer.extract(MetadataPatches::for_create(), &mut object)`. `Err(e)`: `e`. `Ok(Some(_))`: `forbidden` with `crate::api::metadata::ZA_METADATA_REFUSED`.
6. U `:159-177`: `client_properties`, the subscribed flag, `update_calendar(None, object, &mut calendar, access_token, account_id)`, `sync_owner_preferences`.
7. U `:178-192`: `assert_unique_default_alerts`.
8. U `:233-234`: `server_set_values(personal_id, &client_properties)` and `request_alerts.accept(None, ..)`, both before sealing: they read the plaintext entry.
9. F: `seal_calendar(&mut calendar, &keys, account_id)`, its error through `seal_error(err, account_id, u32::MAX)` (MKCOL's form, `crates/dav/src/calendar/mkcol.rs:175-178`): a store failure.
10. F: a batch of its own: `reserve_document_id(account_id, Collection::Calendar)`, U `:245-252` `calendar.insert(access_token.changed_by(), account_id, slot, &mut batch)`, `commit_batch`. The id is `assigned_ids.slot(slot)`.
11. U `:254-262`, `:840-846`: `response.created(id, document_id)`; the server-set properties are added after the last object, with `isDefault` ("Default" below). `newState` is `assigned_ids.change_id(account_id, SyncCollection::Calendar)`.

Update, per object:
1. U `:274-285`: an invalid id is `notFound`; an id in `will_destroy` is `willDestroy`.
2. U `:286-293`: metadata, as create step 5 (`for_update`).
3. U `:296-316`: the stored archive; missing is `notFound`.
4. U `:318-322`: a shared caller without `Read` or `ReadItems` in the stored ACL: `notFound`.
5. F: the object names `shareWith`: `forbidden`, the `shareWith` text.
6. F: `za_mutation_context(server, access_token, account_id, document_id, Bitmap::new().with_item(Acl::Modify))`, PROPPATCH's context (`crates/dav/src/calendar/proppatch.rs:172-184`). `Err(e)`: `za_calendar_set_error(ZaPipelineError::from_calendar(e, account_id, document_id))`. `Ok(None)`: `forbidden`.
7. F: the update's current value is `context.stored`, not the archive of step 3. Its deserialised `Calendar` is `stored`.
8. F: `stored.personal_preferences(context.personal_id)` is `None`: `forbidden`, "You are not allowed to modify this calendar." (upstream's text, `:379`).
9. F: the edited value starts from `calendar_view_in_domain(&context.stored, &context.domain, context.personal_id)`, deserialised; its error through `seal_error(err, account_id, document_id)`.
10. U `:351-369`: `inherit_owner_preferences` (when not shared), `update_calendar(Some(id), object, &mut new_calendar, access_token, account_id)`, `sync_owner_preferences` (when not shared).
11. U `:383-397`: `assert_unique_default_alerts(.., Some(document_id), ..)`.
12. F, in PROPPATCH's order (`proppatch.rs:322-366`): a new batch; `context.assert_in(&mut batch)`; `seal_calendar_changes(&stored, &new_calendar, &context.domain, context.personal_id)`, its error through `seal_error`; U `:445-454` `reschedule_calendar_alarms` with the stored and the sealed settings; U `:455` `request_alerts.accept(Some(document_id), ..)`; U `:467-475` `update(access_token.changed_by(), <context.stored unarchived>, account_id, document_id, &mut batch)`; `commit_batch`.
13. An assertion failure of that commit is the set error `forbidden` with "Another process modified this calendar, please try again." (`:808`). There is no retry, as PROPPATCH has none. Any other error is a store failure.
14. U `:476`: `response.updated.append(id, None)`; `newState` as in create.

Destroy, per id of `will_destroy`:
1. U `:506-527`: not in the cache, or no archive: `notFound`.
2. U `:530-543`: a shared caller without `Acl::Delete` in the stored ACL: `forbidden`, "You are not allowed to delete this calendar."
3. U `:546-577`: the children's privacy classes; `calendarHasEvent` unless `onDestroyRemoveEvents`; a shared caller with restricted children: `forbidden`.
4. P: `za_calendar_delete_target(server, access_token, account_id, document_id, <the archive>)`.
5. P: `za_delete_send_itip(server, access_token, &roles.owner, ZaScheduling::On)`.
6. U `:578`, `:582-584`, `:623-639`: `children = cache.children_ids(document_id)`; `href` as upstream forms `delete_path`; `cleanup = preload_container_cleanup(access_token.changed_by(), account_id, Collection::CalendarEvent, <the children that have metadata kinds>)`.
7. P: `za_delete_calendar(server, access_token, &cache, &keys, &roles, account_id, document_id, target, children, &cleanup, href, send_itip)`.
8. U `:600`: `response.destroyed.push(id)`; `newState = State::Exact(deleted.change_id)`.

A grantee's destroy that passes step 2 is refused by the pipeline (`Store` with a `Security` error) and answers `forbidden`.

Default, after the objects (U `:749-798`, `:816-867`): upstream's rule decides the id (`onSuccessSetIsDefault` as an id that is visible and not destroyed, or as a reference to a calendar this call created; only for an owner; only when the three `not_*` maps are empty; only when it differs from the stored id or the lowest calendar id). The key handler writes it in a batch of its own, `PrincipalField::DefaultCalendarId` on document 0 of `Collection::Principal`, an id and nothing else, and adds upstream's `isDefault` server-set properties. Each created calendar's server-set gets `isDefault` computed as `:816-846` computes it, from the stored id read after the commits. The clear of a destroyed default is `za_delete_calendar`'s, not the handler's.

**Error mapping** (spec 8, for `Calendar/set`; the pipeline rows are `za_calendar_set_error`):

| Condition | Answer |
|---|---|
| a caller with no access to the account | upstream's method error `accountNotFound`, from the method's own access check |
| a keyless token, an impersonation, a caller the access check admits without keys of its own | method error `forbidden`, `ZA_CALENDAR_KEYLESS` (admission, Task 20; and head step 5) |
| `shareWith` named, as a property or a pointer whose first item is `shareWith` | set error `forbidden`, "Calendar sharing is managed on the account page." |
| `metadata` or `privateMetadata` named | set error `forbidden`, `ZA_METADATA_REFUSED` |
| the caller has no stored entry | set error `forbidden`, "You are not allowed to modify this calendar." |
| `Forbidden(_)`, `LinkedEvent`, `Store(e)` with `Security(Unauthorized)` | set error `forbidden`, no description |
| `NotFound` | set error `notFound` |
| `Busy(_)`; `Store(e)` with `e.is_assertion_failure()` | set error `forbidden`, "Another process modified this calendar, please try again." |
| `Store(e)` with `Limit(Quota)` or `Limit(TenantQuota)` | `overQuota` |
| `UidConflict`, `TooManyRecipients`, `Scheduling(_)`, `SchedulingDenied(_)` | not produced on these paths; set error `forbidden`, no description, so the match names every variant |
| `Store(e)`, anything else | `Err(e)`: a store failure; the object and every later one answer `forbidden`, "The server could not complete this change, try again." |
| every upstream check of the order lists | upstream's answer and text |

**Tests.**

`jmap` unit, in `calendar/set/za.rs`:
- `calendar_set_error_table`: the pipeline rows above, row by row, each answer compared as JSON. `Forbidden(Context)`, `Forbidden(SharedCopy)`, `LinkedEvent` and `Store(Security(Unauthorized))` give `{ "type": "forbidden" }`. `NotFound` gives type `notFound`. `Busy(LeaseBusy)`, `Busy(CreateLost)` and `Store(AssertValueFailed)` give `forbidden` with "Another process modified this calendar, please try again." `Store(Limit(Quota))` and `Store(Limit(TenantQuota))` give `overQuota`. `UidConflict { href: None }`, `TooManyRecipients`, `Scheduling(_)` for one `ItipError` and `SchedulingDenied("x")` give `{ "type": "forbidden" }`. `Store(StoreEvent::UnexpectedError)` gives `Err`.

za cases in `calendars.rs`, on the module's `Pair`. `stored(id)` is a module-private read this task adds: the `Calendar` at `ValueKey::archive(key_id, Collection::Calendar, id)`, deserialised. A calendar a case shares is made with `make_calendar` and granted with `share`; its JMAP id is the `Calendar/get` entry whose `name` is the slug.
- `create_seals_name_description_colour`. Create `{ "name": "Name canary", "description": "Description canary", "color": "#ff8800", "sortOrder": 7, "timeZone": "Europe/Vienna", "isVisible": false }`.
  - `Calendar/get` returns those six values.
  - Stored: one preferences entry; its `account_id` is `key_id`, its `name` starts with `$za$`, its `description` and `color` are `None`, its `sort_order` is 7; `acls` is empty.
  - `format!("{stored:?}")` holds none of "Name canary", "Description canary", "#ff8800".
  - A PROPFIND of `D:displayname` on the collection returns "Name canary".
- `update_own_entry`. Update `{ "name": "Renamed canary", "description": null, "color": "#112233", "sortOrder": 9 }`: `updated`.
  - `Calendar/get` returns "Renamed canary", a null description, "#112233", 9.
  - `newState` differs from the state before and equals the state of that `Calendar/get`.
  - Stored: still one sealed entry, `sort_order` 9, neither "Renamed canary" nor "#112233" in the archive.
- `grantee_updates_only_its_entry` (6.7). "jmapcal-own-entry", shared with key2 at `edit`. key2 updates `{ "name": "Reader name canary", "sortOrder": 3 }` in key's account: `updated`.
  - The owner's stored entry equals its value before the update, byte for byte.
  - Stored: two entries, no third; both names start with `$za$`.
  - key2 reads "Reader name canary" in key's account; key still reads "jmapcal-own-entry".
  - Regranted at `view`: the same update is `notUpdated`, `forbidden`.
- `update_without_entry_refused`. A calendar whose stored `preferences` is empty, planted and removed as `tests/src/za/setup.rs:605-660` plants and removes one (the two helpers copied, module-private). The owner's update `{ "name": "x" }` is `notUpdated`, `forbidden`, "You are not allowed to modify this calendar."; the stored `preferences` is still empty.
- `colour_before_and_after_sharing` (D14). "jmapcal-colour": the owner sets "#00aa55", then shares it with key2 at `edit`.
  1. The owner still reads "#00aa55".
  2. key2, which has set nothing, reads "#00aa55" and the name "jmapcal-colour" on the shared calendar.
  3. The owner sets "#112233": `updated`, and reads it back.
  4. key2 sets "#abcdef": `updated`; key2 reads "#abcdef"; the owner still reads "#112233".
  5. Stored: every entry's `color` is `None` and its `name` starts with `$za$`.
- `destroy_with_events_sends_cancellations` (D16). "jmapcal-destroy-jmap" and "jmapcal-destroy-dav", each with one event PUT over CalDAV (`uid_lease::put`, body `fanout::invite(uid, start, &[key2's address, "jmapcal-calendars-out@remote.test"])`), uids `jmapcal-cal-destroy-j` and `jmapcal-cal-destroy-d`. Then `wait_for_queue`, `sink().clear()`, `before = inbox_ids(key2_id)`.
  - `Calendar/set` destroy of the first with `onDestroyRemoveEvents: true`: `destroyed`. CalDAV `DELETE` of the second collection: 204.
  - After `wait_for_queue`: key2 holds exactly one new item per uid, each `METHOD:CANCEL` (`open_pending_item_as`, `item_method`).
  - The sink holds exactly one message per uid to the outside address, each with `calendar_method() == "CANCEL"`.
  - `uid_copies(key_id, "jmapcal-cal-destroy-j") == 0`.
- `destroy_non_empty_without_flag_refused`. A destroy without the flag is `notDestroyed` with type `calendarHasEvent`; `uid_copies` of its event is 1; key2 has no new item; the sink is empty after `wait_for_queue`.
- `grantee_destroy_refused`. "jmapcal-grantee-destroy", shared with key2 at `edit`. key2's destroy with `onDestroyRemoveEvents: true` is `notDestroyed`, `forbidden`; the calendar is still listed for both accounts.
- `on_success_set_is_default`.
  - By reference: create `{ "name": "jmapcal-default" }` with `onSuccessSetIsDefault: "#i0"`: `created(0)["isDefault"]` is true; in `Calendar/get` exactly one calendar has `isDefault` true, the created one.
  - By id: `onSuccessSetIsDefault` with the former default's id restores it.
  - With a failing object in the same call the default is unchanged.
  - The stored value at `PrincipalField::DefaultCalendarId` is the id.
- `share_with_refused` (6.11). `grant` is `{ <key2's id>: { "mayReadItems": true } }`.
  - Create `{ "name": "jmapcal-share", "shareWith": grant }`: `notCreated`, `forbidden`, "Calendar sharing is managed on the account page."
  - Update `{ "shareWith": grant }`: `notUpdated`, the same error.
  - Update `{ "shareWith/<key2's id>": { "mayReadItems": true } }`: `notUpdated`, the same error.
  - The stored `acls` is empty.
- `per_object_commits` (D3).
  - Two creates, `{ "name": "jmapcal-first" }` and `{ "color": "#000000" }` (no name): `created(0)`; `notCreated(1)` is `invalidProperties`; the first is listed; `newState` equals the state of a `Calendar/get` made next.
  - A call whose every object fails returns the old state as `newState`.
  - One call with a create, a failing update and a destroy: the create and the destroy are applied.
- `store_failure_stops_the_call` (5.3). Two empty calendars X and Y exist. `test_hooks::set_delete_commit_fail_once(Some(key_id))`, then one call: create `{ "name": "jmapcal-fail-new" }`, destroy `[X, Y]`. The hook is cleared after the call.
  - The answer is not a method error.
  - `created(0)` is present and the calendar is listed.
  - `notDestroyed[X]` and `notDestroyed[Y]` are `forbidden`, "The server could not complete this change, try again."
  - X and Y are still listed: Y was not attempted.
  - `newState` equals the state of a `Calendar/get` made next (the create's commit).

**Steps:**
- [ ] 1. Write `calendar_set_error_table` and the twelve za cases, each called from `test()` after Task 21's.
- [ ] 2. Run U-ws for `calendar_set_error_table`. Expected: a compile error, `cannot find function za_calendar_set_error`.
- [ ] 3. Run ZA. Expected: FAIL in `jmap_calendar::calendars` at `create_seals_name_description_colour`: no `created` entry, the method still answers `accountNotSupportedByMethod`.
- [ ] 4. Add `ZA_CALENDAR_SET_INCOMPLETE`, `za_set_incomplete` and the `shareWith` text to `api/za_calendar.rs` (I §7.5).
- [ ] 5. Move the `Calendar/set` row (header, "Moving a refusal row"). In `za_variants::calendars` the served block creates `{ "name": "Test Calendar" }`, asserts `created`, destroys it with `Calendar/set` and asserts `destroyed`, so the closing `assert_is_empty` holds.
- [ ] 6. Add `mod za;` and the branch: when `self.za_is_key_account(account_id).await?` is true, return `za::za_calendar_set(self, request, access_token, using).await`.
- [ ] 7. Implement `za_calendar_set_error` as the table says, and `za_calendar_set` in the order above with the store-failure rule. `shareWith` is found on the object's keys: `Key::Property(CalendarProperty::ShareWith)`, or `Key::Property(CalendarProperty::Pointer(p))` whose first item is that property.
- [ ] 8. Run U-ws for `calendar_set_error_table`, then for `calendar_method_classes`. Expected: each passes.
- [ ] 9. Run ZA. Expected: pass. The twelve `calendars` cases prove the task; `gating::test` and `mail::fence` pass with no change beyond step 5.
- [ ] 10. Run JK. Expected: pass.
- [ ] 11. The closing checks.
- [ ] 12. Commit: `Release 6: Calendar/set for key accounts`.

Guards for Task 42: `destroy_with_events_sends_cancellations` (D16), `grantee_updates_only_its_entry` (6.7), `store_failure_stops_the_call` (5.3).

---

### Task 29: default alerts by value

**Spec:** 6.5 (D11).

**Files:**
- Create: `crates/groupware/src/calendar/za_alerts.rs`, with its `#[cfg(test)] mod tests`.
- Modify: `crates/groupware/src/calendar/mod.rs`: one line, `pub mod za_alerts;`.

**Interfaces:**
- Consumes, from `crates/groupware/src/calendar/alerts.rs`, unchanged: `DefaultAlertsResolver::resolve` (`:441`), `DefaultAlerts::merge` (`:54`, `pub(crate)`), `DefaultAlerts::has_email_alerts` and `indexed_alerts` (`:85`, `:97`), `DefaultAlarmText` (`:44`), `DefaultAlert::to_ical` (`:118`), `CalendarSettings: From<&Calendar>` (`:243`), `ICalendarShowWithoutTime::shows_without_time` (`:668`); from calcard, `ICalendar::remove_component_ids` (`crates/calcard/src/icalendar/builder.rs:19`).
- Produces: `za_apply_default_alerts_by_value` (I §3.1). Its `writer_id` is the caller's `access_token.personal_id(owner_id, Collection::Calendar)`.
- Produces, private to the file, called by the unit tests:

```rust
/// Removes every VALARM of `ical` and gives each VEVENT and VTODO one alarm per alert of `alerts`.
/// `false`: the removal was refused and `ical` is unchanged.
fn za_replace_alarms(ical: &mut ICalendar, alerts: &DefaultAlerts, recipient: Option<&str>) -> bool;
```

**Constraints:**
- D11: the event's former alarm components are removed from the tree with `ICalendar::remove_component_ids`, not unlinked: no component of the result is an orphan. Upstream's `apply_default_alerts` unlinks, which is why this is a function of its own.
- D11: each alarm is `DefaultAlert::to_ical(Some(text))`. `text.description` is the first text value of the component's own SUMMARY when it is not empty, otherwise "Reminder". `text.recipient` is the writer's account name; only an email alert carries it.
- D11: the defaults are those `DefaultAlertsResolver::resolve` merges with `use_default_alerts` true: the writer's own entry of each calendar, or the owner's when the writer is a member of the owner's account and has none. A grantee is not a member; its entry is a copy of the owner's, made at the grant.
- D11: with no default alert for the event's kind, every former alarm is still removed and none is added.
- D10, invariant 59: nothing records that the defaults were used. The function writes into the plaintext tree and nowhere else, before the pipeline seals it.
- `alerts.rs` is upstream's and is not edited. The recipient rule repeats `DefaultAlertsResolver::recipient` (`alerts.rs:472-489`, private): the account's name when it is not empty.

**Tests** (`groupware` unit, in `za_alerts.rs`). `OWNER` is 4, `WRITER` is 9. `settings(entries)` builds a `Calendar` whose preference entries carry the given default alerts and converts it to `CalendarSettings`; the alerts under test come from `DefaultAlerts::merge([&settings].into_iter(), personal_id, inherit_from, !ical.shows_without_time())`.
- `timed_and_all_day_defaults`. The owner's entry holds `t1` (`-PT15M`, `ALERT_WITH_TIME`) and `d1` (`PT9H`, no flag).
  - An event with `DTSTART:20310304T100000Z` and `SUMMARY:Title canary` gets exactly one VALARM, with `JSID:t1`, `TRIGGER:-PT15M`, `ACTION:DISPLAY`, `DESCRIPTION:Title canary`.
  - The same event with `DTSTART;VALUE=DATE:20310304` gets exactly one VALARM, with `JSID:d1` and `TRIGGER:PT9H`.
- `former_alarms_removed_not_orphaned`. A master and one override, each with two alarms of its own, one of them holding "old alarm canary"; defaults: `t1`.
  - The result holds two VALARM components, one per VEVENT.
  - Each VALARM's index is in exactly one component's `component_ids`.
  - The serialised tree does not hold "old alarm canary".
- `writer_entry_then_owner_entry`. One timed event; the JSIDs of the result for `personal_id = WRITER`:
  - owner entry `o1` and writer entry `w1`, `inherit_from = Some(OWNER)`: `["w1"]`;
  - owner entry `o1` only, `inherit_from = Some(OWNER)`: `["o1"]`;
  - owner entry `o1` only, `inherit_from = None`: none.
- `email_alert_names_the_writer`. The writer's entry holds `e1` (`-PT30M`, `ALERT_WITH_TIME | ALERT_EMAIL`); the event's SUMMARY is "Mail canary".
  - With recipient `Some("writer@example.com")`: `ACTION:EMAIL`, `ATTENDEE:mailto:writer@example.com`, `SUMMARY:Mail canary`, `DESCRIPTION:Mail canary`.
  - With recipient `None`: no `ATTENDEE` line.
- `no_defaults_leaves_no_alarm`. An entry with no default alert; an event with one alarm of its own.
  - The result holds no `VALARM`.
  - With one default alert and an event without SUMMARY, and with an empty SUMMARY: `DESCRIPTION:Reminder`.

Every case asserts that `za_replace_alarms` returned `true`.

**Steps:**
- [ ] 1. Write the five unit tests.
- [ ] 2. Run U-groupware for `za_alerts`. Expected: a compile error, `cannot find function za_replace_alarms`.
- [ ] 3. Declare the module in `calendar/mod.rs`.
- [ ] 4. Implement `za_replace_alarms`: collect the index of every `VAlarm` component, remove them with one `remove_component_ids` call (it renumbers the rest), then for each VEVENT and VTODO push one `to_ical(Some(text))` component per alert and append its index to the parent's `component_ids`.
- [ ] 5. Implement `za_apply_default_alerts_by_value`: `resolver.resolve(server, owner_id, writer_id, true, calendar_ids.iter().copied(), !ical.shows_without_time())`; when the result has an email alert, the recipient is the name of `server.account(writer_id)` if it is not empty; then `za_replace_alarms`. A `false` from it returns `trc::StoreEvent::UnexpectedError.into_err().details("zero-access: default alerts not applied").account_id(owner_id)`.
- [ ] 6. Run U-groupware for `za_alerts`. Expected: 5 passed.
- [ ] 7. The closing checks.
- [ ] 8. Commit: `Release 6: default alerts applied by value`.

---

### Task 30: `CalendarEvent/set` create

**The interim rule (I §7.3).** `CalendarEvent/set` becomes `Served` in this task, which implements creates only. Until Task 31 (updates), Task 32 (a change of `calendarIds`) and Task 33 (destroys) land, the key writer answers those objects with the set error `forbidden` and `ZA_INTERIM`, "This operation is not available for this account yet.", and never lets them reach upstream's path. No interim arm survives Task 33.

**Spec:** 3.1 (D1, D17), 5.3 (D3, D28), 5.4 (D4, D18), 6.5 (the property rulings, D10, D11), 8; 6.2.

**Files:**
- Create: `crates/jmap/src/calendar_event/set/za.rs`, with its `#[cfg(test)] mod tests`.
- Modify: `crates/jmap/src/calendar_event/set.rs`: a `mod za;` line after the `use` block (`:106`); one branch after `:189` (`let account_id = request.account_id.document_id();`); the wording of the fork's three comments at `:1060-1061`, `:1527-1528` and `:1886-1887`.
- Modify: `crates/jmap/src/api/za_calendar.rs`: the `CalendarEvent/set` row (header, "Moving a refusal row"); the three ruling texts of I §7.5.
- Test: `tests/src/za/jmap_calendar/events.rs` (Task 21's file); `tests/src/za/gating.rs:376-379` and `tests/src/jmap/za_variants.rs:3527-3536` (the row).

**Interfaces:**
- Consumes: `ZaScheduling`, `ZaWritten` (I §2.1); `ZaPipelineError` (I §2.2); `za_writer_keys`, `za_roles`, `ZaRoles` (I §2.3); `za_write_contexts` (I §2.4); `za_take_uid_lease` (I §2.5); `za_schedule_create`, `ZaScheduled` (I §2.6); `ZaEventCreate`, `za_create_event` (I §2.7); `za_apply_default_alerts_by_value` (I §3.1); `za_calendar_keyless_refusal` (I §6); `za_set_incomplete` (I §7.5); `ZaUidLeaseGuard::release` (`crates/common/src/za/lease.rs:283`).
- Consumes, private items of upstream's `calendar_event/set.rs`, named through `super::`: `update_calendar_event` (`:2199`), `PatchTarget` and `EventWrite` (`:156-168`), `DEFAULTED_FLAGS` (`:173`), `generate_uid` (`:3395`), `stamp_created`, `stamp_updated`, `clamp_created` (`:2073-2125`), `assert_participants_limit` (`:2127`), `too_many_events`, `too_many_calendars`, `too_large_event`, `add_calendar_forbidden` (`:2159-2185`). From `calendar_event/mod.rs`: `duplicate_uid` (`:206-210`, private; visible to a descendant module), `is_origin`, `EventValue`, `JSCalendarEntries`.
- Produces: `za_calendar_event_set`, `ZaRulings`, `za_take_rulings`, `za_event_set_error`, `ZA_INTERIM` (I §7.3); the texts for `isDraft`, the three participant flags and a `blobId` link (I §7.5).

**Constraints:**
- The branch classifies with `Server::za_is_key_account`, so `tests/src/za/mail/fence.rs:3502-3533` keeps its answer.
- Invariant 57: for a key account no path of this file calls `create_calendar_event`, `CalendarEvent::insert`, `CalendarEvent::update_full`, `DestroyArchive::remove_all`, `notify_direct_change` or `ItipMessages::queue`. `ItipSendStatus::resolve` is not called here either: the pipeline resolves the status (D18).
- Invariant 68 (D17): upstream's checks are called, not copied; the writer adds no check of its own beyond `za_take_rulings`.
- Invariant 59, D10: no `preferences_mut` call on an event may be reached. Upstream's patcher makes one for `useDefaultAlerts` (`:2279-2285`), so `za_take_rulings` runs before `update_calendar_event` and takes that property out. The per-user split (`:1764-1779`) is not run; `content.preferences` stays empty.
- Invariant 58: the four event flags stay 0; `useDefaultAlerts` is applied by value (D11) and reads back false.
- The `/set` answer is left as upstream forms it: `client_values` and `omitted_flags` are computed from the object as the client sent it, and nothing is added for D11. A client that sent `useDefaultAlerts: true` sees the alerts at its next `CalendarEvent/get`.
- D4, D18: `sendSchedulingMessages` true maps to `ZaScheduling::Required`, false or absent to `ZaScheduling::Off`. Mapping false to `On` is the fault the oracle's kill check names.
- 3.3: upstream's `noSupportedScheduleMethods` refusal (`:1893-1900`) is not run; the pipeline records the status. Upstream's `UidIndex` (`:253-265`, `:1818-1832`) is not used: UID uniqueness is CalDAV's rule, checked under the lease.
- The lease is taken after every check that can refuse the object and before the event data is built. On every path that does not hand it to `za_create_event`, a store failure included, the writer calls `lease.release().await` before it answers, so no lease outlives the object.
- Object metadata is refused where it is extracted (upgrade ruling 66).
- The UID in upstream's "An event with UID .. already exists." is the client's own value and is visible by structure.

**Store failures (I §7.5, spec 5.3 D28).** As in Task 28: each object's body returns `trc::Result<Result<answer, SetError>>`; an `Err` of the body or of `za_event_set_error` is traced once, answers the object and every later object of the call (creates, then updates, then destroys) with `za_set_incomplete()` without attempting the later ones, keeps the earlier answers and `newState`, and returns `Ok`. An error in the head fails the method.

**Order of the key writer.** U lines are at `crates/jmap/src/calendar_event/set.rs`.

Head, once per call:
1. U `:190-198`: `ObjectMetadata::new`, `viewer_change_id`, `fetch_groupware_resources(.., SyncCollection::Calendar)`.
2. U `:199-202`: `account_info(account_id)` (the target's).
3. U `:203-204`: `SetResponse::from_request(..)?.with_state(sampled.assert_state(cache.get_state(false), &request.if_in_state)?)`.
4. U `:205-219`: `will_destroy`, `is_owner`, `identities`, `UidPrivacyConflicts::new`.
5. U `:221-234`: `can_add_calendars`, `can_delete_calendars`.
6. U `:236-244`: the object-count quota.
7. U `:250`: `send_scheduling_messages`.
8. U `:276-285`: `DefaultAlertsResolver::with_resources(account_id, cache.clone())`, the disk quota budget, `MetadataWriter::new(metadata, account_id)`.
9. F: `za_writer_keys(server, access_token, account_id)`: `Ok(Some(keys))` goes on; anything else returns `Err(za_calendar_keyless_refusal().account_id(account_id))`. Then `za_roles(..)` and `server.account(account_id)` (the `AccountCache` the pipeline takes); their errors fail the method.
10. Not taken from upstream: the batch and its provisioning fence (`:246-249`), `PendingCreates`, `UidIndex`, `EventSetContext`, `reject_uncommitted`.

Create, per object, in request order:
1. U `:288-291`: no room in the quota: `too_many_events()`. The count of events created so far in this call replaces `created_slots.len()`.
2. U `:292-295`: `uid_privacy_conflict()`.
3. U `:296-305`: `metadata.extract(MetadataPatches::for_create(), &mut object)`. `Err(e)`: `e`. `Ok(Some(_))`: `forbidden` with `crate::api::metadata::ZA_METADATA_REFUSED`.
4. U `:1651-1661`: `client_values`, `has_version`, `omitted_flags`, from the object as the client sent it.
5. F: `za_take_rulings(&mut object, true)`.
6. U `:1662-1665`: `CalendarEvent::default()`, `CalendarEventContent::default()`, `JSCalendar::new_event()`.
7. U `:1679-1693`: `update_calendar_event(cache, personal_id, PatchTarget { write: EventWrite::Create, id: None, is_origin: None }, object, &mut event, &mut content, &mut group)`.
8. U `:1694-1698`: the calendar-count limit.
9. U `:1700-1721`: every `calendarIds` entry exists ("calendarId {id} does not exist."); `can_add_calendars` (`add_calendar_forbidden`).
10. U `:1722`: `TrackedValues::from_dates`.
11. U `:1724-1731`: `export_icalendar`.
12. U `:1732-1734`: `assert_privacy_allowed(&ical, EventViewer::new(is_member))`.
13. U `:1736-1744`: a generated UID when the object has none.
14. F (in place of `:1746-1759`): when the ruling's `use_default_alerts` is true, `za_apply_default_alerts_by_value(server, &mut resolver, account_id, personal_id, &calendar_ids, &mut ical)`.
15. U `:1761-1763`: `ical.validate_user_data()`. The split at `:1764-1779` is not run.
16. U `:1781-1785`: `itip_assign_organizer` with the target's first address.
17. U `:1786-1794`: the ownership form of the add check.
18. U `:1805-1810`: `stamp_created`; when the event is the account's own, `stamp_updated` and `clamp_created`.
19. U `:1812-1816`: `assert_participants_limit`.
20. U `:1834-1841`: the size cap.
21. U `:1955-1958`: the disk quota pre-check: `overQuota`.
22. P: `za_take_uid_lease(server, &cache, access_token, account_id, &calendar_ids, uid)`.
23. F (in place of `:1843-1861`): `content.data = CalendarEventData::new_for(ical, Tz::Floating, max_ical_instances, &DefaultAlerts::disabled(), true)`.
24. P: `za_write_contexts(server, access_token, account_id, &event, event.names[0].parent_id, Acl::AddItems)`.
25. U `:1862-1876`: `next_event_alarms` over every named calendar.
26. P: `za_schedule_create(server, access_token, &roles, schedule, &mut event, &mut content, account_id)`, `schedule` being `Required` when `send_scheduling_messages`, else `Off`.
27. P: `za_create_event(server, access_token, &cache, &account, &keys, &roles, ZaEventCreate { .. })`.
28. U `:2021`: `disk_quota.reserve(size)`; the created count goes up by one.
29. U `:1960-2003`: upstream's server-set, computed from `written.ical` and `written.document_id` (no `PENDING_DOCUMENT_ID`, no `resolve_document_id`): `dates.with_component(written.ical.main_component()).with_origin(is_origin(&written.ical, account_info.addresses())).server_set(&client_values, None)`, the version, the defaulted flags the client omitted (all false), `ImportedValues { participants: is_organizer_assigned, .. }`.
30. U `:1618-1626`: `response.created(id, written.document_id)`, `response.add_created_properties(&id, server_set)`, `response.new_state = State::Exact(written.change_id).into()`.

Steps 23 to 26 run with the lease held. A `SetError` from steps 1 to 21 goes to `not_created` unchanged; a `ZaPipelineError` from steps 22 to 27 goes through `za_event_set_error(error, Some(uid))`.

Interim arms (this task only): every update of the request is answered `not_updated` and every destroy `not_destroyed`, each with `forbidden` and `ZA_INTERIM`. Upstream's update and destroy loops are never reached for a key account.

**Property rulings** (`za_take_rulings`):

| In the object | `base` | Result |
|---|---|---|
| `isDraft: true` | true | `invalidProperties`, property `isDraft`, "Drafts are not available for zero-access accounts." |
| `mayInviteSelf`, `mayInviteOthers` or `hideAttendees` set true | true | `invalidProperties` naming the property, "This property is not available for zero-access accounts." |
| those four set false or null | true | left in the object; upstream clears the flag |
| `useDefaultAlerts` true, false or null | true | taken out of the object; `use_default_alerts` is whether it was true |
| `useDefaultAlerts` with any other value | true | left in the object; upstream answers "Invalid value." |
| any of the five | false | untouched; upstream's instance validator refuses them with its own text |
| any link with a `blobId`, including one embedded in the event being updated | both | `invalidProperties`, property `links`, "Event attachments are not available for zero-access accounts." |

Blob ids are found as upstream finds them: `BlobProperties::from_patch(object, |_| true)` is not empty.

**Error mapping** (spec 8; the pipeline rows are `za_event_set_error`):

| Condition | JMAP answer |
|---|---|
| a caller with no access to the account | upstream's method error `accountNotFound`, from the method's own access check |
| a keyless token, an impersonation, a caller the access check admits without keys of its own | method error `forbidden`, `ZA_CALENDAR_KEYLESS` (admission, Task 20; and head step 9) |
| upstream's own check answers first (calendar ACL of the cache, privacy, limits, conversion) | upstream's set error and description |
| a `calendarIds` entry that does not exist | upstream's `invalidProperties` on `calendarIds`, "calendarId {id} does not exist." |
| a property ruling of 6.5 | `invalidProperties` naming the property (table above) |
| `metadata` or `privateMetadata` named | set error `forbidden`, `ZA_METADATA_REFUSED` |
| `Forbidden(_)`, `LinkedEvent` | set error `forbidden`, no description |
| `NotFound` | set error `notFound` |
| `UidConflict { .. }` | `duplicate_uid(uid)`: `invalidProperties` on `uid`, "An event with UID {uid} already exists."; with `uid` `None`, the same error without a description |
| `SchedulingDenied(reason)` | set error `forbidden`, `reason` (one of "Calendar scheduling is disabled on this server.", "This account has no calendar address to send scheduling messages from.", "This account is not allowed to send calendar scheduling messages.") |
| `TooManyRecipients` | `invalidProperties` on `participants`, "The number of scheduling message recipients exceeds the maximum allowed." |
| `Scheduling(err)` | `invalidProperties` on `participants`, `err.to_string()` |
| `Busy(_)`; `Store(e)` with `e.is_assertion_failure()` | set error `forbidden`, "Another process modified this calendar event, please try again." |
| `Store(e)` with `Limit(Quota)` or `Limit(TenantQuota)` | `overQuota` |
| `Store(e)` with `Security(Unauthorized)` | set error `forbidden`, no description |
| `Store(e)`, anything else | `Err(e)`, which the writer turns into the store-failure answer: `forbidden`, "The server could not complete this change, try again.", for the object and every later one |

An object that fails does not undo the objects before it.

**Tests.**

`jmap` unit, in `calendar_event/set/za.rs`:
- `rulings_cases`.
  - For each of `isDraft`, `mayInviteSelf`, `mayInviteOthers`, `hideAttendees`: `true` with `base` true is `invalidProperties` whose `properties` is that one name; `false` and `null` pass, stay in the object, and give `use_default_alerts == false`; `true` with `base` false passes.
  - `useDefaultAlerts` as `true`, `false`, `null` with `base` true: taken out of the object; `use_default_alerts` is `true`, `false`, `false`.
  - `useDefaultAlerts` as the string "yes" with `base` true: passes and stays in the object.
  - `useDefaultAlerts: true` with `base` false: passes and stays in the object.
  - A `links` entry whose `blobId` is an upload: `invalidProperties` on `links`, with `base` true and false.
  - A `blobId` embedded in `(1, 2)`: refused with `base` true and false, including when updating event 2 in account 1.
- `event_set_error_table`, each answer compared as JSON.
  - `Forbidden(Context)`, `Forbidden(LinkedMove)`, `LinkedEvent`: `{ "type": "forbidden" }`.
  - `NotFound`: type `notFound`.
  - `Busy(LeaseBusy)`, `Busy(CreateLost)`, `Store(AssertValueFailed)`: `forbidden`, "Another process modified this calendar event, please try again."
  - `UidConflict { href: None }` with uid `u1`: `invalidProperties`, properties `["uid"]`, "An event with UID u1 already exists."
  - `TooManyRecipients`: `invalidProperties`, properties `["participants"]`, "The number of scheduling message recipients exceeds the maximum allowed."
  - `Scheduling(err)` for one `ItipError`: `invalidProperties`, properties `["participants"]`, `err.to_string()`.
  - `SchedulingDenied("This account is not allowed to send calendar scheduling messages.")`: `forbidden` with that text.
  - `Store(Limit(Quota))`, `Store(Limit(TenantQuota))`: type `overQuota`.
  - `Store(Security(Unauthorized))`: `{ "type": "forbidden" }`.
  - `Store(StoreEvent::UnexpectedError)`: `Err(e)`, which the writer turns into the store-failure answer.

za cases in `events.rs`, on the module's `Pair`. `cal` is the default calendar's id (the `Calendar/get` entry with `isDefault` true). `event(uid)` and `event_in(uid, calendar_id)` are module-private builders this task adds: `{ "@type": "Event", "uid": uid, "title": uid, "start": "2031-03-04T10:00:00", "duration": "PT1H", "timeZone": "Etc/UTC", "calendarIds": { <cal, or calendar_id>: true } }`. The body of an event over CalDAV is `drain::body(&dav, &copy_href(test, key_id, uid))`. These cases delete their events over CalDAV: the JMAP destroy arrives in Task 33.
- `create_reads_back_over_caldav`. Create `event("jmapcal-events-create")` with `"title": "Create canary"`, `"description": "Body canary"`.
  - `created(0)["useDefaultAlerts"]` is false (the client omitted it).
  - The CalDAV body holds `UID:jmapcal-events-create`, `SUMMARY:Create canary`, `DESCRIPTION:Body canary`, `DTSTART`.
  - The stored tree (`stored_event(..).tree()`, serialised) holds neither "Create canary" nor "Body canary".
  - `CalendarEvent/get` returns the title, `isDraft` false, `useDefaultAlerts` false.
- `create_in_two_calendars`. `second` is a calendar made with `Calendar/set`. Create `jmapcal-events-two` with `calendarIds: { cal: true, second: true }`.
  - `CalendarEvent/get` returns exactly those two `calendarIds`.
  - A PROPFIND of each of the two collections lists one resource, and each GET holds `UID:jmapcal-events-two`.
- `create_property_rulings` (6.5).
  - `isDraft: true`: `notCreated`, `invalidProperties`, properties `["isDraft"]`, "Drafts are not available for zero-access accounts."; `uid_copies` is 0.
  - `mayInviteSelf`, `mayInviteOthers`, `hideAttendees` set true, one at a time: `invalidProperties` naming the property, "This property is not available for zero-access accounts."; `uid_copies` is 0.
  - Each of the four as `false`: `created`, and `CalendarEvent/get` returns false.
  - `links: { "l1": { "@type": "Link", "blobId": <an upload of the account, `key.jmap_client().await.upload(None, b"x".to_vec(), None)`>, "rel": "enclosure" } }`: `invalidProperties`, properties `["links"]`, "Event attachments are not available for zero-access accounts."
  - `metadata` named: `forbidden`, "Object metadata is not available for zero-access accounts."
- `create_use_default_alerts` (D11). The default calendar gets, with `Calendar/set`, `defaultAlertsWithTime { "t1": display, start, "-PT15M" }` and `defaultAlertsWithoutTime { "d1": display, start, "PT9H" }`.
  - Create `jmapcal-events-alerts` with `"title": "Alert canary"`, `"useDefaultAlerts": true`: the `created` answer carries neither `alerts` nor `useDefaultAlerts` (upstream's answer; the client sent the flag).
  - `CalendarEvent/get`: `useDefaultAlerts` false; `alerts` has one entry, `t1`, whose trigger offset is "-PT15M".
  - The CalDAV body has one `BEGIN:VALARM`, `TRIGGER:-PT15M`, `DESCRIPTION:Alert canary`.
  - An all-day event (`"showWithoutTime": true`) gets `d1` and `TRIGGER:PT9H`.
  - An object that sends its own `alerts` and `useDefaultAlerts: true` stores the defaults only.
  - After the calendar's defaults change, the first event's alerts are unchanged.
  - With an email default alert the VALARM holds `ATTENDEE:mailto:jmapcal-events@example.com`.
  - Cleanup: the events are deleted (no email alarm task stays queued) and the calendar's defaults set back to null.
- `create_uid_rule` (3.3). `jmapcal-events-uid` exists in `cal`.
  - The same UID created in `second`: `created`.
  - The same UID created in `cal` again: `notCreated`, `invalidProperties`, properties `["uid"]`, "An event with UID jmapcal-events-uid already exists."
  - `za_uid_leases(test)` is 0.
- `create_two_objects_one_fails` (D3).
  - `event("jmapcal-events-ok")` and `event("jmapcal-events-bad")` with `isDraft: true`: `created(0)`; `notCreated(1)` is `invalidProperties`; the first is returned by `CalendarEvent/get`, whose state equals `newState`.
  - In the other order the second is still created.
  - A call whose only object fails returns the old state as `newState`.
- `create_stores_no_preferences_and_no_index_document`. Create with `alerts` (`a1`, display, "-PT5M"), `keywords: { "kw": true }`, `color: "#aabbcc"`.
  - `assert_no_event_preferences(test, key_id, document_id)`.
  - After `test.wait_for_tasks_skip_not_due()` (the event's own alarm task is due in 2031): `za_has_calendar_search_documents(key_id)` is false and `itip_tasks_for(test, key_id)` is 0.
- `tasks`. Create `{ "@type": "Task", "uid": "jmapcal-events-task", "title": "Task canary", "due": "2031-03-06T10:00:00", "calendarIds": { cal: true } }`: `CalendarEvent/get` returns `@type` "Task"; the CalDAV body holds `BEGIN:VTODO`.
- `create_holds_the_lease`. Interleaving: a create of `jmapcal-events-lease` is parked at `COMMIT_PAUSE` (`HookParked::start(key_id, test_hooks::set_commit_pause, ..)`); while it is parked a second create of the same UID in the same calendar runs; then the first is released.
  - The second is `notCreated`, `forbidden`, "Another process modified this calendar event, please try again."
  - The first is `created`; `uid_copies` is 1; `za_uid_leases(test)` is 0.

**Steps:**
- [ ] 1. Write the two unit tests.
- [ ] 2. Run U-ws for `rulings_cases`. Expected: a compile error, `cannot find function za_take_rulings`.
- [ ] 3. Write the nine za cases, each called from `test()` after Task 21's.
- [ ] 4. Run ZA. Expected: FAIL in `jmap_calendar::events` at `create_reads_back_over_caldav`: no `created` entry.
- [ ] 5. Add the three ruling texts to `api/za_calendar.rs` (I §7.5) and move the `CalendarEvent/set` row (header, "Moving a refusal row"). In `za_variants::calendars` the row's `calendarIds` key `"a"` is a well-formed id, so the served block names an id no calendar has (`Id::from(999_999u32)`) and asserts that `/methodResponses/0/1/notCreated/e` is `invalidProperties` naming `calendarIds`; nothing is stored.
- [ ] 6. Add `mod za;` and the branch: when `self.za_is_key_account(account_id).await?` is true, return `za::za_calendar_event_set(self, request, access_token, using).await`.
- [ ] 7. Reword the three comments. At `:1060-1061`, `:1527-1528` and `:1886-1887` the fork's comment names `za_assert_calendar_allowed`, which Task 20 removed. Each becomes: `// A key account leaves this handler at its head (release 6 spec 5.4): the owner here is plain and upstream's Send applies.` The `false` argument stays.
- [ ] 8. Implement `za_take_rulings` and `za_event_set_error` as the two tables say. The match of `za_event_set_error` names every variant of `ZaPipelineError`.
- [ ] 9. Implement `za_calendar_event_set`: the head, the create loop in the order above with the store-failure rule, and the two interim arms.
- [ ] 10. Run U-ws for `rulings_cases`, `event_set_error_table` and `calendar_method_classes`. Expected: each passes.
- [ ] 11. Run ZA. Expected: pass. The nine `events` cases prove the task.
- [ ] 12. Run JK. Expected: pass.
- [ ] 13. The closing checks.
- [ ] 14. Commit: `Release 6: CalendarEvent/set create for key accounts`.

Catchers for Task 42 (spec 9.7): the create skips the lease: `create_holds_the_lease` and `create_uid_rule`; the create skips sealing: `create_reads_back_over_caldav`.

---

### Task 31: `CalendarEvent/set` update and scheduling

**Spec:** 3.1 (D1, D17), 5.3, 5.4 (D4, D18), 6.5, 8; 4.7 (review focus 1).

**Files:**
- Modify: `crates/jmap/src/calendar_event/set/za.rs`: the update grouping and the update loop replace the interim update arm.
- Create: `tests/src/za/jmap_calendar/scheduling.rs`.
- Modify: `tests/src/za/jmap_calendar/mod.rs`: `pub mod scheduling;`. `tests/src/za/mod.rs`: `jmap_calendar::scheduling::test(&mut test).await;` after the call of `jmap_calendar::query::test` and before that of `jmap_calendar::sharing::test`.
- Test: `tests/src/za/jmap_calendar/events.rs`.

**Interfaces:**
- Consumes: `ZaEventReader` (I §7.2); `za_write_contexts` (I §2.4); `za_schedule_update`, `ZaScheduled` (I §2.6); `ZaEventUpdate`, `za_commit_update` (I §2.8); `ZaPlacementContexts::unchanged` (I §2.9); `za_sequence_floor` (`crates/groupware/src/calendar/sequence_floor.rs:107`); `ZaEventView` (`crates/groupware/src/calendar/seal/event.rs:72`); Task 30's `za_take_rulings`, `za_event_set_error`, `ZA_INTERIM`; Task 29's `za_apply_default_alerts_by_value`.
- Consumes, private items of upstream's `calendar_event/set.rs`: `EventUpdates`, `EventUpdate` and its methods (`:2559-3121`), `EventOp`, `InstanceOp`, `InstanceAction`, `InstancePlan`, `InstanceMetadata`, `instance_metadata`, `instance_imported_values`, `ComponentSequences::has_same_sequences`.
- Consumes, test helpers (I §10): `pair`, `key_account`, `make_calendar`, `share`, `za_oauth_client`, `za_sign_in`, `za_bearer_jmap`, `za_refresh`, `za_revoke_token`, `za_drop_oauth_client`, `CALENDAR_JMAP_USING`.
- Produces: the update path of `za_calendar_event_set`. No new name.

**Constraints:**
- Invariant 57: no path calls `CalendarEvent::update_full`, `notify_direct_change` or `ItipMessages::queue` for a key account. The update is written only by `za_commit_update`.
- Invariant 68 (D17): upstream's update checks are called in upstream's order; the grouping at `:328-425` is repeated, not rewritten.
- The patch applies to the unsealed view; the stored archives are what `ZaEventUpdate` carries as `stored_event` and `stored_content`.
- Invariant 59, D10: the per-user split (`:820-887`) is not run for any caller. A grantee's `alerts`, `keywords` or `color` are written to the event itself, or refused by upstream's `EventChanges::assert_allowed`.
- 6.5: the five flag rulings apply to the event's own patch only (`base` true). A patch of one occurrence is given to `za_take_rulings` with `base` false, which checks blob links only; upstream's instance validator refuses the five properties with "This property cannot be modified on a single occurrence."
- 5.4: with scheduling on or off, the update carries the stored token pairs and the stored display name (the pipeline does both; the writer passes `display_name` from the view) and applies the SEQUENCE floor right after upstream's increment, on every update.
- D4, D18: `sendSchedulingMessages` true and a change that is not of user data only is `ZaScheduling::Required`; anything else is `Off`. The three `resolve(.., false)` sites of upstream stay unreached.
- 3.3: upstream's `noSupportedScheduleMethods` refusal (`:1067-1083`) is not run.
- Upstream's user-data-only rule keeps its `is_owner` condition, so a grantee's edit that touches no scheduling property stamps `updated` where the owner's does not. Task 43 records it as an accepted residual.
- The `/set` answer is left as upstream forms it: nothing is added when the ruling took `useDefaultAlerts`.
- A change of the event's calendars is Task 32's: in this task it is answered with the interim set error and never written.
- The store-failure rule of Task 30 applies to each event of `updates.pending`.

**Order of the update path.** U lines are at `crates/jmap/src/calendar_event/set.rs`.

Head: Task 30's, and one more step: F: `ZaEventReader::new(server, access_token, account_id)`; `None` cannot occur behind the branch, and it or an error fails the method.

Grouping, once per call, after the creates:
1. U `:328-346`: `has_synthetic_ids`, `is_destroyed_event`, `will_be_destroyed`.
2. U `:347-401`: each update id: invalid is `notFound`; to be destroyed is `willDestroy`; `metadata.extract(MetadataPatches::for_update(), ..)`: `Err(e)` is `e`, `Ok(Some(_))` is `forbidden` with `ZA_METADATA_REFUSED` (in place of the preload at `:367-381`); then `EventOp::Base` or `EventOp::Instance` into `EventUpdates::push`.
3. U `:402-420`: instance destroys pushed as `InstanceAction::Destroy`.
4. U `:422-425`: `destroy_events`. Until Task 33 every id in it, and every `InstancePlan::DestroyEvent`, gets the interim destroy answer.
5. Not taken: `:421` (metadata preload), `:426-442` (`moved_uid_index`).

Per event of `updates.pending`:
1. U `:450-466`: the stored row; missing is `notFound`.
2. U `:467-473`: `assert_privacy_access`.
3. U `:474-477`: the UID privacy conflict.
4. U `:513-525`: the stored content; missing is `notFound`. (`:478-512`, the metadata-only write, is not taken.)
5. F: `reader.view(server, access_token, document_id, &stored_calendar_ids, calendar_event.inner, &content_)`. `None` is `notFound`. Keep the view's content and its `display_name`.
6. U `:529-534`: `new_calendar_event` from the stored row; `new_content` from the view's content, not the stored one.
7. U `:536-552`: `plan_instances(&new_content.data, &mut response)`.
8. U `:554-556`: `personal_id`, `ownership`, `was_origin`.
9. U `:568-581`, owner's form for every caller: the tree is cloned, `previous_event` is taken out of `new_content`, `detach_iana_timezones`, `into_jscalendar`. (`:557-567` and `:576-579` are not taken: there are no preferences.)
10. F: `za_take_rulings` on the base patch (`base` true), and on each instance update's patch (`base` false). The base patch's ruling is kept.
11. U `:582-598`: `tracked_before`, `track_instance_values`. `blob_properties` is empty.
12. U `:599-609`, `:613-619`: `is_restricted`, `per_user_changes`, `before_group`. (`:610-612`: no hidden participants; the flag is never set.)
13. U `:621-634`: `apply_instances`, with `use_default_alerts: false` in `InstanceMetadata`.
14. U `:636-655`: `update_calendar_event(.., PatchTarget { write: EventWrite::Update, id: update.base_id(), is_origin: Some(was_origin) }, update.take_base_patch(), ..)`.
15. U `:657-664`: the calendar-count limit.
16. U `:666-708`: added calendars exist and may be added to; removed calendars may be removed from.
17. U `:709-716`: `is_calendar_ids_changed`. When true: the interim set error (Task 32 replaces this line).
18. U `:732-746`: `is_metadata_changed`, `changes` (`EventChanges::between`, or the per-user patch's). When the ruling's `use_default_alerts` is true, `changes.insert_per_user()` (upstream's `:758-760`).
19. U `:761-767`: the tracked dates.
20. U `:769-788`: `export_icalendar`.
21. U `:789-796`: `changes.classify_recurrence_sets`, `assert_privacy_allowed`.
22. F (in place of `:797-810`): when the ruling's `use_default_alerts` is true, `za_apply_default_alerts_by_value(server, &mut resolver, account_id, personal_id, &new_calendar_ids, &mut new_content.data.event)`.
23. U `:811-818`: `validate_user_data_changes(&previous_event)`.
24. `:820-887` is not taken. `is_user_data_only` starts false.
25. U `:889-909`: organizer assignment, `has_scheduling_changes`, and upstream's rule that sets `is_user_data_only` (its `is_owner &&` condition unchanged).
26. U `:910-920`: `increment_sequence` and `stamp_updated`, under upstream's conditions.
27. F: `za_sequence_floor(&mut new_content.data.event, &previous_event, roles.sender.addresses())`, always.
28. U `:922-931`: `assert_participants_limit` with `max_ical_attendees_per_instance`.
29. U `:933-953`: the UID is unchanged.
30. U `:955-977`: when `is_restricted`, `changes.assert_allowed(..)`; its refusal is `forbidden` with `err.description()`.
31. U `:979-990`: the size and its cap.
32. U `:992-1011`: the previous alarms, computed from the view's content and the stored flags.
33. F (in place of `:1013-1031`): `new_content.data = CalendarEventData::new_for(<the tree>, Tz::Floating, max_ical_instances, &DefaultAlerts::disabled(), true)`.
34. U `:1032-1047`: the next alarms, over the names after the patch.
35. U `:1181-1187`: the disk quota pre-check: `overQuota`.
36. P: `za_write_contexts(server, access_token, account_id, <the stored row, deserialised>, new_calendar_event.names[0].parent_id, Acl::ModifyItems)`, wrapped with `ZaPlacementContexts::unchanged`.
37. P: `za_schedule_update(server, access_token, &roles, schedule, old, &mut new_calendar_event, &mut new_content, account_id, document_id)`. `schedule` is `Required` when `send_scheduling_messages && !is_user_data_only`, else `Off`. `old` is `Some(&previous_event)` unless `is_user_data_only`, then `None`.
38. P: `za_commit_update(server, access_token, &cache, &account, &keys, &roles, ZaEventUpdate { stored_event: &calendar_event_, stored_content: &content_, placement: None, .. })`.
39. U `:1290`: `disk_quota.reserve(extra_bytes)`.
40. U `:1375-1408`, `:1189-1255`: `update.succeed(&mut response)`; the base server-set from `written.ical` (`dates.with_component(written.ical.main_component()).with_origin(is_origin_event).server_set(&client, Some(&before))`, `ImportedValues { participants: is_organizer_assigned, .. }`); each instance's server-set from `written.ical.instance_component(recurrence_id)`; `response.new_state = State::Exact(written.change_id).into()`.

A `SetError` goes to `update.fail(&mut response, err)`; a `ZaPipelineError` from steps 36 to 38 goes through `za_event_set_error(error, None)` first.

**Tests.**

za cases in `events.rs`:
- `update_keeps_pairs_and_display_name`. `jmapcal-events-pairs`: PUT over CalDAV by key with key2 as attendee (`uid_lease::put`, `fanout::invite`); PROPPATCH `D:displayname` "Display canary" on the event. Update `{ "description": "Edited canary" }`, no scheduling flag: `updated`.
  - `fanout::stored_pairs` of key's copy is `Some` before the update and equal after it.
  - A PROPFIND of `D:displayname` on the event still returns "Display canary".
  - The CalDAV body holds `DESCRIPTION:Edited canary`; key2 got no new item.
- `update_applies_sequence_floor`. `jmapcal-events-floor`: PUT over CalDAV with `SEQUENCE:5`, organizer key, one outside attendee (the sink is cleared afterwards). `floor = fanout::stored_sequence(..)`, at least 5. Update `{ "sequence": 1, "description": "floor" }`: `updated`; `fanout::stored_sequence(..)` is `floor`, not 1.
- `update_recurrence_by_synthetic_id`. `jmapcal-events-rec`: a daily event, five occurrences, created over JMAP. `second` is the id `CalendarEvent/query` with `expandRecurrences` returns for the second occurrence. Update `second` with `{ "title": "Override canary" }`: `updated`; the CalDAV body has two `BEGIN:VEVENT`, a `RECURRENCE-ID` and `SUMMARY:Override canary`.
- `update_recurrence_by_override`. The third occurrence through the base id: `recurrenceOverrides/<its local start>` set to `{ "title": "Third canary" }`: `updated`; the body has three `BEGIN:VEVENT` and `SUMMARY:Third canary`.
- `update_occurrence_flags_get_upstreams_refusal`. `second` patched with `{ "isDraft": true }`, with `{ "mayInviteSelf": true }` and with `{ "useDefaultAlerts": true }`: each `notUpdated` with the description "This property cannot be modified on a single occurrence."
- `update_by_restricted_grantee_refused`. In the module's calendar "jmapcal-shared", which its setup shares with key2 at `view`: key2 updates `{ "title": "Not allowed" }` on an event of key: `notUpdated`, `forbidden`, "You are not allowed to modify this event."; the CalDAV body is unchanged.
- `update_uid_change_refused`. Update `{ "uid": "jmapcal-events-other" }`: `notUpdated`, `invalidProperties`, properties `["uid"]`, "You cannot change the UID of a calendar event."
- `update_legacy_event_seals_it`. `plant_event(test, key_id, "default", "jmapcal-legacy.ics", .., None)` (unsealed). Update `{ "title": "Sealed canary" }`: the stored tree, serialised, does not hold "Sealed canary"; the CalDAV body holds `SUMMARY:Sealed canary`.

za cases in `scheduling.rs`. Accounts: `pair(test, "scheduling")` and `key_account(test, "scheduling-noperm", <user_permissions() less Permission::CalendarSchedulingSend>)`. `OUT` is `jmapcal-scheduling-out@remote.test`, `OUT2` `jmapcal-scheduling-out2@remote.test`, `PLAIN` the suite's `plain@example.com`. `invitation(uid, attendees)` is a module-private builder: Task 30's `event(uid)` plus `participants`, the organizer as `{ "@type": "Participant", "calendarAddress": "mailto:<key>", "roles": { "owner": true, "chair": true }, "participationStatus": "accepted" }` and each attendee as `{ "@type": "Participant", "calendarAddress": "mailto:<address>", "kind": "individual", "participationStatus": "needs-action", "expectReply": true }`. `send()` is `[("sendSchedulingMessages", true)]`. The module starts with `sink().clear()` and ends with `Party::delete_items_since` for key2 and key, `clear_received_scheduling(test, &plain)`, the CalDAV deletion (with `fanout::NO_REPLY`) of every event it left, `sink().clear()` and `finish`. The first case runs first: it needs an account that has pinned nobody.
- `first_invitation_and_the_session` (review focus 1; spec D19).
  1. `za_oauth_client(test, admin, "za-jmapcal-app", ZaClientKind::Application)`; `tokens = za_sign_in(.., key's name, STRONG, None)`.
  2. `za_bearer_jmap(&tokens.access, CALENDAR_JMAP_USING, ..)` with `CalendarEvent/set`, `sendSchedulingMessages: true`, create `invitation("jmapcal-sched-first", &[key2])`: status 200 and `created`; key2 holds one new inbox item.
  3. `CalendarEvent/get` of the event with the same access token: status 200. The first pin's rewrap raised the vault revision and evicted the resident keys; the token opens them again from the token record.
  4. `za_refresh(&app, &tokens.refresh)` is granted, and the new access token reads the event: it lists key2 as a participant with the schedule status the CalDAV body records for it.
  5. `token_record(test, key_id)` still holds the grant; the vault revision is one higher than before step 2, and `za_session_binding` of the vault record is unchanged.
  6. `za_revoke_token`, `za_drop_oauth_client`.
- `invite_key_outside_and_plain`. Create `invitation("jmapcal-sched-invite", &[key2, OUT, PLAIN])` with `send()`; `wait_for_queue`.
  - key2 holds exactly one new item, `METHOD:REQUEST`.
  - `sink().take_to(OUT)` holds exactly one message, `calendar_method() == "REQUEST"`.
  - In the organizer's CalDAV body the `SCHEDULE-STATUS` of `OUT` and of `PLAIN` is `["1.1"]`.
  - The CalDAV twin (the same invitation PUT as `jmapcal-sched-invite-dav`): key2's schedule status and the recipients of the stored token pairs are equal on both events.
  - Update `{ "start": "2031-03-04T12:00:00" }` with `send()`: one more `REQUEST` item for key2, one more `REQUEST` to `OUT`.
- `send_false_and_absent` (D4). Once with `sendSchedulingMessages: false`, once with no argument: create `invitation(uid, &[key2, OUT])`; `wait_for_queue`.
  - key2 has no new item; `sink().messages()` is empty.
  - The CalDAV body has no `SCHEDULE-STATUS` for key2 or `OUT`.
  - An update of the start without the flag: still no item, no mail, no status.
- `send_true_denied` (D18). `noperm` creates `invitation("jmapcal-sched-denied", &[OUT])` with `send()`.
  - `notCreated`, `forbidden`, "This account is not allowed to send calendar scheduling messages."
  - `uid_copies` is 0; `za_uid_leases(test)` is 0; the sink is empty.
  - Without the flag the same create succeeds. An update of it with `send()` answers the same `forbidden` and leaves the event unchanged.
  - The same invitation PUT over CalDAV answers 201 and mails nothing.
- `past_event_saved_unsent`. Start "2001-03-04T10:00:00", attendees key2 and `OUT`, `send()`: `created`; no item, no mail.
- `withheld_and_unreachable_statuses` (3.3). Attendees: `jmapcal-scheduling-nobody@example.com` (a local address with no entry) and a participant whose calendar address no method reaches (one for which `itip_unreachable_recipient`, `crates/groupware/src/scheduling/itip.rs`, answers), with `send()`.
  - `created`: the object is saved, not refused.
  - Each attendee's `SCHEDULE-STATUS` equals the one the CalDAV twin (the same tree PUT) records.
  - Control: the same object in `plain@example.com`, with `send()`, is `notCreated` with type `noSupportedScheduleMethods`.
- `mail_guard_on_failed_commit` (5.3, invariant 46 of release 5). Three events `jmapcal-sched-guard-1`, `-2`, `-3`, each created with `send()` and `OUT` as attendee; `wait_for_queue`; `sink().clear()`; `state` is the `CalendarEvent` state. `first`, `second`, `third` are their ids sorted as strings, and the one `CalendarEvent/set` call lists its updates in that order, each `{ "title": "guard canary <n>", "start": "2031-03-04T12:00:00" }`, with `send()`.
  - Interleaving: `UPDATE_COMMIT_PAUSE` is armed with a pause for `key_id` and the call starts on its own task. When that pause reports arrival (the first object's commit), a second pause is armed and the first released. When the second reports arrival (the first object has committed; the call is in the second object's commit), `set_update_commit_fail_once(Some(key_id))` is set and the second pause released. Both hooks are cleared after the call returns.
  - The answer is not a method error.
  - `updated` holds `first`. `notUpdated[second]` and `notUpdated[third]` are `forbidden`, "The server could not complete this change, try again."
  - The first event's CalDAV body holds "guard canary 1". The second's holds no "guard canary". The third was not attempted: its two stored records (`event_records`) are byte for byte those read before the call.
  - `CalendarEvent/changes` since `state` lists `first` alone under `updated`, and its `newState` equals the call's `newState`.
  - After `wait_for_queue` the sink holds exactly one message to `OUT`, for the first event's UID.
  - The trace (`Traced::start`, read as `tests/src/za/imip/outbound.rs:1222-1240` reads it) holds `ZA_IMIP_DROPPED` for the account with the second event's document id, and none for the third.
- `no_itip_task_ever` (invariant 57). Under `hold_task_consumer(test)`: one create and one update with `send()`, attendees key2, `OUT` and `PLAIN`; one destroy over CalDAV. `itip_tasks_for(test, key_id)` is 0.

**Steps:**
- [ ] 1. Write the eight `events` cases.
- [ ] 2. Write `scheduling.rs` with its eight cases and register it (the two lines under Files).
- [ ] 3. Run ZA. Expected: FAIL in `jmap_calendar::events` at `update_keeps_pairs_and_display_name`: no `updated` entry, the interim arm answers `notUpdated`.
- [ ] 4. Implement the head's added step, the grouping and the update loop in `za.rs`, in the order above, replacing the interim update arm. The interim answer stays for a change of `calendarIds` (step 17) and for destroys.
- [ ] 5. Run U-ws for `rulings_cases` and `event_set_error_table`. Expected: each passes.
- [ ] 6. Run ZA. Expected: pass. The eight `events` cases and the eight `scheduling` cases prove the task.
- [ ] 7. Run JK. Expected: pass.
- [ ] 8. The closing checks.
- [ ] 9. Commit: `Release 6: CalendarEvent/set update and scheduling for key accounts`.

Catchers for Task 42 (spec 9.7): the writer calls `ItipMessages::queue`: `scheduling::no_itip_task_ever`; drops the token pairs on update, drops the display name: `events::update_keeps_pairs_and_display_name`; skips the SEQUENCE floor: `events::update_applies_sequence_floor`. `scheduling::send_false_and_absent` catches `sendSchedulingMessages: false` mapped to `On` before the oracle's kill check does.

---

### Task 32: `CalendarEvent/set` placement

**Spec:** 5.2, 6.5 (`calendarIds`), 8.

**Files:**
- Modify: `crates/jmap/src/calendar_event/set/za.rs`: steps 17 and 36 of Task 31's update order.
- Test: `tests/src/za/jmap_calendar/events.rs`.

**Interfaces:**
- Consumes: `ZaPlacementPlan::jmap`, `ZaPlacementPlan::is_change`, `ZaPlacementContexts`, `za_placement_contexts` (I §2.9); the plan path of `za_commit_update` (I §2.8, §2.9: the UID check in each entered calendar, sealing in the target domain, the reseal of the event's dead-property container, one vanished href per left name, the retry).
- Produces: the placement path of `za_calendar_event_set`. No new name.

**Constraints:**
- 5.2: `za_placement_contexts` resolves the contexts of a placement change for both protocols. The writer builds the plan and adds no rule of its own: it asks for no right and gives no refusal that the function does not.
- `content_changed` is whether the shaped tree differs from the stored one. Upstream stamps `updated` on an owner's update, so most JMAP moves ask `ModifyItems` as well; the two share levels cannot tell the difference.
- 5.2: a JMAP update that changes placement and content is one update and one commit.
- D17: the limit on the number of calendars and the existence and cache ACL checks of `calendarIds` are upstream's (`:657-708`), with upstream's descriptions.
- 3.3: UID uniqueness in an entered calendar is the pipeline's check against current archives. Upstream's `moved_uid_index` (`:426-442`, `:717-731`) is not used.
- Invariant 57: the writer logs no vanished href itself; the pipeline does.
- The pipeline is not edited by this task.

**Order.** Two steps of Task 31's update order change:
- Step 17 (U `:709-716`): `is_calendar_ids_changed` no longer answers the interim error.
- Step 36: when `is_calendar_ids_changed` is false, as in Task 31. When true:
  1. `content_changed` is whether the tree after step 27 differs from `previous_event` (compared before step 33 consumes the tree).
  2. `plan = ZaPlacementPlan::jmap(<the stored row's names>, &new_calendar_event.names, content_changed)`.
  3. `contexts = za_placement_contexts(server, access_token, account_id, &plan)`; its error through `za_event_set_error(error, <the event's UID>)`.
  4. `ZaEventUpdate { placement: Some(plan), contexts, addressed: new_calendar_event.names[0].parent_id, .. }`: `names` holds the kept names in stored order and then a new random name per entered calendar, as upstream's `patch_parent_ids` leaves them.

A UID held in an entered calendar comes back from `za_commit_update` as `UidConflict` and answers `invalidProperties` on `uid` with "An event with UID {uid} already exists."

**Tests** (za cases in `events.rs`). `a` is the default calendar; `b` and `c` are made with `Calendar/set`; `s` is "jmapcal-events-shared" (`make_calendar`), shared with key2 at `edit`. The sync assertions follow upstream's own test at `tests/src/jmap/calendar/event.rs:919-1022`: a `sync-collection` REPORT of a calendar from the token taken before the change. Before they return, the cases revoke the grant on `jmapcal-events-shared` and delete `b`, `c` and that calendar: the module's cleanup deletes only its own shared calendar.
- `move_link_unlink_by_calendar_ids`. `jmapcal-events-move` is in `a`.
  - Move, `{ "calendarIds": { b: true } }`: `updated`; `calendarIds` is `{ b }`; the sync of `a` lists the old href with 404; a GET of the old href is 404; the event is served under `b`.
  - Link, `{ "calendarIds/<c>": true }`: `updated`; `calendarIds` is `{ b, c }`; one resource with the UID in each of `b` and `c`; no tombstone.
  - Unlink, `{ "calendarIds/<b>": null }`: `updated`; `calendarIds` is `{ c }`; the sync of `b` lists its href with 404; `c` is unchanged.
  - `uid_copies` is 1 throughout.
- `placement_and_content_together`. `{ "calendarIds": { a: true }, "title": "Moved canary" }`: `updated`.
  - `CalendarEvent/changes` since the state before lists the id once, under `updated`, and its `newState` equals the call's.
  - The CalDAV body at the new href holds `SUMMARY:Moved canary`.
- `placement_refusals`. Each refusal is `notUpdated`, `forbidden`, and leaves `calendarIds` as it was and no tombstone.
  1. An event of `a` linked into `s` as well (`calendarIds/<s>: true`).
  2. An event of `s` linked into `a` as well.
  3. An event in `a` and `b` (two names) moved into `s` (`calendarIds: { s: true }`).
  4. Allowed: a single-name event of `a` moved into `s`: `updated`; key2 reads it over JMAP and CalDAV.
  5. Allowed: moved back into `a`: `updated`; key2 no longer finds it.
  6. A second event with a UID `b` already holds, moved from `a` into `b`: `invalidProperties`, properties `["uid"]`, "An event with UID {uid} already exists."
- `calendar_id_that_does_not_exist`. `missing` is `Id::from(999_999u32)`. An update `{ "calendarIds": { missing: true } }` and a create `event_in("jmapcal-events-missing", missing)`: each `invalidProperties`, properties `["calendarIds"]`, with a description ending "does not exist."

**Steps:**
- [ ] 1. Write the four cases.
- [ ] 2. Run ZA. Expected: FAIL in `jmap_calendar::events` at `move_link_unlink_by_calendar_ids`: no `updated` entry.
- [ ] 3. Implement the two changed steps in `za.rs`.
- [ ] 4. Run ZA. Expected: pass. The four cases prove the task; `jmap_calendar::pipeline`'s placement cases (Task 11) still pass.
- [ ] 5. Run JK. Expected: pass.
- [ ] 6. The closing checks.
- [ ] 7. Commit: `Release 6: CalendarEvent/set placement for key accounts`.

---

### Task 33: `CalendarEvent/set` destroy

**Spec:** 6.5 (destroy), 5.2, 5.3, 5.4 (D18), 8.

**Files:**
- Modify: `crates/jmap/src/calendar_event/set/za.rs`: the destroy loop replaces the interim destroy arm; `ZA_INTERIM` goes.
- Test: `tests/src/za/jmap_calendar/events.rs`, `tests/src/za/jmap_calendar/scheduling.rs`.

**Interfaces:**
- Consumes: `za_delete_send_itip` (I §2.6); `za_destroy_event`, `ZaDeleted` (I §2.10, §2.1).
- Produces: the destroy path of `za_calendar_event_set`. `ZA_INTERIM` leaves the code; I §7.3's interim rule ends here.

**Constraints:**
- Invariant 57: no path calls `DestroyArchive::remove_all`, `notify_direct_change`, `log_calendar_vanished` or `preload_container_cleanup` for a key account's event. The event is deleted only by `za_destroy_event`.
- D18: an event destroy uses the condition CalDAV's DELETE uses, through `za_delete_send_itip`. `sendSchedulingMessages` true is `Required` (a denied status answers `forbidden` before anything is deleted), false or absent is `Off`. The writer does not call `ItipSendStatus::resolve`.
- 6.5: destroying one occurrence through its synthetic id is upstream's conversion to an update (`plan_instances`); it runs through Task 31's update path.
- The stored tree of a key account is sealed. Upstream's ownership test for a caller who may only manage its own items (`:1494-1518`) reads the event's tree: for a key account it is evaluated on the view from `ZaEventReader::view`, never on the stored content.
- The store-failure rule of Task 30 applies to each id of `destroy_events`.

**Order of the destroy path**, per id of `destroy_events` (U lines at `crates/jmap/src/calendar_event/set.rs`):
1. U `:1432-1435`: not in the cache: `notFound`.
2. U `:1437-1453`: the stored row; missing is `notFound`.
3. U `:1456-1462`: `assert_privacy_access`.
4. U `:1494-1518`: for a shared caller, every calendar of the event is in `can_delete_calendars`, or the caller may manage its own items there and owns the event: otherwise `forbidden`, "You are not allowed to remove events from calendar {id}." The ownership is computed lazily from the view.
5. P (in place of `:1520-1548`): `za_delete_send_itip(server, access_token, &roles.owner, schedule)`, `schedule` being `Required` when `send_scheduling_messages`, else `Off`. `SchedulingDenied(reason)` answers `forbidden` with `reason`.
6. P (in place of `:1550-1598`): `za_destroy_event(server, access_token, &cache, &keys, &roles, account_id, document_id, <the stored row's archive>, send_itip)`; its error through `za_event_set_error(error, None)`.
7. U `:1600`: `response.destroyed.push(id)`; `response.new_state = State::Exact(deleted.change_id).into()`.

Not taken: `:1412-1428` (the container cleanup preload), `:1463-1493` (the notification audience and the stored content).

**Tests.**

za cases in `events.rs`:
- `destroy_and_destroy_linked`.
  - A single-name event: `destroyed`; `uid_copies` is 0; a GET of its former href is 404.
  - An event in `a` and `b`: `destroyed`; `CalendarEvent/changes` since the state before lists the id once, under `destroyed`, and its `newState` equals the call's; a sync of `a` and one of `b`, each from the earlier token, lists the name's href with 404.
  - An id that does not exist: `notDestroyed`, `notFound`.
- `destroy_occurrence_is_an_update`. `second` is the synthetic id of the second occurrence of `jmapcal-events-rec`. Its destroy lists `second` under `destroyed`; `uid_copies` is 1; `CalendarEvent/query` with `expandRecurrences` no longer returns `second`; the CalDAV body excludes that date.

za cases in `scheduling.rs`:
- `cancel_and_decline_on_destroy`.
  - Organizer: `jmapcal-sched-cancel` with key2 and `OUT`, created with `send()`; sink cleared. Destroy with `send()`: `destroyed`; after `wait_for_queue` key2 holds exactly one new item, `METHOD:CANCEL`, and the sink exactly one `CANCEL` to `OUT`.
  - Attendee: `jmapcal-sched-decline`, key invites key2; under a `DrainOn` guard `trigger_drain(key2's name, STRONG)` imports it. key2 destroys its copy with `send()`: key gets exactly one new item, `METHOD:REPLY`, with `item_partstat(&item, key2's address) == "DECLINED"`.
  - Without the flag: a third event destroyed by its organizer leaves no item and no mail.
  - Denied: `noperm` destroys one of its events with `send()`: `notDestroyed`, `forbidden`, "This account is not allowed to send calendar scheduling messages."; the event is still there.
- `mail_guard_on_failed_delete` (5.3). Three events `jmapcal-sched-delguard-1`, `-2`, `-3`, each created with `send()` and `OUT` as attendee; `wait_for_queue`; `sink().clear()`; `state` is the `CalendarEvent` state. One call destroys `[first, second, third]` with `send()`.
  - Interleaving, as in `mail_guard_on_failed_commit` with `DELETE_COMMIT_PAUSE` and `set_delete_commit_fail_once(Some(key_id))`: the first pause is released once a second is armed; the fail flag is set while the call is parked in the second object's commit.
  - The answer is not a method error.
  - `destroyed` is `[first]`. `notDestroyed[second]` and `notDestroyed[third]` are `forbidden`, "The server could not complete this change, try again."
  - `uid_copies` of the three UIDs is 0, 1, 1. The third was not attempted: its two stored records are byte for byte those read before the call.
  - `CalendarEvent/changes` since `state` lists `first` alone under `destroyed`, and its `newState` equals the call's `newState`.
  - After `wait_for_queue` the sink holds exactly one `CANCEL` to `OUT`, for the first event's UID.
  - The trace holds `ZA_IMIP_DROPPED` for the account with the second event's document id, and none for the third.

**Steps:**
- [ ] 1. Write the four cases.
- [ ] 2. Run ZA. Expected: FAIL in `jmap_calendar::events` at `destroy_and_destroy_linked`: no `destroyed` entry, the interim arm answers `notDestroyed`.
- [ ] 3. Implement the destroy loop in `za.rs`, in the order above; remove the interim arm and `ZA_INTERIM`. The cases of Tasks 30 to 32 keep deleting their events over CalDAV.
- [ ] 4. Run ZA. Expected: pass. The four cases prove the task.
- [ ] 5. Run JK. Expected: pass.
- [ ] 6. `git grep -n "not available for this account yet" crates/`. Expected: no output.
- [ ] 7. The closing checks.
- [ ] 8. Commit: `Release 6: CalendarEvent/set destroy for key accounts`.

Guards for Task 42: `scheduling::cancel_and_decline_on_destroy` (a destroy that skips the fan-out), `scheduling::mail_guard_on_failed_delete` (mail submitted before the commit; a store failure that fails the method).

---

### Task 34: the drain hook

**Spec:** 5.5; 9.2 (a reply to an outside organizer).

**Files:**
- Modify: `crates/jmap/src/api/request.rs`: one statement before `:194` (`let last_adding = if let Some(keys) = keys {`), and the `use` it needs.
- Modify: `crates/jmap/src/api/za_calendar.rs`: the `#[allow(dead_code)]` on `is_calendar_method` goes.
- Test: `tests/src/za/jmap_calendar/scheduling.rs`.

**Interfaces:**
- Consumes: `is_calendar_method` (I §7.1); the hook's form (I §7.4); `ZaDrain::za_drain` (`crates/groupware/src/calendar/drain.rs:227`); `AccessToken::za_keys_for` (`crates/common/src/auth/access_token.rs:555`).
- Produces: the hook. No new name.

**Constraints:**
- 5.5: a JMAP request runs `za_drain` once, before dispatch, when its token holds its own account's keys and the request holds a method of `Calendar`, `CalendarEvent` or `ParticipantIdentity`. A request of mail methods only, or of `CalendarEventNotification` methods only, does not drain.
- The drain never fails the request: `za_drain` returns an outcome, not a `Result`, and the hook ignores it.
- The hook drains the caller's own inbox, whatever account the methods name.
- The search pass hook beside it (`:194-216`) is not edited.

**Tests** (za cases in `scheduling.rs`; each holds `DrainOn` for its own body).
- `drain_runs_from_jmap`. key creates `invitation("jmapcal-sched-drain", &[key2])` with `send()`.
  - `pending_ids(test, key2_id)` has one entry.
  - key2's `Calendar/get` is not an error.
  - `pending_ids` is then empty and `uid_copies(key2_id, "jmapcal-sched-drain")` is 1.
- `mail_only_request_does_not_drain`. A second invitation, `jmapcal-sched-nodrain`, is pending for key2.
  - After key2's `Mailbox/get`: `pending_ids` still has one entry.
  - After key2's `CalendarEventNotification/get`: still one entry.
  - After key2's `ParticipantIdentity/get`: empty.
- `invitation_by_email_found_by_uid`. The outside organizer `OUT` mails key: `imip::deliver(OUT, key's address, &imip::itip_mail(OUT, key's address, "REQUEST", &imip::event("jmapcal-sched-mail", OUT, &[key's address], 0, "")))`, then `wait_for_queue` and `sink().clear()`.
  - key's `CalendarEvent/query` with the filter `{ "uid": "jmapcal-sched-mail" }` returns one id: the same request drained, then queried.
  - The reply: `p` is the key of the participant whose `calendarAddress` is key's in `CalendarEvent/get`. Update `{ "participants/<p>/participationStatus": "accepted" }` with `send()`: `updated`. After `wait_for_queue` the sink holds exactly one message to `OUT`, with `calendar_method() == "REPLY"`, and its calendar part holds `PARTSTAT=ACCEPTED` on the `ATTENDEE` line of key's address.
- `drain_failure_does_not_fail_the_request`. `jmapcal-sched-drainfail` is pending for key2. The backoff is set before the failure, as `tests/src/za/drain.rs:2611-2615` does: `set_drain_retry_override(Some(2))`, then `set_drain_fail_once(Some(key2_id))`.
  - key2's `Calendar/get` is not an error; `pending_ids` still has one entry.
  - After three seconds a second `Calendar/get` leaves `pending_ids` empty.
  - `set_drain_retry_override(None)`.

**Steps:**
- [ ] 1. Write the four cases.
- [ ] 2. Run ZA. Expected: FAIL in `jmap_calendar::scheduling` at `drain_runs_from_jmap`: the pending item is still there after the `Calendar/get`.
- [ ] 3. Add the hook in `handle_jmap_request`, before `:194`: when `access_token.za_keys_for(owner)` is `Some(keys)` and any call of `request.method_calls` has `is_calendar_method(call.name.obj)`, `self.za_drain(keys).await;`. `owner` is the local of `:174`. Remove the `#[allow(dead_code)]` on `is_calendar_method`.
- [ ] 4. Run ZA. Expected: pass. The four cases prove the task. The modules that read pending items at store level (`fanout`, `drain`, `pinning`, the `imip` modules) pass unchanged: the suite's drain skip covers the new hook as it covers CalDAV's.
- [ ] 5. Run JK. Expected: pass. That suite does not skip drains; its calendar requests now drain.
- [ ] 6. The closing checks.
- [ ] 7. Commit: `Release 6: the drain runs from JMAP calendar requests`.

Catcher for Task 42 (spec 9.7): the drain hook is removed: `scheduling::drain_runs_from_jmap`.

---

### Task 35: the session and principal objects

**Spec:** 6.10 (D13), 6.1.

**Files:**
- Modify: `crates/jmap/src/api/session.rs:272-291` (`za_hidden_capability` and its doc comment), and the comment at `:125-126`.
- Modify: `crates/jmap/src/principal/get.rs:181-188` (`mayGetAvailability`), and the doc comment of `za_shown_capability` (`:370-373`).
- Modify: `crates/jmap/src/api/metadata/za.rs:110-138`: the unit test `key_accounts_are_offered_no_metadata` pins `za_hidden_capability` and changes with it.
- Test, existing assertions: `tests/src/za/gating.rs:233-246`, `:306-311`, `:332-341`; `tests/src/jmap/za_variants.rs` (`principal_get`: the doc comment `:3576-3579`, the comment `:3596-3597`, `:3648-3830`, `:3833-3849`, `:3911-3936`).

**Interfaces:**
- Consumes: the two one-line insertions of I §7.4.
- Produces: `za_hidden_capability` keeps its signature and hides `Metadata` only, when the account or the caller is a key account.

**Constraints:**
- D13: a key account's account object and `primaryAccounts` advertise `urn:ietf:params:jmap:calendars` and `urn:ietf:params:jmap:calendars:parse` for every token, keyed or not; a keyless token meets the refusal per method.
- D13: a grantee's session lists the sharer's account with the calendar capability. It follows from the same rule; no code is added for it.
- 6.10: the `Metadata` capability stays hidden for and from a key account (upgrade ruling 66). That arm of `za_hidden_capability` is not touched.
- 6.10: `Principal/get` shows a key principal's calendar capability block. Its `mayGetAvailability` is false. `accountId`, `mayShareWith` and `calendarAddress` are upstream's values, and the account's calendar capability object is upstream's.
- The fork's change is one removed match arm and one `&& !is_key`.

**Tests** (existing assertions, each changed in place with a comment citing 6.10).
- `key_accounts_are_offered_no_metadata` (`jmap` unit): for a key account, with either caller, `za_hidden_capability(true, _, capability)` equals `capability == Capability::Metadata`. The other two assertions stay.
- `gating.rs:233-246`: for both capabilities, `caps.get(capability)` is `Some` and `session["primaryAccounts"][capability]` is key1's id.
- `gating.rs:306-311` (the caller is `plain`, which has no access to the account): `key_principal["capabilities"][calendars]` is `{ "accountId": null, "mayGetAvailability": false, "mayShareWith": true, "calendarAddress": "mailto:key1@example.com" }`.
- `gating.rs:332-341` (the account's own principal): `key_principal["capabilities"][calendars]` is `{ "accountId": <key1's id>, "mayGetAvailability": false, "mayShareWith": false, "calendarAddress": "mailto:key1@example.com" }`, and `key_principal["accounts"][<key1's id>]["accountCapabilities"].get(calendars)` is `Some`.
- `za_variants::principal_get`:
  - John's `accountCapabilities` (`:3648-3830`) gain, where upstream's own test has them (`tests/src/jmap/principal/get.rs:121-129`):

```json
"urn:ietf:params:jmap:calendars": {
  "maxCalendarsPerEvent": 10,
  "minDateTime": "0001-01-01T00:00:00Z",
  "maxDateTime": "9999-12-31T23:59:59Z",
  "maxExpandedQueryDuration": "P52W1D",
  "maxParticipantsPerEvent": 20,
  "mayCreateCalendar": true
},
"urn:ietf:params:jmap:calendars:parse": {},
```

  - `primaryAccounts` (`:3833-3849`) gains `"urn:ietf:params:jmap:calendars": john_id` and `"urn:ietf:params:jmap:calendars:parse": john_id`. `urn:ietf:params:jmap:metadata` stays absent.
  - The three key principals (`:3911-3936`) each gain a `urn:ietf:params:jmap:calendars` block: John `{ "accountId": john_id, "mayGetAvailability": false, "mayShareWith": false, "calendarAddress": "mailto:jdoe@example.com" }`; Jane `{ "accountId": null, "mayGetAvailability": false, "mayShareWith": true, "calendarAddress": "mailto:jane.smith@example.com" }`; Bill the same with "mailto:bill@example.com". The loop's tuple carries the two values that differ.
  - The Sales group (`:3937-3957`) is unchanged: `mayGetAvailability` true.
  - The doc comment (`:3576-3579`) and the comment at `:3596-3597` say what is pinned now: upstream's objects, less object metadata, with `mayGetAvailability` false for key principals.

**Steps:**
- [ ] 1. Change the unit test.
- [ ] 2. Run U-ws for `key_accounts_are_offered_no_metadata`. Expected: FAIL, `left: true, right: false` for `Calendars`.
- [ ] 3. Change the three places of `gating.rs` and re-pin `za_variants::principal_get`.
- [ ] 4. Implement. In `za_hidden_capability` the arm `Capability::Calendars | Capability::CalendarsParse => account_is_key` goes; its doc comment and the comment at `session.rs:125-126` drop the calendar sentence. In `principal/get.rs:181-188` the value of `mayGetAvailability` gains `&& !is_key`; the doc comment of `za_shown_capability` follows.
- [ ] 5. Run U-ws for `key_accounts_are_offered_no_metadata`. Expected: pass.
- [ ] 6. Run ZA. Expected: pass; `gating::test` proves the task.
- [ ] 7. Run JK. Expected: pass; `za_variants::principal_get` proves the task.
- [ ] 8. The closing checks.
- [ ] 9. Commit: `Release 6: key accounts advertise the calendar capability`.

---

### Task 36: the `jmap_calendar::sharing` module

**Spec:** 6.1, 6.5 (a grantee's writes, D10, D11), 6.7, 6.10, 9.2; review focus 3.

**Files:**
- Test: `tests/src/za/jmap_calendar/sharing.rs` (Task 22's file; cases added). No product file.

**Interfaces:**
- Consumes: the methods Tasks 21 to 35 serve; test helpers (I §10): `pair`, `key_account`, `make_calendar`, `share`, `put_event`, `finish`; `za_post("shares/revoke", { "username", "password", "calendar": slug, "grant": grantee_id })` (`tests/src/utils/za.rs:196`); `assert_no_event_preferences` (`tests/src/utils/za.rs:1213`); the `_account` forms of `Account`'s JMAP helpers (`tests/src/utils/jmap.rs:70`, `:161`, `:202`, `:242`), in which `self` is the caller and `account` the target.
- Produces: six cases.

**Constraints:**
- Accounts: `jmapcal-sharing` (the owner), `jmapcal-sharing-reader` (the grantee), `jmapcal-sharing-2` (never granted). Task 22's set-up created all three (`pair(test, "sharing")` and `key_account(test, "sharing-reader", ..)`); this task creates none. The outside party is `jmapcal-sharing-org@remote.test`.
- `cal` is the module's shared calendar "jmapcal-shared", which Task 22's setup makes, with two events, and shares at `view`; `seed` is one of those events. A case that needs another level regrants with `share` and puts `view` back at its end. A case that makes a calendar revokes its grant before deleting it.
- 6.1: a grantee uses its own keyed session; no case signs in as the owner to act for the grantee.

**Tests.**
- `grantee_reads_and_writes_at_both_levels`.
  - At `view`: the grantee's `Calendar/get` in the owner's account lists one calendar, with `myRights.mayReadItems` true and `myRights.mayWriteAll` false.
  - `CalendarEvent/get` of `seed` returns the title the setup gave it.
  - A create in `cal` is `notCreated`, `forbidden`.
  - An update of `seed`'s title is `notUpdated`, `forbidden`, "You are not allowed to modify this event."
  - An update of `seed`'s `alerts` is `notUpdated`, `forbidden` (6.5: no per-user copy).
  - A destroy of `seed` is `notDestroyed`, `forbidden`.
  - `Calendar/set` update with `shareWith` naming `jmapcal-sharing-2`: `notUpdated`, `forbidden`, "Calendar sharing is managed on the account page."
  - Regranted at `edit`: a create of `jmapcal-sharing-edit` with `alerts` (`a1`, display, "-PT5M"), `keywords: { "kw": true }`, `color: "#aabbcc"` is `created`; `assert_no_event_preferences(test, owner_id, document_id)`; the owner's CalDAV body holds `BEGIN:VALARM` and `TRIGGER:-PT5M`; the owner's `CalendarEvent/get` returns the same alerts, keywords and colour.
  - The grantee updates the title: the owner's CalDAV body shows it. The grantee destroys the event: the owner's GET is 404.
- `delegated_reply_carries_sent_by`. The fixture of `tests/src/za/sharing.rs:5616-5842` (`delegated_receiver_rejections_and_reply`): the owner attends an event of the outside organizer, stored in "jmapcal-shared"; the grantee, at `edit`, answers for the owner. Here the answer is a JMAP update of `participants/<the owner's participant>/participationStatus` to "accepted" with `sendSchedulingMessages: true`: `updated`. After `wait_for_queue`:
  - the sink holds exactly one message to the organizer, with `calendar_method() == "REPLY"`;
  - its calendar part holds `PARTSTAT=ACCEPTED` and `SENT-BY="mailto:jmapcal-sharing-reader@example.com"`.
- `revoke_ends_access`. After `shares/revoke` of the owner's only grant to the grantee, each call of `served_calls` by the grantee answers the method error `accountNotFound`: upstream's access check answers first (spec 6.1). The grantee's session no longer lists the owner's account; a CalDAV PROPFIND of the calendar is refused; the event the grantee wrote is still the owner's to read.
- `non_grantee_refused`. Each call of `served_calls` by `jmapcal-sharing-2` answers the method error `accountNotFound` (spec 6.1: the fork's `forbidden` applies only after the access check passes; Task 37 holds those cases).
- `grantee_default_alerts` (review focus 3). A calendar of its own, "jmapcal-alerts" (`make_calendar`): the owner's entry gets `defaultAlertsWithTime { "o1": display, start, "-PT10M" }`, and only then is it shared with the grantee at `edit`, so the grantee's entry starts as a copy of the owner's.
  - The grantee creates `jmapcal-sharing-alerts-1` with `useDefaultAlerts: true`: the owner's CalDAV body has one `BEGIN:VALARM` with `TRIGGER:-PT10M` (the grantee set none: the owner's defaults, as granted).
  - The grantee sets its own `defaultAlertsWithTime { "g1": email, start, "-PT45M" }`, then creates `jmapcal-sharing-alerts-2` with `useDefaultAlerts: true`: the owner's CalDAV body holds `TRIGGER:-PT45M`, not `TRIGGER:-PT10M`, and `ATTENDEE:mailto:jmapcal-sharing-reader@example.com`.
  - The owner's `CalendarEvent/get` of the second event: `useDefaultAlerts` false; `alerts["g1"]` has the trigger offset "-PT45M".
  - `assert_no_event_preferences(test, owner_id, <the second event>)`.
  - The owner's own create with `useDefaultAlerts: true` still gets `o1`; the first event is unchanged.
  - Cleanup before any wait: the events are deleted, so no email alarm task stays queued.
- `grantee_session_lists_the_sharer` (D13). In the grantee's session object, `accounts[<owner's id>]` has `isPersonal` false, an `accountCapabilities` entry for `urn:ietf:params:jmap:calendars` and none for `urn:ietf:params:jmap:metadata`. The session of `jmapcal-sharing-2` has no entry for the owner's account.

`served_calls(account, calendar, event)` is a module-private list: `Calendar/get`, `Calendar/query`, `Calendar/changes` (`sinceState: "n"`), `Calendar/set` (an update of `calendar`), `CalendarEvent/get`, `CalendarEvent/query`, `CalendarEvent/changes`, `CalendarEvent/set` (an update of `event`), each with `accountId` set to `account`.

**Steps:**
- [ ] 1. Write the six cases, each called from `test()` after `sync_boundary_matches_caldav`.
- [ ] 2. Run ZA. Expected: pass. A failing case is reported against the owning task (Tasks 21, 22, 26, 28, 30 to 33 or 35) with its name and output.
- [ ] 3. Kill check for `grantee_default_alerts`: in `crates/jmap/src/calendar_event/set/za.rs`, pass `account_id` in place of `personal_id` as the writer of `za_apply_default_alerts_by_value` in the create path. Run ZA. Expected: FAIL in `jmap_calendar::sharing` at `grantee_default_alerts` (the second event carries `TRIGGER:-PT10M`). Revert.
- [ ] 4. The closing checks.
- [ ] 5. Commit: `Release 6: the JMAP calendar sharing module`.

---

### Task 37: the `jmap_calendar::refusals` module

**Spec:** 6.1 (D25), 6.2, 6.11, 9.2; invariant 66.

**Files:**
- Test: `tests/src/za/jmap_calendar/refusals.rs` (Task 23's file; cases added).
- Modify: `tests/src/utils/za.rs`: `Keyless::call_using`, beside `Keyless::call` (`:2700-2734`). No product file.

**Interfaces:**
- Consumes: `ZA_CALENDAR_KEYLESS` (I §6); `CALENDAR_JMAP_USING`, `Keyless` (I §10); `ws_connect`, `ws_close` (`tests/src/za/tracing.rs:600`, `:658`); `pending_ids` (`tests/src/utils/za.rs:2855`); `SERVER_URL` (`tests/src/utils/za.rs:124`).
- Produces: `Keyless::call_using` (I §10). Its Bearer arm posts to `{SERVER_URL}/jmap`; its WebSocket arm sends the request as `call` does. `call` keeps the mail list.

**Constraints:**
- The Bearer token of the keyless cases is minted in process and carries no secret, so it is keyless.
- D25: the keyless description for calendars is compared whole.
- Invariant 66: a scheduling inbox item is never listed, opened or destroyed through JMAP. The destroy case reads the store before and after.
- The refusals here are method errors over an authenticated channel, not authentication failures, so no `X-Forwarded-For` address is needed.
- The module's checker is `assert_calendar_keyless`: `za_variants::assert_keyless` is another function, which the retention gate tracks.

**Tests.** `served(account)` is the thirteen methods section 6.2 serves, each with `accountId`:

| Method | Arguments beside `accountId` |
|---|---|
| `Calendar/get`, `Calendar/query`, `CalendarEvent/get`, `CalendarEvent/query`, `ParticipantIdentity/get` | none |
| `Calendar/changes`, `CalendarEvent/changes`, `ParticipantIdentity/changes` | `sinceState: "n"` |
| `CalendarEvent/queryChanges` | `sinceQueryState: "n"` |
| `Calendar/set`, `CalendarEvent/set`, `ParticipantIdentity/set` | `create: {}` |
| `CalendarEvent/parse` | `blobIds: []` |

`assert_calendar_keyless(response, what)`: the first response is an error of type `forbidden` whose `description` is `ZA_CALENDAR_KEYLESS`, "Calendars in this account are encrypted at rest and are not available to this session."

- `keyless_token_on_each_served_method`. Over `Keyless::bearer(&test.server, key's name)` and over `Keyless::WebSocket(ws_connect(key's name, STRONG))`, each call of `served` through `call_using(CALENDAR_JMAP_USING, ..)` passes `assert_calendar_keyless`. Control: the same thirteen calls with the password answer without a method error. The socket is closed.
- `impersonation_refused`. Each call of `served(key's id)` passes `assert_calendar_keyless` in two forms:
  1. a master-user login, Basic `jmapcal-refusals@example.com%admin@example.com` with the administrator's secret (a module-private `jmap_basic(user, secret, calls)`, as `tests/src/za/mail/gates.rs:1746-1765`);
  2. the administrator itself, a holder of the Impersonate permission, naming the key account.
- `every_row_of_6_11`.
  - `shareWith`: a `Calendar/set` create `{ "name": "jmapcal-refusals", "shareWith": { <key2's id>: { "mayReadItems": true } } }` is `notCreated`, `forbidden`, "Calendar sharing is managed on the account page."; an update with `shareWith` answers the same.
  - `CalendarEventNotification/get`, `/query`, `/set` (`destroy: []`), `/changes` (`sinceState: "n"`), `/queryChanges` (`sinceQueryState: "n"`), and `CalendarEvent/copy` with `accountId` and `fromAccountId` both the key account: each the method error `accountNotSupportedByMethod`.
  - Per-user event data: the owner's create with `alerts`, `keywords` and `color` passes `assert_no_event_preferences`. The grantee's side is `sharing::grantee_reads_and_writes_at_both_levels`.
  - Object metadata, with a value built as `metadata_value()` of `tests/src/za/gating.rs:883` builds it: a `Calendar/set` create and a `CalendarEvent/set` create that name `metadata` are `forbidden`, "Object metadata is not available for zero-access accounts."; a `CalendarEvent/get` that asks for `metadata` returns objects without the property; a `CalendarEvent/query` with a `metadata` condition answers the method error `unsupportedFilter`.
  - Inline attachment blobs: a create whose `links` entry holds a `blobId` (an upload of the account) is `invalidProperties` naming `links`, "Event attachments are not available for zero-access accounts."
  - `Principal/getAvailability` of the key principal, asked by the administrator: an empty list.
  - `ParticipantIdentity/set`: a create (`{ "name": "Work", "calendarAddress": "mailto:work@example.com" }`), an update and a destroy each answer the set error `forbidden` with the identities text of I §7.5.
- `notification_destroy_leaves_the_item` (invariant 66). key2 invites key over CalDAV (`jmapcal-refusals-notif`); the suite's drain skip keeps the item pending. `pending = pending_ids(test, key_id)` has one entry; `item` is its id.
  - `CalendarEventNotification/set` with `destroy: [item]`: the method error `accountNotSupportedByMethod`.
  - `pending_ids(test, key_id)` equals `pending`.
  - `CalendarEventNotification/get` with `ids: [item]`: the method error `accountNotSupportedByMethod`.
  - Cleanup: `Party::delete_items` on key; key2's event deleted with `fanout::NO_REPLY`.

**Steps:**
- [ ] 1. Add `Keyless::call_using`.
- [ ] 2. Write the four cases, each called from `test()` after Task 23's and Task 24's.
- [ ] 3. Run ZA. Expected: pass. A failing case is reported against the owning task (Task 20 for admission and its text; Tasks 21 to 24, 26, 28, 30 for a served method).
- [ ] 4. Kill check for `notification_destroy_leaves_the_item` (spec 9.7, "a notification destroy is let through"): in `crates/jmap/src/api/za_calendar.rs`, make `za_calendar_method(MethodObject::CalendarEventNotification, MethodFunction::Set)` return `Served`. Run ZA. Expected: FAIL in `jmap_calendar::refusals` at `notification_destroy_leaves_the_item` (no method error, or the item is gone). Revert.
- [ ] 5. The closing checks.
- [ ] 6. Commit: `Release 6: the JMAP calendar refusals module`.

