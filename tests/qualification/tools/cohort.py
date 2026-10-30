# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
"""Fork-owned cohort coverage, full and runtime-only (rulings 8 and 31), from a merged llvm-cov export.

The figures follow llvm-cov's report, which is what `llvm-cov export`'s file summaries hold:
  - a file's functions are its instantiation groups: the function records whose main file is the file,
    grouped by the start (line, column) of their first region; a group is covered when any of its
    records has a positive count;
  - a file's lines are the sum over those groups of each group's line coverage. Per record, segments
    are built from the record's regions of its main file as llvm's SegmentBuilder builds them, and lines
    are counted as llvm's LineCoverageIterator and LineCoverageStats count them; per group, the lines
    and covered lines are each the maximum over its records.
The runtime-only figure drops the groups that start inside an inline module whose cfg needs `test`
(from the module's first attribute line to its closing brace), and drops the fork's test-support files
whole.
"""
import collections
import json
import os
import re

import rustscan


# ------------------------------------------------------------------ cfg evaluation
def _split_args(s):
    parts, depth, cur, instr = [], 0, "", False
    for ch in s:
        if ch == '"':
            instr = not instr
        if not instr:
            if ch == "(":
                depth += 1
            elif ch == ")":
                depth -= 1
            elif ch == "," and depth == 0:
                parts.append(cur.strip())
                cur = ""
                continue
        cur += ch
    if cur.strip():
        parts.append(cur.strip())
    return parts


def _eval(e, test):
    """cfg predicate with every feature on; None when unknown."""
    e = e.strip()
    for op in ("all", "any", "not"):
        if e.startswith(op + "(") and e.endswith(")"):
            args = [_eval(x, test) for x in _split_args(e[len(op) + 1:-1])]
            if op == "not":
                return None if args[0] is None else (not args[0])
            if op == "all":
                if any(a is False for a in args):
                    return False
                return None if any(a is None for a in args) else True
            if any(a is True for a in args):
                return True
            return None if any(a is None for a in args) else False
    if e == "test":
        return test
    if re.match(r'^feature\s*=\s*"[^"]+"$', e):
        return True
    return None


def needs_test(attr):
    """True when a `cfg(...)` attribute is off whenever `test` is off, whatever the features."""
    if not attr.startswith("cfg(") or not attr.endswith(")"):
        return False
    return _eval(attr[4:-1], test=False) is False


def test_modules(path):
    """Outermost inline modules whose cfg needs `test`: [{module, attrs, lines: [first, last]}]."""
    src = open(path, encoding="utf-8").read()
    nc, clean = rustscan.blank_comments_and_literals(src)
    spans = []
    for m in re.finditer(r"\bmod\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{", clean):
        o = clean.find("{", m.start())
        c = rustscan.match_bracket(clean, o)
        head = re.sub(r"\bpub(\s*\([^)]*\))?\s*$", "", clean[:m.start()].rstrip()).rstrip()
        attrs, first = [], m.start()
        while head.endswith("]"):
            j = head.rfind("#[")
            if j < 0:
                break
            attrs.append(" ".join(nc[j + 2:len(head) - 1].split()))
            first = j
            head = head[:j].rstrip()
        if any(needs_test(a) for a in attrs):
            spans.append({"module": m.group(1), "attrs": attrs,
                          "lines": [rustscan.line_col(clean, first)[0], rustscan.line_col(clean, c)[0]]})
    spans.sort(key=lambda s: s["lines"])
    outer = []
    for s in spans:
        if outer and outer[-1]["lines"][0] <= s["lines"][0] and s["lines"][1] <= outer[-1]["lines"][1]:
            continue
        outer.append(s)
    return outer


