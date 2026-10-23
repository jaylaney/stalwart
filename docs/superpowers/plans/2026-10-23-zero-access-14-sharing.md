# Zero-access plan 14: calendar sharing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let an owner grant view or edit access to a calendar to another local key account,
with encrypted collection keys, current-grant authorization, safe revocation, and the
delegated scheduling behavior specified in revision 13.

**Architecture:** Keep the existing stored Calendar and CalendarEvent layouts. Add versioned
rings and bundles in their existing carriers, a session-owned collection-key cache, and an
archive-backed key-domain resolver shared by DAV, the drain and contacts redelivery.
Fence every event write against its calendar before enabling grants; use upstream scheduling
builders for delegated delivery and enable foreign DAV access after those paths are complete.

**Tech Stack:** Rust 2024; existing Stalwart 0.16.26 workspace, calcard, rkyv checked decoding,
XChaCha20-Poly1305 and X25519 primitives, RocksDB product configuration, existing za and
WebDAV integration harnesses. No new dependency, setting, permission or trace event.

**Spec:** [Calendar sharing design, revision 13](../specs/2026-10-22-zero-access-sharing-design.md),
updated alongside this plan from revision 11 (`0c47b7f0`). This plan was checked against source at
`c3e30e22292ee3d0c27dd0b4f8a6b04fc90b5f17` in
`.worktrees/release3-mail-spec`, branch `release4-sharing-spec-r1`.
The [plan review](2026-10-23-zero-access-14-sharing-review.md) prompted the corrections
recorded in its dispositions; Jay approved D1–D4. References such as §5.5 and case 22
mean sections/cases of the current spec.
The cited release 1–3 and key-pinning contracts remain binding.

**Status:** Proposed implementation plan, 2026-10-23. No implementation, build, runtime test,
benchmark or native-client check was performed while writing it. The request to plan does
not start implementation or authorize a push. Preserve the unrelated untracked mail review.

## Global Constraints

- No existing stored struct changes layout; no hand edits to generated registry schemas or
  trace enums. New versioned payload types in existing encrypted carriers are permitted.
- `POLICY_VERSION = 1`; CK length 32 bytes; random key-domain id 16 bytes per conversion;
  generations start at 1. Never drop a generation while its calendar exists.
- Wrap bytes: event/bundle CK `0x03`, owner ring `0x04`, grant ring `0x05`. Preserve
  existing `0x01` account and `0x02` pending-item/mail formats.
- Purposes are exactly `ck-owner`, `ck-grant`, `ck-dek`, `ck-bundle`,
  `ck-calendar-bundle`, `ck-shared-dek` and `ck-shared-bundle`. Implement §3.2's AAD table
  literally; content/tree/token AAD remains the owner's account and UID.
- Owner ring keys and ledger are encrypted under the owner's EWK. Grant wraps use the
  pinned X25519 key. Rotation uses recorded ledger keys, never newly served directory keys.
- CKs are cached only inside `SessionKeys` by (owner, calendar, domain id, generation).
  Current authorization and current generation come from a fresh calendar read on each
  write. Read responses may memoize contexts within one request only (Task 3).
  Keep `za_keys_for` unchanged; never manufacture an owner's session for a grantee.
- View bits: `Read | ReadItems`. Edit adds `Modify | AddItems | ModifyItems | RemoveItems`.
  No other ACL combination is granted. Group membership and impersonation confer no access.
- Conversion batches contain at most 100 events; share mutations make at most 8 decision
  attempts. Successful conversion chunks do not consume that budget. Conversion assertion
  failures have a separate limit of 8 per request, covering calendar and per-event losses;
  Task 6 defines the HTTP handoff and when a loss also consumes a decision attempt.
  A calendar-conflict retry for a DAV event write happens at most once; calendar PROPPATCH
  keeps upstream's immediate 409. Drain losses remain transient item failures.
- A shared event has exactly one parent. A revoke rotates in its own atomic grant mutation;
  it does not reseal existing events. New grantees receive every generation.
- Grant/revoke permission is `DavCalAcl`. Share endpoint input validation precedes password
  verification. Endpoint verification creates request-scoped keys; no key-cache lookup.
- Shared dead properties, including Apple colour/order, remain shared. Personal names,
  descriptions and custom time zones are per preference entry. Grant copies the owner's
  current preference values with flags cleared; it does not track future owner changes.
- Share notifications retain upstream's empty name. Traces contain ids, levels and counts,
  never content, addresses, fingerprints, keys or endpoint bodies.
  Fixed refusal details are exactly `zero-access: calendar access without a collection key`,
  `zero-access: copy into or out of a shared calendar`,
  `zero-access: event linked into a shared calendar` and
  `zero-access: delegated sender not verified`.
- The writer uses its own session keys and pins. UID lease and SEQUENCE floor use the event
  owner's account. The exception changes snapshot `force_send` and `rsvp` only on listed
  writer addresses L; routing uses the broader recipient set R (Shared contracts). Never change
  `is_server_scheduling`, payload parameters, or receiver validation.
- Accepted inherited behaviors in §6.2 remain accepted. A difference between the owner and
  delegated lifecycle runs beyond the RSVP/PARTSTAT exception is a defect.
- Ordinary accounts, JMAP calendar refusals, CalDAV ACL refusal, scheduling inbox ownership,
  mail-sharing refusal and cross-account COPY/MOVE refusal retain their current behavior.
- Every Cargo command starts with `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`.
  Product builds use `--no-default-features --features rocks`; never deploy `test_mode`.
- Run integration suites sequentially; they share server ports. Select whole suites, not za
  submodules. Preserve the existing drain-skip default and clean up every fixture.

## Review Focus

These additional input classes get explicit tests in their owning tasks, alongside the
spec's numbered cases:

1. Truncated or overflowing ring headers, duplicate generations and mismatched key counts:
   refuse before allocating unchecked sizes or using a cached key (Task 1).
2. A calendar and the event/UID assertion both change before retry: retain the original
   event/fence assertion so the retry cannot overwrite a concurrent edit (Task 4).
3. A request dies after a grant/rename commit but before boundary completion: initial sync
   remains usable, incremental sync stays refused, and the owner's later read repairs it
   from a post-commit change id (Tasks 5, 7 and 10).
4. A MOVE overwrites a scheduled destination in another domain: authorize/fence both
   calendars, keep upstream's `send_itip=false` for overwrite and no scheduling for unlink, assert no
   pending scheduling items, and preserve the encrypted payload while rewrapping its DEK (Task 5).
5. A multi-calendar response contains an inaccessible calendar and a corrupt accessible
   one: omit the former, retain the latter's item error, and serve the healthy items
   without leaking foreign paths (Tasks 3 and 10).
6. A failed share batch gives no indication which assertion failed: fresh reads classify
   terminal account/vault losses; a rebuilt DAV write's second loss ends its retry (Tasks 4, 7).
7. An unlisted writer sub-address resolves to the writer's account: second-run provenance
   admits its one self item, first-run output cannot double-send or record a false refusal,
   and receiver validation stays unchanged (Tasks 8–10).
8. Calendar PROPPATCH cannot insert a missing preferences entry; a large listing resolves
   each calendar once per request, while writes always resolve afresh (Tasks 2, 3, 10).
9. Catch-all resolution preserves quiet lookup for R classification and delegated sender
   verification, including successful lookups with no eventual message. Cold and warm cache
   traces contain neither the catch-all target's domain nor its local part (Task 9).

## Decisions made for implementation

These settle §13.1 within the accepted design; they are planning choices, not new product scope.

- Use one plan because the format, write fences, conversion and access gates form one
  release contract. Tasks below are independently reviewable checkpoints, not independently
  deployable releases.
- Add `groupware::calendar::sharing::{format,resolve,boundary,convert,mutations}`.
  Keep pure crypto/domain code with the seal code; HTTP request/response types stay in
  `http::api::shares`. Add `calendar::delegation` for the second scheduling run.
- Use the existing `AssertValue::Archive` for calendar fences and compare the complete
  `ArchiveVersion` when classifying a failed write. It compares the stored content hash,
  not the change-id field (`crates/store/src/write/assert.rs:71`). Never hash
  `Archive<AlignedBytes>::as_ref()` as if it were the raw stored bytes.
- Always read calendar archives on key-account event writes. Read every distinct parent
  for linked events, and both source and destination for MOVE; no resource-cache hint
  replaces the conversion/revocation fence.
- Use big-endian encoding for all fixed-width unsigned header numbers, including the
  `u16` count and `u64` change ids. The encrypted rkyv payload retains its own encoding.
  Reject malformed/duplicate/unsorted generations and checked-arithmetic overflow.
- Share `ZaCalendarContext` with all data writers, but retain separate writer
  `SessionKeys` for pins and sending identity. Keep unsealed view archives read-only;
  their copied version does not make them valid update inputs.
- Conversion rollback is a resumable state machine using `converted` and `reverting`,
  with a fresh random domain id on any later conversion.
- Boundary completion receives the commit's assigned id. Read back its archive and accept
  it as that commit's archive only if `version.change_id() == Some(assigned_change_id)`;
  assert that read-back version. Otherwise read the new archive first and the owner's
  last change id second. Never reuse the earlier commit id to clear newer pending flags.
- `shares` enumerates candidate owners with `shared_accounts(Collection::Calendar)` and
  resource caches, then checks
  the caller's exact ACL entry and opens only its own ring/view. Ledger addresses remain
  authoritative for an owner's grant list and stable grant id is preferred for revoke.
  Candidate enumeration can include membership; it grants no authority. The foreign gate
  uses a new exact `has_shared_access(owner, Collection::Calendar)` accessor (Task 3),
  not `has_access`, whose membership shortcut would broaden the gate.
  The `access_to` entry itself can derive from group ACLs: this accessor only removes the
  `is_member` shortcut. The fresh calendar's own-id `acls` entry remains the authority.
- Native client behavior is a checklist gate. The Stalwart plan delivers an API handoff
  document; implementing or editing the separate Circulo account-page repository is not
  part of this plan.
- Measure first conversion with 3,000 events, plus an interrupted/resumed run, on the
  implementation machine. Record hardware, build, dataset and elapsed time; do not invent
  a latency promise or use a production calendar.

## Files and interface ownership

Paths below are relative to the repository root. Line references identify the source snapshot;
follow symbols if implementation moves them.

