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
revision 4. Its C1–C11 contracts and the earlier approved feature designs remain binding.

**Status:** Revision 4, proposed, 2026-10-30. It adopts both re-reviews of revision 3
([Fable](../specs/2026-10-27-stalwart-1-upgrade-design-review-r3.md), and the "Revision 3
re-review" section of the
[Codex review](2026-10-27-stalwart-1-upgrade-codex-review-2026-10-30.md)) and Jay's answers
to their questions. Revision 3 adopted the first reviews of both
([Fable](../specs/2026-10-27-stalwart-1-upgrade-design-review.md), Codex). Revision 2
(2026-10-29) moved to 0.16.27 and 1.0.1.
Revision 1 (2026-10-27) targeted v1.0.0 from the 0.16.26 base. This is documentation only:
the planning request authorizes no implementation, build, tests, CI dispatch or live-store
operation.

## Global Constraints

- Fork baseline: `main` at `9a7574203e9821a8313239f51a080c0657672be9` (the merge of PR #66).
  Upstream base: `v0.16.27`, `aa47d8ef7bfba7200d3293198ac2592747b20e73`.
- Merge only `v1.0.1`, commit `23b69755cb3cd8ecd952fe0443c5ac067a506184`; its parent is
  `v1.0.0`, `47df4c0baeb88d83424dfda6aa7cf6040706999e`. Preserve published history: never
  rebase it, and never add or write an upstream remote. Fetch the tag by URL.
- Baseline: the 0.16.27 measurement in the git-ignored `.superpowers/sdd/baseline-01627/`.
  Task 1 commits its reduced form, and the C10 gates compare against that file.
- Maintain upstream: where a choice exists, use upstream's mechanism and code unchanged.
  Add fork code only where a contract requires it, in fork-owned modules reached by one-line
  insertions. Reuse upstream's builders, stores and tasks with different inputs, or skip
  them for key accounts, rather than re-deriving them. Restore upstream's tests verbatim
  wherever their subject exists.
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
6. Any of these, each of which the baseline comparison, the strict executed-sub-test check
   or the ledger review must catch (Tasks 1, 2 and 7):
   - the merge deletes a test file without a conflict;
   - a restored unit test module loses its subject or its libtest identifiers;
   - a sub-test's future is built but never awaited, or returns early;
   - a ported assertion changes with no expectation-ledger entry.
7. A keyless background task rewrites a sealed event (the share-revocation cleanup), or a
   dead property or object-metadata value lands unsealed in `Collection::Metadata` (Tasks
   4 and 6).

## File ownership and dependencies

Crate paths abbreviated as `common/...`, `email/...` and similar below are relative to
`crates/`; all other paths are relative to the repository root.

| Area | Files and responsibility | Owner/checkpoint |
|---|---|---|
| Integration | `Cargo.toml`, `Cargo.lock`, crate manifests, tag merge, conflict ledger | Primary coordinates; one assigned writer |
| Harness | `tests/src/utils/{server,storage,cleanup}.rs`, `tests/src/lib.rs`, restored upstream unit tests (`*restored_tests.rs`), `test_mode` support; the fork's hooks in `types` and `store` far enough to compile (reserved fields and collection, `ZA_UID_LEASE_PREFIX`, `za_test.rs`, `has_account_search_terms`, R58's carrier type) | Task 2 |
| Accounts | `crates/vault/`, `common/src/{auth,cache/principals.rs,ipc.rs,za/}`, HTTP auth/vault/contacts, protocol login handlers | Task 3 |
| Calendars | `groupware/src/calendar/{seal,sharing,drain,fanout,delegation,pinning,sequence}`, the new `privacy`, `hidden`, `participants`, `overlap` and `notification` modules, scheduling, DAV (including `dav/src/common/propfind/`, which replaced `propfind.rs`, and `dav/src/common/dead/`), `common/src/storage/metadata/`, HTTP shares | Task 4 |
| Mail | `email/src/message/{za,metadata,ingest}`, mail readers/writers in IMAP/POP3/JMAP/SMTP and tracing; `imap/src/op/fetch/` (replaced `fetch.rs`) and `imap/src/core/view.rs` (replaced `core/message.rs`); `spam-filter/src/analysis/llm.rs`'s hook, now in the SEL-only `common/src/enterprise/ai/` (a disposition, not a port) | Task 5 |
| Background | `services/src/index_queue/`, `task_manager/tasks/{grantee_cleanup,rebuild_shares}`, `store/src/{search,write}`, `common/src/cache/{large,swap}`, `jmap/src/api/metadata/`, `imap/src/op/metadata/` | Task 6 |
| Coverage | Existing za and protocol tests, `tests/qualification/`, inventory and coverage tooling, the expectation ledger | Task owners; Task 7 integrates |
| Product/release | `tests/product/`, existing CalDAV harness, workflows, identity tooling/docs | Tasks 8–9 |

Tasks 2–6 share types and test hooks. Default to serialized writers in one isolated worktree;
parallelize only after the primary freezes interfaces and assigns non-overlapping files.
In particular, HTTP shares versus contacts, store test support versus assertions, and
protocol login versus mail reader files need explicit ownership handoffs.

Checkpoints 1–6 are reviewable components of one core port, not separate releasable PRs.
Each of Tasks 2–6 closes with a crate-level `cargo check`, in dependency order, with no
permissive shim. `common` depends on `store` and `types`, and every later crate depends on
`common`:
- Task 2: `-p types -p store -p vault`;
- Task 3: `-p common`;
- Task 4: `-p groupware -p dav`;
- Task 5: `-p email -p imap -p jmap -p smtp -p pop3`;
- Task 6: `-p services -p store`.

