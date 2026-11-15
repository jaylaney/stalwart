# Zero-access plan 17: the interface sheet (release 6)

This sheet belongs to [zero-access plan 17: JMAP calendars and browser sign-in](2026-11-14-zero-access-17-jmap-calendars.md); its tasks cite it as "I §n" and copy no signature from it.

Every task of the plan uses the names on this sheet. Line numbers are at
the commit that adds the plan, on `main` at `cc8b50a9b`; they were read at
`82b8984aa`, release 7's last commit, and the one file that differs between
the two is `tests/caldav_client/requirements.txt`. Spec sections are those of the release 6 spec, revision 6
(decisions D1 to D29, invariants 57 to 76). "dav wrapper" means `dav` keeps an item of
the same name and visibility that calls the `groupware` one and maps the
error, so handler bodies and `dav` unit tests change only where a task
says.

Parameters written `server`, `token`, `resources` without a type are
`&Server`, `&AccessToken`, `&GroupwareResources`. Ids are `u32` unless
shown. `Archive` is `Archive<ArchiveBytes>`.

The session rule (spec D19; section 12, decision 4): a key account's
OAuth sessions end only when a credential changes. Every place that
depends on it goes through `za_session_binding` (section 5.1).

## 1. Rules the sheet follows

1. **Owner of each step.** Upstream's validation steps stay in each
   protocol's handler. `groupware` holds only the fork's key-account
   steps. Section 4 assigns every step.
2. **Stages, not one call.** A CalDAV handler calls the pipeline at the
   points where its key branches stand today, in today's order. A CalDAV
   create already works this way (`crates/dav/src/calendar/update.rs:911-1120`).
   JMAP's key writer calls the same stage functions.
3. **Move only what JMAP calls.** Section 2.11 lists what stays in `dav`.
4. **One copy of fork logic.** Where JMAP and CalDAV need the same fork
   rule, the function moves and both call it. Nothing in `groupware`
   mirrors fork code left in `dav`.

Spec decisions this sheet implements: D17 (rule 1), D18 (§2.6), D19 to
D23 (§5), D24 (§5.5), D25 (§6), D12 (§3.2), D15 (§2.4), D11 (§3.1).

## 2. The pipeline: `crates/groupware/src/calendar/pipeline/`

New module, declared `pub mod pipeline;` in
`crates/groupware/src/calendar/mod.rs`. Files: `mod.rs`, `error.rs`,
`admission.rs`, `context.rs`, `uid.rs`, `schedule.rs`, `create.rs`,
`update.rs`, `placement.rs`, `delete.rs`. Every file starts with the SPDX
header.

### 2.1 `mod.rs`

```rust
/// Whether a write schedules, and whether a denied status fails it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZaScheduling {
    /// No status is resolved, nothing is built, statuses stay untouched.
    /// CalDAV: a DELETE with `Schedule-Reply: F`. JMAP: `sendSchedulingMessages` false or absent.
    Off,
    /// Resolve; a denied status is traced and the write saved. CalDAV; JMAP `Calendar/set` destroy (D16).
    On,
    /// Resolve; a denied status returns `SchedulingDenied` before any write. JMAP with `sendSchedulingMessages: true`.
    Required,
}

/// What a committed event write returns.
pub struct ZaWritten {
    pub document_id: u32,
    /// `AssignedIds::last_change_id(owner, SyncCollection::Calendar)` of the commit that stored the event.
    pub change_id: u64,
    pub etag: String,
    pub schedule_tag: Option<u32>,
    /// The plaintext tree as committed: after the fan-out's statuses and sequences, before sealing.
    pub ical: ICalendar,
}

pub struct ZaDeleted {
    /// The assigned Calendar change id, or 0 for an assertion-only CalDAV no-op
    /// when a concurrent unlink already removed the addressed parent.
    pub change_id: u64,
}
```

### 2.2 `error.rs`: the neutral error

```rust
#[derive(Debug)]
pub enum ZaPipelineError {
    Forbidden(ZaRefusal),
    /// A shared calendar is not the event's only calendar (`ZaCalendarError::LinkedEvent`).
    LinkedEvent,
    NotFound,
    Busy(ZaBusy),
    /// `href` is `None` when the conflict is an event the caller may not see.
    UidConflict { href: Option<String> },
    /// The fan-out's messages reach `itip_outbound_max_recipients`.
    TooManyRecipients,
    /// Only for an error `za_itip_fails_write` accepts.
    Scheduling(ItipError),
    /// `ZaScheduling::Required` met a denied status; the text is `ItipSendStatus::reason()`.
    SchedulingDenied(&'static str),
    /// Everything else, carried unchanged: store, seal (`seal_error`), `Security`, assertion and quota errors.
    Store(trc::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZaRefusal { NoKeys, Context, SharedCopy, LinkedMove }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZaBusy { LeaseBusy, CreateLost }

impl From<trc::Error> for ZaPipelineError; // Store(error)

impl ZaPipelineError {
    /// Write scope (`dav/src/calendar/mod.rs:762-778`): `Forbidden` -> `Forbidden(Context)`,
    /// `LinkedEvent` -> `LinkedEvent`, `Store(NotFound)` -> `NotFound`, else `Store(error.into_store_error(owner_id, document_id))`.
    pub fn from_calendar(error: ZaCalendarError, owner_id: u32, document_id: u32) -> Self;
}

/// Read scope (`dav/src/common/za.rs:369-385`): a refusal fails the request (`Err`), any other error fails one item (`Ok`).
pub fn za_read_refusal(error: ZaCalendarError, owner_id: u32, document_id: u32) -> Result<trc::Error, ZaPipelineError>;

/// Whether a scheduling error fails the write. Mirrors upstream's `ItipPrecondition::failed_precondition`
/// (`dav/src/calendar/mod.rs:736-759`) with a match that names every `ItipError` variant, so a new one stops the build.
pub fn za_itip_fails_write(error: &ItipError) -> bool;
```

No blanket `From<SealError>`: every seal error goes through
`seal_error(err, account, document)`. Producers emit their traces where
they do today, with the same `Details` strings; a mapping adds none.


**CalDAV mapping**: `impl From<ZaPipelineError> for DavError` in
`crates/dav/src/calendar/mod.rs`. Every answer is today's.

| Variant | `DavError` |
|---|---|
| `Forbidden(_)` | `Code(FORBIDDEN)` |
| `LinkedEvent` | `Code(CONFLICT)` |
| `NotFound` | `Code(NOT_FOUND)` |
| `Busy(_)` | `Code(SERVICE_UNAVAILABLE)`, no `Retry-After` |
| `UidConflict { href: Some(h) }` | `Condition(FORBIDDEN, CalCondition::NoUidConflict(h))` |
| `UidConflict { href: None }` | `Code(FORBIDDEN)` |
| `TooManyRecipients` | `Condition(PRECONDITION_FAILED, CalCondition::MaxAttendeesPerInstance)` |
| `Scheduling(err)` | `Condition(PRECONDITION_FAILED, err.failed_precondition())` with details `err.to_string()` |
| `SchedulingDenied(_)` | `Code(FORBIDDEN)`; CalDAV never passes `Required` |
| `Store(e)` | `Internal(e)`: the dispatcher's arms decide (quota 507, assertion 503 with `Retry-After`, `Security` 403, else 500) |

**JMAP mapping**: `za_event_set_error` (section 7.3).

| Variant | JMAP answer |
|---|---|
| `Forbidden(_)`, `LinkedEvent` | set error `forbidden` |
| `NotFound` | set error `notFound` |
| `Busy(_)` | set error `forbidden`, "Another process modified this calendar event, please try again." (`jmap/src/calendar_event/set.rs:1611`) |
| `UidConflict { .. }` | `invalidProperties` on `uid`, "An event with UID {uid} already exists." (`jmap/src/calendar_event/mod.rs:209`) |
| `TooManyRecipients` | `invalidProperties` on `participants`, upstream's text (`set.rs:1126-1131`) |
| `Scheduling(err)` | `invalidProperties` on `participants`, `err.to_string()` |
| `SchedulingDenied(reason)` | set error `forbidden`, `reason` |
| `Store(e)`, assertion failure | set error `forbidden`, the "Another process modified" text |
| `Store(e)`, `Limit(Quota \| TenantQuota)` | `overQuota` |
| `Store(e)`, `Security(Unauthorized)` | set error `forbidden` |
| `Store(e)`, anything else | a store failure (§7.5): this object and every later object of the call answer `za_set_incomplete()`; the method does not fail (D28) |

### 2.3 `admission.rs`

```rust
/// The body of `za_calendar_gate` (`dav/src/common/za.rs:96-115`), same trace. `Ok` for an ordinary owner.
pub async fn za_calendar_admission(server, token, owner_id: u32) -> Result<(), ZaPipelineError>;
/// `dav/src/calendar/mod.rs:833-847`. `Ok(None)` for an ordinary owner; `Forbidden(NoKeys)` for a key owner
/// when the token lacks its own unimpersonated keys.
pub async fn za_writer_keys(server, token, owner_id: u32) -> Result<Option<Arc<SessionKeys>>, ZaPipelineError>;
pub struct ZaRoles { pub sender: AccountInfo, pub writer: AccountInfo, pub owner: AccountInfo }
/// `dav/src/calendar/mod.rs:859-889`: the sender is the writer with the owner's addresses added.
pub async fn za_roles(server, token, owner_id: u32) -> Result<ZaRoles, ZaPipelineError>;
```

dav wrappers: `ZeroAccessGate::za_calendar_gate`, `za_writer_keys`,
`za_roles`; `ZaRoles` is re-exported.

### 2.4 `context.rs`

```rust
pub type ZaReadMemo = HashMap<(u32, u32), Arc<ZaCalendarContext>>;
pub async fn za_mutation_context(server, token, owner_id: u32, calendar_id: u32, required: Bitmap<Acl>)
    -> Result<Option<ZaCalendarContext>, ZaCalendarError>;
pub async fn za_read_context(server, token, owner_id: u32, calendar_id: u32, required: Bitmap<Acl>, memo: &mut ZaReadMemo)
    -> Result<Option<Arc<ZaCalendarContext>>, ZaCalendarError>;
pub fn za_event_view_in(context: &ZaCalendarContext, meta: &ArchivedCalendarEvent, content: &Archive, owner_id: u32, document_id: u32)
    -> trc::Result<ZaEventView>;
/// Outer `Err`: a refusal that fails the request. Inner `Err`: fails one item. `Ok(Ok(None))`: an ordinary account.
pub async fn za_read_event_context(server, token, owner_id: u32, document_id: u32, calendar_id: u32, memo: &mut ZaReadMemo)
    -> Result<trc::Result<Option<Arc<ZaCalendarContext>>>, ZaPipelineError>;
pub async fn za_read_event_view(server, token, owner_id: u32, document_id: u32, calendar_id: u32,
    meta: &ArchivedCalendarEvent, content: &Archive, memo: &mut ZaReadMemo)
    -> Result<trc::Result<Option<ZaEventView>>, ZaPipelineError>;
pub async fn za_read_calendar_view(server, token, owner_id: u32, calendar_id: u32, memo: &mut ZaReadMemo)
    -> Result<trc::Result<Option<(Archive, Arc<ZaCalendarContext>)>>, ZaPipelineError>;
/// Every parent of `event`, each with `required` (`dav/src/calendar/mod.rs:780-805`).
pub async fn za_write_contexts(server, token, owner_id: u32, event: &CalendarEvent, addressed: u32, required: Acl)
    -> Result<Vec<ZaCalendarContext>, ZaPipelineError>;
pub async fn za_refresh_event(server, token, owner_id: u32, addressed: u32, required: Acl,
    before: &[ZaCalendarContext], sealed: &mut ZaEvent) -> Result<Option<Vec<ZaCalendarContext>>, ZaPipelineError>;
/// `foreign_calendar_contexts` (`dav/src/common/propfind/za.rs:458-491`) without its test hook: the calendars of
/// `calendar_ids` (restricted to `requested`) that a fresh `Read` context admits. Refusals and deleted calendars are left out.
pub async fn za_foreign_calendar_contexts(server, token, owner_id: u32, calendar_ids: impl Iterator<Item = u32>,
    requested: Option<u32>) -> Result<BTreeMap<u32, Arc<ZaCalendarContext>>, ZaPipelineError>;
```

