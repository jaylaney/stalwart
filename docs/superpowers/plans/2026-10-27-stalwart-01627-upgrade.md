# Stalwart 0.16.27 Fork Update Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate upstream 0.16.27 into the zero-access fork, preserving its formats,
security contracts and public regression suites.

**Architecture:** Merge the pinned .27 tag normally into an isolated fork worktree, resolve
three known conflict paths, adapt all iTIP merge callers and verify the changed DAV/mail
paths. Keep the 0.16 architecture; defer the separate 1.0 project.

**Tech Stack:** Existing Rust 2024 workspace, stable Rust, calcard 0.3.14, rkyv 0.8.18,
RocksDB product, existing Rust and Python/CalDAV tests and fork release tooling.

**Spec:** [0.16.27 update design](../specs/2026-10-27-stalwart-01627-upgrade-design.md).
Existing approved feature designs and the release policy remain binding.

**Status:** Proposed plan, 2026-10-27; revised 2026-10-28 after review.
Plan only; implementation/build/test execution has
not started. The 1.0 plan is deferred and supplies no tasks or acceptance gates to this one.

## Global Constraints

- Starting fork: `f9bc65b3d4f4be5acfae4737eb5fb3957d6f0857`. Old upstream base:
  `18f4e9505027d790fd7e9fdad7f922be6719101d`.
- Target only `v0.16.27`, `aa47d8ef7bfba7200d3293198ac2592747b20e73`. Fetch by URL; never
  write/add an upstream remote or rebase published history.
- Preserve stored layouts, generated-schema ownership, crypto, vault CAS/invalidation,
  key lifetimes, pinning, sharing, scheduling, ordinary-account behavior and key refusals.
- No 1.0 port, private-test dependency, migration, legacy reader, real-store reset or new
  backend/platform qualification. Use fresh worktree-specific state/output directories.
- Keep `STALWART_INCLUDE_SECRETS` unset for normal server/test/product runs. Never expose
  credentials or key material in retained logs, fixtures, assertion messages or artifacts.
- Every Cargo command on this Mac starts with
  `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`.
- Shipping uses `cargo build --locked --release -p stalwart --no-default-features --features
  rocks`; never deploy enterprise/test-mode test binaries. Run integration suites sequentially.
- Primary coordinates Git, actual-diff review and final verification; subagents implement.
  Use standard engineering for this bounded merge, high reasoning for subtle path review,
  and the strongest parent-permitted route with xhigh for final authority/persistence review.
  Resolve supported models at dispatch, honor the originating ceiling and disclose fallbacks.
- Preserve unrelated untracked review documents and both 1.0 planning documents. No push,
  CI dispatch, publication, deployment or destructive cleanup is part of plan execution.

## Review Focus

1. A sealed recurring invitation introduces a custom timezone: merge/reseal retains its
   definition and recurrence identity, including repeated delivery (Task 2).
2. DAV allprop/include changes intersect shared-calendar discovery: eligible calendars stay
   visible, private paths stay hidden/refused, and explicit principal identifies the caller
   without losing request-local admission or stored ETags (Task 2).
3. EmailDelivery state notifications are missing or identify the wrong account: retain
   the supplied EventSource regression in both modes and existing SMTP content-push controls.
   These StateChange sites do not create content-bearing EmailPush objects (Task 2).
4. Resolving versions/lockfiles changes crypto/archive dependencies or labels the product
   as upstream: preserve formats and verify real binary/provenance agreement (Tasks 2–4).
5. A green filtered command runs no relevant test, or existing skips/CI jobs disappear:
   retain suite selectors/counts, new upstream cases and both account modes (Task 4).

## Files and ownership

One implementation writer owns the integration checkout, manifests/lockfile, caller fixes
and focused regressions. Primary performs Git operations and adjudicates review. A separate
writer may own provenance/docs after the integration handoff; never overlap their manifests.
Keep capacity for the fresh independent final reviewer. Do not delegate further without
explicit permission/capacity, and serialize Cargo runs sharing output or test ports.

