# Search regression follow-up: M16, M23 and M48

Date: 2026-11-11. Base: `bae62db6f08af27b1058c148a2fbe292f18635e9`,
branch `release7-search`, plan 16 revision 27 / design revision 33.

This follow-up adds normal regression tests for the three survivors recorded in
the [final mutation report](2026-11-07-zero-access-16-search-mutations-final-review.md).
The historical campaign results are unchanged. No deliberate faulty variant was
prepared or executed in this follow-up; correct-code passes are not new mutation kills.

## Changes

- **M16:** `case_pending_rebuild_cursor_overrides_scanned_state` observes real
  thread-blob loss and starts a real rebuild. It directly seeds only the resident
  scan watermark, checks that every other `Current` predicate holds, and requires
  the real pass to advance the durable cursor. A second pass completes the cursor,
  and a cache eviction/reload verifies exact membership. This reproduces the
  resident state, not the full cross-node schedule that could create it. No new
  production hook was needed.
- **M23:** the partial-record planner test now makes one new document reference
  the record that the same change record deletes. Its assignment must retain that
  thread. A second document shares the subject but has disjoint hashes and stays
  singleton. The test covers the planner contract; ordinary wire reachability of
  that mixed record is not established. In particular, the earlier report's single
  `Email/set` example is unsuitable: creates flush before destroys use a separate
  batch and commit.
- **M48:** a planner test replays an insert for a held document, once live and once
  retained-deleted. It verifies no opener call, no new records/content/aliases or
  witness, preserved identity, and the permitted manifest-only checkpoint advance.

All code edits are in the planner's `#[cfg(test)]` module or the integration-test crate.
Production behavior, stored layouts, defaults and test-suite dispatch are unchanged
apart from registering M16 in the existing recovery module with normal cleanup.

## Review and validation

Two Sol high implementers had disjoint file ownership. A fresh Astra xhigh reviewer
approved requirements and code quality with no actionable findings. Primary
inspection confirmed the production portion of `pass.rs` is byte-identical to base.

- Planner module: 18 passed, 0 failed, 54 filtered; exit 0. Command wall time
  222.419 seconds, including compilation. A fresh check of the final formatted
  source also passed all 18 tests (127.531 seconds including compilation), with
  stable source hashes. Neither unit run emitted a compiler warning.
- Workspace formatting: passed. The first check identified one new expression's
  line wrapping; the correction and both results are retained.
- Canonical RocksDB ZA suite: 1 passed, 0 failed, 116 filtered; exit 0. Test
  time 1663.10 seconds; total command time 1968.776 seconds. M16's banner appeared
  once, later recovery cases ran, all 183 differential forms matched apart from
  the documented K deviation, and final cleanup completed. Source hashes were
  unchanged across the run. The known Darwin compact-unwind linker warning
  was emitted during the 4m 53s compilation; no runtime panic was logged.

The exact commands, source hashes, review, logs and receipts are retained under
`.superpowers/sdd/2026-11-07-zero-access-16-search/task19-regression-followup-2026-11-11/`
in this checkout. This directory is ignored by Git.

The mutation campaign's exclusions and other named-catcher findings are unaffected.
Task 19's external qualification gates remain open. This follow-up establishes
correct-code regression execution and static assertion review only.

Commands (stable Rust, Homebrew rustup first on PATH):

```sh
cargo test --locked --offline -p email --lib \
  --features test_mode,enterprise,common/test_mode \
  message::za::search::pass::tests -- --nocapture
STORE=RocksDb RUST_MIN_STACK=16777216 \
  cargo test --locked --offline -p tests za::za_tests -- --exact --nocapture
cargo fmt --all --check
```

The integration run used a private temporary store and the existing ignored
plan target directory. `BLOB_STORE`, `ZA_KEY_ACCOUNTS` and `ZA_SEARCH_*` environment overrides
were absent. Only the default RocksDB blob path was run for this follow-up; the
prior FileSystem qualification was not repeated.

Validated test-source SHA-256 values:

| File | SHA-256 |
|---|---|
| `crates/email/src/message/za/search/pass.rs` | `d1754b3bb3c57308749c8796b0ab714425f9c1a0e3dbe3f85f254cd127d74d14` |
| `tests/src/za/mail/index.rs` | `e12ddbcd116371982bbb7af68cf236f45cee9a206a0fe6a46c033950e7f0c8f1` |

ZA log SHA-256: `89374082defc06b6e2dd6eb68d37bcb0940e9544970301f2dafbb93e4b499800`.

Final planner log SHA-256: `beab7b2f939a1edbed896c8bab89cf821ef150d1facd0e4b93d36a40948561c1`.
