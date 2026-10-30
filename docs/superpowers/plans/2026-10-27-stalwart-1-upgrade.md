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
revision 5. Its C1–C11 contracts and the earlier approved feature designs remain binding,
with the calendar spec at revision 8 (field-policy version 2).

**Status:** Revision 5, proposed, 2026-10-30. It adopts
[Fable's check of revision 4](../specs/2026-10-27-stalwart-1-upgrade-design-review-r4.md)
and Jay's answers to its two questions; the Codex review's "Revision 4 re-review" found no
new finding. The findings of
[Fable's check of revision 5](../specs/2026-10-27-stalwart-1-upgrade-design-review-r5.md),
none critical or high, are fixed in place, and the reviews stop there. Revision 4 adopted
both re-reviews of revision 3
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
| Calendars | `groupware/src/calendar/{seal,sharing,drain,fanout,delegation,pinning,sequence}`, the new `privacy`, `hidden`, `participants`, `overlap` and `notification` modules, scheduling, DAV (including `dav/src/common/propfind/`, which replaced `propfind.rs`, and `dav/src/common/dead/`), HTTP shares | Task 4 |
| Mail | `email/src/message/{za,metadata,ingest}`, mail readers/writers in IMAP/POP3/JMAP/SMTP and tracing; `imap/src/op/fetch/` (replaced `fetch.rs`) and `imap/src/core/view.rs` (replaced `core/message.rs`); `spam-filter/src/analysis/llm.rs`'s hook, now in the SEL-only `spam-filter/src/analysis/ai/`, which calls `common/src/enterprise/ai/` (a disposition, not a port) | Task 5 |
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
- Task 5: `-p email -p imap -p pop3 -p smtp -p spam-filter`;
- Task 6: `-p services -p store -p jmap -p http@1.0.1 -p managesieve`.

`jmap` depends on `services`, and `http` on `jmap`, `dav` and `services`, so they're checked
at Task 6 (Task 1's ruling 21). A file that first fails to compile at a later task's check
goes back to the task that owns it; the later task reports the failure and doesn't patch it.

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
    whether it executed in each step under the strict rule. The rule needs three positive
    counts:
    - the body record: the async body's own record for an `async fn`, never the outer
      constructor record; the function's own record for a plain `fn`;
    - the region that starts the body's last top-level statement, located from the source;
    - the body's final region: the exit region whose start and end both lie on the body's
      closing brace, not the outer region that starts at the opening brace.

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
  - `spam-filter/src/analysis/llm.rs` (1) moved into the SEL-only `spam-filter/src/analysis/ai/`,
    which calls `common/src/enterprise/ai/`.
- [ ] Freeze the shared adaptations: keyless `Authenticated`, request/connection key ownership,
  account epoch/classification, split calendar views, metadata/extras mapping, pending-ID
  fences and search/snapshot inputs. Use the upstream interfaces named in Tasks 3–6.
- [ ] Count upstream's private-test slots (`#[path = "…/stalwart-tests/…"]`) in the target,
  and record the count: 562 at v1.0.1 (535 test-only, 27 support). List the modules gaining
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
and the test server keeps it. The tier is fixed at boot, so `TestServerBuilder` takes the
swap setting as a bootstrap option for Task 6's own swap suite (`cache_swap_tests`), which
boots one server per tier.

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
    would collide in the 19 directories with two or more such modules.
  - The module keeps its own name (`tests` for all but six: five test modules named
    otherwise and the test-support module `serde_impl`, recorded in the baseline's mapping), so its libtest identifiers are unchanged and Task 7's
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
  refused, including an impersonated JMAP WebSocket after a forced token refresh, whose
  `Mailbox/get` must still be refused (Task 1's ruling 11). Use deterministic epoch pauses for late loaders.
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
  Include a PROPPATCH whose dead-property write flips a presence bit, which
  `rewrite_presence` applies to the event's metadata archive through a store-side merge
  (`groupware/src/metadata/mod.rs:40-60`), and scan that row afterwards. Preserve the
  stored-version identity of read-only views.
- [ ] Port the sealed tree, extras and collection codecs. Seal and sanitize before the
  builders compute derived values, and never index an unsealed read view.
- [ ] Seal key-account dead properties in upstream's metadata store (design: "Dead
  properties and object metadata").
  - **Sealed unit:** for a key account's calendars and events, the per-document container
    of a `MetadataClass::Shared` entry, names and values together, sealed under the
    calendar's collection key. DAV writes only `Shared` (`dead/patch.rs:190`), and the other
    writers of `Private` are refused for key accounts, so no `Private` container exists.
  - **Wrapper:** the sealed container stays a valid upstream container, since keyless code
    parses every container's header (`types/src/metadata/view.rs:62-69`, which also checks
    that the header's kinds equal the entries'). Its header carries `MetadataKinds::DAV`,
    and its one entry is a `Dav` entry (`EntryKind::Dav`, `view.rs:111-135`) under a
    fork-reserved namespace and name. The entry's value is a text-only DAV value (no
    attributes or children) whose text is the base64 of the fork's payload version byte
    followed by the ciphertext of the original container; both
    `MetadataView::from_trusted` and the checked XML validator accept it. Record the exact
    layout in this checkpoint's reviewed interface sheet.
  - **Sites:** in `dav/src/common/dead/patch.rs`, `prepare` builds the edit from the stored
    container's view (`patch.rs:132-141`). Unseal the stored container there, before the
    edit is built, and seal `next` before `change.into_write(…)`. Unseal after `.view()` in `dav/src/common/propfind/dead.rs`
    (`dead_properties`, `dead_property_names`) and `dav/src/common/dead/containers.rs`
    (`get`, `stored_entries`). Each site is a one-line call into a fork module.
    `common/src/storage/metadata/` and `store/src/write/metadata.rs` stay upstream's and
    keyless; they have no keys to seal with.
  - **COPY and MOVE:** `DeadContainers::copy` builds the destination's container from the
    source's stored bytes (`dead/containers.rs:135-150`). In `za_copy_move_event`'s path,
    unseal under the source calendar's key and reseal under the destination's before the
    copy is built.
  - **Not sealed:** the `Viewer{viewer}` and `Owner{owner}` records are upstream's 12-byte
    `ViewerState` counters and stay in upstream's format. A key account's address books,
    contacts and files keep their containers in the clear, as at 0.16.
  - **Keys** hold only ids (`store/src/write/key.rs:589-608`), so nothing about a property's
    name is exposed.
  - **Tests:**
    - keyed round trips, as owner and as grantee, for custom XML properties (including a
      sensitive namespace and name), colour, order and a corrupted sealed container;
    - a COPY and a MOVE of an event with dead properties between two calendars of the
      account, then a PROPFIND at the destination;
    - the `Viewer` and `Owner` counters still decode and merge after sealed writes;
    - keyless paths over sealed containers: `MetadataBuf::read`, `container_kinds` and
      `metadata_used_quota`; deleting an event and a calendar that hold sealed containers
      (`container_cleanup_from`); destroying the account (`destroy_owner_metadata`,
      `purge_private_metadata`). Each runs without keys and without error.
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

  Each runs on the unsealed view with keys and never in keyless code, except the privacy
  check: the DAV sites read `EVENT_PRIVATE`/`EVENT_SECRET` from the resource cache, as
  upstream does (next item).
- [ ] Field-policy version 2 (design: "Privacy flags"; calendar spec revision 8, Jay's
  decision): make `CLASS` visible in `seal/policy.rs` on `VEVENT`, `VTODO` and `VJOURNAL`
  when its value is `ICalendarClassification::{Public, Private, Confidential}`. Any other
  value stays sealed, and its parameters follow the level-3 rule. Replace
  `is_visible_property` with a predicate that takes the entry, so the build names each
  caller (at main `seal/tree.rs:366, 482`, `seal/event.rs:435`, `sharing/format.rs:569` and
  `tests/src/za/leak.rs:457`). `POLICY_VERSION` stays 1: it is the format byte of every
  key-wrap envelope and of the associated data, and field policy 2 is a property of the
  fresh store (design: "Version"). Upstream's `refresh_from_content`
  (`calendar/index.rs:247`) then derives the two flags from the stored tree; add no fork
  code that computes or overrides them. Carry `itip_ingest`'s preservation of the stored
  classification (`calendar/itip.rs:854-875`) into the drain's hand-mirrored merge. Update
  the allowlist units (`event_allowlist_matches_spec_section_6` lists `Class` as sealed
  today) and the leak scanner's allowlist. Add these tests:
  - a differential sharee test: the same GET, PROPFIND, REPORT (calendar-query and
    multiget), free-busy, COPY, MOVE, LOCK, PUT, PROPPATCH and DELETE requests by a sharee
    against a key account's shared calendar and an ordinary account's, for events with
    `CLASS:PUBLIC`, `CLASS:PRIVATE` and `CLASS:CONFIDENTIAL`, give the same status codes
    and the same served properties; the owner sees all three in full;
  - a store scan: each stored event's `EVENT_PRIVATE`/`EVENT_SECRET` bits match its `CLASS`
    after a PUT, a PROPPATCH, a COPY, a MOVE, a drain import and merge, and a sharee's
    write, and after a `CLASS` change from PRIVATE to PUBLIC. Include an inbound update
    whose `CLASS` differs from the stored one, through `itip_ingest` and through the
    drain's merge: the classification it keeps and its flags both match an ordinary
    account's;
  - the recorded limit: an event with `CLASS:X-SOMETHING` stores the value sealed, keeps
    both bits clear, and is served to the sharee in full.
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
`spam-filter/src/analysis/ai/` (which calls `common/src/enterprise/ai/`) and is recorded as a disposition, not ported. SMTP delivery
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
`cargo check -p email -p imap -p pop3 -p smtp -p spam-filter` passes (Task 1's ruling 21).

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
proposed `tests/src/za/cache_swap.rs`, a suite function of its own (`cache_swap_tests`),
outside `za_tests`.

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
    provisioning until it drains. `x:IndexQueueStatus/set`
    (`jmap/src/registry/mapping/index_queue.rs:191-240`) resets the partition's retry state
    so that it retries at once; it removes no entry. The remedy is that reset for a
    transient failure, and deleting the offending document (or the account) for a permanent
    one. Task 9 puts this in CLAUDE.md in place of the failed-task remedy for index work.
    Add a test with a fault hook that fails a document's index build once: the partition in
    retry blocks setup with the existing 409; after `x:IndexQueueStatus/set`, the retry
    succeeds, the entry drains and setup proceeds.
  - **R58:** the vault-absence assertion goes on `account_batch`'s write
    (`store/src/search/index.rs`), the batch the worker commits. It does not go on the queue
    entry, which the producer writes. The worker (`index_queue/index.rs`) attaches it through
    the store-side carrier field (Task 2) on the documents it passes to
    `index_account_documents`; the carrier is attached at the builders' plain returns
    (`index_queue/document.rs`, `build_email_document` and `build_calendar_document`), as at
    0.16. The reason is that `store` cannot call into `common`.
    `account_batch` splits a pass into chunks (`store/src/search/term.rs:364-383`), so one
    fork call after `build_batch` places the assertion in every chunk of a pass that holds
    a fenced document (Task 1's ruling 18). Test it with the `FORCE_COMMIT_SPLIT` hook
    (ruling 17), since fixture-sized data never reaches a second chunk.
  - **R62 and R63:** confirm from source (`services/src/index_queue/index.rs:56-104`) that
    one partition worker runs an account's index and un-index entries in order. If it does,
    retire R62's per-chunk guard and R63's confirmation read on that evidence, and retire
    or flip their tests. If it doesn't, rebuild the guard on the last chunk's `Document`
    key. Keep R62's term scans (`has_account_search_terms`) either way (ruling 19).
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
  `swap_restore_after_revoke_cannot_restore_access` to `cache_swap_tests`, a suite function
  of its own: the tier is built once in `Caches::parse` and `cache/reload.rs` does not
  rebuild it, so the za suite's server cannot switch to it (Jay's decision, 2026-10-30). The
  suite boots its own `TestServer` with `cache.swap` set to `LocalFile` under its temporary
  directory, then another set to `BlobStore` with `BLOB_STORE=FileSystem`, and runs both
  cases on each. Exercise file and blob snapshots with
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
  - `Collection::Metadata`: a key account's calendar or event document has no `Private`
    container, each of its `Shared` containers is a sealed wrapper, and the 12-byte
    `Viewer`/`Owner` counters are permitted. This is not a name check, because names are
    never in keys;
  - calendar trees: `CLASS` is permitted only with a standard value (field-policy version 2).

  Key the rules on the `Subspace` enum, not on bytes, since `m`, `y` and `u` changed
  meaning.

  Use ordinary-account positive controls, and keep the existing scanner controls and
  mutation coverage.

**Check:** No new background path obtains session keys, indexes decrypted content, or
rewrites a key account's sealed calendar data. Every new persistent surface appears in the
leak inventory or has a source-backed exclusion. `cargo check -p services -p store -p jmap -p http@1.0.1 -p managesieve` passes (Task 1's
ruling 21).

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
  - a required sub-test call did not execute in a step where it executed at 0.16.27;
  - a `tests/src` function that ran in a step at 0.16.27 did not run there, under the same
    three counts (Task 1's ruling 31), or a loop-tailed body's loop body ran no iteration
    where it had.

  A test ignored at 0.16.27 that is still ignored, and still listed, passes.
  - Capture both `-- --list` and `-- --ignored --list` for each required package and
    feature combination; the ordinary listing alone does not establish that a test will run.
  - **"Executed"** means all three of these have positive counts in that step's export:
    - the sub-test's body record: for an `async fn`, the async body's own record, whose
      first region starts after the signature; for a plain `fn`, the function's own record;
    - the region that starts the body's last top-level statement, located from the source
      by the method recorded in the baseline's `strict_rule.method`. rustc routes every
      `return` through the closing-brace block, so without this count an early return
      passes the gate;
    - that body's final region: the exit region whose start and end both lie on the
      body's closing brace. The body's outer region also ends there but starts at the
      opening brace and counts entries, so it does not qualify.

    The outer record of an `async fn` only constructs the future, and never counts; neither
    do declared modules. Compare against the baseline recomputed under the same rule
    (Task 1). Sub-tests that Task 1 reported as completing without reaching their final
    region get an explicit completion marker, and the tool checks that marker instead.
- **The coverage tool** compares the candidate's per-file export with the baseline. It fails
  if line coverage of the fork-owned cohort falls below the baseline's exact fraction,
  17,821 of 18,490 lines (96.38%), or if its runtime-only figure (the same files without
  their inline `#[cfg(test)]` modules and the fork's test-support files) falls below
  10,646 of 11,246 lines (94.66%) (Task 1's ruling 31).
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
  and `#[ignore]`, and for a removed `.await`, an added `let _ = <call>` and an added
  `drop(<call>)` of a future. Each addition needs an expectation-ledger entry naming the 1.0 change
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
| ZA cache swap | `cargo test -p tests za::cache_swap::cache_swap_tests -- --exact --nocapture` (its own `LocalFile` and `BlobStore` servers) |
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
    steps, the vault `test_mode` step and the leak-scanner step, plus a new step for
    `cache_swap_tests`;
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
from destroying failed tasks to resetting a partition's retry state with
`x:IndexQueueStatus/set` for a transient failure, and deleting the offending document or
the account for a permanent one; the failed-task remedy stays for the task types that still
exist. CLAUDE.md's zero-access section also records field-policy version 2 (`CLASS`
visible with a standard value) and the `cache_swap_tests` suite. The key-account web page's
claims (`~/Development/circulo-account`, outside this repository) need the same `CLASS`
change; note it for Jay rather than editing that repository.

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
| F-L2: slot classification | Adopted: 534 test-only, 27 support (corrected to 535 by Task 1). |
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
| R62 and R63 | Settled as "subject deleted": `account_batch` has no commit point (F3 verdict on F-M5). Superseded: Task 1 found the split; see "Amended during execution" and ruling 19. |

## Review dispositions (revision 5)

Findings of [Fable's check of revision 4](../specs/2026-10-27-stalwart-1-upgrade-design-review-r4.md)
(R4). The Codex review's "Revision 4 re-review" confirmed no new finding; its note on
`x:IndexQueueStatus/set` is R4-L1. Fable's three partly resolved revision 3 verdicts are
closed by the rows named: R3-H1 by R4-M1 to M3, R3-M5 by R4-M4, C3-2 by R4-H1.

| Finding | Disposition |
|---|---|
| R4-H1: the privacy flags are read from the keyless resource cache, not a keyed view | Adopted, as a visibility-policy change (Jay's answer to question 1: sharers can hide events; the operator may learn each event's classification). Calendar spec revision 8 adds field-policy version 2: `CLASS` is visible with a standard value, and upstream derives and reads the flags unchanged. Non-standard values stay sealed (recorded limit). C6 amended. Design "Privacy flags"; Task 4 (differential sharee test, flag scan); Task 6 (leak rule); Task 9 (CLAUDE.md, the account page's claims). |
| R4-M1: the sealed container must parse as an upstream container | Adopted: a valid container with `MetadataKinds::DAV` in its header and one `Dav` entry under a fork-reserved name, the version byte and ciphertext in its value. Design "Dead properties"; Task 4. |
| R4-M2: the seal sites are DAV's consumers | Adopted: seal in `dead/patch.rs` before `into_write`, unseal in `propfind/dead.rs` and `dead/containers.rs`; `common/src/storage/metadata/` stays upstream's and leaves Task 4's files. Design; Task 4. |
| R4-M3: COPY/MOVE copies container bytes | Adopted: unseal under the source calendar's key and reseal under the destination's in `za_copy_move_event`'s path, with COPY and MOVE round trips. Design; Task 4. |
| R4-M4: the swap tier is fixed at boot | Adopted (Jay's answer to question 2): `cache_swap_tests` is its own suite function, one server per tier, with its own CI step. Design "Cache snapshots"; Tasks 2, 6 and 8. |
| R4-L1: `x:IndexQueueStatus/set` resets, it does not remove | Adopted: reset for a transient failure; delete the document or the account for a permanent one; the test uses a fail-once hook. Design; Tasks 6 and 9. |
| R4-L2: C6 forbade the alarm-flag exception | Adopted: C6 permits flags derived from visible properties, the alarm flags and, from policy version 2, the classification flags. |
| R4-L3: `Private` cannot exist for key accounts | Adopted: stated, the leak rule requires no `Private` container and every `Shared` one sealed, and Email joins the refused-only kinds. Design; Tasks 4 and 6. |
| R4-L4: the presence rewrite | Adopted: `split_metadata_only_update_keeps_sealed_content` includes a PROPPATCH that flips a presence bit. Task 4. |
| R4-L5: "region that ends at the closing brace" | Adopted: the exit region whose start and end lie on the brace, not the outer region. Design "Gates"; Tasks 1 and 7. |
| R4-L6: keyless paths untested | Adopted: keyless reads, deletes, account destruction and quota over sealed containers. Task 4. |
| Stopping rule | One high finding (R4-H1) was a policy choice, now made. By Jay's answer, one narrow check covers the lines revision 5 changes, then reviews stop. |

### Fixed in place after the revision 5 check

Findings of [Fable's check of revision 5](../specs/2026-10-27-stalwart-1-upgrade-design-review-r5.md)
(R5): none critical or high, so by the agreed rule they are fixed here without another
review. Its one partly resolved verdict, R4-M2, is closed by R5-M4.

| Finding | Disposition |
|---|---|
| R5-M1: `POLICY_VERSION` is the envelope and associated-data byte | Adopted, option "separate": `POLICY_VERSION` stays 1, so no envelope, ring, inbox-item or mail format changes; field policy 2 is a property of the fresh 1.0 store. Design "Version"; calendar spec sections 6 and 7; Task 4. |
| R5-M2: five name-only allowlist callers | Adopted: an entry-aware predicate replaces `is_visible_property`, so the build names every caller; the five at main are listed. Design; Task 4. |
| R5-M3: calendar spec sections 7, 10, 11 and 12 | Adopted: sections 6, 11 and 12 (invariant 3) name the value rule; sections 7 and 10 stay true, since the recorded byte stays 1, and section 6 now says so. |
| R5-M4: PROPPATCH edits the stored container | Adopted: `prepare` unseals the stored container before the edit is built. Design; Task 4. |
| R5-L: site count | Adopted: the count is dropped; `assert_event_privacy_access`'s callers are named, and the differential test adds MOVE, PUT, PROPPATCH and DELETE. |
| R5-L: wrapper value encoding | Adopted: a text-only DAV value holding base64 of the version byte and ciphertext. |
| R5-L: per-document flags versus per-component `CLASS` | Adopted: the design says the flags hold the maximum over components, as upstream computes. |
| R5-L: where the classification lands | Adopted: the tree, the row's flags, the resource cache and its swap snapshots. |
| R5-L: the drain's mirror of `set_privacy` | Adopted: the drain's merge carries `itip_ingest`'s preservation, and the store scan includes an inbound update whose `CLASS` differs. Task 4. |

### Amended during execution

Changes made from what Task 1 measured. The
[outcome note](2026-10-27-stalwart-1-upgrade-outcome.md) records each one with its ruling.

| Finding | Change |
|---|---|
| The closing-brace exit region also counts early returns: rustc routes every `return` through it (shown on the 0.16.27 exports) | The executed rule needs a third count: the region that starts the body's last top-level statement. Design "Gates"; Tasks 1 and 7. |
| The cohort is 96.38% (17,821 of 18,490 lines), so a literal 96.4% check fails the unchanged baseline | The coverage gate uses the exact fraction. Design "Gates"; Task 7. |
| 19 shared directories, not 20; five inline test modules aren't named `tests` | Counts corrected; a restored module keeps its own name. Design "Decisions"; Task 2. |
| `account_batch` splits a pass into commit chunks, with the `Document` keys last | R58 is asserted in every chunk; R62's guard and R63 retire only once Task 6 confirms the per-partition ordering; R62's term scans stay. Design "Indexing"; Task 6. |
| `jmap` depends on `services`; no task checked `http`, `managesieve` or `spam-filter` | The crate checks are reordered, and a failure goes back to the file's owner. "File ownership and dependencies". |
| `scim_tests`'s entry function is in the SEL-only `tests/src/scim/mod.rs`, so it can't keep compiling | It takes the SEL-only disposition with its three dual-licensed siblings (Task 2's ruling 38). Design Decision 1. |
| Further corrections to cited paths, lines and signatures, and decisions on the shared interfaces | The outcome note's interface sheet and Task 1's rulings 9–33 amend Tasks 2–7. Where they correct a cite in this plan or the design, they win, and each task's brief carries them. |
