# Fork changelog

This file records releases of the independent zero-access fork. Versioning, provenance,
compatibility, and release qualification follow [the release policy](docs/releases.md).
The inherited [CHANGELOG.md](CHANGELOG.md) records upstream Stalwart history.

## Unreleased

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
