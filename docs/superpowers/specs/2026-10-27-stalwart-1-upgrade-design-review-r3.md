# Review of the Stalwart 1.0.1 upgrade design and plan, revision 3

Reviewed on 2026-10-30 against `6a5ed014b` on `stalwart-1-upgrade-plan` (the checked-out
tree), fork `main` at `9a7574203`, the local upstream objects `v0.16.27`, `v1.0.0` and
`v1.0.1`, the baseline in `.superpowers/sdd/baseline-01627/`, the previous review
(`2026-10-27-stalwart-1-upgrade-design-review.md`, findings F-H1 to F-L10) and the Codex
review (`../plans/2026-10-27-stalwart-1-upgrade-codex-review-2026-10-30.md`, C1 and C2).
No build or test was run. Jay's recorded decisions are taken as given; a decision is
reported only where the text that carries it out contradicts the source or a binding spec.

Line numbers refer to the files at `6a5ed014b`: the design
`docs/superpowers/specs/2026-10-27-stalwart-1-upgrade-design.md` and the plan
`docs/superpowers/plans/2026-10-27-stalwart-1-upgrade.md`.

## Summary

| Severity | Count |
|---|---:|
| Critical | 0 |
| High | 1 |
| Medium | 6 |
| Low | 8 |

Prior findings: 21 resolved, 7 partly resolved, 0 not resolved (table in the next section).

Revision 3 carries every disposition it claims, and most of the new text is correct and
executable. What is left concentrates in four places: the dead-property decision describes
the 1.0.1 metadata store wrongly on four points (R3-H1); the `#[path = "restored_tests.rs"]`
attachment collides within a directory (R3-M1); the per-task `cargo check` order forgets
that `common` depends on `store`, whose fork hooks are Task 6 work (R3-M2); and the leak
rules name `Value` and `Presence` as subspaces, which they are not (R3-M4, an error that
originated in my revision 2 review and that revision 3 copied). One correction to my own
earlier finding: F-L10 was wrong, and the disposition is right.

## Verdicts on the prior findings

| Finding | Verdict | What is left |
|---|---|---|
| F-H1 dead properties | partly resolved | Decision adopted; its description of the store is wrong on four points (R3-H1). |
| F-H2 `ShareRevokeCleanup` | resolved | Skip for key-account owners is implementable at `stage_event`/`stage_calendar`/`stage_notification`; grantee-side writes are counters only (R3-L5 on `rebuild_shares`). |
| F-H3 R54 and the index queue | partly resolved | Queue scan and `QueueStatus` key exist; the grantee clause is not implementable and the queue has no terminal failure state (R3-M3). |
| F-H4 restored-test identifiers | partly resolved | Identifiers preserved by `mod tests;`; the single file name collides in 20 directories (R3-M1). |
| F-M1 nonexistent regression tests | resolved | Now "add", with assertions named; the cited lines exist. |
| F-M2 one compile gate | partly resolved | Per-task `cargo check` added; the order omits `store` and `types` (R3-M2). |
| F-M3 hooks in deleted files | resolved | Named in the design, Task 1 and the ownership table; counts match. |
| F-M4 JMAP metadata not a method | resolved | Refusal placed in `jmap/src/api/metadata/`; `"metadata"` is a property of Mailbox, Email, Calendar, AddressBook and FileNode (`v1.0.1:crates/jmap-proto/src/object/*.rs`). IMAP LIST's return option is missing (R3-L1). |
| F-M5 R58/R62/R63 anchors | resolved | `account_batch` has no commit point (`v1.0.1:crates/store/src/search/index.rs:299-341`), so the "subject deleted" path for R62/R63 is the one the source supports. Layering note in R3-L6. |
| F-M6 un-index reads the stub | resolved | Named with a test. |
| F-M7 stale `itip_merge_changes` step | resolved | Correct now. |
| F-M8 reshaped `EventPreferences` | resolved | The 0.16 rule is real (calendar spec line 717; plan 3 outcome line 79: "closed by refusing JMAP"). At 1.0.1 no DAV path writes event preferences: the only `preferences_mut` under `crates/dav` is the calendar-level `mkcol.rs:114`, and `copy_move.rs:716-717, 1022-1023` only retain the caller's own entries. The "stop and report" clause should not trigger. |
| F-M9 viewer-side filters | resolved | Dispositions and a `CLASS:PRIVATE` test. One more reader to list (R3-L7). |
| F-M10 new key spaces | partly resolved | Two of the five names are not subspaces, and three subspace bytes changed meaning (R3-M4). |
| F-M11 external suites | resolved | Deferred; one stale line remains in Task 9 (R3-L3). |
| F-M12 restore scope | resolved | Rule stated in the design and Task 2. |
| F-L1 `test.yml` | resolved | Restored with `tests/` in Task 1. |
| F-L2 slot split | resolved | 534 and 27. |
| F-L3 "520 of 598" | resolved | 541 of 599 at `9a7574203`, 520 silent, 21 conflicts: matches the dry merge. |
| F-L4 swap tier | partly resolved | "By default, as production does" is false: the registry default is `Disabled` (R3-M5). |
| F-L5 feature list | resolved | Moved to Task 2. |
| F-L6 SEL sub-tests | resolved | Dispositioned. |
| F-L7 directory sites | resolved | Sites named and correct; one ordering remark (R3-L2). |
| F-L8 fork-owned set | resolved | Relative to v1.0.1. |
| F-L9 early returns | resolved | Self-test and diff check. |
| F-L10 CLAUDE.md version | resolved | My finding was wrong: `CLAUDE.md` at `9a7574203` reads `-p http@0.16.27` (lines 11, 12, 24). I had read a stale copy. The disposition stands. |
| C1 async constructor counts | partly resolved | The strict rule is right in intent; "final region" is undefined and the two sync sub-tests have no body record (R3-M6). |
| C2 already-ignored tests | resolved | Status carried in the baseline; "ignored as today" disposition; self-tests. |

