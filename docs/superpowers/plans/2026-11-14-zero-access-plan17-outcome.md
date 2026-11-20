# Zero-access JMAP calendars and browser sign-in: plan 17 outcome

Written 2026-11-17 by plan 17's Task 43 and extended by Task 44 (the final test matrix and the whole-branch review) and by the record of the pull request's review (2026-11-18 and 2026-11-19, "The pull request's review") and of its second review (2026-11-19, "The pull request's second review") and of its third review and the second whole-branch pass (2026-11-20, "The pull request's third review and the second whole-branch pass"). Plan 17 (`2026-11-14-zero-access-17-jmap-calendars.md`, its interface sheet and the six group files beside it) builds release 6 from the spec `2026-11-13-zero-access-jmap-calendars-design.md`, revision 7 (decisions D1 to D29, invariants 57 to 76). A key account's calendar works from an unmodified JMAP client in a browser: an OAuth token issued through the authorization-code flow carries a 32-byte secret after a `~`, a per-account token record (principal field 153) holds the master key wrapped under keys derived from those secrets, and the HTTP layer opens the keys on every request that presents such a token. The JMAP calendar methods read and write through the same key-account pipeline as CalDAV, which moved from `dav` into `groupware::calendar::pipeline`.

What this note can claim. Every result quoted here is local, except the pull request's CI on `dbd2fccc8`, `d6929c89e` and `68d905131` and the manual runs on `d6929c89e` and `68d905131`, quoted where the note says so: RocksDB on one node, debug test builds (except the three full-profile product builds with `--locked`, the final matrix's, row 17b of its second pass, row 7 of the validation on the first review's final source and row 7 of the second review's validation, each used by its fresh-store run; and the coverage net's instrumented builds of the second review; earlier fresh-store runs used a locally built thin-LTO product binary), the in-store blob store unless a line says ZA-FS. No other store backend, no second node, no native client and no browser client was run. A passing suite is not review approval, and a review approval is not CI. The macOS linker's `__eh_frame` note appears in every suite log and is environmental. The reference client's checklist (`docs/zero-access/client-jmap.md`) is a manual gate that has not been run.

## Where things stand

