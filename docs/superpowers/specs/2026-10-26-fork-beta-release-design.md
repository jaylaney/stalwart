# Fork beta release identity

Prepare the first independent beta, version `0.1.0-beta.1`, with release title
`release-0.1.0-beta-1` and immutable tag `fork-v0.1.0-beta.1`.

## Approach

Keep internal dependency crates on upstream `0.16.26`; version the shipping `stalwart`
package independently. Renumbering every crate would create unnecessary dependency churn
and would break the existing numeric-only internal Semver parser. A release-notes-only
snapshot was considered; Jay selected completing fork version/provenance first.

## Contract

- Centralize fork version, upstream base tag and full commit, and full fork source commit.
- Make CLI version/help and operator-facing startup identity distinguish fork and upstream.
- Preserve internal upstream compatibility version semantics and stored formats.
- Discover source identity at build time; avoid stale metadata after a source revision.
  Source archives without Git must report unknown unless supplied explicit build provenance.
- Keep builds without enterprise or test_mode: `--no-default-features --features rocks`.
- Extend the existing Linux x86_64 product workflow to check the built version/provenance
  and retain checksums and attestation. Use the existing fork tag trigger.
- Finalize the fork changelog, upstream provenance, backup/upgrade/rollback limitations,
  exact automated verification and pending native-client gates in release notes.
- Publish the beta as a prerelease only after relevant automated verification and a fresh
  independent review. Native, SQL and cluster coverage must remain explicitly unverified
  unless separately demonstrated. Never move a published tag.

## Scope and verification

Touch only version/provenance plumbing, shipping manifest/lock entry, focused tests,
product workflow and fork release documentation. No account, cryptographic, persistence
or protocol behavior changes. Verify source metadata refresh, no-Git fallback, prerelease
formatting and the actual Linux shipping binary; run relevant existing checks sequentially.
The primary coordinates Git integration and publication; implementation and independent
review run in separate agents under the repository routing policy.