| File/module | Responsibility | Task |
|---|---|---|
| `crates/vault/src/session.rs` | Session-only CK cache and cache identity | 1 |
| `crates/vault/src/keys.rs` | Primitive grant-wrap and purpose-disjointness units | 1 |
| `crates/groupware/src/calendar/mod.rs` | Sharing and delegation module registration | 1, 8 |
| `crates/groupware/src/calendar/sharing/{mod,format}.rs` | Ring headers, new payloads, AAD, entry-set assembly | 1, 6, 7 |
| `crates/groupware/src/calendar/seal/domain.rs` | Crypto-only `ZaKeyDomain` and CK envelopes | 2 |
| `crates/groupware/src/calendar/seal/{event,collection,tree,mod}.rs` | Domain-aware codecs, views, envelope-only rewrap | 2 |
| `crates/groupware/src/calendar/sharing/resolve.rs` | Fresh archive, own-id ACL check, domain and fence | 3, 4, 5, 6 |
| `crates/dav/src/common/za.rs` | DAV resolver/error/view adapters, repair and eventual gate | 3, 5, 10 |
| `crates/dav/src/calendar/{get,freebusy,update,proppatch,delete,mod}.rs` | Readers, event-write fences and delegated integration | 3, 4, 9, 10 |
| `crates/common/src/auth/access_token.rs` | Collection access entry check without is_member shortcut | 3 |
| `crates/groupware/src/calendar/{storage,drain}.rs`; `crates/http/src/api/contacts.rs` | Non-PUT data writers and delete plumbing | 4, 9 |
| `crates/groupware/src/calendar/sharing/boundary.rs`; `crates/dav/src/calendar/copy_move.rs` | Boundary completion, parent rules and domain moves | 5 |
| `crates/groupware/src/calendar/sharing/convert.rs` | Conversion, rewrap, rollback, resume | 6, 7 |
| `crates/groupware/src/calendar/sharing/mutations.rs`; `crates/http/src/api/shares.rs` | Transactional decisions, rotations, API | 7 |
| `crates/http/src/api/{mod,vault}.rs` | Narrow helper visibility and route registration | 7 |
| `crates/groupware/src/calendar/delegation.rs` | Second builders, resolution, combining and provenance | 8, 9 |
| `crates/common/src/cache/principals.rs` | Quiet address resolution through the catch-all fallback | 9 |
| `crates/groupware/src/scheduling/event_cancel.rs` | Snapshot-taking cancel seam | 8 |
| `crates/groupware/src/calendar/fanout.rs`; `crates/dav/src/calendar/{update,delete}.rs` | Event-owner identity and message integration | 9 |
| `crates/dav/src/common/propfind.rs` | Request read memo, discovery and safe sync | 3, 10 |
| `crates/dav/src/{common/uri,principal/propfind}.rs` | Foreign admission and discovery | 10 |
| `crates/common/src/za/test_hooks.rs` | Existing-feature-only pause/failure statics and functions | 4–7 |
| `tests/src/za/{sharing,sharing_races,mod,leak}.rs`; `tests/src/utils/za.rs` | Behavioral/race harness, cleanup and leak rules | All |
| `tests/src/webdav/za_variants.rs` | Extend key-mode ACL behavior | 10 |
| `docs/zero-access/{calendar-sharing-api,manual-checklist}.md`; developer/outcome notes | API handoff and qualification evidence | 12 |

Do not concurrently assign writers to `crates/groupware/src/calendar/seal/*` ,
`crates/groupware/src/calendar/sharing/*` , `update.rs` , `delete.rs` ,
`copy_move.rs`, `test_hooks.rs` or the common test modules. The primary owns staging, commits,
integration and the shared Cargo build directory.

## Shared contracts

The signatures below are **new/proposed** unless marked existing. They are the contracts
neighboring tasks consume; private implementation helpers can follow local style.

- `CollectionKeyId { owner_id: u32, calendar_id: u32, domain_id: [u8; 16],
  generation: u32 }` in `vault::session`; hashable/equatable, no key bytes.
  `SessionKeys::collection_key(&self, id: &CollectionKeyId) -> Option<Secret>` and
  `cache_collection_key(&self, id: CollectionKeyId, key: Secret)` use a short-lived mutex
  guard and clone the zeroizing `Secret`; no lock is held across an await.
- `OwnerRingHeader` has `calendar_id, domain_id, rename_change_id, boundary_pending,
  generations: Vec<u32>`. `GrantRingHeader` additionally has `grantee_id, grant_change_id`.
  `ZaOwnerRing` and `ZaGrant` have exactly §3.3's new payload fields. Opened key/ledger
  material and serialization scratch are zeroized and never derive content-bearing Debug.
- `ZaKeyDomain` owns request-local key material: owner id, an optional owner EWK, and for a
  shared calendar its id, domain id, ordered `BTreeMap<u32, Secret>` and reverting flag.
  A grantee has no EWK. Reads choose the stored envelope's generation; writes choose the
  newest generation unless reverting. Construction is restricted to the resolver/conversion
  layer. Old account-only seal entry points remain wrappers around an owner domain.
- `ZaCalendarContext { owner_id: u32, calendar_id: u32, personal_id: u32,
  stored: Archive<AlignedBytes>, domain: ZaKeyDomain, boundary: Option<ZaSyncBoundary> }`,
  where `ZaSyncBoundary { grant_change_id: u64, rename_change_id: u64, pending: bool }`.
  `assert_in(&self, batch: &mut BatchBuilder)` asserts the stored calendar under its owner.
  It changes the batch context; callers restore the event's account, collection and document
  before appending event operations.
- `za_calendar_context(server: &Server, token: &AccessToken, owner_id: u32,
  calendar_id: u32, required: Bitmap<Acl>) -> Result<Option<ZaCalendarContext>, ZaCalendarError>`
  is async; `None` is the ordinary-account path.
  `za_owned_calendar_context(server: &Server, keys: &SessionKeys, calendar_id: u32)
  -> Result<ZaCalendarContext, ZaCalendarError>` is async for drain, conversion and contacts.
  The latter derives owner solely from `keys.account_id`.
  Resolution itself is read-only. Task 5 adds an explicit owner-only boundary-repair call
  to DAV read adapters; `shares` listing never invokes repair.
- `ZaCalendarError` distinguishes `Forbidden`, `LinkedEvent`, `Seal(SealError)` and
  `Store(trc::Error)`. Adapters retain existing missing-resource handling; direct refusal
  becomes 403, linked writes 409, corruption a request/item 500. Listing omission is explicit,
  not a conversion of all failures to empty results.
- Existing reads return `Archive<AlignedBytes>`; `to_unarchived::<Calendar>()` returns
  `Archive<&ArchivedCalendar>`. `Calendar::update` and `CalendarEvent::update` remain the
  indexing/quota/ACL-notification writers. `commit_batch` returns `AssignedIds`;
  `last_change_id(owner_id)` supplies the actual committed boundary.
- Define **L** as listed scheduling addresses in the merged list but not local under the
  owner-only list (writer-only groups included). **R** is L union attendee addresses from
  either tree resolving to the writer's account, always excluding owner-local addresses.
  L alone gets the snapshot RSVP/PARTSTAT override; R controls retained second-run recipients
  and first-run removal. Use upstream locality before normalization, then
  `za_normalize_address(&attendee.email.email)` and the same helper on message `to` keys.
  Do not rewrite payload addresses. Only a retained recipient actually resolving to the
  writer may enter its inbox; group targets retain normal delivery/status checks.
- Existing `za_fan_out` has this full proposed signature (Task 9):
  `fn za_fan_out(&self, sender: &AccountInfo, event_account: &AccountInfo,
  keys: &SessionKeys, messages: Vec<ZaFanOutMessage>, stored_pairs: Option<TokenPairs>,
  event: &ICalendar, source: ZaSource<'_>, kind: ZaFanOutKind, fence: Option<&ZaUidLease>,
  resolution: Option<&ZaDelegationResolution>)
  -> impl Future<Output = trc::Result<ZaFanOutResult>> + Send`.
  `sender` remains the writer's merged scheduling account info; `event_account` is the
  owner's own account info. Own/plain call sites pass their own account. No stored layout
  changes are needed.
  Its message input carries in-memory `provenance` (`Upstream` or `DelegatedSelf`), with
  request-local recipient-resolution results passed separately. Groupware's
  `za_combine_delegated` constructs `DelegatedSelf`; DAV never constructs that tag. Ordinary,
  attendee-side, drain and contacts call sites wrap upstream messages and pass `None` for
  resolution. Nothing in a client tree or stored item can set provenance.
- All new integration cases are async helpers called by the existing `za::za_tests` suite.
  `sharing::test(test: &mut TestServer)` and
`sharing_races::test(test: &mut TestServer)` are async, matching neighboring modules. Register after
  pinning and before leak scans. Reuse `za_unlock` ,
  `plant_event`, `HookParked`, `za_post_concurrent`, `trigger_drain` and `copy_href`.

## Execution and verification conventions

Use the existing requested checkout for planning. At implementation start inspect its clean
state and attached worktrees, then use an isolated implementation branch/worktree consistent
with repository guidance. Do not discard or stage the unrelated mail review. Read `AGENTS.md`,
`CLAUDE.md` and `README-dev.md` again if changed.

For each task: write its tests, observe the targeted failure, implement, run the stated gate,
inspect the diff, obtain fresh independent review for the changed contract, then commit only
the owned files with the suggested subject. A compilation failure for a new symbol can establish
the initial red phase; behavioral tests must ultimately fail on the relevant broken behavior.
The assertion snippets below specify the essential assertions; fixture construction uses the
existing harness and does not require a new test framework.

Commands used by task gates:

~~~sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo test -p vault -p groupware -p common
~~~

~~~sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
~~~

Use the first command's affected packages for unit-only tasks; use the full za suite for
integration tasks. Do not pretend `sharing::test` is a Cargo-selectable test.
Foreign DAV activation is deliberately last. Before Task 10, owner HTTP tests and direct
resolver/codec/transaction/fan-out tests prove the new internals using disposable store
fixtures; a grantee HTTP request still gets the existing gate's 403. Do not park such a
request at a downstream hook it cannot reach. Tasks 4, 7 and 9 prepare the corresponding
grantee HTTP cases for activation in Task 10; keep their test definitions with that task's
activation commit rather than adding permanently ignored tests or weakening the gate.
Task 10 must run every specified HTTP variant after access opens.

Before Task 11 updates the leak scanner, Tasks 3–10 revoke grants and delete every created
shared calendar before the suite reaches `leak::test`; revocation alone leaves the owner-ring
form and is insufficient. Default-calendar fixtures use dedicated accounts, destroyed after
verification and cleanup. Only Task 11 creates healthy shared fixtures that survive until scanning.

No parallel suite runs. A task touching product call sites also requires a successful
`cargo check -p stalwart --no-default-features --features rocks` with the PATH prefix;
new code must introduce no warnings.

### Task 1: Ring formats and the session CK cache

**Files:** Create `crates/groupware/src/calendar/sharing/{mod,format}.rs` ; modify
`crates/groupware/src/calendar/mod.rs` ,
`crates/vault/src/{session,keys}.rs`. Reuse existing public seal/tree helpers without widening
exports. Unit tests beside the codecs/cache; primitive grant-wrap and `pk-dek` purpose/AAD
disjointness tests live in `vault/src/keys.rs` per §11.1, using fixed §3.2 AAD test vectors.