## Task 1: Prepare the isolated integration and source inventory

**Files:** Read `CLAUDE.md`, `docs/superpowers/plans/README-dev.md`, approved feature outcomes
and `docs/releases.md`. Create the execution record at
`docs/superpowers/plans/2026-10-27-stalwart-01627-upgrade-outcome.md` during implementation.

**Interfaces:** Consumes the pinned refs. Produces a current diff/conflict/caller map and
explicit isolated build/state locations for the implementer and reviewer.

- [ ] Reconfirm fork HEAD and unrelated work; assess any change since the planned baseline.
  Use the worktree skill/native tools to create or reuse an isolated checkout from that tip.
- [ ] Fetch `v0.16.27` by URL and verify the peeled commit matches the target. Inspect the
  .26-to-.27 delta and its overlap with fork hooks; do not fetch/merge upstream `main`.
- [ ] Record the three known conflict paths and all overlapping auto-merges. Enumerate
  `itip_merge_changes` callers across `crates` and `tests`; record the three calls in Task 2.
- [ ] Re-read each changed upstream test against its key-mode replacement (R44). In this
  delta, `tests/src/imap/search.rs` has the variant in `tests/src/imap/za_variants.rs`;
  changed JMAP event-source and WebDAV property tests still run in key mode, and system OIDC
  has no key mode. Record the equivalent result-option assertions before integration.
- [ ] Confirm the tag retains `tests/`, `test_mode`, test workflow and existing store schema.
  Check calcard/rkyv versions/features and numeric reservations 150/151, `1 << 9` and 250.
  Inventory the complete dependency movement without a blanket Cargo update, explicitly
  including sha2/inout, rustls/aws-lc, sequoia-openpgp, Tokio, dns-update and pest/stacker/psm.
  Use the design's dependency disposition: retain current crypto/functional checks and
  record patch-dependency risk; add no old-store migration or mandatory new KDF/AEAD project.
- [ ] Record fresh local target/state paths and baseline test selectors. Keep native-client
  and additional-store evidence pending; do not treat prior-source outcomes as new results.

**Check:** Scope is exactly the pinned patch release plus required fork adaptations,
regressions and identity/docs. No 1.0 persistence or test-harness redesign appears.

## Task 2: Integrate the tag and preserve the changed runtime contracts

**Files:** Merge the tag's supplied files, including manifests/lockfile and generated changes.
Adapt `crates/dav/src/common/propfind.rs`,
`crates/groupware/src/calendar/drain.rs`, `tests/src/webdav/cal_itip.rs`.
Extend `tests/resources/itip/vtimezone.txt`, `tests/src/za/drain.rs`,
`tests/src/za/sharing_review.rs`, `tests/src/imap/za_variants.rs`.
Retain the existing controls in `tests/src/za/mail/reads.rs` without expanding its mock-push
matrix merely for the new StateChange sites.

**Interfaces:** New `itip_merge_changes(ical: &mut ICalendar, itip: &ICalendar,
changes: Vec<MergeAction>)`; existing `za_read_context`/`za_archive_view`/calendar memo and
mail state-only push interfaces remain unchanged.

- [ ] Before the merge, extend the existing timezone fixture and sealed-drain regression.
  Use the established `DrainOn`/pending-item helpers and existing account fixtures. A recurring
  REQUEST update moves one occurrence into a newly referenced custom TZID with its supplied
  VTIMEZONE and `X-LIC-LOCATION:Europe/Paris`. Set the fixture default to UTC and use a winter
  date; arbitrary custom offsets alone are not interpreted by calcard. Assert correct
  recurrence/moved time, one definition and no duplicate after repeated delivery. Add a
  calendar-query time-range REPORT matching the correct moved UTC time and excluding the
  fallback-time window. The no-new-timezone control already defines every referenced TZID.
  Run the relevant whole ZA/WebDAV suites once on .26 for the behavioral red receipt;
  unrelated failures do not establish the regression.
