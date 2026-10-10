# Zero-access calendar: plan 2 outcome and handoff to plan 3

Written 2026-10-07 at the end of the plan 2 execution session. Read this
after the plan 1 outcome note and before starting plan 3. Everything here is
a ruling made during execution, a reviewer finding that was deliberately
deferred, or a fact plan 3 needs; none of it is derivable from the code
alone. Rulings are quoted by id (B1, R4, ...) from the session's
`preflight-rulings.md`; the ones that amend the spec are collected under
"Decisions that are Jay's".

## Where things stand

- Plan 2 (nine tasks plus a Task 0 that the plan did not have) is complete
  on branch `zero-access`: commits `7b61fc44..ac0bcdca` on top of `5fb92e3f`
  (spec revision 5); `ac0bcdca` is the polish wave the final review asked
  for. Task 0 refuses `setup` and `recover` for a disabled
  account or tenant, the one 4.1 item revision 5 scheduled for plan 2. Each
  task had an implementer, a task review, scoped re-reviews where the review
  asked for fixes, and a whole-branch review on 2026-10-07. That review
  approved the handoff to plan 3 with no Critical or Important findings and
  one optional polish wave, applied as `ac0bcdca` and re-reviewed.
- Suites green at the end:
  - `cargo test -p vault` (25), `-p groupware` (48, of which 30 in `seal::`),
    `-p common` (101), `-p http@0.16.25 --features test_mode` (6).
  - `za_tests`, now including the modules `disabled`, `dav_gate` and
    `dav_seal`.
  - `webdav_tests` in baseline mode and with `ZA_KEY_ACCOUNTS=1`. In key mode
    the sub-modules `copy_move`, `acl`, `cal_alarm` and `cal_scheduling` are
    skipped until plan 3 (see "Facts for plan 3").
- The product build is warning-free in the fork crates. It shows 24 upstream
  warnings (store 1, common 18, jmap 3, services 2); the final review blamed
  every one of them to upstream's non-enterprise stubs.
- What now works: a key account's events, tasks, journals and calendar
  collections are stored sealed when written through CalDAV (PUT, MKCALENDAR,
  PROPPATCH) and unsealed on every CalDAV read path (GET, PROPFIND,
  calendar-query, multiget, sync-collection, expand, free-busy). Every
  calendar URI of a key account without that account's session keys is a 403.
  Nothing outside CalDAV is gated yet; that is plan 3.
- Nothing has been pushed.

## Deviations from the plan that later plans must know

Policy and wire format:

- Policy version 1 carries two amendments. `UID` is visible on every
  component type, not only VEVENT, VTODO and VJOURNAL (R1): the AAD, the UID
  index and upstream's 412 check all need it for VFREEBUSY- and
  VAVAILABILITY-only objects. `X-LIC-LOCATION` and `X-MICROSOFT-CDO-TZID` are
  visible on VTIMEZONE (B2, compared case-insensitively, only there):
  calcard 0.3.14 resolves a timezone by name only (`TZID`, else
  `X-LIC-LOCATION`, else `X-MICROSOFT-CDO-TZID`), never by its rules.
  Everything else is sealed, including every other `X-` property and
  parameter.
- The rkyv layout of calcard and `types` values inside the ciphertext is
  kept, as the spec says (R8). calcard stays pinned at 0.3.14. A layout
  change needs a versioned reader; the bundle format byte `0x01` is the
  hook, and lazy re-sealing at session time is possible because the key is
  present then.
- Carrier order is fixed: each component's own `X-ZA-SEALED` is its last
  entry; the root ends `X-ZA-SEALED`, `X-ZA-EXTRA` (only when the display
  name or dead properties are non-empty), `X-ZA-KEY`. A sealed object is
  recognised by its root's last entry being `X-ZA-KEY`; a sealed collection
  by a name starting with `$za$`.
- After the expected carriers are popped, any remaining `X-ZA-*` entry that
  is not a component's trailing `X-ZA-SEALED` is `SealError::Structure`
  (Task 3 fix; `has_stray_carriers`). An absent carrier is not detected:
  deleting `X-ZA-EXTRA` or a `X-ZA-SEALED` bundle silently drops the sealed
  fields. Integrity against a store-writing operator is outside the trust
  model (ledger ruling).
- There are no "already sealed" guards (R2, Task 4 fix). A client body whose
  root ends in `X-ZA-KEY`, or a display name starting with `$za$`, is
  ordinary data and round-trips. Each write path seals exactly once by
  construction; nothing detects a double seal, and reviewers checked every
  path.
