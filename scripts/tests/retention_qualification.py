#!/usr/bin/env python3
"""Disposable post-gate comparison evidence for issue #86."""
import argparse
import csv
import json
import os
import re
import subprocess
import sys
import tempfile
from collections import defaultdict
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_coverage
import coverage_net
import gate_common


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def runnable_ids(text):
    """Keep package and Cargo target labels, dropping runner paths and binary hashes."""
    found = set()
    target = None
    for line in text.splitlines():
        if match := gate_common.RUNNING.match(line):
            target = (gate_common.exe_package(match.group(2)), match.group(1))
        elif gate_common.DOCTESTS.match(line):
            target = None
        elif target and (match := gate_common.ENTRY.match(line)):
            found.add((*target, match.group(1), match.group(2)))
    return sorted(found)


def selectors(out):
    # This runs strictly after measured coverage and removes the listing counters.
    data = {}
    with tempfile.TemporaryDirectory(dir=out) as directory:
        work = Path(directory)
        net = coverage_net.Net(work)
        try:
            skips = net.tests_crate_skips()
            for step in coverage_net.COVERAGE_STEPS:
                spec = coverage_net.STEPS[step]
                cmd = [*spec["cmd"], *(skips if spec.get("skip_tests_crate") else ())]
                if "--" not in cmd:
                    cmd.append("--")
                data[step] = {"command": cmd, "environment": spec["env"]}
                for label, extra in (("listed", []), ("ignored", ["--ignored"])):
                    path = work / "catalog.log"
                    with path.open("wb") as output:
                        result = net.run(cmd + [*extra, "--list"], env=net.env(spec["env"]),
                                         output=output, timeout=120)
                    if result.returncode != 0:
                        raise ValueError(f"{step}: {label} catalog failed")
                    listing = path.read_text(encoding="utf-8", errors="replace")
                    if re.search(r"^\s*(Compiling|Dirty)\s", listing, re.M):
                        raise ValueError(f"{step}: comparison catalog recompiled a binary")
                    data[step][label] = runnable_ids(listing)
                    if label == "listed" and not data[step][label]:
                        raise ValueError(f"{step}: comparison catalog found no test IDs")
        finally:
            net.clear_profiles()
    return data


def listings(reports):
    out = {}
    for mode, ignored in (("list", False), ("ign", True)):
        all_captures = gate_common.load_captures(reports / "listings" / mode, mode, ignored)
        out[mode] = {
            name: {
                "selected": sorted(result["selected"]),
                "universe": sorted(result["universe"]),
            }
            for name, result in all_captures.items()
        }
    return out


def coverage(reports, sources):
    export = reports / "net/merged.json"
    chart = load(export)["data"][0]
    summaries = check_coverage.file_summaries(str(export), str(sources))
    merged_lines = defaultdict(set)
    for source in chart["files"]:
        rel = next((path for path in re.split(r"(?=/crates/|/tests/)", source["filename"])
                    if path.lstrip("/").startswith(("crates/", "tests/"))
                    and path.lstrip("/") in summaries), None)
        if rel is not None:
            merged_lines[rel.lstrip("/")].update(
                line for line, count in check_coverage.cohort.line_stats(source["segments"]).items()
                if count > 0
            )
    return {
        "gate": load(reports / "coverage.json"),
        "file_summaries": summaries,
        "covered_source_lines": {name: sorted(merged_lines[name]) for name in summaries},
    }


def functions(reports):
    document = load(reports / "functions.json")
    # Keep the strict rule's exact entry/completion/loop evidence. Different
    # nonzero counts can be investigated without masking changed requirements.
    return {
        "steps": sorted(document["steps"]),
        "functions": {
            f["key"]: {"steps": f["steps"], "tail_steps": f["tail_steps"]}
            for f in document["functions"]
        },
    }


def attempt_records(paths):
    records = defaultdict(list)
    for path in paths:
        with path.open(newline="", encoding="utf-8") as source:
            for row in csv.DictReader(source, delimiter="\t"):
                if row["step"] not in coverage_net.COVERAGE_STEPS:
                    continue
                records[row["step"]].append({
                    name: row[name] for name in ("attempt", "command", "environment", "exit_status",
                                                 "passed", "failed", "ignored", "failing_tests")
                })
    return dict(records)


def save_comparison(args):
    args.out.mkdir(parents=True, exist_ok=False)
    reports = args.reports
    actual = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if actual != os.environ["GITHUB_SHA"]:
        raise ValueError("comparison checkout differs from its run commit")
    paths = ([reports / "net/steps.tsv"] if args.serial
             else [args.attempt_dir / name / "steps.tsv" for name in
                   coverage_net.ci_steps.COVERAGE_SHARDS])
    facts = {
        "commit": actual,
        "inventory": listings(reports),
        "strict": functions(reports),
        "coverage": coverage(reports, reports / "src"),
        "attempts": attempt_records(paths),
        "coverage_selectors": selectors(args.out),
    }
    (args.out / "comparison.json").write_text(
        json.dumps(facts, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8"
    )


def compare(left, right):
    labels = ("coverage_selectors", "inventory", "strict", "coverage", "attempts")
    differences = {}
    for name in labels:
        a, b = left[name], right[name]
        differences[name] = sorted(key for key in a.keys() | b.keys() if a.get(key) != b.get(key))
    return differences


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    capture = commands.add_parser("capture")
    capture.add_argument("--reports", type=Path, required=True)
    capture.add_argument("--out", type=Path, required=True)
    kind = capture.add_mutually_exclusive_group(required=True)
    kind.add_argument("--serial", action="store_true")
    kind.add_argument("--attempt-dir", type=Path)
    diff = commands.add_parser("compare")
    diff.add_argument("left", type=Path)
    diff.add_argument("right", type=Path)
    args = parser.parse_args()
    if args.command == "capture":
        save_comparison(args)
        return 0
    left, right = load(args.left), load(args.right)
    if left["commit"] != right["commit"]:
        raise ValueError("the comparison files are from different commits")
    changes = compare(left, right)
    print(json.dumps(changes, indent=2, sort_keys=True))
    return int(any(changes.values()))


if __name__ == "__main__":
    sys.exit(main())
