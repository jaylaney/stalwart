#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
"""The executed-sub-test rule of the C10 gates (rulings 7 and 31), computed from llvm-cov exports.

A function counts as executed in a step only if one function record of that step's export has three
positive counts:
  1. body: the function's body record. For an `async fn`, the async body's own record: a closure record
     (v0 symbol prefix `_RNC`) whose first region starts exactly at the body's opening brace. The outer
     record, whose single region covers the signature, only constructs the future and never counts.
     For a plain `fn`, the function's own record: not a closure, first region starting on the line of
     the `fn` keyword.
  2. last: the code region that starts the body's last top-level statement (rustscan.statements): the
     innermost code region of the record that contains the statement's anchor, or else the earliest code
     region that starts between the anchor and the statement's end; of regions with that same start, the
     one with the highest count (a macro maps its whole expansion onto the call site, and the region
     that starts the statement runs on every arrival). The anchor is the statement's first
     token; for a `for` loop, the first token of the iterator expression; for `let`, `if let` and
     `while let`, the first token of the scrutinee after `=` (a pattern's region counts iterations or
     matches, not arrivals). The exit region is never
     chosen. A statement with no code region in the record (one compiled out by a `#[cfg]`, or a tail
     `async move { }` block whose code is the block's own record) is passed over for the statement
     before it; `last_index_from_end` says how many were passed over. A body with no statement that
     has a code region (an empty body, among others) has no count 2, and it is not required.
  3. exit: the code region that starts exactly at the body's closing brace and ends on the same line.
The exit region also counts early returns (rustc routes every `return` through the body's one return
block, whose span is the closing brace); count 2 does not. For a body whose last statement is a loop the
tool also reports `loop`: the region that starts the loop block's first statement (same choice rule),
which counts iterations rather than arrivals.

Every function of `tests/src` in the source tree is evaluated, in every export of the directory; a
function appears in the output when it has a body record in at least one export. Among the records of a
step (both test binaries, every generic instantiation), the reported one is the first by: executed,
then the two counts body and exit, then the largest body count, then the symbol (so a step where no
record was entered reports the same record in every step).

Usage:
  strict_rule.py functions --exports DIR --sources DIR [--out PATH]
  strict_rule.py cohort --merged PATH --sources DIR --baseline PATH [--out PATH]
  strict_rule.py self-check --baseline PATH --exports DIR --merged PATH --sources DIR

  --exports  a directory of `llvm-cov export` JSON files, one per step, named <step>.json
  --merged   the `llvm-cov export` JSON of every step's profiles merged
  --sources  a source tree holding tests/src and crates at the measured commit, e.g.
             `git archive <sha> tests crates | tar -x -C DIR`
  --baseline tests/qualification/baseline-0.16.27.json

`cohort` computes the fork-owned cohort's full and runtime-only coverage (cohort.py) for the files
the baseline's `cohort.files` lists, the test-support files among them being those it marks
`runtime_excluded`.

`self-check` recomputes every function and compares it with the baseline: every recorded
`last_statement` region (sub-test calls and helper functions), every recorded loop-body region, and
every recorded per-step count and executed flag. It also recomputes the cohort and compares every
file's full and runtime-only figures and test-module spans, and both gate fractions
(`cohort.reference.threshold` and `threshold_runtime`). It prints each mismatch and exits 1 if there
is one.
"""
import argparse
import glob
import json
import os
import re
import sys

sys.dont_write_bytecode = True  # keep the committed tools directory free of __pycache__
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cohort  # noqa: E402
import rustscan  # noqa: E402

TESTS_SRC = "tests/src/"


def is_closure(symbol):
    return symbol.startswith("_RNC") or "{{closure}}" in symbol