- Plaintext events and collections in a key account (uploaded before
  conversion, or the default calendar the resource cache creates without a
  key) pass through unseal unchanged and are sealed by the next changing
  write (R4). A no-change PUT does not seal a legacy plaintext event.
- `seal_calendar` refuses unless the collection has exactly one preferences
  entry and it is the owner's (Task 4 fix, because upstream's
  `preferences_mut` appends an entry for a sharee). Reads fall back to entry
  0 and fail closed with `Aead` for another account's entry.
- Shared helpers live in `seal/tree.rs`: `seal_key_envelope`,
  `open_key_envelope`, `open_archive<T>` (copies into `AlignedVec<16>`
  before rkyv's checked decode; a plain `Vec<u8>` slice carries no alignment
  guarantee), `has_stray_carriers`, and the single `WRAP_MK` constant.
- `unseal_tree`, `unseal_event` and `unseal_calendar` leave a partly
  modified target on `Err`, and after a failed in-place `unseal_event` the
  target no longer ends in `X-ZA-KEY`, so `is_sealed` reports it plaintext.
  Callers must discard the target and never retry or pass it through. The
  archive views work on deserialised copies, are read-only, keep the stored
  `version` (ETags stay bound to the stored bytes), and must never be
  written back, passed to `into_inner()` for a write, or used as an update's
  `current` (`AssertValue`).

The DAV gate (`crates/dav/src/common/za.rs`):

