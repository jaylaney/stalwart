# Repository Guidelines

## Project Structure & Module Organization

This Rust 2024 Cargo workspace forks Stalwart for zero-access calendars. `crates/main` builds
`stalwart`; `common`, `store`, and `directory` provide shared infrastructure. Fork logic lives
primarily in `crates/vault`, `crates/groupware`, `crates/dav`, and `crates/http`.
Integration suites and fixtures live in `tests/src` and `tests/resources`; templates, locales,
and deployment assets live in `resources`. Read `CLAUDE.md` and
`docs/superpowers/plans/README-dev.md` before changing fork behavior; approved designs and
outcome records live under `docs/superpowers/`.

## Build, Test, and Development Commands

Use stable Rust. On this Mac, prepend Cargo commands with
`export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`.

- `cargo build --release -p stalwart --no-default-features --features rocks`: build the product
  without enterprise features.
- `cargo fmt --all --check`: run the CI formatting check.
- `cargo test -p vault -p groupware -p common`: run core unit tests.
- `STORE=RocksDb RUST_MIN_STACK=16777216 cargo test -p tests za::za_tests`: run zero-access
  integration tests, including the leak regression.
- With the same environment, run `cargo test -p tests webdav::webdav_tests`; repeat with
  `ZA_KEY_ACCOUNTS=1` to cover encrypted accounts.

For this checkout's local server, use the git-ignored `.run/` scripts: first
`start-recovery.sh`, then `provision.sh`; subsequent starts use `start.sh`.

## Coding Style & Naming Conventions

Follow `.editorconfig`: four spaces, UTF-8, LF, final newline, and 100-column lines.
Use rustfmt, Rust's `snake_case` functions/modules and `PascalCase` types, and neighboring SPDX
headers. Keep fork changes narrow. Do not hand-edit generated registry schemas or event enums.

## Testing Guidelines

Use Rust unit tests and Tokio async tests. Integration suites use descriptive `*_tests`
entrypoints; select whole suites, not their internal submodules. Run suites sequentially because
they share server ports. Some upstream suites require Docker via testcontainers. Add regression
coverage for changed behavior; no numeric coverage threshold is configured. Record commands and
results, and use `docs/zero-access/manual-checklist.md` for client verification.

## Commit & Pull Request Guidelines

Recent commits use concise imperative subjects, such as “Run the zero-access suites in their
own CI job.” Keep changes focused; describe behavior, rationale, related issues, and validation
in fork PRs. This fork is developed independently; changes are not intended for submission or
merging into the original project. The inherited `CONTRIBUTING.md` describes upstream policy,
not this fork's contribution requirements. Never push, open issues/PRs, or draft reports against
upstream `stalwartlabs/stalwart`.

## Security & Persistence

Never log keys or credentials, change stored struct layouts, or deploy `test_mode` builds.
Preserve ordinary-account behavior and route vault mutations through `za_commit`.