**Interfaces:** Produce `CollectionKeyId`, cache methods, headers and §3.3 payloads.
In `format.rs` expose `parse_owner_header(&[u8]) -> Result<OwnerRingHeader, SealError>`,
`parse_grant_header(&[u8]) -> Result<GrantRingHeader, SealError>`;
`open_owner_ring(bytes: &[u8], owner: u32, calendar: u32, ewk: &Secret)
-> Result<(OwnerRingHeader, ZaOwnerRing), SealError>` and
`seal_owner_ring(header: &OwnerRingHeader, ring: &ZaOwnerRing, owner: u32, ewk: &Secret)
-> Result<Vec<u8>, SealError>`.
Grant codecs are `seal_grant_ring(header: &GrantRingHeader, keys: &BTreeMap<u32, Secret>,
owner: u32, public_key: &[u8; 32]) -> Result<Vec<u8>, SealError>` and
`open_grant_ring(bytes: &[u8], owner: u32, calendar: u32, principal: u32,
domain: [u8; 16], private_key: &Secret)
-> Result<(GrantRingHeader, BTreeMap<u32, Secret>), SealError>`.
Also add `pub(crate) fn za_replace_calendar_entries(calendar: &mut Calendar,
owner_id: u32, calendar_id: u32, entries: Vec<CalendarPreferences>, acls: Vec<AclGrant>)
-> Result<(), SealError>` in `format.rs`. This assembles an explicitly prepared entry set
and checks carrier shapes, header identity and ACL/entry correspondence; it does not authorize,
decrypt or commit. Conversion/mutations prepare changed entries with the ring/bundle codecs,
preserve untouched sealed entries, verify ledger correspondence, then use this helper followed
by `Calendar::update`. Rollback supplies the owner-only unshared form after its final scan.

- [ ] **1. Add failing unit tests** `ring_round_trip_and_identity_binding`,
  `ring_headers_reject_malformed_input`, `grant_and_pk_dek_purposes_are_disjoint` and
  `collection_cache_separates_reconversions`. Mutate each identity/AAD field independently;
  test every truncation boundary, invalid flag, unsorted/duplicate generations, bad count,
  payload/header disagreement, unknown version and generation exhaustion.
  Essential assertions include:
  ~~~rust
  assert_eq!(opened_header, header);
  assert_eq!(opened_keys[&1].as_bytes(), key.as_bytes());
  assert!(wrong_principal_open.is_err());
  assert!(wrong_domain_open.is_err());
  assert!(invitation_as_grant.is_err());
  assert!(keys.collection_key(&different_domain_id).is_none());
  ~~~
- [ ] **2. Run vault/groupware unit tests and observe the missing-contract failures.**
- [ ] **3. Implement the exact §3.2/3.3 byte formats.** Reuse `pk_wrap/pk_unwrap`,
  `seal_padded/open_padded` and checked `decode_archive`. Owner header bytes enter owner
  payload AAD; grant records validate own principal/calendar/domain and exact generation set.
  Header metadata remains visible. Preserve old formats; add no raw key Debug output.
- [ ] **4. Run vault/groupware tests and product compile check.** Inspect zeroization,
  bounds checks and cache locking in the diff.
- [ ] **5. Commit:** `Add calendar sharing rings and session key cache`.

### Task 2: Domain-aware event and collection sealing

**Files:** Create `crates/groupware/src/calendar/seal/domain.rs` ; modify
`crates/groupware/src/calendar/seal/{mod,tree,event,collection}.rs` .
Keep the existing private `Bundle` archived layout unchanged.

**Interfaces:** Produce `ZaKeyDomain` and
`seal_event_in_domain(event: &mut CalendarEvent, domain: &ZaKeyDomain,
pairs: Option<&TokenPairs>) -> Result<(), SealError>`,
`unseal_event_in_domain(event: &mut CalendarEvent, domain: &ZaKeyDomain)
-> Result<Option<TokenPairs>, SealError>`,
`event_pairs_in_domain(stored: &Archive<AlignedBytes>, domain: &ZaKeyDomain)
-> Result<Option<TokenPairs>, SealError>`,
`event_view_in_domain(stored: &Archive<AlignedBytes>, domain: &ZaKeyDomain)
-> Result<Archive<AlignedBytes>, SealError>` and
`rewrap_event(event: &mut CalendarEvent, from: &ZaKeyDomain, to: &ZaKeyDomain)
-> Result<(), SealError>`.
For collections expose `calendar_view_in_domain(stored: &Archive<AlignedBytes>, domain: &ZaKeyDomain,
personal_id: u32) -> Result<Archive<AlignedBytes>, SealError>`, and
`seal_calendar_changes(stored: &Calendar, edited: &Calendar, domain: &ZaKeyDomain,
personal_id: u32) -> Result<Calendar, SealError>`. These retain unopened parts from `stored`.

- [ ] **1. Add failing unit tests** `rewrap_changes_only_the_envelope`,
  `shared_collection_uses_two_three_and_five_part_forms`,
  `grantee_view_never_opens_foreign_personal_material`,
  `shared_property_write_preserves_owner_parts` and `plaintext_converts_under_ck`.
  Compare ciphertext carriers directly, not just decrypted equivalence:
  ~~~rust
  assert_eq!(before_encrypted_content, after_encrypted_content);
  assert_eq!(before_token_bundle, after_token_bundle);
  assert_ne!(before_envelope, after_envelope);
  assert_eq!(stored_owner_ring, changed_owner_ring);
  assert_eq!(stored_owner_personal_parts, changed_owner_personal_parts);
  assert_eq!(view.version, stored.version);
  ~~~
- [ ] **2. Run groupware tests and observe the unsupported-format failures.**
- [ ] **3. Implement envelope selection and selective collection views/writes.** `0x03`
  is exactly 82 bytes (policy/type + calendar + generation + 72-byte wrap). Get domain id
  from the fresh ring. Owner mixed-state reads accept legacy/plaintext during conversion;
  a grantee encountering an account-wrapped event fails that item. New bundle writes draw
  fresh DEKs. Read only the requesting principal's personal bundle and the shared bundle
  for a DAV view; other entries remain sealed. Preserve tree order and all token bytes.
  `seal_calendar_changes` rejects any changed preference-entry id set, including an entry
  auto-inserted by upstream's `preferences_mut`. Only a grant creates a grantee entry;
  an ordinary property write must update an existing personal entry. This codec is for
  property writes, not grant/revoke/sweep/rollback: those use Task 1's explicit
  `za_replace_calendar_entries` assembly path after preparing the ring/ledger and bundles.
- [ ] **4. Run groupware/vault tests and product check.** Existing event/collection/item/mail
  format tests must stay green; read-only view archives are never passed as current writes.
- [ ] **5. Commit:** `Support collection-key calendar sealing`.

### Task 3: Fresh calendar resolution and data-read adapters

**Files:** Create `crates/groupware/src/calendar/sharing/resolve.rs`; modify `crates/dav/src/common/za.rs`,
`crates/dav/src/calendar/{get,freebusy}.rs` and loader view plumbing in `crates/dav/src/common/propfind.rs`.
Modify `crates/common/src/auth/access_token.rs` and `tests/src/za/mod.rs` for the accessor
and suite registration. Start `tests/src/za/sharing.rs` with owner-only fixtures and helpers.

**Interfaces:** Implement both shared resolver signatures and `ZaCalendarError` above.
Add `za_foreign_calendar_eligible(token: &AccessToken, grants: &[AclGrant]) -> bool`;
archived callers materialize only ACL ids/bits into this representation. Add
`za_event_contexts(server: &Server, token: &AccessToken, owner_id: u32,
event: &CalendarEvent, addressed_calendar: u32, required: Bitmap<Acl>)
-> Result<Vec<ZaCalendarContext>, ZaCalendarError>` for checking all parent domains.
Add async `za_owned_event_contexts(server: &Server, keys: &SessionKeys,
event: &CalendarEvent, addressed_calendar: u32)
-> Result<Vec<ZaCalendarContext>, ZaCalendarError>` for owned writers; derive owner from keys.
Add public `AccessToken::has_shared_access(account_id: u32, collection: Collection) -> bool`
in common: inspect `inner.access_to` directly, with no `is_member` shortcut. Group-derived
ACLs can populate that entry; only the fresh calendar's own-id grant authorizes use. Keep
existing accessors unchanged. `shared_accounts(Collection::Calendar)` is only a candidate iterator.

- [ ] **1. Add resolver tests** `resolver_requires_own_grant_and_all_bits`,
  `resolver_checks_fresh_ring_before_cache` and `read_errors_keep_their_scope`.
  Cover own keys, unrelated keys, impersonated token, Impersonate permission, group grant,
  missing ring, corrupt ring, unlisted generation, other-calendar envelope, stale cached
  generation and fresh domain id. Assert own-id all-bit inclusion:
  ~~~rust
  assert!(!za_foreign_calendar_eligible(&group_only_token, &grants));
  assert!(!za_foreign_calendar_eligible(&impersonating_token, &grants));
  assert!(matches!(missing_grant, Err(ZaCalendarError::Forbidden)));
  assert!(matches!(corrupt_ring, Err(ZaCalendarError::Seal(_))));
  ~~~
  Use distinct fixture prefixes `share-*` in `sharing.rs` and `race-*` in `sharing_races.rs`,
  with per-case suffixes; never reuse `key1..3` or other modules' accounts. Allocate isolated
  numeric owners in units. Add a request-scoped read-count check for a multi-item listing.
- [ ] **2. Run groupware tests and za suite; establish failures on current resolution.**
- [ ] **3. Implement the context and adapters.** Read the stored calendar afresh for every write.
  For reads, memoize `ZaCalendarContext` by (owner, calendar) in the request's `PropFindData`
  and pass that memo through loader/query/multiget views. Each use still checks the operation's
  required bits; never reuse a context across requests or let a write consume a read memo.
  This uses §4.6's accepted in-flight read window, not a persistent authorization cache.
  Owner opens its ring; grantee opens only its own ring using `za_private_key`. Check
  eligibility and all operation bits before any cache hit. The cache spares unwraps only.
  Wire owner GET/HEAD, loader, query/multiget and owner free-busy to domains; keep the
  foreign URI gate closed until Task 10. Preserve outer access errors versus inner unseal
  errors in `za_archive_view`. Task 5 explicitly wires pending-boundary repair into these
  DAV read adapters; keep the resolver usable without repair by `shares` listing.
- [ ] **4. Run the unit/za gates and product check.** Case 13's password/recovery fixture
  can begin here; prove the owner can read mixed conversion states.
- [ ] **5. Commit:** `Resolve calendar key domains from current archives`.

### Task 4: Fence every event write against its calendars

