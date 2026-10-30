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
     An empty body `{}` has no such region; its one region, spanning both braces, serves instead.
The exit region also counts early returns (rustc routes every `return` through the body's one return
block, whose span is the closing brace); count 2 does not. For a body whose last statement is a loop the
tool also reports `loop`: the region that starts the loop block's first statement (same choice rule),
which counts iterations rather than arrivals. For a function whose last statement is a bare
`async { }` or `async move { }` block (a plain `fn` that returns the future), the tool also reports
`tail`: the block's own closure record (the `_RNC` record whose first region starts at the block's
opening brace) under the same three counts, so a future that was built but never polled shows there.

Every function of `tests/src` in the source tree is evaluated, in every export of the directory; a
function appears in the output when it has a body record in at least one export. A function is
identified by (file, in, name): `in` is its enclosing inline `mod`, `impl` or `trait` header (in angle
brackets) or `fn`, joined by `::`, or null at file level. The line of its `fn` keyword is data, not
identity, so a function keeps its identity when lines above it change. The tool fails when two
functions with a body record share an identity. Among the records of a
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

`self-check` recomputes the coverage-derived parts of the baseline from the exports, the merged export
and the sources, and compares them with what the baseline records:
  - every recorded `last_statement` region (sub-test calls, helper functions, tail blocks), every
    loop-body region, every per-step count and every executed flag, in both directions for helpers
    and tail blocks (a positive body count that the baseline does not record is a mismatch);
  - the coverage-derived fields the gates read: each call's `executed` (per step), `expected_steps` (from its
    mode, env and other conditions, its cfg flag and its suite's steps and their modes), each loop
    body's `positive_at_0_16_27` and `required_in`, each helper's identity data (line, span, async,
    symbol), and the aggregate counts (`subtests.counts` and `helpers.counts`);
  - the uniqueness of every helper identity, in the baseline and in the sources;
  - the cohort: every file's full and runtime-only figures and test-module spans, and both gate
    fractions (`cohort.reference.threshold` and `threshold_runtime`).
It prints each mismatch and exits 1 if there is one.

It does not check the parts of the baseline that come from elsewhere (rechecking them would need the
cargo listings or the scan tooling, which live outside this directory):
  - `libtest` (identifiers, status, CI selection, `net_5b914ddb5`): from the `cargo test ... -- --list`
    and `-- --ignored --list` listings (relisted at 9a7574203 for the tests crate, the unified workspace
    and every CI step; reused from 5b914ddb5 for the per-crate feature sets), the static test scan and
    the 0.16.27 net's steps.tsv;
  - `restored_tests`: from the static scan of the 9a7574203 sources, compared with v0.16.27 and v1.0.1;
  - `dispositions`: from the design's decisions, the SPDX census, steps.tsv and the --ignored listings;
  - the sub-test call list and each call's guards (`mode`, `cfg`, `cfg_enabled_with_default_features`,
    `env`, `other_conditions`, `target`): from the static sub-test scan at 9a7574203 (`expected_steps`
    is checked against them, but they are not rescanned), and `subtests.steps` and `suites`, from
    steps.tsv;
  - `cohort.per_crate`, `cohort.groups`, the files' region counts and `cohort.totals` beyond the two
    fractions: from merged.files.tsv and tables.json;
  - `provenance` and `schema`, which are text.
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
def container_of(containers):
    parts = []
    for kind, name in containers:
        parts.append(name if kind in ("mod", "fn") else "<" + name + ">")
    return "::".join(parts) or None


def identity(file, in_, name):
    return f"{file} | {in_ or '-'} | {name}"


def statement_geo(src, nc, clean, x):
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
    return st


def tail_block(src, nc, clean, x):
    """The geometry of a last statement that is a bare `async { }` or `async move { }` block, else None."""
    m = re.compile(r"async\s+(?:move\s+)?\{").match(clean, x["start"])
    if not m or x["kind"] != "expression":
        return None
    bo = m.end() - 1
    bc = rustscan.match_bracket(clean, bo)
    if bc < 0 or clean[bc + 1:x["end"]].strip() not in ("", ";"):
        return None
    stmts = [y for y in rustscan.statements(clean, bo, bc) if y["kind"] != "item"]
    return {"async": True, "line": rustscan.line_col(src, bo)[0], "anchor": rustscan.line_col(src, x["start"]),
            "open": rustscan.line_col(src, bo), "close": rustscan.line_col(src, bc),
            "stmts": [statement_geo(src, nc, clean, y) for y in stmts]}