# ------------------------------------------------------------------ sources
def scan_sources(root):
    """Every fn with a body under root/tests/src: key -> geometry."""
    fns = {}
    for dp, dn, fn in os.walk(os.path.join(root, TESTS_SRC)):
        dn.sort()
        for f in sorted(fn):
            if not f.endswith(".rs"):
                continue
            path = os.path.join(dp, f)
            rel = os.path.relpath(path, root).replace(os.sep, "/")
            src = open(path, encoding="utf-8").read()
            nc, clean = rustscan.blank_comments_and_literals(src)
            for g in rustscan.functions(nc, clean):
                key = f"{rel}:{g['fn_pos'][0]}:{g['name']}"
                head = clean[:clean.rfind("fn", 0, g["open"])].rstrip()
                attrs = []
                while True:
                    prev = None
                    while prev != head:
                        prev = head
                        head = re.sub(r"(\bpub(\s*\([^)]*\))?|\basync|\bunsafe|\bconst|\bextern(\s+\S+)?)\s*$", "", head).rstrip()
                    if not head.endswith("]"):
                        break
                    j = head.rfind("#[")
                    if j < 0:
                        break
                    attrs.append(" ".join(nc[j + 2:len(head) - 1].split()))
                    head = head[:j].rstrip()
                test_attr = next((a for a in attrs if re.match(r"^(tokio::)?test\b", a)), None)
                ls = rustscan.last_statement(clean, g["open"], g["close"])
                stmts = [x for x in rustscan.statements(clean, g["open"], g["close"]) if x["kind"] != "item"]
                rec = {
                    "key": key, "file": rel, "name": g["name"], "line": g["fn_pos"][0], "async": g["async"],
                    "containers": ["%s %s" % c for c in g["containers"]],
                    "open": rustscan.line_col(src, g["open"]), "close": rustscan.line_col(src, g["close"]),
                    "last": None, "loop": None, "stmts": [], "test_attr": test_attr,
                }
                for x in stmts:
                    st = {"start": rustscan.line_col(src, x["start"]), "anchor": rustscan.line_col(src, x["anchor"]),
                          "end": rustscan.line_col(src, x["end"]), "kind": x["kind"], "expression_tail": x["tail"],
                          "text": " ".join(nc[x["start"]:x["end"]].split())[:100], "loop": None}
                    if x["kind"] == "loop" and x["loop_block"]:
                        bo, bc = x["loop_block"]
                        first = rustscan.first_statement(clean, bo, bc)
                        if first:
                            st["loop"] = {"block_open": rustscan.line_col(src, bo), "block_close": rustscan.line_col(src, bc),
                                          "anchor": rustscan.line_col(src, first["anchor"]), "end": rustscan.line_col(src, first["end"]),
                                          "text": " ".join(nc[first["start"]:first["end"]].split())[:100]}
                    rec["stmts"].append(st)
                if ls:
                    rec["last"] = {"start": rustscan.line_col(src, ls["start"]), "anchor": rustscan.line_col(src, ls["anchor"]),
                                   "end": rustscan.line_col(src, ls["end"]), "kind": ls["kind"], "expression_tail": ls["tail"],
                                   "text": " ".join(nc[ls["start"]:ls["end"]].split())[:100]}
                    if ls["kind"] == "loop" and ls["loop_block"]:
                        bo, bc = ls["loop_block"]
                        first = rustscan.first_statement(clean, bo, bc)
                        if first:
                            rec["loop"] = {"block_open": rustscan.line_col(src, bo), "block_close": rustscan.line_col(src, bc),
                                           "anchor": rustscan.line_col(src, first["anchor"]), "end": rustscan.line_col(src, first["end"]),
                                           "text": " ".join(nc[first["start"]:first["end"]].split())[:100]}
                fns[key] = rec
    return fns


# ------------------------------------------------------------------ exports
def rel_path(filename, root, cache):
    """Map an absolute export path to the tests/src path that exists under the source root."""
    if filename in cache:
        return cache[filename]
    parts = filename.replace("\\", "/").split("/")
    out = None
    for i in range(len(parts)):
        cand = "/".join(parts[i:])
        if cand.startswith(TESTS_SRC) and os.path.isfile(os.path.join(root, cand)):
            out = cand
            break
    cache[filename] = out
    return out