dav wrappers keep their names and `crate::Result` shapes.
`calendar_item_error` and `za_calendar_error` stay as wrappers over
`za_read_refusal` and `from_calendar`. dav's `foreign_calendar_contexts`
wrapper keeps the `propfind_admitted_pause_point` hook, so the hook does
not fire on JMAP.

**D15 predicate** (spec 6.8), in
`crates/groupware/src/calendar/sharing/resolve.rs`:

```rust
impl ZaSyncBoundary {
    /// CalDAV's rule (`dav/src/common/propfind/get.rs:159-162`): a boundary still pending, or a token at or before it.
    pub fn refuses(&self, since: u64) -> bool; // self.pending || since <= self.grant_change_id.max(self.rename_change_id)
}
```


### 2.5 `uid.rs`

```rust
/// Takes the UID creation lease, then checks the UID in every calendar of `calendar_ids` against current archives.
/// A busy lease is `Busy(LeaseBusy)` with today's trace; a hit releases the lease and returns `UidConflict`.
pub async fn za_take_uid_lease(server, resources, token, owner_id: u32, calendar_ids: &[u32], uid: &str)
    -> Result<ZaUidLeaseGuard, ZaPipelineError>;
/// One calendar (`dav/src/calendar/mod.rs:532-598`).
pub async fn za_assert_is_unique_uid(server, resources, token, owner_id: u32, calendar_id: u32, uid: &str)
    -> Result<(), ZaPipelineError>;
pub async fn za_lost_create_fence(server, resources, token, lease: ZaUidLeaseGuard, owner_id: u32,
    calendar_ids: &[u32], uid: &str) -> Result<Infallible, ZaPipelineError>;
/// The bounded second create commit; returns the commit's change id.
pub async fn za_commit_create(server, resources, token, batch: BatchBuilder, lease: ZaUidLeaseGuard, owner_id: u32,
    calendar_ids: &[u32], uid: &str) -> Result<u64, ZaPipelineError>;
```

`CreateLoss` and `za_finish_create_loss` move as private items. CalDAV
passes one-element slices. Hooks `commit_pause_point` and
`take_commit_fail` move with `za_commit_create`.

### 2.6 `schedule.rs`

```rust
pub struct ZaScheduled {
    pub messages: Option<Vec<ZaFanOutMessage>>,
    pub resolution: Option<ZaDelegationResolution>,
}

/// `dav/src/calendar/update.rs:1232-1324`, unchanged logic.
pub fn za_schedule(server, roles: &ZaRoles, resolution: Option<&ZaDelegationResolution>, old: Option<&ICalendar>,
    ical: &mut ICalendar, policy: RecipientPolicy, schedule_tag: &mut Option<u32>, owner_id: u32,
    document_id: Option<u32>) -> Result<Option<Vec<ZaFanOutMessage>>, ZaPipelineError>;
/// `update.rs:1341-1351`.
pub fn za_second_run(result: Result<Option<ZaSelfMessages>, ItipError>) -> Result<Option<ZaSelfMessages>, ZaPipelineError>;

/// The key arm of a create's scheduling (`update.rs:1004-1038` and its `reason` trace at `:1078-1084`).
/// `content.data` must be built. Mutates the tree's statuses and `event.schedule_tag`.
pub async fn za_schedule_create(server, token, roles: &ZaRoles, schedule: ZaScheduling, event: &mut CalendarEvent,
    content: &mut CalendarEventContent, owner_id: u32) -> Result<ZaScheduled, ZaPipelineError>;
/// The key arm of an update's scheduling (`update.rs:498-543` and its `reason` trace at `:619-626`).
/// `old` is the tree before the update; `None` when the handler found a change of user data only.
pub async fn za_schedule_update(server, token, roles: &ZaRoles, schedule: ZaScheduling, old: Option<&ICalendar>,
    new_event: &mut CalendarEvent, new_content: &mut CalendarEventContent, owner_id: u32, document_id: u32)
    -> Result<ZaScheduled, ZaPipelineError>;
/// CalDAV's condition for a delete (`dav/src/calendar/delete.rs:99-102`), with the owner's `AccountInfo`:
/// `Off` -> false; `On` -> the condition; `Required` -> the condition, or `SchedulingDenied` when it is false.
pub fn za_delete_send_itip(server, token, owner: &AccountInfo, schedule: ZaScheduling) -> Result<bool, ZaPipelineError>;
```

The status inside both schedule functions: `Off` gives
`ItipSendStatus::NotRequested`; `On` and `Required` give
`ItipSendStatus::resolve(server, token, &roles.sender,
content.data.event_range_end(), true)`. `SendSealed` runs
`za_resolve_delegation`, `za_schedule` and
`itip_set_unreachable_status(.., roles.sender.addresses())`. With
`Required`, `is_denied()` returns `SchedulingDenied(reason)`. Any other
reason is traced as today (`ItipMessageError`, same keys).


The dav unit test `second_run_fails_only_on_a_failed_precondition`
(`update.rs:1604-1641`) stays in `dav` and wraps the call in
`DavError::from`.

### 2.7 `create.rs`

```rust
pub struct ZaEventCreate {
    /// `names` holds every calendar the event is created in; `schedule_tag` as `za_schedule_create` left it.
    pub event: CalendarEvent,
    /// `data` built with `CalendarEventData::new_for(.., true)`; no preferences.
    pub content: CalendarEventContent,
    pub next_alarms: EventAlarms,
    pub scheduled: ZaScheduled,
    pub lease: ZaUidLeaseGuard,
    pub uid: String,
    /// From `za_write_contexts(.., names[0].parent_id, Acl::AddItems)`.
    pub contexts: Vec<ZaCalendarContext>,
}

/// `za_create_event` (`update.rs:1375-1602`): the reservation commit, the fan-out fenced on the lease,
/// sealing in `contexts[0].domain`, the insert asserting the lease and every context, one retry, the mail guard.
pub async fn za_create_event(server, token, resources, account: &AccountCache, keys: &SessionKeys, roles: &ZaRoles,
    create: ZaEventCreate) -> Result<ZaWritten, ZaPipelineError>;
```

Differences from today's private function, all behaviour-neutral for
CalDAV:

- `lease` is required. The `lease == None` branch (`update.rs:1589-1593`)
  is unreachable today and is removed, so no caller can skip the lease.
- `submitted_ical`, `is_owner` and `personal_id` leave the struct. The
  function returns `ZaWritten.ical`, and `dav` computes
  `serves_submitted_ical` from it with today's expression
  (`update.rs:1473-1479`).
- The calendars of the lease re-check and of the retry are
  `event.names`' parents; the retry's addressed calendar is
  `names[0].parent_id`.

### 2.8 `update.rs`

```rust
pub struct ZaUpdateTarget {
    /// Every parent, `Acl::ModifyItems` (`update.rs:222-236`).
    pub contexts: Vec<ZaCalendarContext>,
    /// Opened in `contexts[0].domain` (`update.rs:237-249`).
    pub view: ZaEventView,
}
/// CalDAV's first stage: called where `update.rs:222-256` runs, before the header preconditions.
pub async fn za_update_target(server, token, owner_id: u32, document_id: u32, addressed: u32, event: &CalendarEvent,
    content: &Archive) -> Result<ZaUpdateTarget, ZaPipelineError>;

pub struct ZaEventUpdate<'x> {
    pub document_id: u32,
    /// The calendar a retry resolves from: the PUT's parent; for JMAP the first calendar the event stays in or enters.
    pub addressed: u32,
    pub stored_event: &'x Archive,
    pub stored_content: &'x Archive,
    /// With the names after the update, and `schedule_tag` as `za_schedule_update` left it.
    /// With a plan, `names` holds the kept names and a new name for each entered calendar.
    pub new_event: CalendarEvent,
    /// `data` built with `new_for(.., true)` after `za_sequence_floor`; no preferences.
    pub new_content: CalendarEventContent,
    /// From the view: the bundle keeps the name (ruling 15).
    pub display_name: Option<String>,
    pub prev_alarms: EventAlarms,
    pub next_alarms: EventAlarms,
    pub scheduled: ZaScheduled,
    pub contexts: ZaPlacementContexts,
    /// `Some` when the update changes the event's calendars (JMAP). `None` for a CalDAV PUT.
    pub placement: Option<ZaPlacementPlan>,
}

/// The key commit of an update (`update.rs:701-828`): the stored pairs opened in the old domain, the fan-out
/// (`ZaFanOutKind::Change`), statuses and sequences, sealing in the new domain, `update_full_at` asserting every
/// context, `replace_event_alarms`, the hooks, one retry after a calendar change, the mail guard. With `placement`:
/// section 2.9.
pub async fn za_commit_update(server, token, resources, account: &AccountCache, keys: &SessionKeys, roles: &ZaRoles,
    update: ZaEventUpdate<'_>) -> Result<ZaWritten, ZaPipelineError>;
```

For Task 11, `CalendarEvent::enqueue_full_at` is a `pub(crate)` helper
in `calendar/storage.rs`, with the same arguments and return value as
`update_full_at`. It contains that method's existing body except the
terminal `batch.commit_point()`. `update_full_at` calls the helper,
then takes its existing commit point and returns the same ETag. Every
existing caller keeps that wrapper. Only `placement: Some(_)` uses the
enqueue helper, on both attempts. Its context assertions, event,
container, alarm tasks and vanished logs remain in one physical batch;
no commit point is added at the end. Batch finalization supplies the
single write. This adds no size refusal or store-wide behavior change.

Hooks `update_commit_pause_point` and `take_update_commit_fail` move with
the key commit. The plain commit and its own `update_commit_pause_point`
stay at `crates/dav/src/calendar/update.rs` (`:829-862`): a test pins
that file name (`tests/src/za/sharing_review.rs:738-755`).


### 2.9 `placement.rs`

One implementation resolves the contexts of a placement change for both
protocols (spec 5.1, 5.2). It is `za_move_contexts`
(`dav/src/calendar/copy_move.rs:1918-1993`) moved and generalised.
CalDAV's event COPY and MOVE call it; nothing mirrors it.

