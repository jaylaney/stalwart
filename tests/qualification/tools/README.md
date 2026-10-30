# Strict-rule tooling

These tools compute two things for the C10 gates (rulings 7, 8, 31 and 32 in
`docs/superpowers/plans/2026-10-27-stalwart-1-upgrade-outcome.md`) from llvm-cov exports:

- the executed-sub-test rule;
- the fork-owned cohort's coverage fractions.

They read the same inputs as the committed baseline, `../baseline-0.16.27.json`, and reproduce it.

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

The module docstring of `strict_rule.py` gives the details: how a statement's anchor is chosen,
how the region is chosen, and what happens when a statement has no region. The baseline's
`subtests.strict_rule.method` describes the same method, with worked examples.

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
  `executed`.

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
- every recorded `last_statement` region, for the sub-test calls and the helper functions;
- every loop-body region;
- every per-step count and executed flag;
- that no function with a positive body count is missing from the baseline;
- every cohort file's full and runtime-only figures and its test-module spans;
- both gate fractions, `cohort.reference.threshold` and `threshold_runtime`.

It prints each mismatch and exits 1 if there is one. On the 0.16.27 inputs it takes about 50
seconds and prints:

```
cohort threshold: 17821/18490
cohort threshold_runtime: 10646/11246
checked: {"call_counts": 332, "call_regions": 197, "cohort_files": 40, "cohort_fractions": 2, "helper_counts": 4199, "helper_regions": 2249, "helpers": 2249, "loop_counts": 19, "loop_regions": 12}
result: OK
```

Task 7's gates run `functions` and `cohort` on the candidate's exports and sources, then compare
the results with the baseline: step by step, never mode by mode, and against both cohort
fractions. These tools do not implement that comparison.