## New findings

### High

#### R3-H1. The dead-property decision describes the metadata store wrongly on four points

- **Where:** design "Dead properties and object metadata" (lines 150–177); plan Task 4
  (lines 351–361); Task 6 leak rule (line 526).
- **Claim:** "`Shared` and `Owner` entries are sealed under the calendar's collection key …
  `Private` and `Viewer` entries are sealed under the viewer's own key … Property names sit
  in the keys and stay visible."
- **Evidence:**
  1. Only two classes carry values. `batch.set(MetadataClass::Shared, …)` and
     `batch.set(MetadataClass::Private { viewer }, …)` are the only value writes
     (`v1.0.1:crates/common/src/storage/metadata/mod.rs`, the `Some(stored) => batch.set(…)`
     arms). `Viewer { viewer }` and `Owner { owner }` keys end at the collection with no
     document id (`v1.0.1:crates/store/src/write/key.rs:601-608`) and are container counters:
     `decrement_containers(&mut batch, MetadataClass::Owner { owner }, count)`
     (`v1.0.1:crates/common/src/storage/metadata/purge.rs:179-186`). Sealing them has no
     meaning and would break the counts.
  2. Names are inside the value, not in the key. The key is
     `account / Collection::Metadata / class / collection / document_id`
     (`key.rs:590-600`); the stored value is a container of `Entry { key, value }` pairs
     (`v1.0.1:crates/types/src/metadata/entries.rs:19-104`). Sealing a container therefore
     hides the names too, and a leak rule that looks for allowlisted names in keys would pass
     vacuously.
  3. "The calendar's collection key" exists only for calendars and events. The store serves
     eight object kinds (`v1.0.1:crates/jmap/src/api/metadata/mod.rs:94-103`: Email, Mailbox,
     SieveScript, Calendar, CalendarEvent, AddressBook, ContactCard, FileNode), and DAV
     PROPPATCH on a key account's address book or file collection still reaches the same
     write path. The text gives those no rule.
  4. "The viewer's own key" is not named. `SessionKeys` exposes `mk()`, `ewk()`, `mwk()` and
     per-calendar `collection_key(id)` (`crates/vault/src/session.rs:184-218`); a key for
     metadata of another owner's object needs a named derivation.
- **Severity:** high (the decision cannot be implemented as written; an implementer must
  reinterpret all four points). **Confidence:** high.
