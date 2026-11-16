# Test retention CI qualification for issue 86

The follow-up starts at `ade47a9ae`. The three coverage groups retain the original 19
configuration names and commands. A separate ordinary job captures the inventory. The
reporter requires validated receipts for the checked-out commit and the exact selected
instrumented build. The retention baseline, dispositions and gate thresholds are the
same inputs as on the serial workflow. [Issue #86](https://github.com/jaylaney/stalwart/issues/86)

## Baseline and transport trials

| Run | Instrumented build | Target bytes | Outcome |
| --- | ---: | ---: | --- |
| [Historical serial run](https://github.com/jaylaney/stalwart/actions/runs/43509717996/job/132176748499) | Included in 2h 39m coverage phase | — | Overall 3h 17m. Inventory failed on that historical head. |
| [Transport trial 1](https://github.com/jaylaney/stalwart/actions/runs/43744029033) | 20m 42s | 19,280,751,439 | Packaging stopped on a public AWS-LC header comment that matched the Basic credential pattern. |
| [Transport trial 2](https://github.com/jaylaney/stalwart/actions/runs/43746550649) | 17m 21s | 19,281,070,126 | Packaging stopped on type declarations in SQLite's generated bindings. |
| [Transport trial 3](https://github.com/jaylaney/stalwart/actions/runs/43748806097) | 14m 40s | 19,281,070,127 | Restored all expected coverage objects. Cargo rebuilt after finding three missing custom build-script executables. |
| [Transport trial 4](https://github.com/jaylaney/stalwart/actions/runs/43751500051) | 15m 58s | 19,281,070,346 | Both restored Cargo build shapes were fresh. The reporter reproduced the leak-test export and rejected the one-step matrix as incomplete. |

The first two transport trials compiled both build configurations. Their times describe build-only
work and provide no comparison with the serial gate's total. The local instrumented
fixture restored a small Cargo target into a fresh checkout at the same absolute path,
passed Cargo's freshness check and produced a fresh coverage export. The hosted probe
uses verbose instrumented test listings to check freshness. It clears listing counters
before collecting coverage because cargo-llvm-cov 0.9.1 rejects `--no-run` in this mode.

Trial 3 packed 4,512,244,449 bytes in 47 seconds. Upload took approximately 38 seconds.
The coverage runner downloaded the artifact in 46 seconds, unpacked it and checked the
object set in 57 seconds, and verified toolchain and object identity after dependency
fetch in 18 seconds. The producer had at least 65,699,819,520 free bytes during packing,
sampled every two seconds.

The Cargo freshness check identified missing `build_script_main-*` executables in two
AWS-LC versions and OpenSSL. All other dirty units depended on those crates. The corrected
file list preserves each executable, its dep-info, and its `build-script-main` hardlink.
A local Cargo fixture with a custom build script restored all three byte for byte and
reported the build fresh. [Trial 3 freshness log](https://github.com/jaylaney/stalwart/actions/runs/43748806097/job/132922845186)

Trial 4 packed 4,517,130,237 bytes in 41 seconds and uploaded them in 23 seconds. The
coverage runner downloaded the archive in 6m 7s, unpacked it in 76 seconds, and checked
both build shapes in under two seconds with zero compilation. The existing leak test
and export took seven seconds. Minimum free space during the two-second sampling was
69,132,685,312 bytes while packing and 70,227,390,464 bytes while unpacking.
The reporter downloaded the build in 5m 19s, unpacked it in 77 seconds with at least
70,237,106,176 free bytes, and re-exported the fresh leak profile in 14 seconds. Both
consumers verified the restored toolchain and report-object hashes.

A [hosted artifact fixture](https://github.com/jaylaney/stalwart/actions/runs/43746694042)
validated evidence from a failed reusable shard and kept its first report red. On
**Re-run failed jobs**, the second report validated fresh shard evidence with the original
build from attempt 1 and passed. This exercises GitHub's artifact and job-output behavior
with a small fixture.

## Repository cache budget

The Actions API returned a 10 GB storage limit and 10,725,606,288 bytes in 12 entries when
the work began. The new production keys cover dependency downloads and are scoped by lane.
The cold comparison skipped saves when the workflow token received HTTP 403 from the
cache storage-limit endpoint. A separate hosted check identified the endpoint and read
12,433,401,138 bytes from the usage endpoint with the same workflow permissions. The
scheduled producers use live usage and the verified 10 GB budget before a save.
Qualification of warm restores depends on observing both scoped writes and corresponding
hits. [Workflow token check](https://github.com/jaylaney/stalwart/actions/runs/43754170815)
The [updated guard](https://github.com/jaylaney/stalwart/actions/runs/43754588999)
read the same live usage and skipped because there was insufficient room.

The existing ordinary unit-test cache restored an exact key in 41 seconds in its original
Ubuntu 26.04 layout. Its 1,994,370,095-byte archive added 7,771,141,421 apparent bytes after
extraction. Both hosts used Rust 1.99.1. The Ubuntu 26.04 restore runner reported GCC 15.2,
binutils 2.46 and glibc 2.43. The retention runner reported GCC 13.3, binutils 2.42 and
glibc 2.39. Ordinary listing jobs use the dependency downloads and compile on Ubuntu 24.04.
[Ordinary cache probe](https://github.com/jaylaney/stalwart/actions/runs/43752266663/job/132928606509)

## Same-source qualification

The retention jobs run on the weekly schedule and manual dispatch. This record tracks
the production run and the disposable same-source comparisons.

The [first same-commit comparison](https://github.com/jaylaney/stalwart/actions/runs/43752266663)
used `4d5fee1930338b494b0c148aa0252bcf4ab36eb1`. Cold listing capture finished in 35m 15s.
All 17 short-shard test commands passed. The shard stayed red because the workspace unit
test `dav_proto::requests::tests::parse_requests` rewrote tracked JSON fixtures and the
next WebDAV command rebuilt coverage objects. The test now writes to a private
temporary directory and still exercises parsing, serialization and file writes.
[Short-shard result](https://github.com/jaylaney/stalwart/actions/runs/43752266663/job/132933900954)
Replaying that snapshot on another hosted runner found 47 fixture timestamp changes and
26 content changes. Cargo named `crates/dav-proto/resources/requests/acl-001.json` as the
build input that made `common` dirty. [Fingerprint replay](https://github.com/jaylaney/stalwart/actions/runs/43757223934/job/132943843746)
The isolated locked unit test confirmed all 94 tracked request fixtures retained their
contents and timestamps. The first comparison was stopped after its diagnostics were saved.

The first [production dispatch](https://github.com/jaylaney/stalwart/actions/runs/43754819285)
and [intentional-failure trial](https://github.com/jaylaney/stalwart/actions/runs/43754934544)
used the earlier unit test. Both were cancelled before the retention report so that
validation can run with the corrected fixture writer.

The replacement [production dispatch](https://github.com/jaylaney/stalwart/actions/runs/43757776610)
uses `3291c73264e1372e255e01cb9cf55bcabb98ef8c`. The corrected
[comparison](https://github.com/jaylaney/stalwart/actions/runs/43757810802) and
[intentional-failure trial](https://github.com/jaylaney/stalwart/actions/runs/43757835753)
use that source with disposable harness files. The comparison runs the warm lane when
both cold producers save their scoped downloads, and checks key invalidation separately.

The corrected short shard passed all 17 configurations and validated the original object
hashes in both runs. The measured driver step took 18m 8s in production and 21m 45s in
the comparison. [Production short shard](https://github.com/jaylaney/stalwart/actions/runs/43757776610/job/132950855150) ·
[Comparison short shard](https://github.com/jaylaney/stalwart/actions/runs/43757810802/job/132950870859)

Both cold producers missed their scoped keys and measured 9,941,331,255 bytes of live
usage. The roughly 59 MB of headroom could not cover both downloads, about 383 MB each
before compression, so both saves and the warm lane were skipped. The disposable
lockfile change generated new keys that missed in both lanes. The original keys were
also cold. [Cache outcomes](https://github.com/jaylaney/stalwart/actions/runs/43757810802/job/132956473426) ·
[Key invalidation check](https://github.com/jaylaney/stalwart/actions/runs/43757810802/job/132956473002)

All three comparison coverage producers passed. The reporter rejected the ordinary
listing at collection because the credential sanitizer had changed valid Rust test IDs
such as `tokenizers::…: test`. The corrected sanitizer preserves structured libtest rows
within Cargo's listing output and continues to redact credentials in diagnostic text.
The selected and ignored listing fixtures cover both behaviors.
[Reporter rejection](https://github.com/jaylaney/stalwart/actions/runs/43757810802/job/132962819014)

The [same-source replay](https://github.com/jaylaney/stalwart/actions/runs/43764661867)
recaptures ordinary listings on `9b966c97ff291fbc45aa1eeca888a6d2f6186e03` using the
corrected sanitizer. It validates the original build and three coverage artifacts by
their exact IDs and applies the stock gates to those profiles and the fresh listings.
The replay binds the original run identity for this comparison. The separate
[production dispatch](https://github.com/jaylaney/stalwart/actions/runs/43764565917)
uses `4528166c26ecca948beeb4ed8a7b5a4f1cd039c7` and exercises production's same-run wiring.
The [updated deliberate failure trial](https://github.com/jaylaney/stalwart/actions/runs/43764608298)
uses the corrected sanitizer as well.

The replay collected all 19 profiles and fresh listings with no missing inputs. Inventory
and coverage both passed. The full cohort covered 23,989 of 24,843 lines (96.56%) and the
runtime cohort covered 13,258 of 13,973 (94.88%). Both totals match the first serial
coverage run. [Replay gates](https://github.com/jaylaney/stalwart/actions/runs/43764661867/job/132977601729)

The [first serial coverage run](https://github.com/jaylaney/stalwart/actions/runs/43757810802/job/132945772920)
completed all 19 profiles in 109m 23s. The disposable serial harness failed to provision
the separate ordinary target directory. Listing capture exited 101 and its inventory
result is incomplete. The [corrected serial reference](https://github.com/jaylaney/stalwart/actions/runs/43769041749)
provisioned both targets. It was superseded by the [current serial reference](https://github.com/jaylaney/stalwart/actions/runs/43771740748),
which also disables index refresh during its `git diff` checks. `GIT_OPTIONAL_LOCKS=0`
alone did not stop `git diff` from rewriting the watched index in the comparison harness.
The [reporter-only replay](https://github.com/jaylaney/stalwart/actions/runs/43771719591)
uses the same read-only Git check and the original validated artifacts.

The final serial and reporter-only replay both passed the stock gates on `9b966c97`.
They matched all 19 instrumented selections, all 28 ordinary and 28 ignored-only listings,
and all 4,143 required execution decisions, including the applied dispositions. The
inventory gate checked 795 tests in 29 configurations. Every executable-line denominator
across 1,978 files matched. The coverage totals and individual reports for all 53 files in
the enforced cohort matched. The replay confirmed that its build object hashes stayed
unchanged through selector re-enumeration. [Final serial](https://github.com/jaylaney/stalwart/actions/runs/43771740748/job/132988936013) ·
[Final replay](https://github.com/jaylaney/stalwart/actions/runs/43771719591/job/132988868050)

Covered-line sets differed in 34 files outside the enforced cohort, with 208 serial-only
and 39 parallel-only lines. Raw records differed for 255 of 3,159 function identities.
The only different `executed` flag was the optional SMTP `TestQueueEvent::is_refresh`
helper, and every stock execution decision matched. The serial net retried only its two
failing SMTP tests by name. The parallel net retried JMAP plain after a thread-merge
assertion and automation after a one-second DKIM selector mismatch. All retries passed.
The merged profiles preserve evidence from both attempts.
[Parallel short-shard diagnostics](https://github.com/jaylaney/stalwart/actions/runs/43757810802/job/132950870859) ·
[Serial retry](https://github.com/jaylaney/stalwart/actions/runs/43771740748/job/132988936013)

## Serial and production retention timing

The production retention check passed on `4528166c26ecca948beeb4ed8a7b5a4f1cd039c7`.
All 19 configurations, evidence collection, merged coverage, executed functions and
both stock gates passed. The workflow finished green across all 14 jobs.
[Production retention result](https://github.com/jaylaney/stalwart/actions/runs/43764565917/job/132986339388)

| Cold run | Wall time through the passing gates | Runner time through the passing gates | Total runner time |
| --- | ---: | ---: | ---: |
| Serial reference, one job | 162m 24s | 162m 24s | 163m 01s |
| Production retention, six jobs | 78m 08s | 174m 20s | 174m 22s |

The observed production run reduced retention wall time by 84m 16s (52%) and added
11m 56s (7%) of runner time through the same endpoint. The serial reference used the
same source as the replay. The independently measured production run used the final PR
implementation with the listing sanitizer fix. The historical 3h 17m run is a separate
observation.

| Metric | Observed |
| --- | ---: |
| Manual dispatch to passing check, including style | 78m 38s |
| Style runner duration | 24s |
| Observed job start gaps after dependencies completed | 3–4s each |
| Instrumented compilation, pack and upload | 15m 56s, 42s, 27s |
| Ordinary listing capture | 25m 32s |
| Short, ZA filesystem and ZA RocksDB driver steps | 18m 04s, 41m 20s, 43m 00s |
| Build artifact and reporter expanded target | 4,517,418,610 and 19,281,127,901 bytes |
| Four build downloads, summed | 18.07 GB, 9m 26s |
| Four target restores, summed | 4m 55s |
| Reporter collection, merge and executed-function timing | 2m 55s, 35s, 2m 24s |

Serial coverage took 125m 20s and ordinary listing capture took 33m 45s. Its runner started
three seconds after dispatch. After both phases it held 19.28 GB of instrumented target
files and 26.15 GB of ordinary target files, with 39.46 GB free in the phase sample.
The production reporter had 58 GiB free after expansion and dependency fetch. Trial 4
provides the two-second disk samples around packing and unpacking.

The [serial reproduction](../../../scripts/tests/README.md#serial-reproduction) uses the
pinned toolchain, an empty ordinary target and the existing gate commands. The build and
evidence artifacts last one day. Run all jobs when a partial rerun would reference expired
artifacts. Sanitized diagnostic receipts last seven days.

## Failed producer and partial rerun

The intentional short-shard failure happened after its full evidence upload. The reporter
collected and merged all inputs and passed both stock gates. Its final check stayed red
because the short producer reported failure. [First-attempt reporter](https://github.com/jaylaney/stalwart/actions/runs/43764608298/job/132984542125)

The failed-jobs-only rerun passed. It selected short receipt `14185582009` from attempt 2
and reused build `14182182574`, ZA RocksDB `14183619195`, ZA filesystem `14183458378`,
and listings `14182837161` from attempt 1. In the replacement short shard, the first SMTP
attempt failed on `smtp::queue::manager::queue_due`. The second command selected that one
test with `--exact` and passed. [Second-attempt reporter](https://github.com/jaylaney/stalwart/actions/runs/43764608298/job/132996956237) ·
[Replacement short shard](https://github.com/jaylaney/stalwart/actions/runs/43764608298/job/132988240333)

The disposable replay also rejected a missing listing receipt, incompatible object set,
and mismatched artifact ID, each with exit 2 and no gate inputs. Its four hosted fixtures
passed for work cutoff, profile settlement and diagnostic upload. [Invalid evidence checks](https://github.com/jaylaney/stalwart/actions/runs/43764661867/job/132977601729)