```rust
/// A change of an event's calendars.
pub struct ZaPlacementPlan {
    /// The event's calendars now: the parents of its stored names, without duplicates, ascending.
    pub parents: Vec<u32>,
    /// How many names the stored event has. CalDAV's test for a linked event counts names.
    pub names: usize,
    /// The calendar the request addresses, resolved first. It must be one of `parents`, or the answer is
    /// `NotFound`. CalDAV: the source of the COPY or MOVE. JMAP: `None`.
    pub source: Option<u32>,
    /// The calendars the event leaves. CalDAV MOVE: the source. CalDAV COPY: none.
    pub left: Vec<u32>,
    /// The calendars the event enters, resolved last, in this order. CalDAV: the target, which may be the
    /// source (a rename, or a copy within one calendar).
    pub entered: Vec<u32>,
    /// JMAP: the update also changes the content.
    pub content_changed: bool,
    /// CalDAV: the request overwrites a resource in the target.
    pub overwrite: bool,
    /// CalDAV: the event is copied or moved as part of a collection.
    pub collection: bool,
}
impl ZaPlacementPlan {
    /// The inputs `za_move_contexts` takes today.
    pub fn caldav(event: &CalendarEvent, source: u32, target: u32, is_move: bool, overwrite: bool,
        collection: bool) -> Self;
    /// From the stored names and the names after the update.
    pub fn jmap(old: &[DavName], new: &[DavName], content_changed: bool) -> Self;
    /// Whether the event leaves or enters any calendar.
    pub fn is_change(&self) -> bool;
    /// The rights asked on every current calendar.
    pub fn parent_rights(&self) -> Bitmap<Acl>;
    /// The rights asked on every entered calendar.
    pub fn entered_rights(&self) -> Bitmap<Acl>;
    /// The refusal for a shared current calendar (`entered` false) or a shared entered calendar (`entered` true).
    pub fn shared_refusal(&self, entered: bool) -> Option<ZaRefusal>;
}

pub struct ZaPlacementContexts {
    /// One context per current calendar: the source first when the plan names one, the rest in ascending id.
    pub parents: Vec<ZaCalendarContext>,
    /// One context per entered calendar, in the plan's order. A calendar that is also a parent has its own entry.
    pub entered: Vec<ZaCalendarContext>,
}
impl ZaPlacementContexts {
    /// A CalDAV PUT, or a JMAP update that keeps the event's calendars.
    pub fn unchanged(contexts: Vec<ZaCalendarContext>) -> Self;
    /// `parents[0].domain`: the domain the stored event is sealed in.
    pub fn from_domain(&self) -> &ZaKeyDomain;
    /// The last entered calendar's domain, or `from_domain` when none is entered.
    pub fn to_domain(&self) -> &ZaKeyDomain;
    pub fn assert_in(&self, batch: &mut BatchBuilder);
    /// Parents, then entered: the order CalDAV's callers index (first the source, last the target).
    pub fn into_vec(self) -> Vec<ZaCalendarContext>;
}

/// Fresh contexts for a placement change, each through `za_mutation_context`.
pub async fn za_placement_contexts(server, token, owner_id: u32, plan: &ZaPlacementPlan)
    -> Result<ZaPlacementContexts, ZaPipelineError>;

/// Moved from `copy_move.rs:1898-1913`, same traces.
pub fn za_shared_copy_refusal(owner_id: u32) -> ZaPipelineError; // Forbidden(SharedCopy)
pub fn za_linked_move_refusal(owner_id: u32) -> ZaPipelineError; // Forbidden(LinkedMove)

/// The event's dead-property container, opened in `from` and sealed in `to` under a fresh data key, queued in
/// `batch`. Nothing is queued when `kinds` is empty or no container is stored.
pub async fn za_reseal_event_container(server, account: &AccountCache, document_id: u32, uid: &str,
    kinds: MetadataKinds, from: &ZaKeyDomain, to: &ZaKeyDomain, batch: &mut BatchBuilder) -> trc::Result<()>;
```

**The rule**, which is today's for every CalDAV input:

- **Rights.** `parent_rights` is `RemoveItems` when `left` is not empty,
  else `ReadItems`; plus `ModifyItems` when `content_changed`.
  `entered_rights` is `AddItems`; plus `RemoveItems` when `overwrite`.
- **Shared calendars.** Let `strict` be `left.is_empty() || collection`.
  A shared current calendar: `SharedCopy` when strict, else `LinkedMove`
  when `names != 1`. A shared entered calendar: `SharedCopy` when strict,
  else `LinkedMove` when `names != 1` or `entered.len() != 1`.
- **Order.** The source: resolved, then `NotFound` unless it is a parent,
  then its shared check. The other parents in ascending id, each
  resolved and then checked. The entered calendars in the plan's order,
  each resolved and then checked. A resolver's error is
  `from_calendar(error, owner_id, calendar)`; no context is
  `Forbidden(Context)`.

A JMAP plan that only leaves calendars asks `RemoveItems` on the
calendars the event stays in, where CalDAV's DELETE of one name asks
nothing of them. No caller can tell the difference: only an owner holds
an event in several calendars, and an owner has every right.

**CalDAV's callers.** `za_copy_move_stored_event`
(`copy_move.rs:2159-2169` and its retry at `:2371-2381`) calls
`za_placement_contexts(.., &ZaPlacementPlan::caldav(..))?.into_vec()` and
keeps indexing the vector as today. `za_move_contexts` and
`linked_move_refusal` leave `dav`. `shared_copy_refusal` stays as a
wrapper for the collection paths (`copy_move.rs:2459` and eight more).

**JMAP.** With a plan, `za_commit_update` also:

- checks the UID in each entered calendar with `za_assert_is_unique_uid`;
- seals in `to_domain`, and when the domain changes calls
  `za_reseal_event_container`;
- writes one `log_calendar_vanished(calendar_id, href)` per left name
  whose parent is present in `resources`. The href uses that attempt's
  matching `contexts.parents` stored `Calendar.name`, not the request
  snapshot's parent path. Canonicalize both the calendar and item name
  with `common::storage::dav::canonical_calcard_segment`, join them
  with `/`, and pass the result to `resources.format_item`. This is the
  cache's path spelling, fenced by the same calendar archive asserted
  in the batch; rebuilding after a rename uses the renamed path;
- on its retry runs `za_placement_contexts` again and rewraps if any
  calendar changed.

**The smallest move for the container** (spec 5.1: the code that reads and writes
dead-property containers stays in `dav`): nothing leaves `crates/dav/src/common/dead/`.
`ContainerWrites` and `read_container` are upstream's code with
enterprise snippets and stay. `za_reseal_event_container` is new fork
code built from pieces `groupware` can already call:
`Server::metadata_container`
(`common/src/storage/metadata/read.rs:105`), `reseal_container`
(`groupware/src/calendar/seal/container.rs:310`),
`ContainerChange::new(..).into_write(account, Collection::CalendarEvent,
MetadataLog::None)` (`common/src/storage/metadata/change.rs:17-82`, which
fills the tenant itself) and `MetadataWrite::build`. It needs no dead
fence: a sealed write of a key account has none
(`dav/src/common/za.rs:555-577`). The CalDAV event move
(`copy_move.rs:2272-2292`) calls it in place of its inline reseal, so one
implementation serves both.

### 2.10 `delete.rs`

```rust
/// One name of an event (`dav/src/calendar/delete.rs:368-432` and the key parts of `:451-600`): the `RemoveItems`
/// context, `delete_za_at` in its domain, the fan-out (`ZaFanOutKind::Deletion`), the hooks, one retry, the mail
/// guard. `content` is the stored, sealed archive or `None`, as the handler read it.
pub async fn za_delete_event(server, token, resources, keys: &SessionKeys, roles: &ZaRoles, owner_id: u32,
    document_id: u32, calendar_id: u32, event: Archive, content: Option<Archive>, href: Option<String>,
    send_itip: bool) -> Result<ZaDeleted, ZaPipelineError>;

/// Every name of an event (JMAP destroy): every parent resolved with `RemoveItems`, `remove_all_za_at`
/// (`groupware/src/calendar/storage.rs:991`), one vanished href per name, then the same commit as `za_delete_event`.
pub async fn za_destroy_event(server, token, resources, keys: &SessionKeys, roles: &ZaRoles, owner_id: u32,
    document_id: u32, event: Archive, send_itip: bool) -> Result<ZaDeleted, ZaPipelineError>;

pub struct ZaCalendarDeleteTarget {
    /// `None` for an ordinary account.
    pub context: Option<ZaCalendarContext>,
    /// The repaired stored calendar; the caller's ETag precondition reads it.
    pub stored: Archive,
    pub deleted_grantees: Vec<u32>,
}
/// `delete.rs:163-191`, for every account: `za_mutation_context(.., Delete | RemoveItems)`.
pub async fn za_calendar_delete_target(server, token, owner_id: u32, calendar_id: u32, stored: Archive)
    -> Result<ZaCalendarDeleteTarget, ZaPipelineError>;
/// The key parts of `delete.rs:211-245,265-270,451-600`: `delete_with_events_za`, the default-calendar clear,
/// the fan-out, the commit, the grantees' invalidation, the mail guard. A grantee's keys are refused
/// (`storage.rs:505-511`): `Store` with a `Security` error.
pub async fn za_delete_calendar(server, token, resources, keys: &SessionKeys, roles: &ZaRoles, owner_id: u32,
    calendar_id: u32, target: ZaCalendarDeleteTarget, children: Vec<u32>, cleanup: &ContainerCleanup,
    href: Option<String>, send_itip: bool) -> Result<ZaDeleted, ZaPipelineError>;
```

Callers compute `send_itip` with `za_delete_send_itip`. Hooks:
`delete_commit_pause_point` fires for an event delete only and
`take_delete_commit_fail` for both, as today (`delete.rs:509-520`).
`za_delete_event` keeps passing the sealed content archive to
`delete_za_at`.

`za_destroy_event` builds one unsplit batch on each attempt. Its existing
`remove_all_za_at` path, alarm clears and shared-container release enqueue
without a commit point; the whole-event caller adds none after its logs.
Finalization supplies one physical write, avoiding a terminal write that
could fail after deletion. Existing CalDAV callers keep their boundaries.
For each stored name whose parent exists in `resources`, derive its href
from the attempt's matching asserted context's stored `Calendar.name`
and the item name, canonicalizing both with `canonical_calcard_segment`
and using `resources.format_item` for the base path (the Task 11 rule).
A rename retry rebuilds that href; `za_delete_event` retains its supplied
request href. No storage-helper API or body changes are needed here.

### 2.11 What stays in `dav`

- Every upstream validation step of PUT, DELETE, COPY, MOVE, MKCOL and
  PROPPATCH, in place.
- `za_uri_gate`, `za_session_keys`, `za_freebusy_access`,
  `za_refuse_cross_account`, `za_calendar_report_uri`, `ZaFreeBusy`,
  `za_freebusy_views`, the inbox helpers, `is_key_account`,
  `za_ordinary_fence`, `za_dead_fence`, `za_dead_fenced`, `ZaDeadSeal`,
  `DeadPatch`, `za_open_container`, `za_reseal_container`,
  `ContainerWrites`, `read_container`.
- `za_copy_move_event`, `za_copy_move_stored_event` (which call
  `za_placement_contexts`), the overwrite fences, `shared_copy_refusal`
  as a wrapper, every collection copy, move and rename path, the
  event and calendar PROPPATCH branches, MKCOL's seal, `validate_ical`,
  `assert_is_unique_uid`, `SupportedComponents`, `ItipPrecondition`.
- Hooks that fire on plain paths: `put_pause_point`, the plain
  `update_commit_pause_point`, `calendar_proppatch_read_pause_point`,
  `share_rename_pause_point`, `copy_pause_point`, the move plans,
  `calendar_sync_log_pause_point`, `propfind_admitted_pause_point`.
- The checks under `cfg(not(any(feature = "dev_mode", feature =
  "test_mode")))` (`delete.rs:117`, `copy_move.rs:108`).
- `za_test_move_event` (`mod.rs:893-925`): no caller; left as is.

### 2.12 Features

`crates/dav/Cargo.toml` and `crates/jmap/Cargo.toml`: `test_mode` gains
`"groupware/test_mode"`. `groupware` gains no dependency.