- **Suggested change:** Rewrite the decision as: the sealed unit is the per-document
  container of a `Shared` or `Private { viewer }` entry; `Owner`/`Viewer` counters stay
  upstream's; for calendars and events, `Shared` seals under the collection key and
  `Private { viewer }` under a named viewer key (for example an HKDF child of `ewk()` with a
  fork label, or the viewer's own calendar key when the viewer is the owner); for the other
  six object kinds, state the rule explicitly (the same as 0.16: plaintext, since those
  objects are not sealed for key accounts, or refuse PROPPATCH dead properties there); the
  leak rule for `Collection::Metadata` is "every container of a key account's calendar or
  event is sealed", not a name check.

### Medium

#### R3-M1. `#[path = "restored_tests.rs"]` resolves to one file per directory and collides

- **Where:** design "Decisions" 2 (lines 387–390); plan Task 2 (lines 230–235); ownership
  table (line 109).
- **Claim:** each inline test module moves "into a fork-owned `restored_tests.rs` next to
  its 1.0 counterpart, attached as `#[cfg(test)] #[path = "restored_tests.rs"] mod tests;`".
- **Evidence:** a `#[path]` on a `mod` item that is not inside an inline module block
  resolves relative to the directory of the file that contains it, not to a per-module
  directory (Rust reference, "The path attribute"; upstream's own slots show it:
  `v1.0.1:crates/utils/src/cache.rs:760` uses `../../../../stalwart-tests/…` from
  `crates/utils/src/`). So `src/a.rs` and `src/b.rs` both point at `src/restored_tests.rs`.
  At `v0.16.27`, 113 non-SEL files under `crates/` carry an inline `#[cfg(test)]` module and
  20 directories hold two or more of them.
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** name the file per module: `#[path = "<module>_restored_tests.rs"]
  mod tests;` or `#[path = "<module>/restored_tests.rs"] mod tests;`. The module name
  `tests` (and so the identifiers) is unaffected.

#### R3-M2. Task 3's `cargo check -p common` cannot pass before `store` and `types` compile, which the plan assigns to Tasks 6 and 2

- **Where:** design "Execution and release gates" (lines 461–467); plan ownership (lines
  122–133), Task 3 check (line 321), Task 6 files (lines 447–451).
- **Claim:** "Upstream's crate dependencies allow this order: Task 3 closes with `common` …
  Task 6 with `services` and `store`."
- **Evidence:** `common` depends on `store` and `types` (`v1.0.1:crates/common/Cargo.toml`,
  `[dependencies]`), and every later crate depends on `common`. The fork modified nine files
  in `store` and two in `types` since `v0.16.27` (`git diff --name-only v0.16.27 main --
  crates/store crates/types`: `search/{document,index,mod}.rs`, `write/{batch,log,mod}.rs`,
  `dispatch/lookup.rs`, `query/log.rs`, `za_test.rs`, `lib.rs`; `types/src/{collection,field}.rs`).
  After the merge those hooks (`with_assert_absent`, the `Store::index` guard,
  `has_account_search_terms`, `ZA_UID_LEASE_PREFIX`, `INDEX_CHUNK_PAUSE`, fields 150/151,
  vanished collection 250) do not compile against 1.0.1 until someone ports them, and the
  plan puts `store/src/{search,write}` under Task 6.
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** add to Task 2 a bullet "make `types` and `store` compile: port the
  reserved fields and collection, `ZA_UID_LEASE_PREFIX`, `za_test.rs` and
  `has_account_search_terms`; leave R58's assertion for Task 6 but keep its carrier type
  compiling", and close Task 2 with `cargo check -p types -p store -p vault`. Then Task 3's
  `-p common` is reachable.

#### R3-M3. R54's new clauses: the grantee count is not implementable and the queue has no terminal failure

- **Where:** design lines 295–301; plan Task 6 (lines 471–475).
- **Claim:** "`ShareRevokeCleanup` counts, for the owner and for the grantees it stages."
- **Evidence:** `TaskShareRevokeCleanup { object_type, account_id, document_id, status }`
  (`v1.0.1:crates/registry/src/schema/structs.rs:6120-6127`) carries only the owner; the
  grantees are computed when the task runs (`remove_grantee_references`,
  `v1.0.1:crates/services/src/task_manager/tasks/grantee_cleanup.rs:45-75`). Their
  grantee-side writes are `decrement_containers` counters in the viewer's account
  (`purge.rs:185-186`), not content, so the fence does not need them. Separately, a failing
  partition retries with exponential backoff capped at 30 minutes and never reaches a
  terminal state (`v1.0.1:crates/services/src/index_queue/manager.rs:316-359`,
  `index_queue/mod.rs:37-38`), so R54's 0.16 "failed" class has no counterpart and
  CLAUDE.md's remedy ("destroy its failed tasks with `x:Task/set`") no longer covers index
  work; the 1.0.1 remedy is the `x:IndexQueueStatus` registry object
  (`v1.0.1:crates/jmap/src/registry/mapping/index_queue.rs:45-210`, `…_get/_query/_set`).
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** count `ShareRevokeCleanup` for `account_id` only and say why the
  grantees need no count; add to Task 6 that a queue entry blocks provisioning until it
  drains, name `x:IndexQueueStatus/set` as the remedy for a stuck partition, and have Task 9
  replace CLAUDE.md's failed-task remedy accordingly.

#### R3-M4. `Value` and `Presence` are not subspaces, and three subspace bytes changed meaning

- **Where:** design "Decisions" 6 (lines 409–413); plan Task 2 (lines 258–263) and Task 6
  (lines 520–526).
- **Claim:** "Cover the new `SearchQueue`, `SearchDocument`, `Presence` and `Value`
  subspaces."
- **Evidence:** `Subspace` at `v1.0.1:crates/store/src/lib.rs:137-162` has 26 variants and
  no `Value` or `Presence`; those are variants of the following enum `Shape { Value,
  Presence, Counter }` (`lib.rs:166-170`), a key's value shape. This error came from my
  revision 2 finding M10, which misread a grep across the two enums; revision 3 copied it.
  Against the fork's 24 constants (`crates/store/src/lib.rs`), the genuinely new subspaces
  are `Immutable` (`c`), `IndexProperty` (`v`), `SearchDocument` (`m`), `SearchQueue` (`y`)
  and `System` (`u`), and three bytes were reassigned: `m` was `IN_MEMORY_VALUE`, `y` was
  `IN_MEMORY_COUNTER`, `u` was `QUOTA`. A byte-keyed scanner rule carried over from 0.16
  would silently apply the wrong rule to those three.
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** replace the two lists with `Immutable`, `IndexProperty`,
  `SearchDocument`, `SearchQueue`, `System` and `Collection::Metadata`; add a note that `m`,
  `y` and `u` changed meaning, so the scanner must key its rules on the enum, not the byte.

#### R3-M5. The swap tier does not start "by default, as production does": the default is `Disabled`

- **Where:** design "Cache snapshots" (lines 312–313); plan Task 2 interfaces (lines
  213–215) and Task 6 (lines 504–507).
- **Evidence:** `CacheSwap::{Disabled, LocalFile, Redis, BlobStore}` with
  `impl Default for CacheSwap { Disabled }`
  (`v1.0.1:crates/registry/src/schema/structs.rs:850-854`, `structs_impl.rs:9206-9210`);
  `SwapTier::build` returns `SwapTier::disabled()` for `Disabled`
  (`v1.0.1:crates/common/src/cache/swap/mod.rs:184`). Calling `SwapTier::start` with the
  default configuration starts nothing, and Task 6's `swap_roundtrip_contains_no_protected_canary`
  and `swap_restore_after_revoke_cannot_restore_access` would exercise no snapshot.
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** have the test server's registry bootstrap set `cache.swap` to
  `LocalFile` under the server's temp directory (and `BlobStore` for the filesystem-blob
  run, if the blob snapshot path is to be covered), name the setting in Task 2, and drop
  "as production does".