def load_step(path, root):
    d = json.load(open(path))["data"][0]
    cache, idx = {}, {}
    for fn in d["functions"]:
        regs = fn["regions"]
        if not regs:
            continue
        fid = regs[0][5]
        rel = rel_path(fn["filenames"][fid], root, cache)
        if rel is None:
            continue
        own = [r for r in regs if r[5] == fid]
        idx.setdefault(rel, []).append({"symbol": fn["name"], "count": fn["count"], "regions": own})
    return idx


def pick_region(code, anchor, end):
    """Among regions containing the anchor, those with the latest start; else, among regions starting
    between the anchor and the end, those with the earliest start. Of those, the highest count, then the
    smallest extent: a macro call maps every region of its expansion onto the call site, and the one
    that starts the statement runs on every arrival, so no region with the same start counts more."""
    inside = [g for g in code if (g[0], g[1]) <= anchor < (g[2], g[3])]
    if inside:
        top = max((g[0], g[1]) for g in inside)
        cand = [g for g in inside if (g[0], g[1]) == top]
    else:
        after = [g for g in code if anchor <= (g[0], g[1]) < end]
        if not after:
            return None
        top = min((g[0], g[1]) for g in after)
        cand = [g for g in after if (g[0], g[1]) == top]
    return max(cand, key=lambda g: (g[4], -g[2], -g[3]))


def evaluate(geo, records):
    open_, close = tuple(geo["open"]), tuple(geo["close"])
    if geo["async"]:
        body = [r for r in records if is_closure(r["symbol"]) and (r["regions"][0][0], r["regions"][0][1]) == open_]
    else:
        body = [r for r in records if not is_closure(r["symbol"]) and r["regions"][0][0] == geo["line"]]
    best = None
    for r in body:
        code = [g for g in r["regions"] if g[7] == 0]
        exit_ = [g for g in code if (g[0], g[1]) == close and g[2] == close[0]]
        rest = [g for g in code if (g[0], g[1]) != close]
        last, used = None, None
        for k in range(len(geo["stmts"]) - 1, -1, -1):
            st = geo["stmts"][k]
            last = pick_region(rest, tuple(st["anchor"]), tuple(st["end"]))
            if last:
                used = k
                break
        st = geo["stmts"][used] if used is not None else None
        loop = pick_region(rest, tuple(st["loop"]["anchor"]), tuple(st["loop"]["end"])) if st and st["loop"] else None
        v = {
            "symbol": r["symbol"],
            "body": r["count"],
            "last": last[4] if last else None,
            "exit": exit_[0][4] if exit_ else None,
            "last_region": last[:4] if last else None,
            "exit_region": exit_[0][:4] if exit_ else None,
            "loop": loop[4] if loop else None,
            "loop_region": loop[:4] if loop else None,
            "last_index_from_end": (len(geo["stmts"]) - 1 - used) if used is not None else None,
        }
        two = bool(v["body"] > 0 and v["exit"])
        v["executed"] = bool(two and (used is None or v["last"]))
        rank = (v["executed"], two, v["body"] > 0, v["body"])
        if best is None or rank > best[0] or (rank == best[0] and r["symbol"] < best[1]["symbol"]):
            best = (rank, v)
    return best[1] if best else None


def compute(exports, root):
    geos = scan_sources(root)
    by_file = {}
    for g in geos.values():
        by_file.setdefault(g["file"], []).append(g)
    steps = sorted(os.path.basename(p)[:-5] for p in glob.glob(os.path.join(exports, "*.json")))
    for g in geos.values():
        g["steps"] = {}
    for step in steps:
        idx = load_step(os.path.join(exports, step + ".json"), root)
        for rel, recs in idx.items():
            for g in by_file.get(rel, []):
                v = evaluate(g, recs)
                if v is not None:
                    g["steps"][step] = v
        print(f"{step}: {sum(len(v) for v in idx.values())} tests/src records", file=sys.stderr, flush=True)
    return steps, geos