### 2.13 Notes from the plan's drafting

- §2.1: `ZaWritten` and `ZaDeleted` derive nothing. `ZaWritten` holds the
  plaintext tree, so it gets no `Debug`.
- §2.2, CalDAV table: a `Scheduling(err)` whose `failed_precondition()` is
  `None` is unreachable (`itip_precondition_classes_agree`) and maps to
  `Code(PRECONDITION_FAILED)`. `TooManyRecipients` carries no details.
- §2.4: `dav`'s wrapper `za_refresh_event` goes in Task 7, when its last
  caller leaves.
- §2.5: `za_take_uid_lease` checks the calendars in slice order; the first
  conflict decides the answer.
- §2.6: `za_schedule_update` with `old == None` gives `NotRequested`,
  whatever `schedule` is. `za_delete_send_itip`'s condition is
  `!status.is_denied()` for `ItipSendStatus::resolve(server, token, owner,
  i64::MAX, true)`; with `Required` the denied reason is that status's, in
  `resolve`'s order: disabled, no address, no permission.
- §2.7: an empty `contexts` releases the lease and answers
  `Forbidden(Context)`.
- §2.8: `za_commit_update` takes one timestamp, in the commit;
  `ZaEventUpdate` carries none. `za_update_target` answers
  `Forbidden(Context)` for an event row with no name. Until Task 11,
  `placement: Some(_)` returns a store error with the details
  `zero-access: placement change not implemented`.
- §2.9: `parents` and `left` are ascending; `entered` is in the order of
  the new names, without duplicates. `ZaPlacementPlan::jmap`'s
  `content_changed` is whether the shaped tree differs from the stored
  one. A private `steps(&self) -> Vec<(u32, bool)>` gives the resolution
  order; `za_placement_contexts` iterates it. CalDAV's caller changes at
  four sites: the two context calls, the inline reseal, and the
  `read_container` call at `copy_move.rs:2202-2210`, which goes. The
  reseal passes the resealed `EncodedMetadata` itself.
- §2.10: `za_destroy_event` resolves with `za_write_contexts(..,
  names[0].parent_id, Acl::RemoveItems)`. A name whose calendar is not in
  the resource snapshot logs nothing. The shared commit of the three
  deletes takes its rebuild as a parameter. For a key account
  `ZaCalendarDeleteTarget.stored` is a clone of the context's archive.

## 3. Other new `groupware` modules

### 3.1 `crates/groupware/src/calendar/za_alerts.rs` (D11)

```rust
/// Replaces the alarms of every VEVENT and VTODO of `ical` with the writer's current default alerts of
/// `calendar_ids`, chosen by `ical.shows_without_time()`. Removes the former alarm components with
/// `ICalendar::remove_component_ids`. Each alarm is `DefaultAlert::to_ical(Some(text))`: the description is the
/// component's SUMMARY or "Reminder", an email alert's recipient is the writer's account name.
pub async fn za_apply_default_alerts_by_value(server, resolver: &mut DefaultAlertsResolver, owner_id: u32,
    writer_id: u32, calendar_ids: &[u32], ical: &mut ICalendar) -> trc::Result<()>;
```

`writer_id` is `access_token.personal_id(owner_id, Collection::Calendar)`.
A second function, `za_replace_alarms(ical, alerts, recipient) -> bool`, is private (Task 29).

### 3.2 `crates/groupware/src/calendar/za_text.rs` (D12)

```rust
/// The text of a set of events as the internal search store would hold it, built in memory.
pub struct ZaEventTextIndex { /* store::za_search::content::ContentPart; the Calendar index fields; the default language */ }
impl ZaEventTextIndex {
    /// Reads `core.email.index_fields[&SearchIndex::Calendar]` and `core.email.default_language`.
    /// Without index fields no document is indexed and every text condition matches nothing.
    pub fn new(server: &Server) -> Self;
    /// `view.index_document(..)`, then `ContentRecord::new(document, document_id, SortKeys::default(), None, false)`.
    pub fn insert(&mut self, account_id: u32, document_id: u32, view: &ArchivedCalendarEventContent);
    /// `ContentPart::resolve`: every `SearchFilter::Text` becomes the `DocumentSet` of the inserted events that match.
    /// A query past the store's leaf limits is the store's error.
    pub fn resolve(&self, filters: &[SearchFilter]) -> trc::Result<Vec<SearchFilter>>;
}
```


## 4. Who owns each step of a write

"Handler" means the protocol's own code: CalDAV's handler in `dav`,
untouched, and for JMAP the key writer calling upstream JMAP's function.

| Step | CalDAV | JMAP key writer |
|---|---|---|
| Parsing, size and attachment limits | handler (`update.rs:118-160`) | handler: upstream's size cap, 6.5's blob refusal |
| Privacy access and privacy allowed | handler | handler (`privacy.rs`) |
| Cache ACL checks | handler | handler, with upstream's descriptions |
| Fresh access rules | pipeline: contexts | pipeline: contexts |
| Preconditions | handler (`If-Match`, `If-Schedule-Tag-Match`) | handler (`ifInState`) |
| Object shape (one UID, one type) | handler (`validate_ical`) | handler: the conversion and the generated UID |
| UID unchanged on update | handler (`update.rs:311-317`) | handler (`set.rs:933-953`) |
| UID unique on create | pipeline: lease | pipeline: lease, in every named calendar |
| UID unique in an entered calendar | handler (`copy_move.rs:2176-2188`) | pipeline: `za_commit_update` with a plan |
| Supported components | handler (`update.rs:318-320,934-936`, `copy_move.rs:2189-2193`) | none: upstream JMAP has no such check for any account. Recorded as an inherited limitation (spec 3.4); the pipeline adds none. |
| Default alerts | handler: strip and restore, a no-op without preferences | fork: `za_apply_default_alerts_by_value` (D11) |
| User-data limits | handler (`update.rs:390-395,888`) | handler: `validate_user_data`, `validate_user_data_changes` |
| Per-user split | skipped for a key account | skipped (D10) |
| Grantee's change check | handler (`update.rs:397-427`) | handler: `EventChanges::assert_allowed` (`set.rs:955-977`) |
| Organizer, SEQUENCE increment, stamps, participants limit | none | handler, with the owner's `AccountInfo` as upstream |
| SEQUENCE floor | handler calls `za_sequence_floor` (`update.rs:469-471`) | handler calls it after the increment |
| Event data | handler: `new_for(.., true)` | handler: `new_for(.., true)` in place of `new_with_default_alerts` |
| Previous and next alarms | handler | handler; next alarms over the new calendars |
| Object-count quota | handler (`update.rs:939-942`) | handler (`set.rs:237-244,288-291`) |
| Tenant quota (enterprise) | handler (`update.rs:629-641,1086-1095`) | none in upstream JMAP |
| Disk quota | pipeline: the commit's `limit_disk_quota` | handler's pre-check, then the commit |
| Scheduling status, builders, second run, recipient cap, unreachable statuses | pipeline | pipeline |
| Fan-out, statuses, sequences, pairs, sealing, commit, retry, mail | pipeline | pipeline |
| Direct-change notification, `ItipMessages::queue` | never for a key account | never (invariant 57) |
| Response | handler | handler: upstream's assembly from `ZaWritten` |

Every step has an owner. Supported components is CalDAV's only (spec
3.4).

## 5. Tokens

### 5.1 `crates/vault/src/record.rs`: the binding

```rust
const LABEL_SESSION_BINDING: &[u8] = b"za/v1/session-binding";

/// What a key account's OAuth tokens and token-record entries bind to (spec D19): a fingerprint of the
/// account's credentials. SHA-256 over, in this order:
///   1. `LABEL_SESSION_BINDING`;
///   2. `salt`: its length as u32 big-endian, then its bytes;
///   3. `argon2_m_cost_kib`, `argon2_t_cost`, `argon2_p_cost`: each as u32 big-endian;
///   4. `verifier_hash`: its length as u32 big-endian, then its bytes;
///   5. `totp_url`: the byte 0 when absent; when present the byte 1, the length as u32 big-endian and the
///      UTF-8 bytes.
/// The value is the digest's first 8 bytes as a big-endian u64. A value of 0 is replaced by 1: upstream reads a
/// credential version of 0 as unversioned.
pub fn za_session_binding(record: &VaultRecord) -> u64;
```

It reads no other field: not `revision`, `state`, a wrap, the public key
or the setup token. So it changes with a password change, a recovery and
a second-factor change, and with no other vault write. The binding is not
the authentication generation: the key cache's generation and
`SessionKeys.generation` stay the vault revision.

| Place | Use |
|---|---|
| `crates/common/src/auth/access_token.rs:172-184` | a key account's credential version: `za_session_binding(&read.record)` in place of `read.record.revision`; the `.unwrap_or(0).max(1)` stays |
| `vault_batch_write` (`crates/common/src/auth/vault.rs:197-239`) | clears field 153 only when `previous` is `None` or its binding differs from the new record's |
| `ZaTokenRecord.binding` (5.3) | the binding the entries were made under |
| the hand-off value (5.3) | the binding at the password step, stored and inside the wrap's associated data |
| `za_token_issue`, `za_token_refresh`, `za_token_open_access` (5.4) | each reads the vault record and compares bindings |
| the assertion-loss loop of 5.4 | a write still asserts the vault record's bytes; on a lost vault assertion the writer reads again: an unchanged binding is retried, three attempts in all, and a changed binding stops it |
| `za_handoff_put` (5.4) | compares the vault revision with `keys.generation`, not bindings, then asserts the original vault bytes after storing the hand-off and before returning its secret: a crossing vault write of any kind fails sign-in; the provisional entry is deleted best-effort and no code or consent token is issued |
| the Bearer arm (5.5, D23) | compares generations: a request crossed by any vault write answers 401; after one that left the binding unchanged the client's refresh succeeds |
| tests | `binding_changes_only_with_credentials` (Task 13); `credential_change_clears_the_token_record`, `tokens::credential_changes_end_sessions`, `tokens::other_vault_writes_keep_sessions`, `tokens::pin_rewrap_keeps_session` (Task 14); the crossed cases of Tasks 16 to 18; `scheduling::first_invitation_and_the_session` (Task 31) |

### 5.2 `crates/vault/src/keys.rs` (additions)

```rust
const LABEL_TOKEN_WRAP: &[u8] = b"za/v1/token/wrap";
const LABEL_TOKEN_TAG: &[u8] = b"za/v1/token/tag";
const LABEL_TOKEN_NEXT: &[u8] = b"za/v1/token/next";
const LABEL_HANDOFF: &[u8] = b"za/v1/handoff";
pub const TOKEN_TAG_LEN: usize = 8;

pub fn derive_token_wrap_key(secret: &Secret) -> Secret;
pub fn derive_token_tag(secret: &Secret) -> [u8; TOKEN_TAG_LEN];
pub fn derive_token_next_key(secret: &Secret) -> Secret;
pub fn derive_handoff_key(secret: &Secret) -> Secret;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZaTokenKind { Access, Refresh { generation: u64 } }
/// aad("token/{grant_id}/access", account) or aad("token/{grant_id}/refresh/{generation}", account)
pub fn token_aad(account_id: u32, grant_id: u64, kind: ZaTokenKind) -> Vec<u8>;
/// aad("token/{grant_id}/next/{generation}", account)
pub fn token_next_aad(account_id: u32, grant_id: u64, generation: u64) -> Vec<u8>;
/// aad("handoff/{binding}/{key_id}", account); `key_id` is the 40-character code or consent token
pub fn handoff_aad(account_id: u32, binding: u64, key_id: &str) -> Vec<u8>;
```