#### R3-M6. The strict "executed" rule needs a definition of "final region" and a sync-function case

- **Where:** design "Gates", executed sub-tests (lines 423–432); plan Task 1 (lines
  155–159) and Task 7 (lines 554–557).
- **Claim:** a sub-test counts as executed if "its async body record has a positive count;
  the body's final region has a positive count".
- **Evidence:** two sub-tests are plain functions with no separate body record
  (`tests/src/imap/body_structure.rs:22`, `tests/src/webdav/cal_itip.rs:42`: `pub fn test()`;
  151 are `pub async fn test(`). llvm-cov's export lists regions in source order; for a body
  whose tail is a branching expression, the last region in source order can belong to an
  untaken branch and read 0 after a complete run, so "final region" must be defined (the
  closing-brace region of the body, or the region with the greatest start offset of kind
  `Code` whose count equals the body's entry count) and checked against the baseline's
  region exports, which exist (`.superpowers/sdd/baseline-01627/coverage/steps/*.json`,
  2.0 GB).
- **Severity:** medium. **Confidence:** medium.
- **Suggested change:** define "final region" precisely in Task 1 and Task 7; add "for a
  sub-test that is not `async`, the function's own record is the body"; require the Task 1
  recompute to report every baseline sub-test that completed but fails the rule, and switch
  that sub-test to an explicit completion marker rather than loosening the rule.

### Low

#### R3-L1. IMAP's LIST return option reads metadata too

- **Where:** design line 175 ("The IMAP refusal is per command"); plan Task 6 (line 512).
- **Evidence:** `ReturnOption::Metadata(entries)` in LIST
  (`v1.0.1:crates/imap/src/op/list.rs:181-182`) returns metadata alongside `GETMETADATA`.
- **Severity:** low. **Confidence:** high.
- **Suggested change:** add `LIST … RETURN (METADATA …)` to the per-command list.

#### R3-L2. `za_refuse_directory_account` is placed later than at 0.16, changing the error order

- **Where:** plan Task 3 (lines 303–305).
- **Evidence:** at 0.16 the refusal follows `directory.authenticate` directly
  (`crates/common/src/auth/authentication.rs:242`); 1.0.1 inserts `assert_account_directory`
  before `build_directory_token` (`v1.0.1:…/authentication.rs:246-250`). Placing the
  refusal after `assert_account_directory` lets a directory-mismatch error precede the
  key-account refusal, against the design's "exact ordering of account and vault failures"
  (line 203).
- **Severity:** low. **Confidence:** medium.
- **Suggested change:** "after `directory.authenticate` returns and before
  `assert_account_directory`".

#### R3-L3. Task 9 still records "the external suites' differences"

- **Where:** plan Task 9 (line 717) versus the design's scope exclusion (lines 88–90).
- **Severity:** low. **Confidence:** high.
- **Suggested change:** delete the bullet or replace it with "the python-caldav result".

#### R3-L4. "Remove the key-mode JMAP ACL variant's `wait_for_tasks`" names one of four waits

- **Where:** design line 292; plan Task 6 (line 492).
- **Evidence:** `tests/src/jmap/za_variants.rs` has four `wait_for_tasks` calls (lines 296,
  762, 1301, 1419); PR #66's is the one under the comment "An unindex queued before the
  import's index write can orphan the document" (`d375c5dc1`). `TestServer::cleanup`
  (`tests/src/utils/server.rs:501-504`) does not wait for tasks itself, so the later waits
  must stay for `assert_is_empty` to see a drained queue.
- **Severity:** low. **Confidence:** high.
- **Suggested change:** identify the wait by its comment and say the others stay.

#### R3-L5. Say explicitly whether `rebuild_shares` runs for key accounts

- **Where:** design lines 229–231 ("gets the same treatment where it would write a key
  account's calendar data"); plan Task 6 (lines 500–501).
- **Evidence:** `rebuild_shares` writes only `acl_grant`/`acl_revoke` index keys derived
  from the account's object ACLs (`v1.0.1:crates/services/src/task_manager/tasks/rebuild_shares.rs:95-121`),
  never event or calendar records. "Where it would write calendar data" therefore matches
  nothing, and an implementer may skip the task wholesale, letting the grantee index drift
  from the ACLs the fork maintains.
- **Severity:** low. **Confidence:** medium.
- **Suggested change:** "`rebuild_shares` runs unchanged for key accounts: it rebuilds the
  ACL index from the calendar ACLs the fork's revoke maintains; `cleanup_cannot_restore_revoked_ring`
  covers it."

#### R3-L6. State the layering of R58's assertion: attached in `services`, honoured in `store`

- **Where:** design lines 302–303; plan Task 6 (lines 476–478).
- **Evidence:** `store` cannot call `Server::za_index_fence` (`common` depends on `store`,
  not the reverse). At 0.16 the fork solved this with `IndexDocument::with_assert_absent` in
  `store` (`crates/store/src/search/document.rs`) set by `common` (`auth/vault.rs`). The same
  split applies to `account_batch`.
- **Severity:** low. **Confidence:** high.
- **Suggested change:** "the worker (`index_queue/index.rs`) attaches the assertion through
  a store-side field on the documents it passes to `index_account_documents`; `account_batch`
  places it in the batch".

#### R3-L7. One more viewer-side reader: the default-alert resolver

- **Where:** design "New sharing filters" (lines 244–252); plan Task 4 (lines 369–376).
- **Evidence:** DAV GET and COPY run `DefaultAlertsResolver`
  (`v1.0.1:crates/dav/src/calendar/get.rs:180`, `copy_move.rs:682, 726, 987, 1032`), which
  reads `CalendarPreferences.default_alerts` through `CalendarSettings`
  (`v1.0.1:crates/groupware/src/calendar/alerts.rs:169-185, 301-305`). The struct is
  unchanged from 0.16.27, but the resolver is new and injects per-user alerts into served
  events, so it belongs on the list of readers that run only on the unsealed view.
- **Severity:** low. **Confidence:** medium.
- **Suggested change:** add `alerts::DefaultAlertsResolver` to the filter list with the
  same disposition.

#### R3-L8. Minor wording: two of the 27 support slots are feature-conditioned

- **Where:** design line 331–332 ("27 are support modules that are always compiled once the
  flag is set").
- **Evidence:** 25 are `cfg(stalwart_tests)`; 2 add a store-feature condition.
- **Severity:** low. **Confidence:** high.
- **Suggested change:** "27 are support modules compiled once the flag is set (two of them
  only with a store feature)".

## Questions for Jay

1. **Dead properties outside calendars (R3-H1, point 3).** For a key account's address
   books, contacts, files, mailboxes and Sieve scripts, should DAV dead properties stay
   plaintext in `Collection::Metadata` (parity with 0.16, where those objects were not
   sealed), or be refused? The calendar spec covers only calendars and events.
2. **The viewer key (R3-H1, point 4).** Which key seals a grantee's `Private { viewer }`
   entries on an owner's event: a fork-labelled child of the viewer's `ewk()`, or the
   viewer's own calendar collection key? The first keeps the entries readable only by the
   viewer; the second ties them to one of the viewer's calendars.
3. **Swap tier in tests (R3-M5).** With the default `Disabled`, do you want the harness to
   configure `LocalFile` for every suite (closest to a deployment that enables it) or only
   in the two swap tests?
4. **A stuck index partition (R3-M3).** A partition that keeps failing blocks provisioning
   of its account indefinitely. Is the `x:IndexQueueStatus/set` remedy acceptable as the
   documented operator action, replacing the failed-task remedy in CLAUDE.md?

## Verified in revision 3

- The four hook-carrying files and their counts (15, 7, 1, 1) and replacements.
- 849/670/139 modified/added/deleted under `crates/`; 541 of 599 under `tests/` at
  `9a7574203`, 520 silent and 21 conflicts; `test.yml` absent at 1.0.1.
- `HttpAuthenticated { in_flight, access_token, epoch }`, `GrantRevocations` and the cited
  line ranges; `assert_account_directory` at `authentication.rs:467`; `synchronize_account`
  at `cache/directory.rs:30`; `merge_directory_secret` is fork code at `:27`.
- `SplitUpdate::full` calls `refresh_from_content`; `itip_merge_changes` returns
  `Result<(), ItipError>` at `inbound.rs:705-709`; the drain already passes the tree.
- `stage_event`'s `update_full` and its queueing from `storage/index/build.rs:483`.
- `SearchIndexClass::{Queue, QueueIndex, QueueStatus}`; the worker reads the row first
  (`index.rs:343-350`); `account_batch` has no commit point; `delete_email_metadata` reads
  `raw_headers()` (`index.rs:834-855`).
- `"metadata"` is a JMAP property on five object types; `MethodObject` unchanged.
- The calendar spec's line 717 and the plan 3 outcome (line 79) support the per-user data
  rule; no DAV write of event preferences exists at 1.0.1.
- The six `#[ignore]`d tests; the SEL-only sub-tests of `system_tests`; the region-level
  exports under `coverage/steps/`.
- `CLAUDE.md` at `9a7574203` already names `http@0.16.27`.
