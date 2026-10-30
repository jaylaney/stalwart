# Stalwart 1.0 Fresh Store Upgrade Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve the fork's existing zero-access behaviour on freshly initialized upstream
1.0.1 stores, and keep the 0.16.27 test suite's coverage. Qualify the non-enterprise
RocksDB product.

**Architecture:** Merge the pinned upstream tag normally into an isolated fork worktree, and
restore the public tests that the merge deletes. Then port enforcement to the new
authentication, split calendar, pending-ID ingestion, index queue and cache boundaries, and
port the retained suite with it. Treat the core port as one coupled integration wave with
reviewed component checkpoints. Gate it against the recorded 0.16.27 test and coverage
baseline.

**Tech Stack:** Rust 2024, stable Rust, upstream 1.0.1's workspace-local libraries and rkyv
configuration, the existing vault crypto, RocksDB, cargo-llvm-cov, the Python product and
CalDAV harnesses, GitHub Actions.

**Spec:** [Stalwart 1.0 fresh store upgrade design](../specs/2026-10-27-stalwart-1-upgrade-design.md),
revision 2. Its C1–C11 contracts and the earlier approved feature designs remain binding.

**Status:** Revision 2, proposed, 2026-10-29. Revision 1 (2026-10-27) targeted v1.0.0 from
the 0.16.26 base. This is documentation only: the planning request authorizes no
implementation, build, tests, CI dispatch or live-store operation.

## Global Constraints

