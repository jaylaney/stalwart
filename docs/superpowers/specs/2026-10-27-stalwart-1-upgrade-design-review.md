# Review of the Stalwart 1.0.1 upgrade design and plan (revision 2)

Reviewed on 2026-10-30, cold, against the repository at `e66044454` (branch
`stalwart-1-upgrade-plan`), fork `main` at `9a7574203`, the local upstream objects
`v0.16.27` (`aa47d8ef7`), `v1.0.0` (`47df4c0ba`) and `v1.0.1` (`23b69755c`), the baseline in
`.superpowers/sdd/baseline-01627/`, and the fork's PRs #62 and #66 and issue #67 (read-only
`gh`). No build or test was run; every claim below comes from reading source or git objects.

Documents under review:

- Design: `docs/superpowers/specs/2026-10-27-stalwart-1-upgrade-design.md` (revision 2).
- Plan: `docs/superpowers/plans/2026-10-27-stalwart-1-upgrade.md` (revision 2).

Line numbers refer to those files as committed at `e66044454`. Upstream references are
written `<ref>:<path>:<line>`.

## Summary

| Severity | Count |
|---|---:|
| Critical | 0 |
| High | 4 |
| Medium | 12 |
| Low | 10 |

The lineage, SHAs, merge statistics, private-test slot counts, baseline numbers, coverage
figures, schema stamping, SEL-only file list and nearly every 1.0.1 interface the plan names
check out (see "Verified claims" at the end). The findings concentrate on four things:
(1) three 1.0.1 surfaces that touch key-account calendar data and that neither document
names (dead properties moved into a separate per-viewer metadata store; the
`ShareRevokeCleanup` task's full rewrite of events; the reshaped per-user
`EventPreferences`); (2) the provisioning fence R54 and the R58/R62/R63 index guards, whose
anchors (the `Task` enum, `Store::index`'s chunk loop) no longer exist; (3) the test-list gate,
which as written would flag every restored upstream unit test as missing; and (4) a handful
of plan steps that a subagent could not execute as written (named tests that do not exist,
a stale `itip_merge_changes` instruction, hooks in files upstream deleted).

## Findings

Ordered by severity. Confidence is my confidence that the finding is real, not that the
suggested change is the right one.

### High

#### H1. Dead properties now live in a separate, unencrypted, per-viewer metadata store that the design treats as three different surfaces

- **Where:** design, "Fresh persistence baseline" (lines 129–134) and "Indexing, metadata
  and cache snapshots" (lines 215–219); plan Task 4 (lines 277–280) and Task 6 (lines 365–367).
- **Claim or gap:** The design says `types::DeadProperty` was removed and the sealed extras
  must be adapted "to the new metadata representation", and separately that the new IMAP
  METADATA and JMAP object-metadata operations "initially refuse key-account access wherever
  they add storage outside the existing encrypted carriers" while "existing DAV property
  operations continue through their ported encrypted mapping". In 1.0.1 these are one
  mechanism, not three: `CalendarEvent` no longer has a `dead_properties` field
  (`v1.0.1:crates/groupware/src/calendar/mod.rs:179-191`; the fork's struct carries
  `dead_properties: DeadProperty` at `crates/groupware/src/calendar/mod.rs:138`), and DAV
  PROPPATCH dead properties are written as documents of the new `Collection::Metadata = 14`
  (`v1.0.1:crates/types/src/collection.rs:80`) with per-viewer classes
  `MetadataClass::{Shared, Private{viewer}, Viewer{viewer}, Owner{owner}}`
  (`v1.0.1:crates/store/src/write/metadata.rs:17-22`), through
  `v1.0.1:crates/dav/src/common/dead/patch.rs` and `common/src/storage/metadata/*`. IMAP
  METADATA and JMAP object metadata write the same store. The fork's sealed-extras carrier
  (38 `dead_properties` references under `crates/groupware/src/calendar/seal/` and
  `crates/dav/src/common/za.rs`) has no home in the 1.0.1 event record, and "continue through
  their ported encrypted mapping" has no carrier to map onto.
- **Severity:** high. **Confidence:** high.
- **Suggested change:** Make an explicit design decision and a Task 4 bullet with a test.
  The options I see: (a) store key-account dead properties sealed inside `Collection::Metadata`
  documents under the collection key, with a fork payload version byte, and extend the leak
  scanner to that collection; (b) keep them inside the sealed content extras for key accounts
  and bypass upstream's metadata store (then IMAP METADATA and JMAP metadata refusals follow
  for free, but PROPFIND of dead properties needs a fork read path); (c) refuse dead-property
  PROPPATCH for key accounts (breaks clients that store colour/order as dead properties).
  Whichever is chosen, say which `MetadataClass` values a key account may hold in the clear,
  if any, and add the collection to Task 6's leak enumeration.

