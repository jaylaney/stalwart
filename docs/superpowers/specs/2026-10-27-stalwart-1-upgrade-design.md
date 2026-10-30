# Stalwart 1.0 fresh store upgrade design

Revision 2, 2026-10-29. Revision 1 (2026-10-27) targeted v1.0.0 from the 0.16.26 base. This
revision moves the base to 0.16.27 and the target to v1.0.1. It also replaces the test
strategy with one that owns the whole 0.16.27 suite, measured against a recorded baseline.
The contracts C1–C11 and the integration architecture of revision 1 stand, with C10
extended.

## Purpose and scope

Move the zero-access fork from its 0.16.27 base to upstream 1.0.1 while preserving its
account, mail, calendar, invitation, pinning and sharing contracts, and as much of its test
coverage as can be kept. Implementation has not started; this design and its execution plan
are proposed for review.

- **Starting fork:** `main` at `9a7574203e9821a8313239f51a080c0657672be9`, the merge of PR #66,
  which makes CI enforce every test that passes at 0.16.27. The baseline below was measured
  one commit earlier, at `5b914ddb5`.
- **Upstream base:** `v0.16.27`, `aa47d8ef7bfba7200d3293198ac2592747b20e73`, merged by PR #62.
- **Target:** the immutable `v1.0.1` release, `23b69755cb3cd8ecd952fe0443c5ac067a506184`.
  It is one commit on top of `v1.0.0` (`47df4c0baeb88d83424dfda6aa7cf6040706999e`), which
  is one commit on top of `v0.16.27`, so the incoming change is exactly upstream's two
  release commits.

A later upstream patch needs its own delta assessment; this plan does not silently follow
upstream `main` or `latest`.

**Deployments.** Only test deployments of the 0.16 fork exist, including `mail.circolo.me`.
Jay confirmed on 2026-10-29 that none of their data needs to be kept. The 1.0 cutover
creates a fresh store and provisions the accounts again: new setup tokens, enrolment, and
clients added again. That runbook belongs in the private deploy repository, not here.

**Qualification scope** is unchanged: single-node RocksDB on the macOS development host and
the existing Linux x86_64 GNU product artifact, with embedded and filesystem blob storage.
Existing backend refusals stay. SQL stores, other backends, clusters and new artifact
platforms gain no qualification from this project.

## Approaches

1. **Merge 1.0.1 directly into the fork and initialize fresh stores (selected).** This keeps
   published Git history and the existing fork modules, tests and public API, and adapts each
   protection to upstream's new storage and runtime boundaries. It needs no migration or
   compatibility machinery, because there is no data to keep.
