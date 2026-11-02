# Fork changelog

This file records releases of the independent zero-access fork. Versioning, provenance,
compatibility, and release qualification follow [the release policy](docs/releases.md).
The inherited [CHANGELOG.md](CHANGELOG.md) records upstream Stalwart history.

## Unreleased

## 0.2.0-beta.1 — 2026-11-02

Immutable tag: `fork-v0.2.0-beta.1`. A limited prerelease on upstream Stalwart `v1.0.1`,
published as a GitHub prerelease. It claims no general client or production readiness until
its pending gates (below) pass.

### Provenance

- Fork source: the commit the tag `fork-v0.2.0-beta.1` names, resolved by
  `git rev-parse 'fork-v0.2.0-beta.1^{commit}'`. The published release notes and shipped
  `stalwart.provenance.json` record the exact full commit.
- Fully integrated upstream base: Stalwart `v1.0.1`, commit
  `23b69755cb3cd8ecd952fe0443c5ac067a506184`
  ([release notes](https://github.com/stalwartlabs/stalwart/releases/tag/v1.0.1)), fetched
  by URL and pinned in `fork-release.toml` and in the verifier's `PINNED_UPSTREAM_TAGS`.
- No additional upstream backports are recorded separately for this preview.
- The published `fork-v0.1.0-beta.1` and `fork-v0.1.1-beta.1` tags and artifacts retain their
  original identity.

### Changes

- Moves the fork from upstream `v0.16.27` to upstream `v1.0.1`. Every zero-access feature is
  carried over: key accounts, sealed calendars, invitations between key accounts, key
  pinning, calendar sharing and key-account mail at rest. Ordinary accounts take 1.0.1's
  behaviour.
- Field policy 2: an event's, task's or journal's `CLASS` is visible to the operator when its
  value is PUBLIC, PRIVATE or CONFIDENTIAL, so that sharers can hide events; any other value
  stays sealed.
- 1.0's object metadata (DAV dead properties, IMAP annotations, JMAP metadata) is refused for
  key accounts, and setup refuses an account that already holds some with 409 "account
  already holds object metadata".
- 1.0 indexes in a queue of its own. Setup refuses an account with queued index work (409
  "account has unfinished tasks"); after a transient failure an administrator resets the
  partition with `x:IndexQueueStatus/set`, and after a permanent one deletes the offending
  document or the account. A key account's mail search document now holds only its ids.
- Every account: an undecodable Bearer value is no longer quoted in authentication traces,
  and HTTP request traces print `[redacted]` for `Authorization`, `Proxy-Authorization` and
  `Cookie` headers. Every ordinary calendar write asserts that the account was not
  provisioned in the meantime, as ordinary mail writes already did.
- Setting up a key account revokes OAuth grants minted before it, and a key account refuses
  a grant token minted while it had no credential. The scheduling inbox's expiry no longer
  removes invitations the drain has not yet opened.
- A key account's sealed events and calendars use about 485 and 491 bytes more quota each
  than their plaintext, which 1.0 enforces at commit.
- Inherited from upstream 1.0.1 for ordinary accounts: a delegate is not invited when the
  delegator forwards the REQUEST (1.0 accepts a REQUEST only from the organizer), and
  calendar text search sees an event only once the asynchronous index has caught up. Key
  accounts' delegates are still invited, by the drain.
- The shipping binary's `--version` and help name the fork version, the upstream tag and
  commit, and the source commit, for example `Stalwart Server fork v0.2.0-beta.1 (upstream
  v1.0.1 23b69755cb3cd8ecd952fe0443c5ac067a506184; source <commit>; clean)`. The release
  verifier also checks that the upstream pin names its tag's commit, is an ancestor of the
  source, is the latest known upstream release in the source's history, and matches the
  internal crates' upstream version.

### Compatibility and upgrade instructions

Fresh stores only. Upstream 1.0.1 cannot open a store written by upstream 0.16 or by the
fork's `0.1.x` releases, and the fork adds no conversion: the server stamps a new store with
its schema version and edition and refuses any other. Stores from `0.1.x` are not
supported. Moving an installation from `0.1.x` means a rebuild on a fresh store: create the
accounts again, issue new setup tokens and enrol each key account again (new keys), and reload data through the clients. Keep the pre-cutover store
snapshot and the `0.1.x` binary for rollback; a rollback restores the old store as it was,
without anything written after the cutover. Back up the complete database, blob store and
configuration and verify restoration before any test.

The planned shipping configuration is Linux `x86_64-unknown-linux-gnu` with RocksDB,
without the enterprise and `test_mode` features, and requires an `x86-64-v2` CPU. Check the
artifact's provenance for its highest referenced GLIBC symbol version and test its runtime
dependencies on the deployment host; it is not a static or broadly portable musl build.

The existing key-account privacy and functionality limits remain described in the
[README](README.md) and [sharing API](docs/zero-access/calendar-sharing-api.md). Key-account
JMAP calendars, calendar full-text search, re-sharing, free/busy-only grants, cross-server
sharing and key-account mail sharing remain refused or deferred.

### Verification and outstanding gates

The [upgrade outcome](docs/superpowers/plans/2026-10-27-stalwart-1-upgrade-outcome.md)
records every command and result; its "Final record" section indexes them. In summary:

- The test matrix, the coverage net and the test retention gates (the design's C10 inventory
  and coverage gates) ran at `f135a9881`, every row green
  (documented SMTP flakes passed rerun by name). CI's `test.yml` at `2867bc24f`, the
  last code change before PR #69's review,
  [run 41432100574](https://github.com/jaylaney/stalwart/actions/runs/41432100574), passed
  every job: style and the gates' workflow check, unit tests (593 passed), upstream's suites,
  the zero-access suites in both blob-store modes and in plain and key mode, the independent
  python-caldav client (plain 68 passed, 3 xfailed; key 74 passed, 2 xfailed), the fresh
  store restart gate, and the test retention gates (full 21,599 / 22,335 and
  runtime-only 12,537 / 13,197 covered lines, each 72 and 44 lines above the 0.16.27
  baseline's fraction).
- PR #69's review found two port regressions (the setup-token request's concurrency slot,
  and a delegated PUT that could exceed the recipient limit), fixed with tests in
  `4a3dcde7f` and `7b088e8dc`. CI's `test.yml` on the PR's final commit `b69bb841b`,
  [run 41472049105](https://github.com/jaylaney/stalwart/actions/runs/41472049105), passed
  every job that runs on pull requests (the test retention gates run only weekly and on
  dispatch).
- The test retention gates were run again on `main` at `32d264cba`
  ([run 41484056674](https://github.com/jaylaney/stalwart/actions/runs/41484056674))
  alongside the release build, and passed with every other job of that run: 795 tests;
  full 21,639 / 22,374 and runtime-only 12,539 / 13,198 covered lines, each 74 and 45 lines
  above the 0.16.27 baseline's fraction.
- Release-preparation checks, rerun at the preparation commit: 9 release-metadata tests
  (`python3 -I scripts/tests/test_release_metadata.py`) and 25 product-verifier tests
  (`python3 -B -I -m unittest discover -s scripts/release -p 'test_*.py'`) passed. A local
  shipping-configuration build at `2867bc24f` (macOS arm64, not a release artifact) printed
  the expected identity and carried no enterprise or test feature.
- The final whole-branch review ran as four Fable lenses and two scoped re-reviews; no
  Critical or Important finding remains.

Gates beyond the automated runs, recorded after publication:

- the exact-source Linux shipping build and its verification, run from the tag through
  `product-build.yml`: passed
  ([run 41485088188](https://github.com/jaylaney/stalwart/actions/runs/41485088188)). The
  published release notes record the checksums, the attestation and the measured GLIBC
  requirement of the real artifact;
- the native Apple Calendar (macOS and iOS), Thunderbird, DAVx5 and mail-client checklist
  under the [manual checklist](docs/zero-access/manual-checklist.md): passed on the deployed
  `fork-v0.2.0-beta.1` build, 2026-11-02, per Jay, with no per-client breakdown recorded;
  results from the `0.1.x` base are not evidence for this release;
- a clean-context whole-branch review: pending. The final review's four lenses were given
  the execution ledger and its rulings, which is not the clean context the plan specifies.

Not qualified: SQL and FoundationDB stores, multi-node execution, platforms other than the
ones named above, upstream's private test suites and the external protocol suites (Dovecot
imaptest, Fastmail's JMAP test suite, Apple's CalDAVTester).

## 0.1.1-beta.1 — 2026-10-29

Immutable tag: `fork-v0.1.1-beta.1`. This preview remains a prerelease.

### Provenance

- Fork source: the commit resolved by `git rev-parse 'fork-v0.1.1-beta.1^{commit}'`.
  The published release notes and shipped `stalwart.provenance.json` must record the exact
  full commit after release preparation is committed and tagged.
- Fully integrated upstream base: Stalwart `v0.16.27`, commit
  `aa47d8ef7bfba7200d3293198ac2592747b20e73`.
- No additional upstream backports are recorded separately for this preview.
- The published `fork-v0.1.0-beta.1` tag and artifacts retain their original identity.

### Changes

- Integrates the [.27 fixes](https://github.com/stalwartlabs/stalwart/releases/tag/v0.16.27):
  iTIP updates retain incoming timezone definitions for moved occurrences; OAuth accepts
  registered loopback redirects without a path; WebDAV `allprop` omits
  `DAV:current-user-principal`; IMAP `APPEND` and cross-account `COPY`/`MOVE` update
  `EmailDelivery` state, and combined `SEARCH RETURN` options retain all matching results
  where requested. Key-account refusal and sealed-data boundaries remain in force.
- Pins WebUI downloads to `v1.0.11` and checks the version before installation, avoiding
  incompatible WebUI updates on the .27 server base.
- Adds fork regressions for the .27 calendar/discovery changes and IMAP saved-search result
  combinations. Corrects system-test timing and upload-limit restoration fixtures while
  retaining their behavioral assertions.
- Uses the public repository's fork regressions and upstream suites for qualification;
  no private upstream test suite is required.
- Restores clean-checkout retrieval of OpenTelemetry from the original project's Git URL,
  pinned to the same `ae66e97b140f70e477ab710686aafce665cc2f8b` source commit.
- Adds no stored-format migration or legacy reader. The upstream 1.0 port remains deferred;
  this .27 integration does not promise ongoing upstream .16 maintenance or support.

### Compatibility and upgrade instructions

This experimental preview is intended for disposable installations. Existing-data upgrades
from `0.1.0-beta.1`, upstream production installations or unpublished fork builds have not
been qualified. Before testing existing data, back up the complete database, blob store and
configuration and verify restoration. Keep the matching pre-upgrade backup: replacing only
the binary is not a rollback guarantee. Unchanged stored layouts and formats do not establish
cross-version archive compatibility or a supported upgrade/rollback contract.

WebUI downloads remain pinned to `v1.0.11`; use the server's checked installer rather than
substituting a newer WebUI. The planned shipping configuration is Linux
`x86_64-unknown-linux-gnu` with RocksDB and excludes enterprise and `test_mode` features.
It requires an `x86-64-v2` CPU. Check the future artifact's provenance for its highest
referenced GLIBC symbol version and test runtime dependencies on the deployment host;
this is not a static or broadly portable musl build.

The existing key-account privacy and functionality limits remain described in the
[README](README.md) and [sharing API](docs/zero-access/calendar-sharing-api.md). This update
establishes no new backend, cluster, platform or native-client qualification.

### Verification and outstanding gates

The [upgrade outcome](docs/superpowers/plans/2026-10-27-stalwart-01627-upgrade-outcome.md)
records historical public-suite commands, results, source/artifact identities, bounded retries
and retained skip/xfail policy. Those results retain their named source identities and do not
establish execution against the future release commit or its shipping artifact.

Fresh release-preparation checks passed: eight release-metadata tests
(`python3 -B -m unittest discover -s scripts/tests -p 'test_release_metadata.py' -v`)
and thirteen product-verifier tests
(`python3 -B -m unittest discover -s scripts/release -p 'test_*.py' -v`).

All four [PR62 CI jobs](https://github.com/jaylaney/stalwart/actions/runs/40904792249)
passed at `7ba688c3051fe258eea15a9fcbcd68e6e594a8c9`. PR64 changes only duplicate-test
selection in the CI workflow; runtime sources, dependency versions/features and release
metadata are unchanged from that qualified source. Its
[CI run](https://github.com/jaylaney/stalwart/actions/runs/40917113519) at
`cce8d8d8f1d60a46242a156045479978f10b17f6` passed Test (including Directory) and Check
Style; remaining jobs were still running during release preparation. The dated changelog
is the only subsequent preparation edit. These CI results do not establish verification
of the future tagged shipping binary.

**TODO before artifact publication:** verify the exact tagged Linux shipping binary,
`--version`/help identity, clean source provenance, ELF CPU/GLIBC/runtime requirements,
checksums and build attestations. No Linux shipping artifact for this release commit has
been qualified. The product tag workflow builds and verifies artifacts after tag push and
uploads them to Actions; it does not create or publish a GitHub release.

Native Apple Calendar (macOS/iOS), Thunderbird, DAVx5 and mail-client qualification remain
pending under the [manual checklist](docs/zero-access/manual-checklist.md). SQL/other stores,
multi-node execution, additional platforms and production deployment remain unqualified.
This preview must remain a prerelease and does not claim general native-client or production
readiness.

## 0.1.0-beta.1 — 2026-10-26

Preview title: `release-0.1.0-beta-1`. Immutable tag: `fork-v0.1.0-beta.1`.

### Provenance

- Fork source: the commit resolved by `git rev-parse 'fork-v0.1.0-beta.1^{commit}'`.
  The published release notes and shipped `stalwart.provenance.json` record the exact full
  commit after release preparation is committed.
- Fully integrated upstream base: Stalwart `v0.16.26`, commit
  `18f4e9505027d790fd7e9fdad7f922be6719101d`.
- No additional upstream backports are recorded separately for this preview.

### Included fork behavior

- Zero-access account setup with password, recovery key, TOTP and app-password support.
- Private calendar events, tasks, journals and calendar metadata encrypted at rest, with
  decryption during authenticated CalDAV requests for ordinary clients.
- Local invitations between key accounts, pinned recipient keys, and owner/grantee view/edit
  calendar sharing, including delegated scheduling with `SENT-BY`.
- Key-account mail encrypted at rest. Contacts and files retain upstream storage behavior.
- Independent product version and full fork/upstream/source identity in operator-facing
  version/help/startup output. Internal crate compatibility versions remain upstream-based.
- Shipping Linux `x86_64-unknown-linux-gnu` build with RocksDB, binary/provenance checksums and
  build attestations. It excludes enterprise and `test_mode` features.

### Compatibility and upgrade instructions

This is the first experimental preview, intended for disposable installations. Upgrades from
upstream production installations or earlier unpublished fork builds have not been qualified.
Before testing existing data, back up the complete database, blob store and configuration and
verify restoration. Keep the matching pre-upgrade backup: rollback by replacing only the
binary is not supported or promised. The release preparation changes no stored layout or
cryptographic format; that does not establish an upgrade/rollback contract for the fork.

The GNU/Linux binary requires an `x86-64-v2` CPU. Consult its provenance for the highest
referenced GLIBC symbol version and test runtime library compatibility on the target host;
it is not a static or broadly portable musl build.

Key-account JMAP calendars, calendar full-text search, re-sharing, free/busy-only grants,
cross-server sharing and key-account mail sharing remain refused or deferred. The running
server receives credentials and handles plaintext during authenticated use; this is
server-side encryption at rest, not end-to-end encryption against a compromised server.
Revocation rotates future-write keys and does not reseal already readable data. See the
[README](README.md) and [sharing API](docs/zero-access/calendar-sharing-api.md) for the
remaining privacy and functionality boundaries.

### Verification and outstanding gates

The [calendar-sharing outcome](docs/superpowers/plans/2026-10-23-zero-access-plan14-outcome.md)
records earlier automated qualification at its named source commits. Those historical
results are not executions against this beta's release commit. The published beta notes
must record fresh release-preparation checks and the exact-source Linux shipping build,
with their commands/results and artifact checksums before publication.

Native Apple Calendar (macOS/iOS), Thunderbird, DAVx5 and mail-client qualification remain
pending under the [manual checklist](docs/zero-access/manual-checklist.md). SQL backends,
multi-node execution and production deployment are not qualified. This preview does not
claim complete native-client or production readiness.