**Files:** Modify `crates/dav/src/calendar/{update,proppatch,delete,mod}.rs`,
`crates/groupware/src/calendar/{storage,drain}.rs`, `crates/http/src/api/contacts.rs`,
`crates/groupware/src/calendar/sharing/resolve.rs`, `crates/common/src/za/test_hooks.rs`
and `tests/src/za/mod.rs` for race-suite registration.
Create `tests/src/za/sharing_races.rs`; extend `tests/src/utils/za.rs` only for shared helpers.

**Interfaces:** Consume Task 3 contexts and Task 2 codecs. `ZaCalendarContext::assert_in`
sets owner/Calendar/document context and asserts the original archive. Add
`calendars_changed(before: &[ZaCalendarContext], after: &[ZaCalendarContext]) -> bool`
using full `ArchiveVersion` equality. Extend existing `delete_za/delete_all_za` inputs to
take the resolved domain separately from writer keys; collection deletion resolves each
event's domain before unsealing.

- [ ] **1. Add failing write-fence tests** for cases 21, 22 and 24's linked-write refusal,
  using staged/rotated owner fixtures until share endpoints exist. Test grantee retry
  authorization through the resolver/commit seam until Task 10 enables its HTTP variants. Add
  `calendar_retry_keeps_event_and_uid_assertions` and
  `non_put_writers_use_current_domain`. Keep `COMMIT_PAUSE` for create; add and hold
  `UPDATE_COMMIT_PAUSE` for update PUT and `DELETE_COMMIT_PAUSE` for DELETE. Case 22 uses
  two independent runs/holds for Alice's and Bob's updates, with fresh fixtures and re-armed
  hooks, never two simultaneous waits on the same account's one-shot slot.
  Case 21 runs both create and update variants. A refused held write leaves any already
  delivered items standing: these hooks follow fan-out, so assert unchanged source data,
  not the absence of recipient items.
  Assert:
  ~~~rust
  assert_eq!(stored_generation, newest_generation);
  assert_eq!(revoked_writer_status, 403);
  assert_eq!(concurrent_event_conflict_status, 409);
  assert_eq!(after_retry_payload, before_retry_payload);
  assert_eq!(fanout_items_after_retry, fanout_items_before_retry);
  assert!(event_still_exists_after_refused_delete);
  ~~~
- [ ] **2. Run za suite and observe unfenced old-generation/revoked-write failures.**
- [ ] **3. Integrate contexts at PUT create/update, event PROPPATCH, DELETE, drain import/
  merge and both contacts reseal paths.** Every seal/rewrap checks every distinct parent;
  shared plus another parent is 409, including access through the unshared path. Unlink-only
  DELETE remains possible without resealing. Keep the owner account for UID lease, index,
  quota and associated data. Place calendar assertions before event sets/delete commit points.
  In `common::za::test_hooks`, add the two named pause statics, setters and pause functions
  using its existing `Pause` shape. Call directly under `cfg(feature = "test_mode")` just
  before update's `commit_batch` (`update.rs:405`) and DELETE's (`delete.rs:276`), keyed by
  the event owner. Leave create's existing hook at `update.rs:631` unchanged and document all three.
- [ ] **4. Implement one retry at the caller's commit boundary.** Re-read calendar(s) on
  `AssertValueFailed`; if unchanged, retain existing event conflict or
  `za_lost_create_fence` behavior. If changed, reauthorize, rewrap only the already sealed
  DEK envelope, rebuild once with fresh calendar assertions and the original event/fence,
  index and alarm operations. Rebuild DELETE with `send_itip=false` and do not invoke its
  fan-out again. Never repeat fan-out. A concurrent event failure must still
  fail. Drain returns a transient item error; contacts retains its refusal-id guard and
  counts unsafe linked reseals as failed. Calendar PROPPATCH has no retry.
  A second commit loss ends the retry without another calendar read: update/DELETE returns
  409, create uses `za_lost_create_fence`, drain returns a transient item error. On retry
  authorization/link refusals, drop or release the UID lease guard; `abandon()` is only for
  a confirmed lost lease fence, never an ordinary 403/409.
- [ ] **5. Run za, affected units and product check.** Inspect delete chunks and verify the
  calendar assertion is in the actual committed event chunk, including unlink/delete-all,
  and the caller restores event account/collection/document context after `assert_in`.
- [ ] **6. Commit:** `Fence calendar event writes against sharing changes`.

### Task 5: Domain moves and durable sync boundaries

**Files:** Create `crates/groupware/src/calendar/sharing/boundary.rs` ; modify
`crates/dav/src/calendar/copy_move.rs` ,
`crates/groupware/src/calendar/sharing/resolve.rs`, `crates/dav/src/common/za.rs`,
`crates/common/src/za/test_hooks.rs` and `tests/src/za/sharing_races.rs`.

**Interfaces:** Add `ZaBoundaryCommit { change_id: u64 }` and async
`za_complete_boundary(server: &Server, keys: &SessionKeys, calendar_id: u32,
own_commit: Option<ZaBoundaryCommit>) -> Result<Archive<AlignedBytes>, ZaCalendarError>`.
Read the archive back after commit; use `own_commit.change_id` only when the read-back
`version.change_id()` equals it. Assert that read-back archive version in step 2; the prepared
archive does not expose its eventual serialized hash. Otherwise use a fresh archive followed
by the owner's `last_change_id`, with that archive's assertion. Pure
`complete_boundary(calendar: &mut Calendar, domain: &ZaKeyDomain, change_id: u64)
-> Result<(), SealError>` updates flagged headers and zero grant ids in the owner's ledger,
preserving every key. Only an owner with its EWK may repair. On a lost repair assertion,
return the previously read pending archive and let the read proceed safely; do not retry
indefinitely or claim the boundary is complete. Other store/seal failures retain their normal
error handling. `shares` listing does not call repair and writes nothing beyond its self-check.

- [ ] **1. Add failing tests** for case 12, case 24's post-precheck link and
  `boundary_completion_cannot_clear_a_newer_pending_write`. Cover event rename, collection
  rename, source/destination domains, linked MOVE, overwrite and interrupted step 2:
  ~~~rust
  assert_eq!(forbidden_copy_status, 403);
  assert_eq!(moved_parent_ids, vec![destination_id]);
  assert_eq!(moved_encrypted_payload, original_encrypted_payload);
  assert!(completed_boundary >= committed_rename_change_id);
  assert_eq!(repaired_archive.version, subsequently_read_archive.version);
  ~~~
- [ ] **2. Run za suite and observe missing parent/domain/boundary protection.**
- [ ] **3. Implement COPY/MOVE fences and restrictions.** Read/assert source, destination
  and other existing parents before link changes, including unshared COPY racing conversion.
  Refuse event COPY involving any shared calendar among source parents, destination or other
  event parents; refuse collection COPY/MOVE with a shared source or destination except the
  owner's same-document rename. Refuse a MOVE leaving a shared event linked.
  Cross-domain event MOVE rewraps only its DEK; same-domain rename keeps ciphertext.
  Preserve domain-aware overwrite deletion with `send_itip=false`; in-account MOVE unlinks
  by removing a name (`copy_move.rs:678`) and performs no source DELETE. Neither schedules.
  Cross-account source deletions remain unreachable for key accounts. Test case 12's scheduled
  overwrite against owner and ordinary-account no-CANCEL behavior: no new queued/pending items,
  and the overwritten event's attendees keep their active copies.
  Hold the existing task-consumer pause while comparing queued scheduling tasks and pending
  items before/after MOVE, so a plain iMIP CANCEL cannot be consumed before the assertion.
- [ ] **4. Implement two-step shared rename.** Step 1 writes slug and pending flag in owner
  and every grant header with the calendar assertion; reseal owner payload AAD. Step 2
  takes `AssignedIds::last_change_id(owner)`, verifies read-back change-id identity and
  completes flags under that archive's assertion, otherwise uses the post-read fallback.
  Wire the explicit repair call into Task 3's owner DAV read adapters in `common/za.rs`
  after resolution detects pending flags; mutation paths also repair. Add a
  `SHARE_BOUNDARY_PAUSE` static/setter/function in `common::za::test_hooks`, called directly
  under `test_mode` from rename/completion after destination lookup and between steps.
  Its test configuration selects `BeforeRenameCommit` or `BeforeBoundaryCompletion`; only
  that phase consumes the one-shot slot, so the earlier site cannot steal a completion hold.
- [ ] **5. Run za and product gates.** Retain initial/pending behavior tests for Task 10's
  actual HTTP sync boundary. Verify archive assertions survive every copy/move commit point.
- [ ] **6. Commit:** `Protect shared calendar moves and sync boundaries`.

### Task 6: Resumable first-grant conversion and rollback

**Files:** Create `crates/groupware/src/calendar/sharing/convert.rs`; modify
`sharing/{mod,format,resolve}.rs` for conversion-state serialization/resolution,
`crates/common/src/za/test_hooks.rs` and `tests/src/za/{sharing,sharing_races}.rs`.

**Interfaces:** Add `ZaConversionCounts { rewrapped: usize, sealed: usize }` and async
`za_convert_calendar(server: &Server, keys: &SessionKeys, calendar_id: u32,
guards: &ZaShareGuards) -> Result<ZaConversionCounts, ZaShareError>`.
Introduce `ZaShareGuards { owner_id: u32, verified_generation: u64, vault_cas: u64,
pin_assertion: Option<AssertValue> }` here; the pin field is initially None. It appends
the verified-vault and owner-registry-presence assertions to each independent conversion
batch. Task 7 adds pin assertion/decision handling. `ZaShareError` carries typed fixed
conflicts, seal/store failures, never user content in Debug.
Add `ZaShareError::ConversionConflict { committed: ZaConversionCounts,
progress_changed: bool }`. It returns control to the HTTP coordinator after an opaque chunk
assertion loss; the flag reports observed calendar/event changes, not which assertion failed.
No groupware code calls HTTP's `za_reread_verified`. Each invocation reports only counts it
committed; HTTP accumulates them once and keeps the conversion-loss budget across resumes.

- [ ] **1. Add failing tests** for cases 3, 21, 23 and all three runs of 24:
  sealed+plaintext conversion, token-preserving rewrap, crash/resume, raced-in link,
  concurrent CK write rolled back to EWK, linked-event write refusal and resident-session
  rollback/reconversion. Assert:
  ~~~rust
  assert_eq!(counts.rewrapped, sealed_event_count);
  assert_eq!(counts.sealed, plaintext_event_count);
  assert_eq!(pairs_after, pairs_before);
  assert!(!failed_conversion_has_any_grant);
  assert_ne!(first_domain_id, reconverted_domain_id);
  assert!(fresh_session_can_read_resident_session_write);
  ~~~
  Add losses where calendar/event and vault/Account change together, plus unchanged-progress
  and repeated per-event losses. Prove terminal guard classification wins and the eighth
  conversion assertion loss ends with conflict. A 3,000-event successful conversion must not
  exhaust either retry budget simply by committing thirty chunks.