- [ ] Primary commits the regression fixture/test changes on the isolated integration branch
  and records that commit with its expected failing assertions. Require a clean tracked
  worktree/index before merging, so abort/reproduction does not depend on dirty test edits.
  This red test checkpoint is not a releasable candidate.
- [ ] Primary starts `git merge --no-commit` of the pinned tag from that clean checkpoint.
  Resolve DAV imports as
  `use std::{borrow::Cow, collections::BTreeMap, sync::Arc};`, retain the independent main
  package version. Do not replace an entire handler or globally replace version strings.
- [ ] Reconcile the lockfile using .27's supplied versions as the baseline, retaining the
  fork manifests and their full dependency/feature graph. Let Cargo restore required fork
  edges without a blanket update. Parse/compare normalized package blocks against .27 and
  the fork; retain vault consumers and zeroize edges, not just two workspace entries.
  Record every extra graph/version change and its manifest reason; unexpected updates fail
  this check. The design lists the 12 package names differing at the starting refs.
- [ ] Use the new three-argument iTIP API at all call sites: upstream `calendar/itip.rs`
  passes `&itip`; fork `calendar/drain.rs` passes `&tree`; the WebDAV fixture helper passes
  `&message.message`. Preserve sender/token/pin/sequence checks, size limits and write fences.
  Record .27's generation of recognized missing timezone definitions and bounded acceptance
  of a missing referenced definition from an authenticated REPLY. Existing same-ID
  definitions are not replaced. Check size/quota order and the visible TZID/X-LIC-LOCATION
  policy. Derived times are computed before sealing; do not turn on background content
  indexing for key accounts to satisfy the time-range test.
- [ ] Retain .27's DAV `allprop`/include and principal selection changes across common,
  principal and dav-proto handlers. Preserve fresh admission, one request-local resolution
  per eligible calendar, pending filtering, unsealed response view and stored ETag identity.
- [ ] Extend the existing shared discovery fixture for empty allprop, explicit
  `current-user-principal` include and PropName. Assert a granted calendar is visible, a
  private one is absent, direct private access is 403, and the explicit principal belongs
  to the authenticated requester. Preserve existing memo/read-count assertions.
- [ ] Review APPEND/COPY and JMAP copy/import/set auto-merges around key sealing,
  provisioning fences, copy refusals and both content-push defenses. These sites construct
  content-free StateChange, so retain the existing SMTP `push_is_state_only` positive/
  negative control and run the supplied import EventSource EmailDelivery test in key and
  plain modes. Do not add a broad per-write mock-push privacy matrix for these state signals.
- [ ] Keep the new `collect_all` search behavior/call site and supplied SEARCH RETURN/SAVE
  regressions, preserving key search refusals and trace masking. In the key search variant,
  change `COUNT 10 MIN 1 MAX 10 ALL 1,10` to `COUNT 10 MIN 1 MAX 10 ALL 1:10`. Add
  `UID SEARCH RETURN (SAVE MIN MAX ALL) OR OR KEYWORD Flag_000 KEYWORD Flag_002 OR KEYWORD
  Flag_003 KEYWORD Flag_005`; assert the saved set via `UID SEARCH $` is `1 3 4 6`.
  Keep FROM/SUBJECT refusal cases, not upstream's content predicates, for key accounts.
  Take supplied OAuth
  loopback, WebUI pin/version-guard and generated secret-masking-helper changes unchanged;
  do not enable the secret-export opt-in or rewrite generated code.
- [ ] Run `cargo check -p stalwart --no-default-features --features rocks` and
  `cargo test -p tests --no-run`. Then run the complete ZA/WebDAV suites for the new cases
  and mail privacy changes. Required regression cases must actually complete; preserve
  sealing/leak assertions and fixture cleanup. Primary reviews the full merge diff and
  stages/commits the coherent merge after the component checks pass. Other focused
  regression additions remain separately identifiable from the pre-merge red checkpoint.

**Check:** No conflict markers remain; all iTIP callers use the incoming calendar; supplied
and fork tests remain active. No stored-format, crypto, authorization or key-lifetime change.

