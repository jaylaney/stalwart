# Fork Beta Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** Ship `0.1.0-beta.1` with reliable fork/upstream/source identity and a Linux x86 binary.

**Architecture:** Leave upstream internal crates and compatibility semantics intact; independently
version the shipping package and generate common runtime identity from shared build metadata.
Use the existing product workflow for shipping builds and attestations.

**Tech Stack:** Rust, Cargo, GitHub Actions, shell/Python focused verification.

**Spec:** `docs/superpowers/specs/2026-10-26-fork-beta-release-design.md` (approved).

## Global Constraints

- Fork version `0.1.0-beta.1`, tag `fork-v0.1.0-beta.1`, title `release-0.1.0-beta-1`.
- Preserve internal crate versions, stored layouts, protocol compatibility semantics and encryption.
- Shipping configuration: `--no-default-features --features rocks`; no enterprise or test_mode.
- Source archives without Git report unknown unless explicit build provenance is supplied.
- No upstream publication. Primary owns commits, Git integration and release publication.

## Review Focus

- Linked worktree and packed-ref metadata must refresh after a new commit.
- Invalid source overrides must fail clearly; no-Git builds must not claim an unrelated parent Git repo.
- Shipping package version, generated fork metadata and tag must agree.
- All operator identity fields distinguish fork from upstream without breaking protocol version parsing.
- Release artifacts match the exact tagged source; native/SQL/cluster gates remain explicit.

### Task 1: Fork runtime identity and provenance

**Files:** `crates/main/Cargo.toml`, `Cargo.lock`, `crates/common/build.rs`, new build-support
metadata helper/tests, `crates/common/src/lib.rs`, operator identity consumers identified by inspection,
and root fork metadata file if needed.

**Interfaces:** Produce centralized fork version/upstream tag/full upstream commit/full fork source
commit, full operator identity, plus an unadorned fork version. Task 2 consumes metadata and CLI output.

- [ ] Add focused failing tests for prerelease formatting, manifest agreement, source SHA validation,
  no-Git fallback and incremental commit refresh including linked worktrees.
- [ ] Record expected failures before implementation; tests may use a small rustc build-script harness
  to avoid compiling the full server for metadata-only checks.
- [ ] Independently version the main package; keep internal package versions unchanged.
- [ ] Generate centralized identity at build time; validate explicit overrides and track Git ref/env
  changes. Pin and verify the upstream base commit from source ancestry/history.
- [ ] Update --version/help/startup/console and appropriate telemetry identity without changing
  numeric-only upstream Semver logic, public protocol versions or data formats.
- [ ] Run focused tests and rustfmt, report evidence/diff for primary review. Do not commit.

### Task 2: Product workflow and release records

**Files:** `.github/workflows/product-build.yml`, release verification helper/tests if justified,
`docs/releases.md`, `CHANGELOG-fork.md`, focused release qualification record.

**Interfaces:** Consume Task 1 metadata path/format and CLI identity. Produce shipping binary,
checksum and durable provenance artifact, validated against checked-out SHA and release tag.

- [ ] Add/run a failing focused check demonstrating missing or mismatched version/source/tag rejection.
- [ ] Extend product workflow to verify actual --version/--help with CONFIG_PATH unset, validate
  tag/version/manifest consistency, generate provenance and include it in the existing artifact.
- [ ] Keep existing action pins and build feature configuration; publish no release from the workflow.
- [ ] Update adoption status and dated beta changelog with upstream provenance, compatibility,
  backup/rollback limits and explicit pending native/SQL/cluster qualification.
- [ ] Run focused checks and diff checks. Report exact commands/results; do not commit or publish.

### Task 3: Independent review, shipping verification and publication

**Files:** Final qualification record and release notes only, plus adjudicated fixes if needed.

- [ ] Primary inspect actual diff; run fresh focused checks and relevant Cargo checks sequentially.
- [ ] Commit preparation; fresh independent whole-branch review against original base and approved spec.
- [ ] Resolve verified findings, then commit and push prepared release branch to fork.
- [ ] Run exact-source Linux product build; verify CLI source identity, artifacts/checksum/provenance.
- [ ] Publish immutable fork tag and GitHub prerelease with verified assets only after applicable gates.
  Record full final source commit and evidence in published notes, avoiding self-referential commits.
- [ ] Confirm remote tag, prerelease flag, assets and checksums. Stop after the authorized beta ships.