- [ ] **2. Run za suite and observe the absent state machine's failures.**
- [ ] **3. Implement the three phases and their fences.** Precheck is a fast refusal;
  phase 1 installs a fresh owner ring with no grants and `converted=false`. Complete the
  parentage walk before rewrapping anything. Rewrap at most 100 events per logical batch,
  preserving sealed bytes and token carriers; fully seal plaintext. Each independently
  committed chunk has its calendar/vault/account and per-event assertions. Re-read changed
  events and skip only when they are valid in the current domain. Finish with a complete
  parentage/envelope scan before returning grant-ready state.
  On any chunk `AssertValueFailed`, re-read the calendar and that chunk's event archives,
  compare their complete versions (including presence), and return `ConversionConflict`
  with committed counts and `progress_changed`. Never infer a safe calendar-only loss from
  changed data: a vault/Account assertion may have lost in the same attempt. HTTP first
  performs Task 7's fresh guard classification on every such return, then resumes only if
  those guards still hold. Resume re-reads the current ring: `reverting` joins rollback;
  otherwise rebuild/skip changed events under the current domain. With unchanged progress,
  HTTP also consumes one decision attempt and redoes all decisions rather than guessing the
  losing key. Every conversion assertion loss, including per-event losses, consumes one of
  the separate 8 conversion-loss slots; the eighth returns 409 `conflict` after checking
  terminal guards. Successful chunks consume neither budget. Other store/seal errors propagate.
- [ ] **4. Implement rollback/resume.** A raced-in link first sets `reverting`, then
  rewraps every CK event back to EWK with per-event/calendar assertions, then restores
  unshared preferences. Keep the domain id/ring until no event needs it. Resume an interrupted
  rollback before considering another conversion; a new conversion draws a new id.
  Phase 1 and the final rollback entry-set replacement use `za_replace_calendar_entries`;
  prepare sealed owner fields through the domain codecs and preserve their plaintext values.
  They do not use the PROPPATCH-only `seal_calendar_changes` entry-set invariant.
  Add `SHARE_PRECHECK_PAUSE`, `SHARE_REWRAP_PAUSE` and `SHARE_REWRAP_FAIL_ONCE` statics and
  functions in `common::za::test_hooks`, called directly from `convert.rs` under `test_mode`,
  including between rewrap batches.
- [ ] **5. Run za/groupware/product gates.** No grant, ACL key or grant ledger entry may
  appear before the final valid scan. Check retry counts against actually committed work.
- [ ] **6. Commit:** `Convert calendars to collection keys with safe rollback`.

### Task 7: Transactional grants, revokes and account API

**Files:** Create `crates/groupware/src/calendar/sharing/mutations.rs` and
`crates/http/src/api/shares.rs` ; modify
`crates/http/src/api/{mod,vault}.rs`, `crates/groupware/src/calendar/sharing/{mod,convert}.rs`,
`crates/common/src/za/test_hooks.rs` and `tests/src/za/{sharing,sharing_races}.rs`.

**Interfaces:** Add `ZaShareLevel::{View, Edit}` with exact bitmap conversion;
`ZaRevokeTarget::{Grant(u32), Address(String)}`;
`ZaShareChange::Grant { account_id: u32, address: String, public_key: [u8; 32],
level: ZaShareLevel }` and `ZaShareChange::Revoke { target: ZaRevokeTarget }`.
`ZaShareGuards` gains optional pin CAS for a grant/re-key, using existing
`ZaPinRead::assertion`. Async `za_share_apply(server: &Server, keys: &SessionKeys,
calendar_id: u32, change: ZaShareChange, guards: &ZaShareGuards)
-> Result<ZaShareApplied, ZaShareError>` performs one authoritative decision/commit attempt.
`ZaShareApplied { changed: bool, affected_grantees: Vec<u32>, rotated: bool,
boundary_commit: Option<ZaBoundaryCommit> }` carries commit metadata, not response content. HTTP owns the eight-attempt
loop and redoes address/key/pin/calendar decisions on a retry.
Add async handlers `za_shares(server: &Server, session: &HttpSessionData,
request: CredentialsRequest) -> trc::Result<HttpResponse>`, and `za_shares_grant` /
`za_shares_revoke` with the same first two arguments and `ShareGrantRequest` /
`ShareRevokeRequest` respectively. DTOs flatten the existing credential fields and add
exactly §4.3/4.4 fields; validate revoke id/address exclusivity before authentication.

- [ ] **1. Add failing endpoint/race tests** for cases 4–8, 13–14 and 16–20. Cover every
  §4 response row, exact stable-id/alias-transfer semantics, sorted lists, all-generation
  grants, gone sweep, ledger-key rotation and same-level re-key invalidation.
  Assert exact public outcomes:
  ~~~rust
  assert_eq!(ambiguous_error["error"], "address is ambiguous, revoke by grant");
  assert_eq!(deleted_owner_error["error"], "account deleted");
  assert_eq!(changed_vault_error["error"], "vault changed since verification");
  assert_eq!(eighth_loss_error["error"], "conflict");
  assert!(!unchanged_grant["changed"].as_bool().unwrap());
  assert_eq!(remaining_grantee_generations, owner_generations);
  ~~~
- [ ] **2. Run za suite and observe missing route/transaction behavior.**
- [ ] **3. Implement input validation and fresh verification.** Reuse vault CORS,
  no-store, anonymous rate limit, JSON/problem bodies and auth accounting. Expose
  `za_reread_verified` narrowly; additionally compare its record revision with
  `verified.keys.generation` before appending its CAS assertion. Require `DavCalAcl` for
  mutations. Grant/revoke never open the owner's private key. Share contacts' quiet
  account/key resolution with a helper returning account id plus current served key.
  Preserve primary-password/TOTP failure accounting. Post-verification sharing conflicts,
  including `not a key account`, `contact not pinned`, calendar/alias/state conflicts and
  retry exhaustion, return their 409 directly; none calls `za_refuse` or counts as a failed
  authentication. Do not route business conflicts through the vault setup refusal helper.
- [ ] **4. Implement atomic decisions.** After grant validation/pin checks and the
  maxShares/linked-event precheck, call Task 6 conversion when needed. Re-read and decide
  again before publication; never publish before its complete valid scan. Accumulate only
  committed conversion counts for the response. One fresh batch holds verified vault hash,
  `RegistryClass::Item { object_id: ObjectType::Account.to_id(),
  item_id: u64::from(owner) }` presence assertion, applicable pin assertion and
  `Calendar::update` with its stored archive. Preserve assertion-before-set ordering.
  Import `registry::types::EnumImpl` for `ObjectType::Account.to_id()`. The store does not
  identify which assertion failed: after a loss, re-read the vault revision and registry
  Account before re-deciding. Missing Account is terminal `account deleted`; with Account
  present, a changed/missing/inactive verified vault is terminal `vault changed since
  verification`. Otherwise re-read address, served key, pin and calendar and redo all decisions,
  up to eight decision attempts. Never infer the losing key from `AssertValueFailed` alone.
  Handle Task 6's `ConversionConflict` through the same fresh vault/Account checks, even
  when `progress_changed` is true. Keep its request-wide loss counter outside the resumed
  conversion call; terminal errors take precedence over exhausted retry budgets. When guards
  hold, resume changed-progress conversion or re-decide unchanged progress as Task 6 specifies.
  Keep same-level/same-key no-ops unless a sweep or boundary completion writes something.
  Rotation is once per mutation needing revoke/re-key/sweep; use ledger keys for remaining
  grants. Do not touch old events or rewrite unaffected bundle envelopes during rotation.
  Grant/revoke/sweep prepare the intended sealed preference entries and matching ring/ledger,
  call `za_replace_calendar_entries`, then `Calendar::update` in the guarded batch. Do not
  send entry-set changes through `seal_calendar_changes`, whose invariant is for PROPPATCH.
- [ ] **5. Complete grant boundaries and invalidate after the commit.** New grant starts
  pending with zero grant id; complete using its commit result. Other mutations complete
  already-pending flags in their batch from a post-archive-read change id. Explicitly
  invalidate every changed/re-keyed/removed/swept grantee and every remaining grantee after
  rotation, even when ACL bits are unchanged. Arrange invalidation as a post-commit duty
  even if a later boundary-completion attempt returns an error.
- [ ] **6. Implement list responses and register the three routes.** Own list uses ledger
  address/key/time, current pin source and fresh active/key-changed/gone state; shared-with-me
  uses exact own-id grants, only key owners and the caller's personal view, name null for an
  unopenable grant ring. Encode href through existing DAV formatting. No production foreign
  DAV admission until Task 10. Add `SHARE_COMMIT_PAUSE` in `common::za::test_hooks`, called
  directly from `mutations.rs` immediately before commit under `test_mode`; HTTP needs no
  new common-feature forwarder. Add both auth/pin failure tests and captured-trace assertions
  for grant, revoke and conversion, following `tests/src/za/mail/traces.rs`: fixed ids/counts
  only, no fixture addresses, fingerprints, calendar names, key bytes or request content.
- [ ] **7. Run za/units/product gates.** Check post-commit cache eviction with
  `vault::session::live::session_keys`. Destroy owner via registry, inspect the orphan-ACL
  race, and remove documented gone-grantee notification fixtures before cleanup.
  In case 14, assert the destroyed owner's old grantee URI also returns 404 after a fresh
  request, in addition to disappearing from discovery.
- [ ] **8. Commit:** `Add conditional calendar sharing endpoints`.

### Task 8: Upstream-parity self-message builders

**Files:** Create `crates/groupware/src/calendar/delegation.rs` ; modify
`crates/groupware/src/calendar/mod.rs` and only the cancel
entry-point seam in `scheduling/event_cancel.rs`. Unit tests in delegation module.

