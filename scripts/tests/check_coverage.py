#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only
"""The C10 coverage gate (Task 7 of the 1.0.1 upgrade plan; Task 1's rulings 8, 25 and 31).

Usage:
  check_coverage.py --baseline PATH --dispositions PATH --merged PATH --sources DIR [--out PATH]

  --baseline      tests/qualification/baseline-0.16.27.json
  --dispositions  tests/qualification/dispositions.json
  --merged        the candidate's `llvm-cov export` JSON of every step's profiles merged
  --sources       the candidate's source tree (holding `crates` and `tests`)
  --out           optional: write the figures as JSON

The cohort is the baseline's 40 files, mapped across moves (the baseline's own `cohort.moves` and
the dispositions file's), plus the fork runtime files the port added (`cohort.additions`). A moved
file is measured at its new path when the sources hold it, else at its old one, so the unchanged
0.16.27 tree passes as its own candidate. Its figures are computed with `cohort.py` (llvm-cov's
report rules), and the gate fails when either falls below the baseline's exact fraction, compared
as integers:
  full          covered * 18490 >= 17821 * lines   (cohort.reference.threshold)
  runtime-only  covered * 11246 >= 10646 * lines   (cohort.reference.threshold_runtime): the same
                files without their inline cfg(test) modules and the test-support files
It also fails when a cohort file is missing from the sources.

Reported beside the cohort, from the export's per-file line summaries, and never counted in it:
  restored_tests         every crates/**/*restored_tests.rs (restored upstream unit tests)
  fork_test_files        the port's fork test and test-support files (`cohort.excluded_additions`)
  tests_crate            tests/**
  upstream product       the rest of crates/, without the SEL-only scim and scim-proto crates,
                         per crate, with its delta against the baseline's per_crate figures (all
                         files less the fork-owned ones). At 0.16.27 upstream's inline test modules
                         sat in their files; at 1.0 they are restored_tests files, so a crate's
                         delta also moves by its tests' lines.
Exit status: 0 pass, 1 gate failure, 2 bad input.
"""
import argparse
import collections
import json
import os
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from gate_common import InputError, load_dispositions, load_json  # noqa: E402
import cohort  # noqa: E402  (tests/qualification/tools, put on the path by gate_common)

SEL_CRATES = {"scim", "scim-proto"}


def cohort_files(base, disp, sources):
    """(files, support, problems, mapping) for the candidate tree."""
    moves = {}
    for m in base["cohort"].get("moves", []):
        moves[m["from"]] = m["to"]
    for f in base["cohort"]["files"]:
        if f.get("moves_to"):
            moves[f["file"]] = f["moves_to"]
    for m in disp["cohort"]["moves"]:
        moves[m["from"]] = m["to"]
    files, support, problems, mapping = [], set(), [], {}

    def exists(p):
        return os.path.isfile(os.path.join(sources, p))

    for f in base["cohort"]["files"]:
        old = f["file"]
        new = moves.get(old)
        path = new if new and exists(new) else old
        if not exists(path):
            problems.append(f"FAIL [cohort_file_missing] {old}" + (f" (moved to {new})" if new else "") +
                            ": not in the sources")
            continue
        mapping[old] = path
        files.append(path)
        if f.get("runtime_excluded"):
            support.add(path)
    for a in disp["cohort"]["additions"]:
        p = a["file"]
        if not exists(p):
            problems.append(f"FAIL [cohort_file_missing] {p} (a cohort addition): not in the sources")
            continue
        if p in files:
            raise InputError(f"cohort addition {p} is already a cohort file")
        files.append(p)
        mapping[p] = p
        if a.get("runtime_excluded"):
            support.add(p)
    return files, support, problems, mapping


def file_summaries(merged, sources):
    """{repo-relative path: (lines, covered)} from the export's per-file summaries."""
    d = load_json(merged, "--merged")
    try:
        files = d["data"][0]["files"]
    except (KeyError, IndexError, TypeError):
        raise InputError("--merged is not an llvm-cov export")
    out = {}
    for f in files:
        parts = f["filename"].replace("\\", "/").split("/")
        rel = None
        for i in range(len(parts)):
            cand = "/".join(parts[i:])
            if cand.startswith(("crates/", "tests/")) and os.path.isfile(os.path.join(sources, cand)):
                rel = cand
                break
        if rel is None:
            continue
        s = f["summary"]["lines"]
        prev = out.get(rel, (0, 0))
        out[rel] = (prev[0] + s["count"], prev[1] + s["covered"])
    return out


def crate_of(rel, sources, cache):
    """The directory name of the nearest Cargo.toml above a crates/ file (crates/trc/event-macro is
    `event-macro`, as the baseline's per_crate names it)."""
    d = os.path.dirname(rel)
    if d not in cache:
        cur = d
        while cur and cur != "crates" and not os.path.isfile(os.path.join(sources, cur, "Cargo.toml")):
            cur = os.path.dirname(cur)
        cache[d] = os.path.basename(cur) if cur and cur != "crates" else rel.split("/")[1]
    return cache[d]


