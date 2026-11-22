# Zero-access JMAP calendars and browser sign-in: plan 17 outcome

Plan 17 ([index](2026-11-14-zero-access-17-jmap-calendars.md)) built release 6 from the spec
`2026-11-13-zero-access-jmap-calendars-design.md`, revision 7 (decisions D1 to D29, invariants 57
to 76). This note was written by Task 43 on 2026-11-17 and extended through the final review and
the pull request's four reviews. After the merge, on 2026-11-21, it was condensed to what binds
later work and what stays open; the last section points to the rest.

## Status and what can be claimed

- **The release.** A key account's calendar works from an unmodified JMAP client in a browser.
  An OAuth token issued through the authorization-code flow carries a 32-byte secret after a `~`.
  A per-account token record (principal field 153) holds the master key wrapped under keys
  derived from those secrets. The HTTP layer opens the keys on every request that presents such
  a token. The JMAP calendar methods read and write through the same key-account pipeline as
  CalDAV, which moved from `dav` into `groupware::calendar::pipeline`.
- **Merged.** Pull request #90 was merged by the project owner on 2026-11-21, merge commit
  `04145d586` on `main`, at head `f559febdb`. He merged before the pinned gates run on that head
  had finished and without the reference client's checklist having been run; both were stated to
  him. That run has since finished: both gates passed ("CI and the gates"). The branch
  `release6-jmap-calendars` sat on `ade47a9ae`. The head of the code is `bea8a39fb` (last
  product change `20ca477f4`); the commits after it are the gates' toolchain pin `6131cc902` and
  records. After the merge this consolidation's branch removed the pin (`cd6f5f417`); the removal
  did not survive the merge of `main` into the branch, whose retention jobs are pinned to Rust
  1.99.1 ("CI and the gates").
- **This consolidation** is pull request #98. The controller merged `main` into its branch on
  2026-11-22 (`c08595afc`); after that merge its `.github`, `scripts`, `crates` and `tests` are
  identical to `main`'s, and the branch changes documents only.
  `git log --oneline ade47a9ae..f559febdb` lists every commit; the fix waves' commits are in "The
  pull request's findings".
- **Command labels**, as the plan's matrices used them (the suites with
  `STORE=RocksDb RUST_MIN_STACK=16777216`): ZA is `cargo test -p tests za::za_tests -- --nocapture`
  with `BLOB_STORE` unset, ZA-FS the same with `BLOB_STORE=FileSystem`; SWAP is
  `cargo test -p tests za::cache_swap::cache_swap_tests -- --exact --nocapture`, `BLOB_STORE`
  unset; WP is `cargo test -p tests webdav::webdav_tests -- --nocapture` and WK the same with
  `ZA_KEY_ACCOUNTS=1`; IP and IK, JP and JK are the same pair for `imap::imap_tests` and
  `jmap::jmap_tests`; SHIP is `cargo check -p stalwart --no-default-features --features rocks` and
  TM `cargo check -p tests --tests`, each with the plan's warning filter; PRODUCT is the local
  product build with thin LTO and 16 codegen units; FRESH is
  `python3 -B tests/product/fresh_store.py --binary target/release/stalwart --log-dir <a new directory>`.
- **Limits.** Local results: RocksDB on one node, macOS on Apple silicon (so the release verifier
  ran without its ELF inspection), debug test builds except the full-profile product builds with
  `--locked` that the fresh-store runs used and the coverage net's instrumented builds, the
  in-store blob store unless a line says ZA-FS. CI results are hosted runs, quoted per head. Not
  run anywhere: the reference client's checklist (`docs/zero-access/client-jmap.md`, its D29 rows
  included), other store backends, a second node, native clients, any browser client. Not run
  locally (CI ran them): the Docker suites (`directory`, `automation_tests`) and the independent
  CalDAV client. A passing suite is not review approval, and a review approval is not CI.
- **D29's evidence** is source and handler evidence, not the reference client's result.
  `paged_sync_retention_collision` reproduces the retention-marker collision through the real
  HTTP handlers: the first pages carry distinct origins, and after a rename the initial-origin
  page is served and the exact-origin page answers `cannotCalculateChanges`. With it:
  `paged_sync_origin_survives_range_changes`, `paged_sync_legacy_and_ordinary`,
  `calendar_state_slot_errors`, `precondition_matrix`, the codec and parser unit tests, and
  `query_changes_paged_origin` for `/queryChanges` from both origins.
- **The plan's five review-focus conditions**, each passing:
  1. the first invitation keeps the browser signed in
     (`scheduling::first_invitation_and_the_session`; limit R31-8: it shows that a vault write
     happened, not that it was the pin's rewrap);
  2. a native client's dead properties survive a browser move
     (`pipeline::placement_reseals_event_container`; with the reseal removed the grantee's
     readback failed);
  3. a grantee's default reminders (`sharing::grantee_default_alerts`; limit R36-5: the first
     event cannot tell the grantee's copy from the owner's live entry);
  4. a browser edit keeps what an ordinary account keeps
     (`oracle::round_trip_keeps_what_an_ordinary_account_keeps`);
  5. search on a large calendar: `query::text_query_cost` printed 300 events opened in 402 ms for
     one text query without a time range and 10 events in 17 ms within one day, on `a5e45dc2f`,
     a debug build on local RocksDB inside the za suite. The counts hold by construction; a
     networked store adds a read latency per candidate (R26-6).
- **Costs the plan named.** The memory of an unranged text query was not measured: the query
  holds every candidate's terms and, by R26-5, every opened event's full plaintext view, for the
  request. The `/set` answer after D11 is upstream's (`events::create_use_default_alerts`); the
  alerts appear on the next `CalendarEvent/get`.

## The session rule

Decided by the project owner on 2026-11-14 (spec D19): a key account's browser sessions end only
on a credential change (a password change, a recovery or a second-factor change), and a grant's
deletion ends that grant's. Every other vault write leaves them; it still evicts resident keys, a
request whose keys were opened at a generation no longer current answers 401 (D23), and the
client's refresh then succeeds. Every use of the rule goes through `za_session_binding`. Since
the final review a second-factor change also draws a fresh salt (FA-1), so the binding never
returns to an earlier value.

## CI and the gates

| Head, pushed | Pull-request run | Manual run, with the retention gates |
|---|---|---|
| `dbd2fccc8`, 2026-11-17 | every job passed | not dispatched |
| `d6929c89e`, 2026-11-19 | every job passed | Automation Tests failed (a); coverage gate failed (c) |
| `68d905131`, 2026-11-20 | every job passed | key-mode JMAP failed (b); both gates passed (c) |
| `8e05a6b44`, 2026-11-20 | every job passed | every job passed, both gates (c) |
| `68a1cc2fb`, 2026-11-21 | cancelled (d) | cancelled (d) |
| `f559febdb`, 2026-11-21 | every job passed | Automation Tests (a) and the net's SMTP step failed (e); both gates passed (c) |

- The runs of the Test workflow, by id (`https://github.com/jaylaney/stalwart/actions/runs/<id>`):
  pull-request runs 44189371288 (`dbd2fccc8`), 44587362075 (`d6929c89e`), 44759113100
  (`68d905131`), 44804627661 (`8e05a6b44`), 44921515919 (`68a1cc2fb`, cancelled), 44924682624
  (`f559febdb`); manually dispatched runs with the retention gates 44587698145 (`d6929c89e`),
  44759179352 (`68d905131`), 44804659603 (`8e05a6b44`), 44921532274 (`68a1cc2fb`, cancelled),
  44924688662 (`f559febdb`).
- A pull-request run's jobs: Check Style, Unit Tests, Test, Upstream Suites, Zero-access Test,
  Build Product, Fresh OSS Store Restart, Independent CalDAV Client (on `dbd2fccc8` also the
  code-scanning analyses). They include the Docker suites and the CalDAV client in both modes.
  The gates job runs on manual dispatch only. On `8e05a6b44` the key-mode JMAP step passed in
  both runs; on `f559febdb` no rerun fired in the Zero-access Test job.
- (a) The Upstream Suites job's Automation Tests step failed at upstream's DKIM selector
  assertion (`tests/src/automation/dkim.rs:89`). On `d6929c89e` the key was generated a second
  after the test read the clock, and the same step passed in the pull-request run on that
  commit. On `f559febdb` the selector was one second later than the clock the test read: the
  failure recorded for 2026-11-19, seen a second time. It was seen a third time in pull request
  #98's own run (below). The controller's classification, both
  times: a timing failure of that test. Cost if wrong: a regression there passed as timing; the
  step runs on every pull-request run.
- (b) The step "JMAP Tests (key accounts)", which had no rerun then, failed at
  `tests/src/jmap/za_variants.rs:1591`, the thread-merge variant, "test# 1/5", thread sizes
  `[1, 3, 6, 212]` where `[1, 3, 218]` is expected. Client 5 is the group `sales@example.com`,
  an ordinary account in key mode, whose threads upstream's background task merges. The step
  passed in the pull-request run on the same commit. The controller's classification: a timing
  failure of upstream's ordinary-account thread merge, the class of Task 24's plain-mode failure;
  the branch changes nothing under `crates/email` or in the thread-merge task. Cost if wrong: an
  ordinary-account threading regression passed as timing.