2. **A 0.16.27 bridge first.** Done (PR #62). Its value for this project is the merge base:
   Git compares the fork against exactly the two upstream commits.
3. **Rebuild the fork from a clean upstream checkout.** Rejected. It makes a clean first
   build easier, but it risks dropping small enforcement hooks and regression cases. The
   fork's history and explicit contract inventory give reviewers a better audit trail.

Use a normal merge in an isolated implementation worktree, and never rebase the published
branch. A dry merge of v1.0.1 into `5b914ddb5` reports 134 conflicts (106 content, 27
modify/delete, 1 add/add). Resolving them is only part of the port. Upstream also replaces
interfaces and introduces new writers, and Git cannot tell that those need a fork
protection.

## Scope exclusions

- No migration of 0.16 stores in either direction, no legacy encrypted archive reader, and
  no preservation of old client sync state. An old store is an unsupported input, not a
  bootstrap source.
- Existing same-store behaviour stays in scope: password and recovery wraps, pending setup,
  conversion of an owner's calendar to shared, resumable conversion and refusal of unsealed
  rows. A test named `legacy` is not a reason to remove it.
- No use of upstream's private tests. Do not enable `cfg(stalwart_tests)`, and do not
  recreate the private support modules its 562 slots expect in a sibling `stalwart-tests`
  checkout.
- No SEL-only test is ported. Enterprise features are excluded from the product. Under SELv2
  (sections 4.1, 4.4 and 2.3), even a subscription permits modified copies only for internal
  use, never in this public repository. The 0.16.27 originals stay recoverable from history.
- No deletion of existing `.run/` state or other worktrees. New tests and manual checks use a
  newly allocated state directory and disposable accounts.
- No new signup, billing, provisioning service, webmail, AI feature or encryption redesign.
- No automatic adoption of newly added protocol operations for key accounts. A new content
  surface either gets a defined refusal or the same encryption and authority treatment as
  the existing operation it extends.
- Planning does not authorize implementation, commits, CI dispatch, publication or
  deployment.

## Binding contracts

The existing approved designs remain the source of product behaviour:

- [Calendar accounts and sealing](2026-10-06-zero-access-calendar-design.md).
- [Invitations](2026-10-11-zero-access-invitations-design.md).
- [Key pinning](2026-10-17-zero-access-key-pinning-design.md).
- [Mail](2026-10-20-zero-access-mail-design.md), including its resolved implementation items.
- [Sharing revision 14](2026-10-22-zero-access-sharing-design.md) and the accepted
  [implementation interpretations](../plans/2026-10-23-zero-access-plan14-outcome.md).
- [Release policy](../../releases.md) and [developer merge gates](../plans/README-dev.md).

The contracts below are acceptance criteria, not an opportunity to redesign those features.

| ID | Contract |
|---|---|
| C1 | The vault is the login authority. Every mutation keeps its revision CAS, the terminal account checks and `za_commit`/`za_vault_commit` invalidation. Registry credential edits cannot bypass it. |
| C2 | Authentication returns keyless common tokens. HTTP keys live for one request; IMAP and POP3 keys live for their authenticated connection. SMTP, ManageSieve, WebSocket and EventSource sessions hold no session keys. Keep the existing idle and hard cache bounds and the session-revocation semantics. |
| C3 | No credential, reversible credential cache key, private key, decrypted protected content or content-bearing diagnostic is added to caches, traces, errors or persistent snapshots. Keep the approved field-specific trace exceptions. |
| C4 | Ordinary accounts keep upstream 1.0 behaviour. Key accounts keep the existing refusals, including the unsupported credential, content-search, mail-sharing and JMAP-calendar operations, and the external-directory refusals and marker preservation (calendar spec 4.3). Keep upstream's OAuth expiry, grant-revocation and scope checks. |
| C5 | Key-account mail is a stub plus a sealed blob, whatever wrote it. Every ordinary writer asserts the vault's absence in its effective commit. Every protected reader has the owner's keys or refuses; background work never acquires those keys. |
| C6 | Calendar metadata and content are sealed or sanitized before serialization and derived indexing. No hidden property can reappear in metadata, alarms, notifications, dead properties or indexes through a split or metadata-only write. |
| C7 | Sharing uses the current principal's keys, the current own-id permission and the authenticated ring identity. Keep the physical parent and link fences, conversion, future-write rotation, sync boundaries, pin-before-deliver, UID leases, SEQUENCE and delegated scheduling semantics. |
| C8 | Search work for key mail keeps only the allowed arrival-time and size fields. Sealed calendar content and protected attachment text stay out of background indexes. Provisioning and deletion fences cover every new queue and commit segment. |
| C9 | Keep upstream 1.0 stored types as supplied: add no fork fields to them, and hand-edit no generated registry schema or event enum. Keep the reserved Principal fields 150/151, the account flag `1 << 9` and the vanished collection 250, after checking each for collisions. |
| C10 | Own the retained test suite. The required set is the 0.16.27 baseline minus its recorded dispositions. A required test cannot disappear, become ignored, stop being selected, or stop reaching the code it reached, while a green aggregate hides the loss. Test-only hooks and the weakened Argon2 never enter shipping builds. |
| C11 | Qualify fresh creation and restart with the actual OSS product binary. Enterprise-enabled harness results are separate evidence, because 1.0 stamps each store with its edition. Keep the fork's independent version and provenance and its immutable release artifacts. |

## Integration architecture

### Fresh persistence baseline

Use upstream 1.0's schema, split calendar storage, serializers and pending-ID transaction
model. Align the fork's archive dependencies with the target workspace, including its rkyv
alignment configuration and local calcard crate. Keep the fork's record fields, crypto
algorithms, KDF inputs, AAD ownership, purposes and wrap identifiers unchanged unless a
separately reviewed port correction is needed. Bytes written by the 0.16 fork need not
deserialize.

The 1.0 schema is the new compatibility boundary. Document that archived inner payloads are
specific to this baseline: a matching old payload version byte does not establish
compatibility across bases. Once fresh 1.0 fixtures exist, lock their formats with
checked-decode, round-trip and process-restart tests. Do not add an old calcard or a
duplicate rkyv dependency to reconstruct the undeployed baseline.

1.0 stamps each store with its edition: schema 7 for `enterprise` builds and `7 | 1 << 31`
without, and each refuses the other's store (`crates/common/src/manager/schema.rs`). The
harness builds with `enterprise`, so its stores are enterprise-edition. That is why C11's
product gate runs the OSS binary.

Upstream removed `types::DeadProperty`. Adapt the sealed extras and collection bundles to the
new metadata representation, and keep every protected value inside encrypted carriers. Use a
fork-owned payload type only where the public metadata representation cannot carry the
existing encrypted extras losslessly. Its fields and version dispatch must be reviewed with
the calendar codec tests before any caller uses it. Do not restore the removed upstream
storage type across the workspace.

### Authentication and authority

Target `Server::authenticate(&AuthRequest) -> trc::Result<Authenticated>` (unchanged in
1.0.1), keeping upstream's `Authenticated { access_token, expires_in }` and its scope and
revocation handling. Attach session keys separately in the authorized protocol layers.
Restore the marker diversion in the new basic-auth path, along with app-password publication,
the vault generation fences and credential redaction. The common authentication result stays
keyless.

Keep the external-directory refusals (`za_refuse_directory_login`,
`za_refuse_directory_account`) at their equivalent sites, and keep `merge_directory_secret`'s
marker check. The `za::external_directory` module (PR #66) is their regression test. The
Bearer arm needs an OIDC provider and is not covered locally. What a directory
synchronisation may change on a key account besides its credential is an open product
question (fork issue #67), outside this port.

Extend the new account-cache loader with the existing key-account classification and epoch
rules. No cached hint replaces the vault and calendar reads a write needs. Keep the quiet
catch-all resolution and the exact ordering of account and vault failures.

### Calendar and sharing writes

`CalendarEvent` is metadata, and `CalendarEventContent { data, preferences }` holds the
tree. Port read and write views together. `CalendarEvent::{insert,update_full,update_meta}`
use `GroupwareWrite`/`SplitUpdate`, which serialize the content and recompute the metadata
from it. Sealing and metadata sanitization must therefore happen before those builders,
including on metadata-only updates, conversion, drain commits and COPY/MOVE.

Port the owner/grantee resolver, the conversion journal, the event and link assertions, and
the lease, pin and ring writes to `PendingId`, `AssignedIds` and the new store batch
interface. Upstream's new grantee-cleanup and share-rebuild tasks must not recreate a grant,
keep unauthorized content, or bypass ring rotation and boundary completion. Keep the
accepted scheduling limits; an upstream behaviour change needs an explicit disposition, not
a silent weakening of the lifecycle oracle.

### Mail writers and readers

Adapt stub construction and the unsealing readers to mail-parser 1.0 and
`email::message::metadata::{build,row,source,view}`. Integrate at `email_ingest_into`, at
grouped batch construction and flush, and at `email_ingest_finish`, not only in the old
single-message wrapper. Classification assertions must survive pending-ID allocation,
commit points, retries, mixed-account work and post-commit notification.

Keep nested MIME and attachment offsets, section reads, encrypted uploads and every refusal
of a raw-blob consumer. Background delivery still seals to the served public key and never
borrows a user's session keys. Submission, spam training, threading and push keep their
existing content restrictions.

### Indexing, metadata and cache snapshots

Replace the protection that lived in `services::task_manager::index` at every new entry
point of `services::index_queue::{document,index,reindex,file}` and at the storage commit
boundaries they use. Index rebuild and attachment extraction fall under the same rule.

**The index/un-index race.** At 0.16, `IndexDocument` and `UnindexDocument` run in separate
workers with nothing ordering them per document. An un-index that overtakes a queued index
write leaves an orphaned search document for a destroyed message. The race is upstream's. In
the fork, `za::mail::fence`'s in-flight index tests pin it, and since PR #66 the key-mode
JMAP ACL variant waits for tasks before its destroy.

1.0.1 replaces the two workers with a partitioned queue (`services::index_queue::manager`):
entries are grouped by index and partition id, and one holder works a partition at a time
under a lock. The port must establish whether a document's index and un-index entries share
a partition and apply in order.

- **If they do,** the race is gone. The fence tests' expected outcome changes (the late
  write no longer lands), and the ACL wait can go.
- **If they don't,** chunk 0 of the index write asserts that the document's source still
  exists (the Email metadata property, or the event or card archive). The failed assertion
  is confirmed by a read outside the transaction, the fork's R58/R62/R63 pattern.

Keep cache snapshots working on stored, sanitized views only. Neither cache population nor
snapshot serialization and restoration may accept an unsealed response view or
`SessionKeys`. Test persistent file and blob snapshots and the shared codecs, including stale
loaders racing provisioning and revocation. Redis and cluster operation remain unqualified,
and the central serialization and authority guards must not depend on which snapshot backend
is selected. Checksum framing named `seal` is not encryption and cannot satisfy this
contract.

The new IMAP METADATA and JMAP object-metadata operations initially refuse key-account
access wherever they add storage outside the existing encrypted carriers. Reuse the
protocols' error conventions, and make capability and refusal behaviour explicit. Existing
DAV property operations continue through their ported encrypted mapping. Ordinary-account
metadata behaviour stays upstream's, and no generated permission or schema change is needed.

## Test suite ownership

### The baseline

The baseline was measured on 2026-10-29 at `5b914ddb5`. Its raw listings, logs and coverage
exports are in the git-ignored `.superpowers/sdd/baseline-01627/`; the plan commits a
reduced form.

- **What 1.0.1 ships:** no public test.
  - The tree has no `#[test]`, no `tests/` crate and no `test_mode` feature.
  - Upstream links its tests through 562 `#[path = "…/stalwart-tests/…"]` slots that point to
    a private sibling checkout (560 at 1.0.0).
  - 530 of the slots compile only when that crate's own tests are built; about 27 are
    support modules that are always compiled once the flag is set.
- **What the fork has at 0.16.27:** 795 test functions.
  - 697 are unit tests in `crates/`: 466 from upstream, 183 of those in the SEL-only scim
    crates.
  - 98 are in the `tests` crate. Its suites make 197 sub-test calls.
  - After PR #66, CI selects 609 of the 795. The other 186 are the SEL-only scim tests and
    the `cluster_tests`, `stress_tests` and `scim_tests` suites.
- **What passes:** every test in the local net except `automation_tests`, which needs
  Docker, as does `directory_tests`. Both run in CI.
- **Coverage of that net,** measured with llvm-cov over 19 steps:

  | Code | Lines | Functions |
  |---|---:|---:|
  | Product code | 63.9% | 64.8% |
  | Product code, without the generated `registry` crate | 73.0% | 71.2% |
  | Fork-owned code (40 files) | 96.4% | 95.0% |
  | Upstream files the fork edited | 75.4% | 75.7% |
  | Upstream files the fork never touched | 57.1% | 57.8% |

  173 of the 197 sub-test calls ran. The rest need Docker, are SEL-only, or are gated by a
  feature or an environment variable.
- **What a plain merge does:** merging v1.0.1 deletes 520 of the 598 files under `tests/`,
  and every upstream unit test in `crates/`, without a single conflict.

### Decisions

1. **Keep the whole 0.16.27 suite and port it.** A test leaves the required set only with a
   recorded disposition, of one of four kinds:
   - **SEL-only:** the `scim` and `scim-proto` crates; `tests/src/directory/oidc.rs`,
     `tests/src/scim/*` (the SEL-only files), `tests/src/system/archiving.rs` and
     `tests/src/system/tenant.rs`; and the sub-test calls to them.
   - **Subject deleted upstream:** the test's subject no longer exists in 1.0.
   - **Internals replaced in 1.0:** the test is replaced by a behaviour-equivalent test of
     the new code, not deleted.
   - **Ungated as today:** `cluster_tests`, `stress_tests` and `scim_tests` keep compiling
     but stay out of the required set, as now.
2. **Restore everything at merge time.**
   - Right after `git merge --no-commit`, restore `tests/` in full from the fork side,
     before resolving anything else, and record the restored paths.
   - Move each 0.16.27 inline test module of upstream's into a fork-owned
     `restored_tests.rs` next to its 1.0 counterpart, attached by a single
     `#[cfg(test)] mod restored_tests;` line. A module whose subject moved follows its
     subject. A module whose subject is gone takes the "subject deleted" disposition.
   - Bring the fork's own unit tests along with their modules.
3. **Restore only the test hooks the suite uses.** Restore the `test_mode` features only
   where the retained suite needs them. Write each hook as a one-line call into a fork-owned
   module, as `common::za::test_hooks` already does.
4. **Replace the one SEL-only helper an AGPL test needs.** The dual-licensed
   `tests/src/directory/issuer.rs` imports `get_token_for_client` from the SEL-only
   `oidc.rs`. Write a replacement from the OAuth 2.0 token-endpoint specification (RFC 6749),
   not from the SEL file.
5. **Keep an expectation ledger.** Every changed assertion in a ported test names the 1.0
   change behind it (a diff hunk or release note) in the outcome note. Reviewers check those
   entries against source. Without upstream's own tests, this is what stops a port from
   quietly accepting a regression.
6. **Make the harness exhaustive.** `wait_for_tasks` covers 1.0's index queue (partitions
   and retries) as well as scheduled tasks. Cleanup enumerates 1.0's closed `Subspace` enum
   with no catch-all arm, so a new subspace stops the build.

### Gates (C10)

- **Test list.** The candidate's libtest listings, per binary and feature set, are compared
  with the baseline minus its dispositions. So are the sub-test calls the suites execute.
  The gate fails on any required test that is missing, ignored or unselected.
- **Executed sub-tests.** Rerun the baseline's 19-step coverage net on the candidate. Each
  sub-test call must execute in every mode where it did at 0.16.27 (llvm-cov's function
  counts). This needs no instrumentation of the suites.
- **Coverage.** Line coverage of fork-owned code must not fall below 96.4%. Upstream crates
  are reported, and a drop is explained.
- **Mutation sample.** Disable each re-ported protection hook in turn, and record that a
  named test fails. Tests adapted to new APIs can pass vacuously.
- **Upstream's hidden tests.** Record the number of private-test slots in each upstream
  release. A new slot marks a module where upstream added a test we cannot see; review that
  module for fork-relevant behaviour.
- **CI.**
  - Port every job PR #66 added (`unit-tests`, `upstream-suites` and the `zero-access`
    additions) to the 1.0 commands.
  - Add Dovecot's imaptest (with the 112 scripts in `tests/resources`), python
    caldav-server-tester and Fastmail's JMAP-TestSuite against the OSS product binary.
  - Run these against the 0.16.27 binary in the same job, and diff the two sets of results.