def pct(c, n):
    return f"{100.0 * c / n:.2f}%" if n else "n/a"


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--baseline", required=True)
    ap.add_argument("--dispositions", required=True)
    ap.add_argument("--merged", required=True)
    ap.add_argument("--sources", required=True)
    ap.add_argument("--out")
    args = ap.parse_args(argv)
    try:
        base = load_json(args.baseline, "baseline")
        disp = load_dispositions(args.dispositions, base)
        files, support, problems, mapping = cohort_files(base, disp, args.sources)
        got = cohort.compute(args.merged, args.sources, files, support)
        summaries = file_summaries(args.merged, args.sources)
    except InputError as e:
        print(f"ERROR: {e}", file=sys.stderr)
        return 2
    ref = base["cohort"]["reference"]
    results = {}
    for name, key in (("full", "threshold"), ("runtime", "threshold_runtime")):
        tot = got["totals"][name]
        num, den = ref[key]["numerator"], ref[key]["denominator"]
        c, n = tot["lines_covered"], tot["lines"]
        ok = n > 0 and c * den >= num * n
        results[name] = {"lines": n, "lines_covered": c, "threshold": [num, den], "pass": ok}
        print(f"cohort {name}: lines {c}/{n} ({pct(c, n)}) against {num}/{den} ({pct(num, den)}): "
              f"{'pass' if ok else 'FAIL'}")
        if not ok:
            problems.append(f"FAIL [cohort_{name}] lines {c}/{n} is below {num}/{den} "
                            f"({c} * {den} = {c * den} < {num} * {n} = {num * n})")
    # per-file deltas within the cohort
    rev = {v: k for k, v in mapping.items()}
    base_files = {f["file"]: f for f in base["cohort"]["files"]}
    print("cohort files below their baseline line fraction:")
    below = 0
    for path in files:
        g = got["files"][path]["full"]
        b = base_files.get(rev.get(path))
        if b is None:
            print(f"  {path}: {g['lines_covered']}/{g['lines']} (added; no baseline figure)")
            continue
        if g["lines_covered"] * b["lines"] < b["lines_covered"] * g["lines"]:
            below += 1
            moved = f" (was {rev[path]})" if rev[path] != path else ""
            print(f"  {path}{moved}: {g['lines_covered']}/{g['lines']} ({pct(g['lines_covered'], g['lines'])}), "
                  f"baseline {b['lines_covered']}/{b['lines']} ({pct(b['lines_covered'], b['lines'])})")
    if not below:
        print("  none")
    # groups outside the cohort
    cohort_set = set(files)
    excluded = {e["file"] for e in disp["cohort"]["excluded_additions"]}
    groups = collections.defaultdict(lambda: [0, 0])
    crates = collections.defaultdict(lambda: [0, 0])
    crate_cache = {}
    for rel, (n, c) in summaries.items():
        if rel in cohort_set:
            continue
        if rel.startswith("tests/"):
            grp = "tests_crate"
        elif rel.endswith("restored_tests.rs"):
            grp = "restored_tests"
        elif rel in excluded:
            grp = "fork_test_files"
        else:
            crate = crate_of(rel, args.sources, crate_cache)
            if crate in SEL_CRATES:
                grp = "sel_only_crates"
            else:
                grp = "upstream_product"
                crates[crate][0] += n
                crates[crate][1] += c
        groups[grp][0] += n
        groups[grp][1] += c
    print("groups outside the cohort (reported, never counted in it):")
    for grp in ("restored_tests", "fork_test_files", "tests_crate", "upstream_product", "sel_only_crates"):
        n, c = groups.get(grp, (0, 0))
        print(f"  {grp}: lines {c}/{n} ({pct(c, n)})")
    print("upstream product per crate (candidate against baseline all-files less fork-owned):")
    deltas = {}
    per_crate = base["cohort"].get("per_crate", {})
    for crate in sorted(set(crates) | {k for k, v in per_crate.items() if not v.get("sel_only_crate")}):
        n, c = crates.get(crate, (0, 0))
        b = per_crate.get(crate)
        if b:
            bn = b["all"]["lines"] - ((b.get("fork_owned") or {}).get("lines") or 0)
            bc = b["all"]["lines_covered"] - ((b.get("fork_owned") or {}).get("lines_covered") or 0)
        else:
            bn = bc = 0
        dp = (100.0 * c / n if n else 0.0) - (100.0 * bc / bn if bn else 0.0)
        deltas[crate] = {"lines": n, "lines_covered": c, "baseline_lines": bn, "baseline_lines_covered": bc,
                         "delta_pct_points": round(dp, 2)}
        print(f"  {crate}: {c}/{n} ({pct(c, n)}), baseline {bc}/{bn} ({pct(bc, bn)}), delta {dp:+.2f} points")
    for p in problems:
        print(p)
    print("result:", f"FAIL ({len(problems)} failures)" if problems else "OK")
    if args.out:
        with open(args.out, "w", encoding="utf-8") as f:
            json.dump({"cohort": results, "files": got["files"], "mapping": mapping,
                       "groups": {k: {"lines": v[0], "lines_covered": v[1]} for k, v in groups.items()},
                       "per_crate": deltas, "failures": problems}, f, indent=1, sort_keys=True)
            f.write("\n")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