**Interfaces:** Split existing cancel into its current public wrapper and
`pub(crate) fn itip_cancel_from_snapshot(ical: &ICalendar, itip: ItipSnapshots<'_>,
is_deletion: bool) -> Result<ItipMessage<ICalendar>, ItipError>`.
Add `ZaSelfMessages { messages: Vec<ItipMessage<ICalendar>>, increments: Vec<u16>,
finalize: bool }`, where `finalize=false` for the upstream cancel fallback.
Define public `ZaDelegationResolution` in `delegation.rs`: crate-private `writer_id`, `owner_id`,
`listed: AHashSet<String>` (L), `recipients: AHashSet<String>` (R) and
`accounts: AHashMap<String, ZaResolvedAccount>`, where crate-private
`ZaResolvedAccount::{Account(u32), NotFound, Failed}` stores lookup outcomes. No content-bearing
Debug. Task 9 implements the async producer; these unit tests inject a fixture resolution
inside the module. Keys are normalized exactly as Shared contracts specify.
Define public `ZaSelfOperation::{Create, Update, Delete}` to identify the chosen upstream
builder branch. The resolver and pure builders share the same private organizer-gate helper.
Expose public `za_finalize(tree: &mut ICalendar, increments: &[u16])` in `delegation.rs`,
a narrow wrapper over groupware's `pub(crate) itip_finalize`; DAV calls this wrapper.
Expose `za_self_create(new: &ICalendar, writer: &AccountInfo, owner: &AccountInfo,
resolution: &ZaDelegationResolution)`,
`za_self_update(old: &ICalendar, new: &ICalendar, writer: &AccountInfo,
owner: &AccountInfo, resolution: &ZaDelegationResolution)` and
`za_self_cancel(old: &ICalendar, writer: &AccountInfo, owner: &AccountInfo,
resolution: &ZaDelegationResolution)`, each returning
`Result<Option<ZaSelfMessages>, ItipError>`.
`None` means the delegated-organizer gate did not apply; keep `NothingToSend` distinct.
The builders use L for overrides and R for retention. Only `Ok(Some(_))` means a builder
ran successfully; preserve it even when its retained message list is empty. `Ok(None)` is
not applicable, and `NothingToSend` remains the inner builder's outcome, never a result
invented by filtering. No directory lookup or error is hidden in an `ItipError` conversion.

- [ ] **1. Add failing unit tests** `second_run_uses_owner_locality`,
  `snapshot_exception_leaves_scheduling_agent_and_payload_untouched`,
  `second_run_precedes_finalize` and `cancel_split_preserves_public_behavior`.
  Cover owner, grantee-organizer, external-organizer and shared group-address gates; create,
  update, create at an existing-event PUT with no schedule tag, cancel fallback, instance
  removal, listed aliases, writer-only groups, sub-address/custom/catch-all matches and delete.
  Assert the RSVP/PARTSTAT override affects L only. Verify that first-run `NothingToSend`
  plus second-run `Ok(Some(_))` implies at least one retained R attempt, including a sole-group
  fixture; first-run `Ok` remains built even when recipient filtering empties it.
  Essential assertions:
  ~~~rust
  assert_eq!(original_tree, tree_after_second_builder);
  assert_eq!(writer_message_sequence, owner_reference_sequence);
  assert!(client_or_none_messages.is_empty());
  assert!(attendee_side_second_run.is_none());
  assert_eq!(payload_rsvp, original_rsvp);
  ~~~
- [ ] **2. Run groupware tests and observe the missing second-run behavior.**
- [ ] **3. Implement the fork orchestration using inner builders.** Build owner-only
  snapshots before mutation; on L entries in both snapshots override
  only `force_send=Some(Request)` and `rsvp=None`. Since attendees are a hash set, rebuild
  entries safely rather than mutating through a hash key reference. Mirror upstream create/
  organizer/mismatch/cancel-fallback checks. Retain only R recipients in each
  kept message and upstream order. No copied scheduling algorithm and no public builder
  invocation on the committed tree for this second run.
- [ ] **4. Run groupware tests and product check.** The public cancel wrapper must still
  produce the same messages/errors for ordinary calls; preserve DTSTAMP normalization only.
- [ ] **5. Commit:** `Build delegated self messages through upstream scheduling`.

### Task 9: Delegated fan-out, finalization and receiver consistency

**Files:** Modify `crates/groupware/src/calendar/{fanout,drain,delegation}.rs`,
`crates/common/src/cache/principals.rs`, `crates/dav/src/calendar/{update,delete}.rs` and
`crates/http/src/api/contacts.rs` fan-out calls.
The five existing call sites are update PUT (`update.rs:359`), create (`:571`), DELETE
(`delete.rs:258`), drain (`drain.rs:1215`) and contacts redelivery (`contacts.rs:700`).
Verify the inventory with `rg`; `copy_move.rs` has no fan-out site and gains none.
Tests in `sharing.rs` plus narrow fan-out/drain unit tests.

**Interfaces:** Add the `event_account: &AccountInfo` fan-out parameter from Shared contracts.
Implement the named producers in `delegation.rs`:

- `pub async fn za_resolve_delegation(server: &Server, writer: &AccountInfo,
  owner: &AccountInfo, operation: ZaSelfOperation, old: Option<&ICalendar>,
  new: Option<&ICalendar>) -> Option<ZaDelegationResolution>` checks the shared organizer gate
  first and returns `None` without lookups if inapplicable. It uses the new tree for the create
  branch and prior tree for update/delete; upstream mismatch/error checks stay in the builders.
  When applicable it collects prior/new attendee addresses and produces L, R and the lookup
  map using `za_account_id_from_email`. Organizer locality follows upstream, not account
  resolution. Resolve each normalized address once. Capture per-address failures
  as `Failed` and trace with the existing fixed recipient-lookup reason and ids only;
  these are not negative account matches or whole-PUT errors. L remains known even if a
  lookup fails, so its retained attempts can receive `5.1`. Unlisted failed lookups retain
  the first run's ordinary attempted-message/error behavior without forced exceptions.
- In `common::cache::principals`, add private async
  `rcpt_id_from_email_inner(&self, address: &str, quiet: bool)
  -> trc::Result<Option<EmailCache>>`. Keep public `rcpt_id_from_email` as its ordinary
  `quiet=false` wrapper. The inner helper retains canonicalization and passes `quiet` to
  both `domain_inner` and `rcpt_id_from_parts_inner`. Change
  `account_id_from_email_inner`'s catch-all fallback (`principals.rs:786–790`) to call this
  helper with its incoming `quiet`; preserve the current single direct catch-all lookup,
  without recursive catch-all or additional sub-address resolution. This makes the existing
  `za_account_id_from_email` quiet for every branch while preserving ordinary lookup results
  and tracing. The R resolver and the drain's named-sender/SENT-BY verification use that path.
- Public `ZaFanOutMessage` has crate-private fields `message: ItipMessage<ICalendar>` and
  `provenance: ZaMessageProvenance`, with crate-private enum `{Upstream, DelegatedSelf}`.
  Its public `upstream(message)` constructor is available to DAV/drain/contacts. The
  `delegated_self(message)` constructor is `pub(crate)` and used only by the combinator.
- `pub fn za_combine_delegated(first: Vec<ItipMessage<ICalendar>>,
  second: Option<ZaSelfMessages>, resolution: &ZaDelegationResolution)
  -> Vec<ZaFanOutMessage>` removes R recipients from first-run messages, drops emptied
  messages, wraps survivors as `Upstream`, and appends retained second-run messages as
  `DelegatedSelf`. It is the sole producer of that provenance, called by DAV at all three
  sites after recording raw builder outcomes and doing any needed finalize. Invoke it only
  when the delegated-organizer gate applies; other paths wrap first-run messages unchanged.
  A gated `NothingToSend` second run passes `None`; an inapplicable second run does not
  invoke this combinator. The combinator does not change built flags, trees or SEQUENCEs.

Pass `Some(&resolution)` to fan-out only for that path, per its full Shared-contracts
signature. Reuse raw lookup results there, verifying writer/owner ids match the current
arguments; failures keep per-recipient `5.1`. Served-key and permission checks still run.
Add `za_mark_sent_by(message: &mut ItipMessage<ICalendar>, writer: &AccountInfo,
event_account: &AccountInfo)` and a typed sender candidate carrying named address and optional
normalized mailto SENT-BY, so the drain can return the named address for merge/token lookup.
Keep original sender verification for candidates without SENT-BY.

- [ ] **1. Add failing tests** for §11.1 sender placement/verification, case 9 and case 10's
  sole-attendee create/SUMMARY/decline/delete and both aliases. Prove:
  ~~~rust
  assert_eq!(item_changed_by, bob_id);
  assert_eq!(merged_reply_sender, alice_address);
  assert!(owner_event_pairs.contains(&bob_address));
  assert_eq!(self_items_per_message, writer_address_count);
  assert_eq!(stored_force_send_parameter, None);
  assert!(schedule_tag_after_self_only_create.is_some());
  ~~~
  Test an ordinary-provenance message addressed to the writer's own account is refused even
  when it resolves internally; only a validated second-run item passes. Test lookup failures
  and a sub-address present in both builders without duplicate items or a false refusal status.
  Compare the owner's stored per-line statuses, not only recipient items. Include a writer-only
  group (ordinary `5.3`, no redirection), an owner-resolving unlisted attendee (excluded), and
  the unlisted writer-organizer attendee-side path (no second run, original self-exclusion).
  Add `catch_all_resolution_is_quiet` using a catch-all target whose domain and local part
  are distinct trace canaries. Capture every event type, following `za/mail/traces.rs`,
  during R classification and delegated sender verification, each with cold and warm
  domain/email caches. Include an unlisted attendee whose RSVP/PARTSTAT suppresses delivery:
  classification still resolves it and must remain quiet. For the drain, use a SENT-BY
  address that resolves through the catch-all to the writer and retain the normal sender
  validation outcome. Assert neither target component nor the original address appears in
  any captured event. An ordinary-lookup control must return the same account and still
  emit its existing address-bearing cache events, proving capture is active and ordinary
  tracing is unchanged. Restore the domain configuration and clean up the fixture.
- [ ] **2. Run groupware and za tests; observe identity, self-routing and finalize failures**
  through direct builder/fan-out/drain fixtures. Run delegated DAV versions in Task 10.
- [ ] **3. Integrate both runs at PUT/create and DELETE.** Fetch owner-only account info at
  the update, create and DELETE-loop sites. Run the second run first on the pre-finalize
  trees. At the existing-event PUT site (`update.rs:259-266`), choose `za_self_update` when
  `schedule_tag.is_some()`, otherwise `za_self_create`, matching the first run's branch.
  Pass that selected `ZaSelfOperation` to `za_resolve_delegation`; only `Some(resolution)`
  enters the second-run/combinator path, while `None` keeps ordinary wrapping and fan-out.
  A new-event PUT selects Create and uses `za_self_create` before `itip_create` at `:495`;
  DELETE selects Delete and uses `za_self_cancel`. The first call then
  mutates/finalizes the tree internally. Treat either run's raw success as built for fan-out
  and schedule tag. If first returns `NothingToSend` and second built in its normal path,
  call Task 8's public `za_finalize` once, with second update increments or `[]` for create.
  Keep cancel fallback unfinalized. Mapped 412 errors
  discard both runs. Apply the existing SEQUENCE floor before both and one reconciliation
  over their combined output. Keep deletion reconciliation disabled as today. Keep
  upstream recipient-limit checks on the combined list and preserve normal error handling.
  A first-run `Ok` stays built/finalized even if the R filter removes all its recipients;
  second-run `Ok(Some(_))` stays built even if its retained list is empty. When first is
  `NothingToSend`, D4's R union guarantees a built second run retains an attempt.
  Call `za_combine_delegated` after using second-run metadata for any finalize; do not build
  tagged self messages in DAV. DELETE already collects `ZaDeleteMessage.message: Option<_>`
  into a vector at `delete.rs:261`; feed it to the same R-filter/append combinator. The owner
  alone reaches this loop through
  DAV until Task 10 opens the gate; direct fixtures exercise the delegated seam in this task.
  Inside fan-out, unzip the tagged vector into equally ordered payload and provenance
  vectors, call existing `za_reconcile_sequences` on the payload slice, then zip them back
  without filtering/reordering. That function mutates payloads in place and changes neither
  length nor order; preserve this dependency and test that tags stay attached. Keep its
  existing signature and unit callers unchanged. Tag `za_decline_all`'s later push as
  `Upstream`; deletion still skips reconciliation.
