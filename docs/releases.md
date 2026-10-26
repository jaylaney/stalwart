# Fork release policy

This fork releases independently of Stalwart. Its public version describes the fork's
compatibility and changes; the upstream release and commit are recorded separately.

## Version numbers

Start the independent release sequence at `0.1.0`. Existing design milestones named
"release 1", "release 2", and so on are development milestones, not public version numbers.

- Before `1.0.0`, increment the patch for compatible fixes and the minor for features or
  breaking changes. Document every breaking change explicitly, even during development.
- Release `1.0.0` when the fork commits to a stable compatibility contract.
- From `1.0.0`, follow [Semantic Versioning](https://semver.org/): patch for compatible fixes,
  minor for compatible features, and major for breaking changes.
- Use prerelease versions such as `0.2.0-beta.1` and `0.2.0-rc.1` for previews.
- Never replace a published version's contents or move its release tag. Ship a new version
  for a correction.

Choose the next fork version from the combined effect of fork and upstream changes on users.
Merging an upstream release does not automatically determine the fork's next version.
Several fork releases may share one upstream base.

Do not use an upstream version with a suffix such as `0.16.26-za.1` as the fork's release
sequence: SemVer treats it as a prerelease of upstream `0.16.26`. Build metadata after `+`
does not affect version ordering and must not be used to distinguish successive releases.

## Tags and provenance

Use `fork-v0.1.0` for the first release tag and the same `fork-v` prefix for subsequent
releases, including previews such as `fork-v0.2.0-beta.1`. This distinguishes fork release
tags from upstream's `v0.16.26` tags. If a product-specific prefix replaces `fork-v`, update
this policy and release tooling together before using it; retain published tags.

Every release entry in [the fork changelog](../CHANGELOG-fork.md) and its published release
notes must record:

- Fork version, tag, and release date.
- Exact fork source commit used for the release.
- Upstream release tag and its exact commit, identifying the latest fully integrated base.
- Any additional upstream backports, with their source commits; they do not imply that the
  whole later upstream release is included.
- User-visible changes, upgrade instructions, compatibility limits, and verification results.

Version output should identify the fork version, upstream base, and fork source commit so an
operator can identify a running build. Keep the upstream base distinct from the fork version.

## Compatibility and data upgrades

The compatibility contract covers public APIs, configuration, supported client behavior,
and supported upgrades of existing stored data. Release notes must identify affected users,
required operator actions, and any changes to those contracts.

Document supported source versions for an upgrade, backup requirements, migration effects,
and whether rollback is supported. Do not infer rollback safety from a version number or
promise that an older binary can read data written by a newer one. Storage and encrypted
format versions remain separate from the public release number; preserve the repository's
stored-layout and versioned-reader requirements.

## Changelogs

[CHANGELOG-fork.md](../CHANGELOG-fork.md) records fork releases, newest first, with an
`Unreleased` section for pending changes. At release time, create a dated version entry with
the provenance and upgrade information above. An upstream merge entry should summarize its
effect on fork users and reference the upstream release notes.

Preserve the inherited [CHANGELOG.md](../CHANGELOG.md) as upstream history. Continue taking
its upstream updates during merges; do not insert fork release entries there.

## Release checklist

- Choose the version and finalize its changelog entry, provenance, and upgrade/rollback notes.
- Check the candidate's package versions, version output, tag handling, artifact names, and
  release-note generation against this policy.
- Run the applicable release qualification in
  [the developer guide](superpowers/plans/README-dev.md), including the upstream merge checks
  when relevant. Record exact commands, results, and outstanding gates for the candidate.
- Record automated results separately from native client verification in
  [the manual checklist](zero-access/manual-checklist.md). Passing automated tests alone does
  not establish native client behavior or untested store/cluster configurations.
- Build shipping artifacts without `test_mode` or enterprise features, using the documented
  product configuration. Identify the source commit and artifact checksums in the release.
- Publish the immutable release tag, notes, and artifacts only after the applicable gates
  pass and publication is authorized. Keep previews marked as prereleases.

## Adoption status

This policy does not itself release `0.1.0`. Package manifests and runtime version strings
still use the upstream numbering. Before the first independent release, implement and verify
the fork version/provenance output and adapt the inherited release workflow: it currently
matches `v*.*.*` tags and extracts release notes from upstream's `CHANGELOG.md`. It must handle
the fork tag prefix, fork changelog, and preview status before publishing fork releases.