### 5.3 `crates/vault/src/tokens.rs` (new; `pub mod tokens;`)

```rust
pub const TOKEN_RECORD_VERSION: u8 = 1;
pub const MAX_REFRESH_ENTRIES: usize = 2;
pub const MAX_ACCESS_ENTRIES: usize = 4;
pub const MAX_KEYED_GRANTS: usize = 32;
pub const SECRET_SEPARATOR: char = '~';
pub const SECRET_TEXT_LEN: usize = 43; // URL-safe base64 of 32 bytes, no padding
pub const CODE_LEN: usize = 40;        // upstream's DEVICE_CODE_LEN

// No derived Debug on any record type: they hold wraps.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, PartialEq, Eq)]
pub struct ZaTokenRecord { pub version: u8, pub binding: u64, pub grants: Vec<ZaTokenGrant> }
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, PartialEq, Eq)]
pub struct ZaTokenGrant { pub grant_id: u64, pub refresh: Vec<ZaRefreshEntry>, pub access: Vec<ZaAccessEntry> }
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, PartialEq, Eq)]
pub struct ZaRefreshEntry { pub generation: u64, pub wrap: Vec<u8>, pub next: Option<Vec<u8>>, pub expires: u64 }
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, PartialEq, Eq)]
pub struct ZaAccessEntry { pub tag: [u8; TOKEN_TAG_LEN], pub wrap: Vec<u8>, pub expires: u64 }

pub enum ZaSuffix { None, Secret(Secret), Malformed }
/// `upstream ~ secret`: splits at the first '~'; the upstream part is borrowed. A value that does not begin
/// with `header` is returned whole with `ZaSuffix::None`, whatever it holds (D26).
pub fn split_secret<'x>(token: &'x str, header: &str) -> (&'x str, ZaSuffix);
pub fn append_secret(token: &mut String, secret: &Secret);
/// A code or consent token: 40 characters alone, or 40 followed by the 43 of a secret, no separator.
pub fn split_code(code: &str) -> (&str, ZaSuffix);
pub fn append_code_secret(code: &mut String, secret: &Secret);

/// One registry read of a grant, for reconciliation.
pub struct ZaGrantState { pub account_id: u32, pub generation: u64, pub last_used_at: i64 }

pub enum ZaRefreshClass {
    /// The presented entry opens. `next` is its sealed next secret, when one was issued.
    Opened { mk: Secret, next: Option<Secret> },
    /// Its entry is gone and the grant holds a higher generation.
    Superseded,
    /// Anything else: the keys are lost.
    Lost,
}

impl ZaTokenRecord {
    pub fn new(binding: u64) -> Self;
    pub fn grant(&self, grant_id: u64) -> Option<&ZaTokenGrant>;
    /// Spec 4.3: drops expired entries, grants that are gone or of another account, refresh entries below the
    /// grant's stored generation, and grants with no live entry. `grants` holds one read per grant named.
    pub fn reconcile(&mut self, account_id: u32, now: u64, grants: &HashMap<u64, Option<ZaGrantState>>) -> bool;
    /// The grant to evict before `adding` is added: `None` under the cap, else the grant with the oldest
    /// `last_used_at`, the lowest grant id on a tie.
    pub fn eviction(&self, adding: u64, grants: &HashMap<u64, Option<ZaGrantState>>) -> Option<u64>;
    pub fn remove_grant(&mut self, grant_id: u64) -> bool;
    pub fn open_access(&self, account_id: u32, grant_id: u64, secret: &Secret, now: u64) -> Option<Secret>;
    pub fn classify_refresh(&self, account_id: u32, grant_id: u64, presented: u64, secret: &Secret, now: u64) -> ZaRefreshClass;
    /// Adds an access entry and returns its secret; past four, drops the entry that expires first.
    pub fn put_access(&mut self, account_id: u32, grant_id: u64, mk: &Secret, expires: u64) -> Secret;
    /// Writes generation `next`'s entry when it is absent and, with `presented`, seals its secret into that entry.
    pub fn put_refresh(&mut self, account_id: u32, grant_id: u64, mk: &Secret, presented: Option<(u64, &Secret)>,
        next: u64, next_secret: &Secret, expires: u64);
    pub fn is_empty(&self) -> bool;
}

/// Hand-off value: version (1) | account id (u32 BE) | binding (u64 BE) | wrap (72) = 85 bytes.
pub const HANDOFF_VERSION: u8 = 1;
pub fn seal_handoff(mk: &Secret, account_id: u32, binding: u64, key_id: &str) -> (Secret, Vec<u8>);
pub fn open_handoff(value: &[u8], secret: &Secret, key_id: &str) -> Option<(u32, u64, Secret)>;
```

A code and a consent token carry the secret with no separator (spec
4.4), so every character stays URL-safe and no client meets `%7E`.

### 5.4 `crates/common/src/za/tokens.rs` (new; `pub mod tokens;` in `za/mod.rs`)

`PrincipalField::ZeroAccessTokens = 153` in `crates/types/src/field.rs`
(variant after `:98`, `u8` arm after `:214`). Stored as a version byte
and an uncompressed checked archive, as the vault record is
(`crates/common/src/auth/vault.rs:106-131,205-212`).

```rust
pub struct ZaTokenRead { pub raw: Option<Vec<u8>>, pub record: Option<ZaTokenRecord> } // record None: absent or unusable
impl ZaTokenRead { pub fn assertion(&self) -> AssertValue; } // Hash(xxh3) of any bytes read, else None

pub(crate) fn tokens_key(account_id: u32) -> ValueKey<ValueClass>;
pub(crate) async fn token_read_store(store: &Store, account_id: u32) -> trc::Result<ZaTokenRead>;
/// One batch: with `vault_cas`, assert the vault record's bytes; assert field 153's previous bytes; then set,
/// or clear when `record` is `None` or empty. `vault_cas` is `None` only for a write that removes entries.
pub(crate) async fn token_write_store(store: &Store, account_id: u32, vault_cas: Option<u64>,
    previous: &ZaTokenRead, record: Option<&ZaTokenRecord>) -> trc::Result<()>;

pub struct ZaIssued { pub access: Secret, pub refresh: Secret }
pub enum ZaTokenIssue { Issued(ZaIssued), VaultChanged }
pub enum ZaTokenRefresh { Issued(ZaIssued), Superseded, Lost, VaultChanged }
pub enum ZaRefreshPrecheck {
    /// No secret and the grant has no entries: upstream's refresh, no further record access.
    Keyless,
    /// A secret is present: rotate, then `za_token_refresh`.
    Keyed,
    /// No secret but the grant has entries, or a malformed suffix: `invalid_grant`, nothing changed.
    Refused,
}
pub struct ZaHandoff { pub account_id: u32, pub binding: u64, pub mk: Secret }

impl Server {
    pub async fn za_token_read(&self, account_id: u32) -> trc::Result<ZaTokenRead>;
    /// Spec 4.4: the first refresh entry (generation 0) and access entry of `grant_id`. Reads the vault record and
    /// requires its binding to equal `binding`; reconciles; evicts and then deletes the least recently used grant
    /// past the cap; writes with both assertions; on an assertion loss reads again, three attempts.
    pub async fn za_token_issue(&self, account_id: u32, grant_id: u64, mk: &Secret, binding: u64,
        access_expires: u64, refresh_expires: u64) -> trc::Result<ZaTokenIssue>;
    /// Spec 4.5 step 1, before the rotation. Called for a key account only.
    pub async fn za_token_refresh_precheck(&self, account_id: u32, grant_id: u64, suffix: &ZaSuffix)
        -> trc::Result<ZaRefreshPrecheck>;
    /// Spec 4.5 steps 2 to 5, after `rotate_oauth_grant` issued `next`.
    pub async fn za_token_refresh(&self, account_id: u32, grant_id: u64, presented: u64, secret: &Secret,
        next: u64, access_expires: u64, refresh_expires: u64) -> trc::Result<ZaTokenRefresh>;
    /// Spec 4.6: reads the token record and the vault record. `Some(keys)`, with the vault revision of that read
    /// as their generation, when the entry opens and the bindings agree; else `None`.
    pub async fn za_token_open_access(&self, account_id: u32, grant_id: u64, secret: &Secret)
        -> trc::Result<Option<Arc<SessionKeys>>>;
    /// Spec 4.7: removes a grant's entries; asserts the record alone. No write when the record lacks the grant.
    /// Under the hook `TOKEN_FORGET_FAIL_ONCE` (§9.1) it returns `StoreEvent::UnexpectedError` with no fixed details.
    pub async fn za_token_forget_grant(&self, account_id: u32, grant_id: u64) -> trc::Result<()>;
    /// D8: the registry's account list, one point read each, reconcile, write asserting the record alone.
    /// A failure on one account is logged. Returns the records changed.
    pub async fn za_token_sweep(&self) -> trc::Result<usize>;
    /// Reads the vault record and requires its revision to equal `keys.generation`. Stores the hand-off under
    /// `KV_OAUTH ‖ '~' ‖ key_id` with `expires` seconds, then asserts the original vault bytes in one assertion-only
    /// data-store batch. Returns the secret only after success. On failure deletes the provisional entry best-effort:
    /// an assertion failure is the fixed sign-in Auth Failed; another store error propagates. No retry, code/token
    /// publication on failure, or persistent mutation in the assertion-only batch.
    pub async fn za_handoff_put(&self, key_id: &str, account_id: u32, keys: &SessionKeys, expires: u64)
        -> trc::Result<Secret>;
    /// Gets and deletes the entry. `None` when it is missing or does not open.
    pub async fn za_handoff_take(&self, key_id: &str, secret: &Secret) -> trc::Result<Option<ZaHandoff>>;
    pub async fn za_handoff_delete(&self, key_id: &str) -> trc::Result<()>;
}
```


### 5.5 Call sites

| File | Change |
|---|---|
| `crates/common/src/auth/oauth/token.rs:162-170` | `decode_access_token` opens `za_split_bearer(token_).0` (§5.7) |
| `crates/common/src/auth/oauth/grant.rs:385-392` | after `Success` and `NotFound`, for a key account only (`za_is_key_account`): `za_token_forget_grant`; an error is logged, never returned |
| `crates/common/src/auth/vault.rs:230-231` | `batch.clear(PrincipalField::ZeroAccessTokens)` when the binding changes, after every assertion and before the sets |
| `crates/http/src/auth/oauth/auth.rs` | `authenticate_login` (`:620-631`) calls `authenticate_with_keys`; `LoginAccount` (`:600-609`) gains `za_keys: Option<Arc<SessionKeys>>`; `issue_auth_code` (`:949-985`) and `request_consent` (`:846-879`) gain `za_keys: Option<&SessionKeys>` and call `za_handoff_put` before storing the code or token, then append the secret; `confirm_consent` (`:881-915`) calls `split_code` first, takes the consent hand-off, deletes it on an early return or a refused consent, and passes its keys on; `approve_device` drops the keys |
| `crates/http/src/auth/oauth/za_token.rs` (new) | the three functions below |
| `crates/http/src/auth/oauth/token.rs` | `authorization_code_grant` (`:272-337`) calls `za_code_handoff`; `NewGrantRequest` gains `za_grant: Option<ZaGrantKeys>`; `issue_new_grant` calls `za_issue_keyed` after `grant_response`; `refresh_token_grant` (`:406-508`) calls `Server::za_token_refresh_precheck` before `rotate_oauth_grant` (from Task 17, which answers `Keyed` with `invalid_grant` until Task 18) and `za_refresh_keyed` after `Issue`; `token_response` (`:652-668`) marks a `Granted` answer `with_untraced_body()` for every account |
| `crates/http/src/auth/za_bearer.rs` (new) | `za_bearer_keys` below |
| `crates/http/src/auth/authenticate.rs` | after `authenticate_with_keys` (`:207`) and before the existing pause point (`:209-210`): `za_bearer_keys`; in the keyed arm (`:262-277`), keys that came from a token and fail the generation fence return the refusal instead of being served |
| `crates/services/src/task_manager/tasks/maintenance.rs:283` | `server.za_token_sweep()` after the lease sweep; its error is logged and does not stop the purge |