- [ ] **4. Separate fan-out identity/locality responsibilities.** Writer owns keys and pins,
  `changed_by` and permissions; owner addresses define token union and persisted current
  pairs. Preserve organizer classification under merged addresses when writer organizes as
  itself. Place/replace SENT-BY on the naming property in every component, removing it for
  own-name sending and leaving other properties alone. Allow the writer's own inbox only
  for validated retained second-run items, one item/token carrier per addressed alias or
  sub-address. On that delegated-organizer path, remove every R recipient from
  first-run `to` lists before appending the retained second-run messages; drop empty messages
  and do not route or record statuses for removed attempts. The carve-out requires the
  `DelegatedSelf` provenance and writer-account resolution, never resolution alone. Owner-local
  group addresses stay excluded. Other retained groups keep their normal delivery statuses
  rather than being redirected. Exclude recipients resolving to the event owner's account
  on this path too, recording the same `5.3` as the owner's own send. Check provenance per
  message/address attempt; cache raw
  account resolution, never self-inbox permission by address. Elsewhere retain the original
  own-inbox exclusion.
  Preserve receive/send permission, event-future and tag gates. Record self delivery status
  on the writer's attendee line. Delegated pin decisions may create first-use pins, but
  never create, consume, refresh or retire withheld entries for the owner's source event.
- [ ] **5. Implement the drain consistency branch.** Require a mailto SENT-BY URI, decode
  with `groupware::decode_mailto_address`, then normalize with
  `common::auth::vault::za_normalize_address`. Read ORGANIZER's SENT-BY from tree entry
  parameters (the snapshot Organizer has no `sent_by` field); ATTENDEE already parses it.
  Use the catch-all-safe quiet lookup defined above to establish SENT-BY resolves to
  changed_by, which is key, and named sender resolves to a different local key account. Do not read the
  owner's event, grant or lease. Use the named sender for merge and token match. Reject
  invalid delegation as history with fixed `zero-access: delegated sender not verified`,
  without generated-enum edits. Preserve upstream incoming snapshot validation.
- [ ] **6. Run units/za/product gates.** Case 9 includes delivery after revoke, cancellation
  after source deletion, own-name organizer EventNotFound replies, no permission, no-message
  writes and attendee-side delegation. Exercise container DELETE; owner shared-calendar
  DELETE invalidates grantees after commit. Capture delegated-refusal traces and assert the
  fixed reason has no address, calendar name, fingerprint, key bytes or request content.
- [ ] **7. Commit:** `Deliver and verify delegated calendar scheduling`.

### Task 10: Enable grantee DAV access and prove lifecycle parity

**Files:** Modify `crates/dav/src/common/{za,uri,propfind}.rs`, `crates/dav/src/calendar/proppatch.rs`,
`crates/dav/src/principal/propfind.rs` for discovery, `tests/src/za/{sharing,sharing_races}.rs`,
`tests/src/utils/za.rs` and `tests/src/webdav/za_variants.rs`.

**Interfaces:** Add DAV `za_calendar_gate(&self, token: &AccessToken, owner_id: u32)
-> impl Future<Output=crate::Result<()>> + Send` while retaining `za_session_keys` for
scheduling/MKCOL. Use Task 3 eligibility everywhere a foreign path is emitted.
Add test-only `normalize_sharing_copy(ical: ICalendar, uid: &str) -> ICalendar` and compare
address-presence sets for stored token pairs; do not normalize their absence away.

- [ ] **1. Add failing HTTP tests** for cases 1, 2, 11, 12 and 15: direct paths, own/foreign
  homes, assisted on/off, depth-1 root, propname, corrupt eligible items, private calendar
  omission, all view/edit methods and preserved refusals. Add all case 1 boundary races.
  Essential assertions:
  ~~~rust
  assert_eq!(old_or_pending_incremental_status, 403);
  assert!(incremental_error_xml.contains("valid-sync-token"));
  assert_eq!(pending_initial_sync_status, 207);
  assert!(!reported_hrefs.contains(&private_tombstone));
  assert!(reported_hrefs.contains(&granted_event_tombstone));
  assert_eq!(owner_name_after_bob_rename, owner_name_before);
  assert_eq!(owner_color_after_bob_edit, bob_color);
  ~~~
- [ ] **2. Run za suite and observe foreign-access refusals and missing filters.**
- [ ] **3. Enable the calendar gate and listings.** Owner arm first; otherwise require
  own session keys, no impersonation flag/permission and Task 3's exact
  `has_shared_access(owner, Collection::Calendar)` check.
  Enforce own-id eligibility before assisted append and propname response; operation
  resolution requires all ACL bits. Direct refusal is 403, listing refusal is omission.
  Wire calendar PROPPATCH to the writer's own personal bundle and shared dead properties,
  copying every other ring/personal bundle verbatim. Keep calendar write conflicts at 409.
- [ ] **4. Implement safe incremental sync.** Fetch vanished entries first, then fresh
  eligible calendar archives; derive current prefix and boundary from the same archive.
  Keep the eligible set independent of change-log-intersected display containers. Refuse
  all incremental tokens when any requested eligible boundary is pending and tokens at or
  below max(grant, rename); allow initial sync. Filter by requested calendar/home and
  eligible current prefixes before emitting/counting responses. Include paused rename,
  token above every prior change id, later-token, slug-reuse and owner-repair tests.
- [ ] **5. Activate all prepared grantee HTTP cases from Tasks 4, 7 and 9, then add and
  run case 25's differential test.** Same lifecycle twice, separate UIDs:
  Alice writes versus Bob writes as edit grantee, with Carol attending both. After each
  of the eleven spec steps drain/fetch Bob and compare normalized copies. Include the
  listed-alias fixture, distinct sub-address fixture, shared and writer-only group fixtures,
  CLIENT/NONE create-delete fixture,
  sole-attendee ForceSend fixture and
  RSVP/PARTSTAT exception run. Preserve SEQUENCE, STATUS, recurrence ids, dates/RRULE and
  every attendee parameter including SCHEDULE-STATUS. Normalize only §11.2's allowed
  fields and ORGANIZER SENT-BY for delegated messages.
  Sub-address fixture: master has Bob and Carol; O1 has Bob, unlisted `bob+x@` and Carol.
  Confirm account resolution of `bob+x@`. Run create, master SUMMARY, DTSTART, remove
  `bob+x@` from O1, re-add it, delete O1, delete event. The removal's partial CANCEL lacks
  a listed Bob line and is refused equally; after re-add, O1 deletion's CANCEL includes both
  Bob lines and is accepted. Preserve exported payloads. Compare inbox message/address counts
  as multisets, token-address presence and copies. After each surviving write, compare
  `schedule_statuses` on the owner's stored R lines in both runs; after DELETE compare absence.
  The owner-side status check kills a leftover first-run `5.3` attempt that writes no item
  and cannot be seen in Bob's REQUEST-exported copy.
  Separately compare sub-address-only delivery/rejection. A writer-only plain `team@` group
  keeps `5.3` and never redirects to Bob; with that group alone, first-run `NothingToSend`
  and second-run success still finalize once and set the tag. An unlisted owner-resolving
  attendee gets `5.3` and no owner inbox item. The exception run applies to L only; add
  unlisted sub-address, custom-rule and catch-all negative variants showing their own
  RSVP/PARTSTAT filters remain intact. Keep receiver validation unchanged.
  ~~~rust
  assert_eq!(normalized_owner_run, normalized_delegated_run);
  assert_eq!(owner_token_addresses, delegated_token_addresses);
  assert_eq!(owner_run_source_statuses, delegated_run_source_statuses);
  assert!(client_none_owner_copy.is_none());
  assert!(client_none_delegated_copy.is_none());
  assert_eq!(exception_delegated_partstat, expected_partstat);
  ~~~
  Use `DrainOn`, future events and synchronous fan-out completion; never
  `wait_for_delivery`. Drain each step within the 32-item cap; account for a deferred
  pending pass explicitly. Record accepted owner-run behaviors without changing the oracle.
- [ ] **6. Extend the key-mode ACL variant.** Keep raw ACL refusal and group-calendar
  half; pin/grant via endpoint, prove view refusal, edit success and revoked access.
  Run za then WebDAV baseline and key mode sequentially.
- [ ] **7. Commit:** `Enable shared calendar DAV access and lifecycle coverage`.

### Task 11: Leak controls and mutation qualification

**Files:** Modify `tests/src/za/{leak,sharing,sharing_races,mod}.rs` and test helpers.
Add `docs/superpowers/plans/2026-10-23-zero-access-plan14-mutations.md` as execution evidence,
not a pre-filled claim.

**Interfaces:** Extend the structural scanner with an owner/calendar/header map before event
checks; use Task 1 checked header parsers without opening rings. Add shared-form-aware
cross-record checks while preserving unshared rules and pending-item `0x02` rules.

- [ ] **1. Add failing negative controls** from §11.4: stale grant entry; ACL without ring;
  missing generation; account-wrapped shared event; unlisted event generation; shared event
  with two parents; malformed owner/grantee carrier shape; nonempty sealed fields; owner/
  grantee names and shared colour as store canaries.
  ~~~rust
  assert!(violations.iter().any(|v| v.contains(expected_violation)));
  assert!(restored_fixture_violations.is_empty());
  ~~~
- [ ] **2. Run za suite and establish that each injected bad fixture is detected.**
- [ ] **3. Implement scanner correspondence rules.** Owner ring five parts, grant three;
  exact current ACL/entry/generation correspondence and exact allowed level bits; shared
  event sole parent and listed generation; unshared event no CK envelope. Every ACL principal
  must be a current key account or a gone grantee awaiting its sweep (§11.4); the suite leaves
  no gone grantee when scanning. Do not claim
  structural inspection verifies cryptography. Keep healthy shared fixtures until the leak
  scan or create them inside its test scope, only after these new scanner rules have landed,
  then clean them before assert_is_empty.