def scan_sources(root):
    """Every fn with a body under root/tests/src, as a list of geometries."""
    fns = []
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
                in_ = container_of(g["containers"])
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
                    "key": identity(rel, in_, g["name"]), "file": rel, "in": in_, "name": g["name"],
                    "line": g["fn_pos"][0], "async": g["async"],
                    "containers": ["%s %s" % c for c in g["containers"]],
                    "open": rustscan.line_col(src, g["open"]), "close": rustscan.line_col(src, g["close"]),
                    "last": None, "loop": None, "stmts": [statement_geo(src, nc, clean, x) for x in stmts],
                    "test_attr": test_attr, "tail": tail_block(src, nc, clean, stmts[-1]) if stmts else None,
                }
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
                fns.append(rec)
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
        if not exit_ and not geo["stmts"]:
            # an empty body `{}` has one region spanning both braces, which serves as its exit
            exit_ = [g for g in code if (g[0], g[1]) == open_ and (g[2], g[3]) == (close[0], close[1] + 1)]
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
    """Evaluate every function in every step. Returns (steps, geos keyed by identity). Exits with an
    error when two functions with a body record share an identity."""
    found = scan_sources(root)
    by_file = {}
    for g in found:
        by_file.setdefault(g["file"], []).append(g)
    steps = sorted(os.path.basename(p)[:-5] for p in glob.glob(os.path.join(exports, "*.json")))
    for g in found:
        g["steps"] = {}
        g["tail_steps"] = {}
    for step in steps:
        idx = load_step(os.path.join(exports, step + ".json"), root)
        for rel, recs in idx.items():
            for g in by_file.get(rel, []):
                v = evaluate(g, recs)
                if v is not None:
                    g["steps"][step] = v
                if g["tail"]:
                    t = evaluate(g["tail"], recs)
                    if t is not None:
                        g["tail_steps"][step] = t
        print(f"{step}: {sum(len(v) for v in idx.values())} tests/src records", file=sys.stderr, flush=True)
    geos, dups = {}, {}
    for g in found:
        if g["key"] in geos:
            dups.setdefault(g["key"], [geos[g["key"]]]).append(g)
            if g["steps"] and not geos[g["key"]]["steps"]:
                geos[g["key"]] = g
        else:
            geos[g["key"]] = g
    bad = {k: v for k, v in dups.items() if sum(1 for g in v if g["steps"]) > 1}
    for k, v in dups.items():
        print(f"note: {len(v)} functions share the identity {k}; at most one has a body record", file=sys.stderr)
    if bad:
        for k, v in bad.items():
            print(f"ERROR: functions with a body record share the identity {k} (lines {[g['line'] for g in v]})", file=sys.stderr)
        sys.exit(2)
    return steps, geos