```rust
// crates/http/src/auth/oauth/za_token.rs
pub(crate) struct ZaGrantKeys { pub mk: Secret, pub binding: u64 }
/// Called after upstream's checks consumed the code. The handler has split the `code` parameter with
/// `split_code` and used its 40 characters for upstream's lookups. For a key account takes the hand-off; a
/// missing or unopenable one, or no secret, is `Err(ErrorType::InvalidGrant)`. For an ordinary account: `Ok(None)` without a secret
/// on the code, `Err(ErrorType::InvalidGrant)` with one.
pub(crate) async fn za_code_handoff(server: &Server, account_id: u32, code: &str, suffix: ZaSuffix)
    -> trc::Result<Result<Option<ZaGrantKeys>, ErrorType>>;
/// Calls `za_token_issue` and appends the secrets to `response`. On `VaultChanged` or an error deletes the grant;
/// `VaultChanged` is `Err(ErrorType::InvalidGrant)`.
pub(crate) async fn za_issue_keyed(server: &Server, account_id: u32, grant_id: u64, keys: &ZaGrantKeys,
    response: &mut OAuthResponse) -> trc::Result<Result<(), ErrorType>>;
/// Calls `za_token_refresh` and appends the secrets. `Lost` deletes the grant. `Superseded`, `Lost` and
/// `VaultChanged` are `Err(ErrorType::InvalidGrant)`.
pub(crate) async fn za_refresh_keyed(server: &Server, account_id: u32, presented: TokenGrant, next: TokenGrant,
    secret: &Secret, response: &mut OAuthResponse) -> trc::Result<Result<(), ErrorType>>;

// crates/http/src/auth/za_bearer.rs
/// `Ok(None)`: the token has no suffix, or does not begin with the server's token header: a keyless token as
/// today. `Ok(Some(keys))`: the entry opened.
/// `Err`: `AuthEvent::Failed` with a fixed reason (§5.6), answered 401 and not counted for banning (D27): a malformed suffix, a suffix on an ordinary
/// account's token, no grant id, or an entry that is missing, expired or does not open.
pub(crate) async fn za_bearer_keys(server: &Server, token: &str, access_token: &AccessToken)
    -> trc::Result<Option<Arc<SessionKeys>>>;
```

The code's 40 characters go to upstream's `key_get` and
`consume_oauth_code`. `OAuthResponse` derives `Debug`; it is never
traced.

### 5.6 Fixed trace texts

The spec fixes that these traces exist and carry no token, secret, code
or tag. Their wording is fixed here.

| Where | Event | Text |
|---|---|---|
| `za_bearer_keys` | `AuthEvent::Failed`, details | `zero-access: malformed token secret` |
| `za_bearer_keys` | `AuthEvent::Failed`, details | `zero-access: token secret on an ordinary account's token` |
| `za_bearer_keys` | `AuthEvent::Failed`, details | `zero-access: token secret on a token without a grant` |
| `za_bearer_keys` | `AuthEvent::Failed`, details | `zero-access: token keys unavailable` |
| the Bearer arm (D23) | `AuthEvent::Failed`, details | `zero-access: token keys of a superseded generation` |
| `za_handoff_put` | `AuthEvent::Failed`, details | `zero-access: vault changed during sign-in` |
| `za_refresh_keyed`, `Lost` | `AuthEvent::Failed`, reason | `zero-access: token keys lost, OAuth grant deleted` |
| `delete_oauth_grant`'s hook | error, details | `zero-access: token entries not removed with their grant` |
| `za_token_issue`, eviction | error, details | `zero-access: evicted grant not deleted` |
| `za_token_issue`, a missing grant | `StoreEvent::UnexpectedError`, details | `zero-access: OAuth grant not found` |
| `za_token_sweep` | error, details | `zero-access: token sweep failed for an account` |
| the maintenance call site | error, details | `zero-access: token sweep failed` |
| `token_write_store` | `StoreEvent::UnexpectedError`, details | `zero-access: a token-record write without a vault assertion may only remove entries` |

A refusal of the fork is returned as it is, without
`authentication_failure`: it is not counted for banning (D27).

### 5.7 Notes from the plan's drafting

```rust
// crates/common/src/za/tokens.rs (Task 16)
/// `split_secret(token, TOKEN_HEADER)`. The one splitter `decode_access_token`, `za_bearer_keys` and
/// `refresh_token_grant` call; none calls `split_secret` itself.
pub fn za_split_bearer(token: &str) -> (&str, ZaSuffix);
```

- `za_token_read` is produced by Task 14; `za_token_refresh_precheck` and
  `ZaRefreshPrecheck` by Task 17.
- `token_write_store` with `vault_cas: None` refuses, with the last text
  of §5.6, a record that holds a grant or an entry `previous` lacks.
- `za_token_forget_grant` reconciles, three attempts, and reads no vault
  record. `za_token_sweep` leaves a record that does not decode and skips
  a lost assertion silently.
- `za_handoff_put` at a stale revision, or after losing its post-store
  vault assertion, returns `AuthEvent::Failed`; the password step answers
  it without `authentication_failure`. After `key_set`, one batch on the
  account's Principal document 0 asserts `ZeroAccessVault` with
  `AssertValue::Hash(read.cas)`. It sets and clears nothing. An ordinary
  follow-up read is insufficient: a backend may reuse a read snapshot.
  Never retry this hand-off under newer vault bytes. A cleanup failure
  cannot turn the refusal into success; an expiring encrypted orphan may
  remain, with neither its secret nor matching code/token published.
- The D6 fence adds one assertion-only data-store batch per keyed
  hand-off: one for a direct code sign-in, two across pending consent and
  its confirmation. A post-store failure adds one attempted in-memory
  delete. Ordinary accounts, Bearer requests and refresh pay no added
  cost from this fence.
- `confirm_consent` reads the vault record, compares bindings through
  `za_session_binding` and builds `SessionKeys` at that read's revision.
- Entry expiries are computed from `now()` when the entry is written.
- `open_handoff` reads the account id and the binding from the value and
  passes both to `handoff_aad`.
- §5.3, rules the signatures leave open. A secret is exactly 43
  characters that decode strictly to 32 bytes; anything else after a `~`
  is `Malformed`. `split_code`: 40 bytes is `(code, None)`; 83 bytes with
  a boundary at 40 whose last 43 decode is `(&code[..40], Secret)`;
  anything else is `(code, Malformed)`, the whole input. `put_access`
  drops, past four entries, the one with the smallest `expires`.
  `put_refresh` adds generation `next` only when absent, seals the next
  secret into the presented entry only when that entry has none, and
  keeps the two highest generations. `reconcile` drops a grant that is
  absent from the map, maps to `None` or is another account's; every
  refresh entry below the stored generation; every entry with `expires
  <= now`; and a grant left empty. `eviction` picks the smallest
  `(last_used_at, grant_id)`, a grant without a state counting as the
  oldest, and nothing when the record names `adding` or holds fewer than
  32 grants. `classify_refresh`: a sealed next secret that does not open
  is `Lost`.

## 6. `common` additions for JMAP

`crates/common/src/za/mod.rs`:

```rust
/// Spec 6.1: the client text of a calendar method refused for a token that holds no keys for the key account.
pub const ZA_CALENDAR_KEYLESS: &str =
    "Calendars in this account are encrypted at rest and are not available to this session.";
```

`crates/common/src/za/calendar.rs` (new; `pub mod calendar;`):

```rust
/// `JmapEvent::Forbidden` with `ZA_CALENDAR_KEYLESS`, as `za_keyless_refusal` is for mail (`za/mail.rs:28-33`).
pub fn za_calendar_keyless_refusal() -> trc::Error;
```

## 7. JMAP

### 7.1 `crates/jmap/src/api/za_calendar.rs` (new)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZaCalendarMethod {
    /// Owner with keys, or a grantee with its own keys and shared access.
    Served,
    /// `CalendarEvent/parse`: the owner with keys only.
    OwnerOnly,
    /// `accountNotSupportedByMethod` for a key account.
    Refused,
}
/// Replaces `za_assert_calendar_allowed` (`api/request.rs:996-1002`). An ordinary account: `Ok`. `Refused`:
/// `JmapEvent::AccountNotSupportedByMethod`. Otherwise `za_calendar_admission`; its refusal becomes
/// `za_calendar_keyless_refusal().account_id(account_id)`.
pub(crate) async fn za_admit_calendar_method(server: &Server, access_token: &AccessToken, account_id: Id,
    method: ZaCalendarMethod) -> trc::Result<()>;
/// The class of each calendar method. Every method starts `Refused`; a task flips the ones it serves.
pub(crate) fn za_calendar_method(obj: MethodObject, function: MethodFunction) -> ZaCalendarMethod;
/// Calendar | CalendarEvent | ParticipantIdentity: the methods that trigger the drain.
pub(crate) fn is_calendar_method(obj: MethodObject) -> bool;
```

Named `za_admit_calendar_method` so it does not collide with
`groupware`'s `za_calendar_admission`. Call sites: `api/request.rs:536,
543,552,561,652,659,668,776,785,794,803,891-892,943` and
`changes/get.rs:203,209,215,228`.

### 7.2 Reads

```rust
// crates/jmap/src/calendar_event/za_read.rs (new)
pub(crate) struct ZaEventReader { /* ZaReadMemo; views by document id */ }
impl ZaEventReader {
    /// `None` for an ordinary account.
    pub(crate) async fn new(server: &Server, access_token: &AccessToken, account_id: u32) -> trc::Result<Option<Self>>;
    /// The view of one event through the read context of one of its calendars. `Ok(None)`: not readable by this
    /// caller, or unreadable (traced): the event is not found. `Err`: the method fails.
    pub(crate) async fn view(&mut self, server: &Server, access_token: &AccessToken, document_id: u32,
        calendar_ids: &[u32], meta: &ArchivedCalendarEvent, content: &Archive) -> trc::Result<Option<&ZaEventView>>;
}

// crates/jmap/src/calendar_event/query/za.rs (new; child of query)
/// Opens every candidate of `mask`, inserts it in a `ZaEventTextIndex`, and replaces the text leaves of `filters`.
pub(super) async fn za_resolve_text_leaves(server: &Server, access_token: &AccessToken, reader: &mut ZaEventReader,
    cache: &GroupwareResources, account_id: u32, mask: &RoaringBitmap, filters: &mut Vec<SearchFilter>) -> trc::Result<()>;

// crates/jmap/src/changes/za_calendar.rs (new)
/// D15. Checks nothing for the owner, an ordinary account or `State::Initial`. `Exact(id)` is checked as `id`;
/// `Intermediate` by the rule of D29 (§7.6). `Err(JmapEvent::CannotCalculateChanges)` when a
/// calendar of `za_foreign_calendar_contexts` refuses.
pub(crate) async fn za_calendar_sync_boundary(server: &Server, access_token: &AccessToken, account_id: u32,
    since: &State) -> trc::Result<()>;
