# Plan 17, group B: pipeline paths only JMAP uses (Tasks 10 to 12)

These tasks belong to [zero-access plan 17: JMAP calendars and browser sign-in](2026-11-14-zero-access-17-jmap-calendars.md), whose header holds the commands, the constraints and the task index. "I §n" is section n of the [interface sheet](2026-11-14-zero-access-17-jmap-calendars-interfaces.md).

Each task adds cases to `tests/src/za/jmap_calendar/pipeline.rs`, which
calls the pipeline in process with a keyed token. The za suite is one
test function; the module runs after `sharing_races::test` and before
`sweep::test`, so a run reaches it only after the calendar modules
before it pass.

### Task 10: a create in several calendars; the `jmap_calendar` test helpers

**Spec:** 5.2 ("A JMAP create may name several calendars under the same
rule"; "A create checks UID uniqueness under the lease in every calendar
it names"; "The commit asserts the context of every old and every new
calendar"), 9.2 (`jmap_calendar::pipeline`).

**Files:**
- Create: `tests/src/za/jmap_calendar/mod.rs`,
  `tests/src/za/jmap_calendar/pipeline.rs`.
- Modify: `tests/src/za/mod.rs:32-33` (declare `pub mod jmap_calendar;`
  between `index_task` and `leak`) and `:190-191`: the call
  `jmap_calendar::pipeline::test(&mut test).await;` goes immediately
  after `sharing_races::test`; Task 14 later puts `tokens::test` before
  it.
- Modify only if a case below fails:
  `crates/groupware/src/calendar/pipeline/uid.rs`,
  `crates/groupware/src/calendar/pipeline/create.rs`.
- Test: `tests/src/za/jmap_calendar/pipeline.rs`.

**Interfaces:**
- Consumes: `ZaScheduling`, `ZaWritten` (I §2.1); `ZaPipelineError`,
  `ZaBusy` (I §2.2); `za_writer_keys`, `za_roles` (I §2.3);
  `za_write_contexts` (I §2.4); `za_take_uid_lease` (I §2.5);
  `za_schedule_create` (I §2.6); `ZaEventCreate`, `za_create_event`
  (I §2.7). Existing test helpers: `za_unlock`, `stored_event`,
  `plant_sealed_event`, `za_uid_leases`, `HookParked`,
  `assert_no_event_preferences` (`tests/src/utils/za.rs`);
  `fanout::{MKCALENDAR, CONTENT_TYPE, NO_REPLY, uid_copies}`;
  `dav_seal::{resource_id, share_calendar}`; `mail::Traced`;
  `test_hooks::{set_commit_pause, set_lease_secs_override}`;
  `ZaUidLease::acquire`.
- Produces, in `tests/src/za/jmap_calendar/mod.rs` (I §10): `Pair`,
  `pair`, `key_account`, `make_calendar`, `share`, `event_ics`,
  `put_event`, `session_token`, `finish`. `session_token` is
  `AccessToken::new_maybe_invalid(test.server.access_token(account_id).await.unwrap()).with_session_keys(za_unlock(test, account_id, STRONG).await)`.
- Produces, private to `pipeline.rs`: the fixture `Fx` and the helper
  `create`, below.

**The fixture and the helper** (in `pipeline.rs`):

```rust
/// The module's pair and the ids of `pipe-a`, `pipe-b` and `pipe-s`.
struct Fx { pair: Pair, a: u32, b: u32, s: u32 }
impl Fx {
    /// `pair(test, "pipeline")`; `make_calendar` for the three slugs in the owner's home;
    /// `share(test, &pair.key, &pair.key2, pair.key2_id, "pipe-s", "edit")`.
    async fn new(test: &mut TestServer) -> Self;
    /// DELETEs the three calendars as the owner (204 each) and removes the inbox items the grantee received.
    async fn cleanup(&self, test: &TestServer);
}

/// The stages a key account's create runs, in the order of the CalDAV handler
/// (`crates/dav/src/calendar/update.rs`, the create branch), with one name per calendar.
async fn create(server: &Server, token: &AccessToken, owner: u32, calendars: &[u32], ics: &str,
    schedule: ZaScheduling) -> Result<ZaWritten, ZaPipelineError>;
```

`create` runs:
`fetch_groupware_resources(token.account_id(), owner, SyncCollection::Calendar)`;
the parse of `ics` and its UID; `za_writer_keys` (a key owner gives
`Some`) and `za_roles`; `za_take_uid_lease(.., calendars, uid)`; an
event with one `DavName { name: format!("{uid}.ics"), parent_id }` per
calendar; the content with
`CalendarEventData::new_for(ical, Tz::Floating, server.core.groupware.max_ical_instances, &DefaultAlerts::disabled(), true)`
and no preferences; `za_write_contexts(.., calendars[0], Acl::AddItems)`;
`next_event_alarms` over `calendars`; `za_schedule_create`;
`server.account(owner)`; `za_create_event`. After the lease is taken,
each early error releases it (`lease.release().await`) before it
returns, so a case can assert zero leases at once.

`test` prints `Running zero-access JMAP calendar pipeline tests...`,
builds `Fx`, runs the cases of Tasks 10, 11 and 12 in the order the
tasks list them, calls `fx.cleanup` and ends with
`finish(test, "pipeline")`.

Names the cases of Tasks 10 to 12 use: the owner is `pair.key`
(`jmapcal-pipeline@example.com`) and the grantee `pair.key2`
(`jmapcal-pipeline-2@example.com`); `a`, `b` and `s` are the ids of the
owner's calendars `pipe-a`, `pipe-b` (unshared) and `pipe-s` (shared
with the grantee at level `edit`); `HOME` is
`/dav/cal/jmapcal-pipeline%40example.com/`.