#### H2. The new `ShareRevokeCleanup` task rewrites events in full, in the background, without keys

- **Where:** design lines 166–167 ("Upstream's new grantee-cleanup and share-rebuild tasks
  must not recreate a grant, keep unauthorized content, or bypass ring rotation"); plan Task 4
  (lines 290–292) and Task 6 (lines 356–357).
- **Claim or gap:** Both documents name the tasks but describe them only in grant/ring terms.
  What the task actually does: `stage_event` strips the lost grantee's preferences and calls
  `CalendarEvent::update_full` (`v1.0.1:crates/services/src/task_manager/tasks/grantee_cleanup/stage.rs:124-207`,
  the `.preferences.retain(...)` and `update_full(` at its lines 69–71), which goes through
  `SplitUpdate::full` → `meta.refresh_from_content(&content, ctx)`
  (`v1.0.1:crates/common/src/storage/index/split.rs:100-113`,
  `v1.0.1:crates/groupware/src/calendar/index.rs:208`): the content archive is re-serialized,
  the metadata (uid, start, duration, size, flags) is recomputed from the content and the
  ETag changes. It also stages mailboxes, calendars, notifications, address books and file
  nodes. The task is queued from the generic index builder whenever an ACL grant disappears
  (`v1.0.1:crates/common/src/storage/index/build.rs:483`), so the fork's own revocation path
  queues it too if it edits ACLs. On a sealed event this is a background writer with no
  session keys recomputing metadata from sealed content, and a plaintext ETag churn on every
  event of the owner the grantee had preferences on. `rebuild_shares` runs from the periodic
  `AccountMaintenance` task (`v1.0.1:crates/services/src/task_manager/tasks/maintenance.rs:435`).
- **Severity:** high. **Confidence:** high.
- **Suggested change:** Add to Task 6: for key-account owners, `stage_event`,
  `stage_calendar` and `stage_notification` must either skip the account (the fork's ring
  handles grantee removal) or use a metadata-only write that does not re-derive metadata
  from content; name the choice in the design's calendar section. Add a test
  (`revoke_cleanup_does_not_rewrite_sealed_event`: content archive bytes and ETag unchanged,
  no plaintext in the record) and make it part of the mutation sample.

#### H3. Ruling R54's `za_has_tasks` and the fence ordering are written against a task model 1.0.1 removed

- **Where:** design, "The index/un-index race" (lines 190–205); plan Task 2 (lines 206–208)
  and Task 6 (lines 344–355).
- **Claim or gap:** CLAUDE.md's provisioning fence (R46 → R54 → R52 → R51 → R60 → R62) relies
  on `Server::za_has_tasks` being an exhaustive match over `Task`
  (`crates/common/src/auth/vault.rs:737-749`, matching `Task::IndexDocument`,
  `UnindexDocument`, `MergeThreads`, `CalendarItipMessage`). In 1.0.1 the `Task` enum has no
  `IndexDocument`, `UnindexDocument` or `IndexTrace` and gains `ShareRevokeCleanup`,
  `DmarcReportWindow` and `TlsReportWindow` (`v1.0.1:crates/registry/src/schema/structs.rs:5893`
  versus `v0.16.27:…:5518`). The index queue is now a key space of its own:
  `SearchIndexClass::Queue { index, id_prefix: account_id, id_suffix, created_at }` with
  value `1` (index) or `0` (un-index), a `QueueIndex { index, partition }` marker and control
  and status keys (`v1.0.1:crates/store/src/write/batch.rs:977-1038`,
  `v1.0.1:crates/store/src/write/key.rs:1143-1147`), in subspace `SearchQueue = b'y'`.
  `destroy_account` clears it (`v1.0.1:crates/services/src/task_manager/tasks/destroy_account.rs:245`).
  The plan's Task 6 says "pause later chunks" and "map old document assertions onto 1.0
  postings/queue commits" but never names the keys R54 must scan, and Task 2's
  `wait_for_tasks` bullet names the manager but not the keys either (the fork's
  `wait_for_tasks` scans only `TaskQueueClass::Task`, `tests/src/utils/storage.rs:202-240`).
  The build will stop at the exhaustive match, which is the intended safety net, but the
  plan should say what replaces it rather than leave it to the implementer.

  On the design's open question: the source shows that a document's index and un-index
  entries share the partition `(index, account_id)` and are ordered by snowflake
  `created_at`; the worker reads the metadata row first and skips an index entry whose row
  is gone (`v1.0.1:crates/services/src/index_queue/index.rs:308-392`, `nothing_to_index` at
  343–350), and a later delete queues an un-index entry in the same partition. So the
  0.16 race is gone by construction, with one remaining window (row read → document write)
  that the next pass of the same partition repairs. A queue-entry scan for the account is
  therefore the right R54 replacement.
- **Severity:** high. **Confidence:** high.
- **Suggested change:** Write the R54 retarget into Task 6 explicitly: scan
  `SearchIndexClass::Queue` entries with `id_prefix == account_id` for every `SearchIndex`
  plus the partition's status key; classify `ShareRevokeCleanup` (counts for the owner and
  the grantees it stages), `DmarcReportWindow` and `TlsReportWindow` (excluded). Record the
  "ordered" determination with the evidence above and keep the ACL-variant wait only if the
  queue scan says the account is still busy. Extend Task 2's `wait_for_tasks` with the same
  keys.

