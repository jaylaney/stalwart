# Test Retention CI Parallelization plan review

Date: 2026-11-14. Reviewed: `docs/superpowers/plans/2026-11-14-test-retention-ci-parallelization.md`
(untracked in the main checkout, 91 lines) against `main` at `cc8b50a9b`, issue #86,
`scripts/tests/`, the `gates` job of `.github/workflows/test.yml`,
`crates/common/build_support/release.rs`, and the cargo-llvm-cov 0.9.1 source
(`~/.cargo/registry/src/*/cargo-llvm-cov-0.9.1`). Timings are taken from the `gates-receipt`
artifacts of runs 43509717996 (failed, `ef6e04705`) and 43621127921 (passed, `82b8984aa`, which
differs from `cc8b50a9b` only in `tests/caldav_client/requirements.txt`).

The plan covers every acceptance criterion in #86. Its fail-closed aggregation and its
single-object-set rule are sound. The pinned cargo-llvm-cov source confirms the plan's premise:
`--no-report` implies `--no-clean` (`cli.rs:1591`), so step commands never delete build
artifacts, and the report discovers objects by walking the target tree. No step command
changes features or flags relative to `build-A` and `build-B` (`coverage_net.py --list`), so
a correctly restored snapshot should compile nothing in a shard.

Two findings block the plan as written. F1: Cargo's freshness check makes the snapshot stale
in every fresh checkout, which the plan treats as only a possibility. F2: the object inventory
omits the property the 0.9.1 report uses to discover objects. Two more set the plan's shape:
the measured step times do not support five shards (F3), and the task order puts the hosted
probe before the workflow check that would let it run (F4).

## Summary

| ID | Severity | Topic | Plan location |
|---|---|---|---|
| F1 | Blocking | Snapshot is always stale after checkout (mtime fingerprints, `release.rs` Git watches) | Task 2, probe bullet |
| F2 | Blocking | Object inventory must cover mode bits and the 0.9.1 discovery rule | Task 1 interfaces, Task 2 |
| F3 | Important | Shard table does not match measured times; current timing data exists and expires 2026-11-21 | Matrix table, Spec paragraph |
| F4 | Important | Hosted probe cannot run before `ci_steps` accepts the new wiring | Task 2 vs Task 4 ordering |
| F5 | Important | Compiled-target cache: commit-exact keys never hit weekly; repo cache quota | Task 2 |
| F6 | Important | Evidence upload on job timeout; za shard cannot fit two 3 h attempts | Task 4 |
| F7 | Important | Toolchain unpinned; runner image skew within one run | Task 2, Task 5 |
| F8 | Important | Every hosted dispatch runs the whole workflow; fault-injection mechanism unspecified | Task 5 |
| F9 | Minor | Reporter needs the full snapshot too; per-shard snapshots cannot be split | Task 2 fallback, Task 3 |
| F10 | Minor | Consumers need Cargo package sources before freshness is decided | Task 2 |
| F11 | Minor | Python stdlib compression is too slow for the snapshot | Task 2 |
| F12 | Minor | Uploading profiles reverses the receipt's "Profiles are not uploaded" policy (public repo) | Task 3 |
| F13 | Minor | `unit-tests` profile includes the skip-list listing run; keep that order | Task 1 |
| F14 | Minor | `JOB_FORBIDDEN` holds `strategy`; relax it per job, not globally | Task 4 |
| F15 | Minor | Artifact name collisions; "Re-run failed jobs" case; fate of the serial job | Task 4, Task 5 |

## F1 Blocking: the snapshot is stale in every fresh checkout