# ------------------------------------------------------------------ llvm-cov line coverage
def build_segments(regions):
    """llvm::coverage SegmentBuilder::buildSegments over one record's regions of its main file.
    Region: [l1, c1, l2, c2, count, fileID, expandedFileID, kind] (kinds: 0 code, 1 expansion,
    2 skipped, 3 gap). Segment: (line, col, count, has_count, is_region_entry, is_gap)."""
    R = sorted(([r[0], r[1], r[2], r[3], r[4], r[7]] for r in regions), key=lambda r: ((r[0], r[1]), (-r[2], -r[3]), r[5]))
    comb = []
    for r in R:
        if comb and comb[-1][:4] == r[:4]:
            if r[5] == comb[-1][5]:
                comb[-1][4] += r[4]
            continue
        comb.append(list(r))
    segs, active = [], []

    def start(r):
        return (r[0], r[1])

    def end(r):
        return (r[2], r[3])

    def seg(r, loc, entry, emit_skipped=False):
        has = not emit_skipped and r[5] != 2
        if segs and not entry and not emit_skipped:
            last = segs[-1]
            if last[3] == has and last[2] == r[4] and not last[4]:
                return
        segs.append((loc[0], loc[1], r[4], True, entry, r[5] == 3) if has else (loc[0], loc[1], 0, False, entry, False))

    def complete(loc, first):
        active[first:] = sorted(active[first:], key=end)
        for i in range(first + 1, len(active)):
            cr, cl = active[i], end(active[i - 1])
            if loc is not None and cl == loc:
                break
            if cl == end(cr):
                continue
            for j in range(i + 1, len(active)):
                if end(cr) == end(active[j]):
                    cr = active[j]
            seg(cr, cl, False)
        last = active[-1]
        if first and end(last) != loc:
            seg(active[first - 1], end(last), False)
        elif not first and (loc is None or loc != end(last)):
            seg(last, end(last), False, True)
        del active[first:]

    for idx, r in enumerate(comb):
        cur = start(r)
        keep = [a for a in active if not (end(a) <= cur)]
        done = [a for a in active if end(a) <= cur]
        if done:
            active[:] = keep + done
            complete(cur, len(keep))
        gap = r[5] == 3
        if cur == end(r):
            skipped = idx + 1 == len(comb) or r[5] == 2
            seg(active[-1] if active else r, cur, not gap, skipped)
            if skipped and active:
                seg(active[-1], cur, False)
            continue
        if idx + 1 == len(comb) or cur != start(comb[idx + 1]):
            seg(r, cur, not gap)
        active.append(r)
    if active:
        complete(None, 0)
    return segs


def line_stats(segments):
    """llvm LineCoverageIterator: {line: count} for every mapped line."""
    out = {}
    if not segments:
        return out
    i, line, wrapped, cur = 0, segments[0][0], None, []
    while i < len(segments):
        if cur:
            wrapped = cur[-1]
        cur = []
        while i < len(segments) and segments[i][0] == line:
            cur.append(segments[i])
            i += 1
        starts = [s for s in cur if not s[5] and s[3] and s[4]]
        min_rc = min(len(starts), 2)
        skipped = bool(cur) and not cur[0][3] and cur[0][4]
        mapped = (not skipped) and ((wrapped is not None and wrapped[3]) or min_rc > 0)
        mapped = mapped or any(s[4] and s[3] for s in cur)
        if mapped:
            cnt = wrapped[2] if wrapped is not None else 0
            for s in starts:
                cnt = max(cnt, s[2])
            out[line] = cnt
        line += 1
    return out


# ------------------------------------------------------------------ the cohort
def compute(merged, sources, files, support):
    """Full and runtime-only figures of `files` (repo-relative paths) from a merged export."""
    want = set(files)
    d = json.load(open(merged))["data"][0]
    groups = collections.defaultdict(dict)  # file -> (line, col) -> [lines, covered, count]
    cache = {}

    def rel(name):
        if name not in cache:
            parts = name.replace("\\", "/").split("/")
            cache[name] = next(("/".join(parts[i:]) for i in range(len(parts)) if "/".join(parts[i:]) in want), None)
        return cache[name]

    for fn in d["functions"]:
        regs = fn["regions"]
        if not regs or regs[0][5] != 0:
            continue
        f = rel(fn["filenames"][0])
        if f is None:
            continue
        key = (regs[0][0], regs[0][1])
        ls = line_stats(build_segments([r for r in regs if r[5] == 0]))
        n, c = len(ls), sum(1 for v in ls.values() if v > 0)
        g = groups[f].setdefault(key, [0, 0, 0])
        g[0], g[1], g[2] = max(g[0], n), max(g[1], c), max(g[2], fn["count"])
    out = {"files": {}, "totals": {"full": collections.Counter(), "runtime": collections.Counter()}}
    for f in sorted(want):
        G = groups.get(f, {})
        full = {"lines": sum(v[0] for v in G.values()), "lines_covered": sum(v[1] for v in G.values()),
                "functions": len(G), "functions_covered": sum(1 for v in G.values() if v[2] > 0)}
        spans = test_modules(os.path.join(sources, f))
        if f in support:
            rt = {"lines": 0, "lines_covered": 0, "functions": 0, "functions_covered": 0}
        else:
            def inside(ln):
                return any(s["lines"][0] <= ln <= s["lines"][1] for s in spans)
            K = {k: v for k, v in G.items() if not inside(k[0])}
            rt = {"lines": sum(v[0] for v in K.values()), "lines_covered": sum(v[1] for v in K.values()),
                  "functions": len(K), "functions_covered": sum(1 for v in K.values() if v[2] > 0)}
        out["files"][f] = {"full": full, "runtime": rt, "cfg_test_modules": [{"module": s["module"], "lines": s["lines"]} for s in spans]}
        out["totals"]["full"].update(full)
        out["totals"]["runtime"].update(rt)
    out["totals"] = {k: dict(v) for k, v in out["totals"].items()}
    return out