**Constraints:**
- The helpers of `jmap_calendar/mod.rs` are `pub(super)`: the later
  modules of the family use them, so their names and signatures are the
  sheet's.
- A parked create runs as a spawned future: it owns a `Server` clone
  (`test.server.clone()`) and a token built before it starts.
- `ZaWritten` has no `Debug`: a case that reports an unexpected result
  prints its document id.
- `key_account` and `Pair.dav2` have no reader in this task. Each
  carries `#[allow(dead_code)]`. Task 11 removes `dav2`'s; the first
  task that calls `key_account` removes that one.
- This task does not touch `dav`.

**Cases.** Every `create` below passes `ZaScheduling::On`.

`create_in_two_calendars`:
1. `create(.., &[a, b], event_ics("jmapcal-pipe-two", "two calendars"))`
   is `Ok(written)`.
2. `za_uid_leases` is 0. `uid_copies(owner, uid)` is 1: one document for
   two names. The stored event's names' parents, sorted, are `[a, b]`
   sorted. `stored.etag() == written.etag`.
   `get_last_change_id(owner, SyncCollection::Calendar)` is
   `Some(written.change_id)`.
3. The owner's GET of `{HOME}pipe-a/{uid}.ics` and of
   `{HOME}pipe-b/{uid}.ics` is 200 with `SUMMARY:two calendars` and no
   `X-ZA-`. `assert_no_event_preferences` holds.
4. Every calendar's context is asserted. MKCALENDAR of `pipe-gone` (201),
   its id `gone`. A
   `create(.., &[a, gone], event_ics("jmapcal-pipe-gone", "second calendar deleted"))`
   is started with `HookParked::start(owner, set_commit_pause, ..)`.
   While it is parked at its commit, the owner's DELETE of
   `{HOME}pipe-gone/` is 204. `parked.finish()` is then
   `Err(ZaPipelineError::NotFound)`: the commit lost its assertion and
   the retry found the calendar gone. `uid_copies(owner, uid)` is 0 (no
   event with an orphan name). `za_uid_leases` reaches 0 within 2 s,
   polled every 50 ms: this path releases the lease from the guard's
   drop, on a spawned task.
5. The first event is deleted with one DELETE per href and `NO_REPLY`.

`create_uid_taken_in_second_calendar`:
1. `put_event(.., "pipe-b", "jmapcal-pipe-taken", "already in pipe-b")`.
2. For the calendars `[a, b]` and then `[b, a]`:
   `create(.., event_ics(uid, "the second copy"))` is
   `Err(UidConflict { href: Some(h) })` with `h` ending
   `/pipe-b/jmapcal-pipe-taken.ics`; `za_uid_leases` is 0 at once;
   `uid_copies` is still 1.
3. The existing event still has one name.
4. `create(.., &[a], event_ics(uid, "same uid, other calendar"))` is
   `Ok`: a UID held in another calendar only is free (CalDAV's rule, one
   UID per calendar).
5. Both events are deleted.

`create_naming_a_shared_calendar_with_another`, UID
`jmapcal-pipe-linked`:
1. For `[a, s]` and then `[s, a]`, under
   `Traced::start(&[EventType::Security(SecurityEvent::Unauthorized)])`:
   `create(.., event_ics(uid, "linked"))` is `Err(LinkedEvent)`; one
   captured event has the owner's account id and the details
   `zero-access: event linked into a shared calendar`; `uid_copies` is
   0; `za_uid_leases` is 0.
2. `create(.., &[s], event_ics(uid, "shared only"))` is `Ok`. With
   `shared = za_owned_calendar_context(server, keys, s)` and
   `personal = za_owned_calendar_context(server, keys, a)`:
   `shared.domain.calendar_id()` is `Some` and `personal`'s is `None`;
   `unseal_event_in_domain` of the stored event is `Ok` in
   `shared.domain` and `Err` in `personal.domain`.
3. The event is deleted.

`lost_fence_rechecks_every_calendar` (`COMMIT_PAUSE`), once for
`("jmapcal-pipe-lost", plant)` and once for
`("jmapcal-pipe-lost-none", no plant)`:
1. `set_lease_secs_override(Some(1))`.
2. `create(.., &[a, b], event_ics(uid, "lost fence"))` is started with
   `HookParked::start(owner, set_commit_pause, ..)`.
3. After 2500 ms, `ZaUidLease::acquire(store, owner, uid)` returns
   `Some`: the expired lease is taken over.
4. With `plant`:
   `plant_sealed_event(test, owner, "pipe-b", "lost-other.ics", event_ics(uid, "the other creator"), STRONG, &TokenPairs::new())`.
   The other creator's copy lands in the second calendar the create
   names.
5. The test's lease releases (`true`); `set_lease_secs_override(None)`.
6. `parked.finish()` is, with `plant`,
   `Err(UidConflict { href: Some(h) })` with `h` ending
   `/pipe-b/lost-other.ics`; without it, `Err(Busy(ZaBusy::CreateLost))`.
7. `uid_copies` is 1 with `plant` and 0 without; `za_uid_leases` is 0.
8. The planted event is deleted.

Steps:

- [ ] Write `jmap_calendar/mod.rs` (`pub mod pipeline;` and the nine
  helpers of I §10) and `pipeline.rs` (the fixture, `create`, the four
  cases, `test`).
- [ ] Register the module in `tests/src/za/mod.rs` at the two places
  named under Files.