#### H4. The test-list gate, as written, flags every restored upstream unit test as missing

- **Where:** design "Decisions" 2 (lines 272–276) and "Gates" (lines 295–297); plan Task 2
  (lines 189–198) and Task 7 (lines 382–385, 402–404).
- **Claim or gap:** Moving each inline `#[cfg(test)] mod tests { … }` into a file attached by
  `#[cfg(test)] mod restored_tests;` changes every libtest identifier from
  `path::tests::name` to `path::restored_tests::name`. The inventory tool "exits nonzero
  when a required identifier is missing", so the 283 restored upstream unit tests
  (466 at `v0.16.27` under `crates/`, less the 183 in `scim` and `scim-proto`; counts
  confirmed by `git grep -c '#[test]|#[tokio::test'`) would each need a rename disposition,
  which defeats the gate's purpose.
- **Severity:** high (gate unworkable as written). **Confidence:** high.
- **Suggested change:** Keep the identifiers: attach the file as
  `#[cfg(test)] #[path = "restored_tests.rs"] mod tests;` (a child module sees the parent's
  private items exactly as the inline module did), or have the tool normalize
  `restored_tests::` to `tests::`. State the rule in Task 2 and in the Task 7 tool
  interface.

### Medium

#### M1. Task 3 asks to "extend existing regression cases" that do not exist

- **Where:** plan Task 3, lines 231–234.
- **Claim or gap:** `scoped_cache_hit_preserves_scope`, `revoked_grant_cannot_repopulate_cache`
  and `expired_bearer_cache_entry_is_refused` appear nowhere in `crates/` or `tests/`
  (`grep -rn --include='*.rs' 'scoped_cache|revoked_grant|expired_bearer'` returns nothing).
  A subagent would either hunt for them or write new tests under those names and report
  "extended".
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** Say "add" and state what each asserts against 1.0.1's
  `HttpAuthenticated { in_flight, access_token, epoch }` and `GrantRevocations`
  (`v1.0.1:crates/http/src/auth/authenticate.rs:20-24`,
  `v1.0.1:crates/common/src/cache/http_auth.rs:35-38, 101-137`).

#### M2. One compile gate at Task 7 for four implementation waves

- **Where:** design "Execution and release gates" (lines 321–323); plan "File ownership"
  (lines 108–111) and Task 7.
- **Claim or gap:** Upstream modified 849 files, added 670 and deleted 139 under `crates/`
  (`git diff --name-status -M v0.16.27 v1.0.1 -- crates`). Tasks 3–6 edit on top of that
  with "the first honest integrated compile gate" at Task 7. Subagents executing Tasks 3–6
  cannot check their own work, and CLAUDE.md's rule that Opus/Sonnet subagents self-verify
  has nothing to verify against. The design accepts this ("not necessarily buildable").
- **Severity:** medium. **Confidence:** medium.
- **Suggested change:** Add crate-level checkpoints without permissive shims: `cargo check
  -p common` closes Task 3; `-p groupware -p dav` closes Task 4; `-p email -p imap -p jmap
  -p smtp -p pop3` closes Task 5; `-p services -p store` closes Task 6. The dependency order
  of the crates allows it, and Task 2 already has to make the workspace resolve.

#### M3. Hooks whose host file upstream deleted are not named anywhere