- (c) The coverage gate compares the cohort with the baseline (full 17821/18490, 96.38%; runtime
  10646/11246, 94.66%):
  - `d6929c89e`: full 30875/32130 (96.09%) and runtime 18086/19187 (94.26%), about 93 and 78
    covered lines short; the coverage net, the listings and the inventory gate passed. The
    controller ruled to close the gap in the pull request with a tests-only wave (wave D: no
    product code, no new hook, no change to the qualification files), measured locally on
    `0c73eafc4` at 31354/32379 and 18345/19216.
  - `68d905131` (the code of `0c73eafc4`; 3 h 10 min): full 31346/32379 (96.81%), 138 above the
    31208 needed; runtime 18337/19216 (95.43%), 146 above the 18191 needed; each 8 lines below
    the local figure, as wave D's re-review expected from a reused profile. Inventory gate
    passed. Rust 1.99.1, inferred from the runner image (the job did not print its compiler).
  - `8e05a6b44` (the code of `6c5c46b26`; 3 h 15 min): full 30776/31866 (96.58%), 62 above the
    30714 needed; runtime 17641/18583 (94.93%), 49 above the 17592 needed. Inventory gate passed.
    Rust 1.100.0, inferred from the image. Every measured total was smaller than on `68d905131`,
    in files wave E did not change too (`crates/common/src/za/tokens.rs` 775 to 727 lines). The
    controller's classification: a change of the measurement with a newer compiler, not coverage
    lost by wave E. Cost if wrong: a real loss read as a toolchain effect.
  - `f559febdb` (run 44924688662; the gates job 2 h 49 min), verbatim from the job's log:
    `cohort full: lines 31455/32488 (96.82%) against 17821/18490 (96.38%): pass` and
    `cohort runtime: lines 18353/19232 (95.43%) against 10646/11246 (94.66%): pass`, 142 lines
    above the 31313 the full figure needs and 147 above the 18206 the runtime figure needs.
    Inventory gate passed. The job printed its compiler, as the pin's commit made it do:
    `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, LLVM 23.1.1; this is the first gates result whose
    compiler is read from the job's own log.
- (d) `68a1cc2fb` (wave F, the CI rerun and their record): its pull-request run was cancelled by
  the workflow's concurrency rule when the fifth push arrived (Check Style, Unit Tests, Test and
  Upstream Suites had passed; the rest was cancelled), and its manual run was cancelled by the
  controller as superseded by the pin. No complete CI result exists for that commit.
- (e) The fifth push (`68a1cc2fb..f559febdb`, 2026-11-21) carried the gates' toolchain pin
  (`6131cc902`) and its record. The manually dispatched run on `f559febdb` (44924688662) was
  started with the pin in place. Its conclusion is "failure", from two jobs: Upstream Suites, at
  the DKIM selector check (a); and the gates job, whose step "Coverage net (19 steps)" failed
  because the net's `smtp` step failed twice: `smtp::queue::retry::queue_retry` failed in the
  whole run and again when the net reran it by name. Every other step of the net passed, and all
  19 profiles were merged. The steps after the net ran and passed: the listings, the executed
  functions, the inventory gate and the coverage gate (c). The controller's classification of
  the `queue_retry` failure: upstream's timing-flaky SMTP test, the pattern recorded for it under
  "Failures classified as timing" (it has failed in a whole run and again alone); the branch
  changes no SMTP code, and the pull request's own run passed its SMTP step on the same commit.
  Cost if wrong: an SMTP queue regression passed as a timing failure.
- **The toolchain pin, made, removed, and kept on `main`.** On 2026-11-21, before the merge, the
  project owner decided item 26: the gates job measured with Rust 1.99.1, the compiler the
  deployed product was built with (`6131cc902`, merged with pull request #90; the controller had
  named the current stable, 1.100.0). On 2026-11-21, after the merge, he decided to remove that
  pin, since upstream pins no toolchain; this reverses item 26. The change, `cd6f5f417` (the gates
  job of `.github/workflows/test.yml` only), took out the job's `RUSTUP_TOOLCHAIN` entry and the
  toolchain install and kept a step printing `rustc --version --verbose`. After pull request #98
  was opened on 2026-11-21, `main` gained pull requests #97 (loopback test listeners), #89 (the
  parallel retention pipeline), #99 (the 0.4.0-beta.1 release preparation) and #103 (its record
  in `CLAUDE.md`), all merged by the project owner by 2026-11-22. #89 rebuilt the retention
  gates: one instrumented build feeds three coverage jobs, a separate job captures the listings,
  and every retention job pins `RUSTUP_TOOLCHAIN` to Rust 1.99.1 and prints `rustc -vV`;
  `CLAUDE.md` there says to keep the pin in step with the compiler used to build the deployed
  product. #103 records in `CLAUDE.md` that the product build uses the runner's stable Rust,
  unpinned, as upstream pins no toolchain, and that `0.4.0-beta.1`'s artifact was built with
  rustc 1.100.0. In the merge of `main` into this branch (`c08595afc`),
  `.github/workflows/test.yml` is `main`'s, and in `CLAUDE.md` the paragraph on the `gates` job is
  `main`'s and the rest is `main`'s text with this branch's move of release 6's inventories. So
  this branch's removal of the pin (`cd6f5f417`, recorded in `ad4142c3a`) does not survive: the
  gates job it edited no longer exists in that form, and the retention jobs on `main` are pinned
  to Rust 1.99.1. The two commits stay in the history.
- **What the gate's figures depend on.** The retention jobs measure with Rust 1.99.1 until the
  pin is changed. The earlier results still show how much the compiler moves the figures: the
  cohort measured about half a point apart on Rust 1.99.1 (`68d905131`) and Rust 1.100.0
  (`8e05a6b44`), and on 1.100.0 the gate passed with 62 and 49 lines to spare. The run on
  `f559febdb`, on Rust 1.99.1, measured the totals of the earlier 1.99.1 run again (upstream's
  product 398,456 lines there and in the run on `68d905131`, against 387,958 in the run on
  `8e05a6b44`, whose image carried Rust 1.100.0), and the margins are back to about 140 lines.
  This confirms the controller's earlier classification that the smaller margins on `8e05a6b44`
  were a change of the measurement and not lost coverage. The baseline file does not record the
  compiler it was measured with.
- **Item 27** (pin the release workflow to the gates' version) stays closed and was never made:
  `main` keeps the product build unpinned on purpose (#103).
- **Pull request #98's own CI**, before the merge with `main` (run 44943408483 on `37abc08ea`):
  the first attempt failed in Upstream Suites at upstream's DKIM selector check
  (`tests/src/automation/dkim.rs:89`), the third occurrence of that timing failure; the failed job
  was rerun and passed, so every job of the run passed. No CI has run on `c08595afc` or later.
- **The CI rerun** (item 25, approved by the project owner on 2026-11-20): `bea8a39fb` gives the
  key-mode JMAP step the one-rerun form of its plain-mode twin, and its entry in
  `scripts/tests/ci_steps.py` gains `rerun="same"`. The cost the controller named: a key-mode
  JMAP regression that fails only sometimes is masked, as for the plain-mode step.

## Local validations

- **Task 9, the move's proof** at `a985f16b8`: global formatting, the product build
  (warning-free), the `groupware` and `vault` units (196 and 47), the workspace units (842 passed,
  2 ignored), ZA, ZA-FS, SWAP, WK, WP, the pinned CalDAV client in key mode (74 passed, 1 skipped,
  2 xfailed) and the fresh-store script (9 of 9 readback checks) passed, with no edit to a CalDAV
  test's assertions.
- **The final matrix, first pass** (Task 44): rows 1 to 10 on `f6f9f0dde` to `7000924b4`, which
  differ in documentation only, each passed on its first attempt. The controller stopped it after
  row 10: every fix of the final review changes `crates/` or `tests/`, and one complete matrix on
  the final source is stronger evidence.
- **The final matrix, second pass**, on the source of `a5e45dc2f`: 32 rows; 30 passed on their
  first attempt. Row 10 (ZA-FS) failed first at a helper timeout (a PROPFIND at
  `tests/src/utils/webdav.rs:120`, under a one-minute load up to 280.86 from other work on the
  host) and passed on the rerun the plan allows. Row 15 (the SMTP tests, `SPAM_RULES_URL` pinned
  to the spam-filter rules v3.0.4) failed in upstream's `queue_retry`
  (`tests/src/smtp/queue/retry.rs:238`, left `[1, 2, 2]`), failed again by name (`[1, 1, 3]`) and
  passed on a further rerun by name ("Rulings that bind later work"). Unit rows: groupware 208,
  vault 59, workspace 895 passed and 2 ignored, smtp 16, http 7. The `--locked` product build was
  clean, and the verifier and the fresh-store script passed. The za suites ran under the project
  owner's load gate of 40. Not run: the Docker suites (Docker was not running), the CalDAV client
  (`python3.12` is not installed; nothing was installed), the gates, CI.
- **On `25e098520`**, after the first review's waves: 31 rows; 30 passed on their first attempt.
  Row 24, key-mode `jmap_tests`, failed in upstream's `calendar::alarm`
  (`tests/src/jmap/calendar/alarm.rs:166`, "EventSource alarms do not match": the channel held
  `k1` only, where `k2` then `k1` are expected) and passed on two further runs of the unchanged
  command; the controller's classification is under "Rulings that bind later work". Workspace
  units 899 and 2 ignored; the product build, the verifier and the fresh-store script passed. Not
  run: the SMTP tests, the Docker suites, the CalDAV client, the gates.
- **On the second review's source** (`f3630cf67`, the product code of `2f00c2abf`; two rows on
  `0c73eafc4`): 18 rows, all passed (groupware 213, vault 64, workspace 912 and 2 ignored, http
  8; the product build, the verifier and the fresh-store script). The integration suites ran only
  under the coverage net's instrumented build, every step but `automation-tests`, with
  `dkim2_dsn_is_signed` passing on the net's rerun by name. Not run: the integration suites in
  the plain test build, the Docker suites, the CalDAV client, the gates in CI.
- **On `6c5c46b26`** (wave E): 9 rows, each passed once (groupware 213, vault 65, workspace 913
  and 2 ignored, http 8, key-mode `jmap_tests`); wave E ran the za suite once on that commit. Not
  run: the product build, the fresh-store script, ZA-FS, SWAP, `webdav_tests`, `imap_tests`,
  plain `jmap_tests`, the SMTP tests, the store suites, `system_tests`.
- **On `bea8a39fb`** (wave F and the CI change): 8 rows, each passed once (groupware 213, vault
  66, workspace 914 and 2 ignored with common 222, http 8); wave F ran the za suite once on
  `5ecae78c4`. Not run: the product build, the fresh-store script, ZA-FS, SWAP, the protocol
  suites in either mode, the SMTP tests, the store suites, `system_tests`.
- Every workspace unit run rewrites 26 tracked fixtures under
  `crates/dav-proto/resources/requests/` (upstream's restored `parse_requests` test); each time
  they were restored.

## The project owner's decisions

- **2026-11-15**: approved the explicit-origin D29 amendment and its implementation
  (`af3ed8934`); origin inference is not reopened.
- **2026-11-15 and 2026-11-16**: the routing of implementers and reviewers was changed on his
  instructions. The routing the plan and the spec first named ran Tasks 1 to 16, Task 17's first
  implementation and review, and Tasks 19 to 21. On his instructions of 2026-11-15, Task 17's
  fix round, Task 18 and the tasks from Task 22 on had other implementers and reviewers; on his
  word of 2026-11-16, every task review and scoped re-review from Task 31's fix round on was
  routed again. The plan's and the spec's routing lines were not edited then.
- **2026-11-17**: raised the za suites' load gate from a one-minute load of 20 to 40 for the final
  matrix ("let's raise it to 40"; the five-minute limit stays below 200). The branch pushed at
  `dbd2fccc8` and pull request #90 opened on his word.
- **2026-11-18**, on the pull request's first review:
  - spec 3.3 corrected for the UID lease, follow-up issue #91, no code change
    (C4617611574 = FB-3);
  - the drain only within the token's scope, on JMAP and CalDAV, and the search pre-pass under
    the same kind of condition (C4617611594 = FC2-3 = R34-4, item 4);
  - one refused occurrence fails only itself (C4617611635 = FC1-7);
  - a key calendar's delete in one transaction on both protocols, refused above 5,000,000 bytes
    before any cancellation, with notes on issue #92 (C4617611521);
  - the fence against a crossing write, with a store-level test (C4617611506);
  - no code change for the UID privacy-conflict answers (C4617611630, inherited);
  - FC1-1 (= R36-10) recorded in spec 3.3, no code change (item 3);
  - the `code` masking confirmed as an amendment of spec section 10 (FA-3, item 1);
  - the co-author line of four documentation commits left as it is (R44F-5);
  - the replies on the review threads posted on his word after the verifications.
- **2026-11-19**: confirmed the fence's refusal after the cancellations as a residual (RA-4,
  item 18), the store ruling (item 19) and the search pass of a `Blob` call for a token without
  the mail scope (RB-3, item 20); RB-4 and RB-5, what a token's scope admits on DAV, no change in
  this pull request (item 21); issues #93 and #94 filed (item 22); the first review's commits
  pushed (`dbd2fccc8..d6929c89e`) and the gates dispatched manually on `d6929c89e`.
- **2026-11-20**:
  - a grantee's `/changes` reporting in `destroyed` the ids of the owner's events it may not
    see, accepted as inherited (FR-2), no code change;
  - D15 stays a sync-correctness rule, not an access control (item 2; FC2-5, FR-1), no code
    change;
  - the second round pushed (`d6929c89e..68d905131`), its addressed threads marked resolved, the
    second whole-branch pass run, issue #95 filed (item 24);
  - item 15 (FC1-4 = FR-7): no change in this pull request, issue #96;
  - item 25: CI's key-mode JMAP step reruns once (`bea8a39fb`);
  - the third round pushed (`68d905131..8e05a6b44`).
- **2026-11-21**: the fourth round pushed (`8e05a6b44..68a1cc2fb`); item 26, the gates measure
  with Rust 1.99.1 (reversed after the merge, below); pull request #90 merged ("Status and what
  can be claimed"); this consolidation asked for.
- **2026-11-21, after the merge**: the gates' toolchain pin removed, since upstream pins no
  toolchain (`cd6f5f417`); this reverses item 26. Superseded by the retention pipeline he merged
  on 2026-11-22 (#89), which pins every retention job; the product build is unpinned on `main`
  (#103).

## Rulings that bind later work

The controller's rulings, each reported to the owner, with what it costs if wrong. Rulings about
process (routing, review order, reruns of a run) are in the history.

### Tokens and sessions

- **Task 13 (preflight).** The binding is the credential fingerprint `za_session_binding`, not
  `record.revision`. If wrong: users signed out after unrelated vault writes.
- **Task 13 (M1).** A strict 43-byte alphabet and final-sextet check runs before `decode_slice`,
  since the decoder's malformed-tail diagnostics re-decode into unzeroed scratch; accepted inputs
  are identical.
- **Task 16.** A Bearer lookup error maps to a fixed `AuthEvent::Failed` and a 401 with no
  payload.
- **Task 17 (preflight).** A failed vault assertion is retried while the binding is unchanged,
  within Task 15's three-attempt bound; only a changed binding answers `invalid_grant`; exhausted
  retries and other failures answer a server error, the grant cleaned up. If wrong: a race with an
  unrelated vault write fails a sign-in.
- **Task 17 (D6).** The password step's hand-off writes one more assertion-only batch against the
  original vault snapshot before it returns the secret; a lost assertion is an authentication
  failure without retry, and a failed operation deletes the provisional entry. Cost: one batch per
  keyed hand-off.
- **Task 17 (cleanup scope).** Upstream's failures after `create_oauth_grant` and before the keyed
  issue may leave a grant row with no wrap, as upstream does.
- **R18-1.** In `za_token_refresh`, a stored generation of the presenting grant above the
  presented one answers `Superseded`: nothing written, nothing deleted. If wrong: a retry is
  refused where it could have been answered; the client already holds the newer token.
- **R18-6, R18-8.** The code stands: no fork trace for `Superseded` and `VaultChanged`; an
  unusable record makes a refresh delete its grant while the sweep leaves the record. Both are
  open spec questions ("Open for the project owner").
- **FA-1 (= FD-2).** Every change of the second factor draws a fresh salt in `za_totp` through
  `za_set_password`, so the binding never recurs. Deleting the grants on a binding change was
  weighed and not taken (not atomic). Cost: one more Argon2 derivation per change, and a future
  second-factor endpoint that does not hold the password cannot use this fix.
- **FA-3 (= FD-1)**, confirmed by the owner on 2026-11-18. The value of every `code` query
  parameter is masked in a traced request URL and in a traced `Referer`, for every request, since
  no account is known there. Cost: an operator no longer sees an authorization code in a debug
  URL trace.
- **C4617611602.** A deleted grant's sessions are evicted in place and the eviction broadcast
  before its token cleanup is awaited. Cost: the doubled eviction on every account (spec 10;
  RB-14).
- **C4617611615.** A keyed cache entry's deadline is fixed at verification, only a live entry is
  published, and the keys are inserted only under the revocation epoch read before verification
  (closes W-1). Cost: per `OAuthGrant` invalidation the keys are removed after the epoch advances.
- **C4633314515.** For keys opened from a Bearer token, `za_bearer_entry` opens the token again
  at publication and lowers the entry's deadline to the fresh remaining lifetime less one second,
  never raising it; an expired token publishes nothing and is served with its keys. The form for
  every account was not taken (a merge-hotspot file every login passes), so an ordinary account's
  entry keeps upstream's behaviour. Cost: one more token open per keyed cache miss; with one whole
  second left nothing is published, and such a request is verified in full and charged to its
  address's anonymous rate limit where one is configured.
- **FR-3 (wave E).** `put_access` removes the earliest-expiring entry, the first on a tie, before
  it adds the new one. If wrong: an older, still valid access token stops opening one refresh
  earlier in that rare case.
- **C4646937244 (wave F).** `put_access` takes the write's time: an access entry already expired
  at it is not stored and removes none; the refresh entry is still written.
- **The cost of evicting an account's resident keys when one of its grants is deleted** (P5 of
  the final review's fix wave, recorded by its re-review): after any grant deletion of a key
  account, each Basic CalDAV client of the account pays one Argon2 verification on its next
  request and another browser session one record read; the grant arm evicts the key cache, not
  the search index cache.
- **RB-17.** The post-write search pass is unchanged: it runs only after an admitted `Email/set`
  or `Email/import` committed an ingest, which a token whose scope excludes mail cannot reach. If
  wrong: one pass after a write the token could not make.

### The pipeline and the calendar delete

- **Preflight: `ZaDeleted` and CalDAV's deletion behaviour stay as they are**, with no post-commit
  receipt field or failure hook: Task 8 produces only `AccessToken` invalidations, and
  `invalidate_caches` propagates errors only from role expansion, so a post-commit error is
  unreachable today. If wrong: a future fallible post-commit step needs committed-receipt handling
  first; a pre-commit hook is not post-commit coverage.
- **Task 8.** The CalDAV delete's assertion-only no-op keeps
  `ids.change_id(owner, Calendar).unwrap_or_default()`: `last_change_id` debug-panics on the
  absent id after a concurrent unlink. The receipt is 0 only when no Calendar change was made.
- **Task 11.** `CalendarEvent::enqueue_full_at` is `update_full_at`'s body without its commit
  point, so one physical batch carries a placement update; existing callers keep the wrapper.
  Vanished hrefs use the asserted parent context's current names, or incremental sync filters out
  an obsolete href.
- **Task 12.** The whole-event destroy adds no final commit point: a split could publish the
  deletion and then fail a context-only write, losing its receipt and mail.
- **C4617611584.** The removed event's href comes from the parent the delete or placement
  asserted, not from the request-start cache.
- **P4.** An empty context list is refused with `Forbidden(Context)` at the head of
  `za_commit_update` and in `za_refresh_event`. If wrong: a refusal where a panic was; the mail
  guard on that exit ends unsettled.
- **The store ruling** (RA-2, RA-5, RA-6, RA-7; confirmed by the owner on 2026-11-19). The one
  transaction, the 5,000,000-byte size refusal and the change-id fence apply on RocksDB, SQLite,
  PostgreSQL and MySQL. On FoundationDB, a read replica and the ephemeral store a key calendar's
  delete stays chunked and unfenced. Why: on FoundationDB one transaction fails a large delete on
  every attempt after its cancellations, and a backend the fork does not qualify does not lose a
  function it had. `za_change_id_fence_tests.rs` pins the property on RocksDB and SQLite;
  PostgreSQL and MySQL hold it by reading. Cost: those three stores keep the partial outcome of a
  failed later chunk.
- **`CalendarTooLarge`** answers `forbidden` with a fixed text over JMAP and 403 over CalDAV,
  since 507 and `tooLarge` mean something else. Cost: a client meets a generic refusal whose text
  names the remedy.
- **RA-9.** The size count leaves out assertions and the change-log record; the size check's
  comment says so, and the count is unchanged.
- **C4633314478.** A listed child the build wrote nothing for refuses the delete with "try again"
  (`Busy`) before the size check and any cancellation, in the fenced branch only.
- **C4633314492.** A keyed `COPY` inside one calendar runs the UID check and answers 403
  `no-uid-conflict`, as upstream answers an ordinary account (release 4's handler answered 204 and
  left two names); a key calendar's delete removes every name an event has under it. Cost: a
  request that "succeeded" is refused.
- **C4633314503.** A deleted calendar's members come from the snapshot's stored event records,
  not from the path index, on every store. That no write fences an event's name is inherited and
  is not taken here.
- **C4633314510.** The size check reserves the counted size of the fence's operation, from the
  one definition the operation uses (`ZaChildFence::operation_class`). Cost: a delete within ten
  counted bytes of the cap is refused.
- **C4644031673 (wave E).** The owner's `onDestroyRemoveEvents` check tests the flag first;
  without it, a check that stops at the first stored record naming the calendar.
- **TenantQuota.** The enterprise-gated arm is removed; a tenant-quota error, which exists only in
  enterprise builds, takes D28's store-failure path, since code under the enterprise feature is
  Stalwart Enterprise Licence code. Cost: an enterprise build answers a tenant-quota failure with
  the generic store-failure error.

### The JMAP surface

- **Task 21, R21-2.** Unreadable calendars and events trace fresh `DataCorruption` events with ids
  and fixed reasons, and every served key-calendar method routes its keyed store errors through
  the same payload-free rebuild, since a deserialization error can carry raw archive bytes. Cost:
  less detailed diagnostics.
- **D28's trace.** The per-object store-failure trace and that of `Calendar/set`'s default write
  are fresh events with the account id, the document id when known and a fixed reason, since the
  original may carry stored bytes. Cost: less detail.
- **Preflight: every submitted `blobId` link is refused**, a same-event embedded blob included,
  since spec 6.5 and 6.11 refuse attachments on every patch; `za_take_rulings` lost its unused
  stored argument. If wrong: unsupported attachment behaviour.
- **R28-1, R28-2.** `request_alerts.accept` runs after `commit_batch` returns `Ok`. A store error
  from `za_writer_keys` is the method's store error, rebuilt at the dispatcher, not the keyless
  refusal; this binds every key writer.
- **R31-6.** An update with `hideAttendees: true` is refused: the query relies on no key event
  carrying the flag.
- **Task 33.** A destroy of an unanswered invitation sends no DECLINED, as CalDAV's DELETE
  (upstream skips a NEEDS-ACTION attendee). If wrong: a decline there would break CalDAV parity.
- **R34-2.** The drain hook drains the caller's own inbox whatever account the methods name.
- **Task 39's F2 and F3.** The share endpoints write the ACL through upstream's builder, which logs
  one share notification per entry change with ids and rights only (Task 39's F2). A view
  grantee's empty `Calendar/set` patch is `forbidden`: the context is taken with `Acl::Modify`
  first (Task 39's F3); if wrong, an empty update to a view-level shared calendar errors. Task
  39's F1 is open item 10.
- **FC2-1, C4617611549.** For a caller that is not a member, an event whose unsealed tree is not
  public enters no text index; its update and destroy are checked against the higher of the
  stored flags' and the unsealed event's privacy (JMAP only); an event whose view does not open
  answers `notFound` (RA-13). Cost: JMAP is stricter than CalDAV there; one content read and
  unseal per grantee destroy.
- **X2 and X4.** The key writer's call of upstream's `increment_sequence` is protected by the
  oracle's Part A (X4 killed) and a wrong extra raise by its step M2 (X3 killed). M7b does not
  protect the call, since the scheduling path raises SEQUENCE itself. The override branch inside
  upstream's function is an accepted residual (X2).
- **The oracle (Task 38).** Items and the mail's calendar part are compared as
  `normalize_jscal(jscalendar_of(tree))`, since by D1 a JMAP write stores the converter's form;
  if wrong, a property the converter drops would pass (statuses and pairs are compared
  separately). The script's texts carry an explicit SEQUENCE (0 at the create, one higher at
  each organizer step that changes the text), and M7b leaves it to the server. M6's pairs name
  the invitee and the outside address. Open item 9.

### Sharing and sync

- **D15** is a sync-correctness rule, not an access control (confirmed by the owner on
  2026-11-20): a client-chosen origin only makes the server answer from log records, which hold
  item ids, never content. If wrong: a grantee that forges a state learns ids changed or
  destroyed before its grant or rename.
- **C4617611565, with RA-1 and fix round 2.** A caller who is not the owner of a key account gets
  the snapshot's highest change id from `/get`, `/query` and `/set`, and from a `/changes` or
  `/queryChanges` page that read the log to its end from an initial or an exact state (the tip of
  a snapshot read before the page). An exact-origin continuation is
  raised to the tip only when the log holds no event change between its last item change and the
  tip; an initial-origin continuation is never raised. Cost: the grantee's state moves on a
  container-only change; one more log read per poll when a container change is the last record
  (RA-15); one log read on the last page of an exact-origin paged sync; residual (a) of spec 6.8.

### Failures classified as timing

- **Upstream's `queue_retry`** (the second matrix pass's row 15): the controller's classification
  is upstream's timing flake under the host's load, not a finding of the release. The two
  failures differ (`[1, 2, 2]`, `[1, 1, 3]`), the branch changes no file under `crates/smtp`,
  `crates/smtp-proto` or `tests/src/smtp`, and the test is on the list CI reruns by name.
  Afterwards it ran alone ten times on `3b2d21864` (the source of `a5e45dc2f`): 8 passed and 2
  failed, at a one-minute load of 20 to 30. It was not run on the base commit, so the base's rate
  is not measured, and the cause was not investigated. Cost if wrong: a queue-retry regression
  passed as a flake; CI's SMTP step would show it. In the gates job's net on `f559febdb` it
  failed in the whole run and again when the net reran it by name, classified the same way
  ((e) under "CI and the gates").
- **Upstream's `calendar::alarm` in key mode** (row 24 on `25e098520`): a timing failure, not a
  result of the round. Its first alert is due about one second after the event's create, at a
  load of 17 to 23; the suite passed in plain mode in the same run and in key mode on `7304f9236`
  and `7bd35c4c4`; what changed under `crates/` after `7304f9236` is not on the alarm path; two
  later runs passed. Limits: one failure in three runs, the base's rate not measured. Cost if
  wrong: a regression of the alarm path in key mode passed as a timing failure.
- **Upstream's ordinary-account thread merge**: plain `jmap_tests` once in Task 24, key mode once
  in CI on `68d905131` (b).
- **Upstream's DKIM selector assertion**: (a) above; three occurrences, the third in pull request
  #98's own run.
- **The 500 ms DAV client timeout** (`tests/src/utils/webdav.rs:100`): a helper timeout in a
  module the round did not touch, at a recorded load spike, rerun once (fix round 1 of the first
  review; row 10 of the second matrix pass).

### The gates' toolchain

- **The pin's scope** (history): that item 26's pin was made in pull request #90, in the gates job
  only, was the controller's ruling; this branch removed that pin after the merge.
- **The removal's version line and place** (history): keeping the compiler's version line after
  the pin's removal, and making the removal a commit of its own in the consolidation pull
  request, were the controller's rulings; the removal did not survive the merge with `main`.
- **`main`'s workflow in the merge.** Taking `main`'s workflow in the merge (`c08595afc`) is the
  controller's ruling, reported to the project owner: `main`'s pipeline shares one instrumented
  build across jobs and so needs one compiler. Cost if wrong: he wants the retention jobs unpinned
  regardless; that is a change to the pipeline of #89, not to this branch.

### Tests that must keep their form

- **R31-1.** `update_applies_sequence_floor` uses a grantee-organized event (CalDAV `SEQUENCE:5`,
  JMAP `sequence: 1`): with an owner-organized one it passed with the floor removed, since
  upstream's increment already stores the stored SEQUENCE plus one there.
- **R40-1.** The leak module takes the authorization code with the label-only form, since
  `unwrap_code()`'s panic would print a consent token with its hand-off secret.
- **FA-2.** The D27 case lowers the ban rate to 5 for one forwarded address and makes six
  refusals; with fifteen refusals against a ban rate of 100 a day it could not fail.
- **Fix round 2's H2.** The parked CalDAV `DELETE` of `destroy_fence_lost_to_a_crossing_write`
  asserts no `Retry-After` beside its 503, which tells the fence's refusal from a lost assertion
  and pins the lost-flag arm.

### Mutation and kill checks

Areas with kill checks: Task 42's mutation sample (spec 9.7's 28 rows, J1 to J26, every one
killed; the controller's X1, killed after the sharing module's move case was strengthened in
`de67af5df`, X3 and X4 killed, X2 an accepted residual); kill checks in Tasks 10, 11, 12, 17, 26,
29, 31, 36, 37 and 38 and Task 40's two negative controls; the final review's fix wave (K1 to K6,
K6b); the first review's waves A and B and their fix rounds (every check made the suite fail but
x1-kF3b, which fix round 2's H2 closed); wave C (cache and calendar parts) and its fix round;
waves E and F. D21 has no row, since its fault cannot be made without adding a read (spec 9.7);
`removal_write_asserts_the_record_alone` and `forget_grant_asserts_the_record_alone` test it. D28
has no fault in spec 9.7's list; `calendar_set_error_table`,
`calendars::store_failure_stops_the_call`, `event_set_error_table` and the two mail-guard cases
test it. The attachment download's denial is an equivalent mutant at suite level (R37-12). Every
table is in the history ("Where the full record is").

## Reviews

The pull request's four reviews were an external reviewer's claims, verified against the code
before anything changed, by the project owner's rule from the first round. No Critical or
Important finding is open. Every reviewer read only and ran nothing; the pull request's reviews
were source reviews without builds or tests. The second whole-branch pass, by its own list, did
not cover the second half of `copy_move.rs`' diff, the test hooks and the unit tests, the
unchanged code the branch calls, most test modules, spec sections 7, 9 and 11 to 13, cluster
behaviour, or the SQL and FoundationDB back ends beyond their write order.

| Stage | Findings | Critical or Important open |
|---|---|---|
| The task reviews | 222 deferred lines, 40 coverage notes | none |
| Final whole-branch review, `5cadcd739` | 0 C, 8 I (6 distinct), 78 Minor, 1 withdrawn | none |
| Its fix wave's re-review | 5 Minor (W-1 to W-5) | none |
| Review of this note's record of it | 18 (R44-1 to R44-18), corrected | none |
| Pull request review 1, `dbd2fccc8` | 12: 3 P1, 7 P2, 2 P3 | none |
| Wave A's review; two re-reviews | 0 C, 4 I, 13 Minor; then 3 and 1 Minor | none |
| Wave B's review; its re-review | 0 C, 2 I, 17 Minor; then 2 nits | none |
| Pull request review 2, `d6929c89e` | 5: 1 P1, 3 P2, 1 P3 | none |
| Wave C, cache part; re-review | 0 C, 0 I, 13 Minor; then 4 Minor | none |
| Wave C, calendar part; re-review | 0 C, 1 I (RC-1, fixed), 11 Minor; then 2 | none |
| Wave D (coverage); re-review | 0 C, 2 I (RD2-1, RD2-2), 20 Minor; then 6 | none |
| Pull request review 3, `68d905131` | 3, each P3 | none |
| Second whole-branch pass, `68d905131` | 0 C, 1 I (FR-1), 6 Minor | none |
| Wave E's review | 0 C, 0 I, 4 Minor | none |
| Pull request review 4, `8e05a6b44` | 2, each P3 | none |
| Wave F's review | 0 C, 0 I, 8 Minor | none |

C and I are Critical and Important; P1 to P3 are the pull request's reviewer's own ratings, and
every one of its findings was confirmed. The final review ran as five lenses (minors counted with
duplicates; FC2-4 withdrawn); its five fixed Important findings and FC1-1, decided by the owner,
leave none open. RC-1 and RD2-1 were fixed, RD2-2's case removed, FR-1 decided with no change.
Review numbers: 5981013324 (second), 5994824096 and 5994832045 (third), 5998450430 (fourth).

The final review's six distinct Important findings: FA-1 = FD-2 (a second-factor change did not
end sessions for good), FA-3 = FD-1 (the hand-off secret and the code reached the request-URL
trace), FA-2 (the D27 case could not fail), FC1-1 (an attendee's create sends no reply), FC1-2 (no
test of a failed create or of kinds crossed in one call) and FC2-1 (a grantee's text query matched
fields `/get` hides). The fix wave: `3f1ce7dc5` (FA-1), `f1c69900d` (FA-3), `f0150ec57` (FC2-1)
and `a5e45dc2f` (P4, the `OAuthGrant` arm evicting resident keys, the compile-time equality of
`CODE_LEN` and upstream's `DEVICE_CODE_LEN`, comments, evidence strings and cases T3 to T11).

## The pull request's findings

| Id | Finding | Fixed in, unless stated |
|---|---|---|
| C4617611506 | a calendar destroy's child list is not fenced (inherited) | `efb845a4b`, `6fbe81a1a`, `0a6139d7a` |
| C4617611521 | a calendar destroy can split across commit points | `c7dfbce95`, `6fbe81a1a`, `7304f9236` |
| C4617611549 | a grantee can change an event private under a sealed `CLASS` | JMAP only: `461f0dd19` |
| C4617611565 | the event state after a sharing boundary is unusable | with a limit: `5f736c77f`, `cf50cd2bb`, fix round 2 |
| C4617611574 | no UID lease when an event moves in (FB-3) | spec 3.3 `53cf84aa9`; #91 |
| C4617611584 | the removed href from a request-start cache | `c65c38347` |
| C4617611594 | the drain before the capability check (FC2-3, R34-4) | `6f0704dcb`, `0ca47a61a` |
| C4617611602 | the record cleaned before the sessions are evicted | `d50107f12` (closes FA-16) |
| C4617611615 | a keyed entry not bounded by the signed expiry | `dff25d1d9`, `7bd35c4c4` (closes W-1) |
| C4617611630 | UID conflict answers depend on unseen events (inherited) | no code change |
| C4617611635 | one refused occurrence fails its siblings (FC1-7) | `d86f90b31` |
| C4617611650 | this note's description of R28-4 | corrected in the note |
| C4633314478 | a child moved out and back during the build | `11a681bbd`, `cef1884a5` |
| C4633314492 | an event with two names keeps the second | both ends: `b4d640c3c` |
| C4633314503 | a second event at one href is not listed | the delete: `85c23c154`, `8e9baf7b7` |
| C4633314510 | the fence's operation added after the size check | `45fa3a034`, `2f00c2abf` |
| C4633314515 | a cached entry outlives the signed expiry (inherited) | keyed entry: `90d6ad286`, `294ebba20` |
| C4644031673 | a discarded membership pass on destroy | `a65717ae9` |
| C4644031680 | a refresh case accepted `Lost` or `Superseded` | asserts `Lost`: `6c5c46b26` |
| C4644038016 | a hand-off case could pass on a generation mismatch | `6c5c46b26` |
| C4646937244 | an expired new access entry removes a live one | `20ca477f4`, `5ecae78c4` |
| C4646937253 | this note's unit totals swapped between crates | corrected in the note |

Fix round 2 of C4617611565 is `5e0e8620b` and `25e098520`. C4617611521 is not inherited for
JMAP; C4617611506 is inherited from upstream in a wider form. C4633314503's name fence on creates
was not taken.

Wave D (tests only): `3a7d5b474`, `c71aba81d`, `c7666dd8b`, `97b1fa457`, `ce01e0f3a`,
`f3630cf67`, `67797004b`, `0c73eafc4`. Wave E's FR-3: `20f4cf9d2`. The CI rerun: `bea8a39fb`.

## Accepted residuals

- **Spec 3.3, in substance.** Each object of a `/set` is its own commit (D3); an unreachable or
  withheld recipient records its status and does not fail the object; an unexpanded text query is
  matched in memory; UID uniqueness is CalDAV's rule; destroying a calendar sends cancellations
  (D16); no drafts, per-user event data or participant-visibility flags; default alerts by value
  (D11).
- **Spec 3.4.** A CalDAV colour and JMAP's `color` stay apart; an event written with
  `sendSchedulingMessages: false` and edited through CalDAV is scheduled from that edit on;
  upstream's conversion decides what survives a round trip, alert-only and keyword-only updates
  included (issue #96); JMAP does not check a calendar's supported component set. Only the round
  trip has a test.
- **4.5's interrupted refresh.** If the grant advances and the record's write does not follow,
  the superseded secret and the stored record still open MK until the next write of the record,
  the client's retry or the sweep, at most one maintenance interval. Upstream refuses the
  superseded token itself at once.
- **D22.** A keyed grant whose entries are gone while its own deletion failed refreshes as a
  keyless grant when presented without the secret; it opens nothing.
- **D7 at a cache hit.** A node that has a token's keys cached serves it until that entry ends,
  even if the record's entry was dropped by the cap of four; a grant's deletion evicts it at once.
- **Section 2.** The token record is new at rest, and an entry can outlive its token until the
  next maintenance run. The hand-off stores MK wrapped under a secret inside the code for the
  code's lifetime. A browser holds a credential that opens the account's keys for as long as its
  grant lives (upstream's default: thirty days without use). Whoever serves a web client's code
  can read what the user reads.
- **Ordinary-account costs** (spec 10): one cached account read at a code exchange, a refresh and
  a grant's deletion; one in-memory delete at a consent confirmation without a secret; two cached
  classification reads for `Calendar/set` and `CalendarEvent/set` (R28-8, R30-3) and three for a
  calendar `/changes` (R22-1), where spec 10 counts one. D29 adds state-codec fields and context
  checks, with no global account lookup and no additional ordinary-account store I/O;
  key-calendar pages carry the origin fields and classify the account when choosing the
  continuation format.
- **The legacy grantee restart** (6.8): a grantee's legacy `r` continuation of a key calendar
  answers `cannotCalculateChanges`, and the client restarts from Initial.
- **4.3, a retried refresh.** The next generation's entry can outlive its token by the retry's
  delay; the token's own expiry and the sweep bound it.
- **5.2.** Upstream stamps `updated` on an owner's update, so most JMAP moves ask `ModifyItems`
  as well; the two share levels cannot tell the difference.
- **Partial coverage.** The refresh steps of 4.5 have a unit test for the classification only
  and otherwise run in the za suite, where `tokens::server_token_answers` reaches and asserts the
  `Lost` arm (kill check e-k1); the pass-through of a foreign Bearer value (D26) has a unit test
  only.
- **R27-1, rollback.** Every binary built before release 6 refuses a coloured personal bundle, so
  a binary rollback below release 6, or a mixed-version cluster, leaves a coloured shared
  calendar unreadable to the older binary.
- **X2.** The override branch of upstream's `increment_sequence` is covered by no test in this
  fork's net, for any account.
- **The final review's residuals:**
  - FA-11: an ordinary account's consent token followed by a 43-character suffix of the secret
    alphabet is accepted, the suffix stripped (an every-account difference in `CLAUDE.md`);
  - FA-14: the token sweep runs only when the purge steps before it succeed;
  - FA-20: a node that missed the invalidation broadcast of a vault write that leaves the binding
    unchanged refuses the account's uncached Bearer requests (a refresh does not help; Basic is
    served) until another invalidation reaches it (cluster only, unqualified);
  - FB-2: a create without a `uid` whose commit fails after its invitations went out is retried
    under a new UID, so attendees hold an event the organizer never stored;
  - FB-7: the placement and destroy batches assert in CalDAV's order at the base;
  - FC1-6, FC1-8 (= R30-2, R31-4), FC1-12, FC1-13, FC1-14 (= R31-3), FC1-15: the event writer's
    answers and check order beside upstream's, as the plan prescribed; nothing stored in the
    clear;
  - FC2-7 (= R21-8, R26-5), FC2-8 (= R26-7): an unranged text query holds its opened views for
    the request, and the store's leaf limit is checked after the candidates open;
  - FC2-9 (= R21-5): one shared calendar that does not open fails a grantee's whole
    `Calendar/get` and its exact-origin `/changes`, where the owner gets `notFound`; it weighs
    more than its rating, since a browser client loses the whole sharer account;
  - FC2-10, FC2-13 (= R22-3), FC2-14, FC2-17: `Calendar/set` reads its context before upstream's
    validation; a non-canonical `z` is accepted and echoed re-encoded; an owner's get that
    completes a sharing boundary returns the state sampled before its write, so the next
    `/changes` reports that calendar as updated; an owner's own sync completes none;
  - FD-13: store errors on the issue and refresh paths keep their context in the trace;
  - FD-16: `za_handoff_take` deletes the hand-off before it checks the secret, so a wrong secret
    consumes it;
  - W-2: `sharing::revoked_and_unshared_calendars_hidden` cannot fail on the fork's fresh-context
    filter (an equivalent mutant at suite level); its comment overstates;
  - W-5: `traced_url` masks a `code` query parameter as a standard parser reads one; one
    percent-encoded inside another parameter and a `;`-separated query pass unmasked; the server
    generates neither;
  - FC1-10, FC2-11, FA-9, FC2-12: the key writers repeat upstream's inline checks, and several
    insertions are larger than one call; the code stays, and `docs/zero-access/release6-code-map.md`
    names them.
- **The first pull-request review's residuals:**
  - RA-4: the calendar delete's fence can refuse after the cancellations went out; key attendees
    then hold a CANCEL and the outside attendees' mail is dropped, until a retry, which converges;
  - the store ruling: on FoundationDB, a read replica and the ephemeral store a key calendar's
    delete stays chunked and unfenced, without the unwritten-child or size refusals, and a failed
    later chunk leaves the earlier chunks committed (iMIP spec 5.4's residual for those stores);
  - RB-3: a keyed token whose scope excludes mail still runs the search pass through a `Blob`
    call;
  - RB-4, RB-5: what a token's scope admits on DAV is upstream's scope model, unchanged; the
    drain's scope rule limits the drain only;
  - C4617611630 (inherited): UID privacy-conflict answers to a grantee depend on events it cannot
    see;
  - spec 6.8 case (a): a paged sync from the initial state is never raised, so it ends at the
    range's last event change and below any sharing boundary above that change, whether the
    boundary completed while it paged or before it began; its next call is refused, and a restart
    that pages from the initial state again ends at the same state, until an event is written,
    the client syncs in one page, or it takes its state from `/get` or `/query`. Nothing is
    skipped, and a client that follows the protocol holds the state `/get` gave it and never
    enters this flow; for this one flow the fault the pull request's review named remains. It is
    the cost the controller's ruling accepted (issue #93);
  - the early drain: an account that lacks a calendar method's JMAP permission still drains on
    the call the dispatcher then refuses, and a call whose back-reference fails has drained.
- **The second pull-request review's residuals:**
  - inherited: no write fences an event's name, so two events can be stored at one href; the
    calendar delete no longer leaves either behind;
  - inherited: an ordinary account's cached Bearer authentication can outlive the signed expiry;
    a key account's entry no longer does;
  - inherited (RC5-6): a forward step of the wall clock after publication can leave an entry live
    past the signed expiry by up to the step;
  - RC-10: release 4's keyed collection `COPY` with an overwrite still lists the overwritten
    calendar's children from the path index and removes each event's first name.
- **The third round's residual** (inherited, accepted by the owner on 2026-11-20): a caller who is
  not the owner receives in `destroyed` the ids of the owner's events it may not see, when those
  events change or are deleted; ids only.
- **The fourth round's residuals:** RF-2, an access entry's expiry can be one second later than
  its token's signed expiry, so an entry written in that second is stored though its token is
  refused; RF-3, on a cluster whose clocks differ by more than the write's overrun, a node with
  the lagging clock refuses a token whose entry was not stored, and the client refreshes.
- **Inherited, one line each** (from upstream or from the base): FA-22, the device-authorization
  answer is traced with its device code, and a retry while a rotation is pending returns the next
  generation; FC2-16, JMAP destroys the default calendar that CalDAV's DELETE refuses in a product
  build; an owner's JMAP update of alarms, keywords or colour only runs unscheduled while CalDAV's
  bumps the schedule tag; a CalDAV DELETE by a non-owner tests ownership on the stored tree; an
  unanswered attendee's JMAP create never schedules its copy; the first of two updates of one id
  in a call is lost; a grantee's destroy in an unshared calendar answers with the calendar's id;
  `/queryChanges` with `expandRecurrences` answers `cannotCalculateChanges` before the access
  check; a `z`, like upstream's `r`, is bound to no account, collection or method; a windowed or
  property-subset read and a full read show default alerts differently; for CalDAV only, a large
  collection delete of an ordinary account can commit in part after its cancellations.

## Deferred findings

Each with the id it has today; the note as it stood at the merge (in the history) holds each
deferred finding in full, except the few this note marks as held by id only. "Weighs more"
marks one a lens of the final review found heavier against the whole branch than at its task.

Removed from this list as no longer true at the head, on the final review's triage reading
(fixed by a later task, or the owed record delivered): Task 18's N1 and O1, R21-10, R22-8,
R23-4, R23-5, R26-8, R30-9, R30-11, R32-2, R34-5, R35-3 and R38-12; and, closed by that review's
fix wave, Task 17's length-constant line, R30-1, R32-1, R37-14, R40-8, and R41-2 with R41-3.
On this note's reading, as Task 43's additions show, R22-4, R27-1, R29-7 and R37-16 were
removed as deferred lines because the record they owed was delivered: what stopped being true is
that it was missing; each now stands under "Merge hotspots" (R27-1 also as an accepted
residual). R28-4 and R30-10 were rewritten to what still
holds. Closed after their task: R31-2 (= FC1-7, `d86f90b31`), R34-4 (= FC2-3, `6f0704dcb`), FA-16
(`d50107f12`), W-1 (`dff25d1d9`), wave B's heading nit (`90d6ad286`).

### Tokens and sessions

- (Task 17) A device-code poll whose `device_code` is the pending user code answers
  `expired_token` for every account where upstream answers `authorization_pending`; no token was
  obtainable that way. In `CLAUDE.md`'s every-account differences.
- (Task 17) A failed hand-off cleanup on the classification error is discarded without a trace,
  as briefed.
- (Task 17) Destroying a key account leaves its outstanding code and hand-off in the in-memory
  store until exchange or expiry (ten minutes).
- R18-2 (= FA-17): a credential change before the refresh's first read answers `Lost`, not
  `VaultChanged`; spec 9.2 allows either.
- R18-5: `delete_keyed_grant`'s failure text says "unissued OAuth grant not deleted" on the
  refresh path, and the `Lost` trace says "deleted" before the deletion is attempted.
- R18-6 (with FA-13): `Superseded` and `VaultChanged` refusals emit no fork trace, where spec 8
  says a refused token open is traced. Weighs more: an operator has little to diagnose a failing
  browser session with.
- R18-7: a well-formed secret appended to a keyless grant's refresh token makes the precheck
  answer `Keyed`, and the `Lost` arm deletes that grant (item 7).
- R18-8 (= FA-6): an unusable token record makes each refresh delete its grant while D8's sweep
  leaves the record.
- FA-5: the D22 precheck's answer when the token record cannot be read.
- RB-12: resident keys can outlast their authentication entry; unreachable.
- RB-18 (inherited): the exits of a grant's deletion that find the grant already gone clean its
  entries but do not evict this node's cached sessions.
- RB-19 (inherited): a lost broadcast leaves a peer serving a deleted grant's token from its cache
  until the entry's TTL or the keys' idle limit.
- RC5-8: a keyed request in a token's last seconds is verified in full and charged to its
  address's anonymous rate limit (spec 10; README-dev gives the figures).
- (wave D's re-review) A second issue for a grant returns a refresh secret no entry holds, and
  after a rotation would add a generation-0 entry below the stored generation (invariant 67); no
  product path reaches it (issue #95).
- (coverage note) The `Lost` trace is unobserved; R18-1's window is wider on networked stores.

### The pipeline and the calendar delete

- R28-3: `za_write_default` reads the previous default after `za_delete_calendar` may have
  cleared it; the effective default is the same.
- R28-4: after a calendar destroy commits, a failed cluster broadcast is only logged; the destroy
  answers `destroyed`, and a peer keeps the grantees' cached entries for the deleted calendar
  until they expire, while every read finds it gone.
- R28-5: after a failed post-commit read of the stored default the fallback can report
  `isDefault: true` wrongly; two traces mislabel.
- R32-8: an event with two names in one calendar loses its second name on a wholesale
  `calendarIds` with the same set, with no vanished href; upstream folds names the same way.
- R32-9: `.filter(ZaPlacementPlan::is_change)` never drops a plan.
- R33-5 (= FB-5, with FC1-9): a destroy with scheduling of a future scheduled event that does not
  unseal is answered as the retryable D28 case and stops every later id; without the flag it
  succeeds.
- FB-4: the placement arm's exits end the mail guard as `ZA_IMIP_ABANDONED` where
  `ZA_IMIP_DROPPED` is true; nothing is mailed either way.
- RA-10: the fence's store property is pinned on RocksDB and SQLite only; PostgreSQL and MySQL
  hold it by reading (MySQL's abort rests on its driver rolling back a dropped transaction, not
  read). A run of the unit test against each would close it.
- RA-11: on RocksDB a large one-transaction delete is retried whole on a conflicting commit of the
  account, up to 24 times or 10 s, then fails after the fan-out (issue #92).
- RA-12: the placement loop's new early return leaves the mail guard unsettled; unreachable.
- RC-3: with a name the path index hides, a keyed `COPY` or `MOVE` into that calendar is not seen
  by the UID check, so two names in one calendar stay reachable through the inherited name race,
  and a single-name delete then removes the first name under the calendar.
- RC-9: a name whose calendar no longer exists counts as a name that keeps the event.
- RC-10: release 4's keyed collection `COPY` with an overwrite (also a residual).
- FR-6: `onSuccessSetIsDefault` writes the default id with no assertion, so a delete of that
  calendar in between leaves a default naming a deleted calendar (a comment on issue #92).
- RE5-3: the owner's destroy still runs upstream's loop over the children from the path index.
- FD-15: `ZaPipelineError` derives `Debug`; no use site formats it.
- (coverage notes) `za_repair_owned_context` and R28-4's reachability were not read; after the
  fan-out `za_commit_update`'s error returns end the mail guard as `ZA_IMIP_ABANDONED`; upstream's
  draft gate and hidden-participant path are not mirrored; the placement UID check holds no lease
  and no fence (issue #91), and on the plan retry `za_placement_contexts(..).await?` returns
  without `za_mail.dropped()`.

### The JMAP surface

- R21-6: two 20-line inline blocks in upstream's `calendar/get.rs`; its re-trace arm nearly
  duplicates `za_read.rs`.
- R21-7: the handlers classify the account again apart from the boundary's decision.
- R21-8 (with FC2-7): the view memo never hits in `CalendarEvent/get` and keeps every opened view
  alive to the method's end.
- R21-9: the event row read follows the content read; a concurrent update can give a spurious
  trace and a transient `notFound`.
- R22-1: three account classifications per calendar `/changes` request.
- R22-5: the `invalidArguments` text is defined twice.
- R23-1: `let mut` edits an upstream line in `participant_identity/set.rs`.
- R25-4: the unzeroed `ContentRecord.terms` and term keys are request-lifetime plaintext in
  process memory, allowed by invariant 60.
- R26-1: a 38-line inline block in upstream's `query.rs` repeats `query/za.rs`' view opening.
- R26-6: two sequential store reads per candidate; a networked store could pass the client's five
  seconds on a large calendar.
- R28-6: a create checks `shareWith` before metadata, an update the reverse; both `forbidden`.
- R28-8: ordinary accounts get two cached classification reads per `Calendar/set`.
- Task 28's F1: `calendar.clone()` copies the whole unsealed calendar for `accept`, for parity;
  Task 28's F2: the head store error is rebuilt with "calendar read failed" details on a write
  method.
- R29-6: the overflow `false` return comes after the edit has started; unreachable.
- R29-8 (item 6): replacing alarms by value drops acknowledged or snoozed state upstream keeps.
- R29-9: the brief added a member fallback the spec lacks.
- R30-3: two classification reads per call for every account.
- R30-6: the unreachable missing-UID arm is a check of the fork's own.
- R30-8: the store-failure trace says "write failed" for read failures too.
- R30-10: the key writer's head reads the account twice when the call has creates.
- R30-12: with duplicate `useDefaultAlerts` keys any true wins.
- R31-11: the unreachable instance `ImportedValues` branch is kept without a comment.
- R31-15 (item 13): a grantee-organized event's JMAP update takes another path than its CalDAV
  twin; with scheduling off it gets no increment and no `updated` stamp.
- R33-9: `destroy` takes `&mut self` and mutates nothing.
- R35-1: `za_shown_capability(is_key, false, key)` is a no-op, and its doc comment describes
  something it does not do. R35-2: the `&& !is_key` in upstream's expression has no comment.
- R35-5 (inherited, medium confidence): `Principal/getAvailability` for a key principal in an
  ordinary group with calendars returns the group's busy periods while `mayGetAvailability` is
  false. Weighs more.
- FC1-3: a repeated update of one event id in one call.
- RA-13: a non-member's destroy of an event whose view does not open answers `notFound` before
  the calendar checks; JMAP is the stricter side.
- RA-17: upstream's `calendar_event/get.rs` lost an import line, a new merge point.

### Sharing and sync

- RA-15: a grantee's poll loses upstream's no-change shortcut when a container change is the last
  record: one more log read per poll.
- RA-16 (inherited): on a cluster a stale resident snapshot can answer a state below the one the
  client sent.
- Spec 6.8 case (b): a grant that commits between a call's boundary check and its read of the tip
  leaves a raised state above the grant, of an exact request or of an exact-origin continuation's
  last page; reading the tip first would close it (issue #94).
- (coverage note) Upstream's paging of `/changes` after a log trim is inherited unchanged
  (item 8); `/queryChanges` origin forwarding has its runtime evidence in Task 26.

### Tests and harnesses

- (Task 17) `PKCE_VERIFIER` is duplicated from `tests/src/utils/oauth.rs`; the device-code guard
  has no observed failing run.
- R18-3 (= FA-18): run 2 of `refresh_crossed_by_credential_change` cannot catch an omitted vault
  assertion. R18-4: no suite case presents a well-formed wrong secret through the endpoint or
  reads the `Lost` trace, and three-attempt exhaustion is unexercised. What later coverage
  reached is the classifier's `Lost` arm, through the server function: since the coverage wave
  the suite case `tokens::server_token_answers` reaches it, and since wave E it asserts `Lost`.
  Task 18's N2: no za case reaches the `Superseded` arm.
- R18-9: parked refreshes hold a five-second client across Argon2 vault writes.
- R18-11: `change_race_password` repeats `change_password`; two blocks of `za_token.rs` repeat.
- FA-8, FA-19: test gaps on the token paths (the note at the merge gives them by id only).
- R21-3 (with R36-4): no test protects the grantee filter in `calendar/get.rs` or the
  try-next-calendar arm in `za_read.rs`; a grantee's `shareWith`, the clear fields of 6.7 and the
  description are unasserted.
- R21-4: the R21-1 regression subscribes to `DataCorruption` only and does not show that no other
  event carries the canary.
- R21-12 (= FC2-18 = FD-4): the served-methods loop in `gating.rs` passes on
  `serverUnavailable`.
- R22-2: the grantee sees one calendar, so its paging asserts nothing about continuation pages.
- R22-6: no test reaches the in-process `z` arms of `changes/get.rs`, `changes/za.rs` and
  `participant_identity/changes.rs`.
- R23-6: no case covers a grantee's identity `/set` reaching upstream's `reject_identity_changes`.
- R24-1: no test of a grantee parsing the owner's blob; R24-2: the inbox check is absolute; R24-3:
  "the method writes nothing" is not asserted.
- R25-1: no test of the `Unresolvable`/`Corrupt` mapping; R25-2: every test uses English; R25-3:
  no row covers the Owner field's second source.
- R26-4: nothing checks that a query without a text condition opens no event. Task 26's N1 to
  N3, O1 and O2: the expanded parity compares starts only, never asserts a non-empty answer,
  asks flat ANDs as one condition, checks one negation, and the rebuilt-count check does not
  attribute events to methods.
- R27-2: no golden vector pins the bundle layout; R27-3: no unit test pins `open_shared` refusing
  a colour; R27-4: that a rollback keeps the colour is shown by reading only.
- R28-7 (= FC2-21): no commit or default-write store-failure case, no D16 decline or denied-status
  case, invariant 58's clear fields unchecked, no metadata refusal case.
- R29-1 to R29-5: the `SUMMARY:Mail` canary passes from the VEVENT's own SUMMARY; the empty-summary
  filter is never exercised; no VTODO or nested-VALARM case; the root's references unchecked; the
  fork's choices untested.
- R30-4: `create_two_objects_one_fails` cannot tell per-object commits from one batch; R30-5: no
  UID case in a non-first calendar, and several cases skip spec 9.2's CalDAV readback.
- R31-5 and R33-1: the D28 trace of an update's and a destroy's store failure is shown by reading
  only. Weighs more: a regression that traced the original error would pass.
- R31-6 (rest), R31-9, R31-10, R31-12, R31-13: the other base-patch rulings untested on update;
  the status comparison passes if both are empty; two refusals never read back; the
  `hideAttendees` refusal sits inside another case; no owner-organized floor input.
- R32-3 to R32-7, R32-10: D28 on the plan path has no case; nothing reads the event as the owner
  after the move back; one commit and two are not told apart; no grantee placement through
  `CalendarEvent/set`; `content_changed` unobserved; `Places::finish` checks only the 200 of
  `shares/revoke`.
- R33-2 to R33-4, R33-6: the own-items leg and `may_write_own` cannot be reached through the
  product and have no test; `InstancePlan::DestroyEvent` is untested;
  `no_itip_task_ever` still deletes over CalDAV; a byte comparison cannot tell "not attempted"
  from "attempted and failed".
- R34-3: no case triggers the drain with a Bearer token that carries a secret. Weighs more,
  though the hook's condition is the same for Basic and Bearer.
- R34-6, R34-7, R34-11, R34-13: discarded replies, no pending-item assertion before the query,
  `clear_inboxes` asserts only that none remains.
- R35-4: no failing run for `mayGetAvailability` left true.
- R36-5: `grantee_default_alerts`' first event cannot tell the grantee's copy from the owner's
  live entry; the update path's writer argument has no grantee case; no za case sends
  `useDefaultAlerts: true` in an update.
- R36-3, R36-6 to R36-8, R36-11 to R36-14: refusals asserted by type only;
  sink mail counted without UID; inboxes emptied wholesale; `new_items_for`'s bool always true;
  `assert_account_not_found` has no in-helper control; `SENT-BY`'s property and owner address not
  pinned; inboxes unchecked; new pending items counted by UID only.
- (coverage note, Task 36) narrower mutants survive by reading: a quiet link or unlink of an
  event with attendees, a quiet move between unshared calendars or by a grantee, recurring or
  multi-attendee events, the commit's bounded retry on a quiet move; X1's stated consequence (a
  later reply refused) is never observed, and the second quiet move has no kill evidence of its
  own.
- R37-3, R37-6 to R37-11, R37-13, R37-15: item ids compared only; some out-of-scope slots
  unasserted; type-only rows; no read-back after the refused `shareWith`; the keyless session check
  omits `calendars:parse`; `call_using`'s Bearer arm repeats `za_bearer_jmap` and drops the status;
  one more temporary upload is left in the owner's account; the third impersonation form does not
  show keys; the metadata read-back lacks the capability. A stale exact state is never sent as
  `ifInState`.
- (coverage note, Task 37) the `stateMismatch` assertion's strength against a fault that takes
  `to_id` as the state is unconfirmed without a run.
- R38-3 to R38-10, R38-13 to R38-16 (R38-6 = FB-8): the attendee copy's `SCHEDULE-STATUS` unread;
  the reply's acceptance caught through a drain on the 500 ms client (the likeliest flake source);
  "drained" satisfiable by the oracle's own GET; a move not told from a copy at M3; first object
  only; empty comparisons counted; pairs compared by recipient; unset expectations not asserted
  empty; M9 and M7b's SEQUENCE checks one-sided; the METHOD line not set against the MIME
  parameter. The oracle covers no grantee step, sharing, alerts or queries.
- R39-3: several refusals assert only `forbidden`; upstream's `calendar::instance::test` has no
  emptiness check of its own.
- R40-7 (= FD-7), R40-9, R40-13: the trace capture's only control is that it is not empty, and it
  ends before the revocation and the password change; `secret_forms` has no unit control;
  `secret_hits` does not decode LZ4-compressed pickles above 8 KiB, has no URL-safe embedded form
  and reads the logical store only. Weighs more: the leak module is the release's only
  whole-flow trace and store net.
- R40-12, R40-16 to R40-18: the owner's upstream grant deletion does not record its branch; the
  invitation's colour is not read back; the task canary is searched in base64 and hex only after
  the destroy; `events >= 3` where the count is 3; the post-session scans assert only
  `token_records == 0`.
- (coverage note, Task 40) no earlier scan meets a live token record; spec 9.5 words the iTIP-task
  rule as a scan rule while the code checks it in the module.
- FB-9, FB-10, FC1-17, FC2-19, FD-5, FD-9: test weaknesses in the placement cases, the
  occurrence-patch rulings and elsewhere (the note at the merge gives them by id only).
- (coverage notes) the payload-free boundary rebuilds Store errors only; that an external search
  store receives no term is shown by reading `dispatch/search.rs` only; the email-alarm cleanup is
  shown only by the closing `wait_for_tasks`, and the lease race is exercised at one
  interleaving; X2 ran under ZA only.
- R41-5: the plan's step 1 script ignores dispositions and does not test equivalents or rename
  targets.
- W-3: `query::grantee_text_skips_a_sealed_class` has no ordinary twin. W-4:
  `tokens::traced_urls_mask_the_code` exercises one of the three URL sites.
- RB-6: the closed FA-16 exit has no test. RB-7: the keys' re-check after insert and their
  removal after the epoch advances are pinned by no test. RB-15: `dff25d1d9` fails its own new
  case until `7bd35c4c4`, so a bisect can land there.
- RC5-2, RC5-3: the liveness filter and the one-second subtraction are decided by no case. Wave
  C's N1: `expired_second_decode_publishes_nothing` can lose its power over the decode-error arm
  on a very loaded host, silently. Wave C's N2: it arms two pause slots without a drop guard.
- RC-4: no case runs the pipeline near the size cap.
- A failing CalDAV collection `DELETE` is no longer exercised over CalDAV on a fenced store; the
  JMAP case and `chunked_collection_delete_unfenced` cover the function.
- `paged_sync_retention_collision` pins an initial-origin continuation's final state, but no case
  makes the call that follows it.
- The parked CalDAV `DELETE` of `destroy_fence_lost_to_a_crossing_write` uses a five-second
  client; the five-second client of `jmap_calendar::calendars` bounds every parked `Calendar/set`
  of the module; the renamed pipeline case's panic text says "stale child list".
- RD2-18: two lines a raw batch clearing an event's content row might reach; not tried.
- (wave D's re-review) the merged coverage figure reused a profile from `f3630cf67`; the
  disk-quota case's comment claims a pre-check no assertion shows; the confidential-UID check
  passes vacuously on an unreadable body; the contacts refusals' ordinary loop reads nothing
  after; three comments trail their cases; `contacts/forget` with null does not assert the wrap
  mark.
- RF-4, RF-5: no test pins `put_access`'s call site in the issue (passing `0` passes) or the
  retry's fresh time.
- (Task 42) R42-2 to R42-5: some kills were observed at another catcher than named (J7's named
  catcher cannot catch it; J3's kill is the indirect doubled mail; J16b's suite run, J19 and J25
  failed in an earlier case of the same property), and J10's and J11's failing iteration is not
  logged. R42-6: J14's kill is the gate's refusal answer, not the item-survival assertion.
  R42-7 to R42-12: J8's mutant is the brief's stronger form; the runner sets no
  `RUST_BACKTRACE`; J26 is unit-only evidence; J18's arm is slightly broader than the row; J1
  keeps the lease and skips only the UID check; unit runs have no command log, and one batch
  remark about an unused import is wrong. The unreached named catchers are not shown to fail.

### Documents and evidence records

- R22-7 and R31-7: the interface sheet's §7.1 call-site list and the plan header's `za_variants`
  row, and the sheet's §7.2 view-error rule (in `/set`, D28 makes it per object), no longer
  matched the code (the sheet is in the history only).
- RC-7's remainder: the comment on the unfenced form in `crates/common/src/za/test_hooks.rs` does
  not say that the unwritten-child refusal is absent there.
- Wave C's N4: two comments cite review tags. Wave B's re-review: one comment line of `drain.rs`
  is 90 columns.
- FD-11: new fork tests sit in `parser_restored_tests.rs`, which the gates' tooling groups as
  restored upstream tests.
- R23-2, R30-7, R35-6, R39-7, R40-14, R40-15, R36-15, R36-16, R38-18, R41-9: comments, case names,
  panic messages and evidence strings that omit, narrate or name the wrong thing.
- Report records. The ids in this entry are deferred notes on the run's own reports and logs,
  which were never in the repository; their text is in the note at the merge commit. Task 17's
  RED file not kept; R18-10, R32-11, R33-8, R34-9, R35-7 (the environmental linker note and
  `lsof`'s warning in logs); R27-5, R29-10, R31-14, R34-8, R34-12, R36-9, R38-17, R39-8 to R39-12,
  R40-11, R41-1, R41-4, R41-6, R41-7 (the reports' counts, hashes, commands, tables and quoted
  lines; R41-6 and R41-7's rerun gap closed by `bea8a39fb`); R39-9 (two observations with no
  change proposed); no Task 41 receipt records `git status` during the runs; RA-8 (wave A's
  implementer went on where its brief said to stop; ruled accepted); the unit count reported for
  wave D's fix round (914 passed) could not be reconciled from the reports; R32-12, R32-13, R33-7,
  R34-10 and Task 36's re-review nits (test hygiene: an unused `ids[1]`, an `assert_ne!` without a
  message).

## Open for the project owner

Undecided items of the list, with their numbers (the decided ones are under "The project owner's
decisions"), each with the controller's recommendation where one was made:

- 5. FC2-6: a default alert's id, a string the client chooses, is stored in the clear.
  Recommended: accept under 6.7 with a line in the spec; refusing or rewriting long ids as a
  follow-up.
- 6. R29-8: acknowledged or snoozed alarm state is lost with by-value default alerts.
- 7. R18-7: a well-formed secret appended to a keyless grant's refresh token deletes that grant;
  the precheck already holds the record and could refuse before the rotation (FR-5).
- 8. Upstream's paging of `/changes` after a log trim is inherited unchanged, for ordinary
  accounts too; pending, not accepted.
- 9. The two oracle rulings of Task 38 (items and mail compared as the JSCalendar projection; the
  script's explicit SEQUENCE with M7b leaving it to the server).
- 10. Task 39's F1 with R39-5: three consequences of D11 are not in spec section 12 (override
  patches in windowed or property-subset reads, the stale description after a rename, alarms that
  stay after a later `useDefaultAlerts: false`). A constant description would remove the first two
  and needs a change to D11's wording.
- 11. Task 39's F3: section 12 says a view-level grantee "cannot rename or recolour" its entry;
  the check also refuses `isVisible` and `isSubscribed`.
- 12. R41-8: the fresh-store restart script exercises nothing release 6 adds.
- 13. R31-15: a grantee-organized event's JMAP update takes another path than its CalDAV twin;
  with scheduling on it still goes out with a raised SEQUENCE, so the gap is confined to
  scheduling off.
- 14. R43-4: whether release 4's sharing spec
  (`docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:148-150` and `:566-570`) and
  plan 14's outcome note (`docs/superpowers/plans/2026-10-23-zero-access-plan14-outcome.md:439`
  and `:1445`) get a pointer to D14 at their statements of the colour exclusion, which spec
  section 10's amendments do not list.
- 16. FB-6: a move out of two unshared calendars into one shared calendar is refused, though two
  updates reach the same end. Recommended: accept, or a follow-up.
- 17. FC2-15: `shareWith` is refused whatever its value, `null` and `{}` included. Recommended:
  let the reference client's checklist decide (it is in the checklist).
- 28. `main`'s rule ties the retention pin to the deployed product's compiler; `0.4.0-beta.1` was
  built with rustc 1.100.0, so deploying it puts the product on 1.100.0 while the retention jobs
  measure with 1.99.1. Moving the pin to 1.100.0 would follow the rule; on that compiler the
  coverage gate passed on `8e05a6b44` with 62 and 49 lines to spare. Put to him by the
  controller; nothing is changed.

Item 27 (pin the release workflow to the gates' version) has left this list: it stays closed and
was never made, since `main` keeps the product build unpinned on purpose (#103; "CI and the
gates").

Proposed spec amendments not made (spec 3.3's lines for FC1-1, the UID lease and a non-member's
effective privacy, which states FC2-2's difference from CalDAV, and section 10's `code` masking,
are made):

- 3.3: FD-3, a key account's reminder push names a positional alert id (`k1`), since the alarm's
  id is sealed; FC1-5, a grantee's email default alert is mailed to the calendar's owner (or 3.4);
  FC2-16, JMAP destroys the default calendar that CalDAV's DELETE refuses (inherited).
- 3.3, 3.4 and 3.5's last sentence (R38-11): a JMAP write stores upstream's converter form, stamps
  `CREATED` and the update stamps, and raises SEQUENCE for a client that omits it.
- Section 10: the device-code poll's changed answer; FA-11; the costs (the sweep's read of field
  153 per account in every purge run, FA-12; a pass over the key cache on any grant deletion; the
  classification reads counted above).
- D8's sentence (FA-6 = R18-8): a refresh over an unreadable token record deletes the grant.
- 4.5 step 5 (FA-17 = R18-2): a refresh crossed by a credential change before its first read is
  `Lost`. 4.5 (R18-7): a keyless grant presented with a well-formed secret.
- Spec 8 (FC1-16): a grantee's update in an unshared calendar answers `notFound`, not `forbidden`;
  and whether `Superseded` and `VaultChanged` refusals are traced (R18-6).
- 6.2, 6.10, 6.11 (R35-5): "an empty list" holds only for a key principal in no ordinary group
  with calendars.
- Section 12: D11's three consequences (item 10); 6.7's wording (item 11).
- Spec 10's amendment list and the sharing spec's colour statement (R43-4, item 14).
- 9.5: the iTIP-task rule is checked in the module, not as a scan rule.

Follow-ups not taken, each recommended by the controller:

- R18-6 with FA-13: three token refusals emit no trace, and a store error on the Bearer key path
  is answered 401 with its cause dropped. Recommended: a server error, traced. Not taken because
  it changes what a client sees under a store failure.
- FB-5 (= R33-5) with FC1-9: an event that does not unseal is `notFound` on update, a store
  failure on a flagged destroy and destroyed without the flag. Recommended: per object on destroy
  too.
- R28-4, FA-5, FB-4, FC1-3 (above).
- A guard in the vault commit funnel that refuses a binding change under an unchanged salt, which
  would make FA-1's fix a checked property of every vault write.
- The account page's text about how long keys stay in memory describes password clients only
  (spec 12; outside this repository).

## Follow-up issues

- **#91**: a placement change checks UID uniqueness without the UID lease, which covers creates
  only (C4617611574 = FB-3; spec 3.3 corrected in `53cf84aa9`).
- **#92** (item 23 of the list for the owner): the calendar delete. Notes of 2026-11-18 (this
  pull request makes it one transaction) and 2026-11-19 (what it keeps: a calendar too large
  for one transaction, the stores that keep the chunked form, the fence's refusal after the
  cancellations, RA-4, a large delete retried whole on a conflicting commit, RA-11, PostgreSQL
  and MySQL read, not run, RA-10), and a comment of
  2026-11-20 on a default-calendar id written across a concurrent delete (FR-6). Its title and
  body were not edited.
- **#93**: carry the first page's tip in the `z` state, so a paged initial sync can end above a
  boundary that predates it (spec 6.8 case (a)); it needs a new version of the state's codec.
- **#94**: read the calendar log's tip before the sharing-boundary check (spec 6.8 case (b)).
- **#95**: a guard in the token record's issue function that refuses a grant already holding
  entries (item 24).
- **#96**: alert-only and keyword-only JMAP updates of a key account's event store the
  converter's output (item 15, FC1-4 = FR-7). Put to the owner with it, by reading only: on a
  recurring event with attendees a keyword or alert change to one occurrence becomes an override,
  which reads as a scheduling change; upstream's branch for an owner's user-data-only patch stores
  no per-user copy, so D10 does not require skipping it.

## Adjusted assertions

Every existing assertion the release changed carries a comment citing the spec section.

- `tests/src/za/gating.rs`: a key account's session advertises `urn:ietf:params:jmap:calendars`
  and `:calendars:parse`, primary for the account; the key principal's calendar block has
  `mayGetAvailability` false and `accountId` null for a caller without access (6.10, D13); the
  loop of eighteen refused rows keeps the six still refused (`CalendarEvent/copy`,
  `CalendarEventNotification/*`) and a second loop asserts the served rows answer neither
  `accountNotSupportedByMethod` nor `forbidden` (6.2, 6.9).
- `tests/src/za/websocket.rs`: the socket's calendar call answers `forbidden` with
  `ZA_CALENDAR_KEYLESS` instead of `accountNotSupportedByMethod` (6.1).
- `tests/src/jmap/za_variants.rs`: `calendars` runs the calendar block in upstream's order
  (Task 39); `principal_get` gains the two calendar capabilities in `primaryAccounts`, the
  account's calendar capability and each principal's calendar block (6.10, D13). The variants
  `calendar_event` (the four flags refused then created without them, 6.5; `useDefaultAlerts`
  false and the alerts by value, D11; the override patch in windowed and property-subset reads,
  Task 39's F1; a duplicate UID refused in a second calendar, 3.3), `calendar_identity` (6.9),
  `calendar_notification` (`accountNotSupportedByMethod`, 6.11), `calendar_acl` (`shareWith`
  refused and grants through the share endpoints, 6.11; share notifications with the level's
  rights, Task 39's F2; a view grantee's empty patch and rename `forbidden`, Task 39's F3;
  upstream's assertions on
  Jane's reads and her update and destroy at view unchanged, R39-1, R39-2) and
  `calendar_availability` (`shareWith` refused; `Principal/getAvailability` expects `[]`, 6.2).
- `tests/src/za/sharing.rs`: the conversion carries the owner's colour in its sealed personal
  bundle, where release 4 asserted none (6.7, D14).
- `tests/src/za/app_password.rs`: a comment only; the exchange issues a keyed grant and a token
  record, which the app-password revoke leaves (9.6, D19).
- `tests/src/za/setup.rs`, `tests/src/za/password.rs`: a key account's credential version is
  `za_session_binding` of the vault record, compared exactly, where it was the vault revision
  (D19).
- `tests/src/utils/cleanup.rs`, `tests/src/utils/storage.rs`: the emptiness check exempts field
  153 while its account exists (9.6).
- Release 5's `chunked_collection_delete` was rewritten to the one-transaction outcome (wave A).

## Other harnesses

- Task 41, on `0237a0b50` plus its two edits, all first runs: WK, IK and JK passed; PRODUCT (thin
  LTO) built without a warning, its version line naming source `0237a0b50` and `dirty`; the
  fresh-store unit suite (9 tests) and FRESH (9 readback checks) passed. The CalDAV client was not
  run (`python3.12` is not on the host; left to CI's `caldav-client` job, which passed in every
  completed pull-request run). `git diff origin/main...HEAD -- tests/src/webdav` was empty, so
  WK runs upstream's CalDAV suite unchanged.
- The key-mode JMAP step got no rerun at Task 41 (`calendar::alarm` failed in none of five local
  key-mode runs); it reruns once since `bea8a39fb` (item 25).
- `tests/qualification/dispositions.json` gained 23 cohort entries, one per fork runtime file the
  release added under `crates/`, each `runtime_excluded: false`, since the move takes a baseline
  cohort file's lines (`crates/dav/src/common/za.rs`) into `pipeline/admission.rs` and
  `pipeline/context.rs`. The cohort grows from 53 to 76 files.

## Merge hotspots

The paragraphs "Release 6 added merge hotspots" and "Release 6 silent dependencies", which
`CLAUDE.md` held and which now stand in `docs/zero-access/release6-code-map.md` ("Merge hotspots",
"Silent dependencies"; `CLAUDE.md` points there), hold the full list: spec 11's two lists, the
additions of Task 43, the pull request's reviews and the third round. Ids they cite from this
note:

- R22-4: upstream's `changes/get.rs` body moved unchanged into `ChangesPage`.
- R27-1: a binary rollback below release 6 cannot read a coloured personal bundle (also a
  residual).
- R29-7: D11's recipient and description rules are copied from upstream's `alerts.rs`.
- R37-16: the pinned 404 text.
- R44 is plan 13's ruling (its key-mode test variants mirror upstream's bodies line for line),
  not an id of this note; the release 6 calendar variants of `tests/src/jmap/za_variants.rs`
  follow it.
- FC1-10, FC2-11, FA-9, FC2-12: the key writers repeat upstream's inline checks, and several fork
  insertions are larger than one call (also residuals).
- RA-17: upstream's `calendar_event/get.rs` lost an import line.
- Wave C's N3: `za_bearer_entry` takes the cache clock's now from upstream's
  `HttpAuthEntries::expires_in(0)`; a change there fails closed, and
  `keyed_entry_ends_before_signed_expiry` would show it.

## Where the full record is

- The note as it stood at the merge:
  `git show 04145d586:docs/superpowers/plans/2026-11-14-zero-access-plan17-outcome.md`. It holds
  the per-task narrative, the commit lists by task, every ruling (process ones included), the
  review rounds, the validation tables row by row, the fixture lists and every mutation and kill
  check table.
- The plan's files at the same commit: the index as executed, the interface sheet
  (`2026-11-14-zero-access-17-jmap-calendars-interfaces.md`) and the six task files
  (`-tasks-{a,b,c,d1,d2,e}.md`), in `docs/superpowers/plans/`. Read against the code they are
  stale in three places: R22-7 and R31-7 (under "Deferred findings"), and the interface sheet and
  group file C state the earlier `put_access` rule and its four-argument signature (wave E
  changed the rule, RE5-2; wave F added a `now` parameter and the expired-entry rule). Five code
  comments cite the interface sheet as "I §n"; the plan index gives its path in the history.
- The design's review record,
  `docs/superpowers/specs/2026-11-13-zero-access-jmap-calendars-design-codex-review.md`, and the
  approved D29 amendment, `docs/superpowers/specs/2026-11-14-zero-access-jmap-paged-sync-amendment.md`,
  at the same commit.
- Pull request #90: every review thread of the four rounds has a reply with the verdict and,
  where something changed, a reply with the outcome and its commits. Its four summary comments,
  one per review round:
  `https://github.com/jaylaney/stalwart/pull/90#issuecomment-7018100487`,
  `https://github.com/jaylaney/stalwart/pull/90#issuecomment-7042807741`,
  `https://github.com/jaylaney/stalwart/pull/90#issuecomment-7048964124`,
  `https://github.com/jaylaney/stalwart/pull/90#issuecomment-7065213456`.
- The CI runs, by id, under "CI and the gates"; the gates' runs are the manually dispatched ones.