- [ ] Run ZA. Expected: it passes, with the module's banner line in the
  output. These paths had no caller before this task, and Tasks 3 and 5
  wrote them to the general form, so the cases are expected to pass. A
  case that fails names a defect in one of two properties, which this
  task then fixes in `groupware` and nowhere else:
  - `za_take_uid_lease` checks every calendar of its slice
    (`pipeline/uid.rs`);
  - `za_create_event` passes every parent of the event's names to
    `za_lost_create_fence` and `za_commit_create`, and asserts every
    context in the insert batch (`pipeline/create.rs`).
- [ ] Kill-check the second case. Fault: in `pipeline/uid.rs`,
  `za_take_uid_lease` checks `&calendar_ids[..1]` only. Run ZA.
  Expected: a panic in `create_uid_taken_in_second_calendar` at the
  `[a, b]` round.
- [ ] Kill-check the fourth case. Fault: in `pipeline/create.rs`, only
  the first parent is passed to `za_lost_create_fence` at the commit.
  Run ZA. Expected: a panic in `lost_fence_rechecks_every_calendar` at
  `jmapcal-pipe-lost` (the answer is `Busy(CreateLost)`, not the
  conflict).
- [ ] The closing checks.
- [ ] Commit: `Release 6: a pipeline create in several calendars`.

### Task 11: placement in an update

**Spec:** 5.2 (placement: the rule, the rights, the UID check in each
entered calendar, the reseal in the target domain with the container,
one commit, the next alarms over the new calendars), 9.1 (the rights and
refusals of a placement change), 9.2 (`jmap_calendar::pipeline`).

**Files:**
- Modify: `crates/groupware/src/calendar/pipeline/placement.rs`
  (`ZaPlacementPlan::jmap`, a unit test);
  `crates/groupware/src/calendar/pipeline/update.rs` (the plan path of
  `za_commit_update`); `tests/src/za/jmap_calendar/pipeline.rs` (the
  `update` helper, four private readers, seven cases);
  `tests/src/za/jmap_calendar/mod.rs` (one `allow` goes);
  `tests/src/za/dav_seal.rs:1257,1267,1292` (`DEAD_CANARIES`,
  `dead_proppatch` and `assert_dead_round_trip` become `pub(super)`; no
  other change in that file).
- Test: `crates/groupware/src/calendar/pipeline/placement.rs` (unit),
  `tests/src/za/jmap_calendar/pipeline.rs`.

**Interfaces:**
- Consumes: `ZaReadMemo`, `za_read_event_view`, `za_write_contexts`,
  `za_refresh_event` (I §2.4); `za_assert_is_unique_uid` (I §2.5);
  `ZaScheduled` (I §2.6); `ZaEventUpdate`, `za_commit_update` (I §2.8);
  `ZaPlacementPlan`, `ZaPlacementContexts`, `za_placement_contexts`,
  `za_reseal_event_container` (I §2.9); `rewrap_event`
  (`crates/groupware/src/calendar/seal/event.rs:616`);
  `calendars_changed`
  (`crates/groupware/src/calendar/sharing/resolve.rs:111`). Test
  helpers: `event_ics`, `put_event`, `session_token` (I §10); `Fx` and
  `create` (Task 10); `dav_seal::assert_dead_container_sealed`;
  `shared_container`, `za_post` (`tests/src/utils/za.rs`);
  `container_is_ck`; `test_hooks::set_update_commit_pause`.
- Produces: `ZaPlacementPlan::jmap` (I §2.9) and the plan path of
  `za_commit_update` (I §2.8, §2.9): with `placement: Some(plan)` the
  commit runs the plan instead of returning Task 7's error. Private to
  `pipeline.rs`: the helper `update` and four readers, below.

**`ZaPlacementPlan::jmap`.** `parents`: the distinct parents of `old`,
ascending. `names`: `old.len()`. `source`: `None`. `left`: the parents
of `old` that no name of `new` has, ascending. `entered`: the parents of
`new` that no name of `old` has, without duplicates, in the order of
`new`. `overwrite` and `collection`: false.

**The plan path of `za_commit_update`.** With a plan, `ZaEventUpdate`
means: `new_event.names` holds the kept names and a new name for each
entered calendar; `contexts` came from `za_placement_contexts` with the
same plan; `addressed` is the first calendar the event stays in or
enters. The commit then differs from Task 7's in five places:

1. Before the stored pairs are opened and before any fan-out: for each
   calendar of `plan.entered`,
   `za_assert_is_unique_uid(server, resources, token, owner, calendar, uid)`
   with the stored row's UID. Its error returns at once; nothing was
   delivered or written.
2. Sealing is in `contexts.to_domain()` (Task 7 already seals there).
3. In each attempt's batch, before `update_full_at`: when the domain
   changes, that is when
   `(from.calendar_id(), from.domain_id()) != (to.calendar_id(), to.domain_id())`
   for `contexts.from_domain()` and `contexts.to_domain()`,
   `za_reseal_event_container(server, account, document_id, uid, <the stored row's metadata kinds>, from, to, &mut batch)`.
4. In each attempt's batch, after `update_full_at` and the alarms: one
   `batch.log_calendar_vanished(parent_id, href)` for each stored name
   that `new_event.names` no longer holds (same parent and same name),
   with `href = resources.format_item(&format!("{}/{}", calendar.path(), name))`
   for the calendar's `resources.container_resource_path_by_id(parent_id)`.
   A name whose calendar is not in `resources` logs nothing.