- Fork baseline: `main` at `9a7574203e9821a8313239f51a080c0657672be9` (the merge of PR #66).
  Upstream base: `v0.16.27`, `aa47d8ef7bfba7200d3293198ac2592747b20e73`.
- Merge only `v1.0.1`, commit `23b69755cb3cd8ecd952fe0443c5ac067a506184`; its parent is
  `v1.0.0`, `47df4c0baeb88d83424dfda6aa7cf6040706999e`. Preserve published history: never
  rebase it, and never add or write an upstream remote. Fetch the tag by URL.
- Baseline: the 0.16.27 measurement in the git-ignored `.superpowers/sdd/baseline-01627/`.
  Task 1 commits its reduced form, and the C10 gates compare against that file.
- Fresh stores only. No migration, backward reader or old-client-state preservation.
  Allocate a new state directory; do not delete/reset existing `.run/` data.
- Keep upstream 1.0 stored types as supplied; add no fork fields to them and hand-edit no
  generated registry schema or event enum. Check Principal 150/151, flag `1 << 9` and
  vanished collection 250; keep vault mutations behind their CAS/invalidation helpers.
- Preserve C1–C11 and the exact existing key lifetimes, crypto purposes, AAD ownership,
  pinning, sharing, scheduling and protocol refusals. No permissive intermediate stubs.
- Qualify single-node RocksDB with embedded and filesystem blobs; preserve backend
  refusals. Do not claim SQL, other stores, Redis/cluster or additional artifact support.
- Every Cargo command on this Mac starts with
  `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"` (the second entry is for
  `cargo-llvm-cov`). The http crate is `-p http@1.0.1`.
- Shipping command: `cargo build --locked --release -p stalwart --no-default-features
  --features rocks`. Never deploy `test_mode` or an enterprise-enabled test binary.
- Never set `RUSTFLAGS=--cfg stalwart_tests`. It links 562 modules from upstream's private
  sibling checkout, and the fork does not recreate them.
- Port no SEL-only test, and take nothing from an SEL-only file. Where an AGPL test needs a
  helper from one, write a replacement from the public specification.
- Run port-sharing integration suites sequentially. Use whole-suite selectors and retain
  all failure/retry results; documented bounded retries are not clean first-pass results.
- Primary owns Git integration and final verification. Coding runs in bounded subagents;
  shared manifests/common interfaces have one owner. No subagent may spawn further agents
  without an explicit capacity allocation. No push/publication/deployment in this plan.
- Apply the global routing policy and originating model ceiling: strong implementation
  with high reasoning for core ports, independent strongest-permitted reviewers with xhigh
  for authority/persistence and the whole branch, standard engineering for bounded harness
  work, and mechanical low only for prescribed transcription. Resolve actual model support
  at execution, disclose limitations, and never use max/ultra without explicit direction.

Contract ownership: C1–C4 are chiefly Task 3; C5 is Tasks 5–6; C6–C7 are Task 4 with Task 6
worker integration; C8 is Task 6; C9 is Tasks 2 and 4; C10 is Tasks 2 and 7; C11 is Tasks 8–9.

## Review Focus

1. A batch prepares ordinary mail, provisioning wins, and grouped flush or a later commit
   segment runs: no plaintext metadata/blob/index can commit as key-account data (Tasks 5–6).
2. A metadata-only calendar edit, notification or new object-metadata method bypasses the
   old tree path: protected fields stay sealed or the operation refuses (Tasks 4 and 6).
3. Cache snapshot creation/restoration races provisioning or revoke: no keys/decrypted
   response view persist, and stale cache data cannot restore access (Tasks 3 and 6).
4. New OAuth scopes/expiry/revocation are lost through fingerprint-cache adaptation or a
   late loader: ordinary scoped tokens remain constrained and key tokens stay keyless (Task 3).
5. The retained mega-suite passes after an inner call disappears, or the enterprise harness
   hides an OSS schema or startup problem: the inventory and product restart gates fail
   (Tasks 2, 7 and 8).
6. The merge deletes a test file without a conflict, a restored unit test module loses its
   subject, or a ported assertion changes with no expectation-ledger entry. The baseline
   comparison, the executed-sub-test check and the ledger review catch each one (Tasks 1, 2
   and 7).

## File ownership and dependencies

Crate paths abbreviated as `common/...`, `email/...` and similar below are relative to
`crates/`; all other paths are relative to the repository root.

| Area | Files and responsibility | Owner/checkpoint |
|---|---|---|
| Integration | `Cargo.toml`, `Cargo.lock`, crate manifests, tag merge, conflict ledger | Primary coordinates; one assigned writer |
| Harness | `tests/src/utils/{server,storage,cleanup}.rs`, `tests/src/lib.rs`, restored upstream unit tests (`restored_tests.rs`), `test_mode` support | Task 2 |
| Accounts | `crates/vault/`, `common/src/{auth,cache/principals.rs,ipc.rs,za/}`, HTTP auth/vault/contacts, protocol login handlers | Task 3 |
| Calendars | `groupware/src/calendar/{seal,sharing,drain,fanout,delegation,pinning,sequence}`, scheduling, DAV, HTTP shares | Task 4 |
| Mail | `email/src/message/{za,metadata,ingest}`, mail readers/writers in IMAP/POP3/JMAP/SMTP and tracing | Task 5 |
| Background | `services/src/index_queue/`, cleanup/rebuild tasks, `store/src/{search,write}`, `common/src/cache/{large,swap}` | Task 6 |
| Coverage | Existing za and protocol tests, `tests/qualification/`, inventory and coverage tooling, the expectation ledger | Task owners; Task 7 integrates |
| Product/release | `tests/product/`, existing CalDAV harness, workflows, identity tooling/docs | Tasks 8–9 |

Tasks 2–6 share types and test hooks. Default to serialized writers in one isolated worktree;
parallelize only after the primary freezes interfaces and assigns non-overlapping files.
In particular, HTTP shares versus contacts, store test support versus assertions, and
protocol login versus mail reader files need explicit ownership handoffs.

Checkpoints 1–6 are reviewable components of one core port, not separate releasable PRs.
The first honest integrated compile gate is Task 7. Earlier blocked commands are recorded
as blocked, not replaced by weakened behavior. Pure vault units may run earlier when the
workspace resolves; do not repeatedly rerun a known blocked integration build.

## Task 1: Capture contracts and prepare the integration worktree

**Files:** Create an execution ledger under the ignored `.superpowers/sdd/`. During
execution, record the hook inventory and the expectation ledger in
`docs/superpowers/plans/2026-10-27-stalwart-1-upgrade-outcome.md`. Create
`tests/qualification/baseline-0.16.27.json`. No production source changes in this
checkpoint.

**Interfaces:** Consumes the pinned refs, C1–C11 and the 0.16.27 baseline. Produces:
- an owner/path/contract/test map for every changed fork runtime hook;
- an agreed interface sheet for Tasks 2–6;
- the committed baseline;
- the list of paths the merge deleted, with how each was restored or dispositioned.

- [ ] Reconfirm the current fork SHA and any unrelated work. If HEAD changed, assess its delta
  against this plan before merging. Read the existing feature outcomes and accepted limits.
- [ ] Commit the reduced baseline from `.superpowers/sdd/baseline-01627/`, measured at
  `5b914ddb5`:
  - every libtest identifier per binary and feature set, with whether CI selects it after
    PR #66;
  - every suite sub-test call, with its mode and cfg guards and whether it executed in each
    mode;
  - per-file line and function coverage of fork-owned code, and per-crate totals;
  - the dispositions known now: SEL-only, ungated as today, and needs Docker so CI only.

  PR #66's merge (`9a7574203`) changed tested code: it added the `za::external_directory`
  module and the ACL variant's wait. So re-list the libtest identifiers and sub-test calls at
  `9a7574203` before committing. Keep the coverage figures from `5b914ddb5`, and note the SHA
  each figure was measured at. The raw coverage exports stay out of Git.
- [ ] Use the worktree skill/native lifecycle tools to create/reuse an isolated implementation
  worktree from the fork tip. Give Cargo output and disposable server state worktree-specific
  paths; do not share them with another running build.
- [ ] Fetch and verify the exact release tag by URL. Inventory fork changes since the old
  base, deleted upstream paths and conflict paths; distinguish textual conflict from a hook
  whose target moved. Record each hook's replacement and regression owner.
- [ ] Freeze the shared adaptations: keyless `Authenticated`, request/connection key ownership,
  account epoch/classification, split calendar views, metadata/extras mapping, pending-ID
  fences and search/snapshot inputs. Use the upstream interfaces named in Tasks 3–6.
- [ ] Count upstream's private-test slots (`#[path = "…/stalwart-tests/…"]`) in the target,
  and record the count: 562 at v1.0.1. List the modules gaining slots relative to the
  previous release, for review in Tasks 3–6.
- [ ] Primary reviews this inventory before assigning writers. Then start the normal merge:
  1. Run `git merge --no-commit`.
  2. Immediately restore `tests/` in full from the fork side (`git checkout HEAD -- tests/`).
  3. Record the restored paths. The dry merge deletes 520 of 598 files without a conflict.
  4. List every upstream file whose 0.16.27 inline test module the merge removed, for Task 2.
  5. Preserve deleted-file fork behaviour in its replacement location.

  The primary coordinates the merge index and the eventual commit, not concurrent component
  writers.

**Check:** Every original runtime hook has an owner and a disposition. Every removed index,
metadata or test-support path, and every removed test, is accounted for. No data migration
task appears in the scope.

## Task 2: Restore the owned workspace and test infrastructure

**Files:** Workspace/crate manifests; `tests/Cargo.toml`; `tests/src/utils/{server,storage,
cleanup}.rs`; `crates/store/src/build/registry.rs`; retained public unit modules in touched
crates; existing fork `test_mode` declarations and hooks.

**Interfaces:** Retains `TestServerBuilder`, `RegistryEnvStores::insert_stores_from_env`,
`RegistryStore::{new,clone_with_public_url}` as test-only helpers. Adapts boot to
`Caches::parse(bp, &storage) -> (Caches, swap_rx)` and the corresponding cache-tier startup.

- [ ] Keep `crates/vault` and `tests` as workspace members alongside new upstream crates;
  accept removal of `crates/migration` without inventing a fork migration replacement.
- [ ] Retain same-store behavior tests even when named `legacy`, including
  `dav_seal::test_legacy`, which injects current-format unsealed rows. Preserve conversion,
  recovery, corrupt-record and mixed-state cases; these are not old-version migration tests.
- [ ] Align calcard, parser, sieve, SMTP, JMAP tools and rkyv dependencies with upstream's
  local crates/types. Regenerate the lockfile deliberately; preserve independent product
  versioning rather than taking upstream's main-package version.
- [ ] Restore `test_mode` only where the retained suite needs it. Port the existing pause,
  fault, trace, mock-network and weakened-Argon2 behaviour under that feature, as one-line
  calls into fork-owned modules. Keep the private `stalwart_tests` references disabled.
- [ ] Restore upstream's unit tests. Move each 0.16.27 inline test module on Task 1's list
  into a fork-owned `restored_tests.rs` next to its 1.0 counterpart, attached by a single
  `#[cfg(test)] mod restored_tests;` line.
  - A module whose subject moved follows its subject.
  - A module whose subject is gone gets the "subject deleted" disposition, with the 1.0 diff
    that removed it.
  - A module whose subject's internals changed gets an equivalent test of the new code,
    recorded as a replacement.
  - The SEL-only `scim` and `scim-proto` tests are dispositioned, not moved. The fork's own
    unit tests move with their modules.
- [ ] Remove the SEL-only test files with a recorded disposition, and remove their sub-test
  calls: `directory/oidc.rs`, the SEL-only `scim/*` files, `system/archiving.rs` and
  `system/tenant.rs`. Write a replacement for `get_token_for_client`, which the dual-licensed
  `directory/issuer.rs` imports from `oidc.rs`. Base it on RFC 6749's token endpoint, not on
  the SEL file.
- [ ] Adapt the two missing registry constructors. Keep the existing fork-owned
  `RegistryEnvStores` trait instead of looking for it in upstream private test support.
- [ ] Port the test boot and cache workers and the queue-drain helpers. Before cleanup or
  scanning, `wait_for_tasks` must wait for both the scheduled tasks and the new partitioned
  index queue (`services::index_queue::manager`), including retries and status keys.
- [ ] Update cleanup and store enumeration for 1.0's key spaces: split content, metadata,
  search postings, dictionaries and external blobs. Match 1.0's closed `Subspace` enum
  exhaustively, with no catch-all arm, so that a subspace added later stops the build.
  Do not exempt an entire new space to hide leftovers. Keep the vault and pin exemptions only
  for fixture accounts that still exist.

**Check:** Manifest and type resolution has no accidental duplication of an old and a new
library at a shared API, and test support stays feature-gated. Every module on Task 1's list
is restored or dispositioned. The final nonzero test inventory is Task 7.

## Task 3: Port vault authority, authentication and session ownership

**Files:** `crates/vault/`; `common/src/auth/{authentication,vault,access_token,mod}.rs`,
`cache/principals.rs`, `ipc.rs`, `za/`; `http/src/auth/authenticate.rs`, vault/contacts APIs;
IMAP/POP3 authentication; SMTP/ManageSieve authentication and JMAP registry refusal sites.
Tests: `tests/src/za/{setup,password,app_password,totp,caches,registry,external_directory,
raw_input,tracing}.rs`, `tests/src/za/mail/login.rs` and vault units.

**Interfaces:** Preserve `Server::authenticate(&AuthRequest) -> trc::Result<Authenticated>`
with upstream `access_token` and `expires_in`; common tokens remain keyless. Preserve new
`HttpAuthenticated` epoch/scope/expiry semantics while protocol layers attach their own
optional `Arc<SessionKeys>`. Retain `za_commit`/`za_vault_commit` as mutation boundaries.

- [ ] Extend existing regression cases for `scoped_cache_hit_preserves_scope`,
  `revoked_grant_cannot_repopulate_cache` and `expired_bearer_cache_entry_is_refused`.
  Assert preserved scope/expiry on ordinary accounts and refusal of forbidden key-account
  bearer/impersonation authentication; use deterministic epoch pauses for late loaders.
- [ ] Port vault storage, checked decoding, pin records and crypto units to the 1.0 archive
  configuration without legacy readers. Assert setup/password/recovery/app-wrap/pin round
  trips and malformed-record refusal without secret-bearing Debug/errors.
- [ ] Place marker handling in the new basic-auth path. Preserve the primary, app-password
  and TOTP logins, pending-wrap publication, the self-check and the terminal
  Account-before-vault ordering.
- [ ] Place `za_refuse_directory_login` and `za_refuse_directory_account` at the 1.0 sites of
  the external-directory Basic and Bearer arms, and keep `merge_directory_secret`'s marker
  check.
  - `za::external_directory` must still fail with each refusal disabled and with the marker
    check disabled.
  - What a synchronisation may change besides the credential is fork issue #67's open
    question, and is not decided here.
- [ ] Port the new account cache with vault classification and generation fencing. Preserve
  keyed credential fingerprints, bounded key residency and invalidation across every vault
  write; keep new grant epochs and expiry/scope checks on cache hits and misses.
- [ ] Restore request/connection ownership at every protocol login and drop boundary.
  Password changes and app revocation retain the approved next-authentication/connection
  semantics; do not invent immediate erasure of already-held connection keys.
- [ ] Preserve CORS, endpoint response shapes and registry-edit refusals. Review pre-auth
  parser/form/error traces and authenticated key-session tracing against canary tests.

**Check:** Source review accounts for every authentication entrypoint. Existing and added
cases run at Task 7; successful compilation alone does not establish C1–C4.

## Task 4: Port calendar sealing, sharing, scheduling and DAV together

**Files:** `groupware/src/calendar/{mod,storage}.rs`, `seal/`, `sharing/`, `drain.rs`,
`fanout.rs`, `delegation.rs`, `pinning.rs`, `sequence.rs`, scheduling; DAV handlers;
`http/src/api/shares.rs`, contacts redelivery integration; relevant common storage builders.
Tests: `tests/src/za/{dav_seal,pairs,fanout,drain,drain_limits,pinning,reply,sharing,
sharing_move_atomic,sharing_races,sharing_review,sharing_tombstones}.rs`, seal/sharing units.

**Interfaces:** Adapt existing seal/domain/context helpers to paired `CalendarEvent` and
`CalendarEventContent`. Target `CalendarEvent::{insert,update_full,update_meta}` and
`GroupwareWrite::full(meta, content, ctx)` / `SplitUpdate::full`; preserve
`ZaCalendarContext::assert_in(&mut BatchBuilder)` authority at the effective transaction.

- [ ] Add `split_metadata_only_update_keeps_sealed_content`: update an allowed metadata
  property, read back as owner/grantee, and scan both rows/indexes for protected canaries.
  Preserve the stored-version identity of read-only views.
- [ ] Port sealed tree/extras/collection codecs and the new metadata mapping. Decide the
  exact fork-owned extra payload, if needed, in this checkpoint's reviewed interface sheet;
  add round trips for custom XML properties, preferences, colour/order and corrupted extras.
  Seal/sanitize before builders compute derived values; never index an unsealed read view.
- [ ] Port all physical event/link assertions, assigned parent/document identity, conversion
  chunks, rollback/reconversion, ring cache identity and fresh own-id grant admission.
  Reuse the existing deterministic races instead of replacing assertions with cache hints.
- [ ] Port fan-out/drain/pins/UID leases/SEQUENCE and delegated scheduling. Update the
  `itip_merge_changes` caller in `calendar/drain.rs` to carry the incoming tree. Add
  `incoming_timezone_update_survives_seal` for a moved recurring occurrence introducing TZID.
- [ ] Adapt DAV GET/PUT/REPORT/PROPFIND/PROPPATCH/ACL/COPY/MOVE/delete and sharing endpoints.
  Keep owner-only inbox access, bounded retries, shared-parent constraints, sync boundaries,
  stable-id revoke and quiet catch-all behavior.
- [ ] Extend lifecycle cases to cover the new grantee cleanup and share rebuild tasks:
  `cleanup_cannot_restore_revoked_ring` must prove revoke remains effective after background
  work, cache refresh and a later grant. Task 6 owns their final worker integration.

**Check:** Every old calendar mutation maps to a full or metadata-only fenced 1.0 write.
Sharing's differential oracle still permits only its recorded limits. Run cases at Task 7.

## Task 5: Port sealed mail, grouped ingestion and all readers

**Files:** `email/src/message/{za,metadata,ingest}`, including `ingest/group.rs`; mail body,
section, upload, copy, submission and trace paths in HTTP/JMAP/IMAP/POP3/SMTP/spam-filter.
Tests: existing `tests/src/za/mail/{delivery,writes,fence,reads,uploads,gates,traces}.rs`,
plain/key IMAP and JMAP variants; related email units.

**Interfaces:** Target `email_ingest_into(&mut IngestEmail, &mut BatchBuilder,
Option<PendingId>) -> trc::Result<PendingIngest>`,
`email_ingest_finish(PendingEmail, &AssignedIds) -> IngestedEmail`, and
`IngestGroup::{ingest,flush}`. Stub/read views use the 1.0 metadata and parser types.

- [ ] Add `grouped_ingest_provisioning_wins_before_flush`,
  `grouped_ingest_mixed_key_and_plain` and
  `grouped_rejection_discards_uncommitted_plain_metadata` to the existing fence/delivery
  modules. Assert sealed key mail, unchanged ordinary mail and no post-provision plaintext
  commit or abandoned plaintext metadata after rejection/retry.
- [ ] Port stub construction, sealed blob generation and marker detection before any stored
  preparation. Carry classification/public-key decisions and vault-absence assertions into
  every effective batch segment, including grouped writes and pending-ID resolution.
- [ ] Port nested MIME/header/source/part readers and encrypted uploads. Extend read fixtures
  for `assigned_ids_preserve_sealed_read_offsets`; assert exact nested message/attachment
  bytes, correct owner binding and refusal of keyless or corrupt reads.
- [ ] Preserve no key-account threading/training/content push, state-only notifications and
  submission behavior across prepare/flush/finish. Recheck raw blob consumers, cross-account
  COPY/MOVE refusals and ordinary-account assertions in copied/search-derived writes.
- [ ] Preserve wire/error/structured trace redaction, including coalesced SMTP RCPT+DATA,
  Sieve/vacation diagnostics, Message-ID fields and parsed header/search fields.

**Check:** Every delivery/import/append/copy/upload/read route has a named sealing, unsealing
or refusal boundary. There is no single-message-only guard left around a grouped bypass.

## Task 6: Port search, snapshots and new metadata surfaces

**Files:** `services/src/index_queue/{document,index,reindex,file}.rs`,
`task_manager/tasks/{grantee_cleanup,rebuild_shares}.rs`; `store/src/{search,write}`;
`common/src/cache/{large,swap}`; `jmap/src/api/metadata/`, JMAP handlers/session capability
mapping; `imap/src/op/metadata/` and capabilities. Tests: `tests/src/za/{index_task,leak,
gating,disabled}.rs`, `mail/{search,fence,traces}.rs`; proposed `tests/src/za/cache_swap.rs`.

**Interfaces:** Retain C8 through new `IndexDocument` builders, queue/reindex and
`BatchBuilder::queue_document_{index,unindex}`. Retain provisioning/deletion assertions at
each search commit segment. Snapshot writers/readers consume stored sanitized views only.

- [ ] Add `reindex_key_account_stays_visible_fields_only` and
  `attachment_indexer_never_reads_sealed_blob`. Verify arrival time/size survive while
  protected terms, thread/header material and extracted attachment text do not persist.
- [ ] Port the old index-task guards to every new worker/rebuild path. Map old document
  assertions onto 1.0 postings/queue commits; pause later chunks while provisioning or
  deleting the account and assert no orphan plaintext/index or infinite retry remains.
- [ ] Settle the index/un-index race (design: "Indexing, metadata and cache snapshots").
  Establish from 1.0.1's `index_queue` source whether a document's index and un-index
  entries share a partition and apply in order. Then reproduce the 0.16 interleaving with a
  pause hook: an un-index queued before the import's index write lands.
  - **If ordered,** record it. Flip the in-flight index tests in `za::mail::fence` to
    expect no late write, and remove the key-mode JMAP ACL variant's `wait_for_tasks`.
  - **If not ordered,** add the source-exists assertion on chunk 0, confirmed by a read
    outside the transaction (the R58/R62/R63 pattern). Attach it only where the search
    store is the data store.
- [ ] Integrate the cleanup and rebuild tasks with the Task 4 resolver and ring authority,
  and make their completion observable to the Task 2 drain helper.
- [ ] Add `swap_roundtrip_contains_no_protected_canary` and
  `swap_restore_after_revoke_cannot_restore_access`. Exercise file and blob snapshots with
  cache eviction, fresh loads, provisioning/revocation races and process-independent decode.
  Scan decoded frames/archives as well as raw files; compression is not a privacy control.
- [ ] Trace all snapshot population, serialization, load and invalidation inputs. Prevent
  unsealed response views/session keys from entering persistent caches. Apply guards in
  shared backend-independent code; keep Redis/cluster runtime qualification out of scope.
- [ ] Gate new IMAP METADATA/JMAP object-metadata key-account operations that lack encrypted
  carriers. Test read/write/copy/batch routes and capability/refusal behavior. Verify plain
  metadata operations remain upstream's and ported DAV properties still round-trip.
- [ ] Extend leak enumeration to new store/search/metadata/cache formats and tasks. Use
  ordinary-account positive controls and test-only planted violations to prove each newly
  covered decoder detects content; retain existing scanner controls/mutation coverage.

**Check:** No new background path obtains session keys or indexes decrypted content. Every
new persistent surface appears in the leak inventory or has a source-backed exclusion.

## Task 7: Reach the integrated compile and coverage gate

**Files:** All the outputs of Tasks 2–6; `tests/qualification/baseline-0.16.27.json` (Task
1); proposed `tests/qualification/dispositions.json`, `scripts/tests/check_test_inventory.py`
and `scripts/tests/check_coverage.py`; the expectation ledger in the outcome note.

**Interfaces:**
- **The inventory tool** accepts `--baseline PATH --dispositions PATH --listing PATH
  --ignored-listing PATH --executed PATH`. It exits nonzero when a required identifier is
  missing or ignored, when a required exact selector selects nothing, or when a required
  sub-test call did not execute in a mode where it executed at 0.16.27.
  - Capture both `-- --list` and `-- --ignored --list` for each required package and
    feature combination; the ordinary listing alone does not establish that a test will run.
  - "Executed" means a sub-test function's llvm-cov execution count is above zero in that
    step's export, as in the baseline. Declared modules do not count.
- **The coverage tool** compares the candidate's per-file export with the baseline. It fails
  if line coverage of fork-owned code falls below 96.4% (measured over the ported fork-owned
  files). It reports per-crate deltas for upstream code.
- **Every disposition** names its kind (SEL-only, subject deleted, replaced by an
  equivalent, ungated as today) and its evidence.

- [ ] Primary inspects all conflict resolutions and dispositions, including paths that
  auto-merged. Search for unresolved conflict markers, disabled tests, placeholder returns,
  lost test hooks and accidentally restored private test dependencies.
- [ ] Compile the shipping product with `cargo check -p stalwart --no-default-features
  --features rocks` and the harness with `cargo test -p tests --no-run`; resolve remaining
  cross-crate errors without reducing contracts. Formatting must also pass.
- [ ] Compare the libtest listings with the committed baseline. Every renamed, replaced or
  removed test needs a named equivalent or an approved disposition with evidence. Fork
  invariants and za sub-tests cannot be deleted as obsolete upstream APIs.
- [ ] Implement the inventory and coverage gates, with self-tests for the missing, ignored,
  empty-selection, not-executed and coverage-drop cases. Prove each gate bites:
  1. In a disposable change, omit a za sub-test call once, and drop one restored unit test
     module once.
  2. Show that the gate fails each time.
  3. Restore, and rerun.
- [ ] Rerun the baseline's 19-step coverage net on the candidate with the same commands
  (`cargo llvm-cov --no-report`, one step at a time, the profiles merged afterwards). Feed
  its exports to both gates.
- [ ] Run the sequential matrix below. Required cases must reach their assertions; a compile
  failure or zero selected tests does not demonstrate a regression check. Judge a failed
  assertion against current 1.0 behaviour rather than rewriting the expectation to match the
  port. Record every changed expectation in the ledger, with the 1.0 change behind it.
- [ ] For each re-ported protection hook named in Tasks 3–6, disable it in a disposable
  change and record that a named test fails. Record a hook no test catches as a finding.
- [ ] Record each gate's exact SHA/command/nonzero counts, new case completion, failures,
  skips and bounded retries. Primary verifies results independently before the core merge
  is described as passing. Make the first integrated checkpoint commit only after resolving
  the merge coherently; further reviewed corrections can be focused commits.

All Cargo rows use the PATH prefix above. Integration rows also use
`STORE=RocksDb RUST_MIN_STACK=16777216`; set `ZA_KEY_ACCOUNTS=1` only on key rows, and unset
it explicitly for plain rows. Use separate processes sequentially.

| Gate | Command or required variant |
|---|---|
| Formatting | `cargo fmt --all --check` |
| Workspace units | The `unit-tests` job's command from PR #66, ported: `cargo test --workspace --exclude tests --exclude scim --exclude scim-proto --lib --bins --features "$UNIT_TEST_FEATURES"`. It includes the restored upstream unit tests, and its listing equals the baseline's minus dispositions. |
| Vault and groupware | `cargo test -p vault -p groupware --features vault/test_mode` |
| ZA | `cargo test -p tests za::za_tests -- --exact --nocapture`, and the leak scanner's unit test `za::leak::tests::try_archive_short_values_are_not_archives -- --exact` |
| ZA filesystem blobs | The same ZA command with `BLOB_STORE=FileSystem` |
| CalDAV plain/key | `cargo test -p tests webdav::webdav_tests -- --exact --nocapture`, twice; and `webdav::cal_query::` |
| IMAP plain/key | `cargo test -p tests imap::imap_tests -- --exact --nocapture`, twice |
| JMAP plain/key | `cargo test -p tests jmap::jmap_tests -- --exact --nocapture`, twice |
| Upstream suites | `store::store_tests`, `store::blob::blob_tests`, `store::lookup::lookup_tests`, `store::search_tests`, `telemetry::telemetry_tests`, `system::system_tests` (without the SEL sub-tests), each by its exact name |
| CI only (Docker) | `directory` (without `oidc`) and `automation::automation_tests`, run by the CI jobs in Task 8 |
| SMTP | `cargo test -p tests smtp` with CI's `SPAM_RULES_URL`, and the four known flakes rerun by name once |

## Task 8: Qualify fresh stores with the OSS shipping binary

**Files:** Proposed `tests/product/fresh_store.py`; existing
`tests/caldav_client/{runner,test_runner}.py` and client manifest/policy files;
`.github/workflows/test.yml`; `scripts/local-server/` schema-dependent provisioning inputs.

**Interfaces:** New CLI: `python3 -B tests/product/fresh_store.py --binary PATH --log-dir PATH`.
It allocates its own fresh state directory and uses public APIs/protocols. It has no store
conversion mode and refuses a pre-existing data directory supplied as a fresh target.

- [ ] Add harness failure tests: wrong binary path, failed startup, failed provisioning,
  restart crash, missing readback and timeout must all return nonzero with sanitized logs.
  Use the established CalDAV `ProductServer` lifecycle pattern; keep bootstrap schema fixes
  consistent between both harnesses without enabling test-only product behavior.
- [ ] Build the locked release product without enterprise/test mode, in a separate Cargo
  process from enterprise-enabled harness builds. Confirm the actual feature graph and
  expected OSS database version rather than inferring them from package naming.
- [ ] Create a fresh store, one ordinary account and two key accounts. Complete setup, pin
  contacts, seed mail with nested attachment bytes and recurring calendar events, and
  exercise view/edit sharing through public interfaces. Do not print credentials or keys.
- [ ] Stop the process after work drains; restart the same binary/store normally. Authenticate
  with new connections and check ordinary/key mail, attachments, calendar preferences,
  recurrence, pins, sharing permissions, revoke/regrant and keyless refusal. An empty-store
  bootstrap/restart alone cannot satisfy this gate.
- [ ] Run the existing unfiltered independent CalDAV runner against that product configuration.
  Preserve its reviewed plain/key collection manifests and exceptions; do not shrink the
  manifest or broaden skips to accommodate failures. Run its complete harness unit suite.
  Use pinned python-caldav commit `111ae14b875f40d100d5259820f10210f00e20f9`, requirements
  from `tests/caldav_client/requirements.txt`, and the existing `collection.json` and
  `compatibility.json` policies. The commands are `python3 -B -m unittest discover -s
  tests/caldav_client -p test_runner.py -v` and `python3 -B tests/caldav_client/runner.py
  --binary PATH --caldav-source PATH --log-dir PATH`, with both modes (the default).
- [ ] Adapt local provisioning scripts to the 1.0 schema and document a new `ZA_RUN_DIR`
  for development. Fresh fixture creation is allowed during execution; deleting old local
  directories or reusing real account state is not part of this task.
- [ ] Restore the CI jobs deliberately, every job and step PR #66 left in place, ported to
  the 1.0 commands:
  - `style`;
  - `test`, with the directory step less `oidc` and the SMTP step;
  - `unit-tests` (with its feature list re-derived for 1.0's workspace);
  - `upstream-suites`;
  - `zero-access`, with its embedded and filesystem blob runs, the plain and key protocol
    steps, the vault `test_mode` step and the leak-scanner step;
  - the independent CalDAV jobs.

  Then add three jobs: the inventory and coverage gates, the fresh OSS restart, and the
  external protocol suites below. Upload sanitized failure receipts. Public CI cannot claim
  that upstream's private suites ran.
- [ ] Add a CI job for the external protocol suites. It runs Dovecot's imaptest (with the 112
  scripts in `tests/resources`), python caldav-server-tester and Fastmail's JMAP-TestSuite
  against the OSS product binary of the candidate, and of the 0.16.27 fork at the baseline
  SHA, in the same job. Diff the two sets of results.
  - Each difference is recorded as a 1.0 behaviour change with its upstream source, or
    treated as a regression.
  - Pin each suite's source revision. These suites add coverage; they do not replace any
    required test.
- [ ] Optionally, if the Bearer arm of the external-directory refusal is to be covered: add a
  CI-only key-account case against a Keycloak container to the directory step.

**Check:** Fresh OSS creation and the persisted-data restart pass separately from the Rust
harness, which is built with `enterprise`. The independent client suite keeps its existing
coverage, and the external suites' differences against 0.16.27 are all explained.

## Task 9: Final review, provenance and release qualification record

**Files:** `fork-release.toml`, `crates/main/Cargo.toml`, lock entry;
`crates/common/{build.rs,build_support/release.rs,src/version.rs}`;
`scripts/tests/test_release_metadata.py`, `scripts/release/`;
`.github/workflows/product-build.yml`; `CHANGELOG-fork.md`, `docs/releases.md`, `CLAUDE.md`,
`README.md`, developer guide and the upgrade outcome record.

**Interfaces:** Preserve the existing binary identity/provenance verifier and immutable
fork release contract. Proposed preview is `0.2.0-beta.1`; final version remains a release
review choice. After full integration, the upstream metadata becomes the pinned 1.0.1 tag
and SHA.

- [ ] Update base/version-specific developer commands, schema/fresh-store instructions and
  package disambiguation. Explain unsupported old stores and separate fork/upstream
  versions. Preserve upstream changelog/history, attribution and license boundaries.
- [ ] Run the metadata and product-verifier unit suites; verify actual version/help/source
  output and shipping features. Do not hard-code a self-referential final source SHA in a
  preparation commit or modify an already published tag/artifact.
  Run `python3 -B scripts/tests/test_release_metadata.py` and `python3 -B -m unittest
  discover -s scripts/release -p 'test_*.py' -v` sequentially.
- [ ] Require a fresh, read-only whole-branch reviewer with clean context: provide base/head,
  repository guidance, this design and accepted feature contracts, without implementer
  reports or prior finding conclusions. Review identity, ordering, persistence, permission,
  content leakage and combined behavior across components.
- [ ] Adjudicate each finding against source; delegate focused fixes to its implementer.
  Re-review affected corrections and rerun their complete relevant gates, expanding the
  final matrix only when new changes/failures require it. Leave no unresolved correctness
  or security finding hidden behind a green build.
- [ ] Prepare exact-source Linux product qualification under the existing workflow. CI
  dispatch/push/publication still needs its own authorization. Record clean source SHA,
  checksum, attestation, CPU baseline and measured GLIBC requirement from the real artifact.
- [ ] Run or explicitly leave pending the existing disposable native Calendar/Mail client
  checklist, with exact build/client versions. Automated clients do not qualify native UI;
  do not reuse previous-base results as evidence for this candidate.
- [ ] Finish the outcome record:
  - the hook dispositions and the test dispositions;
  - the expectation ledger;
  - coverage against the baseline;
  - the private-test slot count;
  - the external suites' differences;
  - the exact commands and counts, failures and retries;
  - the OSS product result and the independent review;
  - the pending native gates and the excluded stores and platforms.

  Keep the candidate a limited prerelease until its applicable gates pass.
- [ ] Note for the deploy repository, which is outside this plan: the `mail.circolo.me`
  cutover is a rebuild on a fresh store. It needs new setup tokens and new enrolment, with
  the pre-cutover snapshot and the 0.16 binary kept for rollback.

**Stop:** Hand over the reviewed branch/qualification record. Publishing, deployment,
system installs and destructive cleanup are separate user actions, not implied completion
steps. The plan is complete only as a plan; implementation starts after review/approval.