- Branch `release6-jmap-calendars`, cut from the plan's branch `za-jmap-calendars-design`, on `origin/main` at `ade47a9ae` (the base after the rebase before Task 1; the checkpoints after Tasks 9, 19 and 37 and before Task 44 found the base unchanged). Head before Task 43's note: `de67af5df`, 82 commits ahead of `origin/main`; with the plan's last commit, 92 commits ahead, on the same base. The branch was pushed at `dbd2fccc8` and pull request #90 opened on the project owner's word (2026-11-17). CI ran on `dbd2fccc8` and passed in every job that runs on a pull request (Check Style, Unit Tests, Test, Upstream Suites, Zero-access Test, Build Product, Independent CalDAV Client, Fresh OSS Store Restart, and the code-scanning analyses); the retention gates run on manual dispatch only and were not dispatched. The commits of the pull request's review were pushed on 2026-11-19 with their record (`dbd2fccc8..d6929c89e`), and the pull request's CI passed in every job on `d6929c89e`. On the owner's word of 2026-11-19 the Test workflow was then dispatched manually on `d6929c89e`, with the retention gates: its Upstream Suites job failed in the Automation Tests step, at upstream's DKIM selector assertion (`tests/src/automation/dkim.rs:89`: the key was generated a second after the test read the clock), while the same step passed in the pull request's run, a failure the controller classified as a timing failure of that test, seen once; the coverage net, the listing capture and the inventory gate passed; and the coverage gate failed ("The pull request's second review"). The commits of the second review and their record (`d6929c89e..68d905131`) were pushed on the owner's word of 2026-11-20. The pull request's CI passed in every job on `68d905131`. In the manually dispatched run on that commit every job passed but the Zero-access Test job, which failed in its key-mode JMAP step at upstream's ordinary-account thread merge (a failure the controller classified as timing), and its gates job had not finished when this record was written, so neither gate has a result on `68d905131` ("The pull request's third review and the second whole-branch pass"). Wave E's commits (`a65717ae9`, `20f4cf9d2`, `6c5c46b26`) are not yet pushed, and no CI run exists for any commit after `68d905131`.
- Routing actually used. The plan header and the spec name Sol implementers and Astra reviewers; that routing ran Tasks 1 to 16, Task 17's first implementation and review, and Tasks 19 to 21. On the project owner's instructions of 2026-11-15, recorded in the ledger, Task 17's fix round and Task 18 had an Opus implementer and Fable reviews; from Task 22 on, implementers were Claude Opus (contract code and test modules that need judgment) or Claude Sonnet (Tasks 23, 24, 29, 34, 35 and 41, and the rows of Task 42), and the reviews of Tasks 22 to 31 ran on Claude Opus. On the owner's word of 2026-11-16, every task review and scoped re-review from Task 31's fix round on ran on Claude Fable (xhigh for identity, authorization, persistence, ordering and the oracle; high otherwise). Task 44's whole-branch review ran as five lenses, A, B, C1, C2 and D ("The final review"). The ledger's "in progress" line of each task names its implementer and reviewer. The plan header's routing lines were not edited.
- Commits, by task (`git log --oneline origin/main..HEAD`):
  - the spec and the plan: `48bb52529`, `678c052fe`, `18644d914`, `b53fa807a`, `8bae80c11`, `8cbdb1df0` (spec revisions 1 to 5), `4b245d927` (plan 17), `d22a76594` (main is the base), `f8bf55cd1` (D19, spec revision 6), `ac846e5ea` (routing), `1ba7751d7` (the preflight's alignment);
  - Task 1 `899671c8c` (the pipeline module and its neutral error);
  - Task 2 `60a7ff75a` (admission, roles and contexts move to `groupware`);
  - Task 3 `cdb612dfe` (the UID lease functions move);
  - Task 4 `fecf17ff4` (the scheduling assembly moves);
  - Task 5 `65a12d2d2` (the key create commit moves);
  - Task 6 `a704fe4a9` (one placement resolver for COPY, MOVE and JMAP);
  - Task 7 `7954a4e87` (the key update commit moves);
  - Task 8 `cd8ba6754` (plan ruling: the delete no-op receipt), `a985f16b8` (the key deletes move);
  - Task 9: no commit (the proof run at `a985f16b8`); `dba2c0c21` (plan: the fresh-store diagnostics directory);
  - Task 10 `edddb4eb3` (a pipeline create in several calendars);
  - Task 11 `fe535098f`, `0ef892222` (plan rulings), `c15d7011d` (placement and content in one update);
  - Task 12 `a538207a5` (plan ruling), `3989cf5a6` (the destroy of a whole event);
  - Task 13 `d6211346e` (token secrets, the token record, the hand-off value), fix `c963b44db`;
  - Task 14 `d75acf9b3` (field 153 and its deletion by a vault write);
  - Task 15 `f0baac1e1` (issue, open and forget);
  - Task 16 `9f7d29690` (Bearer tokens that carry keys);
  - Task 17 `88871cf02` (plan ruling D6), `01b3cd762` (the hand-off and keyed grants at the code exchange), fix `0357e3f86`; then `af3ed8934` (the approved D29 amendment incorporated into the spec and the plan, by another session);
  - Task 18 `c8f97fe69` (refresh), fix `ed32a9b2f`, `67917c44e` (documents reconciled with the fix);
  - Task 19 `4767f08e6` (the maintenance sweep of token records);
  - Task 20 `4ceabe5ba` (the calendar refusal text and the admission table);
  - Task 21 `a0ecc14df` (plan amendment), `cf698b9a7` (`CalendarEvent/get`, `Calendar/get`, `Calendar/query`); `bea178e8c` (plan amendment R21-2);
  - Task 22 `17f666208` (sync origin across pages);
  - Task 23 `1a73f868c` (`ParticipantIdentity`);
  - Task 24 `ccf25bee0` (`CalendarEvent/parse`);
  - Task 25 `e5f8fc53d` (the in-memory text index);
  - Task 26 `37de290ef` (`CalendarEvent/query` and `/queryChanges`), fix `f5eba7e79`;
  - Task 27 `dc6e3c9a8` (a shared calendar's colour in the sealed personal bundle);
  - Task 28 `beba6d2b1` (`Calendar/set`), fix `01c17f30f`, plan `a608ed6f2`;
  - Task 29 `180f76685` (default alerts by value);
  - Task 30 `81e40ef8b` (`CalendarEvent/set` create);
  - Task 31 `55c66805c` (update and scheduling), fix `ba3dbdc0f`, plan `9f52fec70`;
  - Task 32 `061d9af74` (placement);
  - Task 33 `d9da63f8e` (destroy), plan `a2e2009d4`;
  - Task 34 `0eb38ea34` (the drain from JMAP requests), fix `259fdad6d`;
  - Task 35 `799af08ce` (the calendar capability for key accounts);
  - Task 36 `aa2c77f65` (the sharing module), fix rounds `81de6e801` and `de67af5df`;
  - Task 37 `68c01d5ed` (the refusals module), fix `b53c422c5`;
  - Task 38 `b3043adf4`, `908868bd7` (plan rulings), `207b77bb1` (the differential oracle), `0baa31bf2` (plan ruling), fix `1e9be31fa`;
  - Task 39 `c3fa2099c` (plan rulings), `81162b587` (upstream's JMAP calendar tests in key mode), fix `8cf35e32c`, plan `0237a0b50`;
  - Task 40 `3d2abb4e6` (leak rules), fix `f3507acdc`;
  - Task 41 `91ff125fb` (the coverage cohort's additions and the key-mode JMAP step's description);
  - Task 42: no commit (the mutation sample applied temporary edits and reverted them);
  - Task 43 `f6f9f0dde` (this note, the reference client's checklist, `CLAUDE.md` and the release 6 documentation), corrections after its review `5cadcd739` and after its second review `7000924b4`;
  - Task 44's fix wave (`## The final review`): `3f1ce7dc5` (a fresh salt on every second-factor change), `f1c69900d` (the `code` parameter masked in traced request URLs), `f0150ec57` (a grantee's non-public events left out of a text query), `a5e45dc2f` (the smaller fixes and the missing cases); then the record of the review `6a3070e00`, its correction `1e2706814`, and the commit of the final matrix's second pass.
  - after the final matrix, `dbd2fccc8` (what the matrix's reruns rest on); then the pull request's review ("The pull request's review"): `53cf84aa9` (spec 3.3's wording on the UID lease); wave A `c7dfbce95` (a key calendar's delete in one transaction), `efb845a4b` (its fence against a crossing write), `461f0dd19` (effective privacy for a grantee's destroy and update), `5f736c77f` (a grantee's event state at or above the sharing boundary), `c65c38347` (the removed href from the asserted parent), `d86f90b31` (one refused occurrence alone); wave B `6f0704dcb` (the drain and the search pass within the token's scope), `d50107f12` (a deleted grant's sessions evicted before its token cleanup), `dff25d1d9` (a keyed cache entry bounded by its token; the keys fenced on revocation), `7bd35c4c4` (a test's keyed token sealed under the current lifetime); wave A's fix round 1 `cf50cd2bb`, `6fbe81a1a`, `0a6139d7a`, `7304f9236`; wave B's fix round 1 `0ca47a61a`; wave A's fix round 2 `5e0e8620b`, `25e098520`; the commits of this record `063af481c`, `9ce65518f`, `d6929c89e`; then the pull request's second review ("The pull request's second review"): wave C's cache part `90d6ad286` (a keyed cache entry ends before its token's signed expiry), `294ebba20` (its follow-up round); its calendar part `11a681bbd` (a listed child the build did not write refuses the delete), `b4d640c3c` (one name per calendar, every name removed on a calendar's delete), `85c23c154` (a deleted calendar's members from the stored records), `45fa3a034` (the fence's operation counted in the size check), and its fix round `8e9baf7b7`, `cef1884a5`, `2f00c2abf`; wave D (tests only, the coverage gate) `3a7d5b474`, `c71aba81d`, `c7666dd8b`, `97b1fa457`, `ce01e0f3a`, `f3630cf67`, and its fix round `67797004b`, `0c73eafc4`; the commits of the second review's record `9b74cde38`, `68d905131`; then the third round ("The pull request's third review and the second whole-branch pass"): wave E `a65717ae9` (the owner's destroy check tests the flag first), `20f4cf9d2` (the access entry a write adds is never the one removed), `6c5c46b26` (two token cases pinned); and the commit of the third round's record.
- Head of the code: `6c5c46b26`, after the third round's wave E (its product changes are `a65717ae9` and `20f4cf9d2`; `6c5c46b26` changes tests only). After the second review it was `0c73eafc4` (its last product change `2f00c2abf`; the later commits change tests only). After the first review it was `25e098520`. Before it the head of the code was `a5e45dc2f`, and the commits between `a5e45dc2f` and `53cf84aa9` change documentation only. The final matrix below ran on the source of `a5e45dc2f`; the validation on the source of `25e098520` is under "The pull request's review" ("Validation on the final source"): of 31 rows, 30 passed on their first attempt and key-mode `jmap_tests` failed once in upstream's calendar alarm test, then passed on two runs of the unchanged command.
- **The final matrix, first pass** (Task 44, Step 2; logs in the git-ignored run directory). Rows 1 to 8 ran on `f6f9f0dde`, row 9 on `5cadcd739` and row 10 on `7000924b4`; the three differ in documentation files only, and row 10's test binary was built from the clean tree before the fix wave's first edit. Every row passed on its first attempt.

  | Row | Command | Result | Wall time |
  |---|---|---|---|
  | 1 | `cargo fmt --all --check` | no output | 7 s |
  | 2a | `python3 -I scripts/tests/ci_steps.py` | 0 differences | 0 s |
  | 2b | `python3 -I scripts/tests/test_gates.py` | 45 tests OK, 2 skipped | 5 s |
  | 3a | SHIP (`cargo check -p stalwart --no-default-features --features rocks`, with the plan header's warning filter) | clean | 151 s |
  | 3b | TM (`cargo check -p tests --tests`, with the plan header's warning filter) | clean | 61 s |
  | 4 | `cargo test -p vault -p groupware --features vault/test_mode -- --nocapture` | vault 208 passed, groupware 59 passed | 95 s |
  | 5 | U-ws (`cargo test --workspace --exclude tests --exclude scim --exclude scim-proto --lib --bins --features "$UNIT_TEST_FEATURES"`) | 889 passed, 2 ignored, 0 failed | 222 s |
  | 6 | `cargo test -p smtp --lib --bins --features test_mode` | 16 passed | 91 s |
  | 7 | `cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise` | 7 passed | 93 s |
  | 8 | `cargo test -p tests za::leak::tests::try_archive_short_values_are_not_archives -- --exact --nocapture`, `STORE=RocksDb RUST_MIN_STACK=16777216` | 1 passed | 97 s |
  | 9 | ZA: `cargo test -p tests za::za_tests -- --nocapture`, `STORE=RocksDb RUST_MIN_STACK=16777216`, `BLOB_STORE` unset | 1 passed (1695.50 s in the test) | 1790 s |
  | 10 | ZA-FS: `cargo test -p tests za::za_tests -- --nocapture`, `STORE=RocksDb RUST_MIN_STACK=16777216 BLOB_STORE=FileSystem` | 1 passed (1607.24 s in the test) | 1753 s |

  Row 5 rewrites 26 tracked fixtures under `crates/dav-proto/resources/requests/` (upstream's restored `parse_requests` test rewrites them on every run). The same 26 files changed at Task 9; they were restored from the head while row 9 waited, and rows 6 to 8, which ran before that, do not run the dav-proto tests. The load gate for the za suites was a one-minute load below 20; on the project owner's word of 2026-11-17 it was raised to 40 for this matrix (the five-minute limit stays below 200). Row 9 had waited under 20 without starting (lowest sample 21.59) and was relaunched under 40; its highest one-minute load during the run was 37.24, row 10's 65.61, with no helper timeout. `query::text_query_cost` printed 300 events in 446 ms and 10 events in 17 ms in row 9, 405 ms and 16 ms in row 10. The pass stopped after row 10 by ruling (below): every fix of the review changes files under `crates/` or `tests/`, so rows 1 to 18 ran again on the fixed source `a5e45dc2f` (the second pass, below).
- **The final matrix, second pass** (Task 44, Step 5; logs in the git-ignored run directory). Every row ran on the source of `a5e45dc2f`. Rows 1 to 9 ran with `a5e45dc2f` checked out (the documentation commit `6a3070e00` landed during row 9); row 10's first attempt ran on `6a3070e00`; row 10's rerun and rows 11 to 18 ran on `1e2706814`. The two commits above `a5e45dc2f` change only `CHANGELOG-fork.md`, `CLAUDE.md` and `docs/` (their diff over `crates`, `tests`, `scripts`, `.github`, `resources`, `Cargo.lock` and `Cargo.toml` is empty). Totals: 32 rows; 30 passed on their first attempt, and 2 passed after reruns: row 10 on the rerun the plan allows, row 15 on a third attempt after the allowed rerun also failed, by the controller's classification of upstream's `queue_retry` as a timing flake (the ruling below). After the matrix the controller ran `smtp::queue::retry::queue_retry` by name, alone, ten times in a row on `3b2d21864` (the source of `a5e45dc2f`): 8 passed and 2 failed (`[1, 1, 3]` and `[1, 2, 2]`), with the one-minute load between 20 and 30 before each run (the table `task-44-logs/r15-sample.tsv` in the run directory). On this host at that load the test fails alone about one time in five; it was not run on the base commit, so the base's rate is not measured.

  | Row | Command | Result | Wall time |
  |---|---|---|---|
  | 1 | `cargo fmt --all --check` | no output | 6 s |
  | 2a | `python3 -I scripts/tests/ci_steps.py` | 0 differences | 1 s |
  | 2b | `python3 -I scripts/tests/test_gates.py` | 45 tests OK, 2 skipped | 4 s |
  | 3a | SHIP (`cargo check -p stalwart --no-default-features --features rocks`, with the plan header's warning filter) | clean | 112 s |
  | 3b | TM (`cargo check -p tests --tests`, with the plan header's warning filter) | clean | 43 s |
  | 4 | `cargo test -p vault -p groupware --features vault/test_mode -- --nocapture` | vault 208 passed, groupware 59 passed | 38 s |
  | 5 | U-ws (`cargo test --workspace --exclude tests --exclude scim --exclude scim-proto --lib --bins --features "$UNIT_TEST_FEATURES"`) | 895 passed, 2 ignored, 0 failed | 151 s |
  | 6 | `cargo test -p smtp --lib --bins --features test_mode` | 16 passed | 29 s |
  | 7 | `cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise` | 7 passed | 59 s |
  | 8 | `cargo test -p tests za::leak::tests::try_archive_short_values_are_not_archives -- --exact --nocapture`, `STORE=RocksDb RUST_MIN_STACK=16777216` | 1 passed | 75 s |
  | 9 | ZA: `cargo test -p tests za::za_tests -- --nocapture`, `STORE=RocksDb RUST_MIN_STACK=16777216`, `BLOB_STORE` unset | 1 passed (1653.39 s in the test) | 1655 s |
  | 10 | ZA-FS: `cargo test -p tests za::za_tests -- --nocapture`, `STORE=RocksDb RUST_MIN_STACK=16777216 BLOB_STORE=FileSystem` | first attempt failed at a helper timeout (a PROPFIND in the atomic collection MOVE module timed out at `tests/src/utils/webdav.rs:120`, after 169.16 s in the test, under a one-minute load of up to 280.86 from other work on the host); the rerun passed (1600.72 s in the test) | 342 s, then 1766 s |
  | 11 | SWAP: `cargo test -p tests za::cache_swap::cache_swap_tests -- --exact --nocapture`, `BLOB_STORE` unset | 1 passed | 26 s |
  | 12a | WP: `cargo test -p tests webdav::webdav_tests -- --nocapture` | 1 passed | 23 s |
  | 12b | WK: the same with `ZA_KEY_ACCOUNTS=1` | 1 passed | 24 s |
  | 13a | IP: `cargo test -p tests imap::imap_tests -- --nocapture` | 1 passed | 16 s |
  | 13b | IK: the same with `ZA_KEY_ACCOUNTS=1` | 1 passed | 18 s |
  | 14a | JP: `cargo test -p tests jmap::jmap_tests -- --nocapture` | 1 passed | 142 s |
  | 14b | JK: the same with `ZA_KEY_ACCOUNTS=1` | 1 passed | 193 s |
  | 15 | SMTP: `cargo test -p tests smtp -- --nocapture`, with `SPAM_RULES_URL` pinned to the spam-filter rules v3.0.4 release | first attempt: 75 passed, 1 failed (upstream's `smtp::queue::retry::queue_retry`, its assertion at `tests/src/smtp/queue/retry.rs:238`, left `[1, 2, 2]`), 3 ignored; the rerun by name failed at the same assertion (left `[1, 1, 3]`); by ruling (below) a further rerun by name passed | 51 s, 12 s, then 15 s |
  | 16a | `cargo test -p tests 'webdav::cal_query::' -- --nocapture` | 2 passed, 1 ignored (upstream's `#[ignore]`) | 1 s |
  | 16b | `cargo test -p tests store::store_tests -- --nocapture` | 1 passed | 9 s |
  | 16c | `cargo test -p tests store::blob::blob_tests -- --nocapture` | 1 passed | 9 s |
  | 16d | `cargo test -p tests store::lookup::lookup_tests -- --nocapture` | 1 passed | 15 s |
  | 16e | `cargo test -p tests store::search_tests -- --nocapture` | 1 passed | 264 s |
  | 16f | `cargo test -p tests telemetry::telemetry_tests -- --nocapture` | 1 passed | 9 s |
  | 16g | SYS: `cargo test -p tests system::system_tests -- --nocapture` | 1 passed | 64 s |
  | 17a | `python3 -B -m unittest discover -s scripts/release -p 'test_*.py' -v` | 25 tests OK | 4 s |
  | 17b | product build: `cargo build --release --locked -p stalwart --no-default-features --features rocks` (the full release profile, with the plan header's warning filter) | clean, 0 warning lines | 1327 s (22 m 07 s) |
  | 17c | `python3 scripts/release/verify_product.py --binary target/release/stalwart --source-commit "$(git rev-parse HEAD)" --ref refs/heads/release6-jmap-calendars --output <run directory>/stalwart.provenance.json` (without `--inspect-elf`: this host builds no ELF) | passed, provenance file written | 0 s |
  | 18a | `python3 -B -m unittest discover -s tests/product -p test_fresh_store.py -v` | 9 tests OK | 7 s |
  | 18b | FRESH: `python3 -B tests/product/fresh_store.py --binary target/release/stalwart --log-dir <a new directory>` | "fresh store: PASSED: 9 readback checks after the restart" | 8 s |

  Rows 8 to 16g ran with `STORE=RocksDb RUST_MIN_STACK=16777216`. The product binary's `--version`: `Stalwart Server fork v0.4.0-beta.1 (upstream v1.0.3 8b32686532d34623e9ce8e1237cf2bed9554af38; source 1e270681414b35e9c981f28e3a8aecf31574f4dc; clean)`. Row 5 rewrote the same 26 dav-proto fixtures again, and they were restored from the head before row 6, so the tree was clean at the launch of every later row. The highest one-minute loads: row 9 30.41; row 10 280.86 in the failed attempt and 79.48 in the rerun; rows 11 to 14b between 19.99 and 29.82; row 15 20.17, then 17.76 and 14.83 in the reruns; row 16e 76.27, the other 16 rows between 11.21 and 35.61. `query::text_query_cost` printed 300 events in 402 ms and 10 events in 17 ms in row 9, and 432 ms and 17 ms in row 10's passing rerun. No warning named a file under `crates/` or `tests/src/` in any row. The pass's wall time, failed attempts included, was 6518 s, about 109 minutes.
- **Not run by either pass**: the Docker suites (`directory`, `automation_tests`; Docker was not running on the host; left to CI), the independent CalDAV client in either mode (`python3.12` is not installed on the host, and nothing was installed; left to CI's `caldav-client` job), the inventory and coverage gates, and CI on the branch (the gates' dispatch and the push need the owner's word). The limits of this evidence: local RocksDB, one node, debug test builds except the product build of row 17b, the in-store blob store except ZA-FS, macOS on Apple silicon (so the verifier ran without its ELF inspection), no other store backend, no second node, no native or browser client. A passing matrix is not review approval.

## What changed and why

**Group A, the move (Tasks 1 to 9; spec 5.1, D2, D17).** The fork's key-account calendar steps left the `dav` crate for `groupware::calendar::pipeline`: admission and roles (`admission.rs`), read and write contexts (`context.rs`), the UID lease (`uid.rs`), the scheduling assembly and the status of D18 (`schedule.rs`), the create, update and delete commits (`create.rs`, `update.rs`, `delete.rs`) and the placement resolver with the event container's reseal (`placement.rs`). The functions moved with their bodies, traces, hook order, lease releases and mail-guard settlements; the CalDAV handlers kept their upstream steps and call the pipeline where their key branches stood, and `From<ZaPipelineError> for DavError` keeps CalDAV's answers. The proof is that the CalDAV tests passed without edits to their assertions: Task 9's proof run at `a985f16b8` passed global formatting, the product build (7 m 59 s, warning-free), the `groupware` and `vault` units (196 and 47), the workspace units (842 passed, 2 ignored), ZA (1495.32 s), ZA-FS (1409.10 s), SWAP (17.56 s), WK (21.12 s), WP (19.04 s), the pinned CalDAV client in key mode (its harness's 34 unit tests; 74 passed, 1 skipped, 2 xfailed) and the fresh-store script (9 of 9 readback checks after the restart).

**Group B, the pipeline paths only JMAP uses (Tasks 10 to 12; spec 5.2, 5.4, 6.5).** A create named in several calendars checks UID uniqueness under the lease in every calendar (Task 10). A JMAP update can change placement and content in one commit: `ZaPlacementPlan::jmap` resolves the old and new calendars with the rights CalDAV's `COPY` and `MOVE` ask, the event and its dead-property container are resealed in the target domain, and one physical batch carries the whole update (Task 11; `CalendarEvent::enqueue_full_at` factored out of `update_full_at` for it). The destroy of a whole event linked into several calendars runs through the shared delete commit with `za_delete_send_itip` (Task 12).

**Group C, tokens (Tasks 13 to 19; spec section 4).** The vault crate gained the token secret's codec, the derivations and associated data, the token record with its caps and reconciliation, the refresh classifier and the hand-off value, and `za_session_binding`, the 64-bit fingerprint of the credentials that D19 makes a key account's credential version (Task 13). Field 153 and its storage, with the record cleared in `vault_batch_write` exactly when the binding changes (Task 14). Issue, open and forget with the two assertions of 4.3 and the three-attempt bound (Task 15). `decode_access_token` strips a `~` suffix from every token that begins with the server's token header, the token endpoint's successful answers are untraced for every account, and the HTTP layer's Bearer arm opens keys from the record and refuses a stale generation (Task 16). The hand-off at the password step, the code exchange that issues a keyed grant, and the refresh precheck of D22 (Task 17). The refresh of 4.5 (Task 18). The maintenance sweep of D8 and the record's end with its account (Task 19).

**Group D1, JMAP reads, queries and admission (Tasks 20 to 27; spec 6.1 to 6.4, 6.6 to 6.9, D14, D15, D29).** The calendar refusal text of D25 and the admission table (Task 20). `CalendarEvent/get`, `Calendar/get` and `Calendar/query` over unsealed views, with store errors rebuilt payload-free at the boundary (Task 21, and the R21-2 amendment for every later served method). `/changes` with the sharing boundary of D15 and the explicit-origin `z` continuation of D29 (Task 22). `ParticipantIdentity`, the default switch only (Task 23). `CalendarEvent/parse` for the keyed owner (Task 24). The in-memory text index built from release 7's content part (Task 25). `CalendarEvent/query` and `/queryChanges` with text conditions matched in memory (Task 26). A shared calendar's colour in each principal's sealed bundle (Task 27).

**Group D2, JMAP writes, scheduling, sharing and refusals (Tasks 28 to 37; spec 3.1, 5.3 to 5.5, 6.5, 6.7, 6.10, 6.11, D3, D4, D10, D11, D16, D18, D28).** `Calendar/set` with its key writer as a child module of upstream's handler, a calendar destroy that sends cancellations, and `shareWith` refused (Task 28). Default alerts written by value into the sealed tree (Task 29). `CalendarEvent/set` create, update with scheduling, placement and destroy, each object its own commit, a store error answered on that object and every later one (Tasks 30 to 33). The drain runs from a keyed JMAP request that names a calendar method (Task 34); since the pull request's review, only for a token whose scope admits calendars and a calendar call that parsed with its capability in the scope-filtered `using` (spec 5.5). The session and principal objects advertise the calendar capability for key accounts (Task 35). The sharing and refusals modules (Tasks 36 and 37).

**Group E, closing (Tasks 38 to 44; spec 3.5, 3.6, 9.3 to 9.8).** The differential oracle runs one edit script through CalDAV and JMAP and compares the stored event's JSCalendar projection, the items and mail, the schedule statuses and the stored pairs' recipients (Task 38). Upstream's JMAP calendar tests run in key mode through `za_variants::calendars` (Task 39). Leak rules for the token record, token secrets and JMAP calendar writes (Task 40). The other harnesses, the coverage cohort's additions and the key-mode JMAP step's description (Task 41). The mutation sample (Task 42). This documentation (Task 43). The final matrix and the whole-branch review (Task 44).

## The session rule

Decided by the project owner on 2026-11-14 (spec D19; section 12, decision 4): a key account's browser sessions end only when a credential changes, that is on a password change, a recovery or a second-factor change; a grant's deletion ends that grant's sessions. They survive every other vault write: the first pin's rewrap, the self-check's repair, a new recovery key, and an app password's creation, publication, withdrawal or revocation. Each such write still raises the vault revision, so resident keys are evicted and the next request opens them again from the token record; a request whose keys were opened at a generation that is no longer current answers 401 (D23), and the client's refresh then succeeds. Since the final review's fix wave, a second-factor change also draws a fresh salt (`za_totp` calls `za_set_password`), so the binding cannot return to an earlier value and the sessions it ended stay ended (`tokens::second_factor_change_ends_sessions`).

| Where | What it does | Tested by |
|---|---|---|
| `za_session_binding` (`crates/vault/src/record.rs`, Task 13) | the fingerprint of the salt, the Argon2 parameters, the verifier and the second-factor secret, never 0 | `binding_changes_only_with_credentials` |
| a key account's credential version (`crates/common/src/auth/access_token.rs`, Task 14) | is the binding | `tokens::credential_changes_end_sessions`, `tokens::other_vault_writes_keep_sessions` |
| `vault_batch_write` (`crates/common/src/auth/vault.rs`, Task 14) | clears field 153 only when the binding changes | `credential_change_clears_the_token_record`, `record_write_loses_to_vault_write`, `tokens::pin_rewrap_keeps_session` |
| `za_token_issue`, `za_token_open_access` (Task 15) | compare bindings; a lost vault assertion under an unchanged binding is retried | `tokens::issue_refuses_changed_binding`, `tokens::open_access_needs_entry_and_binding` |
| the Bearer arm (Task 16) | D23 | `tokens::bearer_crossed_by_vault_write_is_refused` |
| the code exchange (Task 17) | a crossing credential change refuses; any other crossing vault write is retried | `tokens::exchange_crossed_by_password_change`, `tokens::exchange_crossed_by_recovery`, `tokens::exchange_crossed_by_other_vault_write` |
| the refresh (Task 18) | the same | `tokens::refresh_crossed_by_credential_change`, `tokens::refresh_crossed_by_other_vault_write` |
| a browser's first invitation (Task 31, review focus 1) | the session survives the pin's rewrap | `scheduling::first_invitation_and_the_session` |
| `za_totp` (`crates/http/src/api/vault.rs`, Task 44's fix wave, FA-1) | every enrolment, replacement and removal of the second factor draws a fresh salt through `za_set_password`, so the binding cannot return to an earlier value and the tokens a second-factor change ended stay ended | `tokens::second_factor_change_ends_sessions` (kill check K1) |
| the mutation sample (Task 42) | rows J16 and J16b, both killed | |
| this documentation and the checklist row (Task 43) | state the rule | |

## Rulings

Every `Ruling:` line of the ledger (`.superpowers/sdd/2026-11-14-zero-access-17-jmap-calendars/progress.md`, git-ignored), one entry each: what was decided, why, and what it costs if wrong. Where a later ruling corrects an earlier one, the corrected text is given and the replacement is said.

### Execution and routing

- **Trailers.** The execution session's trailer set was empty at first: commits carried their prescribed subjects without copied attribution or an invented session URL. Why: no trailer identity was prescribed. Cost: none.
- **Task 44's context.** The whole-branch review gets the pinned base and head, the accepted contract and validation-only evidence, without prior findings, implementer reports or these rulings. Why: the owner's rule of an independent clean-context review. Cost: the rulings stay controller material to adjudicate the review's findings.
- **Routing from 2026-11-15** (replaces the plan header's Sol/Astra routing for the rest of the plan, without editing it). First: one Opus implementer at a time, Opus reviewers (xhigh for identity, authorization, persistence, ordering and concurrency). Then, on the owner's instruction "have the review agents run on fable", the reviews run on Fable at xhigh with implementation on Opus; later implementers Opus or Sonnet for mechanical work, task reviewers as the work's class asks, Task 44 on Fable. Why: the owner's instructions of 2026-11-15 and, for the Fable reviews from Task 31's fix round on, of 2026-11-16. Cost: a routing line to correct, no code.
- **D29's approval.** The owner approved the explicit-origin amendment and its implementation on 2026-11-15; `af3ed8934` incorporated it, and the stop before Task 22 was lifted. Why: explicit approval and a committed spec and plan. Cost: none.
- **Parallel implementer and review.** The next task's implementer may run while the previous task's read-only review runs, when their files are disjoint (first applied to Tasks 24 and 25, then 34 and 35). Why: one source writer and one cargo command at a time still hold. Cost: a fix round waits for the running implementer.

### Plan corrections

Each controller ruling that changed the plan's text has its own documentation commit:

- `1ba7751d7`: the preflight's alignment, including Task 13's use of the credential fingerprint of I §5.1 rather than `record.revision` (ruling below).
- `cd8ba6754`: Task 8's delete no-op receipt (`ids.change_id(owner, Calendar).unwrap_or_default()`).
- `dba2c0c21`: the fresh-store command's `--log-dir`.
- `fe535098f`: Task 11's single-batch placement write and its current-context hrefs; `0ef892222`: Task 11's reseal kill expectation (the grantee's readback).
- `a538207a5`: Task 12's destroy without a caller-added final commit point, with current-context hrefs.
- `88871cf02`: the D6 hand-off's assertion-only batch against its vault snapshot.
- `67917c44e`: spec 4.5 step 2 and Task 18's steps gain the superseded-generation guard of R18-1.
- `a0ecc14df`: Task 21's payload-free traces of unreadable calendars and events; `bea178e8c`: every later served key-calendar method routes keyed store errors through the payload-free rebuild (R21-2).
- `a608ed6f2`: `Calendar/set`'s alert acceptance after the commit, a head store error as the method's store error, and TenantQuota removed from the plan's rows.
- `9f52fec70`: Task 30's head states R28-2; Task 31's floor case names the R31-1 input.
- `a2e2009d4`: Task 33's attendee step follows CalDAV's decline rule (accept, then destroy).
- `b3043adf4`: the oracle compares items and the mail's calendar part by JSCalendar projection; `908868bd7`: the oracle's M6 expects pairs for the invitee and the outside address; `0baa31bf2`: the oracle's SEQUENCE rule and step M7b.
- `c3fa2099c`: three rows of Task 39's tables (F1, F2, F3); `0237a0b50`: two rows marked unchanged from upstream (R39-1, R39-2).

### Preflight

- **Task 13 uses the credential fingerprint of I §5.1, not `record.revision`.** Why: Task 13's own tests and spec revision 6's D19 require sessions to survive vault writes that change no credential. Cost if wrong: Task 13's tests fail and users are signed out after unrelated vault writes.
- **Task 17 retries a failed vault assertion when the binding is unchanged**, within Task 15's three-attempt bound; only a changed binding answers `invalid_grant`, exhausted retries and other failures answer a server error with the grant cleaned up. Why: spec 4.3 and 4.4 and Task 17's own race test. Cost if wrong: a successful race with an unrelated vault write would fail.
- **Every submitted `blobId` link is refused**, including a same-event embedded blob; `za_take_rulings` loses its unused stored argument. Why: spec 6.5 and 6.11 refuse attachments on every patch. Cost if wrong: unsupported attachment behaviour.
- **I §7.3's mapper documentation follows D28** (per-object handling). Why: spec 5.3 and I §7.5 keep earlier committed answers. Cost if wrong: a stale comment that could hide committed objects.
- **`ZaDeleted` and CalDAV's deletion behaviour stay as they are**; no new post-commit receipt field or failure hook. Why: Task 8 produces only `AccessToken` invalidations, and `invalidate_caches` propagates errors only from role expansion, so the reported post-commit error is unreachable today. Cost if wrong: a future fallible post-commit step needs committed-receipt handling first; a pre-commit hook is not post-commit coverage.

### Tasks 1 to 12

- **Task 1.** The `pipeline` module declaration follows `pinning` in rustfmt order. Why: FMT and the interface contract. Cost: none.
- **Task 8.** The CalDAV delete's assertion-only no-op keeps `ids.change_id(owner, Calendar).unwrap_or_default()` instead of `last_change_id`. Why: a concurrent unlink can leave the archive under another parent, the storage step then writes nothing and the base answers 204, while `last_change_id` debug-panics on the absent id. Cost if wrong: none found; the receipt is 0 only when no Calendar change was made.
- **Task 9.** FRESH gets the required `--log-dir` (a new directory). Why: the script requires it and CI supplies it. Cost: none; only the diagnostics destination changes.
- **Task 9, generated fixtures.** `dav-proto`'s restored test regenerated 26 tracked JSON files during U-ws; they were restored from the commit with guards, and the report distinguishes a parse success from an absent snapshot comparison. Cost: none to the product.
- **Task 11, revert.** The reseal fault is reverted from an exact saved pre-fault file, not by `git checkout`, so the uncommitted placement implementation survives. Cost: none.
- **Task 11, atomicity.** A narrow factoring in `storage.rs`: `CalendarEvent::enqueue_full_at` has `update_full_at`'s body without its commit point; the placement path calls it on both attempts with no trailing commit point, so one physical batch carries the update. Why: the embedded commit point could split the event and container from later alarms and tombstones. Cost: none for existing callers, which keep the wrapper.
- **Task 11, hrefs.** Vanished hrefs on every attempt use the asserted parent context's current canonical names. Why: a source rename during a pause would otherwise log an obsolete href that incremental sync filters out. Cost: none to CalDAV paths.
- **Task 11, the kill oracle.** The missing-reseal kill is shown by any post-move dead-property readback, with the grantee's decisive: the shared owner keeps its key and can still open the unresealed container. Cost: none; the test order stays.
- **Task 12.** The whole-event destroy omits its caller-added final commit point on both attempts and uses the current-context href rule. Why: a final split could publish the deletion and then fail a context-only write, losing its receipt and mail. Cost: none to existing CalDAV paths.

### Tasks 13 to 19

- **Task 13 (M1).** A strict 43-byte alphabet and final-sextet check runs before `decode_slice`. Why: the decoder's malformed-tail diagnostics re-decode into unzeroed scratch. Cost: none; accepted inputs are identical.
- **Task 14, existing OAuth assertions.** No assertion that a non-credential vault write ends an OAuth session needed a changed outcome; `setup.rs` and `password.rs` joined the task's scope for wording and for an exact `za_session_binding` comparison. Cost: two test files.
- **Task 14, a temporary `#[allow(dead_code)]`** on `token_write_store` until its first caller in Task 15, which removed it. Cost: none.
- **Task 16, Bearer lookup errors** map to a fixed `AuthEvent::Failed` and 401 with no payload. Why: I §5.5 and spec 8. Cost: none to Basic.
- **Task 16, D27's evidence.** The fifteen-refusal control stays, with a recorded limit: it does not prove zero counter increments, since the counter API is private. Cost: D27's zero-increment claim rests on reading the new paths for an absence of the ban counters.
- **Task 17, D6.** The password step's hand-off writes one more assertion-only batch against the original vault snapshot before it returns the secret; a lost assertion is an authentication failure without retry, and a failed operation deletes the provisional entry. Why: a vault write between the password's check and the hand-off's write must refuse the step. Cost: one batch per keyed hand-off.
- **Task 17, cleanup scope.** The cleanup rule applies to the new keyed issuance; upstream's failures after `create_oauth_grant` and before the keyed issue may leave a grant row, as upstream does. Cost: an inherited grant row with no wrap.
- **Task 17, the R17-1 regression** uses inert synthetic canary bytes at a synthetic code's hand-off alias; no real key, wrap, code or token is a canary. Cost: it does not reproduce a real hand-off's disclosure.
- **Task 18, R18-1.** In `za_token_refresh`, after the attempt's registry read and before classifying, a stored generation of the presenting grant above `presented` answers `Superseded`: nothing written, nothing deleted. Why: this request's rotation left the stored generation at `presented` and it never falls, so a higher one is a later rotation. Cost if wrong: a retry is refused where it could have been answered; the client already holds the newer token. The spec and plan text were reconciled in `67917c44e`, replacing the earlier note that they were owed.
- **Task 18, R18-6 and R18-8**: the code stands (no fork trace for `Superseded` and `VaultChanged`; an unusable record makes a refresh delete its grant while the sweep leaves the record). Both are questions for the spec (below).
- **Task 18 documents.** Task 18's documents were reconciled with the accepted fix before Task 19, and the overstated refresh vault-assertion catcher claim removed. Cost: documentation only.

### Tasks 20 to 27

- **Task 20, RED.** The tests stay in the compiled source; a missing-function diagnostic is the accepted RED. Why: an absent test file proves nothing. Cost: one more compile run.
- **Task 21, traces.** Recoverable unreadable-calendar and unreadable-event branches trace fresh `DataCorruption` events with account and document ids and fixed reasons, never the original error context. Why: the deserialization error can carry raw archive bytes. Cost: less detailed corruption diagnostics.
- **R21-2.** Every key-calendar method a later task serves routes its keyed store errors through the same rebuild: dispatcher-admitted methods through `za_serve_calendar_read`, the three admitted inside the changes handlers through the same rebuild applied there. Why: spec 8 binds every path, and `CalendarEvent/query` reads content through the reader whose exposure R21-1 fixed. Cost: one wrapper per arm and two regressions.
- **Task 26's `/queryChanges` call** in the payload-free case. Why: the handler form has no other regression. Cost: one call.
- **D28's trace.** The per-object store-failure trace, and the trace of a store error in `Calendar/set`'s default write, are fresh events with the account id, the document id when known and a fixed reason. Why: the original may carry stored bytes. Cost: less detail.
- **D15 is a sync-correctness rule, not an access control.** A client-chosen origin can only make the server answer from log records for a range, which hold item ids, never content. Cost if wrong: a grantee that forges a state could learn ids changed or destroyed before its grant or rename. Listed for the owner to confirm; confirmed by the project owner on 2026-11-20 ("The pull request's third review and the second whole-branch pass").
- **Task 23's parity** with ordinary accounts is shown by one JP run in Task 24's closing checks. Cost: one plain JMAP run.
- **Task 24's JP flake.** JP's first attempt failed in upstream's `mail::thread_merge` (thread sizes, ordinary accounts) and the rerun on identical source passed; accepted as timing outside the task, since thread merging finishes in a background task. Cost if wrong: an ordinary-account threading regression masked; both runs are kept. The ledger records no recurrence in later JP runs.
- **Task 26's kill check** of the `/queryChanges` handler form closes its evidence gap: the case fails when the rebuild is missing, and Task 21's R21-1 RED showed on the same reader that an unrebuilt error carries the stored bytes. Cost if wrong: the canary assertion itself stays unobserved failing on that path.

### Tasks 28 to 33

- **TenantQuota.** The enterprise-gated arm and its gated assertion are removed; a tenant-quota error, which exists only in enterprise builds, takes the D28 store-failure path. Why: code under the enterprise feature is Stalwart Enterprise Licence code. Cost: an enterprise build answers a tenant-quota failure with the generic store-failure error.
- **R28-1.** `request_alerts.accept` moves after `commit_batch` returns `Ok`. Why: with per-object commits a failed object must not leave its alert ids in the request state. Cost: none in stored state.
- **R28-2.** A store error from `za_writer_keys` is the method's store error, rebuilt at the dispatcher, not the keyless refusal; the same rule binds Tasks 30 to 33. Cost: none.
- **D28's trace evidence.** `store_failure_stops_the_call` captures traces and asserts the fixed reason and that the hook's own details text appears in no event. Cost: one assertion block.
- **R31-1.** `update_applies_sequence_floor` passed with the floor removed, because upstream's increment stores the stored SEQUENCE plus one for an owner-organized event. Its input became a grantee-organized event (CalDAV `SEQUENCE:5`, JMAP `sequence: 1`), which upstream's increment skips and the floor raises; the stored value must equal the floor; a kill check followed. Cost: one changed case and one kill run.
- **R31-6.** One update assertion refuses `hideAttendees: true` with the stored event unchanged. Why: Task 26's query relies on no key event carrying the flag. Cost: one assertion.
- **Task 33's attendee case follows the code.** A destroy of an unanswered invitation sends no DECLINED, as CalDAV's DELETE (upstream skips a NEEDS-ACTION attendee); the case accepts first, then destroys. Why: D18 gives a destroy CalDAV's condition. Cost if wrong: a decline on an unanswered invitation would be a product change against CalDAV parity.

### Tasks 34 to 37

- **Task 35 beside Task 34's review** (disjoint files). Cost: a Task 34 fix round waits.
- **R34-2.** The drain hook drains the caller's own inbox whatever account the methods name: one step with an item pending for key2 and a method naming key's account. Cost: one step.
- **Task 36's additions.** (a) the view grantee's destroy asserts upstream's text "You are not allowed to remove events from calendar {id}."; (b) a seventh case, `move_with_scheduling_into_shared`, a placement change with a fan-out (R32-2). Cost: one case.
- **Task 36's deviations** stand as test-side corrections: trigger checks match the value whatever its parameters (upstream writes `TRIGGER;RELATED=START`), and the delegated-reply fixture is a mailed REQUEST the owner's drain imports and the owner moves. Cost if wrong: a fixture that hides a defect in a planted or PUT attendee copy.
- **R36-10.** Upstream's JMAP create falls back to `itip_attendee_create` when the account is not the organizer; the key writer calls `itip_create` only, so an attendee that creates an already-answered event with `sendSchedulingMessages: true` sends no REPLY and gets no schedule tag. Accepted under D1 (CalDAV has no attendee create); a proposed addition to spec 3.3 and an open item for the owner. Cost if wrong: one missing REPLY path in a browser client.
- **Task 36, fix round 1.** A new case `grantee_destroy_sends_delegated_cancel` (a grantee's JMAP destroy and its CalDAV twin each send one CANCEL with the grantee as `SENT-BY`), and the stored pair read before and after the scheduled move with a kill check. Why: the dispatch as made did not show what R32-2 asked. Cost: one round.
- **Task 37's addition R35-3.** The keyless Bearer token's session object lists the key account with the calendar capability (D13). Cost: three assertions. The same dispatch resolved the D29 case's wording: real `z` states of both origins from the key account's own paged `/changes`; `stateMismatch` with a read-back on `Calendar/set` and `CalendarEvent/set`; a `z` in `ifInState` never changes the keyless or the non-grantee answer, and a difference is reported, not pinned.
- **Task 37's deviations** stand: `notification_destroy_leaves_the_item` runs before `every_row_of_6_11`, the keyless Bearer token is minted with `za_grant_token`, the metadata value is built inline, `CalendarEvent/copy` carries `create {}`, and one temporary upload is left (the final emptiness check passed). Cost: small.
- **Task 37, fix round 1.** R37-1 (a matching `ifInState` is accepted), R37-2 (a third impersonation form: a key account holding `Impersonate` with a view grant and its own password), R37-4 (the attachment download is denied), R37-5 (metadata refused on update). Why: each is part of spec 9.2's list for the module. Cost: one round.

### Task 38, the oracle

- **The dispatch.** The kill check runs on the committed tree, the kill mutation is confined to `ZaEventWriter::destroy`, and an oracle difference stops the implementer rather than being normalised away (spec 3.6).
- **Items by projection.** Items and the mail's calendar part are compared as `normalize_jscal(jscalendar_of(tree))`; other mail parts with `Tally::same`. Why: spec 3.5 compares by recipient, method, UID, SEQUENCE and recurrence id, and by D1 a JMAP write stores the converter's form (`DURATION` for `DTEND`, the export's order) with the stamps D17 assigns to JMAP's shaping. Cost if wrong: a property the converter drops would pass; statuses and pairs are compared separately.
- **Deviations** stand: participant ids derived from mapped names are mapped too, an object-valued `calendarIds` is dropped whole, the script's open parts are the implementer's, item trees are opened before drains, and Part B's refusal is arranged by writing the pin record.
- **The SEQUENCE rule.** The script's texts carry an explicit SEQUENCE, the same on every VEVENT: 0 at the create, one higher at each organizer step that changes the text. Why: the increment is JMAP's shaping for a client that omits `sequence`; a script whose CalDAV text never raises SEQUENCE differs by itself. The rule's original "cost if wrong" said Task 31's floor case and the events module cover the server's increment for a client that omits `sequence`; that was corrected: they do not (R38-1), which led to step M7b.
- **M6's pairs** name the invitee and the outside address: `token_union` mints a pair for every scheduled attendee not local to the sender (release 5 spec 5.2). Cost if wrong: none found.
- **R38-1, step M7b**: between M7 and M8 the master ends one hour later with scheduling, the JMAP patch without `sequence`, so upstream's increment and the pipeline face the CalDAV client's bump; R38-2: a message's calendar part has its own METHOD line compared as text. Cost: one round.

### Tasks 39 to 42

- **F1, a D11 row.** In a windowed or property-subset read, each override whose title differed from the master's when the defaults were written holds the patch `alerts/abc/iCalendar` with that title. Why: D11 writes the alarms per component as upstream's iCalendar view does, and a read is upstream's projection; an ordinary-account control (alarms with different descriptions over CalDAV) showed the same patch. Cost if wrong: a client sees an unknown patch path, and a CalDAV client shows the title the event had when the defaults were written. Open for the owner: a constant description would remove both effects and needs a change to D11's wording. What a client sees is inherited from upstream's two read paths, for every account: a windowed or property-subset read shows the patch, a full read does not.
- **F2.** The share endpoints write the calendar's ACL through upstream's builder, which logs one share notification per entry change with ids and rights only; the variant mirrors upstream's lines with the level's rights. The plan's "the share endpoints write none" was a wrong premise.
- **F3, a 6.7 row.** A view grantee's empty `Calendar/set` patch is `forbidden`: the key writer takes the context with `Acl::Modify` before computing a change. Cost if wrong: an empty update to a view-level shared calendar errors.
- **Task 39, fix round 1.** R39-1: upstream's two assertions on Jane's event update and destroy at view hold unchanged and are restored (upstream's own check answers first in upstream's words); R39-2: Jane's calendar name is compared as upstream does; R39-6: `calendar_notification` ends with `test.assert_is_empty()`; R39-4: two availability assertions changed, not four. Cost: one round.
- **Task 40's deviations** stand, subject to review (the base64 narrowing in particular, later replaced by R40-2).
- **Task 40, fix round 1.** R40-1: the authorization code is taken with the label-only form, since `unwrap_code()`'s panic would print a consent token with its hand-off secret; with it R40-2 (slicing by character, no alignment left unsearched), R40-3 (scans wait for the workers), R40-4 (one scan of the owner before the task's destroy), R40-5 (the refresh entry's next wrap length), R40-6 (each canary and both colours read back through a keyed get) and R40-10 (canaries also searched in base64 and hex). Cost: one round.
- **Task 41, step 1.** The script's seven baseline names are functions earlier releases retired, each with a disposition; nothing is restored and the inventory gate stays the authority.
- **Task 42, batches.** Sequential batches of about seven rows, Sonnet executors, one row per background command, each batch writing its table as it goes; base `f3507acdc`. Why: an executor holding 28 rows hands back early or loses rows.
- **The ZA-short recipe** keeps the four `setup::` calls (`tests/src/za/mod.rs:142-145` at that base), which provision the suite's key accounts, and removes `disabled::test` through `sharing_races::test`; J4 was run again with it as J4s.
- **The controller's rows X1 to X3** (later X4), defined in batch D's dispatch.
- **X1** is a real gap in the fork's own code: a quiet move that lost the stored pairs would leave the organizer's bundle without them. The sharing module's move case was strengthened (Task 36's fix round 2) and X1 run again.
- **X2, as corrected.** The first ruling said the call of `increment_sequence` and its place before the floor are killed by X3 and J6; the review (R42-1) showed the logs do not support it, and the ruling was replaced: X2 stays an accepted residual, not an equivalent mutant, because the mutated function is upstream's, byte-identical to v1.0.3, which the key writer calls by D17. No za case was shown to compare an unscheduled JMAP update that omits `sequence`, and upstream's unit tests of `sequence.rs` are in the private tree no build here compiles, so nothing in this fork's net covers the override branch of the increment for any account. Row X4 (the call removed from the key writer) was added to replace the claim with evidence; its result is under the mutation runs.
- **The X2 and X4 record** (replaces the sentence R42-1 corrected). The key writer's call of upstream's increment is protected by the oracle's Part A (X4's kill), and a wrong extra raise by the oracle's M2 (X3's kill). M7b compares the scheduled path end to end and has killed no mutant of the increment, because the scheduling path repairs it (X2, X4). The override branch inside upstream's `increment_sequence` on an unscheduled update stays an accepted residual in upstream's function (X2).
- **Task 42's close.** All rows run: the brief's 28 killed; X1 killed after the test was strengthened (`de67af5df`), X3 and X4 killed, X2 an accepted residual. Task 42 made no commit; the strengthening is Task 36's.

### Tasks 43 and 44

- **README's search sentence** (Task 43's concern 4). The README's "calendar full-text search stays gated" is corrected in Task 43's fix round, though the brief's README row covered only the deferred list: D12 serves text search of a key account's events over JMAP, so the sentence told a reader of the public README something release 6 made untrue. The fix states what is served and what stays as it was for CalDAV, read from the code and the spec. Cost if wrong: one sentence of the README.
- **Task 44's matrix beside Task 43's review.** The matrix starts on `f6f9f0dde` before Task 43 is closed: a fix to Task 43 can only be a documentation commit, which changes no file a matrix row compiles or runs. Each row records its head and the tree hashes of the source directories and the Cargo files; the product build, its verifier and the fresh-store script are held until no documentation commit is pending, since the verifier compares the binary's source commit with the head. The plan itself puts a documentation commit after the matrix. Cost if wrong: a row rerun on the later head.
- **R43-3.** The project rule governs: the outcome note names an inherited behaviour without describing how it arises, and the paged-`/changes` item is not an accepted residual, since nobody accepted it; it moves to the open items as pending the owner's decision. Why: the note is public. Cost if wrong: the public note says less than it could.
- **R43-4.** Release 4's sharing spec and plan 14's outcome note are not edited by Task 43: they are approved records of their own release, and a pointer in them is the owner's call. Their four statements of the colour exclusion D14 replaces are recorded under known staleness and put to the owner, with the observation that spec section 10's amendments do not list the sharing spec's colour statement. This supersedes the earlier ledger line that asked for a note in those documents. Cost if wrong: one more small edit when the owner asks for the pointers.
- **R43-6 and the twelve minors** (R43-7 to R43-18) are corrected in the same fix round, though the brief's tables did not list those rows: each is a statement of fact in a public document or in the file every later session reads first, and leaving them would have the whole-branch review spend its one pass re-finding them. Cost if wrong: a larger fix diff for the scoped re-review.
- **Five lenses for the whole-branch review.** Task 44's whole-branch review runs as five independent reviewers in one pass, each with a lens (A sign-in, the token record and the session rule; B the pipeline, the move out of `dav` and parity with CalDAV; C1 `CalendarEvent/set`, scheduling and default alerts; C2 admission, reads, `Calendar/set`, queries, paged sync and sharing over JMAP; D leaks across the change, the tests that changed, the gates and the documents), not one reviewer. Why: the branch is large (118 files and about 42,800 added lines as counted at `de67af5df`, the figure the ruling was made with; the reviewed head `5cadcd739` is 126 files and 44,038 insertions), more than one reader can read closely in one context, and the project's earlier whole-branch reviews ran as lenses. This is the one whole-branch pass the owner authorized. Cost if wrong: an issue between two lenses' slices is seen by neither; each lens was told to follow calls out of its slice.
- **Independent briefs.** The initial briefs carry no earlier finding, implementer report, task-review conclusion or ruling of this note, and the reviewers do not open this note or the run directory; the ledger's 222 deferred lines and 40 coverage notes are put to each lens only after its report, for triage. Cost if wrong: a second, shorter turn per lens.
- **The review before the matrix finished.** The review was dispatched on `5cadcd739` while the matrix's suite rows still waited on the load gate, with an extract of the rows already run, a departure from the plan's order, whose Step 3 has the matrix run on the head first. Why: the gate's wait was outside anyone's control, the review reads only, and a matrix failure is adjudicated whatever the reviewers knew. Cost if wrong: a reviewer spends attention on something a failing row would have pointed at.
- **N1 (Task 43, second round).** The note's R43-3 entry now says that it names an inherited behaviour without describing how it arises, and that the item is pending the owner's decision. The wording fault was the controller's: the earlier text was copied from a private record into this public note. Cost: none beyond the edit.
- **N2 to N8 (Task 43, second round)** are corrected in the same round, each one phrase; this note's "Rulings" was not extended then to the rulings after `5cadcd739`, which this record adds. Cost: a second small fix diff to re-review.
- **The matrix pauses after row 10.** Rows 11 to 18, with a rerun of rows 1 to 10, run once on the head the fix wave leaves. Why: every fix changes files under `crates/` or `tests/`, so most rows would be rerun anyway, and one complete matrix on the final source is stronger evidence than rows split across two heads; the full-profile product build is not spent on a head about to change. Cost if wrong: a fault in rows 11 to 18 that predates the fixes is found one step later and cannot be separated from them by the record.
- **FA-1 = FD-2** is fixed by drawing a fresh salt on every second-factor change, in the TOTP endpoint, through the existing `za_set_password`: the binding then never recurs, the change is atomic with the vault write, and no stored layout, registry object, D19 input or D19 text changes. Deleting the grants on a binding change was weighed and not taken (not atomic; a failed deletion leaves the revival in place); a guard in the commit funnel that refuses a binding change under an unchanged salt was left out of the wave and offered to the owner. Cost if wrong: one more Argon2 derivation per second-factor change, and a future second-factor endpoint that does not hold the password could not use this fix.
- **FA-3 = FD-1** is fixed by masking the value of every `code` query parameter where a request URL is formatted for the trace, and in a traced `Referer`, for every request: no account is known when the URL is traced. A third every-account trace exception beside D24 and D26, it needs the owner's confirmation as an amendment of spec section 10; until then it is the controller's ruling (confirmed by the owner on 2026-11-18). Cost if wrong: an operator no longer sees an authorization code in a debug URL trace.
- **FC2-1** is fixed in the query's text resolution: for a caller that is not a member of the account, an event whose unsealed tree is not public enters no text index, as on an ordinary account (D12) and as `/get` shows it (6.3). CalDAV still serves such an event whole to a grantee (FC2-2), field policy 2's recorded limit, which goes to the owner as a line for spec 3.3. Cost if wrong: JMAP is stricter than CalDAV in one more place for such an event.
- **FC1-1 = R36-10** changes no product code: the code is what D1 asks for, and a fallback could only sit in the pipeline's create arm, which CalDAV shares and releases 2 and 5 qualified. The wave adds one case that pins the behaviour; the owner is asked to add the line to spec 3.3 or to amend 3.2. Cost if wrong: an invitation accepted by creating the copy goes unanswered until the owner rules.
- **FA-2 and FC1-2** are fixed as tests, FA-2 with a kill check that shows the case can now fail. Cost: the runs.
- **The Minor findings** are disposed by id as the adjudication lists (the table under "The final review"): the wave takes a broken property, a missing test of a spec requirement or a one-line correction, and leaves what changes a client-visible answer under a fault, what the plan prescribed, and what only the owner can decide. Cost if wrong: a deferred or residual item is fixed in a follow-up, with its record here.
- **The gate of 40 for the remaining runs.** The fix wave's runs and the second matrix pass use the gate the owner set for this matrix: the same task's runs on the same host in the same hours. Cost if wrong: a load-induced helper timeout, which the rerun rule answers.
- **P4 gains `za_refresh_event`'s empty-list refusal** (`Forbidden(Context)`), the same class of fault as FB-1 in a function the wave otherwise leaves as moved; covered by reading only, since its trigger needs a lost assertion and a reclassified account in between. Cost if wrong: a refusal where a panic was; the mail guard on that exit ends unsettled, as on the function's other error exits.
- **W-1 to W-5 are parked, with no second fix wave**: W-1 deferred with its fix named, W-2 a residual (an equivalent mutant at suite level), W-3 and W-4 deferred test gaps, W-5 a residual stated with the masking's rule. The second matrix pass continues on `a5e45dc2f`, since no code changes after the re-review. Cost if wrong: each is a small follow-up; none stores, serves or traces more than before the wave.
- **All eighteen findings of the documentation review corrected in one round.** The review of this note's record of the final review (R44-1 to R44-18) is corrected in one fix round, the Minor findings included. Why: as at Task 43, each is a phrase in a public record, and this note is what the owner reads to accept residuals. The fix was committed before the matrix reached the product build. Cost if wrong: one more scoped re-review of a small diff.
- **Row 15's failure is upstream's timing flake, and the matrix goes on.** The failure of `smtp::queue::retry::queue_retry` in the second pass is classified as upstream's timing flake under this host's load, not as a finding of the release: the test is rerun by name up to five more times, each attempt recorded, stopping at the first pass, and the matrix then continues with row 16. Why: the assertion compares whole-second differences between each retry's due time and the loop's clock (1, 2, 3), which the test's own polling loop shifts by one when the host is slow; the two failures differ from each other (`[1, 2, 2]` and `[1, 1, 3]`), which a deterministic fault would not; the branch changes no file under `crates/smtp`, `crates/smtp-proto` or `tests/src/smtp`; and the test is on the list the project and CI already rerun by name. Had none passed, the matrix would have stopped and the test would have been run on the base commit. The first further rerun passed, so the base run never happened. After the matrix the controller ran `smtp::queue::retry::queue_retry` by name, alone, ten times in a row on `3b2d21864` (the source of `a5e45dc2f`): 8 passed and 2 failed (`[1, 1, 3]` and `[1, 2, 2]`), with the one-minute load between 20 and 30 before each run (the table `task-44-logs/r15-sample.tsv` in the run directory). On this host at that load the test fails alone about one time in five; it was not run on the base commit, so the base's rate is not measured. Cost if wrong: a real regression in queue retry would be passed as a flake; the base has the same code, so it would be upstream's, and CI's SMTP step would show it.

The load gate's change from 20 to 40 for this matrix was the project owner's own instruction of 2026-11-17 ("let's raise it to 40"); it is recorded here as his decision, not as a ruling.

### The pull request's review

The controller's rulings of the round ("The pull request's review" below), in order. The project owner's decisions of 2026-11-18 are listed there as his, not here.

- **C4617611549**: fixed, JMAP only; a non-member's update and destroy are checked against the event's effective privacy. Why: `CalendarEvent/get` and the text query already treat such an event as private, and `/set` let an `edit` grantee destroy or declassify it. Cost: one content read and one unseal per grantee destroy; JMAP is stricter than CalDAV in one more place, and a non-member's destroy of an event whose view does not open answers `notFound` before the calendar checks (RA-13).
- **C4617611565**: fixed, with a limit, by giving a non-owner of a key account the snapshot's highest change id, narrowed by the guarded raise (RA-1) and by fix round 2 (initial-origin continuations). The fix holds for the states `CalendarEvent/get`, `/query` and `/set` give, and for `/changes` and `/queryChanges` from the initial state in one page, from an exact state, and continuing an exact-origin paged sync; it does not hold for a continuation of a paged sync from the initial state. Why: every new share of a quiet calendar handed its grantee a state its next call refused. Cost: the grantee's state moves on a container-only change; one more log read per poll when a container change is the last record; one log read on the last page of an exact-origin paged sync. What remains, accepted: a paged sync from the initial state ends where upstream ends it, at the range's last event change, below any boundary above that change, whether it completed while the sync paged or before it began (the `z` state carries no tip of its first page, and raising over the first kind would hide a newly shared calendar's events); its next call is refused, and a paged restart ends at the same state until an event is written, the client syncs in one page, or it takes its state from `/get` or `/query`. A client that follows the protocol holds the state `/get` gave it and never enters this flow; for this one flow the fault the review named remains.
- **C4617611584**: fixed; the tombstone's parent is the asserted one. Why: release 6's own guard skipped it. Cost: none.
- **C4617611602**: fixed by evicting in place and broadcasting before the cleanup (the verification's form A), FA-16 closed with it. Why: a slow or interrupted cleanup delayed the eviction, and a retried revocation does not repair it. Cost: the doubled eviction on every account (spec 10; RB-14).
- **C4617611615**: fixed by fixing the entry's deadline at verification and publishing only a live entry, W-1 closed with the epoch fence on the keys. Why: spec 4.6 says a cached token is served at most for its remaining life. Cost: none on the product path; per `OAuthGrant` invalidation the keys are removed after the epoch advances.
- **C4617611650**: corrected in this note. Cost: none.
- **Wave A's stop condition** (the store property fails on the ephemeral store): passing it is accepted. Why: the condition was meant for a data store with calendars and did not say so. Cost if wrong: none; the store ruling below replaced the question.
- **`CalendarTooLarge`** is answered `forbidden` with a fixed text over JMAP and 403 over CalDAV, the implementer's choice. Why: 507 and `tooLarge` mean something else. Cost: a client meets a generic refusal whose text names the remedy.
- **RA-1**: fixed. Why: the hole was in the verification's design, which the brief passed on. Cost: one log read on a continuation's final page.
- **RA-2, RA-5, RA-6, RA-7, the store ruling** (replaces the withdrawn "no store distinction, record the limit"): the one transaction, the size refusal and the fence apply where the store can establish them; FoundationDB, a read replica and the ephemeral store keep the chunked, unfenced delete. Why: on FoundationDB one transaction fails a large delete on every attempt after its cancellations; a backend the fork does not qualify does not lose a function it had. The property is pinned by the unit test on RocksDB and SQLite and held by reading on PostgreSQL and MySQL (RA-10). Cost: those three stores keep the partial outcome of a failed later chunk; release 4 refuses them outright for a rarer route. Put to the owner and confirmed by the project owner on 2026-11-19.
- **RA-3**: fixed with a case in which only the merge operation refuses. Cost: one test hook.
- **RA-4**: accepted as a residual, put to the owner and confirmed by the project owner on 2026-11-19. Why: it is the delete's existing failure class, with two new triggers (a create into the calendar during the fan-out, where the alternative is an orphaned event, and any calendar commit of the account between the last counter read and the end of the commit), and a retry converges; narrowing it (an internal retry with scheduling off, or an exact membership revision) is for issue #92. Cost: until a retry, key attendees hold a CANCEL for events the organizer still has.
- **The size refusal end to end**: a case with a test-mode threshold. Why: the owner decided "before any cancellation". Cost: one test hook.
- **Wave A's deferred minors**: RA-8, RA-10, RA-11, RA-12, RA-13, RA-15, RA-16, RA-17 and the CalDAV collection `DELETE` failure no longer exercised over CalDAV ("Deferred findings").
- **The rerun after a timeout in an untouched module** (fix round 1): accepted, the 500 ms DAV client timeout at a recorded load spike. Cost if wrong: a regression in the rule-expansion trace module passed as a flake; the rerun and the last whole run passed it.
- **Fix round 1's three new minors**: (1) removed in fix round 2; (2) accepted, the flow's behaviour before the wave; (3) pinned in fix round 2. One follow-up was proposed to the owner and filed on his word of 2026-11-19 as issues #94 and #93: read the tip before the boundary check, which closes spec 6.8's case (b); and carry the first page's tip in the `z` state, which would let a paged initial sync end above a boundary that predates it and so remove case (a), a change that needs a new version of the state's codec (an earlier wording of this record named a third case that the last re-review's trace showed cannot arise from a state the server issued).
- **The post-write search pass** (wave B's hand-back): no change; an earlier ruling to extend the condition there was withdrawn. Why: the pass runs only after an admitted `Email/set` or `Email/import` committed an ingest, which a token whose scope excludes mail cannot reach (RB-17). Cost if wrong: one pass after a write the token could not make.
- **The wave's co-author lines** stay, by the owner's word on the same question. Cost: none.
- **RB-1, RB-2**: fixed, with kill checks for the three conditions RB-8 named. Cost: one round.
- **RB-3**: accepted as the decision reads, put to the owner and confirmed by the project owner on 2026-11-19. Why: the `Blob` call is served, not refused, and the pass is catch-up work. Cost: a token whose scope excludes mail still advances the mail index through a `Blob` call.
- **RB-4, RB-5**: the missing drain is the rule the owner decided. What a token's scope admits on DAV is upstream's scope model, unchanged; the drain's scope rule limits the drain, not what a token can read. No change in this pull request: the project owner's disposition of 2026-11-19. Cost: none in this pull request.
- **RB-9, RB-10, RB-11**: fixed. Cost: one round.
- **Wave B's deferred minors**: RB-6, RB-7, RB-12, RB-14, RB-15, RB-18, RB-19 ("Deferred findings").
- **Wave B's fix round 1 re-review**: its two wording nits are recorded and not changed. Cost: a heading that understates the remaining skew (rewritten by the second review's wave C, `90d6ad286`, so that nit no longer applies); one long comment line.
- **The validation's failed row 24** (key-mode `jmap_tests` on the source of `25e098520`, upstream's `calendar::alarm` test, "EventSource alarms do not match"): classified as a timing failure of upstream's alarm test, not a result of this round's changes; the row is recorded as failed, with the two later runs of the unchanged command, which passed. Why: the test's first alert is due about one second after the event's create and the one-minute load was 17 to 23; the suite passed in plain mode in the same run and in key mode on `7304f9236` and `7bd35c4c4`; between `7304f9236` and the final code only `changes/za_calendar.rs` and a comment in `drain.rs` differ under `crates/`, and the alarm path reaches neither. Cost if wrong: a regression of the alarm path in key mode passed as a timing failure; one failure in three runs is a small sample, the base branch's rate was not measured, and CI's key-mode JMAP step has no rerun, so the same failure there would need one.

### The pull request's second review

The controller's rulings of the second round ("The pull request's second review"), in order, each reported to the owner; none was put to him as a decision.

- **C4633314478**: a listed child the build wrote nothing for refuses the delete with "try again" (`Busy`), before the size check and any cancellation, in the fenced branch only; the stores that keep the chunked form keep it. Why: the fence was to refuse any delete whose child list is not the calendar's, and this interleaving passed it. Cost: none in the "try again" rate (every other skip was already refused by the first comparison).
- **C4633314492**: both ends. The keyed `COPY` inside one calendar runs the UID check with or without an overwrite and answers 403 `no-uid-conflict`, as upstream answers an ordinary account (a change to release 4's handler, whose `COPY` answered 204 and left the event with two names); and a key calendar's delete removes every name the event has under it, deleting the event when no other calendar holds it, so a store that already has such an event is cleaned. Why: the finding was confirmed with no race needed, and upstream's handler makes the check. Cost: a request that "succeeded" is refused.
- **C4633314503**: the calendar's members, for the build, both comparisons and the `onDestroyRemoveEvents` check, come from the snapshot's stored event records and their parents, not from the path index, on every store (it is part of what the delete writes). The root, that no write fences an event's name, is inherited, a design of its own on a path upstream shares, and is not taken here. Why: the path index shows one document per href. Cost: the member list is taken up to four times for one calendar delete, each an in-memory pass over the account's event records (the owner's `onDestroyRemoveEvents` check over JMAP, the build, and the two comparisons on a fenced store); three times for a CalDAV collection `DELETE`; once or twice on the stores that keep the chunked form. No store read.
- **C4633314510**: the size check reserves what the fence's operation will add, from one definition shared with the operation. Why: the operation is added after the check, so a batch at the cap could send its cancellations and commit over it. Cost: a delete within ten counted bytes of the cap is refused.
- **C4633314515**: the fork-only form. At publication, for keys opened from a Bearer token, the cache clock is read, the token is opened again (no event, no failure counted), an expired or unopenable token publishes nothing and the request is served with its keys, and otherwise the entry's deadline is lowered to the fresh remaining lifetime less one second, never raised. The form that carries the absolute expiry through upstream's authentication path for every account was not taken (a merge-hotspot file every login passes; an ordinary account's entry would change), so an ordinary account's entry keeps upstream's behaviour. Why: the finding is confirmed, and the fork can bound its own keyed entry at its own publication step without changing the authentication path every account takes. Cost: one more token open per keyed cache miss; a keyed entry ends up to two seconds before its token, and with one whole second left nothing is published, so a keyed request in that window is verified in full and charged to its address's anonymous rate limit where one is configured (a client that refreshes before `expires_in` never reaches it).
- **The cache part's review**: RC5-7, RC5-13, RC5-9, RC5-11, RC5-1, RC5-4, RC5-5 and RC5-12 taken into a follow-up round; RC5-2, RC5-3 and RC5-8 deferred, and its re-review's N1 to N4; RC5-6 recorded as a residual; RC5-10 (the records) closed by this record. Cost: one round.
- **The calendar part's review**: RC-1, RC-2, RC-5, RC-6, RC-8, RC-3's comment, RC-11 and RC-12 fixed in a fix round; RC-3, RC-4, RC-9 and RC-10 deferred; RC-7 (the documents and one code comment did not follow the code) closed by this record, but for the code comment describing the unfenced form in `crates/common/src/za/test_hooks.rs`, which does not mention that the unwritten-child refusal is absent there (deferred); RC-11's wait, left open by the re-review, handed to the coverage wave (test-only). Cost: one round; RC-3 leaves the inherited name race a way to two names in one calendar; RC-10 leaves release 4's collection `COPY` with an overwrite not removing a hidden document or a second name under the calendar it overwrites.
- **The coverage gate** (the manual run on `d6929c89e`): closed in this pull request by a tests-only wave (wave D) before the next push, measured locally with the project's net and gate script, to at least 120 covered lines of margin on both figures; no product code, no new hook, no change to the qualification files. Cost: a local measurement, which CI's gates job confirms or not after the push.
- **The manual run's Automation Tests failure**: classified as a timing failure of upstream's DKIM selector test, seen once (the key was generated a second after the test read the clock), since the same step passed in the pull request's run on the same commit. Cost if wrong, as the controller confirmed it: a regression in that path would be passed as a timing failure; the step runs in CI on every pull-request run, so it would show again.

### The pull request's third review and the second whole-branch pass

Decided by the project owner on 2026-11-20: a grantee's calendar `/changes` reports the ids of the owner's events outside the calendars shared with it as destroyed (bare ids), accepted as inherited behaviour; and D15 stays a sync-correctness rule, not an access control (item 2 of the list for the owner, the final review's FC2-5, the fresh pass's FR-1). No code change for either. The rest are the controller's rulings, reported to the owner; none was put to him as a decision.

- **The three findings of the third review** (C4644031673, C4644031680, C4644038016): fixed in one wave together with what the fresh pass found, after that pass reported, so the tree did not move under its reviewer. Why: one wave means one review, one validation and one push. Cost: the three waited some hours; none changes behaviour, and the first removes one in-memory pass.
- **FR-3**: fixed in wave E (`put_access` removes before it adds). Cost if wrong: an older, still valid access token stops opening one refresh earlier than before in that same rare case.
- **FR-2**: one line in spec 6.8 and a pointer in 3.4, without how it arises; put to the owner (decided above). On 2026-11-20 the controller also recorded it, outside this repository, as an inherited behaviour. A filter of a grantee's destroyed list for key calendars, which would need each destroyed event's former calendar (the log does not carry it), was named to him as later design work and is not planned.
- **FR-4, FR-5**: already in the record (FA-14; R18-7, item 7); no change.
- **FR-6**: deferred, as a comment on issue #92. **FR-7**: one sentence in spec 3.4; no code change (D10 puts the user data in the tree).
- **Wave E's review**: no fix round. RE5-1 for this record; RE5-2: the plan's files are the plan as it was executed and are not amended after its end, and spec 4.3 and this note carry the eviction rule as it now is (cost if wrong: a reader of the plan's interface sheet reads the earlier rule); RE5-3 deferred; RE5-4 recorded.
- **The manual run's key-mode JMAP failure on `68d905131`**: classified as a timing failure of upstream's ordinary-account thread merge, the class of Task 24's recorded plain-mode failure; the branch changes nothing under `crates/email` and nothing in the thread-merge task. Cost if wrong: an ordinary-account threading regression passed as a timing failure; the step runs again in CI on the next push. A rerun for that CI step is proposed to the owner (item 25) and not made.

## Review focus

1. **The first invitation keeps the browser signed in** (Task 31, `scheduling::first_invitation_and_the_session`). Passes: after the invitation to another key account pinned the invitee's key, the same token reads the event, its refresh is granted, the grant is still in the token record, the vault revision rose by one and the binding is unchanged. Step 4 checks the CalDAV body's `SCHEDULE-STATUS` of `1.2`, because upstream's JSCalendar projection drops `SCHEDULE-STATUS`. Limit (R31-8): the case shows that a vault write happened, not that it was the pin's rewrap.
2. **A native client's dead properties across a browser move** (Task 11, `pipeline::placement_reseals_event_container`). Passes; with the reseal removed it failed at the grantee's readback (a per-item 500 with no dead-property values), while the owner's readback still succeeded because a shared owner keeps the key that opens the unresealed container.
3. **A grantee's default reminders** (Task 36, `sharing::grantee_default_alerts`). Passes: the grantee's first event with `useDefaultAlerts: true` stores one alarm with the owner's `-PT10M` (the grantee's entry is a copy of the owner's from the grant); after the grantee sets its own email alert `-PT45M`, its second event stores `-PT45M` only, with `ATTENDEE:mailto:jmapcal-sharing-reader@example.com` (the grantee), reads back `useDefaultAlerts: false`, and holds no event preferences. Its kill check (the writer's account id in place of the grantee's personal id) failed at the second event's alarms. Limit (R36-5): the first event cannot tell the grantee's copy from the owner's live entry.
4. **An edit in the browser drops what a native client wrote** (Task 38, `oracle::round_trip_keeps_what_an_ordinary_account_keeps`). Passes: on the key account `X-JMAPCAL-KEEP`, `DESCRIPTION:alarm text` and `X-JMAPCAL-ALARM` survived the JMAP edit, and its lines equal the ordinary account's. Part B: after an unscheduled and a scheduled JMAP update, the pairs' recipients, the display name and the withheld entries are what they were. Row X4 was killed here (below).
5. **Search on a large calendar** (Task 26, `query::text_query_cost`). The ZA run of the final matrix's second pass, on the fixed source `a5e45dc2f`, printed `text_query_cost: 300 events opened, one text query without a time range, 402 ms` and `text_query_cost: 10 events opened, the same query within one day, 17 ms` (its ZA-FS run 432 ms and 17 ms; the first pass 446 and 17 ms, and 405 and 16 ms); Task 36's fix round 2 printed 440 ms and 17 ms, and Task 26's own run 687 ms and 16 ms. All are a debug build on local RocksDB, inside the za suite. The counts are by construction: the case asserts the account holds exactly the 300 events before the unranged query, and no counter verifies the number of views opened. A networked store would add a read latency per candidate (R26-6), and memory was not measured.

## Mutation runs (plan 17)

### Task 42, the brief's 28 rows (spec 9.7)

Each row was one exact replacement applied to `f3507acdc`, run as its row names (ZA: `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests -- --nocapture`, which stops at its first panic; U-vault, U-groupware, U-common: the named unit test), then reverted with `git checkout -- crates/ tests/`; each batch ended with an empty `git status --short crates/ tests/`. Logs and diffs are in the git-ignored `mutations/` directory. The task's review read every diff and log and confirmed each first failure. **28 of 28 rows were killed.** The first failure was at the named catcher, or one the row allows, for 23; at another test of the same property for 5 (J3, J7, J16b's suite run, J19, J25). File and line are as of `f3507acdc`.

| Id | Fault | Edit | First failure observed | Named catcher? |
|---|---|---|---|---|
| J1 | the JMAP create skips the lease (its UID check) | `calendar_event/set/za.rs`, create: `za_take_uid_lease` gets `&[]` | `tests/src/utils/jmap.rs:586`, "Missing not created item 0": the third create of `jmapcal-events-uid` was created | yes, by inference: `events::create_uid_rule` |
| J1b | the lease check covers one calendar | `pipeline/uid.rs`, `za_take_uid_lease`: `take(1)` | `pipeline.rs:662`, "create_uid_taken_in_second_calendar [a, b]: inserted conflicting document 4" | yes |
| J2 | the JMAP create skips sealing | `calendar_event/set.rs:189`: the key branch never taken | `utils/jmap.rs:574`, "Missing created item 0", `notCreated` `forbidden` "Another process modified this calendar event, please try again." (the ordinary fence) | yes, by inference: `events::create_reads_back_over_caldav` |
| J2f | the same, without the fence | J2 and the fence line removed | `events.rs:976`, "the stored tree holds a sealed value in the clear" | an earlier `events` case the row allows (`create_reads_back_over_caldav`) |
| J3 | the JMAP create calls `ItipMessages::queue` | `set/za.rs`, create: upstream's builder queues the tree's messages (corrected once: the first edit had no document set) | `scheduling.rs:475`, "invite_key_outside_and_plain: mail to OUT", left 2, right 1 | no: an earlier scheduling case (the queued task's mail doubled); `no_itip_task_ever` not reached (R42-3) |
| J4 | an update drops the token pairs | `pipeline/update.rs`, `za_commit_update`: `pairs = None` | `tests/src/za/fanout.rs:423`, "the token is stable across updates" (CalDAV; the function is shared) | a module before `tokens::test`, as the row expected; follow-up J4s below |
| J5 | an update drops the display name | `set/za.rs:1390`: `display_name: None` | `utils/webdav.rs:1008` from `events.rs:1455`, left `{""}`, right `{"Display canary"}` | yes: `events::update_keeps_pairs_and_display_name` |
| J6 | an update skips the SEQUENCE floor | `set/za.rs:1188`: the `za_sequence_floor` call removed | `events.rs:1514`, "update_applies_sequence_floor: the stored SEQUENCE is not the floor", left 1, right 5 | yes |
| J7 | a text filter reads the stored tree | `calendar_event/query.rs:549`, expanded loop, key arm yields the stored content | `query.rs:518`, the expanded `title: offsite` query, left 0, right 1 | the catcher is `query::two_accounts_same_answers`' expanded assertions (`query.rs:512-525`), the only place the suite checks the expanded filter against the view; the brief's `text_matches_the_view_not_the_tree` runs unexpanded queries and cannot catch it (R42-2) |
| J8 | the unexpanded matcher reads one component at a time | `za_text.rs`, `ZaEventTextIndex::insert`: an inline copy of `index_document` with `take(1)` (corrected once for an import) | unit `za_text.rs:315`, "Title \"team offsite\" English"; ZA `query.rs:435`, "condition 8: {\"title\":\"team offsite\"}", left `[]`, right `["jmapcal-query-series"]` | yes, both (`text_index_cases`; `two_accounts_same_answers`) |
| J9 | a token-record write omits the vault assertion | `common/src/za/tokens.rs`, `token_write_store`: the vault assertion removed | unit `common/src/za/tokens.rs:786`, `unwrap_err()` on `Ok`; ZA `za/tokens.rs:1024` in `assert_invalid_grant` | yes, both (ZA by inference: `exchange_crossed_by_password_change`) |
| J10 | a superseded refresh generation deletes the grant | `http/src/auth/oauth/za_token.rs`: `Superseded` deletes as `Lost` | `za/tokens.rs:2218`, "the superseded retry deleted the grant" | yes: `retry_of_g_while_g1_completes` (iteration not logged, R42-5) |
| J11 | a lost record race puts back a pruned entry | `tokens.rs`, `za_token_refresh`: the retry writes the first attempt's record | `za/tokens.rs:1024`, the retry came out granted | the named case, one assertion before "generation g is in the record" (R42-5) |
| J12 | the sweep keeps a refresh entry below the stored generation | `za_token_sweep`: generation 0 given to `reconcile` | `za/tokens.rs:2430`, "D8: only the stored generation survives", left `[0, 1]`, right `[1]` | yes: `interrupted_refresh_cleared_by_sweep` |
| J13 | the drain hook is removed | `api/request.rs:207`: the `za_drain` call removed | `scheduling.rs:1380`, "drain_runs_from_jmap: the item is still pending after Calendar/get" | yes |
| J14 | a notification destroy is let through | `api/za_calendar.rs:168`: `(CalendarEventNotification, Set)` served | `refusals.rs:933`, left `None`, right `Some("accountNotSupportedByMethod")`; the destroy answered `notFound` and the item stayed | yes: `notification_destroy_leaves_the_item`; the kill is the gate's answer, not the item-survival assertion (R42-6) |
| J15 | the wrap's associated data omits the grant id | `vault/src/keys.rs`, `token_aad` | unit `vault/src/tokens.rs:445`, `other_grant.open_access(..).is_none()` | yes (unit only, as the row says) |
| J16 | a credential change leaves the token record | `auth/vault.rs`, `vault_batch_write`: the clear removed | unit `auth/vault.rs:1568`, `raw_token_record(..).is_none()`; ZA `za/tokens.rs:937`, "unexpected token record after password change" | yes, both (ZA by inference: `credential_changes_end_sessions`) |
| J16b | a vault write that changes no credential deletes the record (D19) | the clear made unconditional | unit `auth/vault.rs:1593`; ZA `za/tokens.rs:937`, "unexpected token record after first pin" | unit yes; ZA: `pin_rewrap_keeps_session`, an earlier case of the same property; `other_vault_writes_keep_sessions` not reached (R42-4) |
| J17 | a superseded refresh entry is kept | `vault/src/tokens.rs:188`, `reconcile` | unit `vault/src/tokens.rs:665`, left `[0, 1]`, right `[1]`; ZA `za/tokens.rs:2340`, same values | yes, both |
| J18 | an unopenable secret is served keyless | `za_bearer.rs:54`: the catch-all arm returns `Ok(None)` | `za/tokens.rs:341`, "session case 0", left 200, right 401 | yes: `tampered_or_foreign_secret_is_401` |
| J19 | the hand-off is not deleted on redemption | `za_handoff_take`: no delete | `za/tokens.rs:1195`, the consent hand-off still stored after redemption | no: `consent_path`, an earlier case of the same property (R42-4) |
| J20 | the cap's eviction leaves the evicted grant | `za_token_issue`: the eviction's deletion skipped | `za/tokens.rs:747`, the evicted grant still stored | yes: `cap_evicts_least_recently_used` |
| J21 | a refresh without the secret of a keyed grant rotates it | `za_token_refresh_precheck`: the guard made always true | `za/tokens.rs:1024`, not `invalid_grant` | yes: `refresh_without_secret_of_keyed_grant_refused` |
| J22 | a Bearer request at a stale generation is served | `authenticate.rs:285`: the stale-generation refusal removed | `za/tokens.rs:446`, left 200, right 401 | yes: `bearer_crossed_by_vault_write_is_refused` |
| J23 | a successful token answer is traced | `oauth/token.rs:738`: `with_untraced_body()` removed | `za/tokens.rs:1892`, "the trace contains an OAuth token or a secret" | yes: `granted_answer_is_untraced` |
| J25 | the unexpanded matcher's document is built from the stored tree | `query/za.rs:93`: the stored content inserted | `query.rs:435`, "condition 1: {\"title\":\"budget\"}", left `[]`, right `["jmapcal-query-meeting"]` | `two_accounts_same_answers`, the same property; the named `text_matches_the_view_not_the_tree` runs later and was not reached, and would catch it by reading (R42-4) |
| J26 | a placement change asks for a right `MOVE` does not | `placement.rs:121`, `entered_rights`: `ModifyItems` added | unit `placement.rs:481` and `:415`, left `Bitmap 48`, right `Bitmap 16` | yes, both (unit only, R42-9) |

There is no J24.

### The controller's rows

| Id | Fault | First failure observed | Outcome |
|---|---|---|---|
| X1 | `za_commit_update`: `pairs = None` when the placement plan is a change and no message was built (a quiet move) | none at `f3507acdc`: the whole suite passed (1728 s) | survived; ruled a real gap; Task 36's fix round 2 (`de67af5df`) added two quiet moves to `move_with_scheduling_into_shared`; against `de67af5df` X1 failed at `sharing.rs:2129`, "event 30 holds no pairs" (inside `stored_pair`, in the first quiet move by the run order; no backtrace) |
| X2 | `increment_sequence` (upstream's `groupware/src/calendar/sequence.rs`) skips components with a RECURRENCE-ID | none: the whole suite passed (1834 s); M7b printed the sides equal (stored 6, items 6, mail 6) because the scheduling path raised the override itself | survived; an accepted residual in an upstream function, not an equivalent mutant (ruling above); run under ZA only, not JK or JP |
| X3 | the key writer's update raises every scheduling component's SEQUENCE once more after `increment_sequence` (written inline: the accessors are a private trait's; the first form did not compile) | `tests/src/za/imip/oracle.rs:604`, oracle main step M2, the JSCalendar comparison: CalDAV side `"sequence": 1`, JMAP side `"sequence": 2`, everything else equal | killed; the cross-side comparison failing that R38-12 asked to see; M7b was not reached |
| X4 | `calendar_event/set/za.rs:1182`: the `increment_sequence` call of the update deleted (base `de67af5df`) | `tests/src/za/jmap_calendar/oracle.rs:1801`, in `round_trip_keeps_what_an_ordinary_account_keeps`: the key account's lines hold no `SEQUENCE` line where the ordinary account's hold `SEQUENCE:1`; the oracle's main, quiet and inbound steps all passed first (M2 stored SEQUENCE 1 on both sides) | killed (ZA, exit 101, 369 s; no JK run needed; `task-42-batch-E.md`). Every scripted step before it printed the sides equal, M7b included (stored 6, item 6, mail 6 on both sides): on a scheduled update the scheduling path raises SEQUENCE itself, so M7b does not protect the call; the oracle's Part A does, on an unscheduled edit, by setting the key writer against upstream's writer for an ordinary account. With X3 (a wrong extra raise, killed at M2) this is the X2 and X4 record ruled above; the override branch inside upstream's function stays X2's accepted residual |

### Follow-up runs

- **J4s.** J4's edit with the corrected ZA-short recipe (the four `setup::` calls kept, `disabled::test` through `sharing_races::test` removed): `events.rs:1444`, "update_keeps_pairs_and_display_name: the update dropped the token pairs", the named catcher. The first ZA-short form removed `setup::test` too and stopped in `tokens::test`'s own setup (`tests/src/utils/za.rs:181`, 409 "not a key account"), so it did not reach the catcher.
- **Corrected first attempts.** J3's first edit queued in a batch with no document set and failed for that reason ("No document is set for a document task"); it was corrected once and run again. J8's and X3's first edits did not compile and were not run.
- **X1 after the strengthening**, above.
- No row was rerun for a result; every kill was observed once. The runner sets no `RUST_BACKTRACE`, so a panic inside a helper names the helper's line; the review checked each attribution by inference (J1, J2, J5, J9, J11, J16, J16b, J21) against the test source and each holds. Unit runs (J8, J9, J15, J16, J16b, J17, J26) have no command log (R42-12).

### Survivors

X1 survived and was answered with a strengthened test, which kills it. X2 survived and is an accepted residual in an upstream function. X3 and X4 were killed. **Of the brief's 28 rows, no mutation survived.**

### Kill checks earlier tasks ran

Each was made against a task's source, reverted (from a saved file where uncommitted work had to survive, otherwise with `git checkout`), with an empty `git status --short crates/` afterwards; none was committed. Lines are as of each run's source.

| Task | Mutation | Caught by |
|---|---|---|
| 10 | `uid.rs`: the loop over `&calendar_ids[..1]` | `pipeline.rs:344`, "create_uid_taken_in_second_calendar [a, b]: inserted conflicting document 4" |
| 10 | `create.rs:255`: the commit path's lost-create fence over the first calendar only | `pipeline.rs:502`, "lost_fence_rechecks_every_calendar jmapcal-pipe-lost: Busy(CreateLost)" |
| 11 | the container reseal removed from the update | `dav_seal.rs:1299` from `pipeline.rs:898`: the grantee's read got a per-item 500 with no dead-property values; the owner's read at `:897` succeeded |
| 12 | the destroy's `send_itip` set to false | `pipeline.rs:1657`, `destroy_single_name_equals_dav_delete`, grantee new inbox items left 1, right 2 |
| 17 | R17-2's hand-off cleanup removed from `za_token.rs` | `tokens.rs:1561`, `classification_failure_removes_the_handoff`, "the hand-off outlived its code" |
| 26 | the `/queryChanges` handler form's rebuild removed (`changes/query.rs`) | `query.rs:1287`, "no rebuilt Store error was traced for both methods (1)" |
| 29 | `za_replace_alarms`: no removal; unlink instead of remove | unit: `former_alarms_removed_not_orphaned` and `no_defaults_leaves_no_alarm` fail under both |
| 31 | the `za_sequence_floor` call removed | `events.rs:1492`, "the stored SEQUENCE is not the floor", left 1, right 5 |
| 36 | the create's default-alert writer is the account id, not the personal id | `sharing.rs:1981`, `grantee_default_alerts`, "the second event's alarms" (the owner's `-PT10M`) |
| 36 | `pairs = None` on every placement change (`pipeline/update.rs:146`) | `sharing.rs:2209`, "the stored pair changed across the move" |
| 36 | X1 against the fix round 2 commit | `sharing.rs:2129`, "event 30 holds no pairs" |
| 37 | `CalendarEventNotification/set` served | `refusals.rs:734`, `notification_destroy_leaves_the_item` (no method error; upstream's handler answered the pending item `notFound`, so the item-gone half did not trigger, R37-3) |
| 38 | `ZaEventWriter::destroy`: scheduling off behaves as on | first `scheduling::cancel_and_decline_on_destroy` (an item without the flag); with that module's call skipped, `oracle.rs:1303` at `207b77bb1`, "quiet step Q2 (first CalDAV, second JMAP): JMAP: the new inbox items", left `[(Invitee, "CANCEL")]`, right `[]` |

Task 40's two negative controls caught their faults (junk in field 153: "field 153 does not parse as a token record"; an access secret appended to the record found in `Subspace::Property`). Task 22 executed no kill check; its report argues by reading which case fails for each removal.

### What has no row

- **D21** ("a write that only removes entries asserts the record alone") has no row: a removal reads no vault record, so the fault cannot be made without adding a read, and nothing would make that read stale (spec 9.7). `removal_write_asserts_the_record_alone` and `forget_grant_asserts_the_record_alone` test it.
- **D28** has no fault in spec 9.7's list. Its tests are `calendar_set_error_table` and `calendars::store_failure_stops_the_call` (Task 28), `event_set_error_table` (Task 30) and the two mail-guard cases (Tasks 31 and 33).
- Known equivalent at suite level, not run: the attachment download's denial at `jmap/src/blob/download.rs` (R37-12); the 404 stays with the denial removed, because the embedded reader finds no attachment in the sealed tree.

### Kill checks of the final review's fix wave

The six kill checks K1 to K6 (and K6b) of Task 44's fix wave are tabled under "The final review" below.

### Kill checks of the pull request's review

The kill checks of the two fix waves and their fix rounds (kA1 to kA6, x1-kF1 to x1-kF4 with x1-kF3a and x1-kF3b, x2-kH1, x2-kH1b and x2-kH2 for wave A; B-K1 to B-K3 and y1-k1, y1-k1b, y1-k2 and y1-k3 for wave B) are tabled under "The pull request's review" below. Every one made the suite fail except x1-kF3b, which fix round 2's H2 closed (x2-kH2).

## The final review

### How it ran

Five independent reviewers read the whole branch at `5cadcd739` against the base `ade47a9ae`, each with a lens: A sign-in, the token record and the session rule; B the pipeline, the move out of `dav` and parity with CalDAV; C1 `CalendarEvent/set`, scheduling and default alerts; C2 admission, reads, `Calendar/set`, queries, paged sync and sharing over JMAP; D leaks across the change, the tests that changed, the gates and the documents. Five rather than one because the reviewed diff (126 files, 44,038 insertions at `5cadcd739`) is more than one reader can read closely in one context (ruling above). Their briefs held no earlier finding, report or ruling; the deferred lines and coverage notes of the task reviews were put to each lens only after its report, for a triage section. The review started before the matrix finished, with an extract of the rows already run (ruling above). Each reviewer read only and ran nothing.

### Totals

No Critical. Eight findings rated Important, six distinct (FA-1 = FD-2, FA-3 = FD-1). 78 Minor, with duplicates across lenses. One finding withdrawn by its own reviewer (FC2-4: a master login returns a token with no keys, as the ledger's R37-14 says).

### The six Important findings

- **FA-1 = FD-2: a second-factor change did not end sessions for good.** The binding is a function of the salt, the Argon2 parameters, the verifier and the second-factor secret, so removing a second factor restored the earlier binding and the old tokens authenticated again, keyless. Ruling: fixed; every change of the second factor draws a fresh salt. The wave: `za_totp` calls `za_set_password` with the password it has just verified and the master key it holds, in the record it commits (P1); the case `tokens::second_factor_change_ends_sessions` holds real tokens across second-factor changes (the factor removed, the same URL enrolled again) and a recovery (T1).
- **FA-3 = FD-1: the hand-off secret and the code reached the request-URL trace** when the client's redirect is served by this server. Ruling: fixed for every request, pending the owner's confirmation as an amendment of spec section 10 (confirmed by the owner on 2026-11-18). The wave: `traced_url` (`crates/http-proto/src/request.rs`) masks the value of every `code` query parameter, `traced_header` passes a `referer` through it, and the three URL sites of `crates/http/src/request.rs` format through it (P2); six unit tests and the case `tokens::traced_urls_mask_the_code` (T2).
- **FA-2: the D27 case could not fail** (fifteen refusals against a ban rate of 100 a day). Ruling: fixed as a test. The wave: the case lowers the ban rate to 5 for one forwarded address, makes six refusals and shows the address is not banned and a valid request from it is served (T3; kill checks K6 and K6b).
- **FC1-1 (the ledger's R36-10): a key account that creates an event as an attendee with the scheduling flag sends no reply**, and that copy never schedules afterwards; upstream's JMAP create falls back to the attendee's reply, CalDAV's does not. Ruling: no product change (it is what D1 asks for); a pinning case, and the owner decides between a line in spec 3.3 and an amendment of 3.2. The wave: `scheduling::attendee_create_sends_no_reply` (T5).
- **FC1-2: no test made a `CalendarEvent/set` create fail with a store error, and none crossed object kinds** (D28, invariant 75). Ruling: fixed as a test, with R32-1. The wave: `scheduling::mail_guard_on_failed_create` (a create's commit fails: the create before it keeps its commit and its mail, the failed create's mail is dropped, and every later object of the call, of any kind, gets the same answer unattempted), and `events::placement_refused` asserts the whole refusal (T4).
- **FC2-1: a grantee's text query matched the fields `CalendarEvent/get` hides**, for an event whose CLASS is sealed (a non-standard value). Ruling: fixed; for a caller that is not a member of the account an event whose unsealed tree is not public enters no text index (P3, `crates/jmap/src/calendar_event/query/za.rs`). The case `query::grantee_text_skips_a_sealed_class` (T6). The remaining difference from CalDAV (FC2-2) goes to the owner.

### The fix wave

One implementer, four commits on `7000924b4`:

- `3f1ce7dc5`: P1 and T1.
- `f1c69900d`: P2 and T2.
- `f0150ec57`: P3 and T6.
- `a5e45dc2f`: P4 (an empty context list refused with `Forbidden(Context)` at the head of `za_commit_update`, and by the controller's addition in `za_refresh_event`), P5 (the `OAuthGrant` arm of `crates/common/src/cache/invalidate.rs` also evicts the account's resident keys), P6 (a compile-time equality of `CODE_LEN` and upstream's `DEVICE_CODE_LEN` beside `is_server_code`); comments for FA-15, FC2-11, FD-12 and R37-14; the two evidence strings of `tests/qualification/dispositions.json` (FD-10 = R41-2, R41-3); and the cases T3 to T5 and T7 to T11 (T7 `sharing::revoked_and_unshared_calendars_hidden`; T8 the ordinary local attendee's delivery; T9 a leak scan while a hand-off entry is live; T10 `pipeline::update_without_contexts_is_refused`; T11 resident keys gone after a grant's revocation).

Its runs: TM and SHIP clean, six format checks clean, the six `traced_url` unit tests, vault 208 and groupware 59, `ci_steps.py` 0 differences, `test_gates.py` OK, and one whole ZA passed (1804 s), on the working tree before the commits were built (they were built after a two-line reorder in T1, below). Two limits held for the wave's own runs, which the re-review recorded: the unit tests of `common`, `jmap` and `http`, whose product code the wave changed, were compiled but not selected; and on the committed tree the cases T4, T5, T7, T8 and T9, and `query`'s cases after T6, had run only in that whole-suite run on the working tree (the kill runs passed the token module, `pipeline`, `events` and `calendars` on the committed tree). The final matrix's second pass closed both on the committed source of `a5e45dc2f`: rows 5 and 7 ran those crates' unit tests (895 and 7 passed), and rows 9 and 10 ran those cases. Differences from its brief that its report records: T6 sits in `query.rs` beside `grantee_privacy`, not in `sharing.rs`, and has no ordinary twin (W-3); T1's removal step checks the tokens before the binding (the first K1 run failed at the binding comparison, and the commits were rebuilt once with that reorder); K6 as prescribed is caught by an older assertion, so K6b was added; `placement_refused` asserts no description, since the placement refusals are a bare `forbidden`; `traced_url` leaves a bare `code` without `=` unmasked and turns `code=` into `code=[redacted]`.

Cost the re-review recorded for the documents: after any grant deletion of a key account (P5), each Basic CalDAV client of the account pays one Argon2 verification on its next request and another browser session one record read; the grant arm evicts the key cache, not the search index cache.

### Kill checks K1 to K6

Each on the committed tree, with a short form of the suite (a block of module calls in `tests/src/za/mod.rs` removed, never committed), and the edited files restored after each (`git status` of `crates/` and `tests/` empty).

| Id | Change made | Case that failed | First panic |
|---|---|---|---|
| K1 | P1 undone (`za_totp` without the fresh salt) | `tokens::second_factor_change_ends_sessions` | "an access token was served after the removal", left 200, right 401 (the first run, before the reorder, failed at "removing the factor restored the earlier binding") |
| K2 | the third URL site formats the URL as before | `tokens::traced_urls_mask_the_code` | "a code value appeared in a trace" |
| K3 | P3 undone | `query::grantee_text_skips_a_sealed_class` | "the grantee's match of {"title":"cloakedcanary"}", left `["l"]`, right `[]` |
| K4 | P4's guard in `za_commit_update` undone | `pipeline::update_without_contexts_is_refused` | "index out of bounds: the len is 0 but the index is 0", in `placement.rs` |
| K5 | P5 undone | `tokens::sign_in_gives_keyed_tokens` | "resident keys outlived the grant's deletion" |
| K6 | the Bearer arm's refusal counted as an authentication failure | `tokens::tampered_or_foreign_secret_is_401` | its older trace-count assertion ("zero-access: token keys unavailable", left 0, right 9), before the new ban assertions |
| K6b | the refusal counted for banning without changing its error | `tokens::tampered_or_foreign_secret_is_401` | "fork refusal 5 from one address", left 429, right 401 |

### The re-review

A fresh reviewer read the wave's diff: every item addressed (P1 to P6, the four comments, the data file, T1 to T11), no new Critical or Important breakage, the kill checks shown from their logs, the tree restored. Its five Minor findings, ruled by the controller with no second fix wave:

- **W-1** (deferred, with the fix named): a request that passed authentication before a grant's revocation can make that grant's keys resident after the eviction; they are unreachable and stay until the idle or age limit. Fix for a follow-up: insert the keys only when the authentication entry went in.
- **W-2** (residual): T7 cannot fail on the fork's fresh-context filter, which sits behind upstream's ACL masks in every state a test can build without a hook; the case stands as coverage of the answer a grantee gets, and its comment overstates.
- **W-3** (deferred): T6 has no ordinary twin.
- **W-4** (deferred): T2 exercises one of the three URL sites; the helper has six unit tests.
- **W-5** (residual): `traced_url` masks a `code` query parameter as a standard parser reads one; a callback URL percent-encoded inside another parameter's value and a `;`-separated query pass unmasked. The server generates neither.

### The disposition of every finding

- **Fixed in the wave**: FA-1 = FD-2; FA-3 = FD-1 (pending the owner's confirmation; confirmed by the owner on 2026-11-18); FA-2; FC1-2, with R32-1; FC2-1; FB-1; FA-4; FA-10 (with ledger 478); FA-15; FC2-11 (its comments; its list of copied checks goes to the documents); FD-12; R37-14's comment; FD-10 (= R41-2, R41-3); FC1-11; FC2-20; FD-8; FA-7 = FD-6 (by FA-1's case).
- **Corrected in the documents**: ledger 477 (the device-code poll's changed answer, also for the owner); FA-12 (the sweep's per-account read, also for the owner); FA-21 (tokens refused once after the upgrade); FD-14 (spec 2's delta for an operator); R35-5's wording (also for the owner); FC1-10, FA-9 and FC2-12 (the repeated upstream checks and the larger insertions, named as silent dependencies; the code stays, a residual).
- **For the owner** (the code stays until he rules): FC1-1 (with its pinning case; settled 2026-11-18: no code change, spec 3.3); FC2-2 (a line in spec 3.3); FC2-3 = R34-4 (settled 2026-11-18: fixed, `6f0704dcb`, spec 5.5); FC2-5 (D15; confirmed by the owner on 2026-11-20); FC2-6 (a default alert's id in the clear); FC2-15 (an empty `shareWith` refused; in the checklist); FD-3 (a reminder's push names a positional alert id; a line in spec 3.3); FC1-5 (a grantee's email default alert is mailed to the owner); FC1-4 (no change recommended); FA-6 = R18-8 (D8's sentence); FA-17 = R18-2 (4.5 step 5's wording); FB-3 (the placement UID check outside the lease; spec 3.3's wording; settled 2026-11-18: spec 3.3 corrected in `53cf84aa9`, issue #91); FB-5 = R33-5, with FC1-9 (an event that does not unseal answers three ways); FC1-16 (spec 8's row for missing access); FB-6 (a move out of two unshared calendars into one shared one); R18-6 with FA-13 (token refusals without a trace; a store error on the Bearer path answered 401); R28-4 (a calendar destroy's invalidation of its grantees is best-effort across a cluster: the destroy succeeds and a peer can miss it; corrected after the pull request's review, C4617611650).
- **Residual**: FA-11 (an ordinary account's consent token with a 43-character suffix accepted); FA-14 (the sweep runs only after the purge steps before it succeed); FA-20 (a node that missed an invalidation broadcast refuses the account's uncached Bearer requests); FB-2 (a create without a `uid` retried under a new UID after its fan-out); FB-7 (the assertion order of the placement and destroy batches, CalDAV's at the base); FC1-6, FC1-7, FC1-8, FC1-12, FC1-13, FC1-14, FC1-15 (the event writer's answers or order beside upstream's, as the plan prescribes, nothing stored in the clear; FC1-7 settled 2026-11-18: fixed, `d86f90b31`); FC2-7, FC2-8 (an unranged text query's views held for the request; the leaf limit checked after the candidates open); FC2-9 (one shared calendar that does not open fails a grantee's `Calendar/get` and exact-origin `/changes`); FC2-10, FC2-13, FC2-14, FC2-17 (`Calendar/set`'s context before validation; non-canonical `z`; an owner's get that completes a boundary answers the state sampled before its write, so the next `/changes` reports that calendar as updated; an owner's sync completes none); FD-13 (store errors on the issue and refresh paths keep their context); FD-16 (`za_handoff_take` deletes before it checks the secret).
- **Deferred**: FA-5 (the D22 precheck and an unreadable record); FA-8, FA-18, FA-19 (test gaps on the token paths); FA-16 (one exit of `delete_oauth_grant` skips the entry removal; bounded by the sweep); FB-4 (the placement arm's exits trace "abandoned" where the outcome is known); FB-8 = R38-6, FB-9, FB-10 (test weaknesses); FC1-3 (a repeated update of one event in one call); FC1-17 (thin coverage of the occurrence-patch rulings); FC2-18 = FD-4 = R21-12, FC2-19, FC2-21 = R28-7, FD-5, FD-7 = R40-7, FD-9 (test weaknesses); FD-11 (new fork tests in `parser_restored_tests.rs`; to the gates' run); FD-15 (`ZaPipelineError` derives `Debug`; no use site formats it).
- **Inherited** (from upstream or from the base, not this release's; one line each): FA-22 (the device-authorization answer is traced with its device code; a retry while a rotation is pending returns the next generation); FC2-16 (JMAP destroys the default calendar that CalDAV's DELETE refuses in a product build; also a line in spec 3.3 for the owner); and from the lenses: an owner's JMAP update of alarms, keywords or colour only runs unscheduled while CalDAV's bumps the schedule tag; a CalDAV DELETE by a non-owner tests ownership on the stored tree; a large collection delete can commit in part after its cancellations went out (for JMAP not inherited, and fixed for a key account on RocksDB, SQLite, PostgreSQL and MySQL: see "The pull request's review"); an unanswered attendee's JMAP create never schedules its copy; the first of two updates of one id in a call is lost; a grantee's destroy in an unshared calendar answers with the calendar's id; `/queryChanges` with `expandRecurrences` answers `cannotCalculateChanges` before the access check; a grantee's `/changes` reports updates of events it cannot see as destroyed ids; a `z`, like upstream's `r`, is bound to no account, collection or method; a windowed or property-subset read and a full read show default alerts differently.
- **Withdrawn**: FC2-4.

The deferred lines of the task reviews: no lens found one that requires a change of product behaviour before the merge. What they asked to be closed is ruled above (R36-10, R30-1 with R32-3, R33-1 and R32-1, R41-2 and R41-3, ledger 477, R35-5, R34-4, the D15 ruling, R37-14, R28-4). The run they asked for, ZA with the filesystem blob store, passed as the matrix's row 10; the inventory and coverage gates waited for the owner's word, and were dispatched on it on 2026-11-19 ("Where things stand"; the coverage gate failed, "The pull request's second review").

## The pull request's review

### How it ran

Pull request #90 received a static review of `dbd2fccc8`, posted on the pull request as twelve inline findings, three rated P1, seven P2 and two P3 by its author. The project owner's word on it: the findings are an external reviewer's claims, verified before anything changes, and none overrides an earlier decision by itself. Each finding was traced against the code at `dbd2fccc8` before any change, in two read-only verifications (one for the calendar and pipeline findings, one for the sessions and the drain). All twelve were confirmed; one was wider than stated (C4617611565) and one reached the update as well as the destroy (C4617611549). The project owner decided the open ones on 2026-11-18; the controller ruled on the rest ("Rulings", "The pull request's review"). Two fix waves changed the code: wave A the calendar and the pipeline, wave B the sessions and the drain. Each wave was reviewed by a read-only reviewer, each review's findings were ruled on and fixed or recorded, and each fix round was re-reviewed. Each thread of the review has a reply with the verdict and, where something changed, a second reply, posted after the push, with the outcome and the commits; the five threads of the second review have a reply with the verdict and the plan.

The round's commits on top of `dbd2fccc8`, in order: `53cf84aa9` (spec 3.3's wording on the UID lease); wave A `c7dfbce95`, `efb845a4b`, `461f0dd19`, `5f736c77f`, `c65c38347`, `d86f90b31`; wave B `6f0704dcb`, `d50107f12`, `dff25d1d9`, `7bd35c4c4`; wave A's fix round 1 `cf50cd2bb`, `6fbe81a1a`, `0a6139d7a`, `7304f9236`; wave B's fix round 1 `0ca47a61a`; wave A's fix round 2 `5e0e8620b`, `25e098520`. The code's head is `25e098520`.

Decided by the project owner on 2026-11-18: spec 3.3 corrected for the UID lease, with follow-up issue #91 and no code change (C4617611574 = FB-3); the drain only within the token's scope, on JMAP and CalDAV, and the search pass under the same kind of condition (C4617611594 = FC2-3); the occurrence ruling fixed in the wave (C4617611635 = FC1-7); a key calendar's delete in one transaction on both protocols, refused above 5,000,000 bytes before any cancellation, with issue #92 to carry notes of what this pull request does and what stays open (C4617611521; the notes as added: "Where the verification changed an earlier position"); the fence against a crossing write, with a store-level test (C4617611506); no code change for the UID privacy-conflict answers (C4617611630, inherited); FC1-1 recorded in spec 3.3 with no code change; the `code` masking confirmed as an amendment of spec section 10; the co-author line of four documentation commits left as it is (R44F-5); the replies posted on the review threads after the verifications.

### The twelve findings

| Id | Finding (short) | Also known as | Decided by | Outcome |
|---|---|---|---|---|
| C4617611506 | A calendar destroy's child list is not fenced against a create that crosses it | inherited from upstream, and wider there | the project owner | fixed: `efb845a4b`, with `6fbe81a1a` and `0a6139d7a` |
| C4617611521 | A calendar destroy can split across commit points | the final review recorded it as inherited; for JMAP it is not | the project owner | fixed: `c7dfbce95`, with `6fbe81a1a` and `7304f9236` |
| C4617611549 | A grantee can destroy or change an event whose effective privacy is private under a sealed `CLASS` | the class of FC2-1, on a mutation | the controller | fixed, JMAP only: `461f0dd19` |
| C4617611565 | The event state after a sharing boundary is unusable | near FC2-17; wider than stated (the grant itself is a boundary; `/get`, `/query` and `/set` hand out the same state) | the controller | fixed, with a limit: `5f736c77f`, narrowed by `cf50cd2bb` and by fix round 2 |
| C4617611574 | The UID lease is not held when an event is moved into a calendar | FB-3 | the project owner | spec wording (`53cf84aa9`), issue #91 |
| C4617611584 | The removed href is taken from a request-start cache | new | the controller | fixed: `c65c38347` |
| C4617611594 | The drain runs before the calendar capability is checked | FC2-3, R34-4 | the project owner | fixed: `6f0704dcb`, tests extended in `0ca47a61a` |
| C4617611602 | A grant's deletion cleans the token record before its cached sessions are evicted | new; closes FA-16 with it | the controller | fixed: `d50107f12` |
| C4617611615 | A keyed cache entry is not bounded by the signed expiry | new; closes W-1 with it | the controller | fixed: `dff25d1d9`, `7bd35c4c4` |
| C4617611630 | UID privacy-conflict answers depend on events a grantee cannot see | inherited | the project owner | no code change |
| C4617611635 | One refused occurrence fails its siblings | FC1-7, R31-2 | the project owner | fixed: `d86f90b31` |
| C4617611650 | The outcome note's description of R28-4 | documentation | the controller | corrected in this note (the entries of R28-4 under "The final review", "Deferred findings" and "Decisions and remaining qualification") |

### Where the verification changed an earlier position

The final review recorded the split calendar destroy among the inherited behaviours ("a large collection delete can commit in part after its cancellations went out"), and the project owner first accepted the recommendation to leave it to a follow-up, issue #92. The verification showed that for JMAP it is not inherited: upstream's JMAP destroy of an ordinary account's calendar deletes the calendar and its events in one transaction, and the split came from release 6 routing a key account's destroy through CalDAV's function. The earlier acceptance was therefore replaced by the fix: on the verification's evidence the owner decided to fix it in this round, on both protocols. Issue #92 carries a note of 2026-11-18 that this pull request makes the delete one transaction, and a note added on 2026-11-19, after the push, of what it keeps: a calendar too large for one transaction, the stores that keep the chunked form, the fence's refusal after the cancellations, a large delete retried whole on a conflicting commit, and PostgreSQL and MySQL read, not run. Its title and body were not edited. It is inherited for CalDAV only, where upstream's collection `DELETE` of an ordinary account splits the same way.

The verification also widened two findings: C4617611565, since a grant is itself a sharing boundary and `CalendarEvent/get`, `/query` and `/set` hand out the same unusable state as `/changes`; and C4617611549, since the same flags-only check opened the update, through which a grantee could declassify and rewrite the event. C4617611506 is inherited from upstream, which has it in a wider form for an ordinary account.

### Wave A: the calendar and the pipeline

| Item | Commit | What it changed |
|---|---|---|
| A1 | `c7dfbce95` | A key calendar and its events are deleted in one transaction: the flag `za_one_transaction` makes `commit_point()` a no-op, `FORCE_COMMIT_SPLIT` included; `za_exceeds_one_chunk` refuses a batch above 5,000,000 bytes by the builder's own count with `ZaPipelineError::CalendarTooLarge`, before the fan-out (JMAP `forbidden` with a fixed text, CalDAV 403). Release 5's `chunked_collection_delete` was rewritten to the new outcome. |
| A2 | `efb845a4b` | `ZaChildFence`: the child list compared with a snapshot no older than the account's Calendar change counter before the first fan-out and again immediately before the commit (`Busy(ZaBusy::CalendarChanged)` on a difference), and a merge operation on the default calendar id that fails the commit unless its Calendar change id is the one after the counter read. A store unit test, `za_change_id_fence_tests.rs`. |
| A3 | `461f0dd19` | For a caller who is not a member, `CalendarEvent/set`'s update and destroy check the higher of the stored flags' privacy and the unsealed event's; an event whose view does not open answers `notFound`. CalDAV unchanged. |
| A4 | `5f736c77f` | `za_event_state`: a non-owner of a key account is given the snapshot's highest change id by `/get`, `/query` and `/set`, and `/changes` raises a final exact state to the tip of a snapshot read before the page. |
| A5 | `c65c38347` | The removed event's href is taken from the parent the delete or placement asserted, not from the request-start cache; the two `expect`s became a `Forbidden(Context)` refusal. |
| A6 | `d86f90b31` | A refused attachment link fails only its own occurrence operation; the others are applied and committed together. |

Differences from the designs:
- The change-id fence is not new to the codebase: release 4's existing-destination collection MOVE uses it (`za_move_overwrite_container`) and refuses FoundationDB, a read replica and the ephemeral store up front.
- The store property the fence relies on (a batch's change id allocated before its merge functions run, and an error from one aborting the transaction) holds on RocksDB and SQLite and fails on the ephemeral store, which keeps the earlier writes. The brief said to stop in that case; the implementer went on and reported it, and the controller accepted that (the condition was meant for a data store with calendars).
- The verification's design raised every final page of a grantee's sync; the wave's review showed that it skipped event changes on a paged continuation (RA-1). Fix round 1 guarded the raise and fix round 2 removed it for an initial-origin continuation.
- The verification's design put the one transaction on every store; the wave's review showed that on FoundationDB it fails a large delete on every attempt (RA-2). Fix round 1 keeps the earlier form on FoundationDB, a read replica and the ephemeral store (since the second review that form also takes the members from the stored records and removes every name under the calendar; spec 5.3).
- A4 changed `paged_sync_retention_collision`'s final-state assertion to the log's tip, a change its brief did not list; fix round 2 restored the form it had at `53cf84aa9`.
- A5's new early return in the placement loop leaves the mail guard unsettled, as the loop's other early returns do (RA-12; unreachable).

Its runs, on `d86f90b31` unless a line says otherwise (logs in the git-ignored run directory):

| Run | Result |
|---|---|
| TM after each item and on each intermediate commit's tree | clean (one attempt failed on a misplaced hunk before its commit was made; the staging was redone) |
| `cargo test -p store --lib --features rocks,sqlite,test_mode -- za_change_id_fence one_transaction_batch_never_splits` | 4 passed (the first attempt did not compile and was corrected) |
| format checks of `store`, `groupware`, `dav`, `jmap`, `tests`; SHIP; TM | clean, no warning |
| `cargo test -p vault -p groupware --features vault/test_mode` | 267 passed |
| the workspace unit command filtered to the wave's unit tests | 8 passed |
| ZA, whole | passed (1666.85 s in the test) |
| WK; JK | passed |

The host's one-minute load was 90 to 175 during most of the run; the runner's gate held the suite until it fell below 40.

**Fix round 1** (`cf50cd2bb`, `6fbe81a1a`, `0a6139d7a`, `7304f9236`, on `7bd35c4c4`): F1 (RA-1), the guarded raise: a `z` continuation's final state is raised only when the log holds no item change between it and the tip, with a test that pages to the end of a range and then reads what followed; F2 (RA-2, RA-5, RA-6, RA-7), the one transaction, the size refusal and the fence only where the store has the contract, with the hook `UNFENCED_CALENDAR_DELETE` and the case `chunked_collection_delete_unfenced` pinning the earlier form; F3 (RA-3), the hook `CALENDAR_DELETE_FENCE_PAUSE` and the case `destroy_fence_lost_to_a_crossing_write`, in which only the merge operation can refuse, over JMAP and CalDAV; F4, the hook `COMMIT_BYTES_OVERRIDE` and the case `size_refusal_before_any_cancellation`; F5, RA-14's two steps in `grantee_held_to_effective_privacy`, RA-9's and RA-7's comments. After the runs two test-only changes were folded into F3 (the CalDAV step, which the last whole-suite run covered, and a correction of its comment), rewriting the round's own last two commits. Its runs: TM per commit and SHIP clean, format checks clean, a short-form ZA before committing passed, vault and groupware 267, the filtered workspace units 8; the first whole ZA failed in a module the round did not touch (the rule-expansion trace tests, the 500 ms DAV client timeout at `tests/src/utils/webdav.rs:120`, while the one-minute load rose from 18 to 112), the rerun passed (1700.74 s in the test), and the whole ZA after the CalDAV step passed (1695.59 s); WK and JK passed.

**Fix round 2** (`5e0e8620b`, `25e098520`, on `0ca47a61a`): H1, no raise on a continuation of an initial-origin sync (the tip is not read for it), `paged_sync_retention_collision`'s assertion back to its form at `53cf84aa9`, and the cases `initial_sync_not_raised_over_a_grant` and `exact_sync_raised_over_a_container_change`; H2, the parked CalDAV `DELETE` of `destroy_fence_lost_to_a_crossing_write` asserts no `Retry-After` beside its 503, which tells the fence's refusal from a lost assertion and pins the lost-flag arm. Its runs: TM and SHIP clean, the format checks of `jmap` and `tests` clean, and a short-form ZA on `25e098520` (lines 146 to 192 of `tests/src/za/mod.rs` removed, the JMAP calendar modules kept) passed (1461.98 s in the test); the whole suite was not run, as its brief said. The limit of `initial_sync_not_raised_over_a_grant`'s fixture: the module's two setup events are older than the granted calendar's event, so that event is not the range's first change as in the review's interleaving and can be sent while the sync pages; the case asserts the final-state rule, which is the fault.

### Wave B: the sessions and the drain

| Item | Commit | What it changed |
|---|---|---|
| B1 | `6f0704dcb` | `za_drain_keys`, the one scope rule of the CalDAV and the JMAP drain (the caller's own keys, only when the token's scope admits calendars), and `za_call_admitted`, the JMAP call condition (the call parsed and its capability is in the scope-filtered `using`); the search pre-pass takes the same call condition. The helper `za_keyed_grant`. |
| B2 | `d50107f12` | `za_evict_deleted_grant`: a deleted grant's sessions evicted on this node and the eviction broadcast before the token record's cleanup is awaited; the exit whose re-read finds the grant gone now removes the entries (FA-16). The hook `TOKEN_FORGET_PAUSE`. |
| B3 | `dff25d1d9` | Upstream's "Cache credentials" block moved above the key lookup, so the entry's deadline is fixed at verification; only a live entry is published; `za_insert_keys_fenced` inserts the keys only under the revocation epoch read before verification, and the `OAuthGrant` arm removes the account's keys after the epoch advances (W-1). |
| | `7bd35c4c4` | `za_keyed_grant` seals with a server built from the current configuration: the first ZA run failed in B3's own case because the in-process helper sealed with the configuration of server start, so a "three-second" token had the one-hour default (a test fault; RB-15). |

Differences from the designs: B3's two cases run right after `bearer_crossed_by_vault_write_is_refused`, where nothing of `token-owner` is cached, not near the sweep's case; the expiry case checks the keys by the token's fingerprint; B1's case adds a step with the calendars capability left out of `using`; B2's eviction is a fork helper called in one line from `grant.rs`; B-K3 (K3 in the wave's report; this note names wave B's three kill checks B-K1 to B-K3, apart from the final review's K1 to K6) failed at an earlier assertion than its brief predicted.

Its runs: the format checks of `common`, `jmap`, `dav`, `tests` and `http` clean, `cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise` 7 passed, `cargo test -p common --lib` 208 passed, SHIP and TM clean, all on `dff25d1d9`; ZA on `dff25d1d9` failed at `tokens.rs:570` ("an expired token's entry was published", the test fault above); on `7bd35c4c4`, TM and the `tests` format check run directly, clean, ZA passed (1771 s wall), and WP, WK, IP, IK, JP and JK passed. No allowed rerun was needed.

**Fix round 1** (`0ca47a61a`, on `7304f9236`): G1 (RB-1), an unparsed `Calendar/get` step in `drain_only_within_the_token_scope`; G2 (RB-2), an unknown method and an unparsed `Email/get` in `case_scope_excludes_mail`, under one search-pass counter; G3, `za_keyed_grant` seals after the record's entry is issued and the expiry case uses a five-second token and a seven-second park (RB-9), the contacts-only token reads its session first so its `PROPFIND` is a cache hit (RB-10), the expiry case's heading (RB-11), and `drain.rs`' header states the new trigger (RB-13's comment part). Its runs: TM (on the uncommitted tree equal to the commit), the format checks of `tests` and `groupware` and SHIP clean, and a short-form ZA (setup, `tokens::test`, `jmap_calendar::scheduling::test` and `mail::index::test` kept) passed (549 s wall).

### Kill checks

Each ran on a committed tree with a short form of the za suite (module calls removed from `tests/src/za/mod.rs`, never committed), and the edited files were restored after each. The line numbers in the table are those of the tree each check ran on.

| Id | Change made | Case that failed | First panic |
|---|---|---|---|
| kA1 | the `za_one_transaction` branch at the head of `commit_point()` removed | `calendars::destroy_is_one_transaction`, its forced-split step | "Missing destroyed in response": `notDestroyed`, `forbidden`, "The server could not complete this change, try again." |
| kA2 | the fence undone (`current()` compares nothing, the merge never fails) | `pipeline::calendar_delete_with_a_stale_child_list` | "deleted with a stale child list" (`pipeline.rs:2028`) |
| kA3 | both effective-privacy checks disabled | `sharing::grantee_held_to_effective_privacy` | "Missing not destroyed item": the grantee destroyed the veiled event |
| kA4 | the raise in `za_calendar_changes` disabled | `sharing::grantee_state_above_the_boundary` | `assert!(s > b)`, "Exact(23) 26" (`sharing.rs:527`) |
| kA5 | the request-start guard restored at the destroy | `pipeline::removed_href_from_the_asserted_parent` | left `[]`, right the `pipe-late` href (`pipeline.rs:2116`) |
| kA6 | the `try_fold` that fails every sibling restored | `events::occurrence_link_refused_alone` | `destroyed == [occ3]` failed; all three occurrences answered `invalidProperties` on `links` (`events.rs:1686`) |
| x1-kF1 | the raise made unconditional | `sharing::paged_sync_origin_survives_range_changes` | "the range-final page", left `Exact(58)`, right `Exact(54)` (`sharing.rs:818`) |
| x1-kF2 | the `UNFENCED_CALENDAR_DELETE` switch ignored | `imip::outbound`'s `chunked_collection_delete_unfenced` | "the chunked DELETE did not fail", left 204, right 500 (`outbound.rs:1290`) |
| x1-kF3a | the merge operation's check made to pass always | `calendars::destroy_fence_lost_to_a_crossing_write` | "Missing not destroyed item": the calendar was destroyed |
| x1-kF3b | the lost-flag arm in `za_commit_delete` removed | **none at first**: the run passed the JMAP calendar module and died later in an unrelated mail module (a JMAP client timeout while the load rose to 50) | without the arm the lost fence answers as a lost assertion; JMAP answers both alike, and the round's report said no answer told them apart. The re-review showed that CalDAV's lost assertion carries `Retry-After` and the fence's bare 503 does not; fix round 2's H2 asserts it, and x2-kH2 then failed |
| x1-kF4 | the size check moved into `za_commit_delete` after the fan-out | `calendars::size_refusal_before_any_cancellation` | "a cancellation reached key2", left `[14, 15, 16]`, right `[14]` (`calendars.rs:1021`) |
| x2-kH1 | `initial_origin` forced false | `sharing::paged_sync_retention_collision`, its restored assertion, which runs before the new case | left `Exact(34)`, right `Exact(32)` (`sharing.rs:679`) |
| x2-kH1b | the same, with the new grant case called first (a local reorder, not committed) | `sharing::initial_sync_not_raised_over_a_grant` | "the final state Exact(38) is above the grant's boundary 37" |
| x2-kH2 | the lost-flag arm removed | `calendars::destroy_fence_lost_to_a_crossing_write`, its CalDAV step | "the fence's refusal answered as a lost assertion", left `Some("1")`, right `None` (`calendars.rs:968`) |
| B-K1 | `za_drain_keys`' scope filter made to pass always | `scheduling::drain_only_within_the_token_scope` | "a mail-only PROPFIND drained", left 0, right 1 (`scheduling.rs:1715`) |
| B-K2 | the `za_evict_deleted_grant` call in `grant.rs` deleted | `tokens::deleted_grant_evicted_before_cleanup` | "a deleted grant's token was served while its cleanup was parked", left 200, right 401 (`tokens.rs:1316`) |
| B-K3 | `is_live(&entry)` made to pass always | `tokens::keyed_entry_bounded_by_token_expiry` | "an expired token's entry was published" (`tokens.rs:570`) |
| y1-k1 | `za_call_admitted`'s `RequestMethod::Error` return disabled | `scheduling::drain_only_within_the_token_scope`, the unparsed step | "a Calendar/get that did not parse drained", left 0, right 1 (`scheduling.rs:1715`) |
| y1-k1b | the same, with the scheduling module left out | `mail::index`'s `case_scope_excludes_mail`, the unknown-method step | "a mail-scoped token's call of an unknown method ran a search pass", left `[1, 1, 2, 1, 0]`, right `[0; 5]` (`index.rs:2064`) |
| y1-k2 | `za_call_admitted` removed from the search pre-pass's condition only | `mail::index`'s `case_scope_excludes_mail` | "a calendars-only token's refused Email/get ran a search pass" (`index.rs:2044`) |
| y1-k3 | `za_insert_keys_fenced`'s epoch checks made to pass always | `tokens::keyed_request_crossed_by_revocation` | "no resident keys for the account", from `assert_nothing_cached` (`tests/src/utils/za.rs:356`) |

Not shown by a run: `case_scope_excludes_mail`'s unparsed `Email/get` step (the suite stops at the first panic; it rests on the same return as the unknown-method step), and the expiry case at five and seven seconds (B-K3 ran at three and four).

### The reviews and their rulings

**Wave A's review** (read-only, at `d86f90b31`): no Critical, four Important, thirteen Minor.
- RA-1 (a grantee's paged sync skipped event changes written while it paged): fixed (F1). The hole was in the verification's design, which the brief passed on.
- RA-2, RA-5, RA-6, RA-7 (the one transaction and the fence on FoundationDB, a read replica and the ephemeral store): the first ruling, no store distinction with the limit recorded, was **withdrawn** when the review showed that on FoundationDB one transaction makes the delete of a calendar of a few thousand events fail on every attempt, after delivering its cancellations each time. The ruling that replaced it: the one transaction, the size refusal and the fence apply where the store can establish them (RocksDB, SQLite, PostgreSQL, MySQL); on FoundationDB, a read replica and the ephemeral store a key calendar's delete keeps the form it had before the wave (F2; since the second review that form also takes the members from the stored records and removes every name under the calendar, spec 5.3). A backend the fork does not qualify does not lose a function it had.
- RA-3 (no test failed when the merge operation was wrong): fixed with a case in which only the merge operation refuses (F3).
- RA-4 (a fenced refusal lands after the cancellations went out, and more writes cause it): an accepted residual, put to the owner and confirmed by the project owner on 2026-11-19.
- RA-9 (the size count leaves out assertions and the change-log record): the size check's comment corrected in F5; the count is unchanged. RA-14 (two missing steps of the effective-privacy case): fixed in F5. Both were taken into fix round 1 by the controller's ruling ("with the fix round go the cheap ones").
- The size refusal got an end-to-end case (F4), since the owner decided "before any cancellation".
- Deferred: RA-8, RA-10, RA-11, RA-12, RA-13, RA-15, RA-16 (inherited), RA-17, and a failing CalDAV collection `DELETE`, no longer exercised over CalDAV ("Deferred findings").

**Its fix round 1's re-review** (at `7304f9236`): all findings addressed, no new Critical or Important breakage, three new Minor ones. The rerun after the timeout in an untouched module was accepted (the 500 ms DAV client timeout at a recorded load spike). (1) The raise lifted an initial-origin paged sync over a grant that completed while it paged: fix round 2 removes the raise for a continuation of an initial-origin sync. (2) A restart is still forced once in a corner of that flow: accepted, it is that flow's behaviour before the wave; after fix round 2 it covers every paged sync from the initial state and can repeat (its re-review, below; "Accepted residuals"). (3) The lost-flag arm was unpinned, and the round's reason (no answer tells the two apart) was wrong for CalDAV: fix round 2 adds the `Retry-After` assertion. A follow-up was proposed to the owner and filed on his word of 2026-11-19 as issues #93 and #94 ("Decisions and remaining qualification", item 22); an earlier wording of this record named a third case that the last re-review's trace showed cannot arise from a state the server issued.

**Its fix round 2's re-review** (at `25e098520`): both items addressed, no new Critical or Important breakage. It traced the state rule for every way a non-owner's `CalendarEvent/changes` or `/queryChanges` call can end and found no case in which an event change is skipped. Its one new Minor item is the accepted cost of the ruling: a paged sync from the initial state ends below a sharing boundary that was complete before it began, as well as one that completed while it paged, and a restart that pages again is refused again (recorded with fix round 1's item (2) under "Accepted residuals"); with it, `paged_sync_retention_collision` pins that final state but no case makes the call that follows it. Outside the fix diff it observed a window within one call: a grant that commits between the boundary check and the tip read leaves a raised state above the grant, of an exact request or of the last page of an exact-origin continuation (spec 6.8, case (b)); and it noted that the parked CalDAV `DELETE` of the fence case is sent with a five-second client timeout and held at a pause, older than the round. All three are deferred ("Deferred findings").

**Wave B's review** (read-only, at `7bd35c4c4`): no Critical, two Important, seventeen Minor, no fault found in the product code.
- RB-1 and RB-2 (both cases passed without the "parsed" half of their rule): fixed (G1, G2), with kill checks for the three conditions RB-8 named (y1-k1, y1-k1b, y1-k2, y1-k3).
- RB-3 (a token whose scope excludes mail still runs the search pass through an admitted `Blob` call, since every interoperable scope carries the blob capability): accepted as the decision reads; the call is served, not refused, and the pass is catch-up work. Put to the owner and confirmed by the project owner on 2026-11-19.
- RB-4 and RB-5: the drain's absence is the rule the owner decided. What a token's scope admits on DAV is upstream's scope model, unchanged; the drain's scope rule limits the drain, not what a token can read. The project owner's disposition of 2026-11-19: no change for it in this pull request.
- RB-9, RB-10 and RB-11 (margins and a heading): fixed (G3).
- RB-13 (the documents and `drain.rs`' header state the old trigger): the header in G3, the documents in this record's commit.
- RB-17 (the implementer's concern about the post-write search pass is not reachable): an earlier ruling to extend the condition to the post-write pass was **withdrawn**. The post-write pass runs only after an admitted `Email/set` or `Email/import` committed an ingest in the same request, which a token whose scope excludes mail cannot reach; it is unchanged.
- RB-16 (the wave's commits name a different co-author than the brief): they stay, by the owner's word on the same question.
- Deferred: RB-6, RB-7, RB-12, RB-14 (the doubled eviction's cost, in spec 10 and `CLAUDE.md`), RB-15, RB-18 and RB-19 (inherited) ("Deferred findings").

**Its fix round 1's re-review** (at `0ca47a61a`): every finding addressed, no new Critical or Important breakage; two wording nits recorded, not changed: the expiry case's heading says "sub-second skew", which understates what remains (the time from the decode to the cache deadline, plus under a second of rounding), and one comment line of `drain.rs` is 90 columns. Wave B is complete. (The heading was rewritten by the second review's wave C, `90d6ad286`; the first nit no longer applies.)

### Validation on the final source

The runs above are the waves' own, each on the tree its row names. This subsection records the runs on the final source (logs in the git-ignored run directory). Rows 1 to 16 ran with `25e098520` checked out, on a clean tree; rows 17 to 31 ran with `063af481c` checked out (the first commit of this record, documentation only), whose `crates` and `tests` trees are those of `25e098520` (`d5084d5e5` and `89731f438`), so every row ran the source of `25e098520`. Of 31 rows, 30 passed on their first attempt and one failed (row 24); row 24's unchanged command was then run twice more and passed both times.

| Row | Command | Result | Wall time | Head |
|---|---|---|---|---|
| 1 | `cargo fmt --all --check` | no output | 6 s | `25e098520` |
| 2 | `python3 -I scripts/tests/ci_steps.py` | exit 0 | 0 s | `25e098520` |
| 3 | `python3 -I scripts/tests/test_gates.py` | exit 0 | 5 s | `25e098520` |
| 4 | SHIP (`cargo check -p stalwart --no-default-features --features rocks`, with the warning filter) | clean | 115 s | `25e098520` |
| 5 | TM (`cargo check -p tests --tests`, with the warning filter) | clean | 65 s | `25e098520` |
| 6 | `python3 -B -m unittest discover -s scripts/release -p 'test_*.py' -v` | 25 tests OK | 4 s | `25e098520` |
| 7 | product build: `cargo build --release --locked -p stalwart --no-default-features --features rocks` (the full release profile, with the warning filter) | clean | 1206 s | `25e098520` |
| 8 | `python3 scripts/release/verify_product.py --binary target/release/stalwart --source-commit 25e09852058647bdfce54367757eaae070df6413 --ref refs/heads/release6-jmap-calendars --output <run directory>/stalwart.provenance.json` (without `--inspect-elf`: this host builds no ELF) | passed, provenance file written | 0 s | `25e098520` |
| 9 | `python3 -B -m unittest discover -s tests/product -p test_fresh_store.py -v` | 9 tests OK | 7 s | `25e098520` |
| 10 | FRESH: `python3 -B tests/product/fresh_store.py --binary target/release/stalwart --log-dir <a new directory>` | "fresh store: PASSED: 9 readback checks after the restart" | 13 s | `25e098520` |
| 11 | `cargo test -p vault -p groupware --features vault/test_mode -- --nocapture` | vault 208 passed, groupware 59 passed | 126 s | `25e098520` |
| 12 | U-ws (`cargo test --workspace --exclude tests --exclude scim --exclude scim-proto --lib --bins --features "$UNIT_TEST_FEATURES"`, the list read from `test.yml`) | 899 passed, 2 ignored, 0 failed | 264 s | `25e098520` |
| 13 | `cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise` | 7 passed | 68 s | `25e098520` |
| 14 | `cargo test -p smtp --lib --bins --features test_mode` | 16 passed | 50 s | `25e098520` |
| 15 | `cargo test -p tests za::leak::tests::try_archive_short_values_are_not_archives -- --exact --nocapture` | 1 passed | 133 s | `25e098520` |
| 16 | ZA: `cargo test -p tests za::za_tests -- --nocapture`, `BLOB_STORE` unset | 1 passed (1704.11 s in the test), no rerun | 1795 s | `25e098520` |
| 17 | ZA-FS: the same with `BLOB_STORE=FileSystem` | 1 passed (1606.99 s in the test), no rerun | 1809 s | `063af481c` |
| 18 | SWAP: `cargo test -p tests za::cache_swap::cache_swap_tests -- --exact --nocapture`, `BLOB_STORE` unset | 1 passed | 26 s | `063af481c` |
| 19 | WP: `cargo test -p tests webdav::webdav_tests -- --nocapture` | 1 passed, no rerun | 23 s | `063af481c` |
| 20 | WK: the same with `ZA_KEY_ACCOUNTS=1` | 1 passed, no rerun | 27 s | `063af481c` |
| 21 | IP: `cargo test -p tests imap::imap_tests -- --nocapture` | 1 passed | 14 s | `063af481c` |
| 22 | IK: the same with `ZA_KEY_ACCOUNTS=1` | 1 passed | 15 s | `063af481c` |
| 23 | JP: `cargo test -p tests jmap::jmap_tests -- --nocapture` | 1 passed, no rerun | 144 s | `063af481c` |
| 24 | JK: the same with `ZA_KEY_ACCOUNTS=1` | **failed** (exit 101) in upstream's `calendar::alarm` test, below; the two later runs of the same command passed | 198 s | `063af481c` |
| 25 | `cargo test -p tests 'webdav::cal_query::' -- --nocapture` | 2 passed, 1 ignored (upstream's `#[ignore]`) | 1 s | `063af481c` |
| 26 | `cargo test -p tests store::store_tests -- --nocapture` | 1 passed | 14 s | `063af481c` |
| 27 | `cargo test -p tests store::blob::blob_tests -- --nocapture` | 1 passed | 9 s | `063af481c` |
| 28 | `cargo test -p tests store::lookup::lookup_tests -- --nocapture` | 1 passed | 16 s | `063af481c` |
| 29 | `cargo test -p tests store::search_tests -- --nocapture` | 1 passed | 264 s | `063af481c` |
| 30 | `cargo test -p tests telemetry::telemetry_tests -- --nocapture` | 1 passed | 10 s | `063af481c` |
| 31 | SYS: `cargo test -p tests system::system_tests -- --nocapture` | 1 passed | 66 s | `063af481c` |

Rows 15 to 31 ran with `STORE=RocksDb RUST_MIN_STACK=16777216`. The product binary's `--version`: `Stalwart Server fork v0.4.0-beta.1 (upstream v1.0.3 8b32686532d34623e9ce8e1237cf2bed9554af38; source 25e09852058647bdfce54367757eaae070df6413; clean)`; the verifier (row 8) printed the same line and passed, and the fresh-store script (row 10) passed its nine readback checks after the restart. Row 12 rewrote the same 26 tracked fixtures under `crates/dav-proto/resources/requests/` as at Task 9 and the final matrix (upstream's restored `parse_requests` test rewrites them on every run); they were restored with `git checkout -- crates/dav-proto/resources/requests/` before the next row, and nothing else was restored. The highest one-minute loads: row 16 128.34, row 17 54.49, row 29 103.23, row 30 37.67; the other suite rows between 16.79 and 27.27.

**Row 24 failed.** Key-mode `jmap::jmap_tests` failed once, in upstream's `calendar::alarm` test (`tests/src/jmap/calendar/alarm.rs:166`, "EventSource alarms do not match"): the EventSource channel held the alert `k1` only, where `k2` then `k1` are expected. The test is upstream's (no fork commit has touched it) and runs its upstream body in key mode: it creates an event that starts five seconds after the request with an alert due four seconds before the start (`k2`, about one second after the create) and one due two seconds before it (`k1`), then waits up to seven seconds for both on EventSource and WebSocket. The one-minute load was 17 to 23 during the run. The unchanged command was then run twice more on the same head, with the same environment: both passed (198 s and 197 s wall; 196.98 s and 195.60 s in the test), at a one-minute load that peaked at about 35. The controller classified the failure as a timing failure of upstream's alarm test, not a result of this round's changes. Its grounds: the first alert is due about one second after the create, at a load of 17 to 23; the suite passed in plain mode in the same run (row 23) and in key mode on `7304f9236` and `7bd35c4c4` during the waves; between `7304f9236` and the final code only `crates/jmap/src/changes/za_calendar.rs` (a non-owner's event state in `/changes`) and a comment in `crates/groupware/src/calendar/drain.rs` differ under `crates/`, and the alarm path reaches neither; and two runs of the unchanged command passed. Its limits: one failure in three runs on this code is a small sample; the test's failure rate on the base branch was not measured; and CI's key-mode JMAP step has no rerun, so the same failure there would need one.

Not run in this validation: the SMTP tests (`cargo test -p tests smtp`; no SMTP file changed in the round, and CI runs them on the pull request), `directory_tests` and `automation_tests` (they need Docker), the independent CalDAV client, and the retention gates. Everything here is local: RocksDB, one node, debug test builds (except the product build of row 7), macOS. A passing suite is not review approval.

## The pull request's second review

### How it ran

After the push of `d6929c89e` the pull request received a second review (review 5981013324, 2026-11-19), a source review of the pushed head with no builds or tests. Its summary found the twelve earlier findings addressed, tracked in #91 to #94, or explicitly retained (the account-wide UID conflict response), and raised five new inline findings. It was handled as the first review was, by the project owner's rule from the first round (an external reviewer's claims, verified before anything changes); his words of this round only said that new comments had arrived. Each was traced against the code at `d6929c89e` in one read-only verification; all five were confirmed. The rulings on them, and the coverage wave, are the controller's and were reported to the owner; none was put to him as a decision. Each of the five threads has a reply with the verdict and the plan.

### The five findings

| Id | Finding (short) | Verdict | Whose it is | Outcome |
|---|---|---|---|---|
| C4633314478 [P1] | A child that moved out of the calendar and back while the delete was built is skipped and left naming the deleted calendar | confirmed; very unlikely (two moves of one event placed around its archive read) | a gap in the fence of `efb845a4b`; the skip itself is upstream's; the wave had narrowed an inherited exposure | fixed: `11a681bbd`, reordered in `cef1884a5` |
| C4633314492 [P2] | An event with two names in the deleted calendar keeps the second | confirmed; no race needed | release 4's keyed CalDAV `COPY` skips, for a `COPY` with an overwrite inside one calendar, the UID check upstream makes; the first-name unlink is upstream's | fixed at both ends: `b4d640c3c` |
| C4633314503 [P2] | A second event stored at the same href is not in the child list | confirmed with corrections (it is not reachable by one client's ordinary requests; the enumeration is inherited) | inherited, the state and the orphan | the delete fixed: `85c23c154`, `8e9baf7b7`; the name fence on creates not taken |
| C4633314510 [P3] | The fence's operation is added after the size check | confirmed; at most ten bytes | this pull request's waves | fixed: `45fa3a034`, `2f00c2abf` |
| C4633314515 [P2] | The cache deadline adds the decode-time lifetime after the lookups | confirmed; the overrun is the two lookups plus under a second of rounding | inherited for every account; reached by a key account's token since release 6 caches one | fixed for a keyed entry, in the fork's publication step: `90d6ad286`, `294ebba20` |

### Wave C, the cache part

| Commit | What it changed |
|---|---|
| `90d6ad286` | In the keyed arm of `authenticate_uncached`, keys opened from a Bearer token pass through the new `za_bearer_entry` before the liveness test: it opens the token again and lowers the entry's deadline to the fresh remaining lifetime less one second, never raising it; a decode error (an expired token) publishes nothing, and the request is served with its keys. Keys from Basic keep the entry as built; D23's refusal is unchanged. The test hook `TOKEN_VERSION_CHECK_PAUSE` and the case `keyed_entry_ends_before_signed_expiry`. |
| `294ebba20` | The follow-up round: the cache clock is read before the token is decoded (RC5-7), upstream's import list is restored and the grant type named by its path (RC5-13), the comments above upstream's cache block and on the hook (RC5-9, RC5-11), the new case `expired_second_decode_publishes_nothing`, in which the decode-error arm alone decides (RC5-1), an assertion that an entry was published before the expiry and messages on the cache assertions (RC5-4, RC5-12), and a fifteen-second token released at ten seconds (RC5-5). |

Differences from the design: the verification's test used a five-second token, a three-second park and a request at six seconds; with both whole-second clocks counted that left under a second of margin, so the case used ten, five and 11.2 seconds, then (RC5-5) fifteen, ten and 16.2 seconds, the brief's example of twelve and eight seconds failing the third margin when the first decode comes five seconds late.

Its runs, on `90d6ad286`: TM before the commit, the format checks of `http`, `common` and `tests`, `cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise` (7 passed), `cargo test -p common --lib` (208 passed), SHIP and TM clean, and the za suite's short form (setup, `tokens::test`, `jmap_calendar::scheduling::test`, `mail::index::test`) passed (453 s wall). The follow-up round, on `294ebba20`: TM, the three format checks, the `http` unit tests (7 passed), SHIP and TM clean, and a short form of setup and the tokens module passed (333 s wall). No run failed.

The line numbers in this table are those of the tree each check ran on.

| Id | Change made | Case that failed | First panic |
|---|---|---|---|
| c5-k1 (`90d6ad286`) | the deadline no longer lowered (`.min(deadline).max(entry.expires)`) | `tokens::keyed_entry_ends_before_signed_expiry` | "a cache entry outlived its token's signed expiry", left 200, right 401 (`tokens.rs:626`) |
| c5x-k1 (`294ebba20`) | the same | the same, after its new "entry cached" assertion passed | the same message (`tokens.rs:634`) |
| c5x-k2 (`294ebba20`) | a decode error returns the entry unlowered | `tokens::expired_second_decode_publishes_nothing` | "an entry was published after the signed expiry" (`tokens.rs:695`) |

### Wave C, the calendar part

| Commit | What it changed |
|---|---|
| `11a681bbd` | `delete_with_events_za` returns `ZaCalendarBuild` with the count of listed children it wrote nothing for (no archive, or no name under the calendar); in the fenced branch a count above zero answers `Busy` ("try again") before the size check and any cancellation. The hook `CALENDAR_DELETE_BUILT_PAUSE` and the case `child_moved_out_and_back_refused`. |
| `b4d640c3c` | The keyed `COPY`'s condition becomes `source != target \|\| !is_move`, so a `COPY` inside one calendar answers 403 `no-uid-conflict` with or without an overwrite; and a key account's collection delete removes every name an event has under the calendar (`all_names`), deleting the event with its cancellation when no other calendar holds it. The cases `same_calendar_copy_refused` and `delete_removes_every_name`. |
| `85c23c154` | `za_calendar_members` takes the members from the snapshot's stored records for the build and both comparisons, on every store; `za_delete_calendar` loses its `children` parameter. The case `delete_reaches_a_shadowed_document`. |
| `45fa3a034` | `za_exceeds_one_chunk` takes a reserve, the counted size of the fence's operation; the unit test `size_check_reserves_the_fence_operation`. |
| `8e9baf7b7` | The owner's `onDestroyRemoveEvents` check reads `za_calendar_members` from the snapshot handed to the delete (RC-1); the case `remove_events_flag_sees_a_hidden_document`; `HookParked::relay` and `RelayedPause` for the moved-child case's two pauses (RC-11). |
| `cef1884a5` | The first comparison runs before the unwritten-child refusal, so a retry finds a fresh snapshot (RC-2); an ordinary account's delete counts nothing (RC-8); the `COPY` comment; the pipeline's stale-list case renamed `calendar_delete_with_a_stale_snapshot` (RC-12). |
| `2f00c2abf` | `ZaChildFence::operation_class` is the one definition of the fence's operation, and `reserve` its counted size (RC-6); the unit test pins both sides of the boundary (RC-5). |

Differences from the designs: the unit test of the reserve sits in `batch.rs`, beside the private cap it needs; `za_delete_calendar`'s `children` parameter was dropped, so no caller's list can look like the source; and the moved-child case first drove its two pauses by hand, then through `HookParked::relay`.

Its runs: TM per commit; `cargo test -p store --lib --features rocks,sqlite,test_mode -- size_check_reserves_the_fence_operation one_transaction_batch_never_splits za_change_id_fence` (5 passed); the format checks; SHIP; `cargo test -p vault -p groupware --features vault/test_mode` (267 passed); the workspace unit command filtered to the delete's unit tests and mapper tables (8 passed, no dav-proto fixture rewritten); the whole za suite (passed on its first run, 1726.21 s in the test); plain and key-mode `webdav_tests` and key-mode `jmap_tests` (passed on the first run); all on `45fa3a034`. The fix round: TM per commit, the store unit tests (5 passed), SHIP, the format checks, vault and groupware (267 passed), and the za suite's short form with the JMAP calendar modules kept, on `2f00c2abf` (passed, 1677 s); the whole suite and the protocol suites were not run in it, as its brief said. No run failed.

The line numbers in this table are those of the tree each check ran on.

| Id | Change made | Case that failed | First panic |
|---|---|---|---|
| c-kC1 (`45fa3a034`) | the unwritten-child refusal removed | `calendars::child_moved_out_and_back_refused` | `destroyed` left `["n", "o"]`, right `["n"]`: A was destroyed with E still naming it (`calendars.rs:1125`) |
| c-kC2a | the `COPY`'s condition restored to skip a `COPY` with an overwrite | `calendars::same_calendar_copy_refused` | "Expected 403 Forbidden but got 204 No Content" (`tests/src/utils/webdav.rs:599`) |
| c-kC2b | the first-name removal restored for a key account | `calendars::delete_removes_every_name` | `uid_copies` left 1, right 0: a name left under the deleted calendar (`calendars.rs:1239`) |
| c-kC3 | the members taken from the path index again | `calendars::delete_reaches_a_shadowed_document` | left 1, right 0 for `jmapcal-cal-shadow-2`: the second document left (`calendars.rs:1327`) |
| c-kC4 | the reserve ignored; store unit test | `size_check_reserves_the_fence_operation` | "assertion failed: at_cap.za_exceeds_one_chunk(reserve)" (`batch.rs:1362`) |
| d-kD1 (`2f00c2abf`) | the owner's flag check back on the path index | `calendars::remove_events_flag_sees_a_hidden_document` | "Missing not destroyed item": the destroy without the flag answered `destroyed` |
| d-kD2 | the unwritten-child refusal removed, after the reordering | `calendars::child_moved_out_and_back_refused` | the same as c-kC1 (`calendars.rs:1123`): this check is the one that refuses there |
| d-kD3 | the reserve halved; store unit test | `size_check_reserves_the_fence_operation` | "assertion failed: over_by_one.za_exceeds_one_chunk(reserve)" (`batch.rs:1373`) |

### The reviews of wave C and their rulings

**The cache part's review** (read-only, at `90d6ad286`): no Critical, no Important, thirteen Minor (RC5-1 to RC5-13). Taken into the follow-up round: RC5-7, RC5-13, RC5-9, RC5-11, RC5-1, RC5-4, RC5-5 and RC5-12. Deferred: RC5-2 and RC5-3 (the liveness filter and the one-second subtraction are decided by no case), RC5-8 (the cost: a keyed request in a token's last seconds is verified in full and charged to its address's anonymous rate limit). RC5-6 (a forward step of the wall clock after publication outruns the bound; upstream's cache design) is recorded as a residual ("Accepted residuals"), and RC5-10 (the records) is closed by this record. **Its re-review** (at `294ebba20`): every finding addressed, no new Critical or Important breakage; four Minor, all deferred: N1 (the new case can lose its power over the decode-error arm on a very loaded host, silently), N2 (two pause slots armed without a drop guard), N3 (a silent dependency: the cache clock's now is taken from upstream's `HttpAuthEntries::expires_in(0)`; if upstream changed it the lowering would fail closed and `keyed_entry_ends_before_signed_expiry` would show it) and N4 (two comments cite review tags).

**The calendar part's review** (read-only, at `45fa3a034`): no Critical; one Important at the low end, RC-1 (a destroy without `onDestroyRemoveEvents` was decided from the path index, so when a calendar's only events were ones the index hides, the delete, now listing from the records, removed an event the client asked to keep; before the wave the same request left it naming the deleted calendar); eleven Minor. Fixed in the fix round: RC-1, RC-2, RC-5 and RC-6, RC-8, RC-3's comment, RC-11 (the pauses) and RC-12. RC-7 (the documents and one code comment did not follow the code) is closed by this record, except the code comment describing the unfenced form in `crates/common/src/za/test_hooks.rs`, which does not mention that the unwritten-child refusal is absent there (deferred). **Its re-review** (at `2f00c2abf`): all addressed but RC-11's wait, which is test-only and was handed to the coverage wave (`3a7d5b474` gives the parked call a 60-second client, so the relay's 30-second wait is the binding limit), and no product-code breakage. Its new items N-1 (a slow run of the moved-child case takes 30 seconds to fail and names the pause, when the cause is the client's five-second timeout) and N-2 (`RelayedPause` without `#[must_use]`), and its observation of a test comment's overclaim (`same_calendar_copy_refused`'s), are answered by `3a7d5b474`; its note that D4 has no named run (no command of the round deleted an ordinary account's calendar) has since been covered, since the coverage net ran plain `webdav_tests` and the whole za suite on the later head; its observations of the module-wide five-second client and an older panic text are deferred. Deferred: RC-3 (with a name the path index hides, a keyed `COPY` or `MOVE` into that calendar is not seen by the UID check, so two names in one calendar stay reachable through the inherited name race, and a single-name delete then removes the first name under the calendar), RC-4 (no case runs the pipeline near the size cap), RC-9 (a name whose calendar no longer exists counts as a name that keeps the event) and RC-10 (release 4's keyed collection `COPY` with an overwrite still lists from the path index and removes the first name; outside this pull request's diff).

### The coverage gate and wave D

The manual run with the gates on `d6929c89e`, the first gates run of release 6, failed the coverage gate: cohort full 30875/32130 (96.09%) against the baseline's 17821/18490 (96.38%), and cohort runtime 18086/19187 (94.26%) against 10646/11246 (94.66%), about 93 and 78 covered lines short. The coverage net (19 steps), the listing capture and the inventory gate passed. The controller ruled that the gap is closed in this pull request by a tests-only wave before the next push, measured locally with the project's net and gate script to at least 120 covered lines of margin on both figures, with no product code, no new hook and no change to the qualification files.

Wave D: `3a7d5b474` (the test-hygiene points handed over from wave C's re-review: the moved-child case's parked call gets a 60-second client, `RelayedPause` is `#[must_use]`, a test comment), then `c71aba81d`, `c7666dd8b`, `97b1fa457`, `ce01e0f3a` and `f3630cf67`: cases for the remaining answers of the two JMAP writers, the token record's server-side answers, the contacts endpoints' refusals and a failed pin read in the fan-out, and unit tests of the vault, sealing and pinning helpers. Its fix round `67797004b` and `0c73eafc4` follows the wave's review (below). Every change under `crates/` is inside a `#[cfg(test)]` module.

| Measurement (the project's net and `check_coverage.py`; the first row CI's, on Linux with all 19 steps, the others local, on macOS) | Full | Runtime |
|---|---|---|
| in CI on `d6929c89e` | 30875/32130, failing | 18086/19187, failing |
| subset of six steps on `2f00c2abf`, before the wave | 30915/32173, 94 lines short | 18112/19216, 79 lines short |
| whole net less `automation-tests` on `f3630cf67` | 31323/32373 (96.76%), 121 to spare | 18320/19216 (95.34%), 129 to spare |
| `0c73eafc4`, after the fix round (its changed steps merged with the retained profiles of the unchanged ones) | 31354/32379 (96.83%), 146 to spare | 18345/19216 (95.47%), 154 to spare |

The first row is CI's, on Linux with all 19 steps; the other three are local macOS measurements without the Docker-backed `automation-tests` step, and CI's gates job, which decides, has not run on their commits. The wave's runs: the net subsets and the whole net passed every step, the SMTP step's first attempt failing upstream's `dkim2_dsn_is_signed` (a documented timing flake) and passing on the net's rerun by name; its iterations of the za suite under a temporary, uncommitted gate failed several times while cases were written (a client timeout under load, the case's own assertions, a step that could not be built and was dropped) before passing. The fix round's first measurement run of the za suite under the temporary gate failed both attempts at the disk-quota case (`events.rs:2859`), whose new assertion followed the review's reading that the answer carries no description: it carries "Account quota exceeded."; the second run, with the case asserting that text, passed. Its net on `0c73eafc4` ran the unit tests (914 passed, 2 ignored) and the whole za suite (passed).

What the wave could not cover without product code or a new hook, from its report's lists, as its fix round corrected them: a grantee with write-own or restricted rights (the account page grants only `view` and `edit`), imported blob values (every blob id is refused), store errors and rows gone between the cache and the read, commit failures of `Calendar/set` create and update, a failing `export_icalendar`, seal errors, the token record's assertion races and in-memory delete race, the remaining races and empty contexts of the pipeline, lookup and unwrap failures in the mail and DAV paths, the fan-out's read failures, a split error and a lost retirement, and the dead or unreachable lines (`unreachable!`; of the test-hook module's uncovered lines most are mutex-poison closures, and the rest are named in the report: closures that run only with another account's entries, a setter with no caller, two "already armed" panics and the search benchmark's admission; a random refusal id equal to the old one).

**The wave's review** (read-only, at `f3630cf67`): no Critical; two Important, both test weaknesses (RD2-1, a case that could not tell the hidden refusal of a confidential UID from the named one; RD2-2, a case asserting nothing its lines decide); twenty Minor (assertions weaker than their comments, comparisons that would print the stored token record on failure, four entries of the wave's "unreachable" list that a test can reach, inaccuracies in the report). The controller's ruling: a fix round takes RD2-1 to RD2-16 (RD2-2's case removed; the three privacy lines it was added for stay covered incidentally, with nothing asserted on them); RD2-18 is left untried; RD2-21 (the commits' co-author line names the model that wrote them) stays; RD2-22 (the fixture record) is this record's. The fix round's report also answers RD2-17 (the uncovered lines of the test-hook module, counted), RD2-19 (its own contradiction, corrected) and RD2-20 (the final measurement, stated with the tree's state). Two of the review's readings did not match the server, and the cases say what it does: a second issue for a grant appends an access entry and both access tokens open; the disk quota's answer carries the text "Account quota exceeded.". **The fix round's re-review** (read-only, at `0c73eafc4`): every finding addressed, RD2-2 by removing the case, RD2-8 and RD2-9 to what the server does; no new Critical or Important breakage; no product code in the round. Its six Minor items and three out-of-scope observations are deferred ("Deferred findings"). Two bear on this record. The merged coverage figure reuses the filesystem-blob step's profile from `f3630cf67`, a run that still held the removed case, so the margins of 146 and 154 may be overstated by up to about eight lines each (the lines RD2-2's case was added for, which no remaining case reaches); they stay above 120 either way. And the token record's issue function, called a second time for a grant that already holds entries, would return a refresh secret no entry holds and, after a rotation, add a generation-0 entry below the stored generation, which invariant 67 says no write adds; no product path calls it twice, since its one caller issues for the grant it just created. A guard was proposed to the owner as a follow-up ("Decisions and remaining qualification", item 24) and filed on his word of 2026-11-20 as issue #95; no code changes here.

### Validation on the second review's source

Rows 1 to 16 ran with `f3630cf67` checked out on a clean tree, the head after the coverage wave's first six commits, whose product code is that of `2f00c2abf` (the round's last product change; the commits between add tests only). Rows 17 and 18 ran on `0c73eafc4`. Every row passed.

| Row | Command | Result | Wall time | Head |
|---|---|---|---|---|
| 1 | `cargo fmt --all --check` | no output | 7 s | `f3630cf67` |
| 2 | `python3 -I scripts/tests/ci_steps.py` | exit 0 | 0 s | `f3630cf67` |
| 3 | `python3 -I scripts/tests/test_gates.py` | exit 0 | 5 s | `f3630cf67` |
| 4 | SHIP | clean | 167 s | `f3630cf67` |
| 5 | TM | clean | 87 s | `f3630cf67` |
| 6 | `python3 -B -m unittest discover -s scripts/release -p 'test_*.py' -v` | 25 tests OK | 4 s | `f3630cf67` |
| 7 | product build: `cargo build --release --locked -p stalwart --no-default-features --features rocks` (the full release profile) | clean | 1232 s | `f3630cf67` |
| 8 | `python3 scripts/release/verify_product.py` on that binary and `f3630cf67` | passed, provenance file written | 1 s | `f3630cf67` |
| 9 | `python3 -B -m unittest discover -s tests/product -p test_fresh_store.py -v` | 9 tests OK | 6 s | `f3630cf67` |
| 10 | FRESH: `python3 -B tests/product/fresh_store.py` on that binary | "fresh store: PASSED: 9 readback checks after the restart" | 9 s | `f3630cf67` |
| 11 | `cargo test -p vault -p groupware --features vault/test_mode -- --nocapture` | vault 213 passed, groupware 64 passed | 172 s | `f3630cf67` |
| 12 | U-ws (the workspace unit command with `UNIT_TEST_FEATURES` read from `test.yml`) | 912 passed, 2 ignored, 0 failed | 321 s | `f3630cf67` |
| 13 | `cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise` | 8 passed | 82 s | `f3630cf67` |
| 14 | `cargo test -p smtp --lib --bins --features test_mode` | 16 passed | 48 s | `f3630cf67` |
| 15 | `cargo test -p tests za::leak::tests::try_archive_short_values_are_not_archives -- --exact --nocapture` | 1 passed | 155 s | `f3630cf67` |
| 16 | SWAP: `cargo test -p tests za::cache_swap::cache_swap_tests -- --exact --nocapture` | 1 passed | 24 s | `f3630cf67` |
| 17 | `cargo fmt --all --check` | no output | 6 s | `0c73eafc4` |
| 18 | TM | clean | 215 s | `0c73eafc4` |

Rows 15 and 16 ran with `STORE=RocksDb RUST_MIN_STACK=16777216`. The product binary's `--version`: `Stalwart Server fork v0.4.0-beta.1 (upstream v1.0.3 8b32686532d34623e9ce8e1237cf2bed9554af38; source f3630cf67301181fb2d246887bc6e1d104ec8734; clean)`; the verifier printed the same line and passed, and the fresh-store script passed its nine readback checks after the restart. Row 12 rewrote the same 26 tracked dav-proto fixtures as before; they were restored with `git checkout -- crates/dav-proto/resources/requests/`, and nothing else was restored. The product build was not repeated on `0c73eafc4`: the shipping binary does not compile test modules.

The integration suites did not run in the plain test build for this head. They ran on these trees under the coverage net's instrumented build (the same commands as CI's steps, under `cargo llvm-cov`): on `f3630cf67` every step of the net but `automation-tests` (both za suite modes, plain and key-mode `webdav_tests`, `imap_tests` and `jmap_tests`, the SMTP tests with the one flake above passing on the rerun by name, `system_tests`, `store_tests`, `blob_tests`, `lookup_tests`, `search_tests`, `telemetry_tests`, `cal_query` and the workspace unit tests), every step passing; on `0c73eafc4` the steps whose tests changed (the unit tests, 914 passed and 2 ignored, and the za suite, passed). The coverage figures above are a local measurement that CI's gates job has not confirmed; the inventory gate was not run locally.

Not run in this validation: `directory_tests` and `automation_tests` (Docker), the independent CalDAV client, the retention gates in CI, and the integration suites in the plain test build. Everything here is local: RocksDB, one node, debug or instrumented test builds, macOS. A passing suite is not review approval.

## The pull request's third review and the second whole-branch pass

### How it ran

On the project owner's word of 2026-11-20 the second round (`d6929c89e..68d905131`) was pushed, and, on his word, the review threads whose findings were addressed were marked resolved. The reviewer then posted a third review (reviews 5994824096 and 5994832045, 2026-11-20, on `68d905131`), a source review with no builds or tests. Its summary found all five findings of the second review addressed within the documented scope, and it raised three new inline findings, each rated P3. They were handled by the project owner's rule from the first round (an external reviewer's claims, verified before anything changes): the controller verified each by reading the named lines at `68d905131` before anything changed, and all three were confirmed. Each of the three threads has a reply with the verdict; C4633314515, left open for the scope of its fix, was resolved after the reviewer's summary called all five addressed.

On the same day, on the owner's word, a second whole-branch review pass ran (the plan requires his word for a second pass): one reviewer with a clean context, read-only, given the spec, `CLAUDE.md`'s invariants and the code, and told not to read this note, the plan's files, the review files or the run directory. By the controller's ruling the three findings and what the fresh pass found were fixed in one wave after that pass reported, so the tree did not move under its reviewer: wave E. The owner made two decisions on 2026-11-20 (below); everything else in this round is the controller's ruling, reported to him.

### The three findings

| Id | Finding (short) | Verdict | Whose it is | Outcome |
|---|---|---|---|---|
| C4644031673 [P3] | The owner's `Calendar/set` destroy with `onDestroyRemoveEvents` makes the membership pass and discards it | confirmed: at `68d905131` the pass over the snapshot's records (collect, sort, dedup) ran before the flag was tested; in memory, no store read | the second review's fix round (`8e9baf7b7`, RC-1) | fixed: the flag is tested first, and without it a check that stops at the first record naming the calendar; no behaviour change: `a65717ae9` |
| C4644031680 [P3] | A refresh with a wrong secret at the grant's current generation accepted `Lost` or `Superseded` | confirmed: the case presents the stored generation, so the server answers `Lost`, deterministically; the HTTP caller deletes the grant on `Lost` and keeps it on `Superseded` | the coverage wave's case | fixed: the case asserts `Lost`, with a kill check: `6c5c46b26` |
| C4644038016 [P3] | The cross-account hand-off case could pass on a generation mismatch alone | confirmed: `za_handoff_put` refuses an account or a generation mismatch with one error, and the case left the generation uncontrolled | the coverage wave's case | fixed: the foreign keys carry the target vault's revision, the ids are asserted different and the generations equal, with a kill check: `6c5c46b26` |

### The second whole-branch pass

The reviewer was given the spec, `CLAUDE.md`'s invariants and the code at `68d905131` against the base `ade47a9ae`, and not this note, the plan's files, the earlier reviews or their rulings. Nothing was built or run. Its result: no Critical finding, one Important and six Minor. The controller mapped each to the record (searches of this note, the spec and the ledger) and read FR-3, FR-6 and FR-7 at the code.

| Finding | Already in the record | Ruling or decision |
|---|---|---|
| FR-1 (Important): a grantee can skip D15's boundary check by presenting a `z` state with an origin of its choosing; the answer holds item ids only | yes: the ruling at Task 22 (D15 is a sync-correctness rule, not an access control), the final review's FC2-5, item 2 of the list for the owner | decided by the project owner on 2026-11-20: D15 stays a sync-correctness rule and is not an access control; no code change. The final review's lens had rated it Minor (FC2-5); this reviewer rated it Important |
| FR-2 (Minor): a grantee's calendar `/changes` reports in `destroyed` the ids of the owner's events outside its shared calendars, while CalDAV's sync limits vanished entries to eligible calendars | the principle only (spec 6.8: upstream's filtering for a grantee); the consequence was stated nowhere | decided by the project owner on 2026-11-20: inherited behaviour, accepted, no code change. One line in spec 6.8 and a pointer in 3.4 (the controller's ruling); filtering for key calendars was named to him as later design work and is not planned |
| FR-3 (Minor): past four access entries, the entry a write adds could be the one removed (a lowered access-token lifetime, or node clocks that differ) | no | fixed in wave E (`20f4cf9d2`); spec 4.3 states the rule |
| FR-4 (Minor): the token sweep is skipped when an earlier purge step fails | yes: FA-14, an accepted residual | no change |
| FR-5 (Minor): a well-formed suffix on a keyless grant's refresh token rotates the grant, then deletes it | yes: R18-7, item 7 of the list for the owner, a spec question on 4.5 | no change; the reviewer's note that the precheck could refuse before the rotation is added to item 7 |
| FR-6 (Minor, suspected): `onSuccessSetIsDefault` writes the default id with no assertion, so a delete of that calendar between the call's snapshot and the write leaves a default naming a deleted calendar | no | the mechanism confirmed by reading; deferred, as a comment on issue #92 (2026-11-20); the consequence beyond "no calendar reports `isDefault`" is not traced |
| FR-7 (Minor, suspected): every JMAP update of a key account's event, alert-only and keyword-only ones included, stores the converter's output, where upstream keeps the stored tree for a per-user-only change | partly: spec 3.4's round-trip limitation | the mechanism confirmed by reading; one sentence in spec 3.4; no code change (D10 puts the user data in the tree) |

What it covered, from its own list: the token, session and cache code in depth (the token record, the Bearer arm, the code exchange and refresh, the grant's deletion, the key cache's fences); the pipeline's modules read whole; the JMAP key writers, readers, admission and changes code read whole, with the state codec; the CalDAV call sites' diffs; three of the token cases; and every touched upstream path for ordinary accounts against `CLAUDE.md`'s exceptions and costs, where it found nothing outside them. What it did not cover: the second half of `copy_move.rs`' diff; the test hooks and the unit tests; the unchanged code the branch calls (sharing resolution, the fan-out, delegation, the drain, the converter, upstream's change paging); most of the test modules, so whether FR-1 to FR-7 are exercised; spec sections 7, 9 and 11 to 13; cluster behaviour; and the SQL and FoundationDB back ends beyond their write order. These are limits of the pass, not findings.

### Wave E

| Commit | What it changed |
|---|---|
| `a65717ae9` | The owner's `onDestroyRemoveEvents` check tests the flag first; without it `za_calendar_has_member` (`crates/groupware/src/calendar/pipeline/delete.rs:493`) answers whether any stored record names the calendar, stopping at the first. It and `za_calendar_members` go through one predicate, `za_record_names_calendar` (`:502`). The refusals keep their order and texts. No unit test: `delete.rs` has no test module, and the suite's destroy cases decide the owner's branches. |
| `20f4cf9d2` | `put_access` (`crates/vault/src/tokens.rs`) removes the earliest-expiring stored entry, the first on a tie, before it adds the new one, so the entry it adds never competes. The unit test `put_access_keeps_the_entry_it_adds`: a fifth entry that expires below all four, one that expires later, and a tie. |
| `6c5c46b26` | `tokens::server_token_answers`: a wrong secret at the grant's stored generation must answer `Lost`; the cross-account hand-off case builds the foreign keys at the target vault's revision and asserts that the account ids differ and the generations match. |

Its runs: the new unit test, written first, failed at its first assertion, "the access entry just added does not open" (`crates/vault/src/tokens.rs:651` on that tree), and passed after the fix; `cargo test -p vault -p groupware --features vault/test_mode` passed (vault 65, groupware 213) after a first attempt that did not compile (a missing `&` in `a65717ae9`'s code, fixed before the commit), and again after rustfmt reflowed the new test; `cargo check -p tests --tests` without a warning; the format checks of `vault`, `groupware`, `jmap` and `tests`; and the whole za suite on `6c5c46b26`, clean tree, passed on its first run (1698.38 s in the test).

The line numbers in this table are those of the tree each check ran on. Both ran on a short form of the suite: its setup lines, `setup::test` and `tokens::test` (the tokens module's first case needs an account `setup::test` creates; a first attempt without it failed before any case under test).

| Id | Change made | Case that failed | First panic |
|---|---|---|---|
| e-k1 | `classify_refresh` answers `Superseded` where the presented entry does not open (`crates/vault/src/tokens.rs:275`) | `tokens::server_token_answers` | "a refresh with a secret no entry holds at the grant's generation was not lost" (`tokens.rs:3111`) |
| e-k2 | `za_handoff_put` without its account-id condition (`crates/common/src/za/tokens.rs:234-236`) | `tokens::server_token_answers` | "a hand-off was put with another account's keys" (`tokens.rs:3040`) |

While the second kill check's mutation was in the tree (the uncommitted removal of the account-id condition, as the brief asked), the development harness flagged the implementer. The controller checked that no commit carries the mutation and that the file was restored. The implementer's two concerns: `CLAUDE.md` still named `za_calendar_members` for the owner's check (corrected by this record); and the kill checks' short form kept `setup::test`.

### The review of wave E and its rulings

**Wave E's review** (read-only, at `6c5c46b26`): the wave does what its brief asks, quality approved; no Critical or Important finding, four Minor. No fix round followed.
- RE5-1 (`CLAUDE.md`'s line references into `delete.rs` low by thirteen lines after `a65717ae9`, and the function the owner's check calls changed): corrected by this record.
- RE5-2 (the plan's interface sheet and task file state the earlier `put_access` rule): the plan's files are the plan as it was executed and are not amended after its end; spec 4.3 and this note carry the rule as it now is.
- RE5-3 (the owner's destroy still runs the loop over the calendar's children from the path index, which its path does not read; it mirrors upstream's handler body and is in memory): deferred.
- RE5-4 (the report attributes the red run's three-line shift to rustfmt; the shift is the three lines the fix added to `put_access`'s doc comment): recorded; the evidence is consistent.
- What the review could not verify from the diff (the logs, the tree state of the check and format rows, the shipping configuration) the controller settled from the wave's logs and the validation below.

### CI and the gates on `68d905131`

- The pull request's run on `68d905131`: every job passed (Check Style, Unit Tests, Test, Upstream Suites, Zero-access Test, Build Product, Fresh OSS Store Restart, Independent CalDAV Client); the gates job does not run on a pull-request run.
- The manually dispatched run on `68d905131`, with the retention gates: Check Style, Unit Tests, Test, Upstream Suites, Build Product, Fresh OSS Store Restart and Independent CalDAV Client passed. Its Zero-access Test job failed, in the step "JMAP Tests (key accounts)", which has no rerun in CI: `tests/src/jmap/za_variants.rs:1591:13`, the thread-merge variant, "test# 1/5", thread sizes `[1, 3, 6, 212]` where `[1, 3, 218]` is expected. Client 5 of that case is the group `sales@example.com`, which stays an ordinary account in key mode, so its threads are merged by upstream's background task. Every other step of the job passed (both modes of the zero-access suite, the cache-swap suite, both CalDAV modes, key-mode IMAP). The same step passed in the pull request's run on the same commit and locally on `6c5c46b26` (the validation's row 9). The controller classified the failure as a timing failure of upstream's ordinary-account thread merge ("Rulings").
- That run's gates job had not finished when this record was written (still in its coverage net after more than two hours): neither gate has a result on `68d905131`.
- No CI run exists for any commit after `68d905131`.

### Validation on the final source

Nine rows ran on `6c5c46b26`, a clean tree. Every row passed on its first attempt.

| Row | Command | Result | Wall time |
|---|---|---|---|
| 1 | `cargo fmt --all --check` | no output | 6 s |
| 2 | `python3 -I scripts/tests/ci_steps.py` | exit 0, no difference | 0 s |
| 3 | `python3 -I scripts/tests/test_gates.py` | 45 tests OK (2 skipped) | 4 s |
| 4 | SHIP (`cargo check -p stalwart --no-default-features --features rocks`) | clean | 115 s |
| 5 | TM | clean | 90 s |
| 6 | `cargo test -p vault -p groupware --features vault/test_mode -- --nocapture` | 213 and 65 passed | 110 s |
| 7 | U-ws (the workspace unit command with CI's features written out) | 913 passed, 2 ignored, 0 failed | 396 s |
| 8 | `cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise` | 8 passed | 72 s |
| 9 | key-mode `jmap_tests` (`ZA_KEY_ACCOUNTS=1`, `STORE=RocksDb RUST_MIN_STACK=16777216`, `BLOB_STORE` unset) | 1 passed, no rerun (201.26 s in the test) | 321 s |

Row 9 started at a one-minute load of about 35. Row 7 rewrote the tracked dav-proto fixtures as before; they were restored with `git checkout -- crates/dav-proto/resources/requests/`. Not run in this validation: the product build with its verifier, the fresh-store script, the zero-access suite (run once on this commit by wave E, above), its filesystem-blob mode, the cache-swap suite, `webdav_tests`, `imap_tests`, plain-mode `jmap_tests`, the SMTP tests, the store suites and `system_tests`. Everything here is local: RocksDB, one node, debug test builds, macOS. A passing suite is not review approval.

## Other harnesses

Task 41's records, all first runs, on `0237a0b50` (Task 41's base, which holds Task 39's fix `8cf35e32c`) plus Task 41's two edits:

| Label | Result | Time |
|---|---|---|
| WK | `test webdav::webdav_tests ... ok` | 24.91 s |
| IK | `test imap::imap_tests ... ok` | 13.65 s |
| JK | `test jmap::jmap_tests ... ok` | 183.49 s |
| PRODUCT (thin LTO, 16 codegen units) | exit 0, no warning; version line names source `0237a0b50` and `dirty` (the task's two edits were uncommitted at build time) | 5 m 27 s |
| the fresh-store unit suite | 9 tests OK | 7 s |
| FRESH | `PASSED: 9 readback checks after the restart` | 10 s |
| CALDAV, key and plain | not run: `python3.12` is not on the host; nothing was installed; left to CI's `caldav-client` job | n/a |

`git diff origin/main...HEAD -- tests/src/webdav` is empty, so WK runs upstream's CalDAV suite unchanged on the head.

**The rerun decision.** No rerun was added to the key-mode JMAP step: `calendar::alarm` failed in none of five key-mode runs (Task 39's three counted runs and its fix round's, and Task 41's). The decision rests on local runs only; key-mode JMAP passed again in the second pass of Task 44's local matrix (row 14b), and the first hosted-runner evidence comes with CI on the branch, which this plan did not run. The step's description in `test.yml` changed (a comment only), and `ci_steps.py` is unchanged; `python3 -I scripts/tests/ci_steps.py` and `python3 -I scripts/tests/test_gates.py` passed.

**The cohort additions.** `tests/qualification/dispositions.json` gained 23 entries, one per fork runtime file release 6 added under `crates/` (the pipeline's ten files, `za_alerts.rs`, `za_text.rs`, `vault/src/tokens.rs`, `common/src/za/tokens.rs`, `common/src/za/calendar.rs`, `za_bearer.rs`, `za_token.rs`, `api/za_calendar.rs`, `za_read.rs`, `query/za.rs`, the two key writers and `changes/za_calendar.rs`), each with `runtime_excluded: false`. Why: `crates/dav/src/common/za.rs` is a file of the baseline cohort and the move takes its lines into `pipeline/admission.rs` and `pipeline/context.rs`; left out, those lines and the rest of the release's fork code would leave the measured cohort. The cohort grows from 53 to 76 files; the coverage fraction stays unmeasured until the gates run (Task 44 did not dispatch them; the owner's word is needed), and a shortfall there is answered with tests. Two evidence strings in that file were wrong (R41-2, R41-3); the final review's fix wave corrected them.

## Adjusted assertions

Every existing assertion a task changed, each with a comment in the code citing the spec section. Lines are at `de67af5df`.

- **`tests/src/za/gating.rs`** (Tasks 21 to 26, 28, 30, 35). `:233-249`: a key account's session advertises `urn:ietf:params:jmap:calendars` and `:calendars:parse`, primary for the account (spec 6.10, D13). `:312-323` and `:344-362`: the key principal's calendar capability block, `mayGetAvailability` false, `accountId` null for a caller without access and the account's own for itself (spec 6.10). `:382-416`: the loop of eighteen refused rows now holds the six still refused (`CalendarEvent/copy`, `CalendarEventNotification/*`, `accountNotSupportedByMethod`); `:418-471`: the served rows, each asserting no `accountNotSupportedByMethod` or `forbidden` (spec 6.2; `ParticipantIdentity` per 6.9). Rows moved between the loops task by task as the plan's "Moving a refusal row" says.
- **`tests/src/za/websocket.rs:75-85`** (Task 21): the same socket's calendar call answers `forbidden` with `ZA_CALENDAR_KEYLESS` instead of `accountNotSupportedByMethod` (spec 6.1).
- **`tests/src/jmap/za_variants.rs`, `calendars`** (`:3530`): the served block and refusal loop that Tasks 21 to 30 kept there were replaced by Task 39 with the calendar block in upstream's order (below). **`principal_get`** (`:6002`, Task 35): the session's `primaryAccounts` gain the two calendar capabilities (`:6273`), the account object offers the calendar capability (`:6113`), and each principal shows its calendar block with `mayGetAvailability` false (`:6349`) (spec 6.10, D13).
- **`tests/src/za/sharing.rs:1906-1911`** (Task 27): the conversion carries the owner's colour in its sealed personal bundle, where release 4 asserted none (spec 6.7, D14).
- **`tests/src/za/app_password.rs:686-689`** (Task 17): a comment only: the exchange now issues a keyed grant and a token record, which the app-password revoke leaves in place (spec 9.6, D19).
- **`tests/src/za/setup.rs:785-802`** and **`tests/src/za/password.rs:158-160`** (Task 14): a key account's credential version is `za_session_binding` of the vault record, compared exactly, where it was the vault revision (spec D19); the password case's comment says the same.
- **`tests/src/utils/cleanup.rs`** and **`tests/src/utils/storage.rs`** (Task 14): the emptiness check exempts field 153 while its account exists (spec 9.6).
- **Task 39's tables** (`za_variants.rs`; upstream's line numbers in each body). Taken from the code and the corrected tables (`c3fa2099c`, `0237a0b50`), not from the report's row count:
  - `calendar_event` (`:3563`, mirror of `calendar::event::test`): `:85-90` the create with `isDraft`, `mayInviteSelf`, `mayInviteOthers`, `hideAttendees` true is refused `invalidProperties`, then created without them (6.5); `:154-160` the flags read false (6.3); `:169`, `:175-187` event 2's `useDefaultAlerts` false and its alerts the value `events::create_use_default_alerts` pins (D11); `:237-240`, `:250` the same (6.3, D11); `:280-313`, `:475-506` the override patch `alerts/abc/iCalendar` in windowed and property-subset reads (D11, D1; ruling F1); `:350-370` the duplicate UID created in calendar 1 and refused in calendar 2 (3.3); `:467` `useDefaultAlerts` false after the patch (D11).
  - `calendar_identity` (`:4798`): the destroy of `b`, the two foreign-address creates and the create with `onSuccessSetIsDefault: "#i0"` answer `forbidden`; the list keeps both identities; `onSuccessSetIsDefault: "b"` switches the default (6.9).
  - `calendar_notification` (`:4953`): each `CalendarEventNotification` method answers `accountNotSupportedByMethod` (6.11).
  - `calendar_acl` (`:5005`): `:146-161` John's `shareWith` refused `forbidden`, the grant made at `view` through the share endpoints (6.11); `:232-289`, `:399-455`, `:652-707` the share notifications with the rights of the level granted or revoked (6.2; ruling F2); `:294-301` Jane's empty patch at view `forbidden` (6.7; ruling F3); `:302-313` Jane's `shareWith: {}` `forbidden` (6.11); an added row: Jane's rename at view `forbidden` (6.7); `:357-369` the grant at `edit`; `:372-394` Jane's `myRights` at `edit` (no `mayDelete`, `mayShare`, `mayReadFreeBusy`, `maySetMetadata`) (6.7); `:474-525` `CalendarEvent/copy` `accountNotSupportedByMethod` (6.11); `:606-617` the revocation through the endpoint; `:710-736` the grant at `edit` and Jane's destroy refused "You are not allowed to delete this calendar." (6.7); `:738-744` the cleanup without `destroy_all_event_notifications` (6.11). Upstream's assertions at `:162-190`, Jane's reads of John's calendar, `:314-325` and `:330-353` stand unchanged (R39-1, R39-2).
  - `calendar_availability` (`:5792`): `:94-109` the `shareWith` grant refused `forbidden` (6.11); the two `Principal/getAvailability` assertions at `:127-177` and `:235-243` expect `[]` (6.2).

## Accepted residuals

- **Spec 3.3, in substance.** Each object of a `/set` is its own commit (D3); an unreachable or withheld recipient records its status and does not fail the object; an unexpanded text query is matched in memory; UID uniqueness is CalDAV's rule; destroying a calendar sends cancellations (D16); no drafts, per-user event data or participant-visibility flags; default alerts by value (D11).
- **Spec 3.4's limitations**: a CalDAV colour and JMAP's `color` stay apart; an event written with `sendSchedulingMessages: false` and edited through CalDAV is scheduled from that edit on; upstream's conversion decides what survives a round trip, since the third round stated to cover a key account's alert-only and keyword-only JMAP updates too (FR-7); JMAP does not check a calendar's supported component set. Only the round trip has a test (`oracle::round_trip_keeps_what_an_ordinary_account_keeps`).
- **4.5's interrupted refresh.** If the grant advances and the record's write does not follow, the superseded secret and the stored record still open MK until the next write of the record, the client's retry or the sweep, at most one maintenance interval. Upstream refuses the superseded token itself at once.
- **D22's residual.** A keyed grant whose entries are gone while its own deletion failed (D20's eviction, 4.4's first write) refreshes as a keyless grant when its client presents the token without the secret. It opens nothing.
- **D7 at a cache hit.** D7 is decided at a cache miss: a node that has a token's keys cached serves it until that cache entry ends, even if the record's entry was dropped by the cap of four. A grant's deletion evicts the cached entry at once.
- **Section 2.** The token record is new at rest, and an entry can outlive its token until the next maintenance run (an expired token, a grant destroyed through the registry, an interrupted refresh). The hand-off stores MK wrapped under a secret inside the code for the code's lifetime. A browser now holds a credential that opens the account's keys for as long as its grant lives (upstream's default: thirty days without use). Whoever serves a web client's code can read what the user reads.
- **The three ordinary-account I/O costs** (spec 10): one cached account read at a code exchange, at a refresh and at a grant's deletion; one in-memory delete at a consent confirmation that carries no secret; cached classification reads in each calendar method, which spec 10 counts as one, while the code makes two for `Calendar/set` and `CalendarEvent/set` (admission, then the handler's head; R28-8, R30-3) and more than one for a calendar `/changes` request (R22-1). **D29's costs**: state-codec fields and context checks, with no global account lookup and no additional ordinary-account store I/O; key-calendar pages carry the origin fields and classify the account when choosing the continuation format. **The legacy grantee restart** (6.8): a grantee's legacy `r` continuation of a key calendar answers `cannotCalculateChanges`, and the client restarts from Initial.
- **4.3, a retried refresh.** An entry's expiry is computed when it is written, so on a retried refresh the next generation's entry can outlive its token by the retry's delay; the token's own expiry and the sweep bound it.
- **5.2.** Upstream stamps `updated` on an owner's update, so most JMAP moves ask `ModifyItems` as well; the two share levels cannot tell the difference.
- **Coverage that stays partial.** The refresh steps of 4.5 have a unit test for the classification only (`classify_refresh_cases`) and run otherwise in the za suite, where no case reached the `Lost` or `Superseded` arm of the classifier (R18-4, N2); since the coverage wave `tokens::server_token_answers` reaches its `Lost` arm through the server function, and since wave E it asserts `Lost` (kill check e-k1). The pass-through of a foreign Bearer value (D26) has a unit test only.
- **R27-1, rollback.** Every binary built before release 6 refuses a coloured personal bundle (the opener at the base `ade47a9ae`, release 7 included), so a binary rollback below release 6, or a cluster of mixed versions, leaves a coloured shared calendar unreadable to the older binary.
- **X2.** The override branch of upstream's `increment_sequence` is covered by no test in this fork's net, for any account.
- **The final review's residuals** (by id; the review files hold each in full):
  - FA-11: an ordinary account's consent token followed by a 43-character suffix of the secret alphabet is accepted, the suffix stripped (also an every-account difference in `CLAUDE.md`).
  - FA-14: the token sweep runs only when the purge steps before it in the maintenance task succeed.
  - FA-20: a node that missed the invalidation broadcast of a vault write that leaves the binding unchanged refuses every uncached Bearer request of the account (a refresh does not help; Basic is served) until another invalidation of the account reaches it. Cluster only; unqualified here.
  - FB-2: a create without a `uid` whose commit fails after its invitations went out is retried by the client under a new UID, so the attendees hold an event of a UID the organizer never stored (a line in the checklist).
  - FB-7: the placement and destroy batches assert in CalDAV's order at the base.
  - FC1-6, FC1-8 (= R30-2, R31-4), FC1-12, FC1-13, FC1-14 (= R31-3), FC1-15: the event writer's answers and check order beside upstream's, as the plan prescribes; nothing is stored in the clear by any. FC1-7 (= R31-2) is no longer a residual: fixed after the pull request's review (`d86f90b31`), on the project owner's decision.
  - FC2-7 (= R21-8, R26-5) and FC2-8 (= R26-7): an unranged text query holds its opened views for the request, and the store's leaf limit is checked after the candidates are opened.
  - FC2-9 (= R21-5): one shared calendar that does not open fails a grantee's whole `Calendar/get` and its exact-origin `/changes`, where the owner gets `notFound`. **Weighs more than its rating** (lens C2): a browser client then loses the whole sharer account, not one calendar.
  - FC2-10, FC2-13 (= R22-3), FC2-14, FC2-17: `Calendar/set` reads its context before upstream's validation; a non-canonical `z` is accepted and echoed re-encoded; an owner's get that completes a sharing boundary returns the state sampled before its own write, so the next `/changes` reports that calendar as updated; an owner's own sync completes none.
  - FD-13: store errors on the issue and refresh paths keep their context in the trace.
  - FD-16: `za_handoff_take` deletes the hand-off before it checks the secret, so a wrong secret consumes it.
  - W-2: `sharing::revoked_and_unshared_calendars_hidden` cannot fail on the fork's fresh-context filter, which sits behind upstream's ACL masks in every state a test builds without a hook (an equivalent mutant at suite level); its comment overstates.
  - W-5: `traced_url` masks a `code` query parameter as a standard parser reads one; a callback URL percent-encoded inside another parameter's value, and a `;`-separated query, pass unmasked. The server generates neither.
  - FC1-10, FC2-11 (its list), FA-9, FC2-12: the key writers repeat upstream's inline checks, and several fork insertions are larger than one call or a short branch; the code stays, and `CLAUDE.md` names them as silent dependencies and hotspots.
- **The pull request's review's residuals** ("The pull request's review"):
  - RA-4: the calendar delete's fence can refuse after the cancellations went out; key attendees then hold a CANCEL for events the organizer still has, and the outside attendees' mail is dropped, until a retry, which converges. Accepted, put to the owner and confirmed by the project owner on 2026-11-19.
  - The store ruling: on FoundationDB, a read replica and the ephemeral store a key calendar's delete stays chunked and unfenced, with no unwritten-child refusal and no size refusal (it takes the members from the stored records and removes every name, as elsewhere), and a failed later chunk leaves the earlier chunks' deletions committed (release 5's recorded residual, iMIP spec 5.4, for those stores). Put to the owner and confirmed by the project owner on 2026-11-19.
  - RB-3: a keyed token whose scope excludes mail still runs the search pass through an admitted `Blob` call. Accepted as the decision reads, put to the owner and confirmed by the project owner on 2026-11-19.
  - RB-4 and RB-5: what a token's scope admits on DAV is upstream's scope model, unchanged; the drain's scope rule limits the drain, not what a token can read. The project owner's disposition of 2026-11-19: no change in this pull request.
  - C4617611630: UID privacy-conflict answers to a grantee depend on events it cannot see; inherited from upstream, no code change, decided by the project owner on 2026-11-18.
  - Fix round 1's re-review, item (2), as fix round 2 left it (and its re-review's new minor item): a paged sync from the initial state is never raised, so it ends at the range's last event change and below any sharing boundary above that change, whether the boundary completed while it paged or before it began; its next call is refused, and a restart that pages from the initial state again ends at the same state, until an event is written, the client syncs in one page, or it takes its state from `/get` or `/query`. Nothing is skipped, and a client that follows the protocol holds the state `/get` gave it and never enters this flow; for this one flow the fault the pull request's review named remains. It is the cost the controller's ruling accepted; at `53cf84aa9` the flow behaved the same.
  - The early drain of the decided rule: an account that lacks a calendar method's JMAP permission still drains on the calendar call the dispatcher then refuses, and a call whose back-reference fails has drained (spec 5.5).
- **The pull request's second review's residuals** ("The pull request's second review"):
  - Inherited: no write fences an event's name, for any account, so two events can be stored at one href; the calendar delete no longer leaves either behind, and the name fence itself is not taken in this pull request.
  - Inherited: an ordinary account's cached Bearer authentication outlives the signed expiry by the lookups after the decode; a key account's entry no longer does.
  - Inherited (upstream's cache design): a forward step of the wall clock after publication can leave an entry live past the signed expiry by up to the step (RC5-6).
  - RC-10: release 4's keyed collection `COPY` with an overwrite still lists the overwritten calendar's children from the path index and removes each event's first name; outside this pull request's diff.
  - The items of #91 to #94 stay open.
- **The third round's residual** ("The pull request's third review and the second whole-branch pass"):
  - Inherited, accepted by the project owner on 2026-11-20: a caller who is not the owner receives in `destroyed` the ids of the owner's events outside its shared calendars when those events change or are deleted; ids only.

## Deferred findings

What stays open after this plan, by task: every `minor (deferred ...)` line of the ledger, with its id where the ledger gives one. The arrow names where the ledger routed it. The review files (`task-<n>-review.md`, `task-<n>-fix-review.md`) hold each in full.

23 of these lines are no longer true at the head, 17 on the final review's triage reading (fixed by a later task, or the owed record delivered) and 6 closed by its fix wave (Task 17's length-constant line, R30-1, R32-1, R37-14, R40-8, and R41-2 with R41-3), and they are removed: Task 17's length-constant line, Task 18's N1 and O1, R21-10, R22-4, R22-8, R23-4, R23-5, R26-8, R27-1, R29-7, R30-1, R30-9, R30-11, R32-1, R32-2, R34-5, R35-3, R37-14, R37-16, R38-12, R40-8, and the line of R41-2 and R41-3; R28-4 and R30-10 are rewritten to what still holds. A line marked **weighs more than its rating** is one a lens found heavier against the whole branch than at its task.

### Task 17

- (no id) A device-code poll whose `device_code` is the pending user code now answers `expired_token` for every account instead of `authorization_pending` (`token.rs:368-370`); no token was obtainable that way. Recorded since the final review among `CLAUDE.md`'s every-account differences, and proposed to the owner for spec section 10.
- (no id) A failed hand-off cleanup on the classification error is discarded without a trace (`za_token.rs:36`), as briefed.
- (no id) `PKCE_VERIFIER` duplicated in `tests/src/za/tokens.rs` from `tests/src/utils/oauth.rs`.
- (no id, evidence) The RED run's `tokens.rs` hash differs from the first GREEN and the committed one; the RED file is not preserved.
- (no id, evidence) The device-code guard has no observed failing run; a candidate mutation row.
- (no id, reviewer's out-of-scope note) Destroying a key account leaves its outstanding code and hand-off in the in-memory store until exchange or expiry (ten minutes).

### Task 18

- R18-2: a credential change before the refresh's first read answers `Lost` (grant deleted, "token keys lost" trace), not `VaultChanged`; spec 9.2 allows either.
- R18-3: run 2 of `refresh_crossed_by_credential_change` cannot catch an omitted vault assertion (the binding change clears field 153 in the vault write's batch).
- R18-4: no suite case reaches `classify_refresh`'s `Lost` arm, presents a well-formed wrong secret or reads the `Lost` trace; three-attempt exhaustion unexercised.
- R18-5: `delete_keyed_grant`'s failure text says "unissued OAuth grant not deleted" on the refresh path, and the `Lost` trace says "deleted" before the deletion is attempted.
- R18-6 (plan-mandated): `Superseded` and `VaultChanged` refusals emit no fork trace; spec 8 says a refused token open is traced (→ a question for the spec). **Weighs more than its rating** (lens A; lens D notes it as a gap against spec 8): with FA-13, an operator has little to diagnose a failing browser session with.
- R18-7: a well-formed secret appended to a keyless grant's refresh token makes the precheck answer `Keyed`, and the `Lost` arm deletes that grant; the spec is silent.
- R18-8 (plan-mandated): an unusable token record makes each refresh delete its grant while D8's sweep leaves such a record alone (→ a mixed-version question for the spec).
- R18-9: parked refreshes hold a five-second client across Argon2 vault writes; passed under a one-minute load of 20 to 54.
- R18-10: the inherited linker warning and "tests (lib test) generated 1 warning" in the ZA and SYS logs.
- R18-11: `change_race_password` repeats `change_password`; `za_token.rs:123-126` repeats `:86-89`.
- N2: no ZA case reaches `classify_refresh`'s `Superseded` arm after the fix; the vault unit test covers the classifier.

### Task 21

- R21-3: no test protects the grantee filter in `calendar/get.rs:108-130` or the try-next-calendar arm in `za_read.rs:81` (→ Task 36 or 42; still open after Task 36, see R36-4).
- R21-4: the R21-1 regression subscribes to `DataCorruption` only and does not show that no other event carries the canary.
- R21-5 (plan-mandated): one unreadable shared calendar fails a grantee's whole `Calendar/get` where the owner gets `notFound`. **Weighs more than its rating** (lens C2, as FC2-9): a browser client loses the whole sharer account, not one calendar.
- R21-6 (plan-mandated): two 20-line inline blocks in upstream's `calendar/get.rs`; the re-trace arm nearly duplicates `za_read.rs:70-79`; import groups reflowed.
- R21-7: the handlers classify the account again apart from the boundary's decision.
- R21-8: the view memo never hits in `CalendarEvent/get` and keeps every opened view alive to the method's end.
- R21-9: the event row read follows the content read; a concurrent update can give a spurious trace and a transient `notFound`.
- R21-12 (plan-mandated): the served-methods loop in `gating.rs` passes on `serverUnavailable`.

### Task 22

- R22-1: three account classifications per calendar `/changes` request.
- R22-2: the grantee sees one calendar, so its `Calendar/changes` paging asserts nothing about continuation pages.
- R22-3: non-canonical `z` strings are accepted, so `oldState` echoes a re-encoding.
- R22-5: the `invalidArguments` text is defined twice (jmap-proto `parser.rs`, jmap `za_calendar.rs`).
- R22-6: no test reaches the in-process `z` arms at `changes/get.rs`, `changes/za.rs`, `participant_identity/changes.rs`.
- R22-7 (→ Task 43 or a documentation commit): I §7.1's call-site list and the header's `za_variants` row no longer match the code (recorded under "Decisions and remaining qualification").

### Task 23

- R23-1: `let mut` edits an upstream line in `participant_identity/set.rs:52`.
- R23-2: the moved gating rows' comment cites spec 6.9 only; the header asks for 6.2.
- R23-6: no case covers a grantee's `/set` on the owner's account reaching upstream's `reject_identity_changes`.

### Task 24

- R24-1: no test of a grantee parsing the owner's blob under its own account id.
- R24-2 (brief-mandated): the inbox emptiness check is absolute; a before/after delta would be sturdier.
- R24-3: "the method writes nothing" is not asserted.

### Task 25

- R25-1: no test covers the `Unresolvable`/`Corrupt` to `UnexpectedError` mapping.
- R25-2: every test uses English.
- R25-3: no row covers the Owner field's second source (an attendee with an owner role).
- R25-4: the unzeroed `ContentRecord.terms` and verbatim term keys are request-lifetime plaintext in process memory, allowed by invariant 60.

### Task 26

- R26-1: a 38-line inline block in upstream's `query.rs` repeats `query/za.rs`' view-opening steps.
- R26-4: nothing checks that a query without a text condition opens no event.
- R26-5 (→ the final review): an unexpanded text query keeps every opened event's full plaintext view for the request.
- R26-6: two sequential store reads per candidate; a networked store could pass the client's five seconds on a large calendar.
- R26-7: a query over the store's text-leaf limit is refused only after every candidate is opened.
- N1: expanded parity compares starts only, and two fixtures share a start.
- N2: the expanded loop never asserts a non-empty answer.
- N3: the expanded form asks the flat ANDs only as one multi-property condition.
- O1: the rebuilt-count check does not attribute events to methods; the canary assertion's sensitivity on that path is unexercised.
- O2: the negated expanded form is checked for one negation only.

### Task 27

- R27-2: no golden vector pins the bundle layout spec 11 depends on.
- R27-3: no unit test pins `open_shared` refusing a colour.
- R27-4: that rollback keeps the colour is shown by reading only.
- R27-5: the report names the wrong za module for the changed assertion.

### Task 28

- R28-3: `za_write_default` reads the previous default after `za_delete_calendar` may have cleared it; one edge differs from upstream, the effective default is the same.
- R28-4 (→ the owner, after the whole-branch review; corrected after the pull request's review, C4617611650): `za_commit_delete` submits the mail and then returns the result of the grantees' cache invalidation, after the deletion committed. On this path that result cannot be an error: the invalidation holds only `AccessToken` entries, for which `invalidate_caches` applies the changes on this node, awaits `cluster_broadcast` and returns `Ok(())`, and `cluster_broadcast` only logs a failed send. A committed calendar destroy is therefore answered `destroyed` with its `newState`, on one node and on a cluster. What a failed broadcast leaves is a peer that misses the invalidation: it keeps the grantees' cached authorization and resident collection keys for the deleted calendar until they expire or are next invalidated, while every read still resolves the calendar from the store and finds it gone. Invariant 75 is not touched. (This entry first said that a failed broadcast answered the committed destroy with D28's "try again"; the verification of the pull request's review showed that it cannot.)
- R28-5: after a failed post-commit read of the stored default the fallback can report `isDefault: true` wrongly; two traces mislabel.
- R28-6 (brief-internal): create checks `shareWith` before metadata, update the reverse; both answer `forbidden`.
- R28-7: coverage gaps: no create/update commit or default-write store-failure case, no D16 decline or denied-status case, invariant 58's clear fields unchecked, no `Calendar/set` metadata refusal case, `grantee_destroy_refused` reaches upstream's refusal.
- R28-8: ordinary accounts get two cached classification reads per `Calendar/set`.
- F1: `calendar.clone()` copies the whole unsealed calendar for `accept`, kept for parity.
- F2: the head store error is rebuilt with "calendar read failed" details on a write method.

### Task 29

- R29-1: the `SUMMARY:Mail` canary assertion passes from the VEVENT's own SUMMARY.
- R29-2: the parser drops an empty SUMMARY, so the empty-summary filter is never exercised.
- R29-3: no VTODO case and no nested-VALARM case.
- R29-4: that the root still references both VEVENTs is unchecked.
- R29-5 (brief-mandated): the tests exercise upstream's merge; the fork's choices are untested.
- R29-6: the overflow `false` return comes after the edit has started (unreachable).
- R29-8 (→ the owner): replacing alarms by value drops acknowledged or snoozed state that upstream keeps.
- R29-9: the brief adds a member fallback the spec lacks.
- R29-10: the SHIP, TM, FMT and mutation logs lack commands, source hashes and the mutation diffs.

### Task 30

- R30-2 (plan-mandated): the disk-quota and UID checks run in another order than upstream's.
- R30-3 (plan-mandated): two classification reads per call for every account.
- R30-4: `create_two_objects_one_fails` cannot tell per-object commits from one batch.
- R30-5: no case checks the UID in a non-first calendar; several cases skip spec 9.2's CalDAV readback.
- R30-6: the unreachable missing-UID arm is a check of the fork's own.
- R30-7 (plan-mandated wording): the reworded comment at `set.rs:1890` is inaccurate for `CalendarEvent/copy`.
- R30-8: the store-failure trace says "write failed" for read failures too.
- R30-10: the head of `CalendarEvent/set`'s key writer reads the account twice when the call has creates.
- R30-12: with duplicate `useDefaultAlerts` keys any true wins.

### Task 31

- R31-2: one occurrence's ruling refusal fails every sibling occurrence, including a destroy. Fixed after the pull request's review (`d86f90b31`, FC1-7 = C4617611635).
- R31-3 (plan-mandated): metadata on a synthetic id gets the fork refusal.
- R31-4 (plan-mandated): the quota check runs before scheduling, so a denied update answers `overQuota`.
- R31-5: the update path's D28 trace content is shown by reading only. **Weighs more than its rating**, with R33-1 (lens D): a regression that traced the original store error would pass.
- R31-6 (rest): the other base-patch rulings untested on update.
- R31-7 (→ Task 43 or a sheet commit): I §7.2 says a view error fails the method; in `/set`, D28 makes it per-object (recorded under "Decisions and remaining qualification").
- R31-8: review focus 1 shows a vault write happened, not that it was the pin's rewrap.
- R31-9: the JMAP/CalDAV comparison of key2's status passes if both are empty.
- R31-10: two refusal cases never read back.
- R31-11: the unreachable instance `ImportedValues` branch is kept without a comment.
- R31-12: the `hideAttendees` update refusal lives inside `update_uid_change_refused`.
- R31-13: no events case runs the owner-organized floor input any more.
- R31-14: the fix report's load figure does not match the preflight log.
- R31-15 (→ Task 38 or 43): the update tests origin against the owner's addresses while the floor and the sender use the sender's roles, so a grantee-organized event's JMAP update without scheduling gets no increment and no `updated` stamp; not examined with scheduling on.

### Task 32

- R32-3: D28 on the plan path has no case.
- R32-4: after the move back nothing reads the event as the owner.
- R32-5: `placement_and_content_together` cannot tell one commit from two (Task 11's case proves the single commit).
- R32-6: no case drives a grantee's placement change through `CalendarEvent/set`.
- R32-7: no case observes `content_changed`; either fixed value is equivalent at suite level.
- R32-8 (→ the whole-branch review): an event with two names in one calendar loses its second name on a wholesale `calendarIds` with the same set, with no vanished href; upstream folds names the same way.
- R32-9: `.filter(ZaPlacementPlan::is_change)` never drops a plan; one predicate or a `debug_assert` would guard divergence.
- R32-10: `Places::finish` checks only the 200 of `shares/revoke`.
- R32-11: the macOS linker note in suite output.
- R32-12: a local `stored_event` beside the field of the same name.
- R32-13: `events.rs` grows to 2233 lines; a listing is repeated.

### Task 33

- R33-1: the D28 trace of a destroy's store failure is not asserted; the event writer's create and update traces are equally unasserted. **Weighs more than its rating** (lens D; see R31-5).
- R33-2 (plan-mandated): the own-items leg and `may_write_own` cannot be reached through the product and have no test.
- R33-3: `InstancePlan::DestroyEvent` is newly live and untested.
- R33-4: `no_itip_task_ever` still deletes over CalDAV.
- R33-5 (→ the whole-branch review, then the owner as FB-5 with FC1-9): a destroy with scheduling of a future scheduled event whose sealed tree does not open is answered as the retryable D28 case and stops every later id; without the flag it succeeds.
- R33-6 (plan-mandated): a byte comparison cannot tell "not attempted" from "attempted and failed".
- R33-7: test hygiene (a helper defined twice, a repeated lookup, a keep-list use, a bare `created_id(0)` as an assertion).
- R33-8: the macOS linker note in every ZA and JK build.
- R33-9: `destroy` takes `&mut self` and mutates nothing; one name used three ways.

### Task 34

- R34-3: no case sends the triggering request with a Bearer token that carries a secret. **Weighs more than its rating** (lens A), though the risk is low: the hook's condition is the same for Basic and Bearer.
- R34-4 (→ the owner and Task 43; the final review's FC2-3): the hook runs before the `using` check, `allowed_capabilities` and every permission check, so a keyed token whose OAuth scope excludes calendars still drains when it names a calendar method (recorded under "Decisions and remaining qualification"). **Weighs more than its rating** (lenses A and C2; lens C1 calls it worth a decision but not blocking): release 6 first gives a key account scoped tokens that carry keys, and CalDAV's drain has the same property from this release on. Decided by the project owner on 2026-11-18 and fixed after the pull request's review (`6f0704dcb`, `0ca47a61a`): the drain runs only for a token whose scope admits calendars, on both protocols, and on JMAP only for a calendar call that parsed with its capability in the scope-filtered `using`.
- R34-6: the `CalendarEventNotification/get` reply is discarded.
- R34-7: the mail case does not assert one pending item before the query.
- R34-8: two RED receipts recorded different file hashes.
- R34-9: the macOS linker note; `lsof`'s "output may be incomplete" in the preflights.
- R34-10: comment accuracy in `scheduling.rs`.
- R34-11: the answer of key2's `Calendar/get` on key's account is discarded.
- R34-12: the fix round ran TM, ZA and FMT, not SHIP (tests only).
- R34-13: `clear_inboxes` asserts only that no pending item remains.

### Task 35

- R35-1: the filter `za_shown_capability(is_key, false, key)` is now a no-op, and its doc comment describes something it does not do.
- R35-2: the `&& !is_key` in upstream's expression has no comment.
- R35-4: no failing run for `mayGetAvailability` left true.
- R35-5 (inherited, medium confidence; → the owner and Task 43): `Principal/getAvailability` for a key principal in an ordinary group with calendars returns the group's busy periods while `mayGetAvailability` is false (recorded under "Decisions and remaining qualification"; README-dev's wording corrected after the final review). **Weighs more than its rating** (lens D): the documents said "an empty list".
- R35-6: a comment in `za_variants.rs` omits and narrates.
- R35-7: the macOS linker note; `lsof`'s warning.

### Task 36

- R36-3: the view grantee's create and alerts update assert the type `forbidden` only, where upstream words both.
- R36-4 (rest): R21-3 stays open; Task 21's concern (3) stays open (a grantee's `Calendar/get` `shareWith`, the clear fields of 6.7 and the description unasserted).
- R36-5: `grantee_default_alerts`' first event cannot tell the grantee's copy from the owner's live entry; the update path's writer argument has no grantee case; no ZA case sends `useDefaultAlerts: true` in an update.
- R36-6: `delegated_reply_carries_sent_by` counts sink mail by recipient and method, not UID.
- R36-7: `move_with_scheduling_into_shared` empties both inboxes wholesale.
- R36-8: `new_items_for`'s bool is always true; the first invitation's method is not asserted.
- R36-9: the report's load range does not match the preflight logs; one run's source is kept as a hash only.
- R36-11: `assert_account_not_found` has no in-helper control.
- R36-12: `grantee_destroy_sends_delegated_cancel` does not pin the property or the owner's address on the `SENT-BY` line.
- R36-13: the new case never checks the owner's or the grantee's inbox.
- R36-14: each step counts new pending items by UID, not all new items.
- (no id, re-review nits) `ids[1]` collected and unused; an `assert_ne!` without a message.
- R36-15: `stored_pair`'s panics name neither the case nor the step.
- R36-16: the case name and message prefix still say `move_with_scheduling_into_shared` though the loop's moves are quiet.

### Task 37

- R37-3: the invariant 66 read-back compares pending item ids only.
- R37-6: some out-of-scope slots stay unasserted for a key account (same generic parser path).
- R37-7: two rows assert the error type only where a fixed text exists.
- R37-8: no read-back after the refused `shareWith` update; a comment claims more than the check.
- R37-9: the keyless session check omits `urn:ietf:params:jmap:calendars:parse`.
- R37-10: `call_using`'s Bearer arm repeats `za_bearer_jmap` and drops the status.
- R37-11: one more temporary upload is left in the owner's account.
- R37-12: the attachment download's 404 stays with the rule it cites removed; an equivalent mutant at suite level.
- R37-13: the third impersonation form does not show in the test that the holder holds keys or shared access.
- R37-15: the metadata read-back uses a get without the metadata capability.
- (no id, re-review notes) the D29 case never sends a stale exact state as `ifInState`; the fixture `jmapcal-refusals-impersonate` needed a line in the fixture inventory (added to `CLAUDE.md`).

### Task 38

- R38-3: the organizer's `SCHEDULE-STATUS` in the attendee's copy is not read.
- R38-4: a JMAP reply's acceptance is caught only through a drain inside comparison 6's GET on the 500 ms client; the likeliest flake source.
- R38-5: "drained" at M8 and M9 can be satisfied by the oracle's own CalDAV GET.
- R38-6: an event left in both calendars at M3 can pass; the placement step does not tell a move from a copy.
- R38-7: `jscalendar_of` projects the first object only.
- R38-8: the tally line counts empty comparisons; it is not quoted here as "values compared".
- R38-9: Part B compares the pairs' recipients, not token values, and does not assert the list non-empty.
- R38-10: an unset items or sink in `Expect` is not asserted empty.
- R38-11 (spec half; → Task 43): spec 3.5's last sentence and 3.3/3.4 owe the record of Q1 and M2 (recorded under "Decisions and remaining qualification").
- R38-13: M9's reply takes its SEQUENCE from each side's stored event.
- R38-14: the METHOD line is not set against the message's MIME parameter.
- R38-15: M7b asserts only that the top-level `sequence` left the patch.
- R38-16: M7b's item and mail SEQUENCE are pinned only against the CalDAV side.
- R38-17: the per-step line prints the first SEQUENCE of each text; it is not quoted here as covering the override.
- R38-18: the module's header comment names neither M7b's exception nor the METHOD comparison.

### Task 39

- R39-3: several refusals assert only the type `forbidden`.
- R39-5 (→ Task 43 and the owner, with F1): three consequences of D11 are not in spec section 12 (recorded under "Decisions and remaining qualification").
- R39-7: the entry's comment says "in upstream's order", but availability runs before `principal_get`.
- R39-8: the F1 control's source was not kept, only its hash and output.
- R39-9: two observations with no change proposed.
- R39-10, R39-11, R39-12: the report's acl table lists three unchanged rows, says one read is "no longer listed" though its row is, and overstates "ends as upstream's body ends"; this note takes the adjusted assertions from the code.
- (no id, re-review notes) the report's head table shows a superseded hash; upstream's `calendar::instance::test` has no emptiness check of its own; `CLAUDE.md` and README-dev described key mode as running "the calendar refusals" (corrected by this task).

### Task 40

- R40-7: the trace capture's only control is that it is not empty, and it ends before the revocation and the password change. **Weighs more than its rating**, with R40-9 and R40-13 (lens D): the leak module is the release's only whole-flow trace and store net.
- R40-9: `secret_forms` has no unit control.
- R40-11: the report says 26 labelled secrets; the code builds 24.
- R40-12: the conditional deletion of the owner's upstream OAuth grant does not record which branch it took.
- R40-13: `secret_hits` does not decode LZ4-compressed pickles above 8 KiB, has no URL-safe embedded form and reads the logical store only (an evidence limit).
- R40-14: `leak.rs`' header comment does not mention the new rule.
- (no id, re-review notes) R40-9 carries more weight; the post-session scans assert only `token_records == 0`.
- R40-15: the comment at `leak.rs:1894-1896` and the report give the wrong character count for a 32-byte value's slices; the behaviour is right.
- R40-16: the invitation's own colour is not read back.
- R40-17: the task canary is searched in base64 and hex only after the task is destroyed.
- R40-18: `events >= 3` where the count is exactly 3.

### Task 41

- R41-1: the report omits the product binary's version line.
- R41-4: the PRODUCT, fresh-store unit and FRESH receipts record no command, environment or HEAD.
- R41-5: the plan's step 1 script ignores dispositions and does not test equivalents or rename targets.
- R41-6, R41-7: the rerun decision rests on local evidence only; the report does not table the five runs. **Weighs more than its rating** (lens D): the key-mode JMAP step has no rerun and now runs upstream's timing-sensitive `calendar::alarm`; the first hosted-runner evidence comes with the pull request.
- R41-9: the new evidence strings drop the backticks the existing entries use.

### Task 42

- R42-2 (→ this table): J7's named catcher cannot catch J7; the expanded assertions of `two_accounts_same_answers` are the only catcher.
- R42-3: J3's kill is the indirect doubled mail.
- R42-4: J16b's suite run, J19 and J25 each failed in an earlier case of the same property.
- R42-5: for J10 and J11 the failing loop iteration is not in the log.
- R42-6: J14's kill is the gate's refusal answer, not the item-survival assertion.
- R42-7 to R42-12: J8's mutant is the brief's stronger form; the runner sets no `RUST_BACKTRACE`; J26 is unit-only evidence; J18's arm is slightly broader than the row; J1 keeps the lease and skips only the UID check; unit runs have no command log, and one batch remark about an unused import is wrong.

### For the whole-branch review

The ledger's coverage notes (⚠️) that were open for Task 44's reviewer and stay open after its review:

- Task 18: the `Lost` trace is unobserved (the leak module's capture now spans a refresh for two accounts); R18-1's window is wider on networked stores than on local RocksDB.
- Task 21: the payload-free boundary rebuilds Store errors only.
- Task 22: `/queryChanges` origin forwarding has its runtime evidence in Task 26; upstream's paging of `/changes` after a log trim is inherited unchanged (open item 4 below).
- Task 26: cost figures are local RocksDB, one node, debug build; that an external search store receives no term is by reading `dispatch/search.rs` only.
- Task 28: `za_delete_calendar`'s post-commit error reachability (R28-4) and `za_repair_owned_context` were not read.
- Task 30: the email-alarm cleanup is shown only by the closing `wait_for_tasks`; the lease race is exercised at one interleaving (`COMMIT_PAUSE`).
- Task 31: after the fan-out, `za_commit_update`'s own error returns end the mail guard as `ZA_IMIP_ABANDONED` rather than `ZA_IMIP_DROPPED` (pipeline code); upstream's draft gate on scheduling and its hidden-participant path are not mirrored (they rely on no key event carrying those flags).
- Task 32: the placement UID check holds no lease and no batch fence (`pipeline/update.rs`), so a concurrent create or move of the same UID into the same calendar can both commit, while spec 3.3 said "checked under the UID lease" (true of creates only; spec 3.3 corrected in `53cf84aa9` on the project owner's decision, follow-up issue #91); on the plan retry `za_placement_contexts(..).await?` returns without `za_mail.dropped()` (the no-plan branch has the same shape).
- Task 36 (fix round 2's re-review): narrower mutants still survive by reading: a quiet link or unlink of an event with attendees, a quiet move between two unshared calendars of the same domain, a grantee's quiet move, recurring or multi-attendee events, the commit's bounded retry on a quiet move. X1's stated consequence (a later reply refused) is never observed, and the second quiet move has no kill evidence of its own.
- Task 37: the strength of the `stateMismatch` assertion against a fault that takes `to_id` as the state for calendars is unconfirmed without a run (R37-1's control shows it).
- Task 38: R31-15 is not examined by the oracle, which has no grantee step; the oracle covers no sharing, alerts or queries.
- Task 40: no earlier scan meets a live token record, so the field-153 rule's only live exercise is `leak::test_jmap_calendars`; spec 9.5 words the iTIP-task rule as a scan rule "for every scanned account" while the code checks it in the module.
- Task 41: the CalDAV client was not run in either mode, so that leg of spec 5.1's proof of the move stays open until CI; the inventory and coverage gates did not run in Task 44 (they wait for the owner's word); no receipt records `git status` during the runs.
- Task 42: the unreached named catchers are not shown to fail under their mutants; a clean tree after each row rests on the hash check and the batch-end status; X2 ran under ZA only; the unscheduled JMAP update that omits `sequence` on an event with an override is compared by no test (X2's residual).
- R32-8 and R33-5 (above): the review found R32-8 still true and latent (upstream folds names the same way) and sent R33-5 to the owner.

### The final review

Deferred by the final review and its re-review (by id; the review files hold each in full):

- FA-5: the D22 precheck's answer when the token record cannot be read.
- FA-8, FA-18 (= R18-3), FA-19: test gaps on the token paths.
- FA-16: one exit of `delete_oauth_grant` skips the record entry's removal; the sweep bounds it. Closed after the pull request's review (`d50107f12`).
- FB-4 (with Task 31's and Task 32's coverage notes): the placement arm's exits end the mail guard as `ZA_IMIP_ABANDONED` where `ZA_IMIP_DROPPED` is true; nothing is mailed either way.
- FB-8 (= R38-6), FB-9, FB-10: test weaknesses in the oracle and the placement cases.
- FC1-3: a repeated update of one event id in one call.
- FC1-17: thin coverage of the occurrence-patch rulings.
- FC2-18 (= FD-4 = R21-12), FC2-19, FC2-21 (= R28-7), FD-5, FD-7 (= R40-7), FD-9: test weaknesses.
- FD-11: new fork tests sit in `parser_restored_tests.rs`, which the gates' tooling groups as restored upstream tests; for the gates' run.
- FD-15: `ZaPipelineError` derives `Debug`; no use site formats it.
- W-1: a request that passed authentication before a grant's revocation can make that grant's keys resident after the eviction; they are unreachable and stay until the key cache's idle or age limit. Closed after the pull request's review (`dff25d1d9`): the keys are inserted only under the revocation epoch read before verification, and a grant's invalidation removes them after the epoch advances.
- W-3: `query::grantee_text_skips_a_sealed_class` has no ordinary twin.
- W-4: `tokens::traced_urls_mask_the_code` exercises one of the three URL sites; `traced_url` has six unit tests.

### The pull request's review

Deferred by the rulings on the two waves' reviews (the review reports hold each in full):

- RA-8: wave A's implementer went on where its brief said to stop (the store property failing on the ephemeral store); ruled accepted.
- RA-10: the change-id fence's store property is pinned on RocksDB and SQLite only; PostgreSQL and MySQL hold it by reading (MySQL's abort rests on its driver rolling back a dropped transaction, not read). A run of the unit test against each would close it.
- RA-11: on RocksDB a large one-transaction delete is retried whole on a conflicting commit of the account (a mail delivery writes the same quota counter), up to 24 times or 10 s, then fails after the fan-out. For issue #92.
- RA-12: the placement loop's new early return leaves the mail guard unsettled, so a write known not to have committed traces `ZA_IMIP_ABANDONED`; unreachable, as the loop's other early returns.
- RA-13: a non-member's destroy of an event whose view does not open now answers `notFound` before the calendar checks; JMAP the stricter side.
- RA-15: a grantee's poll loses upstream's no-change shortcut when a container change is the last record of the log: one more log read per poll.
- RA-16 (inherited): on a cluster a stale resident snapshot can answer a state below the one the client sent.
- RA-17: upstream's `calendar_event/get.rs` lost an import line, a new merge point (in `CLAUDE.md`'s hotspots).
- A failing CalDAV collection `DELETE` (5xx with one dropped trace per event) is no longer exercised over CalDAV on a fenced store; the JMAP case covers the shared function, and `chunked_collection_delete_unfenced` the unfenced form.
- RB-6: the closed FA-16 exit has no test; reaching it needs a hook between the failed write and the re-read.
- RB-7: the re-check after the keys' insert and the keys' removal after the epoch advances are sound by trace and pinned by no test; each needs a hook between two statements.
- RB-12 (older): resident keys can outlast their authentication entry; unreachable, since only a live entry leads to them.
- RB-14: the doubled eviction's cost (stated in spec 10 and `CLAUDE.md`).
- RB-15: `dff25d1d9` fails its own new case until `7bd35c4c4`, so a bisect can land on a red suite there.
- RB-18 (inherited): the exits of a grant's deletion that learn the grant is already gone clean its token entries but do not evict this node's cached sessions.
- RB-19 (inherited): a lost broadcast leaves a peer serving a deleted grant's token from its cache until the entry's TTL or the keys' idle limit.
- Wave B's fix round 1 re-review, N-1 and N-2: the expiry case's heading says "sub-second skew", which understates what remains (the time from the decode to the cache deadline, plus under a second of rounding; the heading was rewritten by the second review's wave C, `90d6ad286`, so this nit no longer applies); one comment line of `drain.rs` is 90 columns.
- Wave A's fix round 2 re-review, a window within one call (Minor): a grant that commits between a call's boundary check and its read of the tip leaves the raised state of an exact request, or of the last page of an exact-origin continuation, above the grant, so the newly shared calendar's older events are not sent (spec 6.8, case (b)). It comes from wave A's raise (`5f736c77f`); reading the tip before the boundary check would close it. It was not changed in this pull request: the window is the store reads of one request, and a code change would have voided the validation in flight.
- Wave A's fix round 2 re-review: `paged_sync_retention_collision` pins the final state of an initial-origin continuation, but no case makes the call that follows it.
- Wave A's fix round 2 re-review, outside its fix diff (Minor): the parked CalDAV `DELETE` of `destroy_fence_lost_to_a_crossing_write` is sent with a five-second client timeout and held at a pause; older than the round.

### The pull request's second review

Deferred by the rulings on the reviews of wave C (the review reports hold each in full):

- RC5-2, RC5-3: the liveness filter and the one-second subtraction of a keyed entry's deadline are decided by no case.
- RC5-8: the cost of a keyed request in a token's last seconds (stated in spec section 10; `docs/README-dev.md` gives the two figures).
- N1: `expired_second_decode_publishes_nothing` can lose its power over the decode-error arm on a very loaded host, silently.
- N2: that case arms two pause slots without a drop guard.
- N3: the cache clock's now is taken from upstream's `HttpAuthEntries::expires_in(0)` (a silent dependency, in `CLAUDE.md`).
- N4: two comments cite review tags.
- RC-3: with a name the path index hides, a keyed `COPY` or `MOVE` into that calendar is not seen by the UID check, so two names in one calendar stay reachable through the inherited name race, and a single-name delete then removes the first name under the calendar.
- RC-4: no case runs the pipeline near the size cap.
- RC-9: a name whose calendar no longer exists counts as a name that keeps the event.
- RC-10: release 4's keyed collection `COPY` with an overwrite (also under "Accepted residuals").
- RC-7's remainder: the code comment describing the unfenced form in `crates/common/src/za/test_hooks.rs` does not mention that the unwritten-child refusal is absent there.
- The calendar part's re-review, outside its fix diff: the five-second client of `jmap_calendar::calendars` bounds every parked `Calendar/set` of the module, not only the moved-child case; the renamed pipeline case's panic text keeps the older wording ("stale child list").
- RD2-18 (wave D's review): two lines that a raw batch clearing an event's content row might reach; not tried.
- Wave D's fix round re-review (Minor): the merged coverage figure reuses the filesystem-blob step's profile from `f3630cf67`, which still held the removed case, so the margins may be overstated by up to about eight lines each; a fresh profile of that step would settle it.
- Wave D's fix round re-review (Minor): the second-issue case pins the stored half of a latent oddity of the token record's issue function: a second issue for a grant returns a refresh secret no entry holds (a client's next refresh would be `Lost` and the grant deleted, a sign-out); no product path reaches it.
- Wave D's fix round re-review (Minor): the update's disk-quota case's comment says the pre-check refuses, which no assertion shows, since both paths give the same answer.
- Wave D's fix round re-review (Minor): the confidential-UID case's negative check passes vacuously if the refused response's body cannot be read.
- Wave D's fix round re-review (Minor): the ordinary-account loop of the contacts refusals asserts the answer only; nothing is read for the ordinary account after it.
- Wave D's fix round re-review (Minor): three comments trail their cases (two doc comments that do not list their new blocks, and one spec citation that names the wrong spec).
- Wave D's fix round re-review, outside its fix diff: a second issue after a rotation would add a generation-0 refresh entry below the stored generation (invariant 67); unreachable; the guard is filed as issue #95 (item 24).
- Wave D's fix round re-review, outside its fix diff: the unit count reported for the fix round (914 passed) could not be reconciled from the reports.
- Wave D's fix round re-review, outside its fix diff: the first review's open item on `contacts/forget` with null stands (the vault's wrap mark before and after is not asserted).

### The pull request's third review and the second whole-branch pass

- FR-6 (the fresh pass, suspected): `onSuccessSetIsDefault` writes the default calendar id with no assertion, so a delete of that calendar between the call's snapshot and the write leaves a default naming a deleted calendar; the mechanism confirmed by reading, the consequence beyond "no calendar reports `isDefault`" not traced. Recorded as a comment on issue #92 (2026-11-20).
- RE5-3 (wave E's review): the owner's destroy still runs the loop over the calendar's children from the path index, which its path does not read; it mirrors upstream's handler body and is in memory.

## Merge hotspots

`CLAUDE.md`'s paragraphs "Release 6 added merge hotspots" and "Release 6 silent dependencies" hold the full list: spec 11's two lists with Task 43's additions. In short:

- **Fork insertions in upstream files** (spec 11): `crates/http/src/auth/oauth/{auth,token}.rs`, `crates/common/src/auth/oauth/{token,grant}.rs`, `crates/http/src/auth/authenticate.rs`, `crates/common/src/auth/{vault,access_token}.rs`, `crates/types/src/field.rs`, `crates/services/src/task_manager/tasks/maintenance.rs`, `crates/jmap/src/api/{request,session}.rs`, `crates/jmap/src/principal/get.rs`, `crates/jmap/src/changes/{get,query}.rs`, the two `/set` handlers' head branches, the two `/get` handlers, two points in `calendar_event/query.rs`, `participant_identity/{set,changes}.rs`, the shrunken `dav` key branches, `crates/{dav,jmap}/Cargo.toml`, and D29's `crates/jmap-proto/src/{types/state,request/parser}.rs` and `changes/za.rs`.
- **Added by Task 43**: upstream's `changes/get.rs` body moved unchanged into `ChangesPage` (R22-4); the key writers repeat the heads of upstream's `/set` handlers; `za_itip_fails_write` names every `ItipError` variant; `za_split_bearer` relies on `TOKEN_HEADER`; D29's codec, context checks, origin preservation, legacy owner chain and `/queryChanges` forwarding; D11's recipient and description rules copied from `alerts.rs`; the query branch's reliance on no `EVENT_HIDE_ATTENDEES`; each use of `za_session_binding`; the pinned 404 text (R37-16); the oracle's dependence on upstream's converter; the calendar variants of `za_variants.rs` (R44); R27-1's rollback.
- **Added by the pull request's review**: `crates/store/src/write/{batch,mod}.rs` (the one-transaction flag, its size predicate, the branch at the head of `commit_point()` and the declaration of `za_change_id_fence_tests.rs`); `crates/dav/src/request.rs` (the drain's keys through `za_drain_keys`) and `crates/dav/src/calendar/mod.rs` (the `CalendarTooLarge` arm, 403); `crates/jmap/src/api/request.rs` again (`za_call_admitted` in the drain's and the search pre-pass's conditions); `crates/common/src/auth/oauth/grant.rs` again (`za_evict_deleted_grant` before the token cleanup; the cleanup on the exit whose re-read finds the grant gone); `crates/common/src/cache/invalidate.rs` again (the account's keys removed after the epoch advances); `crates/http/src/auth/authenticate.rs` again (the "Cache credentials" block moved above the key lookup, the live-entry condition, `za_insert_keys_fenced`); `crates/jmap/src/calendar_event/{get,query}.rs` (`za_event_state`; `get.rs` lost an upstream import line, RA-17). Silent dependencies: the fence relies on the store allocating a batch's change ids in the transaction before its merge functions run and aborting on their error (pinned for RocksDB and SQLite, read for PostgreSQL and MySQL), and mirrors release 4's `za_move_overwrite_container`; the guarded raise reads the log with the reader upstream's page uses.
- **Added by the pull request's second review**: the keyed `COPY`'s condition in `crates/dav/src/calendar/copy_move.rs` (a `COPY` inside one calendar runs the UID check); the result struct `ZaCalendarBuild` of `delete_with_events_za` and the `all_names` argument of `delete_from_calendar_za` in upstream's `crates/groupware/src/calendar/storage.rs`; `za_bearer_entry` beside the keyed arm of `crates/http/src/auth/authenticate.rs`. Silent dependencies: the cache clock's now taken from upstream's `HttpAuthEntries::expires_in(0)` (N3); `za_calendar_members` reading the snapshot's resource records.

## Decisions and remaining qualification

**Approved.** D19 (decided 2026-11-14) and the explicit-origin D29 amendment (approved and recorded 2026-11-15) are approved; origin inference is not reopened.

**D29's evidence.** Task 22 (handler level, local RocksDB): `paged_sync_retention_collision` reproduces the retention-marker collision through the real HTTP handlers with a log truncated by the existing `truncate_change_log` API in setup, not by the maintenance timer: the first pages carry distinct origins, and after a rename the initial-origin page is served and the exact-origin page answers `cannotCalculateChanges`. `paged_sync_origin_survives_range_changes`, `paged_sync_legacy_and_ordinary` (a grantee's legacy `r` restarts from Initial; a key owner's `r` chain stays `r`; ordinary accounts' states byte-equal to upstream's), `calendar_state_slot_errors` and `precondition_matrix` (out-of-scope `z` is `invalidArguments` in a state slot and `stateMismatch` in a precondition slot) and the codec and parser unit tests (`za_calendar_state_*`, `za_calendar_state_contexts`). Task 26: `query_changes_paged_origin` pages `/queryChanges` from Initial and from an exact state, three pages or more, every intermediate state a `z` with its origin unchanged; exact-origin continuations are refused while a boundary is pending and after a rename, initial-origin ones served; legacy and ordinary forms as above. This is source and handler evidence; it is not the reference client's result. The checklist's D29 rows (`docs/zero-access/client-jmap.md`) are that separate gate, and they have not been run.

**Unresolved findings.** No task review left a Critical or Important finding after its fix rounds. The final review found no Critical finding and six distinct Important ones (with 78 Minor findings, duplicates included): five of the six are fixed (FA-1, FA-2, FA-3, FC1-2, FC2-1; FA-3's masking confirmed by the project owner on 2026-11-18, item 1 below), and FC1-1 was decided by the owner on 2026-11-18 with no code change and a line in spec 3.3 (item 3 below). The pull request's review then confirmed twelve findings, all fixed (C4617611565 fixed, with a limit: a paged sync from the initial state still ends below the boundary), corrected in the documents or decided by the owner, and its two fix waves' reviews and their fix rounds' re-reviews left no Critical or Important finding unaddressed ("The pull request's review"). On the fixed source the final matrix had 30 rows pass on their first attempt and 2 after reruns: row 10 on the rerun the plan allows, row 15 on a third attempt after the allowed rerun also failed, by the controller's classification of upstream's `queue_retry` as a timing flake ("Rulings"). On the source of `25e098520`, after the pull request's review, 30 of 31 validation rows passed on their first attempt; key-mode `jmap_tests` failed once in upstream's `calendar::alarm` test and passed on two runs of the unchanged command, a failure the controller classified as timing ("The pull request's review"). What the final matrix did not run locally, the Docker suites and the independent CalDAV client, the pull request's CI ran on `dbd2fccc8`, which carries the final matrix's source, and again on `d6929c89e`; open are those suites, the CalDAV client, CI and the retention gates on the second review's commits, and the reference client's checklist, which has not run at all (below). The gates ran once, on `d6929c89e`, where the coverage gate failed; wave D's local measurement on `0c73eafc4` passes it, which CI's gates job has not confirmed ("The pull request's second review"). The third review's three findings are fixed; of the fresh pass's findings, FR-1 and FR-2 were decided by the owner on 2026-11-20 with no code change, FR-3 is fixed, FR-4 and FR-5 were already recorded, FR-6 is deferred and FR-7 is stated in spec 3.4; wave E's review left no Critical or Important finding. The gates job of the manual run on `68d905131` had not finished when this record was written, and that run's key-mode JMAP step failed once ("The pull request's third review and the second whole-branch pass").

**Observed status of the costs the brief names.** The memory of an unranged text query was not measured: the query holds every candidate's terms, and by R26-5 every opened event's full plaintext view, for the request (spec 6.4 accepted holding the terms). Its time is review focus 5's. The `/set` answer after D11 is upstream's, as spec 6.5 says: `events::create_use_default_alerts` asserts that a create sent with `useDefaultAlerts: true` answers with neither `alerts` nor `useDefaultAlerts` (`events.rs:1158-1161`), and `events::create_reads_back_over_caldav` that a create without the flag answers `useDefaultAlerts: false` (`events.rs:960`); the alerts appear on the next `CalendarEvent/get`.

**Open items for the project owner.** None blocked a task. This list joins the task reviews' open items with the final review's; where the two overlapped, the final review's id is named beside the ledger's. Each carries the adjudication's recommendation.

To confirm:

1. **The `code` masking (FA-3 = FD-1)**, as an amendment of spec section 10: the value of every `code` query parameter is masked in a traced request URL and a traced `Referer`, for every request, a third every-account trace exception beside D24 and D26. Recommended: confirm; it is in the code at `f1c69900d`. **Confirmed** by the project owner on 2026-11-18; spec section 10 states it.
2. **D15 as a sync-correctness rule, not an access control** (ruling at Task 22; the final review's FC2-5, which shows the token that skips the boundary check, since a `z` state's origin is not authenticated). Recommended: confirm. **Confirmed** by the project owner on 2026-11-20, after the fresh pass rated the same mechanism Important (FR-1), together with his acceptance of a grantee's `destroyed` ids as inherited (FR-2); no code change ("The pull request's third review and the second whole-branch pass").

To decide:

3. **FC1-1 (= R36-10)**: a key account that creates an event as an attendee with the scheduling flag sends no reply, and that copy never schedules afterwards; upstream's JMAP create falls back to the attendee's reply, CalDAV's does not. The code is what D1 asks for; the case `scheduling::attendee_create_sends_no_reply` pins it. Two consistent ways out: add the line to spec 3.3 (recommended), or amend 3.2 so the scheduling input carries "a create by a non-organizer may reply", set by JMAP only, with one fallback in the pipeline's create arm (which CalDAV shares). Lens B's answer on what such a reply would reach: for a key organizer the fan-out would not attempt it (the copy holds no token pair, the organizer's status would record 5.3, and the drain drops a reply without a token); a mail-class organizer would be mailed. **Decided** by the project owner on 2026-11-18: no code change, a line in spec 3.3.
4. **FC2-3 (= R34-4)**: the drain runs before the `using`, scope and permission checks, so a keyed token whose OAuth scope excludes calendars still drains (spec section 2 says a scope limits what the server will do for the token). The drain does only what the account's next keyed calendar request does, and the token chooses nothing. If the scope is to be honoured, the JMAP change is one condition; CalDAV's drain has no scope-filtered value at that point. Otherwise a line in section 2. **Decided** by the project owner on 2026-11-18 and **fixed** (`6f0704dcb`, `0ca47a61a`), with CalDAV and the search pass: the verification showed that the token's scope is a value both triggers can read; spec 5.5 and section 10 state the rule.
5. **FC2-6**: a default alert's id, a string the client chooses, is stored in the clear. Recommended: accept under 6.7 ("default alerts stay visible") with a line in the spec now; the key writer refusing or rewriting ids that are not short tokens as a follow-up.
6. R29-8: acknowledged or snoozed alarm state is lost with by-value default alerts.
7. R18-7: a well-formed secret appended to a keyless grant's refresh token deletes that grant. The fresh pass (FR-5) adds that the precheck already holds the record and could refuse before the rotation.
8. Upstream's paging of `/changes` after a log trim is inherited unchanged and applies to ordinary accounts' continuations as well (Task 22's concern 1); pending the owner's decision, not accepted.
9. The two oracle rulings of Task 38: items and mail compared as the JSCalendar projection, and the script's explicit SEQUENCE with M7b leaving it to the server.
10. Task 39's F1 with R39-5: three consequences of D11 not in spec section 12. A constant description on every component would remove the first two and needs a change to D11's wording.
11. Task 39's F3 note: section 12 words the view-level consequence as "cannot rename or recolour"; the check also refuses `isVisible` and `isSubscribed`.
12. R41-8: the fresh-store restart script exercises nothing release 6 adds (no OAuth grant, no token record across the restart, no JMAP calendar call).
13. R31-15: a grantee-organized event's JMAP update takes a different path from its CalDAV twin (origin tested against the owner's addresses, the floor and sender with the sender's roles); with scheduling on, the update still goes out with a raised SEQUENCE (lens C1), so the gap is confined to scheduling off.
14. R43-4: whether release 4's sharing spec and plan 14's outcome note get a pointer to D14 at their four statements of the colour exclusion (listed under known staleness below).
15. FC1-4 (an owner's per-user-only update stores the converter's whole output): no loss was established; recommended no change.
16. FB-6 (a move out of two unshared calendars into one shared calendar is refused, though two updates reach the same end state): recommended accept, or a follow-up.
17. FC2-15 (`shareWith` is refused whatever its value, `null` and `{}` included): recommended to let the reference client's checklist decide; if the client sends an empty `shareWith` on a create or rename, those writes fail. The line is in the checklist's "what the client may meet".

**Settled by the pull request's review** (2026-11-18): FB-3, spec 3.3's wording corrected and follow-up issue #91 filed; FC2-3, fixed with CalDAV and the search pass (item 4); FC1-7, fixed; the split calendar destroy, fixed on RocksDB, SQLite, PostgreSQL and MySQL, with issue #92's notes (item 23); FC1-1, no code change, in spec 3.3 (item 3); FA-3's masking, confirmed (item 1); FA-16 and W-1, closed by wave B; R44F-5, the co-author line of four documentation commits, which stays by the project owner's word.

**For the owner, after the pull request's review** (none blocks the pull request):

18. RA-4: accept the calendar delete fence's refusal after the cancellations as a residual, or ask for the internal retry. Accepted: confirmed by the project owner on 2026-11-19.
19. The store ruling: FoundationDB, a read replica and the ephemeral store keep the chunked, unfenced calendar delete (release 4 refuses those stores outright for a rarer route). Confirmed by the project owner on 2026-11-19.
20. RB-3: a `Blob` call of a token without the mail scope still triggers the search pass. Confirmed by the project owner on 2026-11-19.
21. RB-4 and RB-5: what a token's scope admits on DAV is upstream's scope model, unchanged; the drain's scope rule limits the drain, not what a token can read. Whether the fork should change that was the owner's question; his disposition of 2026-11-19: no change in this pull request.
22. Filed on the owner's word of 2026-11-19 as issues #94 (read the calendar log's tip before the sharing-boundary check) and #93 (a calendar sync paged from the initial state can end below a sharing boundary), for spec 6.8's two open cases: read the tip before the boundary check, which closes spec 6.8's case (b); and carry the first page's tip in the `z` state, which would let a paged initial sync end above a boundary that predates it and so remove case (a), a change that needs a new version of the state's codec. An earlier wording of this record named a third case that the last re-review's trace showed cannot arise from a state the server issued.
23. Issue #92 carries a note of 2026-11-18 that this pull request makes the delete one transaction, and a note added on 2026-11-19, after the push, of what it keeps: a calendar too large for one transaction, the stores that keep the chunked form, the fence's refusal after the cancellations (RA-4), a large delete retried whole on a conflicting commit (RA-11), and PostgreSQL and MySQL read, not run (RA-10). A comment added on 2026-11-20 records the default-calendar id written across a concurrent delete (FR-6). Its title and body are still unedited.

**After the pull request's second review**: its rulings and the coverage wave are the controller's and were reported to the owner; none was put to him as a decision. Each of the five findings is fixed; the name fence on creates that C4633314503's root would need is not taken ("Accepted residuals").

24. A guard in the token record's issue function that refuses a grant already holding entries, proposed as a follow-up by the controller after wave D's fix round re-review. Called a second time for such a grant, the function would return a refresh secret no entry holds and, after a rotation, add a generation-0 refresh entry below the stored generation, which invariant 67 says no write adds. No product path calls it twice (its one caller issues for the grant it has just created), so no code changes in this pull request. Filed on the owner's word of 2026-11-20 as issue #95.

**For the owner, after the third round** ("The pull request's third review and the second whole-branch pass"):

25. A rerun for CI's key-mode JMAP step ("JMAP Tests (key accounts)"), which has none: it failed once in the manual run on `68d905131` at upstream's ordinary-account thread merge and passed in the pull request's run on the same commit. Proposed by the controller and not made.

**Proposed spec amendments, for the owner** (the spec file was not edited by this plan; after the pull request's review, the lines below marked as made are in the spec):

- **3.3**:
  - FC1-1 (if the first way out of item 3 is taken): an attendee's JMAP create sends no reply, and that copy does not schedule afterwards. Made (spec 3.3), on the owner's decision of 2026-11-18.
  - FC2-2: a grantee's JMAP read of an event whose CLASS is sealed is private (as `/get` and, since the fix wave, the text query treat it), while CalDAV serves it whole; field policy 2's recorded limit, not this release's. Spec 3.3's line on a non-member held to an event's effective privacy, added by the controller's ruling on C4617611549, now states this difference, for writes too.
  - FD-3: a key account's reminder push names a positional alert id (`k1`), since the alarm's id is sealed.
  - FC1-5: a grantee's email default alert is mailed to the calendar's owner (release 1's alarm task); or a line in 3.4.
  - FC2-16: JMAP destroys the default calendar that CalDAV's DELETE refuses in a product build (inherited).
  - The UID lease: "checked under the UID lease" holds for creates only (FB-3); a placement change checks against current archives without a lease, as CalDAV's MOVE does. Made (`53cf84aa9`), on the owner's decision; follow-up issue #91.
- **3.3 and 3.4, 3.5's last sentence** (R38-11): record that a JMAP write stores upstream's converter form (`DURATION` for `DTEND`, the export's property order), stamps `CREATED` and the update stamps, and raises SEQUENCE for a client that omits it, so its iCalendar text differs from a CalDAV client's for the same edit; the oracle compares at the JSCalendar projection for that reason.
- **Section 10**:
  - the `code` masking (item 1), a third every-account trace exception (made, after the owner's confirmation);
  - the device-code poll whose `device_code` is the pending user code now answers `expired_token` where upstream answers `authorization_pending` (ledger 477), and an ordinary account's consent token with a 43-character suffix is accepted with the suffix stripped (FA-11);
  - the costs: the token sweep's one read of field 153 per account in every purge run (FA-12); a pass over the key cache on any account's grant deletion (FA-4's fix); two cached classification reads for `Calendar/set` and `CalendarEvent/set` and three for a calendar `/changes`, where the section says one.
- **D8's sentence** (FA-6 = R18-8): a refresh over an unreadable token record deletes the grant, since it cannot rewrap a master key it cannot open; "a sign-in or a refresh writes over it" holds for a sign-in only.
- **4.5 step 5** (FA-17 = R18-2): a refresh crossed by a credential change before its first read is classified `Lost` and deletes the grant; spec 9.2 tolerates either answer.
- **Spec 8's row for missing access** (FC1-16): a grantee's update in an unshared calendar answers `notFound`, which reveals less than the row's `forbidden`. **8** also: whether `Superseded` and `VaultChanged` refusals are traced (R18-6).
- **6.2, 6.10, 6.11** (R35-5): "an empty list" holds only when the key principal is in no ordinary group with calendars; a key member of an ordinary group returns that group's busy periods.
- **Section 12 (D11)** (R39-5): add the `iCalendar` member and override patches in subset reads, the stale description after a rename, and alarms that stay after a later `useDefaultAlerts: false`.
- **Section 12 (6.7)**: "cannot rename or recolour its own entry" becomes "cannot change its own entry, its visibility and subscription flags included".
- **Spec 10's amendment list and the sharing spec's colour statement** (R43-4): the amendments do not list the sharing spec's statement that no entry's bundle carries a colour, which D14 replaces.
- **4.5** (R18-7): a keyless grant presented with a well-formed secret.
- **9.5** (Task 40's note): the iTIP-task rule is checked in the module, not as a scan rule for every scanned account.

**Follow-ups not taken by the fix wave**, each with the adjudication's recommendation:

- R18-6 with FA-13: three token refusals emit no trace, and a store error on the Bearer key path is answered 401 with its cause dropped. Recommended: a store error answers a server error and is traced. Not in the wave, because it changes what a client sees under a store fault.
- FB-5 (= R33-5) with FC1-9: for the owner or a member, an event that does not unseal is `notFound` on update, a store failure on a flagged destroy (stopping later ids on every retry) and destroyed without the flag; since the pull request's review a caller who is not a member is answered `notFound` (RA-13). Recommended: treat it per object on destroy too.
- R28-4: after a calendar destroy commits, a failed cluster broadcast is only logged; the destroy answers `destroyed`, and a peer can miss the grantees' invalidation until their cached entries expire (corrected after the pull request's review).
- FA-5: the D22 precheck's answer when the token record cannot be read.
- FA-16: one exit of `delete_oauth_grant` skips the record entry's removal (bounded by the sweep). Closed by wave B of the pull request's review (`d50107f12`).
- FB-4: the placement arm's exits trace `ZA_IMIP_ABANDONED` where the outcome is known.
- FC1-3: a repeated update of one event id in one call.
- A guard in the vault commit funnel that refuses a binding change under an unchanged salt, which would make FA-1's fix a checked property of every vault write (the unit test `credential_change_clears_the_token_record` would need four rows changed).
- W-1 (resident keys after a revocation crossed by an authenticated request), with its fix named under "Deferred findings". Closed by wave B of the pull request's review (`dff25d1d9`).

**Known staleness in the plan's own documents and in older records** (not corrected here; none of these files is in this task's list): R22-7 (the interface sheet's §7.1 call-site list and the header's `za_variants` row); R31-7 (the sheet's §7.2 says a view error fails the method; in `/set`, D28 makes it per-object); the plan header's routing lines (R41-2 and R41-3 in `dispositions.json` were corrected by the final review's fix wave). Release 4's documents still state the colour exclusion that D14 replaces (Task 27's note; ruling R43-4): the sharing spec `docs/superpowers/specs/2026-10-22-zero-access-sharing-design.md:148-150` ("no supported client sets a per-principal colour") and `:566-570` ("No entry's bundle carries a colour a client could set"), and plan 14's outcome note `docs/superpowers/plans/2026-10-23-zero-access-plan14-outcome.md:439` and `:1445` (row T6-P: the shared format excludes the JMAP-only personal colour). Since release 6, each principal's colour of a shared calendar is sealed in its own personal bundle and the grant's conversion carries the owner's. Those approved records are not edited by Task 43.

**Remaining qualification.** The whole-branch review has run (above), and the final matrix's second pass ran every row on the source of `a5e45dc2f`: 30 rows passed on their first attempt and 2 after reruns, row 10 on the rerun the plan allows, row 15 on a third attempt after the allowed rerun also failed, by the controller's classification of upstream's `queue_retry` as a timing flake ("Where things stand", "Rulings"). The classification of row 15 rests on the ten-run sample after the matrix (2 failures in 10 alone, at a load of 20 to 30) and on the branch changing no SMTP file, not on a run on the base commit, and the failure's cause was not investigated; CI's SMTP step reruns a failed test by name once, so the pattern seen here (a failure in the whole run, then again alone) would fail that step, and that is where it shows. Not qualified by this plan: the reference client's checklist (`docs/zero-access/client-jmap.md`, its D29 rows included), the independent CalDAV client in either mode (so that leg of spec 5.1's proof of the move stays open until CI), the Docker suites (`directory`, `automation_tests`), the inventory and coverage gates with the cohort's new figure (FD-11), CI on the branch, the native-client rows of the manual checklist, other store backends, a second node, and the account page's text about how long keys stay in memory, which describes password clients only (spec 12, a follow-up outside this repository). Since this list was written, the branch was pushed at `dbd2fccc8` and pull request #90 opened on the project owner's word (2026-11-17); CI ran on `dbd2fccc8` and passed in every job that runs on a pull request (Check Style, Unit Tests, Test, Upstream Suites, Zero-access Test, Build Product, Independent CalDAV Client, Fresh OSS Store Restart, and the code-scanning analyses), while the retention gates run on manual dispatch only and were not dispatched. On that head the jobs that passed included the Docker-dependent `directory` suite (in "Test"), `automation_tests` (in "Upstream Suites") and the independent CalDAV client in both modes. The commits of the pull request's review were pushed on 2026-11-19 with their record (`dbd2fccc8..d6929c89e`), and the pull request's CI passed in every job on `d6929c89e`. On the owner's word of 2026-11-19 the Test workflow was then dispatched manually on `d6929c89e`, with the retention gates: its Upstream Suites job failed in the Automation Tests step, at upstream's DKIM selector assertion (`tests/src/automation/dkim.rs:89`: the key was generated a second after the test read the clock), while the same step passed in the pull request's run, a failure the controller classified as a timing failure of that test, seen once; the coverage net, the listing capture and the inventory gate passed; and the coverage gate failed ("The pull request's second review"). The commits of the second review and their record (`d6929c89e..68d905131`) were pushed on the owner's word of 2026-11-20: the pull request's CI passed in every job on `68d905131`, including the Docker suites and the CalDAV client in both modes; the manual run on that commit failed in its key-mode JMAP step, a failure the controller classified as timing, and its gates job had not finished when this record was written, so neither gate has a result after `d6929c89e` ("The pull request's third review and the second whole-branch pass"). Wave E's commits are not yet pushed, and no CI run exists for any commit after `68d905131`. The reference client's checklist has not run at all. Dispatching the gates and merging remain the owner's to authorize. A passing suite here is not review approval. After the pull request's review the code changed (head `25e098520`); the waves' own runs and the validation on the final source are under "The pull request's review", where its one failed row (key-mode `jmap_tests`, then two passing runs of the same command) is set out. After the second review the code changed again (head `0c73eafc4`, product code last changed at `2f00c2abf`): its validation ran the integration suites only under the coverage net's instrumented build, not in the plain test build, and its coverage figures are a local measurement that CI's gates job has not confirmed ("The pull request's second review"). After the third round the code changed again (head `6c5c46b26`, wave E): its validation ran nine rows locally, the za suite ran once in wave E, and the rest is listed as not run there ("The pull request's third review and the second whole-branch pass").