5. On an assertion loss at the first attempt:
   `fresh = za_placement_contexts(server, token, owner, &plan).await?`.
   If neither `calendars_changed(&contexts.parents, &fresh.parents)` nor
   `calendars_changed(&contexts.entered, &fresh.entered)` holds, the mail
   is dropped and the error returns. Otherwise the sealed event is
   rewrapped from the old `to_domain()` to the fresh one with
   `rewrap_event` (a seal error through
   `seal_error(err, owner, document_id)`), the contexts become `fresh`,
   and the second attempt rebuilds its batch, with places 3 and 4 against
   the fresh contexts.

Everything else is Task 7's: the pairs opened in `from_domain()`, the
fan-out, the display name, one timestamp, both hooks, the mail guard.

**Constraints:**
- The rights and the refusals come from `za_placement_contexts`, which
  CalDAV's COPY and MOVE call. `za_placement_contexts`, `parent_rights`,
  `entered_rights` and `shared_refusal` do not change.
- A placement change with or without a content change is one commit and
  one change id.
- A move between an unshared and a shared calendar reseals the event in
  the target domain and reseals its dead-property container in the same
  commit, or a native client can no longer read the properties it wrote
  (review focus 2).
- The UID check of an entered calendar is against current archives
  (`za_assert_is_unique_uid`), not a snapshot, and precedes every
  delivery.
- Every old and every entered calendar's context is asserted in the
  commit (`contexts.assert_in`).
- The plan path adds no settlement of the mail guard and no hook.
- This task does not touch `dav`.

**The helpers** (in `pipeline.rs`):