- [ ] **4. Run every mutation listed below serially.** Apply one temporary mutation, run
  the owning suite/case, record the observed assertion, restore it, verify restoration, then
  proceed. Never commit broken mutations. A surviving mutation is unresolved coverage unless
  its observational equivalence is demonstrated and recorded.
- [ ] **5. Run the unmutated za suite and affected units after restoring all mutations.**
- [ ] **6. Commit:** `Qualify calendar sharing leak and race protections`.

Required mutation-to-case ledger (all §11.6 checks; grouped only for readability):

| Mutation(s), tested separately | Required detecting case |
|---|---|
| Invalidation before commit; no access-token invalidation; refresh_acls-only same-level re-key | 16; 17; 17 |
| No vault assertion; no pin assertion; no endpoint calendar assertion | 18; 19; 20 |
| No event-write calendar assertion; no DELETE calendar assertion | 21 and 22; 22 |
| No conversion rewrap; plaintext event skipped | 3; 3 |
| No rotation; rotate to served key instead of ledger key; newest-generation-only new grant | 6; 8 with remaining-key substitution; 7 |
| Resolver uses is_member; resolver skips required bits; listing refusal aborts request | 11; 11; 11 |
| Gate accepts keyless token; gate accepts Impersonate; assisted listing skips eligibility | 11; 11; 11 |
| No delegated sender check; no SENT-BY placement | 9; 9 |
| Token locality uses merged list; own inbox excluded; one self item for aliases | 10; 10; 10 aliases |
| No single-parent rule; MOVE preserves other names | 12; 12 |
| No owner registry assertion; revoke directory match before ledger | 14 race; 5 alias transfer |
| No vanished filter; no sync boundary bound; rename does not advance boundary | 1; 1 slug reuse; 1 rename |
| Boundary uses pre-commit id; initial sync refused while pending | 1 paused rename; 1 paused rename |
| Rewrap before parentage walk; linked write rule removed; cache omits domain id | 24 first; 24 second; 24 third |
| Self delivery requires first-run messages; no self CANCEL | 10 sole decline; 10 deletion |
| Second run uses merged addresses; runs after finalize; public builder on real tree | 25; 25 DTSTART; 25 DTSTART |
| force_send-only override; schedule tag unset when only second run built | 25 exception; 10 sole create |
| Writer gets first-run output; SEQUENCE normalized by differential helper | 25; 25 with n+2 mutation |
| Second run on attendee side | 9 delegation; record equivalent if unobservable, do not claim a kill |
| Scheduling agent overridden; reconciliation without finalize | 25 CLIENT/NONE; 25 sole ForceSend and 10 |
| Writer classification by exact address list; first-run writer recipients retained | 25 sub-address item counts/tokens; 25 owner's stored per-line SCHEDULE-STATUS |
| Self-inbox admission by account match without provenance | Task 9 negative routing unit |
| Catch-all fallback drops the quiet flag | Task 9 catch-all trace fixture, R classification and delegated sender verification |
| Exception uses R rather than L; writer-only group omitted from R | 25 unlisted/custom/catch-all negative exception; 25 group status and sole-group finalize |
| MOVE overwrite schedules | 12 pending-item/queued-task counts and active recipient copies |
| Grantee dead property overwrites owner's personal bundle | 2 and §11.1 |

### Task 12: Final acceptance, API handoff and native qualification

**Files:** Create `docs/zero-access/calendar-sharing-api.md` and
`docs/superpowers/plans/2026-10-23-zero-access-plan14-outcome.md`.
Update `docs/zero-access/manual-checklist.md`, `CLAUDE.md`,
`docs/superpowers/plans/README-dev.md` and the root `README.md` sharing capability summary.
The outcome file is written from actual execution results.

**Interfaces:** The API handoff reproduces §4's request/response fields, status bodies,
pinning/removal flows, id-based revoke, privacy boundary, delegated-delivery remedy limit and
revocation claim. Reference it for the separate Circulo api.md update; do not edit that
repository as part of this task.

- [ ] **1. Run the final automated net sequentially and record actual commands/results.**
  ~~~sh
  export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
  cargo fmt --all --check
  cargo test -p vault -p groupware -p common
  cargo test -p http@0.16.26 --features test_mode
  STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture
  STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests webdav::webdav_tests
  STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests webdav::webdav_tests
  STORE=RocksDb RUST_MIN_STACK=16777216 ZA_KEY_ACCOUNTS=1 cargo test -p tests jmap::jmap_tests
  cargo build --release -p stalwart --no-default-features --features rocks
  ~~~
  Confirm nonzero test counts and inspect new-code warnings. A known cal_itip timing failure
  may be rerun once per README-dev, recording both attempts. Do not silently retry other
  failures until green or claim unexecuted SQL/cluster behavior was tested.
  The key-mode JMAP run explicitly owns §5.7's unchanged calendar-refusal regression checks
  in `tests/src/jmap/za_variants.rs`; record that those assertions actually ran.
- [ ] **2. Run the 3,000-event first-grant/resume benchmark with disposable data.** Record
  rewrapped/sealed counts, whole-request duration, build and machine; verify Bob's final
  read and Alice's unchanged content. API handoff states the measurement's conditions.
- [ ] **3. Have a fresh read-only reviewer inspect the complete implementation branch.**
  Give base/head refs, repository rules, revision 13 spec and this plan, with clean context.
  Review combined authorization, key-domain identity, ordering, conversion/revoke fences,
  boundary completion and scheduling lifecycle. Resolve confirmed findings in bounded fix
  rounds, then rerun only affected checks plus any necessary combined gate.
- [ ] **4. Perform and record native checks from §11.5.** Apple Calendar on macOS and iOS:
  assisted discovery, view/edit, shared colour behavior, invites/SENT-BY, responses from own
  and shared copies, sole-attendee decline, display alarm, drag behavior, revoke/regrant and
  rename. Thunderbird with assisted discovery off must use its second home href. Record
  immediate UI behavior separately from post-drain/refresh state. DAVx5 is optional.
  If devices/access are unavailable, mark these gates pending with the exact missing evidence;
  automated success cannot substitute for them.
- [ ] **5. Update documentation and outcome.** Record implementations of invariants 33–40,
  automated and native evidence separately, remaining accepted inherited behaviors, cluster
  pubsub/residency limitation and pending downstream page work. Update merge hotspots,
  including the cancel-builder seam and finalize dependencies. Keep unrelated inherited
  upstream issues out of public issue reports/docs except the accepted design behaviors.
  Record the `share-*`/`race-*` fixture naming allocations in `CLAUDE.md` beside its next-free
  account note; retain other modules' allocations.
- [ ] **6. Commit:** `Document and verify calendar sharing release readiness`.
  Push, deployment, release and cross-repository work require their own authorization.

## Coverage and dependency check

| Spec requirement | Owning task(s) / acceptance |
|---|---|
| §1 scope, levels and non-goals; §9 feature matrix | 7, 10; cases 1, 2, 4, 11, 12, 15 and preserved refusals |
| §2 trust/metadata; notifications and traces | 1, 7, 9, 11, 12; scanner, empty notification names, trace assertions |
| §3.1–3.4 formats/domain/bundles | 1–3; §11.1 and cases 2, 3 |
| §3.5 conversion and rollback | 4–6; cases 3, 21, 23, 24 |
| §3.6 residency and token-pair handling | 1, 3, 4, 7, 9; cases 10, 17, 24 |
| §4 API, atomicity, alias identity, invalidation, page handoff | 7, 12; cases 4–8, 14, 16–20 |
| §5.1–5.4 authorization, views, discovery and sync | 3, 5, 10; cases 1, 2, 11, 15 |
| §5.5 every event writer and calendar properties | 2–4, 9–10; cases 2, 21, 22, 24 |
| §5.6 COPY/MOVE, linked state and rename | 5, 10; cases 1, 12, 24 |
| §5.7 JMAP unchanged | 12; existing refusal tests |
| §6 scheduling, parity and accepted limits | 8–10; cases 9, 10, 25 |
| §7 rotation, re-key and gone sweep | 7; cases 5–8, 14, 17 |
| §8 provisioning, drain, pinning, mail and destruction | 4, 7, 9–12; existing tests, cases 13–14 |
| §11.1 units; §11.2 all 25 cases | 1–10; per-task tests and whole za suite |
| §11.3 key WebDAV; §11.4 scanner; §11.5 native; §11.6 kills | 10; 11; 12; 11 |
| Invariants 33, 34, 39 | 1–3, 7, 11 |
| Invariants 35–37 | 4–7, 11 |
| Invariant 38 | 8–10, 11 |
| Invariant 40 | 3, 7, 10–11 |
| §13 implementation choices; §14 merge guidance | Decisions above, 8–9 and 12 |

The critical dependency is **all event/link writers fenced (Tasks 4–5) before conversion or
grant publication (Tasks 6–7)**. The foreign URI gate opens only in Task 10, after the
delegated send/receive integration. Units for Task 8 can be prepared independently after
the interface map is accepted, but shared scheduling call-site edits remain serialized.

Plan self-review: all 25 integration cases, §11.1 unit families, §11.4 negative controls,
§11.6 mutations and invariants 33–40 have owners above. Task 11 explicitly distinguishes
an observationally equivalent attendee-side mutation from a successful kill. Whole-branch
review and native qualification remain execution gates, with no current passing claim.

Planning evidence came from primary source inspection and two bounded read-only investigations.
Those already-running investigators inherited their parent route because the originating
model ceiling was not exposed. Per Jay's later instruction, any new planning/review subagents
use `gpt-6.1-sol`; future implementation routing is resolved separately under AGENTS.md. A fresh
`gpt-6.1-sol` plan reviewer was requested twice, but both dispatches returned
`agent thread limit reached`; no independent reviewer was created through those dispatches.
The separately supplied [plan review](2026-10-23-zero-access-14-sharing-review.md), committed
at `5fa7ebd4`, records two independent review passes. Its findings were checked against the
unchanged source and incorporated here, with D1/D2 in spec revision 12. Its subsequent recheck
is addressed by revision 13 and the appended disposition, including Jay's D3/D4 decisions.
A new `gpt-6.1-sol` high dispatch for this correction pass and a retry after Jay resumed
the turn both returned `agent thread limit reached`; no substitute agent or fresh
independent review ran. The
primary checked the source and documentation. Implementation, runtime tests and native
qualification remain unperformed.

A subsequent revision 13 review completed with two `gpt-6.1-sol` xhigh reviewers and one
`gpt-6-sol` high reviewer across calendar transactions, delegated scheduling, and interface/
fixture coverage. The primary checked its sole retained finding, R19, against source.
Task 9 now owns the quiet catch-all dependency and its trace regression fixtures, with an
additional mutation check above. This addresses the finding in the plan; the source fix
and runtime validation remain implementation work. See the review file's revision 13
independent re-review for the original finding and evidence boundary.