- **Where:** plan "File ownership and dependencies" (lines 92–101) and Task 1 (lines 145–147).
- **Claim or gap:** Four upstream files that carry fork hooks are modify/delete conflicts
  in the dry merge: `crates/dav/src/common/propfind.rs` (15 `za_` call sites) became
  `propfind/{data,dead,get,item,load,mod}.rs`; `crates/imap/src/op/fetch.rs` (7) became
  `fetch/{mod,section,source,structure}.rs`; `crates/imap/src/core/message.rs` (1) became
  `core/view.rs`; `crates/spam-filter/src/analysis/llm.rs` (1) became
  `common/src/enterprise/ai/*` (SEL). The ownership table names `email/src/message/{za,
  metadata,ingest}` but none of these. Task 1 says "distinguish textual conflict from a hook
  whose target moved" generically.
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** Name the four files and their replacement directories in the
  ownership table (DAV → Task 4, IMAP → Task 5, llm → Task 5 as a disposition) so the
  inventory in Task 1 has a known-answer check.

#### M4. JMAP object metadata is not a new method and cannot be refused by the dispatcher gate

- **Where:** design lines 215–219; plan Task 6, lines 365–367.
- **Claim or gap:** `MethodObject` is identical at `v0.16.27` and `v1.0.1`
  (`v1.0.1:crates/jmap-proto/src/request/method.rs:24`). Object metadata is a property
  layer inside existing `*/get`, `*/set`, `*/query` and `*/copy`
  (`v1.0.1:crates/jmap/src/api/metadata/{apply,write,patch,get,preload,copy,query}.rs`), and
  the fork's gate in `crates/jmap/src/api/request.rs` is per method. The IMAP side is a
  set of new commands (`GETMETADATA`, `SETMETADATA`, `v1.0.1:crates/imap/src/op/metadata/`)
  and can be gated per command. In addition, object writers now record "presence" flags of
  which metadata kinds exist (`v1.0.1:crates/email/src/presence.rs:144`,
  `v1.0.1:crates/common/src/storage/index/presence.rs`).
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** Place the key-account refusal in `metadata::write`/`apply` and
  `metadata::get`/`preload` (and `copy`), not in the dispatcher; say so in Task 6 and add a
  test that `Mailbox/set` with metadata properties on a key account is refused while the
  same call without them succeeds.

#### M5. The R58/R62/R63 anchors have no counterpart in 1.0.1's index writer

- **Where:** design lines 185–188; plan Task 6 interface (lines 337–339).
- **Claim or gap:** CLAUDE.md anchors R58 to `IndexDocument::with_assert_absent` placed in
  the first commit chunk of `Store::index`, and R62/R63 to each later chunk asserting its
  `Document` key. In 1.0.1 `Store::index` is gone; documents are written by
  `index_account_documents` → `account_batch` → one `write_batch` per account batch
  (`v1.0.1:crates/store/src/search/index.rs:21-31, 299`), and `IndexDocument`
  (`v1.0.1:crates/store/src/search/mod.rs:175`) has no assertion builder. The plan speaks of
  "search commit segments" without naming the new write path, so an implementer could
  attach the fence to the queue entry (wrong: the entry is written by the producer) instead
  of the batch the worker commits.
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** In Task 6's interface sheet name `account_batch` as the single
  place for the vault-absence assertion, and decide whether R62/R63 still have a subject:
  if one batch per account per pass is the rule, the between-chunks un-index case collapses
  and the two rulings take the "subject deleted" disposition with this evidence.

#### M6. The un-index path of the 1.0.1 worker parses the stored row's raw headers