# ------------------------------------------------------------------ self-check
def self_check(args):
    base = json.load(open(args.baseline))
    steps, geos = compute(args.exports, args.sources)
    problems, checked = [], {"call_regions": 0, "call_counts": 0, "loop_regions": 0, "loop_counts": 0,
                              "helper_regions": 0, "helper_counts": 0, "helpers": 0}

    def region_of(g, field):
        entered = [v for v in g["steps"].values() if v["body"] > 0]
        regs = {tuple(v[field]) for v in (entered or g["steps"].values()) if v.get(field)}
        if len(regs) > 1:
            problems.append(f"{g['key']}: {field} differs between steps: {sorted(regs)}")
        return list(next(iter(regs))) if regs else None

    for c in base["subtests"]["calls"]:
        m = c.get("measured")
        if not m:
            continue
        file, line = m["target"].rsplit(":", 1)
        key = f"{file}:{line}:{c['call'].split('::')[-1]}"
        g = geos.get(key)
        if g is None:
            problems.append(f"call {c['suite']} -> {c['call']}: no function {key} in the sources")
            continue
        rec_ls = m.get("last_statement") or {}
        if "region" in rec_ls:
            checked["call_regions"] += 1
            got = region_of(g, "last_region")
            if got != rec_ls["region"]:
                problems.append(f"call {c['suite']} -> {c['call']}: last_statement region {got} != recorded {rec_ls['region']}")
        for step, v in (m.get("steps") or {}).items():
            checked["call_counts"] += 1
            got = g["steps"].get(step)
            want = (v["body_count"], v["last_statement_count"], v["final_count"], v["executed"])
            have = (got["body"], got["last"], got["exit"], got["executed"]) if got else (0, None, None, False)
            if got is None and v["body_count"] == 0 and not v["executed"]:
                continue
            if have != want:
                problems.append(f"call {c['suite']} -> {c['call']} [{step}]: (body, last, exit, executed) {have} != recorded {want}")
        lb = m.get("loop_body")
        if lb:
            checked["loop_regions"] += 1
            got = region_of(g, "loop_region")
            if got != lb.get("region"):
                problems.append(f"call {c['suite']} -> {c['call']}: loop-body region {got} != recorded {lb.get('region')}")
            for step, cnt in (lb.get("counts") or {}).items():
                checked["loop_counts"] += 1
                have = g["steps"].get(step, {}).get("loop")
                if have != cnt:
                    problems.append(f"call {c['suite']} -> {c['call']} [{step}]: loop-body count {have} != recorded {cnt}")

    helpers = base.get("helpers", {}).get("functions", [])
    seen = set()
    for h in helpers:
        checked["helpers"] += 1
        key = h["key"]
        seen.add(key)
        g = geos.get(key)
        if g is None:
            problems.append(f"helper {key}: not in the sources")
            continue
        if "last_statement" in h:
            checked["helper_regions"] += 1
            got = region_of(g, "last_region")
            if got != h["last_statement"]:
                problems.append(f"helper {key}: last_statement region {got} != recorded {h['last_statement']}")
        recorded = {}
        for field in ("executed", "entered_not_executed"):
            for step, counts in (h.get(field) or {}).items():
                recorded[step] = (counts, field == "executed")
        for step in steps:
            got = g["steps"].get(step)
            if step in recorded:
                checked["helper_counts"] += 1
                counts, ex = recorded[step]
                have = ([got["body"], got["last"], got["exit"]], got["executed"]) if got else (None, False)
                if have != (counts, ex):
                    problems.append(f"helper {key} [{step}]: {have} != recorded {(counts, ex)}")
            elif got and got["body"] > 0:
                problems.append(f"helper {key} [{step}]: body count {got['body']} but nothing recorded")
    if helpers:
        for key, g in geos.items():
            if key not in seen and g["steps"]:
                problems.append(f"helper {key}: has a body record but is not in the baseline")

    cohort_problems, cohort_checked = check_cohort(base, args.merged, args.sources)
    problems += cohort_problems
    checked.update(cohort_checked)

    for p in problems:
        print("MISMATCH", p)
    print("checked:", json.dumps(checked, sort_keys=True))
    print("result:", "FAIL (%d mismatches)" % len(problems) if problems else "OK")
    return 1 if problems else 0


