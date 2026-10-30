# Strict-rule tooling

These tools compute two things for the C10 gates (rulings 7, 8, 31 and 32 in
`docs/superpowers/plans/2026-10-27-stalwart-1-upgrade-outcome.md`) from llvm-cov exports:

- the executed-sub-test rule;
- the fork-owned cohort's coverage fractions.

They read the same inputs as the coverage-derived parts of the committed baseline,
`../baseline-0.16.27.json`, and reproduce those parts. "Not checked" below lists the parts that come
from elsewhere.

- `strict_rule.py`: the command-line tool.
- `rustscan.py`: the source scanner it uses. It finds functions, their bodies, and the top-level
  statements of a body, each with its anchor.
- `cohort.py`: the cohort calculator. It computes the full and runtime-only figures with
  llvm-cov's report rules.

The tools need Python 3.9 or later and no packages. They hard-code no paths.

## The rule

A function executes in a step only if one function record of that step's export has three
positive counts:

1. **The body record.** For an `async fn`, this is the async body's own closure record, not the
   outer record. For a plain `fn`, it is the function's own record.
2. **The region that starts the body's last top-level statement.**
3. **The closing-brace exit region.**

The module docstring of `strict_rule.py` gives the details:
- how a statement's anchor is chosen and how its region is chosen;
- what happens when a statement has no region;
- how an empty body's exit is found.

The baseline's `subtests.strict_rule.method` describes the same method, with worked examples.

Some plain functions return an `async { }` or `async move { }` block as their last statement. For
those, the tool also measures the block's own closure record under the same three counts: this
is `tail` in the output, and `tail_block` in the baseline. A future that is built but never
polled therefore shows there.

## Identity

A function is identified by `(file, in, name)`:
- `in` is the enclosing inline `mod`, the `impl` or `trait` header in angle brackets, or the
  enclosing `fn`, joined by `::`;
- `in` is null for a function at file level.

The line of the `fn` keyword is data, not identity. A function therefore keeps its identity when
lines above it change. The identity is unique across all 2,267 functions at 0.16.27.
- `functions` exits with an error when two functions with a body record share an identity.
- `self-check` reports a mismatch when two baseline entries share one.

A gate maps a candidate's functions onto the baseline by this identity. A function that is
renamed, or moved to another file or another `impl`, needs a disposition that names both its old
and its new `(file, in, name)`.

## Inputs

- **The exports:** a directory of `llvm-cov export` JSON files, one per step, named
  `<step>.json`. For the 0.16.27 baseline, this is the 19 steps' exports in the git-ignored
  `.superpowers/sdd/baseline-01627/coverage/steps/`.
- **The merged export:** `llvm-cov export` JSON of every step's profiles merged. For the baseline,
  this is `.superpowers/sdd/baseline-01627/coverage/merged.json`.
- **The sources:** a tree holding `tests` and `crates` at the commit the exports were measured at.
  For the baseline that commit is `5b914ddb5`:

  ```sh
  mkdir -p /tmp/src-5b914ddb5
  git archive 5b914ddb59f5129ce1b6c2b5190a77e179fb2c42 tests crates | tar -x -C /tmp/src-5b914ddb5
  ```

The tools map an export's absolute file names onto that tree by their `tests/src/...` or
`crates/...` suffix. The exports can therefore come from any checkout path.

## Commands

Compute the three counts of every `tests/src` function, per step:

```sh
python3 -I tests/qualification/tools/strict_rule.py functions \
  --exports .superpowers/sdd/baseline-01627/coverage/steps \
  --sources /tmp/src-5b914ddb5 \
  --out /tmp/functions.json
```

For each function, the output gives:
- its geometry: body braces, top-level statements, anchors, and the loop block's first statement;
- per step, the reported record: symbol, `body`, `last`, `exit`, `loop`, the chosen regions, and
  `executed`;
- for a function with a tail block, the same for the block, under `tail_steps`.

Compute the cohort's full and runtime-only coverage:

```sh
python3 -I tests/qualification/tools/strict_rule.py cohort \
  --merged .superpowers/sdd/baseline-01627/coverage/merged.json \
  --sources /tmp/src-5b914ddb5 \
  --baseline tests/qualification/baseline-0.16.27.json \
  [--out /tmp/cohort.json]
```