## Task 3: Update fork identity and maintenance documentation

**Files:** `fork-release.toml`, `crates/main/Cargo.toml`, `Cargo.lock`;
`scripts/tests/test_release_metadata.py`; read/test `scripts/release/test_verify_product.py`;
`CHANGELOG-fork.md`, `docs/releases.md`, `CLAUDE.md`, `docs/superpowers/plans/README-dev.md`.
Keep the existing build helper/verifier interfaces unless a regression proves a change needed.

**Interfaces:** Proposed fork preview `0.1.0-beta.2`; upstream tag `v0.16.27` and full target
SHA. Product version/help/startup/provenance continue to report fork, upstream and source
separately. Internal upstream package versions follow .27; do not renumber vault globally.

- [ ] Set the proposed preview consistently in release metadata, main package and lock
  entry; set upstream provenance to the pinned .27 tag/SHA. Do not rename the fork to .27
  or change the beta.1 tag/artifacts. Primary coordinates this manifest handoff after Task 2.
- [ ] Align `scripts/tests/test_release_metadata.py`'s `VERSION` with the proposed shipping
  version because its manifest-array regression copies the real main manifest. Its other
  synthetic upstream values need no global replacement. Leave the self-contained
  `scripts/release/test_verify_product.py` fixtures unchanged unless a specific correction
  requires it. If changing those fixtures, retain a different wrong-tag/version control
  (for example beta.3), not a replacement that silently makes a refusal case valid.
- [ ] Record the .27 behavior and independent version under Unreleased; update current-base
  developer commands, including `http@0.16.27`. Preserve historical outcome records and root
  upstream changelog history. Explain deferred 1.0, public tests and unchanged format scope
  without promising ongoing upstream .16 support or binary-only rollback safety.
  Replace README-dev's `git merge upstream/vX.Y.Z` example with URL fetch, verified tag and
  normal merge in the isolated branch, consistent with the no-upstream-remote rule.
- [ ] Run `python3 -B scripts/tests/test_release_metadata.py` and
  `python3 -B -m unittest discover -s scripts/release -p 'test_*.py' -v`. Expect all cases
  to pass, including malformed/mismatched provenance, clean/dirty and wrong-tag controls.
- [ ] Primary inspects the narrow identity/doc diff and commits it separately from runtime
  integration once these checks pass. Publishing is not part of this task.

## Task 4: Run the existing qualification net and independent final review

**Files:** Existing tests and `.github/workflows/test.yml`, `.github/workflows/product-build.yml`;
the new outcome record. No new CI framework or product restart harness is required.

**Interfaces:** Retain existing suite entrypoints, CI jobs, Python CalDAV manifests/policies
and product verifier. Every Cargo command uses the PATH prefix above. For integration rows
set `STORE=RocksDb RUST_MIN_STACK=16777216`; clear `ZA_KEY_ACCOUNTS` for plain runs, set it
to `1` only for key runs, and use `BLOB_STORE=FileSystem` only on its named ZA variant.

- [ ] Run the sequential matrix below on the candidate. Record nonzero selected test counts,
  named new-case completion, failures and retries; zero selected tests fails the gate.
  Do not remove test modules or broaden skips to obtain green results. Do not repeat passing
  checks without intervening changes, failures or new evidence.