Plan location: Task 2, third bullet ("If checkout timestamps alone invalidate the snapshot,
normalize tracked source times consistently ... and repeat the probe").

This invalidation is certain, not contingent. Cargo decides whether a path package is fresh by
comparing its dep-info output's mtime with the mtime of every input it lists. `actions/checkout`
writes every source file at checkout time, which is later than the snapshot's outputs, so every
workspace crate rebuilds in every shard. Shard mode would then either recompile for about 18
minutes (the passing run's `build-A` plus `build-B` took 852 s + 231 s) or trip the object fence.

Normalizing tracked source times is not enough. `common`'s build script
(`crates/common/build_support/release.rs:121-140`) emits `rerun-if-changed` for every
`git ls-files` path and also for `.git/HEAD`, `.git/index`, `.git/packed-refs` and `.git/refs`.
Those four Git paths are new in each checkout. A rerun of that build script rebuilds `common`
and everything that depends on it, which is almost the whole workspace. The script also runs
`git status` (`:141`), which can refresh and rewrite `.git/index` whenever file stat data
changed, for example after an mtime normalization.

Recommendation: make this a designed step of Task 2 rather than a fallback. Pick one approach:

- After the last Git command in the producer, every shard and the reporter, set the mtime of
  every `git ls-files` path and of the four Git paths to one fixed value, the commit time. Run
  `git update-index --refresh` before the normalization, so the build script's `git status`
  finds nothing to rewrite.
- Build and run from a `git archive` export at the same absolute path in every job, with
  `STALWART_SOURCE_COMMIT` set. `release.rs` then watches only `fork-release.toml` and
  `crates/main/Cargo.toml`. The embedded identity then reads `provided` instead of `clean`.
  That string does not affect line coverage, because build scripts are outside the report,
  but record it in the serial comparison.

In either case, add a cheap shard-side check alongside the digest fence: fail the shard if any
attempt log contains a Cargo `Compiling` line. A rebuild then fails with its cause named,
before anyone has to read a digest difference.

## F2 Blocking: the object inventory must record what 0.9.1 uses to discover objects

Plan location: Task 1 interfaces ("a sorted inventory of report-object paths and SHA-256
digests"); Task 2 probe ("compressed tar that preserves modes").

In 0.9.1, `object_files` (`report.rs:240-480`) walks `<target>/llvm-cov-target/debug`. It skips
`incremental`, `.fingerprint`, build-script output and build-script binaries, and keeps a
regular file only when it is executable (`mode & 0o111`, `report.rs:346`) and its stem matches
`^(lib)?(<workspace package or target name>)(-[0-9a-f]+)?$` (`pkg_hash_re`, `report.rs:578`).

An inventory of path and digest alone cannot see a lost executable bit. That loss is a real
risk, because `actions/upload-artifact` drops modes, which is why `product-build` already
tars its binary. A shard or reporter whose objects lost the bit would report on fewer objects:
functions disappear from the exports, and the denominators move in either direction, while
every digest still matches.

Recommendation:

- Record mode, size and SHA-256 for every file the 0.9.1 rule selects, applying the rule
  verbatim, including the name regex built from `cargo metadata` workspace members and
  targets.
- Separately record path, mode and size for every other file under `llvm-cov-target`, so an
  extra stale executable that matches the regex is caught.
- If `cargo llvm-cov report -v` prints the `llvm-cov export` invocation with its `-object`
  arguments (check this in the probe), capture it in `<step>.export.log` and compare that
  actual list with the manifest. That answers Task 1's "check the actual object discovery used
  by 0.9.1" directly.
- Hashing the full inventory before and after each shard is expensive at this size. Time it in
  the probe and hash only the selected objects.

## F3 Important: the shard table is not supported by the measured times

Plan location: "Use one initial matrix with five shard IDs ... pending per-step timing"; Spec
paragraph (cites the failed run only).

Per-step timing already exists. The `steps.tsv` in each run's `gates-receipt` records every
attempt's wall time. Passing run 43621127921 at `82b8984aa`, effectively the plan's starting
point:

| Group | Wall time |
|---|---|
| Install cargo-llvm-cov | 61 s |
| `build-A` + `build-B` | 852 s + 231 s (18 m) |
| `za-tests` | 2562 s (43 m) |
| `za-tests-fs-blob` | 2587 s (43 m) |
| All other 17 steps together (`unit-tests` 226 s, `search-tests` 280 s, `jmap-key` 230 s, `jmap-plain` 170 s, the rest under 75 s each) | 1257 s (21 m) |
| Exports (net step total minus attempt walls) | about 2.6 m |
| Listing capture | 29 m |
| Executed functions + both gates | about 1.5 m |
| Whole job | 2 h 39 m |

The failed run 43509717996 has the same shape: za steps 56 m and 53 m, the rest about 24 m,
builds 22 m.

The critical path is install, build, pack and upload, download and unpack, `za-tests`, then the
reporter. Splitting the remaining 21 minutes into `unit`, `protocols` and `services` shortens
nothing, yet each extra shard pays a full snapshot download and unpack, a cargo-llvm-cov
install, and its share of the inventory hashing.

Recommendation:

- Start with three shards: `za-rocks`, `za-filesystem`, and one `rest` shard holding the other
  17 steps in `ORDER`.
- Consider letting the producer run `rest` itself after it uploads the snapshot. That saves one
  transfer, and its fence runs against the producer's own tree.
- The expected best case is about 66 m plus one pack, two unpacks (shard and reporter) and the
  uploads, against 159 m serial. State that bound in the plan, so the transport budget is
  explicit: the change stops paying off once transfer and unpack overhead exceeds about 45
  minutes per path.
- Cite run 43621127921 in the Spec paragraph as the passing serial reference, alongside the
  failed run.
- Copy both runs' `steps.tsv` into the qualification record before their artifacts expire on
  2026-11-21.

## F4 Important: the hosted probe cannot run before the workflow check accepts it

Plan location: Task 2's hosted probe and Task 4's `check_workflow` update. Task 2 comes first.

Every gates path is locked to the current serial shape:

- `gates` has `needs: style`.
- `style`'s Check Gates step runs `ci_steps.check_workflow`, which requires the `gates` job's
  `if` to equal `GATES_WHEN` exactly, forbids `strategy` on it (`JOB_FORBIDDEN`,
  `ci_steps.py:84`), and requires the two exact `GATES_COMMANDS` (`:69-73`).
- `coverage_net.py` and `capture_listings.py` both call `refuse_diverged_workflow`.

So a probe wired into `test.yml` fails `style`, which skips every gates job, and the scripts
refuse to run anyway. A probe in a new workflow file cannot be dispatched until that file
exists on the default branch.

Recommendation: state the sequencing. Either land Task 4's check changes together with the
probe wiring on a disposable branch, or keep the probe inside `test.yml` behind a dispatch
input that the updated check recognizes.

## F5 Important: compiled-target cache scope and quota

Plan location: Task 2 ("Include the exact source commit in compiled-target keys", "Allow only
exact-key compiled target restores").

`release.rs` embeds the commit, and keys carry it, so a compiled-target cache hits only on a
rerun or dispatch of the same commit. Scheduled weekly runs land on a new commit and never
hit. Meanwhile each run would save an instrumented target that is probably tens of GB (its size
is unmeasured). GitHub's default repository cache quota is 10 GB with least-recently-used
eviction, and five jobs in `test.yml` already use `Swatinem/rust-cache`, so this cache would
evict theirs or fail to save. Task 5's "warm" run on the same commit would therefore overstate
the weekly benefit.

Third-party crates are never report objects, because `pkg_hash_re` matches only workspace
package and target names. A cache of compiled third-party dependencies, keyed by toolchain,
lock file, features and flags but not by commit, is therefore safe for the object set and gives
real reuse across commits.

Recommendation:

- Measure the target's size first.
- Cache downloads plus compiled third-party dependencies; never cache workspace crates.
- Treat "new commit, warm dependencies" as the representative measurement, and the same-commit
  warm run as a secondary one.

## F6 Important: evidence on job timeout, and the 6 h hosted cap

Plan location: Task 4 ("stages and uploads available evidence after failures using
`!cancelled()`").

A job that reaches `timeout-minutes` is cancelled, and `!cancelled()` steps are then skipped,
probably even when they have not started; `always()` steps still run. Confirm this in the
probe. `TIMEOUTS` gives each za step 3 h per attempt (`coverage_net.py:64`), so a first
attempt that times out leaves its retry no room under the 360-minute hosted job cap. The job
times out, and the shard's diagnostics are never uploaded. Today's serial job has the same
exposure under its 330-minute limit, but the plan explicitly promises downloadable receipts
after failures.

Recommendation: set each shard's `timeout-minutes` explicitly and record the za shard's
worst case. Run the evidence stage and upload under `always()`; the `gates` job's success
requirement keeps the gate red. If neither is acceptable, state the gap as accepted.

## F7 Important: unpinned toolchain and runner image skew

Plan location: Task 2 ("Match the runner/toolchain identity before unpacking"); Task 5.

There is no `rust-toolchain` file, and the gates job uses whatever stable toolchain the runner
image ships (`rustup component add llvm-tools-preview` only). GitHub rolls out new images
gradually, so the producer and the shards of one run can land on different images. A rustc
version change makes Cargo rebuild everything. The identity check catches this, but only as a
spurious red run.

Recommendation: install one exact toolchain and its `llvm-tools` in every gates job,
including the serial reference job, so the comparison stays like for like. Consider caching
the cargo-llvm-cov binary instead of compiling it in each job (61 s per job).

## F8 Important: cost and mechanics of the hosted qualification

Plan location: Task 5.

`workflow_dispatch` runs every job in `test.yml`: `test`, `unit-tests`, `upstream-suites`,
`zero-access` (240-minute timeout), `product-build`, `caldav-client`, `fresh-store` and the
gates. Task 5 needs at least five dispatches: cold, warm, a build-identity change, a failing
shard and a mismatched artifact, plus the serial comparison and any reruns. Those other jobs
also compete for concurrent runners, which distorts the queue and critical-path figures Task
5 records.

The plan also does not say how a failing shard or a mismatched artifact is induced on hosted
runners. A permanent dispatch input that breaks a shard or swaps an artifact would itself be a
way to weaken the gate.

Recommendation:

- Budget the runner minutes, or add a dispatch input that skips the non-gates jobs. Those jobs'
  `if` conditions are pinned by `ci_steps`, so that input would need to go through the check.
- Induce the negative cases with disposable commits on a throwaway branch.
- Have `check_workflow` reject any fault-injection input in the qualified workflow.

## F9 Minor: the reporter needs the full snapshot, and the snapshot cannot be split per shard

Plan location: Task 2 ("If the snapshot is too expensive, measure a smaller object or build
transfer"); Task 3 (`--merge-only`).

`--merge-only` reuses `Net.export`, which runs `cargo llvm-cov report` against the target tree,
so the `gates` job must also download and unpack the snapshot and install cargo-llvm-cov and
`llvm-tools`. That is a third transfer, and it sits on the critical path. State it and time it.

A smaller snapshot also has a limit. Each serial per-step export saw the objects of both builds,
and a step's profile can hold counters for any binary it executed. A shard that held only
`build-B`'s objects, for example, would silently lose those counters in its per-step exports,
which `strict_rule.py functions` reads. A reduction may drop only files that are neither report
objects nor needed by Cargo to decide freshness.

## F10 Minor: consumers need Cargo's package sources

Cargo resolves the unit graph, and downloads any missing packages, before it decides
freshness, and cargo-llvm-cov runs `cargo metadata` as well. So every shard and the reporter
need the downloads cache restored, or network access. The plan saves caches only from the
owning build job; state that consumers restore the downloads scope. If changing the shared
configuration is acceptable, `CARGO_NET_OFFLINE=true` in the consumer jobs would turn
dependency drift into an error rather than a silent fetch.

## F11 Minor: snapshot compression

Plan location: Task 2 (`pack-build`, "Python 3.9+ stdlib").

At Python 3.9, `tarfile` compresses only with gzip, bzip2 or xz, all single-threaded, which is
roughly ten minutes or more each way at tens of GB. Have `retention_artifacts.py` call the
runner's `tar` with `zstd -T0` if the image ships zstd (check in the probe), set
`compression-level: 0` on `upload-artifact` for the already compressed archive, and use
`retention-days: 1` for the snapshot and the per-shard artifacts.

## F12 Minor: profiles and exports become public artifacts

Plan location: Task 3 ("Stage the per-step region JSON and indexed profile for each available
step").

The current receipt deliberately excludes profiles (`test.yml:674`: "Profiles are not
uploaded"), and `jaylaney/stalwart` is public. Its run artifacts, including the instrumented
snapshot, can be downloaded by any signed-in GitHub user. The content is probably harmless
(source paths, counters, and binaries built from public source), but the plan should record
the policy change, restrict the 7-day retention to the final sanitized receipt, and keep the
intermediate artifacts at one day.

## F13 Minor: keep the `unit-tests` skip-list listing inside the step's profile window

Plan location: Task 1 ("Reuse `Net.step`, `Net.attempt` and `Net.export`").

`Net.step` clears profiles (`coverage_net.py:221`) and then calls `tests_crate_skips()`
(`:225`). That runs the instrumented `-p tests -- --list`, and the profraw it writes is merged
into `unit-tests` attempt 1. A refactor that moves the skip computation into build-only mode,
or before the clear, changes that step's profile relative to the serial net. Add a test that
pins the order.

## F14 Minor: relax `JOB_FORBIDDEN` per job

Plan location: Task 4 ("Update `ci_steps.check_workflow` ... matrix membership, fail-fast").

`strategy` is in `JOB_FORBIDDEN`, which applies to the gated ordinary jobs as well. Allow it
only on `gates-coverage`, with an exact expected value (`fail-fast: false`, and `matrix.shard`
equal to the keys of `COVERAGE_SHARDS`, with no `include` or `exclude`). Removing it from the
global set would let a gated ordinary job gain a matrix. The new `gates` job's `if`, which
combines `!cancelled()` with the `needs.*.result` checks, replaces `GATES_WHEN` and needs the
same exact match.

## F15 Minor: artifact names, partial reruns, the serial job's future

- If the serial reference job and the distributed `gates` job both upload `gates-receipt`,
  `upload-artifact` refuses the second name in the same run. Give them distinct names.
- After "Re-run failed jobs", the `gates` job runs with a new `run_attempt` while successful
  producers keep artifacts from the earlier attempt, so the collector must reject them. That is
  the intended fail-closed behaviour. Add it as an explicit offline and hosted case, and say so
  in the README's rerun procedure.
- State whether `retention_compare_serial` and the serial job stay after qualification. They
  cost nothing while off, and keeping them preserves a like-for-like reference for later
  upstream merges.

## Checked and found consistent

- The cohort floors 17821/18490 and 10646/11246 match `check_coverage.py:21-22`.
- The shard table covers the 19 baseline names exactly once. The two builds and `ORDER` match
  `coverage_net.py:60`.
- SMTP's retry-by-name, the no-names failure and the use of profiles from both attempts match
  `Net.step` (`coverage_net.py:219-250`).
- Both gates pass at the plan's starting point. Run 43621127921 reported inventory `OK` after
  the disposition repair in `7e09e1922`, and coverage full 23989/24843 and runtime 13258/13973,
  both passing. Task 5's requirement that the two stock gates pass therefore has no known
  blocker.
- No step command differs from the builds in features or flags, so a correctly restored and
  fresh snapshot needs no compilation in a shard, which is what the object fence assumes.