The baseline's `cohort.files` gives the file list. The test-support files are the ones it marks
`runtime_excluded`.
- **The full figure** counts every line and function of those files.
- **The runtime-only figure** drops the functions that start inside an inline module whose cfg
  needs `test`, and drops the test-support files whole.

The file's docstring gives the counting rules. On the 0.16.27 inputs it prints:

```
full: lines 17821/18490, functions 1841/1939
runtime: lines 10646/11246, functions 1184/1240
```

Check the baseline against the exports, the merged export and the sources:

```sh
python3 -I tests/qualification/tools/strict_rule.py self-check \
  --baseline tests/qualification/baseline-0.16.27.json \
  --exports .superpowers/sdd/baseline-01627/coverage/steps \
  --merged .superpowers/sdd/baseline-01627/coverage/merged.json \
  --sources /tmp/src-5b914ddb5
```

`self-check` recomputes every function and compares the results with the baseline:
- **recorded values:**
  - every `last_statement` region, for the sub-test calls, the helper functions and the tail
    blocks;
  - every loop-body region;
  - every per-step count and executed flag.
- **Both directions** for helpers and tail blocks: a positive body count the baseline does not
  record is a mismatch.
- **The coverage-derived fields the gates read:**
  - each call's `executed`, per step;
  - each call's `expected_steps`, derived from its mode, env, other conditions and cfg flag, and
    from its suite's steps and their modes;
  - each loop body's `positive_at_0_16_27` and `required_in`;
  - each helper's line, span, `async` and symbol;
  - the aggregate counts in `subtests.counts` and `helpers.counts`.
- **Identity:** that every helper identity is unique.
- **The cohort:**
  - every file's full and runtime-only figures and its test-module spans;
  - both gate fractions, `cohort.reference.threshold` and `threshold_runtime`.

**Not checked.** These parts of the baseline come from elsewhere, so `self-check` does not
recompute them. Rechecking them would need the cargo listings or the scan tooling, which live
outside this directory.

| Section | Comes from |
|---|---|
| `libtest`: identifiers, status, CI selection, `net_5b914ddb5` | The `cargo test ... -- --list` and `-- --ignored --list` listings (relisted at 9a7574203 for the tests crate, the unified workspace and every CI step; reused from 5b914ddb5 for the per-crate feature sets), the static test scan, and the 0.16.27 net's `steps.tsv` |
| `restored_tests` | The static scan of the 9a7574203 sources, compared with v0.16.27 and v1.0.1 |
| `dispositions` | The design's decisions, the SPDX census, `steps.tsv` and the `--ignored` listings |
| The sub-test call list and each call's guards (`mode`, `cfg`, `env`, `other_conditions`, `target`) | The static sub-test scan at 9a7574203. `expected_steps` is checked against these guards, but they are not rescanned. |
| `subtests.steps` and `subtests.suites` | `steps.tsv` |
| `cohort.per_crate`, `cohort.groups`, each file's region counts, and `cohort.totals` beyond the two fractions | `merged.files.tsv` and `tables.json` |
| `provenance` and `schema` | Text |

It prints each mismatch and exits 1 if there is one. On the 0.16.27 inputs it takes about 50
seconds and prints:

```
cohort threshold: 17821/18490
cohort threshold_runtime: 10646/11246
checked: {"aggregates": 13, "call_counts": 332, "call_executed": 197, "call_expected_steps": 198, "call_regions": 197, "cohort_files": 40, "cohort_fractions": 2, "helper_counts": 4199, "helper_identity": 2249, "helper_regions": 2249, "helpers": 2249, "loop_counts": 19, "loop_derived": 12, "loop_regions": 12, "tail_blocks": 12, "tail_counts": 24}
result: OK
```

Task 7's gates run `functions` and `cohort` on the candidate's exports and sources, then compare
the results with the baseline:
- step by step, never mode by mode;
- functions by identity;
- tail blocks wherever they executed;
- against both cohort fractions.

These tools do not implement that comparison.