| Gate | Command or existing workflow |
|---|---|
| Whitespace/style | `git diff --check f9bc65b3d4f4be5acfae4737eb5fb3957d6f0857 HEAD`; working/index diff checks while editing; `cargo fmt --all --check` |
| Core units | `cargo test -p vault -p groupware -p common` |
| HTTP units | `cargo test -p http@0.16.27 --features test_mode` |
| Edited-crate units | `cargo test -p imap -p jmap -p dav -p dav-proto` |
| Protocol/store units | `cargo test -p jmap_proto`; `cargo test -p imap_proto`; `cargo test -p store` |
| ZA embedded | `cargo test -p tests za::za_tests -- --exact --nocapture` |
| ZA filesystem blobs | Same ZA command with `BLOB_STORE=FileSystem` |
| WebDAV plain and key | `cargo test -p tests webdav::webdav_tests -- --exact --nocapture`, both modes |
| IMAP plain and key | `cargo test -p tests imap::imap_tests -- --exact --nocapture`, both modes |
| JMAP plain and key | `cargo test -p tests jmap::jmap_tests -- --exact --nocapture`, both modes |
| OAuth registration integration | `cargo test -p tests system::system_tests -- --exact --nocapture`, including .27's new pathless loopback cases |
| Ordinary-account CI | Existing directory/SMTP gates and pinned environments in `test.yml` |
| OSS shipping product | `cargo build --locked --release -p stalwart --no-default-features --features rocks` |
| Independent CalDAV | Existing unfiltered `caldav-client` job, pinned source/manifests/policies and full harness unit suite |
| Identity | Both Python commands in Task 3, plus actual product version/help verification |

- [ ] Execute the workflow's directory/SMTP and independent-client commands locally using
  their existing setup/environments. For CalDAV, retain python-caldav commit
  `111ae14b875f40d100d5259820f10210f00e20f9`, isolated venv/requirements and both default
  modes; run `python3 -B -m unittest discover -s tests/caldav_client -p test_runner.py -v`
  and `python3 -B tests/caldav_client/runner.py --binary PATH --caldav-source PATH
  --log-dir PATH`. If Docker, dependencies or other prerequisites are unavailable, record
  the affected gate pending; do not dispatch CI, install system software or claim full
  qualification as a workaround. The outcome separates completed local gates from future CI.
- [ ] Confirm new OAuth loopback, WebUI version guard, DAV allprop/include, IMAP SEARCH/SAVE
  and EmailDelivery tests are present and executed in their appropriate suites. Preserve
  documented bounded timing-flake retries and all original failure receipts. No private
  upstream suite or previously pending native run is counted as executed.
  The system suite is included because its OIDC cases changed in .27. For its documented
  timing failures, allow one unchanged-source whole-suite diagnostic rerun, retaining both
  results; an unresolved failure remains a pending gate and cannot be hidden by a new skip.
- [ ] Require a fresh read-only whole-branch reviewer with clean context, base/head refs,
  repository instructions and this design/plan, excluding implementer reports and previous
  finding conclusions. Cover every supplied change and auto-merge, format reservations,
  secrets/default masking, DAV authority, calendar ordering, mail privacy and product identity.
- [ ] Adjudicate findings against the actual diff; delegate focused corrections, re-review
  them and rerun the affected complete gates. Primary verifies fresh results before calling
  the branch passing. Record warning scope rather than claiming an unmeasured warning-free build.
- [ ] Verify the local macOS binary's identity using the existing identity functions in
  `scripts/release/verify_product.py`: `load_metadata(root)`,
  `verify_checkout(root, source_commit)` and
  `validate_identity(metadata, version_output, help_output, source_commit, branch_ref)`.
  Use the actual committed HEAD/branch, clear `CONFIG_PATH` in the binary subprocess
  environment, and capture stderr with stdout for `--help`. Do not run the full provenance
  CLI on a macOS binary: its target/CPU constants describe the Linux shipping artifact.
  Reserve that CLI with `--inspect-elf` for the future authorized exact-source Linux workflow,
  which records checksum, attestation, CPU baseline and measured GLIBC requirement. CI
  dispatch or pushing still requires separate authorization.
- [ ] Finish the outcome record with exact SHA, merge/caller dispositions, commands/results,
  new regression outcomes, failures/retries, independent review and pending native/store/
  platform gates. Native Calendar/Mail checks follow the existing manual checklist on
  disposable state; automated results do not establish native-client behavior.

**Stop:** Hand over the reviewed, locally qualified branch and outstanding gates. Do not
push, publish, deploy, modify old local state or begin 1.0. Planning itself changes only
the design/plan documents; implementation starts after their review/approval.