- **Where:** plan Task 6, lines 344–346 ("Port the old index-task guards to every new
  worker/rebuild path").
- **Claim or gap:** `delete_email_metadata` unarchives the metadata row and reads
  `row.raw_headers()` to resolve the thread subject
  (`v1.0.1:crates/services/src/index_queue/index.rs:834-855`). For a key account's stub the
  head of `raw_headers` is the fork's format byte (CLAUDE.md: "the one discriminator"), so
  the un-index side must recognize stubs as the index side does; the plan's wording reads as
  index-only.
- **Severity:** medium. **Confidence:** medium.
- **Suggested change:** Name `delete_email_metadata` in Task 6 and add a destroyed-stub
  un-index test to `index_task.rs`.

#### M7. The `itip_merge_changes` instruction is stale and would send an implementer the wrong way

- **Where:** plan Task 4, lines 284–285 ("Update the `itip_merge_changes` caller in
  `calendar/drain.rs` to carry the incoming tree").
- **Claim or gap:** The fork's drain already passes the incoming tree
  (`crates/groupware/src/calendar/drain.rs:1199`, `itip_merge_changes(&mut event.data.event,
  &tree, changes)`), and the 0.16.27 signature already takes it
  (`v0.16.27:crates/groupware/src/scheduling/inbound.rs:545`). The 1.0.1 change is the new
  `Result<(), ItipError>` return (`v1.0.1:…/inbound.rs:705-709`), which the drain's
  hand-mirrored `merge` (CLAUDE.md: it mirrors `commit_itip_merge` by hand) must propagate.
  The sentence is unchanged from revision 1 (`.superpowers/sdd/1.0-docs-r1/…upgrade.md:215`).
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** Replace with: "propagate `itip_merge_changes`'s new `ItipError`
  result through the drain's `merge` and `import`; re-read both against 1.0.1's
  `itip_ingest` and `commit_itip_merge`".

#### M8. Per-user event data was reshaped and now sits beside the sealed tree; its carrier is undecided

- **Where:** design C6 (line 100) and "Calendar and sharing writes" (lines 158–161); plan
  Task 4 interface (lines 269–272).
- **Claim or gap:** At 0.16.27 `EventPreferences { account_id, flags, properties, alerts }`
  (`v0.16.27:crates/groupware/src/calendar/mod.rs:197-201`). At 1.0.1 it is
  `{ account_id, updated, instances: Vec<EventUserData { recurrence_key, flags, color,
  keywords: Vec<String>, alerts: Vec<ICalendarComponent> }> }`
  (`v1.0.1:crates/groupware/src/calendar/mod.rs:380-395`) and lives in
  `CalendarEventContent.preferences` next to `data` (`…:196-199`). The background alarm code
  reads `ArchivedEventPreferences` (`v1.0.1:crates/groupware/src/calendar/alarm.rs:325-335`),
  DAV copy/move retains per-account preferences (59 references under `crates/dav`), and the
  fork's VALARM allowlist exposes only `TRIGGER`, `ACTION`, `REPEAT`, `DURATION`
  (`crates/groupware/src/calendar/seal/policy.rs:49`), so per-user alert `DESCRIPTION`,
  `SUMMARY` and `keywords` are hidden-class content. Neither document says whether
  `preferences` is inside the sealed carrier, nor under which key a grantee's preferences on
  an owner's event are sealed (the grantee's own key makes them unreadable to the alarm
  task; the collection key makes them readable to every grantee).
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** Decide it in the design's calendar section and add a Task 4 round
  trip plus a leak canary for per-user keywords and alert text, in both the owner and the
  grantee position.

#### M9. 1.0.1's new content filters on shared calendars are not in scope

- **Where:** design "Calendar and sharing writes" (lines 156–169); plan Task 4.
- **Claim or gap:** 1.0.1 adds `calendar/privacy.rs` (`EventPrivacy::check_access(viewer)`,
  CLASS-based filtering for non-owners), `hidden.rs` (`HiddenParticipantsMerge`),
  `participants.rs`, `overlap.rs`, `notification.rs` (`NotificationViewer`,
  `visible_notifications`) and `dav/calendar/{view,filter}.rs`. In the fork's sharing model a
  grantee holds the collection key, so these filters must run on the unsealed view and must
  not be evaluated by code without keys; also `EventPrivacy::from_flags` derives privacy from
  metadata flags, which the fork's sanitized metadata must carry correctly.
- **Severity:** medium. **Confidence:** medium.
- **Suggested change:** Add a Task 4 bullet listing these modules with a disposition each
  (runs on unsealed view; metadata flag kept; refused), and a grantee-side test for
  CLASS:PRIVATE on a shared sealed calendar.

#### M10. The leak scanner and cleanup need 1.0.1's new key spaces named

- **Where:** plan Task 2 (lines 209–213) and Task 6 (lines 368–370).
- **Claim or gap:** The `SUBSPACE_*` constants the harness enumerates
  (`tests/src/utils/cleanup.rs:265-287`, 24 at the fork) do not exist at 1.0.1 (0 `pub const
  SUBSPACE_`); the closed `Subspace` enum (`v1.0.1:crates/store/src/lib.rs:137`) adds
  `SearchQueue = b'y'`, `SearchDocument = b'm'`, `Value` and `Presence`, and `Collection`
  adds `Metadata = 14` (with `None` moving to 15). Task 2 covers the cleanup; Task 6's leak
  bullet is generic ("new store/search/metadata/cache formats").
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** Name `SearchQueue`, `Presence`, `Value` and `Collection::Metadata`
  in Task 6's leak bullet, each with a rule (what a key account may hold there in the
  clear) and a planted-violation control.

#### M11. The external protocol suites are unscoped, unpinned and costly