- `za_session_keys` returns `Ok(None)` for a non-key account and for an
  unknown account (R5, so upstream's 404 stands). For a key account without
  the keys it logs `SecurityEvent::Unauthorized` with details starting
  `zero-access:` and returns 403. The URI gate in `validate_uri_with_status`
  fires for `Collection::Calendar | CalendarEventNotification` (B3); the
  PROPFIND loader's per-item lookup covers `Calendar` and `CalendarEvent`.
  Address books, files and principals are not gated in release 1.
- `za_refuse_cross_account` refuses (403) a COPY or MOVE across accounts when
  either side is a key account (spec 8.1).
- `za_freebusy_access` returns `ZaFreeBusy::{Plain, Unsealed, Withheld}` (R6):
  a key account whose keys the caller lacks yields a free-busy object with no
  periods instead of a 403. The owner's own `free-busy-query` is stopped by
  the URI gate before it gets there.
- `za_event_view` and `za_calendar_view` return a `Cow` over the stored
  archive: borrowed for non-key accounts, an owned unsealed view for key
  accounts.

Non-key behaviour changes (need Jay's acceptance, decision 6):

- Task 5 fix: the calendar-query, calendar-multiget and free-busy REPORT arms
  in `request.rs` require a `cal` or `itip` prefix (405 otherwise, for all
  accounts; `za_calendar_report_uri`). Before this, those REPORTs sent via
  `/dav/pal`, `/dav/card` or `/dav/file` read the calendar store past the
  URI gate (a Critical finding). Multiget also skips hrefs whose collection
  differs from the request's (a per-href 404, which for addressbook-multiget
  was previously a nonsense read).

Other deviations:

- Task 0: `za_assert_enabled` runs in `za_setup` and `za_recover` after
  credential verification; a disabled account or tenant gets 403 through
  `SecurityEvent::Unauthorized`. Upstream never invalidates members' access
  tokens on a Tenant change (the member fan-out in
  `cache/invalidate.rs:145-152` is enterprise-only), so a tenant re-enable is
  stale until the token expires. The test asserts only the 403.
- Spec 10 "distinct event type" and the collection id in the logged error are
  not implemented. No new event variants (CLAUDE.md); the signal is
  `SecurityEvent::Unauthorized` with details starting `zero-access:`, and
  unseal errors are `StoreEvent::DataCorruption` naming account and document
  id (R11).
- Spec 7.3 "an `X-ZA-*` property on a non-key account is an error" is waived
  (R7): non-key accounts take unchanged paths.
- Sealed events charge only the PUT body (`size` stays `bytes.len()`), and
  once display name and dead properties live in `X-ZA-EXTRA` they are no
  longer charged by `CalendarEvent::size()` (ruling, Task 8). Collections
  charge their real sealed bytes: the sealed preferences name costs about
  491 bytes per collection.
- Test harness changes: the za suite raises `rate_limit_anonymous` to 10000
  per minute (the vault modules had exceeded upstream's 100 per minute from
  127.0.0.1 and the suite only passed when a minute boundary fell inside the
  run); in key mode mike's `MaxDiskQuota` is `1024 +
  SEALED_COLLECTION_QUOTA_OVERHEAD` (640, measured about 491) in
  `tests/src/webdav/mod.rs`, so upstream's `put_get` quota test keeps its
  shape; tests call `wait_for_tasks` before deleting a just-PUT event
  (upstream search-index race). Upstream's same-account collection COPY with
  `Overwrite: T` onto a collection that shares events returns 409 for every
  account (`AssertValue` twice in one batch); the sealing test copies onto a
  fresh collection instead.
- The Bearer API-key case of the gate test was dropped: plan 1 refuses
  API-key creation for key accounts (`tests/src/za/registry.rs:83`).

## Decisions that are Jay's to confirm or reverse

Decided 2026-10-07: all seven candidates and the two non-spec items were accepted as recommended and are now spec revision 6 (candidate 7 is carried into plan 3 as an obligation). The list below is kept as the record of what was decided.

Candidates for spec revision 6:

1. Policy amendments R1 (UID visible on every component) and B2
   (`X-LIC-LOCATION`, `X-MICROSOFT-CDO-TZID` visible on VTIMEZONE), spec
   section 6. Cost if wrong: free-busy-object UIDs visible, which events
   already expose; a zone name visible that `TZID` shows anyway.
2. R4 (plaintext objects in a key account pass through unseal and are sealed
   by the next write; spec 7.3 and 10), R7 (the 7.3 non-key `X-ZA-*` error
   is waived) and R11 (the 10 "distinct event type" becomes
   `SecurityEvent::Unauthorized` with `zero-access:` details).
3. Reword the "truncated bundle is an error" constraint to exclude absent
   carriers: integrity is not promised against an operator who can write to
   the store; the operator can drop sealed fields silently but cannot read
   them.
4. Quota rule. Collections charge real sealed bytes (about 500 B each);
   events charge only the PUT body, so the per-component sealed overhead
   (at least about 400 B per component with sealed content) and the
   `X-ZA-EXTRA` content are uncharged. Pick one rule.
5. State in the spec what is visible by structure: component type names
   (including `X-` components), carrier presence and the 256-byte size class
   of each bundle, and free text in visible slots (TZID values, x-name
   STATUS and TRANSP values, `X-` parts of RRULE).
6. Accept the non-key behaviour changes from the Task 5 fix (405 or 404 for
   calendar REPORTs on the wrong prefix).
7. `CalendarEvent.preferences` (per-account properties and alarms, written
   only through JMAP) is not sealed. Decide whether that is a spec gap to
   close in plan 3 after tracing which paths populate it for key accounts.

Also Jay's, but not spec text:

- R8: calcard and `types` rkyv layout inside the ciphertext is a deferred
  design risk (a layout change needs a versioned reader).
- A plaintext stored name starting with `$za$` is read as sealed and bricks
  that collection. It is reachable only through an admin-configured
  default-calendar name or conversion of an account that already has such a
  collection. Accepted for release 1.

## Deferred findings (reviewed, not fixed), by area

Security and robustness:

- Shared and assisted-discovery listing: when a key account's calendar is
  shared to a grantee, the loader's `za_session_keys` fails the grantee's
  whole PROPFIND with 403 (it should answer per item or skip). Existing
  grants on key-owned calendars are not revoked.
- A plaintext event whose root really ends in a client `X-ZA-KEY` is read as
  sealed and fails with `Decode`, `Format`, `Aead` or `Policy` until it is
  rewritten. Plan 3 must close every keyless writer into key accounts (iMIP,
  JMAP, internal scheduling) or strip or refuse root `X-ZA-*` from them;
  one failed item must not fail a listing.
- Plaintext reaches `RuleExpansionError` traces (`query.rs:235` and
  `dates.rs:178`) and the iTIP task queue between plans 2 and 3 (R9,
  accepted; plan 3 removes both).
- JMAP `Calendar/set` and `CalendarEvent/set` still write key-account data in
  plaintext, and a sharee's JMAP write can append a second preferences entry
  (the owner's PROPPATCH then answers 500, which fails closed). The JMAP
  linked-blob download for CalendarEvent is ungated.
- A depth-1 PROPFIND of the `/dav/cal/` root lists a key account's home
  collection metadata for grantees. Server-side scheduling and the outbox
  free-busy loop read key accounts' busy times without keys (the `Withheld`
  path covers only content).
- Legacy plaintext `$za$` names brick a collection (above).
- A sealed event with a plaintext `display_name` or dead properties outside
  the bundle passes through unseal unchanged.
- Decrypted content buffers (the `open_bytes` `Vec`, the aligned copy, the
  serialised `Extra` plaintext) are not zeroized; the invariant covers key
  material only.
- A metadata-only PROPFIND or sync-collection unseals every item (cost only);
  `getlastmodified`, schedule tag and ETag are read from the view, equal to
  the stored values by construction. A corrupt sealed event fails a whole
  free-busy computation (fails closed), and answers 500 on GET before the
  304 and 412 header checks.
- The bundle does not commit to the timezone kind or to carrier presence;
  names, descriptions and colours are swappable between collections of one
  account (account-only AAD, by design). A cross-account `copy_container` of
  a sealed collection yields `Aead`; the gate refuses it first.
- `tree_aad` hard-codes `POLICY_VERSION`; a v2 reader will need the stored
  byte. Padding bytes are not checked for zero (the AEAD covers them).
- Plan 1's deferred item that `setup` does not
  repeat the data check after up to seven days still stands (R4 note).

Tests worth adding:

- **Unreachable, no test (plan 7)** The scheduling outbox free-busy `Withheld` path.
  The outbox refuses a key attendee with 3.7 before free-busy is built
  (`crates/dav/src/calendar/scheduling.rs:375-385`), and a free-busy REPORT
  without the account's keys is refused by the URI gate
  (`crates/dav/src/common/uri.rs:105-115`). `ZaFreeBusy::Withheld` stays as
  defence in depth.
- **Closed (plan 7)** An OAuth Bearer request to a key account's calendar without keys (no
  helper in the tests crate; the Task 5 re-review left it open). Bearer case
  in `tests/src/za/dav_gate.rs`.
- **Closed (plan 7)** Revocation asserted in the cross-account grant test (`dav_gate.rs`, after
  the grant is dropped); tampered-event 500 on
  GET, HEAD and PUT (`dav_seal::test_reports`); read of a legacy plaintext
  event followed by a sealing write (`dav_seal::test_legacy`); 304,
  `If-Match` and `If-None-Match: *` cases on sealed events
  (`dav_seal::test_conditional`, with a plain account as the oracle).
- **Closed (plan 7)** Removal of description and colour on a sealed collection; a
  creationdate-only PROPPATCH staying sealed; event PROPPATCH leaving
  the stored `size` unchanged; `tz()` of a sealed custom timezone in a
  time-range REPORT. All in `dav_seal::test_collections`.
- Whether a key account's quota returns to baseline after a sealed
  collection is deleted (unasserted).
- Policy tests for UID on VALARM, VTIMEZONE, VCALENDAR, STANDARD and
  DAYLIGHT; a multibyte-name carrier case, an empty component and an
  `X-`-only component for `tree_has_carriers`.
- The 640-byte key-mode quota overhead is empirical: a change in the sealed
  name size would break key-mode `put_get` with an opaque failure (the
  constant's comment names the cause).

Structure:

- The duplication between `za_event_view` and `za_calendar_view`, and
  between `za_freebusy_access` and `za_session_keys`. The polish wave
  `ac0bcdca` already folded the `propfind.rs` loader insertion into the
  `za_archive_view` helper, gave `seal_error` neutral wording, and added the
  PROPFIND `getetag` == PUT ETag test.
- `is_za_entry` duplicates the prefix test in `tree_has_carriers` (Task 3
  re-review); duplicate aad helpers; `mkcalendar_body` test helper duplicates
  the client's body builder; one extra `is_key_account` lookup on every
  MKCOL and PROPPATCH before upstream's checks.
- The plan 2 task text predates the amendments (guards, the ruled policy
  changes, `za_event_view`); the rulings file and this note win.

## Facts for plan 3

Interfaces from `groupware::calendar::seal` (`mod.rs` re-exports; `policy`,
`tree`, `event` and `collection` are public modules):

- `seal_event(event: &mut CalendarEvent, keys: &SessionKeys, account_id: u32)
  -> Result<(), SealError>`: fresh DEK per call; refuses an empty tree, a
  non-VCALENDAR root, and keys of another account.
- `unseal_event(event: &mut CalendarEvent, keys: &SessionKeys, account_id:
  u32) -> Result<(), SealError>`: plaintext passes through; discard the
  target on `Err`.
- `unseal_event_archive(stored: &Archive<AlignedBytes>, keys: &SessionKeys,
  account_id: u32) -> Result<Archive<AlignedBytes>, SealError>`: read-only
  view; version equals the stored version.
- `seal_calendar(calendar: &mut Calendar, keys: &SessionKeys, account_id:
  u32) -> Result<(), SealError>`, `unseal_calendar(..)` and
  `unseal_calendar_archive(..)` with the same shapes;
  `calendar_is_sealed(calendar: &Calendar, account_id: u32) -> bool`;
  `COLLECTION_MARKER = vault::ZA_MARKER`.
- `SealError { NotSealed, Structure(&'static str), Format, Decode, Aead,
  Policy(u8) }` (nothing returns `NotSealed` today);
  `seal_error(err, account_id, document_id) -> trc::Error` (a
  `StoreEvent::DataCorruption`, content-free);
  `tree_has_carriers(ical: &ICalendar) -> bool`, the test for a client body
  carrying any `X-ZA-*` name.
- In `tree`: `SEALED_PROP`, `KEY_PROP`, `EXTRA_PROP`; `seal_tree`,
  `unseal_tree`, `seal_bytes`, `open_bytes`, `tree_aad`, `is_carrier`,
  `text_entry`, `entry_text`. `is_sealed(&CalendarEvent)` is `pub(crate)`
  in `event.rs`, so dav cannot call it. `policy::POLICY_VERSION = 1`,
  `is_visible_property`, `is_visible_parameter`.

Interfaces from `dav::common::za` (all `pub(crate)`):

- `trait ZeroAccessGate` on `Server`: `za_session_keys(&AccessToken,
  account_id) -> Result<Option<Arc<SessionKeys>>>`,
  `za_freebusy_access(&AccessToken, account_id) -> Result<ZaFreeBusy>`,
  `za_refuse_cross_account(from_account_id, to_account_id) -> Result<()>`.
- `enum ZaFreeBusy { Plain, Unsealed(Arc<SessionKeys>), Withheld }`.
- `za_calendar_report_uri(DavResourceName) -> Result<()>`,
  `za_event_view` and `za_calendar_view(stored, keys:
  Option<&Arc<SessionKeys>>, account_id, document_id) ->
  trc::Result<Cow<Archive<AlignedBytes>>>`, and the free `async fn
  za_archive_view(server, access_token, account_id, document_id,
  collection, stored)` that the PROPFIND loader calls (gate only for
  calendar collections, borrowed archive otherwise).
- Plan 1 interfaces still apply: `AccessToken::za_keys_for(account_id)`,
  keyless `Server::authenticate`, keys attached only by the HTTP layer.

Plan 3 must:

- Gate the JMAP calendar methods (`Calendar/*`, `CalendarEvent/*`, and the
  linked-blob download for CalendarEvent) and fail closed on WebSocket and
  EventSource calendar access for key accounts.
- Close every keyless writer into a key account (iMIP, JMAP, internal
  scheduling), and refuse or strip root `X-ZA-*` from keyless writers.
- Remove the two trace sites that print plaintext (`query.rs:235`,
  `dates.rs:178`) and keep plaintext out of the iTIP task queue; turn iTIP
  off for key accounts.
- Replace R6's empty free-busy with a per-recipient refusal in the
  scheduling outbox POST attendee loop (`3-gating.md:98-101,187`).
- Repeat the data check at `setup` (plan 1 and R4 carry-over).
- Handle the shared-listing 403 (answer per item or skip) and revoke
  existing grants on key-owned calendars.
- Optionally validate the default-calendar name against `$za$`, and decide
  decision 7 (`CalendarEvent.preferences`).
- Key-mode `webdav_tests` skips `copy_move` (key-account MOVE into a group
  calendar is refused by design), `acl`, `cal_alarm` and `cal_scheduling`;
  plan 3 should re-enable whichever it makes pass.

Test facts:

- The za suite module order is `setup, disabled, password, app_password,
  totp, registry, cors, caches, dav_gate, dav_seal::{test, test_reports,
  test_collections}`; each module uses a fresh key account and ends with
  `assert_is_empty`, with key accounts destroyed at the end.
- Helpers in `tests/src/za/dav_seal.rs`: `EVENT` (a fixture with a canary in
  every sealed slot), `CANARIES`, `raw_event` and `raw_calendar` (read the
  stored archive straight from the store, to assert no canary is at rest).
- Key-mode quota: `SEALED_COLLECTION_QUOTA_OVERHEAD = 640` in
  `tests/src/webdav/mod.rs`. Rate limit: the za suite sets
  `rate_limit_anonymous` to 10000 per 60 s in `tests/src/za/mod.rs`.
- `DummyWebDavClient::mkcol` does not declare the `C:` prefix, so
  `dav_seal.rs` builds MKCALENDAR bodies itself (`mkcalendar_body`); the
  client's `proppatch` takes four arguments (`path, set, clear, headers`).
- Upstream's `cal_itip` sub-test of `webdav_tests` stays timing-flaky; rerun
  once before treating a failure as a regression.
