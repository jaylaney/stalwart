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
| [Transport trial 3](https://github.com/jaylaney/stalwart/actions/runs/43748806097) | Pending | — | Tests the corrected credential checks and target-local restore. |

The first two transport trials compiled both build configurations. Their times describe build-only
work and provide no comparison with the serial gate's total. The local instrumented
fixture restored a small Cargo target into a fresh checkout at the same absolute path,
passed Cargo's freshness check and produced a fresh coverage export. The hosted probe
uses verbose instrumented test listings to check freshness. It clears listing counters
before collecting coverage because cargo-llvm-cov 0.9.1 rejects `--no-run` in this mode.

A [hosted artifact fixture](https://github.com/jaylaney/stalwart/actions/runs/43746694042)
validated evidence from a failed reusable shard and kept its first report red. On
**Re-run failed jobs**, the second report validated fresh shard evidence with the original
build from attempt 1 and passed. This exercises GitHub's artifact and job-output behavior
with a small fixture.

## Repository cache budget

The Actions API returned a 10 GB storage limit and 10,725,606,288 bytes in 12 entries when
the work began. The new production keys cover dependency downloads and are scoped by lane.
The scheduled producers check available room before a save. Qualification of warm restores
depends on observing both scoped writes and corresponding hits.

## Pending hosted qualification

The final measurements need side-by-side serial and distributed runs on the same source,
including selected tests, executed-function evidence and merged coverage. Record cold and
warm cache results, invalidation, wall time, summed runner time, transfer and disk use, and
the results of negative paths. The retention jobs are gated to the weekly schedule and
manual dispatch.