- **Product.** The OSS create-and-restart gate creates fresh accounts and data, stops the
  process, restarts on the same store, and checks mail, calendars, pins and sharing after a
  fresh authentication. This is a persistence test on one version, not a migration test.

## Execution and release gates

The [implementation plan](../plans/2026-10-27-stalwart-1-upgrade.md) uses one integration
branch with bounded ownership. Early source checkpoints are not independently deployable,
and not necessarily buildable: common types affect most crates. No permissive shim, skipped
protection, ignored test or temporary production feature gate may manufacture a green build.

1. Approve this design and the plan.
2. Merge the pinned release in an isolated worktree, restore the tests, inventory the fork's
   hooks, and settle the shared interface changes.
3. Complete the coupled core port, and reach a product and test compilation gate free of
   conflicts, with every contract mapped to code.
4. Pass the C10 gates, the units, the za and leak suites, the plain and key protocol suites,
   the retained ordinary-account CI, the independent CalDAV suites and the fresh OSS
   create-and-restart gate.
5. Complete a fresh, clean-context review of the whole branch. Close its findings, and rerun
   the affected complete gates after the fixes.
6. Record the exact candidate SHA, commands, results, skips, retries, expectation-ledger
   entries and artifact provenance. Native client checks remain a separate gate under the
   existing manual checklist.

Propose `0.2.0-beta.1` for the next fork preview, because storage compatibility changes. This
does not rename the fork to upstream 1.0. Finalize the version under the release policy at
implementation and release review. Publish only with separate authorization, and keep any
release with pending native gates explicitly limited and marked as a prerelease.

## Source references

- [Pinned upstream release](https://github.com/stalwartlabs/stalwart/releases/tag/v1.0.1)
  and its parent [v1.0.0](https://github.com/stalwartlabs/stalwart/releases/tag/v1.0.0): the
  new database layout, metadata, cache and index changes, and the move to private tests.
- In the target: `crates/common/src/auth/authentication.rs`, `cache/swap/` and
  `storage/index/split.rs`; `crates/common/src/manager/schema.rs`;
  `crates/groupware/src/calendar/{mod,storage}.rs`; `crates/email/src/message/{ingest,metadata}`;
  `crates/services/src/index_queue/`.
- The baseline in `.superpowers/sdd/baseline-01627/`; PRs #62 and #66; fork issue #67.
- The fork's `CLAUDE.md`, existing tests and the linked approved designs take precedence for
  the preserved contracts. Upstream's performance claims are not qualification of this fork.
