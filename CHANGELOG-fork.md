# Fork changelog

This file records releases of the independent zero-access fork. Versioning, provenance,
compatibility, and release qualification follow [the release policy](docs/releases.md).
The inherited [CHANGELOG.md](CHANGELOG.md) records upstream Stalwart history.

## Unreleased

Preparing `0.1.1-beta.1`, independently versioned from the integrated upstream base
Stalwart `v0.16.27`, commit `aa47d8ef7bfba7200d3293198ac2592747b20e73`.
Publication and release qualification remain separate gates; the published
`0.1.0-beta.1` tag and artifacts retain their original identity.

- Integrates the [.27 fixes](https://github.com/stalwartlabs/stalwart/releases/tag/v0.16.27):
  iTIP updates retain incoming timezone definitions for moved occurrences; OAuth accepts
  registered loopback redirects without a path; WebDAV `allprop` omits
  `DAV:current-user-principal`; IMAP `APPEND` and cross-account `COPY`/`MOVE` update
  `EmailDelivery` state, and combined `SEARCH RETURN` options retain all matching results
  where requested. Key-account refusal and sealed-data boundaries remain in force.
- Pins WebUI downloads to `v1.0.11` and checks the version before installation, avoiding
  incompatible WebUI updates on the .27 server base.
- Uses the public repository's tests for qualification, including fork regressions and
  public upstream suites; no private upstream test suite is required. Exact candidate
  results and outstanding gates must be recorded before publication.
- Adds no stored-format migration or legacy reader. Existing backup and restoration
  requirements still apply; replacing only the binary is not a rollback guarantee.
  This update establishes no new backend, cluster, platform or native-client qualification.
- The upstream 1.0 port remains deferred. This .27 integration does not promise ongoing
  upstream .16 maintenance or support.

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
