# The C10 gates

Two gates compare a candidate (the fork on upstream 1.0.1) with the 0.16.27 baseline,
`tests/qualification/baseline-0.16.27.json`. Task 7 of the upgrade plan defines them, and
Task 1's rulings 7, 8, 25, 31 and 32 set their rules.

- `check_test_inventory.py`: the libtest inventory and the executed rule.
- `check_coverage.py`: the fork-owned cohort's line coverage.
- `gate_common.py`: what both share. It loads the dispositions file and parses listing captures.
- `test_gates.py`: offline self-tests of both gates. They need no cargo.

Both gates build on `tests/qualification/tools/` (`strict_rule.py`, `cohort.py`,
`rustscan.py`; see the README there) and reimplement neither the strict rule nor the cohort
fractions. Both need Python 3.9 or later and no packages. Run every Python command with `-I`.

Exit status, for both gates: 0 pass, 1 gate failure, 2 bad input.

## The dispositions file

`tests/qualification/dispositions.json` lists everything whose 1.0 counterpart differs from
0.16.27. Its `schema` key describes every field. In short:

- **`dispositions`**: one entry per decision.
  - `kind`: `sel_only`, `subject_deleted`, `internals_replaced_equivalent`,
    `internals_replaced_no_equivalent`, `ungated_as_today`, `ignored_as_today` or
    `needs_docker_ci_only`.
  - `scope`: `libtest`, `crate`, `file`, `sub_test_call`, `function`, `configuration` or
    `case`. A `case` entry (ruling 103) records a case retired inside a function that still
    runs. It is documentation only: the gates never apply it.
  - `subjects`: what the decision covers.
  - `steps` (optional): the baseline step names the entry is limited to. Without it, the entry
    applies in every step. Use it when a function stopped running in one step only (its caller
    there was removed) and stays required in the others.
  - `evidence`: the 1.0 change, ruling or expectation-ledger row.
  - `equivalents`: for `internals_replaced_equivalent` only. The gate checks the
    equivalents in the subject's place, where the subject was required.

  The baseline's own 34 dispositions apply too. They are not repeated in the file.
- **`renames`**: exact `{from, to}` pairs for libtest identifiers and for `tests/src`
  functions (`file`, `in`, `name`, the strict-rule identity). Also step and configuration
  names. The gate never matches names fuzzily. If the candidate still has the old identifier,
  the gate accepts it.
- **`selection_moves`**: tests whose CI step changed. For example, ruling 53's DKIM tests leave
  the workspace unit step for a build without `smtp/enterprise`. `to_command` is the command
  whose `--list` output the new step's capture holds.
- **`cohort`**: three lists.
  - `moves`: cohort files at new paths.
  - `additions`: fork runtime files the port added, which join the cohort.
  - `excluded_additions`: fork test and test-support files the port added. They are reported
    as their own group and never counted in the cohort.

An entry marked `"unconfirmed": true` is a proposal for the primary. The gates do not apply
it. A requirement that only an unconfirmed entry would satisfy fails as `unconfirmed`,
`rename_unconfirmed` or `move_unconfirmed`, and the failure names the entry. Set the flag to
`false` once the entry is confirmed.

## The inventory gate

```sh
python3 -I scripts/tests/check_test_inventory.py \
  --baseline tests/qualification/baseline-0.16.27.json \
  --dispositions tests/qualification/dispositions.json \
  --listing <listings dir> --ignored-listing <ignored listings dir> \
  --executed <functions.json>
```

The tool's docstring lists every check. Each failure line gives:
- the check;
- the test, call or function;
- the step;
- the count that failed: body, last statement, exit or loop.

### Inputs

**`--listing`** is a directory holding one file per configuration. Each file is the output of
one `cargo test ... -- --list` command, stdout and stderr together. Cargo prints the
`Running ... (target/debug/deps/<crate>-<hash>)` lines that name each test's package on
stderr, so capture both with `2>&1`.
- **Naming.** A file is named `<configuration>.txt`, or it starts with
  `# config: <configuration>`.
- **Configurations.** The names are the baseline's: `libtest.configurations` in the baseline
  JSON, with the 0.16.27 command of each. The gate requires a capture for each of these:
  - every CI step (`kind: ci_step`);
  - a selection move's `to_configuration`, wherever a moved test is no longer in its old step.

  Package and feature-set captures (`kind: feature_set`) are optional. Each one you capture is
  checked: a test it listed at 0.16.27 must still be listed there.