# ------------------------------------------------------------------ self-check
def self_check(args):
    base = json.load(open(args.baseline))
    steps, geos = compute(args.exports, args.sources)
    by_line = {(g["file"], g["line"], g["name"]): g for g in geos.values()}
    problems = []
    checked = {"call_regions": 0, "call_counts": 0, "call_executed": 0, "call_expected_steps": 0,
               "loop_regions": 0, "loop_counts": 0, "loop_derived": 0, "helper_regions": 0, "helper_counts": 0,
               "helper_identity": 0, "helpers": 0, "tail_blocks": 0, "tail_counts": 0, "aggregates": 0}

    def region_of(name, vals, field):
        entered = [v for v in vals if v["body"] > 0]
        regs = {tuple(v[field]) for v in (entered or vals) if v.get(field)}
        if len(regs) > 1:
            problems.append(f"{name}: {field} differs between steps: {sorted(regs)}")
        return list(next(iter(regs))) if regs else None

    def counts(v):
        return [v["body"], v["last"], v["exit"]]

    # ------------------------------------------------ sub-test calls
    sub = base["subtests"]
    step_mode = {st_: v["mode"] for st_, v in sub["steps"].items()}
    derived_exec = {}
    for c in sub["calls"]:
        name = f"call {c['suite']} -> {c['call']}"
        net = sub["suites"][c["suite"]]["net_steps"]
        exp = [st_ for st_ in net if c["mode"] in ("both", step_mode[st_]) and not c["env"]
               and not c["other_conditions"] and c["cfg_enabled_with_default_features"] is not False]
        checked["call_expected_steps"] += 1
        if exp != c["expected_steps"]:
            problems.append(f"{name}: expected_steps {c['expected_steps']} != derived {exp}")
        m = c.get("measured")
        if not m:
            if c.get("executed") is not None:
                problems.append(f"{name}: not measured but executed is {c['executed']}")
            continue
        file, line = m["target"].rsplit(":", 1)
        g = by_line.get((file, int(line), c["call"].split("::")[-1]))
        if g is None:
            problems.append(f"{name}: no function {m['target']} in the sources")
            continue
        vals = list(g["steps"].values())
        rec_ls = m.get("last_statement") or {}
        if "region" in rec_ls:
            checked["call_regions"] += 1
            got = region_of(name, vals, "last_region")
            if got != rec_ls["region"]:
                problems.append(f"{name}: last_statement region {got} != recorded {rec_ls['region']}")
        ex = {}
        for step, v in (m.get("steps") or {}).items():
            checked["call_counts"] += 1
            got = g["steps"].get(step)
            ex[step] = bool(got and got["executed"])
            want = (v["body_count"], v["last_statement_count"], v["final_count"], v["executed"])
            have = (got["body"], got["last"], got["exit"], got["executed"]) if got else (0, None, None, False)
            if got is None and v["body_count"] == 0 and not v["executed"]:
                continue
            if have != want:
                problems.append(f"{name} [{step}]: (body, last, exit, executed) {have} != recorded {want}")
        derived_exec[(c["suite"], c["order"])] = ex
        checked["call_executed"] += 1
        if c.get("executed") != ex:
            problems.append(f"{name}: executed {c.get('executed')} != derived {ex}")
        lb = m.get("loop_body")
        if lb:
            checked["loop_regions"] += 1
            got = region_of(name, vals, "loop_region")
            if got != lb.get("region"):
                problems.append(f"{name}: loop-body region {got} != recorded {lb.get('region')}")
            want_counts = {st_: g["steps"][st_]["loop"] for st_ in ex if st_ in g["steps"]}
            checked["loop_counts"] += len(want_counts)
            if lb.get("counts") != want_counts:
                problems.append(f"{name}: loop-body counts {lb.get('counts')} != derived {want_counts}")
            pos = sorted(st_ for st_, n in want_counts.items() if n)
            req = sorted(st_ for st_ in pos if ex.get(st_))
            checked["loop_derived"] += 1
            if lb.get("positive_at_0_16_27") != pos or lb.get("required_in") != req:
                problems.append(f"{name}: loop-body positive_at_0_16_27 {lb.get('positive_at_0_16_27')} / required_in "
                                f"{lb.get('required_in')} != derived {pos} / {req}")

    # aggregates of the calls
    agg = {"calls": len(sub["calls"]), "measured": sum(1 for c in sub["calls"] if c.get("measured")),
           "added_after_measurement": sum(1 for c in sub["calls"] if c.get("added_after_measurement")),
           "executed_in_at_least_one_step": sum(1 for ex in derived_exec.values() if any(ex.values()))}
    per_step = {}
    for ex in derived_exec.values():
        for st_, v in ex.items():
            per_step[st_] = per_step.get(st_, 0) + int(v)
    agg["executed_per_step"] = per_step
    per_suite = {}
    for suite, info in sub["suites"].items():
        cs = [c for c in sub["calls"] if c["suite"] == suite]
        d = {"calls": len(cs)}
        for st_ in info["net_steps"]:
            d[st_] = sum(1 for c in cs if derived_exec.get((suite, c["order"]), {}).get(st_))
        per_suite[suite] = d
    agg["per_suite"] = per_suite
    for k, v in agg.items():
        checked["aggregates"] += 1
        if sub["counts"].get(k) != v:
            problems.append(f"subtests.counts.{k} {sub['counts'].get(k)} != derived {v}")

    # ------------------------------------------------ helper functions
    hsec = base.get("helpers", {})
    helpers = hsec.get("functions", [])
    seen = {}
    for h in helpers:
        k = identity(h["file"], h.get("in"), h["name"])
        if k in seen:
            problems.append(f"helper {k}: two baseline entries share this identity")
        seen[k] = h
    for k, h in seen.items():
        checked["helpers"] += 1
        g = geos.get(k)
        if g is None:
            problems.append(f"helper {k}: not in the sources")
            continue
        vals = list(g["steps"].values())
        checked["helper_identity"] += 1
        ex_steps = [st_ for st_ in steps if g["steps"].get(st_, {}).get("executed")]
        en_steps = [st_ for st_ in steps if st_ in g["steps"] and g["steps"][st_]["body"] > 0 and not g["steps"][st_]["executed"]]
        pick = [g["steps"][st_] for st_ in ex_steps] or [g["steps"][st_] for st_ in en_steps] or vals
        ident_have = (g["line"], [g["line"], g["close"][0]], g["async"], pick[0]["symbol"] if pick else None)
        ident_want = (h.get("line"), h.get("span"), h.get("async"), h.get("symbol"))
        if ident_have != ident_want:
            problems.append(f"helper {k}: (line, span, async, symbol) {ident_want} != derived {ident_have}")
        checked["helper_regions"] += 1
        got = region_of(k, vals, "last_region")
        if got != h.get("last_statement"):
            problems.append(f"helper {k}: last_statement region {got} != recorded {h.get('last_statement')}")
        for field, want_ex in (("executed", True), ("entered_not_executed", False)):
            have = {st_: counts(g["steps"][st_]) for st_ in (ex_steps if want_ex else en_steps)}
            checked["helper_counts"] += len(have)
            if (h.get(field) or {}) != have:
                problems.append(f"helper {k}: {field} {h.get(field) or {}} != derived {have}")
        tb = h.get("tail_block")
        if g["tail"] is None:
            if tb is not None:
                problems.append(f"helper {k}: tail_block recorded but the function has no tail block")
            continue
        checked["tail_blocks"] += 1
        tvals = list(g["tail_steps"].values())
        if not tvals:
            if tb is not None:
                problems.append(f"helper {k}: tail_block recorded but no tail-block record found")
            continue
        if tb is None:
            problems.append(f"helper {k}: tail block with a record but no tail_block recorded")
            continue
        t_ex = [st_ for st_ in steps if g["tail_steps"].get(st_, {}).get("executed")]
        t_en = [st_ for st_ in steps if st_ in g["tail_steps"] and g["tail_steps"][st_]["body"] > 0 and not g["tail_steps"][st_]["executed"]]
        tpick = [g["tail_steps"][st_] for st_ in t_ex] or [g["tail_steps"][st_] for st_ in t_en] or tvals
        t_have = (list(g["tail"]["open"]), tpick[0]["symbol"])
        if (tb.get("open"), tb.get("symbol")) != t_have:
            problems.append(f"helper {k}: tail_block (open, symbol) {(tb.get('open'), tb.get('symbol'))} != derived {t_have}")
        got = region_of(k + " tail", tvals, "last_region")
        if got != tb.get("last_statement"):
            problems.append(f"helper {k}: tail_block last_statement region {got} != recorded {tb.get('last_statement')}")
        for field, want_steps in (("executed", t_ex), ("entered_not_executed", t_en)):
            have = {st_: counts(g["tail_steps"][st_]) for st_ in want_steps}
            checked["tail_counts"] += len(have)
            if (tb.get(field) or {}) != have:
                problems.append(f"helper {k}: tail_block {field} {tb.get(field) or {}} != derived {have}")
    if helpers:
        for k, g in geos.items():
            if k not in seen and g["steps"]:
                problems.append(f"helper {k}: has a body record but is not in the baseline")
        hc = hsec.get("counts", {})
        with_rec = [g for g in geos.values() if g["steps"]]
        hp = {}
        for g in with_rec:
            for st_, v in g["steps"].items():
                if v["executed"]:
                    hp[st_] = hp.get(st_, 0) + 1
        tp = {}
        for g in with_rec:
            for st_, v in g["tail_steps"].items():
                if v["executed"]:
                    tp[st_] = tp.get(st_, 0) + 1
        derived = {
            "functions_in_sources": len(geos),
            "with_a_body_record": len(with_rec),
            "executed_in_at_least_one_step": sum(1 for g in with_rec if any(v["executed"] for v in g["steps"].values())),
            "executed_per_step": hp,
            "no_body_record": sorted(([g["file"], g["in"], g["name"], g["line"]] for g in geos.values() if not g["steps"]),
                                    key=lambda x: (x[0], x[1] or "", x[2], x[3])),
            "tail_blocks": sum(1 for g in with_rec if g["tail"] and g["tail_steps"]),
            "tail_blocks_executed_per_step": tp,
        }
        for k, v in derived.items():
            checked["aggregates"] += 1
            if hc.get(k) != v:
                problems.append(f"helpers.counts.{k} {hc.get(k)} != derived {v}")

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
        out = {"steps": steps, "functions": [geos[k] for k in sorted(geos, key=lambda k: (geos[k]["file"], geos[k]["in"] or "", geos[k]["name"]))]}
        text = json.dumps(out, indent=1, sort_keys=True)
        if args.out:
            open(args.out, "w").write(text + "\n")
        else:
            print(text)
        return 0
    return self_check(args)


if __name__ == "__main__":
    sys.exit(main())