- **Where:** design "Gates" (lines 310–313); plan Task 8 (lines 491–498).
- **Claim or gap:** Dovecot's `imaptest` must be built from Dovecot source (no package in
  `ubuntu-latest`); the 112 scripts exist at `tests/resources/imap-test/` (112 files, 111
  top-level entries) but the fork has no runner (the only `imaptest` reference is a mailbox
  name, `tests/src/imap/mailbox.rs:388`). Fastmail's JMAP-TestSuite is Perl and needs a
  server adapter. "python caldav-server-tester" is ambiguous (python-caldav's checker, which
  `tests/caldav_client/runner.py` already exercises, versus Apple's CalDAVTester). The job
  also builds two product binaries (candidate and 0.16.27). No source revision is named, and
  the CI time is unbudgeted.
- **Severity:** medium. **Confidence:** high.
- **Suggested change:** Either name each suite's repository, revision, build steps and
  expected runtime, or move the three suites to a later, separate plan and keep the
  0.16.27-diff idea for python-caldav only, which already runs.

#### M12. "Keep the whole 0.16.27 suite" understates what restoring upstream's unit tests costs

- **Where:** design "Decisions" 1–2 (lines 260–277); plan Task 2 (lines 189–198).
- **Claim or gap:** 283 upstream unit tests target internals, and 849 of the files they
  sit in changed. Many will take the "internals replaced → equivalent test of the new code"
  disposition, where the porter writes and certifies the equivalence without upstream's own
  test as a check; the expectation ledger records it but cannot validate it. The coverage
  gate guards only fork-owned code (96.4%), so a weak equivalent is invisible to every gate.
  No budget or stop rule is given.
- **Severity:** medium. **Confidence:** medium.
- **Suggested change:** Decide a rule before Task 2 starts (for example: restore every
  test whose subject is unchanged or renamed; for a replaced internal, write the equivalent
  only when the module is on the fork's hotspot list or the test exercised a public
  behaviour; disposition the rest as "internals replaced, no equivalent" with the diff
  hunk). See Q3.

### Low

#### L1. `.github/workflows/test.yml` is a modify/delete conflict that Task 1 does not resolve

- **Where:** plan Task 1 (lines 154–160) and Task 8 (lines 478–486).
- **Evidence:** upstream deleted the file (`git ls-tree v1.0.1 .github/workflows/` has no
  `test.yml`); the dry merge lists it under modify/delete. Task 1 restores only `tests/`.
- **Severity:** low. **Confidence:** high.
- **Suggested change:** Restore the fork side in Task 1 as a placeholder
  (`git checkout HEAD -- .github/workflows/test.yml`) so the merge can be committed; Task 8
  rewrites it.

#### L2. Private-test slot classification is slightly off

- **Where:** design lines 232–234 ("530 … compile only when that crate's own tests are built;
  about 27 are support modules").
- **Evidence:** 532 `cfg(all(test, stalwart_tests))` plus 2 test-and-feature guards = 534;
  25 `cfg(stalwart_tests)` plus 2 feature-only guards = 27 (`git grep -n -B3 '#[path = ".*stalwart-tests' v1.0.1 -- crates`).
- **Severity:** low. **Confidence:** high.
- **Suggested change:** 534 and 27.

#### L3. "520 of the 598 files" is measured at `5b914ddb5`, not at the stated starting fork

- **Where:** design line 255; plan line 157.
- **Evidence:** `tests/` has 598 files at `5b914ddb5` and 599 at `9a7574203` (PR #66 added
  `tests/src/za/external_directory.rs`); 541 files under `tests/` are deleted upstream, 21
  of them as modify/delete conflicts, 520 silently.
- **Severity:** low. **Confidence:** high.
- **Suggested change:** State the SHA, or say "520 of 599 at `9a7574203`", and mention the
  21 conflicting test files so the restore step is understood to cover both.

#### L4. `Caches::parse` is async and returns a swap receiver the harness must start or deliberately drop

- **Where:** plan Task 2 interface (lines 175–176).
- **Evidence:** `let (cache, swap_rx) = Caches::parse(&mut bootstrap, &core.storage).await;`
  and `SwapTier::start(&inner, swap_rx)` (`v1.0.1:crates/common/src/manager/boot.rs:158, 225`);
  the harness calls `Caches::parse(&mut bp).await` at `tests/src/utils/server.rs:362, 592`.
- **Severity:** low. **Confidence:** high.
- **Suggested change:** Say whether the test server starts the swap tier (Task 6's swap
  snapshot tests need a running writer) and name the two call sites.

#### L5. The unit-tests feature list must be re-derived in Task 2, not Task 8

- **Where:** plan Task 7 matrix (line 432) and Task 8 (line 482).
- **Evidence:** `UNIT_TEST_FEATURES` (`.github/workflows/test.yml:158-178`) names
  `<crate>/test_mode` for 19 crates; no 1.0.1 crate has that feature. Task 7 runs the
  command before Task 8 re-derives the list.
- **Severity:** low. **Confidence:** high.
- **Suggested change:** Move "re-derive the feature list" into Task 2's `test_mode` bullet.

#### L6. The executed-sub-test gate needs the two SEL sub-tests of `system_tests` dispositioned

- **Where:** design line 253 ("173 of the 197 sub-test calls ran"); plan Task 1 (line 136).
- **Evidence:** the baseline ran `tenant` and `archiving` because the harness builds with
  `enterprise` (`.superpowers/sdd/baseline-01627/coverage/summary.md:153`); the design removes
  them (line 263).
- **Severity:** low. **Confidence:** high.
- **Suggested change:** List `system::tenant::test` and `system::archiving::test` in the
  committed dispositions as SEL-only so the gate does not fail on them.

#### L7. The directory-login site moved into `synchronize_account`; name it

- **Where:** design lines 145–147; plan Task 3 (lines 242–246).
- **Evidence:** `merge_directory_secret` is fork code (`crates/common/src/cache/directory.rs:27`,
  absent at `v0.16.27`); 1.0.1 builds the directory token through `build_directory_token` →
  `synchronize_account` (`v1.0.1:crates/common/src/auth/authentication.rs:663-672`,
  `v1.0.1:crates/common/src/cache/directory.rs:30`) after `assert_account_directory`
  (`…/authentication.rs:467`).
- **Severity:** low. **Confidence:** high.
- **Suggested change:** Name `authenticate_basic`'s directory branch, `assert_account_directory`
  and `synchronize_account` as the three 1.0.1 sites of the two refusals and the marker check.

#### L8. Fork-owned file set must be redefined against the new base

- **Where:** design "Gates", coverage (lines 301–302); plan Task 7 interface (lines 390–392).
- **Evidence:** the baseline defines fork-owned code as files added relative to `v0.16.27`
  (`coverage/summary.md`, "What the numbers cover").
- **Severity:** low. **Confidence:** high.
- **Suggested change:** Define it as files added relative to `v1.0.1` plus the `vault` crate
  and `za*` paths, and say so in the tool's interface.

#### L9. An early-return adaptation passes every gate

- **Where:** design "Gates" (lines 295–304); plan Task 7 (lines 414–417).
- **Evidence:** "executed" is a nonzero llvm-cov count on the sub-test function; a ported
  sub-test that returns early on a 1.0 condition (`if …unsupported… { return; }`) executes,
  is listed, keeps fork coverage and fails no mutation.
- **Severity:** low. **Confidence:** medium.
- **Suggested change:** Add to Task 7's inspection: no early return or `#[cfg]` body skip
  introduced in a ported test without a ledger entry, checked by diffing the test files
  against `5b914ddb5` for `return` and `cfg!` additions.

#### L10. CLAUDE.md's `-p http@0.16.26` is already stale at the 0.16.27 base

- **Where:** plan Global Constraints (line 48) versus `CLAUDE.md` ("use `-p http@0.16.26`").
- **Evidence:** `crates/http/Cargo.toml` is `0.16.27` at `main` and `1.0.1` at the target.
- **Severity:** low. **Confidence:** high.
- **Suggested change:** Task 9's CLAUDE.md update should fix the version and the pinned
  product-build line.

## Questions for Jay

1. **Dead properties (H1).** Which carrier do key-account dead properties get at 1.0.1:
   sealed `Collection::Metadata` documents under the collection key, the sealed content
   extras with upstream's metadata store bypassed, or a refusal? The same answer decides
   whether IMAP METADATA and JMAP object metadata are "refuse" or "seal".
2. **`ShareRevokeCleanup` (H2).** For a key-account owner, should the task skip the account
   entirely (the fork's ring already revokes), or run with a metadata-only write? Skipping is
   simpler and keeps ETags stable; running changes every affected event's ETag.
3. **Restore scope (M12).** Do you want all 283 upstream unit tests restored or ported, or a
   stop rule (unchanged-subject tests restored; replaced-internals tests only on hotspot
   modules)? This is the single largest effort item in the plan and it is not budgeted.
4. **External suites (M11).** Keep imaptest, JMAP-TestSuite and caldav-server-tester in this
   plan, or defer them to a follow-up and keep only the python-caldav diff against 0.16.27?
5. **Index/un-index (H3).** Given the source evidence that entries are ordered per account
   partition, adopt "ordered" now and drop the chunk-0 source-exists branch? If you keep the
   branch for ordinary accounts, it becomes an invariant-9 exception that CLAUDE.md must list.
6. **Per-user event data (M8).** Are `EventPreferences` sealed, and under which key for a
   grantee's preferences on an owner's event?
7. **Swap tier in tests (L4).** Should the harness start the swap tier by default (needed for
   Task 6's snapshot tests, closer to production) or only in those tests?
8. **Version.** `0.2.0-beta.1` is proposed; the release policy's "minor for breaking changes"
   supports it. Confirm, or pick `0.2.0` straight away if the native gates will be run before
   publication.

## Verified claims

The following statements in the documents were checked and hold:

- SHAs and lineage: `v1.0.1 = 23b69755c`, parent `v1.0.0 = 47df4c0ba`, parent
  `v0.16.27 = aa47d8ef7`; `git merge-base main v1.0.1` is `aa47d8ef7`. PR #62 merged at
  `05956a670`, PR #66 at `9a7574203`, issue #67 open.
- Dry merge of `v1.0.1` into `5b914ddb5` (`git merge-tree --write-tree`): 106 content, 27
  modify/delete, 1 add/add conflicts. No conflicting file under `crates/` had an inline
  `#[cfg(test)]` module at `v0.16.27`.
- 1.0.1 has no `#[test]`, no `tests/` directory, no `test_mode` feature, no `crates/migration`,
  no `.github/workflows/test.yml`; 562 `#[path = "…/stalwart-tests/…"]` slots (560 at 1.0.0);
  new workspace crates `calcard`, `jmap-tools`, `s3`, `sieve`, `smtp-proto`, `smtp-report`,
  `telemetry`, `text-extract`; `smtp-proto`'s `RequestReceiver::buf()` still exists
  (`v1.0.1:crates/smtp-proto/src/request/receiver.rs:161`).
- Baseline: 795 test functions, 697 unit tests (466 upstream, 183 SEL), 98 in `tests`, 197
  sub-test calls of which 173 executed, 19 coverage steps, the five coverage percentages,
  and the eleven SEL-only files under `tests/` (`oidc.rs`, eight `scim/*`, `archiving.rs`,
  `tenant.rs`); `issuer.rs` is dual-licensed and imports `get_token_for_client` from `oidc.rs`.
- Schema stamping: `DATABASE_SCHEMA_VERSION_BASE = 7`, `DATABASE_SCHEMA_OSS_FLAG = 1 << 31`,
  each edition refusing the other's store (`v1.0.1:crates/common/src/lib.rs:131-137`,
  `crates/common/src/manager/schema.rs:33-48`); the harness builds `store`/`common` with
  `enterprise` (`tests/Cargo.toml:22, 42`).
- Reserved ids: `PrincipalField` 150/151 free (highest upstream value 110), account flag
  `1 << 9` free (highest `1 << 8`), vanished collection 250 free (251–254 used).
- Interfaces: `Server::authenticate(&AuthRequest) -> trc::Result<Authenticated { access_token,
  expires_in }>`; `HttpAuthenticated`; `CalendarEventContent { data, preferences }`;
  `CalendarEvent::{insert, update_full, update_meta}`; `GroupwareWrite::{full, meta_only}` (an
  enum) and `SplitUpdate::{full, meta_only}`; `email_ingest_into(&mut IngestEmail, &mut
  BatchBuilder, Option<PendingId>) -> PendingIngest`, `email_ingest_finish(PendingEmail,
  &AssignedIds)`, `IngestGroup::{ingest, flush}` (used by IMAP APPEND and JMAP
  `email/grouped.rs`; SMTP delivery still calls `email_ingest`); `metadata/{build, row,
  source, view}.rs`; `index_queue/{document, index, reindex, file, manager}.rs`;
  `tasks/{grantee_cleanup, rebuild_shares}.rs`; `cache/{large.rs, swap/}`;
  `storage/index/split.rs`; `BatchBuilder::queue_document_{index, unindex}`; `IndexDocument`;
  `PendingId`, `AssignedIds`; `types::DeadProperty` gone (162 references at 0.16.27, 0 at
  1.0.1); the swap framing's checksum is named `seal` (`cache/swap/frame.rs:26`).
- Fork facts: `RegistryStore::new` and `clone_with_public_url` are fork additions
  (`crates/store/src/build/registry.rs:133, 140`); `RegistryEnvStores` lives in
  `tests/src/utils/storage.rs:32`; `ZaCalendarContext::assert_in` at
  `crates/groupware/src/calendar/sharing/resolve.rs:90`; the named test files under
  `tests/src/za/` and `tests/src/za/mail/` exist; `tests/product/` and `tests/qualification/`
  do not yet (proposed); `fork-release.toml` pins `v0.16.27`/`aa47d8ef7` and version
  `0.1.1-beta.1`; the CI jobs `style`, `test`, `unit-tests`, `upstream-suites`, `zero-access`
  and `caldav-client` exist with the steps the plan lists; python-caldav is pinned at
  `111ae14b…` in both `runner.py` and `test.yml`.