def cohort_inputs(base):
    files = [f["file"] for f in base["cohort"]["files"]]
    support = {f["file"] for f in base["cohort"]["files"] if f.get("runtime_excluded")}
    return files, support


def check_cohort(base, merged, sources):
    files, support = cohort_inputs(base)
    got = cohort.compute(merged, sources, files, support)
    problems, checked = [], {"cohort_files": 0, "cohort_fractions": 0}
    for f in base["cohort"]["files"]:
        checked["cohort_files"] += 1
        g = got["files"][f["file"]]
        full = {k: f[k] for k in ("lines", "lines_covered", "functions", "functions_covered")}
        for name, have, want in (("full", g["full"], full), ("runtime", g["runtime"], f["runtime"]),
                                 ("cfg_test_modules", g["cfg_test_modules"], f["cfg_test_modules"])):
            if have != want:
                problems.append(f"cohort {f['file']}: {name} {have} != recorded {want}")
    ref = base["cohort"]["reference"]
    for name, tot in (("threshold", got["totals"]["full"]), ("threshold_runtime", got["totals"]["runtime"])):
        checked["cohort_fractions"] += 1
        have = (tot["lines_covered"], tot["lines"])
        want = (ref[name]["numerator"], ref[name]["denominator"])
        if have != want:
            problems.append(f"cohort {name}: {have[0]}/{have[1]} != recorded {want[0]}/{want[1]}")
        else:
            print(f"cohort {name}: {have[0]}/{have[1]}")
    return problems, checked


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    f = sub.add_parser("functions", help="compute the three counts of every tests/src function per step")
    f.add_argument("--exports", required=True)
    f.add_argument("--sources", required=True)
    f.add_argument("--out")
    c = sub.add_parser("cohort", help="compute the cohort's full and runtime-only coverage from a merged export")
    c.add_argument("--merged", required=True)
    c.add_argument("--sources", required=True)
    c.add_argument("--baseline", required=True)
    c.add_argument("--out")
    s = sub.add_parser("self-check", help="reproduce the baseline's recorded regions, counts and cohort fractions")
    s.add_argument("--baseline", required=True)
    s.add_argument("--exports", required=True)
    s.add_argument("--merged", required=True)
    s.add_argument("--sources", required=True)
    args = ap.parse_args()
    if args.cmd == "cohort":
        files, support = cohort_inputs(json.load(open(args.baseline)))
        got = cohort.compute(args.merged, args.sources, files, support)
        text = json.dumps(got, indent=1, sort_keys=True)
        if args.out:
            open(args.out, "w").write(text + "\n")
        for name, tot in (("full", got["totals"]["full"]), ("runtime", got["totals"]["runtime"])):
            print(f"{name}: lines {tot['lines_covered']}/{tot['lines']}, functions {tot['functions_covered']}/{tot['functions']}")
        return 0
    if args.cmd == "functions":
        steps, geos = compute(args.exports, args.sources)
        out = {"steps": steps, "functions": [geos[k] for k in sorted(geos)]}
        text = json.dumps(out, indent=1, sort_keys=True)
        if args.out:
            open(args.out, "w").write(text + "\n")
        else:
            print(text)
        return 0
    return self_check(args)


if __name__ == "__main__":
    sys.exit(main())
