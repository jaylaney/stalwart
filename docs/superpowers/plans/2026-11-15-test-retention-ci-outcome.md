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

## Pending hosted qualification

The final measurements need side-by-side serial and distributed runs on the same source,
including selected tests, executed-function evidence and merged coverage. Record cold and
warm cache results, invalidation, wall time, summed runner time, transfer and disk use, and
the results of negative paths. The retention jobs are gated to the weekly schedule and
manual dispatch.

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