Task 2 must already have made the workspace resolve. The integrated compile gate of the
product and the harness is Task 7. A command that is still blocked is recorded as blocked,
not replaced by weakened behaviour. Pure vault units may run as soon as the workspace
resolves.

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
  - every libtest identifier per binary and feature set, with its status: run, ignored (the
    six `#[ignore]`d tests) or not selected, and whether CI selects it after PR #66;
  - every suite sub-test call, with its mode and cfg guards, whether it is `async`, and
    whether it executed in each mode under the strict rule. The rule needs two positive
    counts:
    - the body record: the async body's own record for an `async fn`, never the outer
      constructor record; the function's own record for a plain `fn`;
    - the body's final region, the region that ends at the body's closing brace.

    Recompute this from the stored region-level exports in `coverage/steps/*.json`; the
    baseline's `subtests-ran.json` used a permissive rule. Report every sub-test that
    completed at 0.16.27 but fails the rule (a diverging or branching tail). Give each an
    explicit completion marker in Task 7, rather than loosening the rule.
  - the fork-owned cohort: the baseline's 40 fork-owned files, with per-file line and
    function coverage, frozen as the coverage gate's reference and mapped across moves in
    Task 2. Also the per-crate totals.
  - the mapping from each upstream source module with an inline test module to its
    restored file name (Task 2's naming rule);
  - the dispositions known now: SEL-only (including the `system::tenant::test` and
    `system::archiving::test` calls), ungated as today, ignored as today, and needs Docker
    so CI only.

  PR #66's merge (`9a7574203`) changed tested code: it added the `za::external_directory`
  module and the ACL variant's wait. So re-list the libtest identifiers and sub-test calls at
  `9a7574203` before committing. Keep the coverage figures from `5b914ddb5`, and note the SHA
  each figure was measured at. The raw coverage exports stay out of Git.
- [ ] Use the worktree skill/native lifecycle tools to create/reuse an isolated implementation
  worktree from the fork tip. Give Cargo output and disposable server state worktree-specific
  paths; do not share them with another running build.
- [ ] Fetch and verify the exact release tag by URL. Inventory the fork's changes since the
  old base, the deleted upstream paths and the conflict paths. Distinguish a textual conflict
  from a hook whose target moved, and record each hook's replacement and regression owner.
  The known answer to check against is the four files that carried fork hooks and that
  upstream deleted or split:
  - `dav/src/common/propfind.rs` (15 call sites) became `propfind/`;
  - `imap/src/op/fetch.rs` (7) became `op/fetch/`;
  - `imap/src/core/message.rs` (1) became `core/view.rs`;
  - `spam-filter/src/analysis/llm.rs` (1) moved into the SEL-only `common/src/enterprise/ai/`.
- [ ] Freeze the shared adaptations: keyless `Authenticated`, request/connection key ownership,
  account epoch/classification, split calendar views, metadata/extras mapping, pending-ID
  fences and search/snapshot inputs. Use the upstream interfaces named in Tasks 3–6.
- [ ] Count upstream's private-test slots (`#[path = "…/stalwart-tests/…"]`) in the target,
  and record the count: 562 at v1.0.1 (534 test-only, 27 support). List the modules gaining
  slots relative to the previous release, for review in Tasks 3–6.
- [ ] Primary reviews this inventory before assigning writers. Then start the normal merge:
  1. Run `git merge --no-commit`.
  2. Immediately restore `tests/` and `.github/workflows/test.yml` in full from the fork side
     (`git checkout HEAD -- tests/ .github/workflows/test.yml`). Task 8 rewrites the
     workflow.
  3. Record the restored paths. At `9a7574203`, upstream deletes 541 of the 599 files under
     `tests/`: 520 without a conflict and 21 as modify/delete conflicts.
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

**Interfaces:** Retains `TestServerBuilder`, `RegistryEnvStores::insert_stores_from_env` and
`RegistryStore::{new,clone_with_public_url}` as test-only helpers. Adapts the harness's two
cache sites (`tests/src/utils/server.rs:362` and `:592`) to 1.0.1's
`let (cache, swap_rx) = Caches::parse(&mut bootstrap, &core.storage).await`, and hands the
receiver to `SwapTier::start(&inner, swap_rx)` as boot does
(`common/src/manager/boot.rs:158, 225`). The registry default for `cache.swap` is `Disabled`,
and the test server keeps it. Task 6's swap tests configure `LocalFile` under the server's
temporary directory, and `BlobStore` for the filesystem-blob run.

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
  Re-derive the `unit-tests` job's `UNIT_TEST_FEATURES` list for 1.0's workspace now: no
  1.0.1 crate has `test_mode` until this step adds it. Task 7's matrix runs the list.
- [ ] Make `types` and `store` compile first, because `common` depends on both. Port the
  fork's hooks in those crates:
  - the reserved `PrincipalField` 150/151 and vanished collection 250;
  - `ZA_UID_LEASE_PREFIX`, `za_test.rs` and `has_account_search_terms`;
  - R58's carrier type, compiling, with its placement left to Task 6.

  Close this task with `cargo check -p types -p store -p vault`.
- [ ] Restore upstream's unit tests verbatim wherever their subject exists. Move each 0.16.27
  inline test module on Task 1's list into a fork-owned file next to its 1.0 counterpart,
  with a file name unique to its module:
  - for a module file `<stem>.rs`: `#[cfg(test)] #[path = "<stem>_restored_tests.rs"] mod tests;`;
  - for `mod.rs`, `lib.rs` or `main.rs`: `#[path = "restored_tests.rs"]`.

  Notes on this rule:
  - A `#[path]` resolves relative to the directory of the declaring file, so a shared name
    would collide in the 20 directories with two or more such modules.
  - The module keeps the name `tests`, so its libtest identifiers are unchanged and Task 7's
    gate needs no rename table. As a child module it sees the parent's private items, as
    the inline module did.
  - Follow Task 1's mapping, and update it where a subject moved.
  - Subject unchanged or renamed: restore verbatim, adapting only names and call shapes. A
    module whose subject moved follows its subject.
  - Internals replaced: write an equivalent test of the new code only when the test
    exercised public behaviour (a parser, a protocol exchange, a stored format) or the
    module is a fork merge hotspot (CLAUDE.md's lists). Otherwise record "internals
    replaced, no equivalent", with the 1.0 diff hunk.
  - Subject gone: record "subject deleted", with the 1.0 diff that removed it.
  - The SEL-only `scim` and `scim-proto` tests are dispositioned, not moved. The six
    `#[ignore]`d tests stay ignored and compiling. The fork's own unit tests move with their
    modules.
- [ ] Remove the SEL-only test files with a recorded disposition, and remove their sub-test
  calls: `directory/oidc.rs`, the SEL-only `scim/*` files, `system/archiving.rs` and
  `system/tenant.rs`. Write a replacement for `get_token_for_client`, which the dual-licensed
  `directory/issuer.rs` imports from `oidc.rs`. Base it on RFC 6749's token endpoint, not on
  the SEL file.
- [ ] Adapt the two missing registry constructors. Keep the existing fork-owned
  `RegistryEnvStores` trait instead of looking for it in upstream private test support.
- [ ] Port the test boot and cache workers and the queue-drain helpers. Before cleanup or
  scanning, `wait_for_tasks` must wait for the scheduled tasks (`TaskQueueClass::Task`). It
  must also wait for the index queue's entries: the `SearchIndexClass::Queue` keys in
  subspace `SearchQueue` (`y`), with their `QueueIndex` markers and status keys, retries
  included (`store/src/write/{batch,key}.rs`).
- [ ] Update cleanup and store enumeration for 1.0's key spaces. The fork's 24 `SUBSPACE_*`
  constants (`tests/src/utils/cleanup.rs:265-287`) do not exist at 1.0.1. Instead, match
  1.0's closed `Subspace` enum (`store/src/lib.rs:137`) exhaustively, with no catch-all arm,
  so that a subspace added later stops the build.
  - Cover the new subspaces `Immutable` (`c`), `IndexProperty` (`v`), `SearchDocument`
    (`m`), `SearchQueue` (`y`) and `System` (`u`), and `Collection::Metadata` (14).
    `Value` and `Presence` are `Shape` variants, not subspaces.
  - Three bytes changed meaning since 0.16: `m` was `IN_MEMORY_VALUE`, `y` was
    `IN_MEMORY_COUNTER` and `u` was `QUOTA`. Key every rule on the enum, never on the byte.
  - Do not exempt an entire new space to hide leftovers.
  - Keep the vault and pin exemptions only for fixture accounts that still exist.

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

- [ ] Add three new regression cases; none exists at 0.16.27. Each runs against 1.0.1's
  `HttpAuthenticated { in_flight, access_token, epoch }` and `GrantRevocations`
  (`http/src/auth/authenticate.rs:20-24`, `common/src/cache/http_auth.rs:35-38, 101-137`):
  - `scoped_cache_hit_preserves_scope`: a cached ordinary-account token keeps its OAuth
    scope on a cache hit;
  - `revoked_grant_cannot_repopulate_cache`: a loader that started before a grant's
    revocation cannot write the revoked token back;
  - `expired_bearer_cache_entry_is_refused`: an expired cached bearer token is refused.

  Assert also that forbidden key-account bearer and impersonation authentication is still
  refused. Use deterministic epoch pauses for late loaders.
- [ ] Port vault storage, checked decoding, pin records and crypto units to the 1.0 archive
  configuration without legacy readers. Assert setup/password/recovery/app-wrap/pin round
  trips and malformed-record refusal without secret-bearing Debug/errors.
- [ ] Place marker handling in the new basic-auth path. Preserve the primary, app-password
  and TOTP logins, pending-wrap publication, the self-check and the terminal
  Account-before-vault ordering.
- [ ] Place the external-directory refusals and the marker check at their 1.0.1 sites:
  - `za_refuse_directory_login` in `authenticate_basic`'s directory branch;
  - `za_refuse_directory_account` after `directory.authenticate` returns and before
    `assert_account_directory` (`common/src/auth/authentication.rs:246-250, 467`), which
    keeps 0.16's error order, and in the Bearer arm;
  - `merge_directory_secret`'s marker check in `synchronize_account`
    (`common/src/cache/directory.rs:30`).
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

**Check:** Source review accounts for every authentication entrypoint. `cargo check -p common`
passes. The existing and added cases run at Task 7; successful compilation alone does not
establish C1–C4.

## Task 4: Port calendar sealing, sharing, scheduling and DAV together

**Files:**
- `groupware/src/calendar/{mod,storage}.rs`, `seal/`, `sharing/`, `drain.rs`, `fanout.rs`,
  `delegation.rs`, `pinning.rs`, `sequence.rs`;
- 1.0.1's `calendar/{privacy,hidden,participants,overlap,notification}.rs`;
- scheduling;
- the DAV handlers, including `dav/src/common/propfind/` (which replaced `propfind.rs` and
  its 15 fork call sites), `dav/src/common/dead/` and `dav/src/calendar/{view,filter}.rs`;
- `common/src/storage/metadata/`;
- `http/src/api/shares.rs`, the contacts redelivery integration, and the relevant common
  storage builders.

Tests: `tests/src/za/{dav_seal,pairs,fanout,drain,drain_limits,pinning,reply,sharing,
sharing_move_atomic,sharing_races,sharing_review,sharing_tombstones}.rs`, seal/sharing units.

**Interfaces:** Adapt existing seal/domain/context helpers to paired `CalendarEvent` and
`CalendarEventContent`. Target `CalendarEvent::{insert,update_full,update_meta}` and
`GroupwareWrite::full(meta, content, ctx)` / `SplitUpdate::full`; preserve
`ZaCalendarContext::assert_in(&mut BatchBuilder)` authority at the effective transaction.

- [ ] Add `split_metadata_only_update_keeps_sealed_content`: update an allowed metadata
  property, read back as owner/grantee, and scan both rows/indexes for protected canaries.
  Preserve the stored-version identity of read-only views.
- [ ] Port the sealed tree, extras and collection codecs. Seal and sanitize before the
  builders compute derived values, and never index an unsealed read view.
- [ ] Seal key-account dead properties in upstream's metadata store (design: "Dead
  properties and object metadata"). Do this in the common write and read path of
  `common/src/storage/metadata/`, which `dav/src/common/dead/` uses. Do not add a fork path.
  - **Sealed unit:** for a key account's calendars and events, the per-document container
    of a `MetadataClass::Shared` or `Private{viewer}` entry, names and values together,
    sealed under the calendar's collection key. A fork payload version byte marks it.
  - **Not sealed:** the `Viewer{viewer}` and `Owner{owner}` records are upstream's 12-byte
    `ViewerState` counters and stay in upstream's format. A key account's address books,
    contacts and files keep their containers in the clear, as at 0.16.
  - **Keys** hold only ids (`store/src/write/key.rs:589-608`), so nothing about a property's
    name is exposed.
  - **Tests:** add round trips for custom XML properties (including a sensitive namespace
    and name), colour, order and a corrupted sealed container, as owner and as grantee.
    Add a check that `Viewer` and `Owner` counters still decode and merge after sealed
    writes. Fix the payload format in this checkpoint's reviewed interface sheet.
- [ ] Per-user event data: keep the 0.16 rule (calendar spec, table near line 717). These
  preferences are JMAP-only, and JMAP calendars are refused for key accounts. Assert that a
  key account's `CalendarEventContent.preferences` stays empty through DAV PUT, COPY, MOVE,
  scheduling and the drain. No DAV path writes event preferences at 1.0.1: the only DAV
  `preferences_mut` is the calendar-level `mkcol.rs:114`, and `copy_move.rs` retains only
  the caller's own entries. 1.0.1 reshaped the struct to
  `{ account_id, updated, instances: Vec<EventUserData> }`.
- [ ] Give each of 1.0.1's viewer-side readers a disposition:
  - `privacy.rs` (`EventPrivacy::check_access`, `from_flags`), `hidden.rs`,
    `participants.rs`, `overlap.rs`, and `notification.rs` (`visible_notifications`);
  - `dav/calendar/{view,filter}.rs`;
  - `alerts.rs`'s `DefaultAlertsResolver`, run by DAV GET and COPY (`dav/src/calendar/get.rs:180`,
    `copy_move.rs:682, 726, 987, 1032`).

  Each runs on the unsealed view with keys and never in keyless code.
- [ ] Keep `CLASS` sealed. For a key account, never persist `EVENT_PRIVATE` or
  `EVENT_SECRET` (`calendar/mod.rs:149-150`) in the sanitized metadata row: they encode
  `CLASS`. Derive `EventPrivacy` on the keyed, unsealed view before upstream's filters run.
  Add two tests:
  - a grantee-side test of `CLASS:PRIVATE` and `CLASS:CONFIDENTIAL` on a shared sealed
    calendar;
  - a store scan showing that neither flag is set on any stored row of the key account.
- [ ] Port all physical event/link assertions, assigned parent/document identity, conversion
  chunks, rollback/reconversion, ring cache identity and fresh own-id grant admission.
  Reuse the existing deterministic races instead of replacing assertions with cache hints.
- [ ] Port the fan-out, the drain, pins, UID leases, SEQUENCE and delegated scheduling.
  `itip_merge_changes` now returns `Result<(), ItipError>` (`scheduling/inbound.rs:705-709`);
  the drain already passes it the incoming tree. Propagate that result through the drain's
  `merge` and `import`, and re-read both against 1.0.1's `itip_ingest` and
  `commit_itip_merge`, which they mirror by hand. Add
  `incoming_timezone_update_survives_seal` for a moved recurring occurrence that introduces
  a TZID.
- [ ] Adapt DAV GET/PUT/REPORT/PROPFIND/PROPPATCH/ACL/COPY/MOVE/delete and sharing endpoints.
  Keep owner-only inbox access, bounded retries, shared-parent constraints, sync boundaries,
  stable-id revoke and quiet catch-all behavior.
- [ ] Extend the lifecycle cases to cover the new grantee-cleanup and share-rebuild tasks:
  - `cleanup_cannot_restore_revoked_ring` proves revocation stays effective after
    background work, a cache refresh and a later grant;
  - `revoke_cleanup_does_not_rewrite_sealed_event` proves that after a grantee loses
    access, a key-account owner's events keep their content archive bytes and ETags, and no
    record holds plaintext. This is because `ShareRevokeCleanup` skips key-account owners
    (Task 6).

  Both join the mutation sample. Task 6 owns the workers' final integration.

**Check:** Every old calendar mutation maps to a full or metadata-only fenced 1.0 write, and
every dead-property write lands sealed. Sharing's differential oracle still permits only its
recorded limits. `cargo check -p groupware -p dav` passes. Run the cases at Task 7.

## Task 5: Port sealed mail, grouped ingestion and all readers

**Files:**
- `email/src/message/{za,metadata,ingest}`, including `ingest/group.rs`;
- the mail body, section, upload, copy, submission and trace paths in HTTP, JMAP, IMAP,
  POP3, SMTP and the spam filter;
- `imap/src/op/fetch/`, which replaced `fetch.rs` and its 7 fork call sites;
- `imap/src/core/view.rs`, which replaced `core/message.rs`.

`spam-filter/src/analysis/llm.rs`'s one hook moved into the SEL-only
`common/src/enterprise/ai/` and is recorded as a disposition, not ported. SMTP delivery
still calls the single-message `email_ingest`.
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

**Check:** Every delivery, import, append, copy, upload and read route has a named sealing,
unsealing or refusal boundary. No single-message-only guard is left around a grouped bypass.
`cargo check -p email -p imap -p jmap -p smtp -p pop3` passes.

## Task 6: Port search, snapshots and new metadata surfaces

**Files:**
- `services/src/index_queue/{document,index,reindex,file,manager}.rs`;
- `task_manager/tasks/{grantee_cleanup,rebuild_shares}.rs`;
- `store/src/{search,write}`, including `search/index.rs` (`account_batch`);
- `common/src/auth/vault.rs` (`za_has_tasks`);
- `common/src/cache/{large,swap}`;
- `jmap/src/api/metadata/`, JMAP handlers and the session capability mapping;
- `imap/src/op/metadata/` and capabilities.

Tests: `tests/src/za/{index_task,leak,gating,disabled}.rs`, `mail/{search,fence,traces}.rs`;
proposed `tests/src/za/cache_swap.rs`.

**Interfaces:**
- Retain C8 through the new `IndexDocument` builders, the queue and reindex, and
  `BatchBuilder::queue_document_{index,unindex}`.
- The provisioning fence's anchors are:
  - the account's `SearchIndexClass::Queue` entries and their status keys (for R54);
  - `account_batch`'s write (for R58).
- Snapshot writers and readers consume stored, sanitized views only.

- [ ] Add `reindex_key_account_stays_visible_fields_only` and
  `attachment_indexer_never_reads_sealed_blob`. Verify arrival time/size survive while
  protected terms, thread/header material and extracted attachment text do not persist.
- [ ] Retarget the provisioning fence (design: "Indexing, metadata and cache snapshots").
  - **R54:** `Server::za_has_tasks` scans the account's `SearchIndexClass::Queue` entries
    (`id_prefix == account_id`) for every `SearchIndex`, plus the partition's status key.
    It keeps its exhaustive match over `Task`, which now classifies the new types:
    - `ShareRevokeCleanup` counts for its `account_id`, the owner. The record holds only the
      owner (`registry/src/schema/structs.rs:6120-6127`), and the grantee-side writes are
      container counters, not content.
    - `DmarcReportWindow` and `TlsReportWindow` are excluded.

    Keep R54's position in the fence order.
  - **No terminal failure:** a failing partition retries with backoff capped at 30 minutes
    and never fails terminally (`index_queue/manager.rs:316-359`). So a queue entry blocks
    provisioning until it drains. The operator's remedy for a stuck partition is
    `x:IndexQueueStatus/set` (`jmap/src/registry/mapping/index_queue.rs`); Task 9 replaces
    CLAUDE.md's failed-task remedy for index work with it. Add a test: a partition held in
    retry blocks setup with the existing 409, and clearing it through
    `x:IndexQueueStatus/set` lets setup proceed.
  - **R58:** the vault-absence assertion goes on `account_batch`'s write
    (`store/src/search/index.rs`), the batch the worker commits. It does not go on the queue
    entry, which the producer writes. The worker (`index_queue/index.rs`) attaches it through
    the store-side carrier field (Task 2) on the documents it passes to
    `index_account_documents`, and `account_batch` places it in the batch, as at 0.16. The
    reason is that `store` cannot call into `common`.
  - **R62 and R63:** `account_batch` commits each pass in one write with no commit point
    (`store/src/search/index.rs:299-341`). Both take the "subject deleted" disposition with
    that evidence. Retire their tests, or flip them where the scenario still applies.
  - Assert, with pauses, that provisioning or deleting the account mid-pass leaves no orphan
    plaintext or index and no endless retry.
- [ ] Confirm the index queue is ordered, and adopt it. Upstream's source shows that a
  document's index and un-index entries share the `(index, account)` partition and run in
  `created_at` order, and that the worker skips an index entry whose row is gone
  (`services/src/index_queue/index.rs:308-392`). Reproduce the 0.16 interleaving with a
  pause hook: an un-index queued before the import's index write runs. If nothing is
  orphaned:
  - record the evidence;
  - flip the in-flight index tests in `za::mail::fence` to expect no late write;
  - in `tests/src/jmap/za_variants.rs`, remove only the `wait_for_tasks` that PR #66 added,
    the one under the comment "An unindex queued before the import's index write can orphan
    the document". The variant's other three waits stay, because `TestServer::cleanup` does
    not wait for tasks itself.

  If the reproduction does orphan a document, stop and report before adding any guard.
- [ ] The un-index path: `delete_email_metadata` reads the stored row's `raw_headers`
  (`services/src/index_queue/index.rs:834-855`), so it must recognize a key account's stub
  by its format byte. Add a destroyed-stub un-index test to `index_task.rs`.
- [ ] The share-revocation cleanup skips key-account owners. In `grantee_cleanup`'s
  `stage_event`, `stage_calendar` and `stage_notification`, return without staging when the
  owner is a key account: a one-line call into a fork-owned check. The fork's keyed revoke
  handles the grantee's entries.
  - `rebuild_shares` runs unchanged for key accounts. It writes only the `acl_grant` and
    `acl_revoke` index keys derived from object ACLs (`rebuild_shares.rs:95-121`), and keeps
    the grantee index in line with the ACLs the fork's revoke maintains.
    `cleanup_cannot_restore_revoked_ring` covers it.
  - Integrate both tasks with the Task 4 resolver and ring authority, and make their
    completion observable to the Task 2 drain helper.
- [ ] Add `swap_roundtrip_contains_no_protected_canary` and
  `swap_restore_after_revoke_cannot_restore_access`. Exercise file and blob snapshots with
  cache eviction, fresh loads, provisioning/revocation races and process-independent decode.
  Scan decoded frames/archives as well as raw files; compression is not a privacy control.
- [ ] Trace all snapshot population, serialization, load and invalidation inputs. Prevent
  unsealed response views/session keys from entering persistent caches. Apply guards in
  shared backend-independent code; keep Redis/cluster runtime qualification out of scope.
- [ ] Refuse IMAP METADATA and JMAP object metadata for key accounts.
  - **IMAP:** refuse per command: `GETMETADATA` and `SETMETADATA` (`imap/src/op/metadata/`),
    and `LIST … RETURN (METADATA …)` (`imap/src/op/list.rs:181-182`).
  - **JMAP:** object metadata is a property layer inside the existing `*/get`, `*/set`,
    `*/query` and `*/copy` methods, not a new method, so the dispatcher gate cannot catch
    it. Place the refusal in `jmap/src/api/metadata/{write,apply,get,preload,copy}.rs`.
  - **Tests:** `Mailbox/set` with metadata properties on a key account is refused, while the
    same call without them succeeds. Make capability and refusal behaviour explicit.
  - Plain metadata operations stay upstream's. The presence flags name metadata kinds, not
    values, and stay upstream's.
- [ ] Extend the leak enumeration to 1.0.1's new spaces, each with a stated rule for what a
  key account may hold there in the clear, and a planted-violation control:
  - `SearchQueue` (`y`): queue entries carry ids only;
  - `SearchDocument` (`m`): arrival time and size only;
  - `Immutable` (`c`), `IndexProperty` (`v`) and `System` (`u`): a rule for each from the
    source, before the scanner treats them as clean;
  - `Collection::Metadata`: every container of a key account's calendar or event document
    is sealed, and the 12-byte `Viewer`/`Owner` counters are permitted. This is not a name
    check, because names are never in keys.

  Key the rules on the `Subspace` enum, not on bytes, since `m`, `y` and `u` changed
  meaning.

  Use ordinary-account positive controls, and keep the existing scanner controls and
  mutation coverage.

**Check:** No new background path obtains session keys, indexes decrypted content, or
rewrites a key account's sealed calendar data. Every new persistent surface appears in the
leak inventory or has a source-backed exclusion. `cargo check -p services -p store` passes.

## Task 7: Reach the integrated compile and coverage gate

**Files:** All the outputs of Tasks 2–6; `tests/qualification/baseline-0.16.27.json` (Task
1); proposed `tests/qualification/dispositions.json`, `scripts/tests/check_test_inventory.py`
and `scripts/tests/check_coverage.py`; the expectation ledger in the outcome note.

**Interfaces:**
- **The inventory tool** accepts `--baseline PATH --dispositions PATH --listing PATH
  --ignored-listing PATH --executed PATH`. The baseline carries each test's 0.16.27 status:
  run, ignored or not selected. The tool exits nonzero when:
  - a required identifier is missing or unselected;
  - a test that ran at 0.16.27 is now ignored;
  - a required exact selector selects nothing;
  - a status change has no disposition;
  - a required sub-test call did not execute in a mode where it executed at 0.16.27.

  A test ignored at 0.16.27 that is still ignored, and still listed, passes.
  - Capture both `-- --list` and `-- --ignored --list` for each required package and
    feature combination; the ordinary listing alone does not establish that a test will run.
  - **"Executed"** means both of these have positive counts in that step's export:
    - the sub-test's body record: for an `async fn`, the async body's own record, whose
      first region starts after the signature; for a plain `fn`, the function's own record;
    - that body's final region: the region that ends at the body's closing brace.

    The outer record of an `async fn` only constructs the future, and never counts; neither
    do declared modules. Compare against the baseline recomputed under the same rule
    (Task 1). Sub-tests that Task 1 reported as completing without reaching their final
    region get an explicit completion marker, and the tool checks that marker instead.
- **The coverage tool** compares the candidate's per-file export with the baseline. It fails
  if line coverage of the fork-owned cohort falls below 96.4%.
  - The cohort is the baseline's 40 files, mapped across moves, plus fork runtime code added
    during the port, listed as it is added.
  - Restored upstream tests (`*restored_tests.rs`) and the `tests` crate are reported as
    separate groups, outside the cohort and outside product code.
  - It reports per-crate deltas for upstream code.
- **Every disposition** names its kind (SEL-only, subject deleted, internals replaced with or
  without an equivalent, ungated as today, ignored as today) and its evidence.

- [ ] Primary inspects all conflict resolutions and dispositions, including paths that
  auto-merged. Search for unresolved conflict markers, disabled tests, placeholder returns,
  lost test hooks and accidentally restored private test dependencies.
- [ ] Compile the shipping product with `cargo check -p stalwart --no-default-features
  --features rocks` and the harness with `cargo test -p tests --no-run`; resolve remaining
  cross-crate errors without reducing contracts. Formatting must also pass.
- [ ] Compare the libtest listings with the committed baseline. Every renamed, replaced or
  removed test needs a named equivalent or an approved disposition with evidence. Fork
  invariants and za sub-tests cannot be deleted as obsolete upstream APIs.
- [ ] Implement the inventory and coverage gates, with self-tests. The unchanged baseline
  must pass, including its two synchronous sub-tests (`imap::body_structure::test`,
  `webdav::cal_itip::test`). Each of these, made in a disposable change, must fail a gate:
  - a missing test;
  - a newly ignored test that ran at 0.16.27;
  - an empty selection;
  - an omitted za sub-test call, and an omitted synchronous sub-test call;
  - a za sub-test whose future is built but never awaited (`let _ = module::test(&mut test);`);
  - a sub-test that returns early before its assertions;
  - a dropped restored unit test module;
  - a coverage drop in the fork-owned cohort that restored test files would otherwise
    offset.

  Show the failure each time, then restore and rerun.
- [ ] Diff every ported test file against `5b914ddb5` for added `return`, `cfg!`, `#[cfg]`
  and `#[ignore]`. Each addition needs an expectation-ledger entry naming the 1.0 change
  behind it.
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
| Workspace units | The `unit-tests` job's command from PR #66, with the feature list Task 2 re-derived: `cargo test --workspace --exclude tests --exclude scim --exclude scim-proto --lib --bins --features "$UNIT_TEST_FEATURES"`. It includes the restored upstream unit tests, and its listing equals the baseline's minus dispositions, the six ignored tests listed as ignored. |
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

  Then add two jobs: the inventory and coverage gates, and the fresh OSS restart. Upload
  sanitized failure receipts. Public CI cannot claim that upstream's private suites ran.
  The external protocol suites (Dovecot imaptest, Fastmail JMAP-TestSuite, Apple
  CalDAVTester) are deferred to a follow-up plan, by the design's scope exclusions.
- [ ] Optionally, if the Bearer arm of the external-directory refusal is to be covered: add a
  CI-only key-account case against a Keycloak container to the directory step.

**Check:** Fresh OSS creation and the persisted-data restart pass separately from the Rust
harness, which is built with `enterprise`. The independent client suite keeps its existing
coverage.

## Task 9: Final review, provenance and release qualification record

**Files:** `fork-release.toml`, `crates/main/Cargo.toml`, lock entry;
`crates/common/{build.rs,build_support/release.rs,src/version.rs}`;
`scripts/tests/test_release_metadata.py`, `scripts/release/`;
`.github/workflows/product-build.yml`; `CHANGELOG-fork.md`, `docs/releases.md`, `CLAUDE.md`,
`README.md`, developer guide and the upgrade outcome record.

**Interfaces:** Preserve the existing binary identity/provenance verifier and immutable
fork release contract. The preview is `0.2.0-beta.1` (Jay confirmed on 2026-10-30). After
full integration, the upstream metadata becomes the pinned 1.0.1 tag and SHA. CLAUDE.md's
package selectors become `-p http@1.0.1`. Its provisioning remedy for index work changes
from destroying failed tasks to clearing a stuck partition with `x:IndexQueueStatus/set`;
the failed-task remedy stays for the task types that still exist.

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
  - the python-caldav result;
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

## Review dispositions (revision 3)

Findings of the [Fable review](../specs/2026-10-27-stalwart-1-upgrade-design-review.md) (F) and
the [Codex review](2026-10-27-stalwart-1-upgrade-codex-review-2026-10-30.md) (C) of revision 2.

| Finding | Disposition |
|---|---|
| C1: an async constructor counts as executed | Adopted. "Executed" needs the body record and its final region; Task 1 recomputes the baseline; Task 7 adds the unawaited-future and early-return self-tests. |
| C2: already-ignored tests | Adopted. The baseline carries each test's status; the gate fails only on a test newly ignored after it ran; disposition "ignored as today". |
| F-H1: dead properties in `Collection::Metadata` | Adopted (Jay's decision: seal in upstream's store). Design "Dead properties and object metadata"; Task 4. |
| F-H2: `ShareRevokeCleanup` rewrites sealed events | Adopted (Jay's decision: skip key-account owners). Design "Calendar and sharing writes"; Tasks 4 and 6. |
| F-H3: R54 written against removed task types | Adopted. The fence scans `SearchIndexClass::Queue`, and the new task types are classified; "ordered" is adopted, per the source (Jay's decision). Task 6; `wait_for_tasks` in Task 2. |
| F-H4: restored tests renamed | Adopted: `#[path = "restored_tests.rs"] mod tests;`, refined in revision 4 to a per-module file name (F3-M1). Task 2. |
| F-M1: nonexistent regression tests | Adopted: "add", with what each asserts. Task 3. |
| F-M2: one compile gate | Adopted: crate-level `cargo check` at the end of Tasks 3–6. |
| F-M3: hooks in files upstream deleted | Adopted: named in Task 1, the ownership table and Tasks 4–5. |
| F-M4: JMAP object metadata is not a method | Adopted: refusal in `jmap/src/api/metadata/`; IMAP per command. Task 6. |
| F-M5: R58/R62/R63 anchors gone | Adopted: R58 on `account_batch`; R62/R63 to be confirmed as "subject deleted". Task 6. |
| F-M6: un-index reads stub headers | Adopted: destroyed-stub un-index test. Task 6. |
| F-M7: stale `itip_merge_changes` step | Adopted: propagate the new `ItipError` result. Task 4. |
| F-M8: reshaped `EventPreferences` | Adopted (Jay's decision: keep the 0.16 rule, gated and asserted empty; stop and report if DAV writes them). Task 4. |
| F-M9: new viewer-side filters | Adopted: dispositions and a `CLASS:PRIVATE` grantee test. Task 4. |
| F-M10: new key spaces | Adopted: named in Task 2 (cleanup) and Task 6 (leak rules). |
| F-M11: external suites unscoped | Adopted (Jay's decision: deferred to a follow-up plan). Design scope exclusions; Task 8. |
| F-M12: restore cost and stop rule | Adopted (Jay's decision: verbatim where the subject exists; equivalents for public behaviour or hotspots; upstream coverage reported). Task 2. |
| F-L1: `test.yml` modify/delete | Adopted: restored with `tests/` in Task 1. |
| F-L2: slot classification | Adopted: 534 test-only, 27 support. |
| F-L3: 520 of 598 at the wrong SHA | Adopted: 541 of 599 at `9a7574203`, 520 silent and 21 conflicts. |
| F-L4: swap tier | Adopted (Jay's decision: started by default). Task 2 interfaces. |
| F-L5: feature list re-derived late | Adopted: moved to Task 2. |
| F-L6: SEL sub-tests of `system_tests` | Adopted: dispositioned in Task 1. |
| F-L7: directory sites | Adopted: named in Task 3. |
| F-L8: fork-owned set | Adopted: relative to v1.0.1. Task 7. |
| F-L9: early returns pass every gate | Adopted: an early-return self-test, and a diff check requiring ledger entries. Task 7. |
| F-L10: CLAUDE.md's http version | Not a defect: CLAUDE.md already reads `0.16.27` at `9a7574203`; Task 9 moves it to `1.0.1`. |
| Version | `0.2.0-beta.1` (Jay confirmed). |

## Review dispositions (revision 4)

Findings of the [Fable re-review](../specs/2026-10-27-stalwart-1-upgrade-design-review-r3.md)
(F3) and of the Codex review's "Revision 3 re-review" section (C3), both of revision 3.

| Finding | Disposition |
|---|---|
| F3-H1, C3-1, C3-3: the dead-property store described wrongly | Adopted, by parity of construction: seal the per-document `Shared`/`Private` container, names and values together, under the collection key; `Viewer`/`Owner` counters stay upstream's; other object kinds stay in the clear, as at 0.16; the leak rule checks sealed containers, not names (Jay's answers to F3 questions 1 and 2). Design "Dead properties and object metadata"; Tasks 4 and 6. |
| C3-2: `CLASS` persisted through privacy flags | Adopted: `EVENT_PRIVATE`/`EVENT_SECRET` never persisted for key accounts, derived on the keyed view; C6 amended. Task 4. |
| F3-M1, C3-4: restored-test paths collide | Adopted: `<stem>_restored_tests.rs` per module (`restored_tests.rs` for `mod.rs`, `lib.rs`, `main.rs`); mapping in Task 1. Task 2. |
| C3-5: restored tests inflate fork coverage | Adopted: the frozen 40-file cohort, mapped across moves, plus listed new fork code; restored tests and the `tests` crate reported separately. Tasks 1 and 7. |
| F3-M6, C3-6: "final region" undefined; synchronous sub-tests | Adopted: the body record is defined for `async fn` and plain `fn`, the final region is the closing-brace region, sub-tests that complete without reaching it get a completion marker, and there are sync self-tests. Tasks 1 and 7. |
| F3-M2: `common` needs `store` and `types` | Adopted: Task 2 ports their fork hooks and closes with `cargo check -p types -p store -p vault`. |
| F3-M3: R54's grantee count; no terminal failure | Adopted: `ShareRevokeCleanup` counts for its owner only; a stuck partition is cleared with `x:IndexQueueStatus/set` (Jay's answer to F3 question 4), with a test; CLAUDE.md updated in Task 9. Task 6. |
| F3-M4: `Value`/`Presence` are not subspaces | Adopted: the new subspaces are listed and the changed bytes `m`, `y`, `u` noted, with rules keyed on the enum. Tasks 2 and 6. |
| F3-M5: the swap tier's default is `Disabled` | Adopted: the harness keeps `Disabled`; the swap tests configure `LocalFile` and `BlobStore` (Jay's answer to F3 question 3). Task 2. |
| F3-L1: IMAP `LIST … RETURN (METADATA)` | Adopted. Task 6. |
| F3-L2: directory refusal order | Adopted: after `directory.authenticate`, before `assert_account_directory`. Task 3. |
| F3-L3: stale external-suites line | Adopted: replaced with the python-caldav result. Task 9. |
| F3-L4: which ACL wait to remove | Adopted: PR #66's, identified by its comment; the other three stay. Task 6. |
| F3-L5: `rebuild_shares` | Adopted: it runs unchanged for key accounts (ACL index keys only). Task 6. |
| F3-L6: R58's layering | Adopted: attached by the worker through a store-side field, and placed by `account_batch`. Task 6. |
| F3-L7: `DefaultAlertsResolver` | Adopted: added to the viewer-side readers. Task 4. |
| F3-L8: slot wording | Adopted. |
| F-M8's stop clause | Removed: no DAV path writes event preferences at 1.0.1 (F3 verdict). |
| R62 and R63 | Settled as "subject deleted": `account_batch` has no commit point (F3 verdict on F-M5). |