```

Insertion points: `calendar_event/get.rs:258-261` (the content handed to
the projector), `calendar/get.rs:159-175` (`za_read_calendar_view`),
`calendar_event/query.rs` between `:373` and `:417` and at `:483-498`,
`changes/get.rs:245-254` (one arm in the fork's `match object` block), and
`changes/query.rs:308-313` for the admission of `/queryChanges`. For
`Intermediate`, see §7.6.

### 7.3 Writes

```rust
// crates/jmap/src/calendar_event/set/za.rs (new; `mod za;` in set.rs; branch after set.rs:189)
pub(super) async fn za_calendar_event_set(server: &Server, request: SetRequest<'_, CalendarEvent>,
    access_token: &AccessToken, using: CapabilityIds) -> trc::Result<SetResponse<CalendarEvent>>;
pub(super) struct ZaRulings { pub use_default_alerts: bool }
/// Spec 6.5. With `base`: refuses `isDraft: true` and a true `mayInviteSelf`, `mayInviteOthers` or `hideAttendees`,
/// and takes `useDefaultAlerts` out of the object. For every patch: refuses every link with a `blobId`,
/// including one embedded in the event being updated.
pub(super) fn za_take_rulings(object: &mut EventValue<'_>, base: bool)
    -> Result<ZaRulings, SetError<JSCalendarProperty<Id>>>;
/// The table of 2.2. The caller handles `Err` per object under D28 (§7.5), preserving committed answers.
pub(super) fn za_event_set_error(error: ZaPipelineError, uid: Option<&str>)
    -> Result<SetError<JSCalendarProperty<Id>>, trc::Error>;

// crates/jmap/src/calendar/set/za.rs (new; `mod za;` in calendar/set.rs; branch at calendar/set.rs:80)
pub(super) async fn za_calendar_set(server: &Server, request: SetRequest<'_, Calendar>,
    access_token: &AccessToken, using: CapabilityIds) -> trc::Result<SetResponse<Calendar>>;
```

Per object, `za_calendar_event_set` runs, in upstream's order (creates,
updates, destroys):

- **create**: upstream's checks; `za_take_rulings`; upstream's patch,
  count check, conversion, generated UID; D11; `validate_user_data`;
  organizer, stamps, participants limit, size cap; quota pre-checks;
  `za_take_uid_lease` over every named calendar; `new_for(.., true)`;
  `za_write_contexts`; next alarms; `za_schedule_create`;
  `za_create_event`; upstream's server-set from `ZaWritten.ical` with the
  real document id.
- **update**: the stored row and content; `ZaEventReader::view`;
  upstream's projection of the view; `za_take_rulings`; upstream's patch,
  instances and conversion; D11; `validate_user_data_changes`; organizer,
  increment, stamps; `za_sequence_floor`; participants limit;
  UID-unchanged; `EventChanges::assert_allowed`; size cap; previous
  alarms; `new_for(.., true)`; next alarms; `za_write_contexts` or, when
  the calendars change, `za_placement_contexts`; `za_schedule_update`;
  `za_commit_update`.
- **destroy**: upstream's checks; `za_delete_send_itip`;
  `za_destroy_event`.

`response.new_state` is `State::Exact(change_id)` of the last commit.
The writer never calls `notify_direct_change`, `ItipMessages::queue`,
`CalendarEvent::insert`, `update_full` or `DestroyArchive::remove_all`.

`za_calendar_set`: create calls `seal_calendar` with the owner's keys
before upstream's `insert`; update resolves `za_mutation_context(..,
Modify)`, edits `calendar_view_in_domain` with upstream's
`update_calendar`, asserts the context, calls `seal_calendar_changes` and
upstream's `update`; destroy calls `za_calendar_delete_target` and
`za_delete_calendar` with `resources.children_ids(calendar_id)` and
`ZaScheduling::On`. `shareWith` is refused before `update_calendar`.

### 7.4 One-line insertions

- `api/request.rs` before `:194`: `if let Some(keys) =
  access_token.za_keys_for(owner) && request.method_calls.iter().any(|c|
  is_calendar_method(c.name.obj)) { self.za_drain(keys).await; }`.
- `api/session.rs:287`: the `Calendars | CalendarsParse` arm of
  `za_hidden_capability` goes.
- `principal/get.rs:181-188`: `mayGetAvailability` gains `&& !is_key`.
- `participant_identity/set.rs` after `:77`: for a key owner every
  create, update and destroy moves to its `not_*` map with `forbidden`.

### 7.5 Fixed JMAP texts and the store failure

In `crates/jmap/src/api/za_calendar.rs` unless noted:

```rust
pub(crate) const ZA_CALENDAR_SET_INCOMPLETE: &str = "The server could not complete this change, try again.";
/// Set error `forbidden` with `ZA_CALENDAR_SET_INCOMPLETE`.
pub(crate) fn za_set_incomplete<P>() -> SetError<P>;
```

The store-failure rule (D28), for `za_calendar_set` and
`za_calendar_event_set`. Each object's body returns
`trc::Result<Result<answer, SetError>>`. An `Err` of that body, and the
`Err` of `za_event_set_error` or `za_calendar_set_error`, is a store
failure: the writer traces it once (`trc::error!`, the account id and,
when known, the document id), answers the object with
`za_set_incomplete()`, and answers every later object of the call (the
remaining creates, then updates, then destroys) the same way without
attempting it. Objects already answered keep their answers; `newState` is
the last commit's change id; the method returns `Ok`. An error in the
head, before the first object, fails the method. A store error in
`Calendar/set`'s default write is traced, the default stays as it was,
and the method still answers.

Other fixed descriptions: `shareWith`, "Calendar sharing is managed on the
account page."; `isDraft: true`, "Drafts are not available for zero-access
accounts."; a participant-visibility flag set true, "This property is not
available for zero-access accounts."; a link with a `blobId`, on `links`,
"Event attachments are not available for zero-access accounts."; the
interim arms of Task 30 (`ZA_INTERIM`, gone after Task 33), "This
operation is not available for this account yet."; an identity create,
update or destroy (`ZA_IDENTITIES_FIXED`, private to
`participant_identity/set.rs`, beside `za_refuse_identity_edits`),
"Participant identities in this account cannot be created, changed or
destroyed; only the default identity can be switched."

### 7.6 Notes from the plan's drafting

- §7.1: `/queryChanges` has its own admission call in
  `changes/query.rs:308-313`, after upstream's access check repeated there.
  `OwnerOnly` emits the admission trace itself. `Served`, `OwnerOnly` and
  `is_calendar_method` carry `#[allow(dead_code)]` until Tasks 21, 24 and
  34 remove it. `Calendar/query` is `Served` from Task 21.
- §7.2, the rule of `ZaEventReader::view`: a memoised view is returned at
  once; otherwise each calendar id is tried in order through
  `za_read_event_view`. `Ok(Ok(Some(view)))` is memoised and returned;
  `Ok(Ok(None))` is `Ok(None)`; an item error that is
  `StoreEvent::DataCorruption` is traced and is `Ok(None)`; any other item
  error and `Err(Store(error))` are `Err(error)`; any other refusal tries
  the next calendar and, after the last, is `Ok(None)`. Its callers read
  the stored row.
- §7.2, a read-scope error on the JMAP side: `Store(error)` fails the
  method; a `DataCorruption` item is not found; any other refusal is not
  found or, for the boundary, `cannotCalculateChanges`.
- §7.2, `Intermediate` (D29): a page of a sync that started from an
  initial state is never refused. A later page compares the state's
  `from_id` with the log's first retained change id: equal means an
  initial start, and the page is served; otherwise it is checked as
  `refuses(from_id - 1)`. `JMAPIntermediateState` is `{ from_id, to_id,
  items_sent }` (`crates/jmap-proto/src/types/state.rs:19-23`, built at
  `crates/jmap/src/changes/page.rs:278-279`). Task 22 confirms this from
  the code first, and stops and reports if the two starts cannot be told
  apart.
- §7.2: `za_resolve_text_leaves` opens the mask intersected with the sets
  under AND only (private `za_text_candidates(mask, filters)`). The query
  path charges no memory budget for the views it opens.
  `ZaEventTextIndex` has a private `with_fields(index_fields:
  Option<AHashSet<SearchField>>, default_language)` that `new` calls.
- §7.3: `fn za_calendar_set_error(error: ZaPipelineError) ->
  Result<SetError<CalendarProperty>, trc::Error>`, private to
  `calendar/set/za.rs`. `Forbidden(_)`, `LinkedEvent` and
  `Store(Security(Unauthorized))` are `forbidden`; `NotFound` is
  `notFound`; `Busy(_)` and an assertion failure are `forbidden` with
  "Another process modified this calendar, please try again."; a quota
  limit is `overQuota`; the scheduling variants are not produced on these
  paths and map to `forbidden`; anything else is `Err`, the store
  failure of §7.5. `za_event_set_error`'s `Err` is the same case.
- §7.3: the `/set` answer after D11 is upstream's, computed from the
  object as the client sent it. It carries no `alerts`, and reports
  `useDefaultAlerts: false` only when the client omitted the flag. Nothing
  is inserted in upstream's server-set.
- §7.3: `ZaEventReader::new` sits in the handler's head. `Calendar/set`
  resolves `za_roles` in the head when the call has destroys. So only a
  head error fails the method (D28).
- §7.2: the private helper of the paged sync, in `changes/za_calendar.rs`,
  is `since_change_id(since: &State, first_retained: Option<u64>) ->
  Option<u64>`.
- §7.5, who produces each text: Task 28, `ZA_CALENDAR_SET_INCOMPLETE`,
  `za_set_incomplete` and the `shareWith` text; Task 30, the three ruling
  texts and `ZA_INTERIM`; Task 23, the identities text, which Task 37
  asserts.
- §7.4: Task 35 also changes the unit test
  `key_accounts_are_offered_no_metadata` in
  `crates/jmap/src/api/metadata/za.rs`.

## 8. D14: `crates/groupware/src/calendar/seal/collection.rs`

No new name. `seal_personal_entry` (`:205-235`) writes the entry's colour
into the bundle; the opener (`:318-334`) returns it; `seal_calendar_changes`
(`:474-475`) accepts a personal colour; the grant's conversion
(`sharing/convert.rs`) carries the owner's colour. The bundle's rkyv
layout does not change.

## 9. Test hooks

Existing hooks keep their names and statics in
`crates/common/src/za/test_hooks.rs`. After the move these fire from
`groupware`, on CalDAV and JMAP writes: `COMMIT_PAUSE`,
`COMMIT_FAIL_ONCE`, `UPDATE_COMMIT_PAUSE` (the key commit),
`UPDATE_COMMIT_FAIL_ONCE`, `DELETE_COMMIT_PAUSE`,
`DELETE_COMMIT_FAIL_ONCE`. The module's table (`:7-100`) is updated.

New, all in `crates/common/src/za/test_hooks.rs`, keyed by account id,
called from `crates/common/src/za/tokens.rs`:

```rust
pub static TOKEN_EXCHANGE_PAUSE: Mutex<Option<Arc<Pause>>>;      // za_token_issue: after the binding check, before the batch
pub static TOKEN_HANDOFF_WRITE_PAUSE: Mutex<Option<Arc<Pause>>>; // za_handoff_put: after the revision check and seal, immediately before key_set (Task 17)
pub static TOKEN_REFRESH_READ_PAUSE: Mutex<Option<Arc<Pause>>>;  // za_token_refresh: before its first record read
pub static TOKEN_REFRESH_WRITE_PAUSE: Mutex<Option<Arc<Pause>>>; // za_token_refresh: after MK opened and the entries built, before the batch
pub static TOKEN_REFRESH_WRITE_FAIL_ONCE: Mutex<Option<u32>>;    // za_token_refresh: the batch is not written and a store error returns
pub fn set_token_exchange_pause(p: Option<Arc<Pause>>);        pub async fn token_exchange_pause_point(account_id: u32);
pub fn set_token_handoff_write_pause(p: Option<Arc<Pause>>);   pub async fn token_handoff_write_pause_point(account_id: u32);
pub fn set_token_refresh_read_pause(p: Option<Arc<Pause>>);    pub async fn token_refresh_read_pause_point(account_id: u32);
pub fn set_token_refresh_write_pause(p: Option<Arc<Pause>>);   pub async fn token_refresh_write_pause_point(account_id: u32);
pub fn set_token_refresh_write_fail_once(account_id: Option<u32>); pub fn take_token_refresh_write_fail(account_id: u32) -> bool;
```

The Bearer case crossed by a vault write uses the existing http pause
(`za_test::PAUSE`, `crates/http/src/auth/authenticate.rs:209-210`), which
now sits between `za_bearer_keys` and the generation fence.

### 9.1 The grant-removal hook

```rust
pub static TOKEN_FORGET_FAIL_ONCE: Mutex<Option<u32>>; // za_token_forget_grant: at its head, `StoreEvent::UnexpectedError` with no fixed details returns and nothing is read
pub fn set_token_forget_fail_once(account_id: Option<u32>); pub fn take_token_forget_fail(account_id: u32) -> bool;
```

Produced by Task 15. `TOKEN_REFRESH_WRITE_FAIL_ONCE`'s error carries the
details `zero-access test hook: token refresh write failure`.

## 10. Test helpers

In a new `tests/src/utils/oauth.rs`, re-exported from
`tests/src/utils/za.rs`:

```rust
pub enum ZaClientKind { Registered, Application, Stateless }
pub struct ZaOAuthClient { pub client_id: String, pub redirect_uri: String, ids: Vec<Id> }
pub async fn za_oauth_client(test: &TestServer, admin: &Account, client_id: &str, kind: ZaClientKind) -> ZaOAuthClient;
pub async fn za_drop_oauth_client(test: &TestServer, admin: &Account, client: ZaOAuthClient);
pub const ZA_SIGNIN_SCOPE: &str = "urn:ietf:params:oauth:scope:calendars urn:ietf:params:oauth:scope:mail offline_access";
pub struct ZaTokens { pub access: String, pub refresh: String, pub grant_id: u64, pub expires_in: u64 }
pub async fn za_authorize(client: &ZaOAuthClient, name: &str, secret: &str, mfa: Option<&str>, consent: Option<bool>, from_ip: Option<&str>) -> LoginResponse;
pub async fn za_confirm_consent(client: &ZaOAuthClient, consent_token: &str, consent: bool) -> LoginResponse;
pub async fn za_exchange(client: &ZaOAuthClient, code: &str) -> TokenResponse;
pub async fn za_refresh(client: &ZaOAuthClient, refresh: &str) -> TokenResponse;
pub async fn za_revoke_token(client: &ZaOAuthClient, token: &str) -> u16;
pub async fn za_sign_in(test: &TestServer, client: &ZaOAuthClient, name: &str, secret: &str, mfa: Option<&str>) -> ZaTokens;
pub fn za_split_token(token: &str) -> (&str, Option<&str>);
pub fn za_tamper_secret(token: &str) -> String;

pub(crate) const CALENDAR_JMAP_USING: &[&str];
pub async fn za_bearer_jmap(token: &str, using: &[&str], calls: Value) -> (u16, JmapResponse);
pub async fn za_bearer_session(token: &str) -> (u16, Value);
pub fn za_bearer_dav(account: &Account, token: &str) -> DummyWebDavClient;

pub async fn raw_token_record(test: &TestServer, account_id: u32) -> Option<Vec<u8>>;
pub async fn set_raw_token_record(test: &TestServer, account_id: u32, raw: Option<&[u8]>);
pub async fn token_record(test: &TestServer, account_id: u32) -> Option<ZaTokenRecord>;
pub async fn handoff_exists(test: &TestServer, code_or_consent: &str) -> bool;

pub async fn run_purge_maintenance(test: &TestServer, admin: &Account); // sweep.rs:46-53 lifted
#[must_use] pub struct OAuthExpiryGuard(/* restores on drop */);
pub fn oauth_expiry(test: &TestServer, access: Option<u64>, refresh: Option<u64>, code: Option<u64>) -> OAuthExpiryGuard;
```

In `tests/src/utils/za.rs`:

```rust
impl Keyless { pub async fn call_using(&mut self, using: &[&str], calls: Value) -> JmapResponse; } // `call` hard-codes the mail list
pub fn jscalendar_of(ics: &str) -> Value;      // upstream's converter, for the oracle's JMAP input
pub fn normalize_jscal(value: Value) -> Value; // drops `id`, `baseEventId`, the keys of `calendarIds`, `created` and `updated`, at any depth
```

`tests/src/utils/cleanup.rs:484-503` exempts field 153 while its account
exists; its message (`:258-262`) and `tests/src/utils/storage.rs:373`
name it. `tests/src/za/imip/oracle.rs`' `Tally`, `mail_key`,
`mail_parts`, `raw_header` and `without_schedule_status` become
`pub(crate)`.

Fixtures, all at example.com: `tokens` uses `token-owner`, `token-mfa`,
`token-consent`, `token-cap`, `token-race`, `token-plain` and the client
ids `za-token-app` (an Application) and `za-token-consent` (registered).
Each `jmap_calendar` module uses `jmapcal-<module>` and
`jmapcal-<module>-2`; `sharing` adds `jmapcal-sharing-reader`; the oracle
uses `jmapcal-oracle-cd` and `jmapcal-oracle-jm`, each with an invitee
`jmapcal-oracle-cd-inv` and `jmapcal-oracle-jm-inv`; the leak module uses
`jmapcal-leak` and `jmapcal-leak-2`. Outside parties are sink addresses
at `remote.test` with the prefix `jmapcal-`. Deliberate authentication
failures use `X-Forwarded-For` addresses in `10.0.60.x`.

Module order in `tests/src/za/mod.rs`, after `sharing_races::test` and
before `sweep::test`: `tokens`, `jmap_calendar::pipeline`, `events`,
`calendars`, `query`, `scheduling`, `sharing`, `refusals`, `oracle`.
`leak::test_jmap_calendars` runs after `leak::test_imip`.

### 10.1 Set-up helpers of the `jmap_calendar` modules

In `tests/src/za/jmap_calendar/mod.rs`, produced by Task 10 unless noted.

```rust
/// A module's two key accounts: `jmapcal-<module>@example.com` and `jmapcal-<module>-2@example.com`, password
/// `STRONG`, description the local part, `user_permissions()`, registered with `test.insert_account`.
pub(super) struct Pair { pub key: Account, pub key2: Account, pub key_id: u32, pub key2_id: u32,
    pub dav: DummyWebDavClient, pub dav2: DummyWebDavClient }
pub(super) async fn pair(test: &mut TestServer, module: &str) -> Pair;
/// One more key account of the family, `jmapcal-<local>@example.com`, with the given permissions.
pub(super) async fn key_account(test: &mut TestServer, local: &str, permissions: Vec<Permission>) -> (Account, u32);
/// MKCALENDAR of `<slug>` in the owner's home with `fanout::MKCALENDAR` (201). Returns its document id.
pub(super) async fn make_calendar(test: &TestServer, owner: &Account, dav: &DummyWebDavClient, slug: &str) -> u32;
/// `dav_seal::share_calendar` at `level` ("view" or "edit").
pub(super) async fn share(test: &TestServer, owner: &Account, grantee: &Account, grantee_id: u32, slug: &str, level: &str);
/// One VEVENT, no attendees: `UID`, `DTSTAMP:20240101T000000Z`, `DTSTART:20990301T100000Z`, `DURATION:PT1H`,
/// `SEQUENCE:0`, `SUMMARY`, CRLF line ends.
pub(super) fn event_ics(uid: &str, summary: &str) -> String;
/// PUT of `event_ics` at `<slug>/<uid>.ics` (201). Returns the event's document id.
pub(super) async fn put_event(test: &TestServer, owner: &Account, dav: &DummyWebDavClient, slug: &str, uid: &str,
    summary: &str) -> u32;
/// The account's token with its real permissions and its keys, in process.
pub(super) async fn session_token(test: &TestServer, account_id: u32) -> AccessToken;
/// The module's end: zero UID leases, then `test.wait_for_tasks()`.
pub(super) async fn finish(test: &TestServer, module: &str);
/// Task 21. Marks the calendar's sync boundary pending at store level, as `tests/src/za/sharing.rs:7127-7135` does.
pub(super) async fn mark_boundary_pending(test: &TestServer, owner_id: u32, calendar_id: u32);
/// Task 21. `za_owned_calendar_context(..).boundary.unwrap()` with the owner unlocked.
pub(super) async fn owner_boundary(test: &TestServer, owner_id: u32, calendar_id: u32) -> ZaSyncBoundary;
```

In a test block, `none()` is `Vec::<(&str, &str)>::new()` and a create id
of `jmap_create` is `i0`, `i1`, ...

`key_account` and `Pair.dav2` carry `#[allow(dead_code)]` from Task 10.
Task 11 removes `dav2`'s. Task 22, the first caller of `key_account`,
removes the other.

### 10.2 Corrections and fixtures

- `CALENDAR_JMAP_USING` is `urn:ietf:params:jmap:core`, `:calendars`,
  `:calendars:parse`, `:principals` and `:mail`.
- `ZaClientKind::Registered` has the grant types `AuthorizationCode`,
  `RefreshToken` and `DeviceCode`. `za_tamper_secret` swaps the first
  character after `~` between `A` and `B`.
- `tests/src/za/imip/oracle.rs`: `Kind` and `Tally`'s three fields become
  `pub(crate)` too.
- No codec of the token record is public: tests read through
  `Server::za_token_read`.
- `tests/src/za/leak.rs` gains `Scan.token_records: usize`,
  `secret_hits(test, secrets: &[&[u8]]) -> Vec<(Subspace, Vec<u8>, usize)>`
  and `test_jmap_calendars`.
- `gating::method_error` becomes `pub(crate)` in Task 21, visibility
  only; the `jmap_calendar` modules import it. The checker of
  `refusals.rs` is `assert_calendar_keyless`.
- Fixtures. A `jmapcal-<module>-2` account is always a key account; an
  ordinary one takes `-plain` (`jmapcal-query-plain`,
  `jmapcal-oracle-plain`, as `token-plain`). More: `token-gone` (Task 19,
  destroyed by its case), `jmapcal-scheduling-noperm`,
  `jmapcal-sharing-reader`, the alias `jmapcal-refusals-alias`.
  `jmapcal-scheduling-nobody@example.com` is a local address with no
  entry, not an account. Outside
  addresses are `jmapcal-<module>-<role>@remote.test`
  (`jmapcal-pipeline-att`, `jmapcal-scheduling-out` and `-out2`,
  `jmapcal-query-olive` and `-arthur`, `jmapcal-calendars-out`,
  `jmapcal-oracle-cd-out` and `-jm-out`, `jmapcal-leak-out`,
  `jmapcal-leak-addr-canary`). OAuth client ids: `za-token-app`,
  `za-token-consent`, `za-jmapcal-app`, `za-jmapcal-leak`.