- **Filtered steps.** For a step that runs a filter, capture the filtered command. A file may
  also hold `## SELECTED` and `## UNFILTERED` sections, as the baseline's captures do. Then
  `SELECTED` is the step's selection, and both sections count as listed.

**`--ignored-listing`** is the same, for `-- --ignored --list`. It needs a capture of each
baseline `kind: ignored_listing` configuration, and the `--ignored-only` suffix of a name is
optional.

List the configurations and their 0.16.27 commands:

```sh
python3 -I -c 'import json; c = json.load(open("tests/qualification/baseline-0.16.27.json"))["libtest"]["configurations"]
for k, v in sorted(c.items()):
    if v["kind"] in ("ci_step", "ignored_listing"): print(k, "\t", v["command"])'
```

Use each step's 1.0 command, from `.github/workflows/test.yml` once Task 8 rewrites it.
Before then:
- the workspace unit step's feature list is the outcome note's "`UNIT_TEST_FEATURES` for 1.0";
- `http` is `-p http@1.0.1`;
- each selection move's `to_command` gives its new step.

Capture under the integration rows' environment (`STORE=RocksDb RUST_MIN_STACK=16777216`).

**`--executed`** is the strict rule, recomputed on the candidate. It is the output of
`strict_rule.py functions` over the candidate's per-step exports and sources:

```sh
mkdir -p /tmp/src-candidate
git archive <candidate sha> tests crates | tar -x -C /tmp/src-candidate
python3 -I tests/qualification/tools/strict_rule.py functions \
  --exports <steps dir> --sources /tmp/src-candidate --out /tmp/functions-candidate.json
```

- **The steps directory.** It holds one region-level `llvm-cov export` JSON per coverage step,
  named `<step>.json`. Use the baseline's 19 step names (`helpers.steps`) and its commands
  (`subtests.steps[*].command` and `.superpowers/sdd/baseline-01627/coverage/steps.tsv`).
- **How to measure.** Run each step with `cargo llvm-cov --no-report ...`, one at a time. Then
  export each step from that step's profile alone.
- **Step names.** A step with no export fails every requirement in it, so keep the names. If a
  step must be renamed, add the pair to `renames.steps`.

## The coverage gate

```sh
python3 -I scripts/tests/check_coverage.py \
  --baseline tests/qualification/baseline-0.16.27.json \
  --dispositions tests/qualification/dispositions.json \
  --merged <merged export> --sources /tmp/src-candidate [--out /tmp/coverage-candidate.json]
```

`--merged` is the `llvm-cov export` JSON of every step's profiles merged, the same profiles
the steps directory holds. `--sources` is the candidate tree used for `--executed`.

The gate fails in two cases.
- **A fraction is too low.** The cohort's full or runtime-only line fraction falls below
  17,821/18,490 or 10,646/11,246. The comparison is exact: the integers are cross-multiplied.
- **A cohort file is missing** from the sources.

It also reports these groups, which never count in the cohort:
- `restored_tests` (`*restored_tests.rs`);
- `fork_test_files` (the `excluded_additions`);
- `tests_crate` (`tests/`);
- the upstream product, as a total and per crate, with deltas against the baseline's
  `per_crate` figures.

At 0.16.27, upstream's inline test modules sat in their own files; at 1.0 they live in the
`restored_tests` files. A crate's delta therefore also moves by its tests' lines.

## Self-tests

```sh
python3 -I scripts/tests/test_gates.py -v
```

The 28 offline tests take about 4 seconds. Their docstring lists the cases:
- the strict rule on synthetic exports, run through `strict_rule.py functions`;
- listing faults against a candidate built from the committed baseline's own records;
- coverage exports one line below each threshold, with a restored test file that would
  otherwise offset the drop.

### The baseline as its own candidate

`--real DIR` adds two tests that feed the gates the 0.16.27 baseline's raw inputs as the
candidate. Both gates must pass: with an empty dispositions file, and, for the inventory, with
the committed one. `DIR` holds:

- `list/` and `ign/`: the 105 captures of
  `.superpowers/sdd/2026-10-27-stalwart-1-upgrade/baseline-A/combined/libtest/`, the
  `--ignored-only` files in `ign/` and the others in `list/` (symbolic links will do);
- `functions.json`: from `strict_rule.py functions` over
  `.superpowers/sdd/baseline-01627/coverage/steps` and the 5b914ddb5 sources;
- `merged.json`: `.superpowers/sdd/baseline-01627/coverage/merged.json`;
- `sources/`: the 5b914ddb5 tree (`git archive 5b914ddb5 tests crates`).
