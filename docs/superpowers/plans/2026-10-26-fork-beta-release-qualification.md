# First fork beta: preparation qualification

Candidate version: `0.1.0-beta.1`. Release title: `release-0.1.0-beta-1`.
The immutable release tag `fork-v0.1.0-beta.1`, published notes and artifact provenance resolve
its exact final source commit after preparation is committed.

## Fresh local automated evidence

Executed in the isolated release worktree on 2026-10-26 with stable Rust via
`export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`. Core units reused the existing local
`CARGO_TARGET_DIR=/Users/jay/Development/stalwart/target`; no concurrent Cargo run shared it.

| Command | Actual result |
|---|---|
| `python3 scripts/tests/test_release_metadata.py` | 8 passed, 6.473 s |
| `/opt/homebrew/bin/python3 -B -m unittest discover -s scripts/release -p 'test_*.py' -v` | 13 passed, 0.422 s |
| `cargo fmt --all --check` | exit 0 |
| `git diff --check` | exit 0 |
| `cargo test -p vault -p groupware -p common` | exit 0; common 121, groupware 149, vault 45 passed; 0 failed; doc tests 0 |
| `cargo test -p http@0.16.26 --features test_mode` | exit 0; 6 passed, 0 failed; doc tests 0 |

The first core-unit attempt failed in the new build helper because array-valued package
metadata preceded the version field. A regression reproduced that real manifest layout;
the parser correction passed the complete core gate above. Existing non-enterprise warnings
remain; this is not a warning-free build claim.

The metadata harness executes the real common build script and a dependency-free Cargo
fixture. It checks prerelease formatting, real shipping manifest layout/agreement, malformed
and mismatched provenance, archive fallback and enclosing-repository isolation, clean/dirty
transitions, and incremental commit refresh in ordinary and packed-ref linked worktrees.
The product validator tests exercise version/help agreement, tag/source validation,
configuration isolation, JSON/checksum output and numeric GLIBC version comparison.

## Final release gates

The primary performs fresh independent whole-branch review, fork CI, and an exact-source
Linux shipping build before publishing the prerelease. Their final source SHA, run links,
results and artifact checksums belong in the published release notes and shipped provenance;
this file does not claim those future gates have passed.

Shipping command: `cargo build --locked --release -p stalwart --no-default-features --features rocks`.
The product workflow must execute and verify the compiled binary's version/help against its
clean checkout, not infer runtime identity from source-only tests. The GNU/Linux artifact
requires x86-64-v2; its actual GLIBC symbol requirement is measured during that build.

## Evidence limits

Earlier functional qualification remains in the calendar-sharing outcome at its named
commits; it is not relabeled as a run on the beta commit. macOS/iOS native Calendar, Thunderbird,
DAVx5, native mail clients, SQL backends, multi-node behavior and production deployment remain
pending. The manual client checklist remains uncompleted. This explicitly limited preview
must be published as a prerelease and makes no production or native-client readiness claim.