```rust
/// The stages a key account's update runs when its calendars or its content change, in process.
/// `calendars`: the event's calendars after the update. `ics`: `Some` replaces the tree.
async fn update(server: &Server, token: &AccessToken, owner: u32, document_id: u32, calendars: &[u32],
    ics: Option<&str>) -> Result<ZaWritten, ZaPipelineError>;
/// The owner's two domains, from `za_owned_calendar_context` of `pipe-a` and of `pipe-s`.
async fn domains(test: &TestServer, fx: &Fx) -> (ZaKeyDomain, ZaKeyDomain); // (personal, shared)
/// The hrefs logged as vanished at exactly `change_id`, sorted:
/// `store().vanished_with_change_id::<String>(owner, VanishedCollection::Calendar.into(),
/// Query::RangeInclusive(change_id, change_id))`.
async fn vanished_at(test: &TestServer, owner: u32, change_id: u64) -> Vec<String>;
/// The parents of the stored event's names, sorted.
async fn parents_of(test: &TestServer, owner: u32, document_id: u32) -> Vec<u32>;
/// The change ids of the calendar log's records after `tip`, in order:
/// `store().za_changes_records_range(owner, SyncCollection::Calendar, tip, u64::MAX)`.
async fn changes_after(test: &TestServer, owner: u32, tip: u64) -> Vec<u64>;
```

`update` runs: `fetch_groupware_resources`; the stored row and content
archives (`ValueKey::archive` and the `CalendarEventField::Content`
property); `za_read_event_view` through the stored first name's
calendar, with a new `ZaReadMemo` (the view's content and display name);
the old tree from the view; the new tree, parsed from `ics` or the old
one; the new names (each stored name whose parent is in `calendars`,
then for each other calendar of `calendars`
`DavName { name: <the stored first name's name>, parent_id }`);
`plan = ZaPlacementPlan::jmap(&stored.names, &new_names, ics.is_some())`;
the contexts (`za_placement_contexts(.., &plan)` when `plan.is_change()`,
else
`ZaPlacementContexts::unchanged(za_write_contexts(.., stored.names[0].parent_id, Acl::ModifyItems))`);
`za_writer_keys` and `za_roles`; when `ics` is `Some`,
`za_sequence_floor(&mut new, &old, roles.sender.addresses())` and
`CalendarEventData::new_for(new, Tz::Floating, max_ical_instances, &DefaultAlerts::disabled(), true)`
as the new content's data, else the view's content deserialized; the
previous alarms from the view over the old parents and the next alarms
from the new content over `calendars`, both with `next_event_alarms`;
`scheduled: ZaScheduled { messages: None, resolution: None }`;
`server.account(owner)`; `za_commit_update` with
`addressed: new_names[0].parent_id`, the stored archives, the new row
(the stored row with `names = new_names`), the view's display name, and
`placement: plan.is_change().then_some(plan)`.

**Unit test** `jmap_plans` (`groupware`, in `placement.rs`). Each row is
`ZaPlacementPlan::jmap(names(old), names(new), content_changed)`, where
`names` gives one name per listed parent. In every row `parents` is
`old`'s distinct parents ascending, `names` is `old`'s length, `source`
is `None`, and `overwrite` and `collection` are false.

| # | old | new | content | `left` | `entered` | `parent_rights` | `entered_rights` | shared current | shared entered |
|---|---|---|---|---|---|---|---|---|---|
| 1, a move | `[1]` | `[2]` | no | `[1]` | `[2]` | `RemoveItems` | `AddItems` | none | none |
| 2, a link | `[1]` | `[1, 2]` | no | `[]` | `[2]` | `ReadItems` | `AddItems` | `SharedCopy` | `SharedCopy` |
| 3, an unlink | `[1, 2]` | `[1]` | no | `[2]` | `[]` | `RemoveItems` | `AddItems` | `LinkedMove` | `LinkedMove` |
| 4, a move with a content change | `[1]` | `[2]` | yes | `[1]` | `[2]` | `RemoveItems`, `ModifyItems` | `AddItems` | none | none |
| 5, a link with a content change | `[1]` | `[1, 2]` | yes | `[]` | `[2]` | `ReadItems`, `ModifyItems` | `AddItems` | `SharedCopy` | `SharedCopy` |
| 6, a move of a linked event | `[1, 2]` | `[1, 3]` | no | `[2]` | `[3]` | `RemoveItems` | `AddItems` | `LinkedMove` | `LinkedMove` |
| 7, a move into two calendars | `[1]` | `[2, 3]` | no | `[1]` | `[2, 3]` | `RemoveItems` | `AddItems` | none | `LinkedMove` |
| 8, the same calendars, reordered | `[1, 2]` | `[2, 1]` | no | `[]` | `[]` | not asserted | not asserted | not asserted | not asserted |
| 9, a content change only | `[1]` | `[1]` | yes | `[]` | `[]` | not asserted | not asserted | not asserted | not asserted |

`is_change()` is true in rows 1 to 7 and false in rows 8 and 9. Row 1
reads: into or out of a shared calendar a move is allowed, because the
event has one name. Row 4 reads: a pure move never asks `ModifyItems`.

The order row: `jmap(names([5, 1, 3]), names([9, 1, 7, 9]), false)` has
`parents` `[1, 3, 5]`, `left` `[3, 5]`, `entered` `[9, 7]`, and `steps()`
`[(1, false), (3, false), (5, false), (9, true), (7, true)]`.

**Cases.** The owner, the grantee, `a`, `b`, `s` and `HOME` are Task
10's. Every `update` uses a token built after the case's CalDAV setup.
Each case deletes its events with `NO_REPLY` before it returns.

`placement_move_between_unshared`:
1. `id = put_event(.., "pipe-a", "jmapcal-pipe-move", "moves to b")`.
2. `update(.., id, &[b], None)` is `Ok(written)` with
   `written.document_id == id`.
3. `parents_of` is `[b]`. `vanished_at(owner, written.change_id)` is
   `["{HOME}pipe-a/{uid}.ics"]`. The owner's GET of that href is 404; of
   `{HOME}pipe-b/{uid}.ics` it is 200 with `SUMMARY:moves to b` and no
   `X-ZA-`.
4. The stored event opens (`unseal_event_in_domain`) in the personal
   domain and not in the shared one. `stored.etag() == written.etag`.
   `assert_no_event_preferences` holds.

`placement_unshared_to_shared_reseals`:
1. `id = put_event(.., "pipe-a", "jmapcal-pipe-share", "enters the shared calendar")`.
2. `update(.., id, &[s], None)` is `Ok`. The stored event opens in the
   shared domain and not in the personal one. The GET of
   `{HOME}pipe-s/{uid}.ics` is 200 with
   `SUMMARY:enters the shared calendar`, for the owner and for the
   grantee.
3. `update(.., id, &[a], None)` is `Ok`. The stored event opens in the
   personal domain and not in the shared one.

`placement_reseals_event_container` (review focus 2), value
`jmapcal-pipe-dead-value`:
1. `id = put_event(.., "pipe-a", "jmapcal-pipe-dead", "annotated by a native client")`.
   The owner's PROPPATCH of `dead_proppatch(value)` at
   `{HOME}pipe-a/{uid}.ics` is 207, and `assert_dead_round_trip` holds
   there for the owner.
2. `update(.., id, &[s], None)` is `Ok`. `assert_dead_round_trip` holds
   at `{HOME}pipe-s/{uid}.ics` for the owner and for the grantee: the
   native client still reads what it wrote.
   `container_is_ck(shared_container(owner, Collection::CalendarEvent, id))`
   is true. `assert_dead_container_sealed` holds with `DEAD_CANARIES`
   and the value.
3. `update(.., id, &[b], None)` is `Ok`. `assert_dead_round_trip` holds
   at `{HOME}pipe-b/{uid}.ics` for the owner. `container_is_ck` is
   false. `assert_dead_container_sealed` holds with the same canaries.

`placement_matches_caldav`. Each row runs twice on twin events with
their own UIDs (`jmapcal-pipe-twin-<row>-p` and `-c`): once as the
pipeline's `update`, once as the CalDAV request by the owner. Both runs
are traced with
`Traced::start(&[EventType::Security(SecurityEvent::Unauthorized)])`.

| Row | Setup of each twin | Pipeline: `calendars` | CalDAV twin | Answer | Trace details | Parents after |
|---|---|---|---|---|---|---|
| move | in `pipe-a` | `[b]` | `MOVE pipe-a/x → pipe-b/x` | `Ok` / 201 | none | `[b]` |
| link | in `pipe-a` | `[a, b]` | `COPY pipe-a/x → pipe-b/x` | `Ok` / 201 | none | `[a, b]` |
| unlink | in `pipe-a`, copied to `pipe-b` | `[a]` | `DELETE pipe-b/x` with `NO_REPLY` | `Ok` / 204 | none | `[a]` |
| into shared | in `pipe-a` | `[s]` | `MOVE pipe-a/x → pipe-s/x` | `Ok` / 201 | none | `[s]` |
| out of shared | in `pipe-s` | `[a]` | `MOVE pipe-s/x → pipe-a/x` | `Ok` / 201 | none | `[a]` |
| link into shared | in `pipe-a` | `[a, s]` | `COPY pipe-a/x → pipe-s/x` | `Err(Forbidden(ZaRefusal::SharedCopy))` / 403 | `zero-access: copy into or out of a shared calendar` | `[a]` |
| link out of shared | in `pipe-s` | `[s, a]` | `COPY pipe-s/x → pipe-a/x` | `Err(Forbidden(ZaRefusal::SharedCopy))` / 403 | `zero-access: copy into or out of a shared calendar` | `[s]` |
| linked into shared | in `pipe-a`, copied to `pipe-b` | `[a, s]` | `MOVE pipe-b/x → pipe-s/x` | `Err(Forbidden(ZaRefusal::LinkedMove))` / 403 | `zero-access: event linked into a shared calendar` | `[a, b]` |

For each row:
- the pipeline's result is the row's answer, and the CalDAV status is a
  success for an `Ok` row and 403 for a refusal row;
- the details of the captured events whose account id is the owner's,
  deduplicated, equal the row's trace details, for the pipeline's run
  and for CalDAV's run;
- `parents_of` equals the row's last column for the pipeline's twin and
  for CalDAV's twin;
- both twins are deleted when the row ends.

`placement_uid_taken_in_entered_calendar`:
1. `id = put_event(.., "pipe-a", "jmapcal-pipe-uid", "wants to enter b")`.
   The owner's PUT of `event_ics(uid, "holds the uid in b")` at
   `{HOME}pipe-b/holder.ics` is 201: the same UID in another calendar is
   allowed.
2. `update(.., id, &[b], None)` is `Err(UidConflict { href: Some(h) })`
   with `h` ending `/pipe-b/holder.ics`.
3. The stored event's `version` is what it was before the call;
   `parents_of` is `[a]`.
4. The twin MOVE of `{HOME}pipe-a/{uid}.ics` to `{HOME}pipe-b/{uid}.ics`
   is 403.

`placement_with_content_change_commits_once`:
1. `id = put_event(.., "pipe-a", "jmapcal-pipe-both", "before")`;
   `tip = get_last_change_id(owner, SyncCollection::Calendar)`.
2. `update(.., id, &[b], Some(event_ics(uid, "after")))` is
   `Ok(written)`.
3. `changes_after(owner, tip)` is `[written.change_id]`: one commit.
   `vanished_at(owner, written.change_id)` is
   `["{HOME}pipe-a/{uid}.ics"]`. `parents_of` is `[b]`.
4. The owner's GET of `{HOME}pipe-b/{uid}.ics` is 200 with
   `SUMMARY:after` and without `SUMMARY:before`. `written.ical` holds
   `SUMMARY:after`.

`placement_retry_after_grant_change` (`UPDATE_COMMIT_PAUSE`):
1. MKCALENDAR of `pipe-g` (201), its id `g`.
   `id = put_event(.., "pipe-a", "jmapcal-pipe-retry", "moves while pipe-g is shared")`.
2. `update(.., id, &[g], None)` is started with
   `HookParked::start(owner, set_update_commit_pause, ..)`. It parks
   immediately before its commit, with `pipe-g`'s unshared context
   asserted in the batch.
3. While it is parked,
   `za_post("shares/grant", {"username": <owner>, "password": STRONG, "calendar": "pipe-g", "address": <grantee>, "level": "edit"})`
   answers 200. The grant changes `pipe-g`'s archive and its key domain;
   the grantee is pinned already, by the fixture's share.
4. `parked.finish()` is `Ok(written)`.
5. `parents_of` is `[g]`. `za_owned_calendar_context(.., g)`'s
   `domain.calendar_id()` is `Some`, and the stored event opens in that
   domain: the retry did not store it in the domain `pipe-g` had before
   the grant. `stored.etag() == written.etag`. The grantee's GET of
   `{HOME}pipe-g/{uid}.ics` is 200 with the summary.
6. The event and `pipe-g` are deleted.

Steps:

- [ ] Write `jmap_plans`.
- [ ] Run U-groupware `jmap_plans`. Expected: no associated function
  `jmap` on `ZaPlacementPlan`.
- [ ] Implement `ZaPlacementPlan::jmap` by its rule.
- [ ] Run U-groupware `jmap_plans`, then `caldav_plan`. Expected: all
  three tests pass.
- [ ] Write `update`, the four readers and the seven cases; call the
  cases from `test` after Task 10's, in the order above. Make the three
  items of `tests/src/za/dav_seal.rs` `pub(super)`, and remove
  `#[allow(dead_code)]` from `Pair.dav2` in `jmap_calendar/mod.rs`.
- [ ] Run ZA. Expected: a panic in `placement_move_between_unshared`
  whose message holds `zero-access: placement change not implemented`.
- [ ] Implement the plan path of `za_commit_update` by the five places,
  and remove Task 7's error arm.
- [ ] Run ZA and WK. Expected: both pass. WK runs because
  `za_commit_update` is the CalDAV PUT's commit too.
- [ ] Kill-check the container reseal. Fault: in `pipeline/update.rs`,
  the call of `za_reseal_event_container` is removed. Run ZA. Expected:
  a panic in `placement_reseals_event_container`, at the first
  `assert_dead_round_trip` after the move.
- [ ] The closing checks.
- [ ] Commit: `Release 6: placement and content in one pipeline update`.

Catchers this task names for the mutation sample (spec 9.7): "a
placement change asks for a right CalDAV's `MOVE` does not" is caught by
`jmap_plans` (row 1: a pure move asks `RemoveItems` and nothing else)
and by `caldav_plans_keep_todays_rule`.

### Task 12: the destroy of a whole event; `za_delete_send_itip`

**Spec:** 5.2 (delete an event: `CalendarEvent/set` destroy), 5.4 (D18:
"An event destroy uses the condition CalDAV's `DELETE` uses, with the
same two answers"), 6.5 (destroy: cancellations and declines go through
the fan-out), 9.2 (`jmap_calendar::pipeline`).

**Files:**
- Modify: `crates/groupware/src/calendar/pipeline/delete.rs`
  (`za_destroy_event`; the shared commit's rebuild parameter);
  `crates/groupware/src/calendar/pipeline/schedule.rs`
  (`za_delete_send_itip`); `tests/src/za/jmap_calendar/pipeline.rs` (the
  `destroy` helper, `invitation`, four cases).
- Test: `tests/src/za/jmap_calendar/pipeline.rs`.

**Interfaces:**
- Consumes: `ZaDeleted`, `ZaScheduling` (I §2.1); `ZaPipelineError`
  (I §2.2); `ZaRoles`, `za_writer_keys`, `za_roles` (I §2.3);
  `za_write_contexts` (I §2.4); the shared commit of
  `pipeline/delete.rs` (Task 8); `remove_all_za_at`
  (`crates/groupware/src/calendar/storage.rs:991`);
  `ItipSendStatus::resolve`
  (`crates/groupware/src/calendar/itip.rs:1399`). Test helpers:
  `event_ics`, `put_event`, `session_token` (I §10); `Fx`, `create`,
  `vanished_at`, `changes_after` (Tasks 10 and 11); `uid_lease::put`
  (a five-second PUT); `inbox_ids`, `new_inbox_items`,
  `open_pending_item_as`, `item_method`, `item_sequence`,
  `item_attendees`, `wait_for_queue`, `itip_tasks_for`
  (`tests/src/utils/za.rs`); `smtp_sink::sink`.
- Produces: `za_destroy_event` (I §2.10); `za_delete_send_itip`
  (I §2.6). Private to `pipeline.rs`: `destroy` and `invitation`, below.

**`za_delete_send_itip`.** CalDAV's condition
(`crates/dav/src/calendar/delete.rs:99-102`, without its header term,
which `Off` stands for) is
`server.core.groupware.itip_enabled && !owner.addresses().is_empty() && token.has_permission(Permission::CalendarSchedulingSend)`.
It equals `!status.is_denied()` for
`status = ItipSendStatus::resolve(server, token, owner, i64::MAX, true)`;
compute it that way, so the denied reason comes from the same place.
With `Required` and a denied status the error is
`SchedulingDenied(status.reason())`, one of these three texts, in
`resolve`'s order:
- `"Calendar scheduling is disabled on this server."`
- `"This account has no calendar address to send scheduling messages from."`
- `"This account is not allowed to send calendar scheduling messages."`

`dav` does not call this function: its handler keeps upstream's line.

**`za_destroy_event`, in order.**
1. The stored row, deserialized; its names.
2. `za_write_contexts(server, token, owner_id, &row, row.names[0].parent_id, Acl::RemoveItems)`:
   every parent, each with `RemoveItems`. An empty result returns
   `Forbidden(ZaRefusal::Context)`.
3. A new batch; every context's assertion; `cutoff = now() as i64`; a
   clone of `event` kept for the retry.
4. `DestroyArchive(row unarchived).remove_all_za_at` with `roles.sender`,
   `content` `None` (the function reads it when it needs it),
   `AlarmTasks::None`, `send_itip`,
   `ContainerCleanup::empty(Collection::CalendarEvent)`,
   `Some(&contexts[0].domain)` and the cutoff.
5. One `batch.log_calendar_vanished(parent_id, href)` per stored name,
   with `href = resources.format_item(&format!("{}/{}", calendar.path(), name))`
   for `resources.container_resource_path_by_id(parent_id)`; a name whose
   calendar is not in `resources` logs nothing. Then
   `batch.commit_point()`, as the one-name delete does after its log.
6. The shared commit of Task 8, as an event delete: the fan-out of the
   returned message, `delete_commit_pause_point`,
   `take_delete_commit_fail`, the commit, the mail guard. Its retry
   rebuilds steps 2 to 5 with fresh contexts, `send_itip` false and the
   original cutoff, and runs only when `calendars_changed` holds. The
   shared commit takes the rebuild as a parameter, so that
   `za_delete_event` and this function use one implementation of it.

**Constraints:**
- A destroy of an event with one name stores and sends what a CalDAV
  `DELETE` of that name stores and sends.
- A destroy is one commit: every name goes, one vanished href per name,
  one change id.
- With `Required` and a denied status nothing is resolved, delivered or
  deleted: the caller gets `SchedulingDenied` before it calls
  `za_destroy_event`.
- The mail guard and the hooks behave as in Task 8; `za_delete_event`
  and `za_delete_calendar` keep their behaviour.
- The event's dead-property container is released by `remove_all_za_at`
  (`release_or_read`), as on a CalDAV delete.
- This task does not touch `dav`.

**The helpers** (in `pipeline.rs`):

```rust
/// The stages a key account's destroy runs, in process.
async fn destroy(server: &Server, token: &AccessToken, owner: u32, document_id: u32,
    schedule: ZaScheduling) -> Result<ZaDeleted, ZaPipelineError>;
/// `event_ics`, organized by the owner (`ORGANIZER:mailto:jmapcal-pipeline@example.com`), with one
/// `ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:` line per attendee.
fn invitation(uid: &str, summary: &str, attendees: &[&str]) -> String;
const OUTSIDE: &str = "jmapcal-pipeline-att@remote.test";
```

`destroy` runs: `fetch_groupware_resources`; the stored row archive;
`za_writer_keys` and `za_roles`;
`send_itip = za_delete_send_itip(server, token, &roles.owner, schedule)?`;
`za_destroy_event`.

**Cases.** The owner, the grantee, `a`, `b`, `s` and `HOME` are Task
10's.

`destroy_linked_event`:
1. `create(.., &[a, b], event_ics("jmapcal-pipe-destroy", "linked, then gone"), On)`
   is `Ok(written)`; `tip = written.change_id`.
2. `destroy(.., written.document_id, On)` is `Ok(deleted)`.
3. `uid_copies` is 0: no name survived. `changes_after(owner, tip)` is
   `[deleted.change_id]`: one commit. `vanished_at(owner, deleted.change_id)`
   is `["{HOME}pipe-a/{uid}.ics", "{HOME}pipe-b/{uid}.ics"]`: one href
   per name. The owner's GET of each href is 404. `za_uid_leases` is 0.

`destroy_single_name_equals_dav_delete`, twins `jmapcal-pipe-twin-dav`
and `jmapcal-pipe-twin-pipe`:
1. Each twin is PUT with the five-second client at
   `{HOME}pipe-a/{uid}.ics` as
   `invitation(uid, "cancelled twice", &[<grantee>, OUTSIDE])`: 201. A
   key attendee gets an inbox item, an outside one gets mail. Then
   `wait_for_queue`, `before = inbox_ids(grantee)` and `sink().clear()`.
2. The owner's DELETE of the first twin, through a five-second client
   because it mails, is 204.
3. `destroy(.., <the second twin's id>, On)`, with a token built now, is
   `Ok(deleted)`. Then `wait_for_queue`.
4. The same stored outcome: `uid_copies` is 0 for both twins;
   `vanished_at(owner, deleted.change_id)` is
   `["{HOME}pipe-a/jmapcal-pipe-twin-pipe.ics"]`.
5. The same items for the key attendee: the grantee's new inbox items,
   each opened with `open_pending_item_as(test, grantee, STRONG, id)` and
   keyed by its UID, are exactly two, one per twin. The tuples
   `(item_method, item_sequence, item_attendees)` of the two are equal,
   and the method is `ICalendarMethod::Cancel`.
6. The same mail for the outside attendee: `sink().take_to(OUTSIDE)`
   holds exactly two messages, one per twin, matched by `UID:<uid>` in
   the calendar part. Their `calendar_method()` values are equal, and
   are `CANCEL` ignoring case.
7. `itip_tasks_for(test, owner)` is 0: no task carried either
   cancellation.
8. The grantee's new inbox items are removed and the sink is cleared.

`delete_send_itip_matches_dav`. The tokens are
`AccessToken::from_permissions(owner, [Permission::CalendarSchedulingSend])`
and `AccessToken::from_permissions(owner, [])`; the owner's info is
`test.server.account_info(owner)`, and a clone of it with `addresses`
cleared. `test.server.core.groupware.itip_enabled` is true in the suite,
and the case asserts it. "Handler" is the condition of
`crates/dav/src/calendar/delete.rs:99-102`, header term aside, computed
in the case from the same inputs.

| Token | Owner's info | Handler | `Off` | `On` | `Required` |
|---|---|---|---|---|---|
| may send | as stored | true | `Ok(false)` | `Ok(true)` | `Ok(true)` |
| may send | no address | false | `Ok(false)` | `Ok(false)` | `Err(SchedulingDenied("This account has no calendar address to send scheduling messages from."))` |
| may not send | as stored | false | `Ok(false)` | `Ok(false)` | `Err(SchedulingDenied("This account is not allowed to send calendar scheduling messages."))` |
| may not send | no address | false | `Ok(false)` | `Ok(false)` | `Err(SchedulingDenied("This account has no calendar address to send scheduling messages from."))` |

In every row the `On` answer equals the handler's condition. The last
row shows the address is checked before the permission, as
`ItipSendStatus::resolve` does.

`destroy_required_denied`:
1. `id = put_event(.., "pipe-a", "jmapcal-pipe-denied", "stays")`; the
   stored event is read.
2. The token is
   `AccessToken::from_permissions(owner, []).with_session_keys(za_unlock(test, owner, STRONG))`:
   the account's keys, but no permission to send.
3. `destroy(.., id, ZaScheduling::Required)` is
   `Err(SchedulingDenied("This account is not allowed to send calendar scheduling messages."))`.
4. `uid_copies` is 1; the stored event's `version` is unchanged; the
   owner's GET of `{HOME}pipe-a/{uid}.ics` is 200.
5. `destroy(.., id, ZaScheduling::On)` with the same token is `Ok`: the
   delete is saved unsent, as a CalDAV DELETE is. `uid_copies` is 0.

Steps:

- [ ] Write `destroy`, `invitation` and the four cases; call the cases
  from `test` after Task 11's, in the order above.
- [ ] Run TM. Expected: `za_destroy_event` and `za_delete_send_itip` are
  not found.
- [ ] Implement `za_delete_send_itip` in `pipeline/schedule.rs`.
- [ ] Implement `za_destroy_event` in `pipeline/delete.rs`, and give the
  shared commit its rebuild parameter.
- [ ] Run ZA and WK. Expected: both pass. WK runs because the shared
  commit of a CalDAV DELETE gained a parameter.
- [ ] Kill-check the twin case. Fault: `za_destroy_event` passes
  `send_itip: false` to `remove_all_za_at`. Run ZA. Expected: a panic in
  `destroy_single_name_equals_dav_delete`, where the grantee's new items
  are counted (one, not two).
- [ ] The closing checks.
- [ ] Commit: `Release 6: the pipeline's destroy of a whole event`.
