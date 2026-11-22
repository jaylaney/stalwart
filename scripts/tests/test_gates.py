#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only
"""Offline self-tests of the two C10 gates (`check_test_inventory.py`, `check_coverage.py`).

They need no cargo. Each test builds small inputs in a temporary directory, runs a gate as a
command (`python3 -I`), and checks its exit status and the failure it names:
  - the strict rule on synthetic llvm-cov exports of a synthetic `tests/src` file, run through
    `strict_rule.py functions` first, as the gate's inputs are: an early return before the last
    statement, a future built but never awaited, an omitted synchronous call, a loop with no
    iteration, a helper and a tail block that did not run, a loss in one step only, renames and
    dispositions;
  - the listings, against the committed baseline: a candidate built from the baseline's own records
    passes, then a missing test, a newly ignored test, an empty selector, a missing capture, a dropped
    restored unit-test module, a status change, an omitted za sub-test in one step, an unrun
    synchronous sub-test, and the dispositions file's renames and selection moves;
  - the coverage gate on a synthetic merged export over the cohort's 40 paths: the baseline's exact
    fractions pass; one line below either threshold fails although a restored test file adds 50,000
    covered lines; a missing cohort file and an uncovered addition fail;
  - the CI workflow against `ci_steps.STEPS`, the table the net and the capture read: the committed
    test.yml matches it; a narrowed filter, a dropped feature, a changed environment, a dropped,
    renamed or disabled step, a narrowed rerun, `continue-on-error`, an untracked `cargo test`
    step and a gates job that runs fewer steps are each reported; and the net and the capture
    follow the table and the baseline's step names and commands.

Usage:
  python3 -I scripts/tests/test_gates.py [-v] [--real DIR]

With `--real DIR`, it also feeds the gates the 0.16.27 baseline's own raw inputs as the candidate
(README.md, "The baseline as its own candidate"), which must pass.
"""
import contextlib
import copy
import io
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock
import warnings

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gate_common  # noqa: E402,F401  (puts tests/qualification/tools on the path)
import capture_listings  # noqa: E402
import ci_steps  # noqa: E402
import cohort  # noqa: E402
import coverage_net  # noqa: E402
import retention_workflow  # noqa: E402
import strict_rule  # noqa: E402

ROOT = gate_common.ROOT
BASELINE = os.path.join(ROOT, "tests", "qualification", "baseline-0.16.27.json")
DISPOSITIONS = os.path.join(ROOT, "tests", "qualification", "dispositions.json")
INVENTORY = os.path.join(HERE, "check_test_inventory.py")
COVERAGE = os.path.join(HERE, "check_coverage.py")
REAL = None


def run(tool, *args):
    p = subprocess.run([sys.executable, "-I", tool, *args], capture_output=True, text=True)
    return p.returncode, p.stdout + p.stderr


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def write_json(path, value):
    write(path, json.dumps(value))


# ====================================================================== the strict rule
SUITE = """\
pub async fn sub_async(test: &T) {
    let a = one();
    if a == 0 {
        return;
    }
    assert_eq!(a, 1);
}

pub fn sub_sync(test: &T) {
    let a = one();
    assert_eq!(a, 1);
}

pub async fn sub_loop(test: &T) {
    let items = list();
    for item in items {
        check(item);
    }
}

pub async fn helper() -> u32 {
    let x = one();
    x + 1
}

pub fn builder() -> impl Future<Output = ()> {
    let a = one();
    async move {
        let b = a;
        use_it(b);
    }
}
"""
FILE = "tests/src/suite.rs"
CALLS = ["sub_async", "sub_sync", "sub_loop"]
STEPS = ["s1", "s2"]


@contextlib.contextmanager
def quiet():
    """strict_rule's per-step progress lines and the ResourceWarning of its unclosed source reads."""
    with contextlib.redirect_stderr(io.StringIO()), warnings.catch_warnings():
        warnings.simplefilter("ignore", ResourceWarning)
        yield


def geometry(src_root):
    with quiet():
        return {g["name"]: g for g in strict_rule.scan_sources(src_root)}


def body_regions(geo, counts, open_at_fn_line):
    """The regions of one body record: the outer region, the last statement's, the loop's and the
    closing-brace exit region."""
    o, c = geo["open"], geo["close"]
    start = [geo["line"], 1] if open_at_fn_line else list(o)
    regs = [[start[0], start[1], c[0], c[1] + 1, counts["body"], 0, 0, 0]]
    if geo["stmts"]:
        st = geo["stmts"][-1]
        regs.append([st["anchor"][0], st["anchor"][1], st["end"][0], st["end"][1], counts["last"], 0, 0, 0])
        if st.get("loop"):
            lp = st["loop"]
            regs.append([lp["anchor"][0], lp["anchor"][1], lp["end"][0], lp["end"][1], counts.get("loop", 1), 0, 0, 0])
    regs.append([c[0], c[1], c[0], c[1] + 1, counts["exit"], 0, 0, 0])
    return regs


def records(geos, name, counts):
    """The function records of `name` for one step: a plain fn's own record; an async fn's outer
    record (it only builds the future) and its body's closure record; a tail block's closure."""
    g = geos[name]
    out = []
    if g["async"]:
        out.append({"name": f"_RNvCs0_5tests{name}", "count": counts["outer"],
                    "regions": [[g["line"], 1, g["open"][0], g["open"][1], counts["outer"], 0, 0, 0]]})
        out.append({"name": f"_RNCNvCs0_5tests{name}0", "count": counts["body"],
                    "regions": body_regions(g, counts, open_at_fn_line=False)})
    else:
        out.append({"name": f"_RNvCs0_5tests{name}", "count": counts["body"],
                    "regions": body_regions(g, counts, open_at_fn_line=True)})
    if g["tail"]:
        t = dict(g["tail"], line=g["tail"]["open"][0])
        tc = counts.get("tail", {"body": 1, "last": 1, "exit": 1})
        out.append({"name": f"_RNCNvCs0_5tests{name}s_0", "count": tc["body"],
                    "regions": body_regions(t, tc, open_at_fn_line=False)})
    return out


def full_counts():
    return {"outer": 1, "body": 1, "last": 1, "exit": 1, "loop": 3}


class StrictRuleCase:
    """A synthetic source tree, its per-step exports and a baseline derived from them."""

    def __init__(self, tmp):
        self.tmp = tmp
        self.src = os.path.join(tmp, "src")
        write(os.path.join(self.src, FILE), SUITE)
        self.geos = geometry(self.src)
        self.counts = {s: {n: full_counts() for n in self.geos} for s in STEPS}
        self.base = self.derive_baseline()

    def export(self, counts, root, out_dir, drop=()):
        os.makedirs(out_dir, exist_ok=True)
        geos = geometry(root)
        for step in STEPS:
            fns = []
            for name in geos:
                if (step, name) in drop:
                    continue
                for r in records(geos, name, counts[step].get(name, full_counts())):
                    r["filenames"] = [os.path.join("/build/checkout", FILE)]
                    fns.append(r)
            write_json(os.path.join(out_dir, step + ".json"), {"data": [{"functions": fns, "files": []}]})

    def functions(self, exports, root, out):
        with quiet():
            steps, geos = strict_rule.compute(exports, root)
        write_json(out, {"steps": steps, "functions": [geos[k] for k in sorted(geos)]})
        return {k: g for k, g in geos.items()}

    def derive_baseline(self):
        ex = os.path.join(self.tmp, "base-exports")
        self.export(self.counts, self.src, ex)
        geos = self.functions(ex, self.src, os.path.join(self.tmp, "base-functions.json"))
        helpers, calls = [], []
        for k, g in sorted(geos.items()):
            h = {"file": g["file"], "in": g["in"], "name": g["name"], "line": g["line"],
                 "executed": {s: [v["body"], v["last"], v["exit"]] for s, v in g["steps"].items() if v["executed"]}}
            if g["tail"] and g["tail_steps"]:
                h["tail_block"] = {"executed": {s: [v["body"], v["last"], v["exit"]]
                                                for s, v in g["tail_steps"].items() if v["executed"]}}
            helpers.append(h)
        for i, n in enumerate(CALLS):
            g = geos[strict_rule.identity(FILE, None, n)]
            m = {"target": f"{FILE}:{g['line']}"}
            if n == "sub_loop":
                cnt = {s: g["steps"][s]["loop"] for s in STEPS}
                m["loop_body"] = {"counts": cnt, "required_in": STEPS}
            calls.append({"suite": "suite::suite_tests", "call": f"suite::{n}", "order": i + 1, "measured": m,
                          "executed": {s: g["steps"][s]["executed"] for s in STEPS}})
        return {"libtest": {"tests": [], "configurations": {}}, "dispositions": [],
                "subtests": {"calls": calls, "strict_rule": {"needs_completion_marker": []}},
                "helpers": {"functions": helpers, "steps": STEPS}}

    def gate(self, counts=None, root=None, drop=(), dispositions=None, name="cand"):
        d = os.path.join(self.tmp, name)
        root = root or self.src
        self.export(counts or self.counts, root, os.path.join(d, "exports"), drop)
        self.functions(os.path.join(d, "exports"), root, os.path.join(d, "functions.json"))
        write_json(os.path.join(d, "baseline.json"), self.base)
        write_json(os.path.join(d, "dispositions.json"), dispositions or {})
        write(os.path.join(d, "list", "empty.txt"), "")
        write(os.path.join(d, "ign", "empty.txt"), "")
        return run(INVENTORY, "--baseline", os.path.join(d, "baseline.json"),
                   "--dispositions", os.path.join(d, "dispositions.json"),
                   "--listing", os.path.join(d, "list"), "--ignored-listing", os.path.join(d, "ign"),
                   "--executed", os.path.join(d, "functions.json"))


class StrictRuleTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="gates-strict-")
        self.case = StrictRuleCase(self.tmp)

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def counts(self, step, name, **kw):
        c = copy.deepcopy(self.case.counts)
        c[step][name].update(kw)
        return c

    def assertFails(self, result, *needles):
        code, out = result
        self.assertEqual(code, 1, out)
        for n in needles:
            self.assertIn(n, out)

    def test_the_unchanged_baseline_passes(self):
        code, out = self.case.gate()
        self.assertEqual(code, 0, out)
        self.assertIn("result: OK", out)
        # every function and step was required: 5 functions x 2 steps, a tail block and a loop
        self.assertIn("'call_steps': 6", out)
        self.assertIn("'helper_steps': 4", out)
        self.assertIn("'tail_steps': 2", out)
        self.assertIn("'loop_steps': 2", out)

    def test_a_body_that_ran_but_skipped_its_last_statement_fails(self):
        # the early return: body and exit counts positive, the last statement's region 0
        self.assertFails(self.case.gate(self.counts("s1", "sub_async", last=0)),
                         "[call_not_executed] call suite::suite_tests -> suite::sub_async",
                         "[step s1]", "last-statement count 0", "body 1, last 0, exit 1")

    def test_a_future_built_but_never_awaited_fails(self):
        # `let _ = sub_async(&test);`: the outer record runs, the body's closure record does not
        code, out = self.case.gate(self.counts("s2", "sub_async", body=0, last=0, exit=0))
        self.assertFails((code, out), "suite::sub_async", "[step s2]", "body count 0")
        self.assertNotIn("[step s1]", out)

    def test_an_omitted_synchronous_call_fails(self):
        self.assertFails(self.case.gate(drop={("s1", "sub_sync")}),
                         "[call_not_executed] call suite::suite_tests -> suite::sub_sync", "[step s1]",
                         "no body record in this step's export")

    def test_a_loop_with_no_iteration_fails(self):
        self.assertFails(self.case.gate(self.counts("s1", "sub_loop", loop=0)),
                         "[loop_not_executed] call suite::suite_tests -> suite::sub_loop", "[step s1]",
                         "the loop ran no iteration")

    def test_a_helper_that_returned_early_fails(self):
        self.assertFails(self.case.gate(self.counts("s2", "helper", last=0)),
                         "[helper_not_executed] function tests/src/suite.rs | - | helper", "[step s2]")

    def test_a_tail_block_built_but_never_polled_fails(self):
        c = self.counts("s1", "builder", tail={"body": 0, "last": 0, "exit": 0})
        self.assertFails(self.case.gate(c), "[tail_not_executed] tail block of tests/src/suite.rs | - | builder",
                         "[step s1]", "body count 0")

    def renamed_tree(self):
        root = os.path.join(self.tmp, "renamed")
        write(os.path.join(root, FILE), SUITE.replace("pub async fn helper()", "pub async fn helper_two()"))
        counts = copy.deepcopy(self.case.counts)
        for s in STEPS:
            counts[s]["helper_two"] = counts[s].pop("helper")
        return root, counts

    def test_a_renamed_helper_needs_a_rename_entry(self):
        root, counts = self.renamed_tree()
        self.assertFails(self.case.gate(counts, root), "[helper_not_executed] function tests/src/suite.rs | - | helper",
                         "no body record in any step's export")
        rename = {"renames": {"functions": [{"from": {"file": FILE, "name": "helper"},
                                              "to": {"file": FILE, "name": "helper_two"}, "evidence": "test"}]}}
        code, out = self.case.gate(counts, root, dispositions=rename)
        self.assertEqual(code, 0, out)
        rename["renames"]["functions"][0]["unconfirmed"] = True
        self.assertFails(self.case.gate(counts, root, dispositions=rename), "[rename_unconfirmed]")

    def test_dispositions_excuse_only_when_confirmed_and_equivalents_hold(self):
        root, counts = self.renamed_tree()
        deleted = {"dispositions": [{"id": "d1", "kind": "subject_deleted", "scope": "function", "evidence": "test",
                                     "subjects": [{"file": FILE, "name": "helper"}]}]}
        code, out = self.case.gate(counts, root, dispositions=deleted)
        self.assertEqual(code, 0, out)
        self.assertIn("'subject_deleted': 2", out)
        deleted["dispositions"][0]["unconfirmed"] = True
        self.assertFails(self.case.gate(counts, root, dispositions=deleted), "[unconfirmed]", "d1")
        equiv = {"dispositions": [{"id": "d2", "kind": "internals_replaced_equivalent", "scope": "function",
                                   "evidence": "test", "subjects": [{"file": FILE, "name": "helper"}],
                                   "equivalents": [{"file": FILE, "name": "helper_two"}]}]}
        code, out = self.case.gate(counts, root, dispositions=equiv)
        self.assertEqual(code, 0, out)
        for s in STEPS:
            counts[s]["helper_two"]["last"] = 0
        self.assertFails(self.case.gate(counts, root, dispositions=equiv), "equivalent tests/src/suite.rs | - | helper_two")

    def test_a_disposition_limited_to_steps_excuses_only_there(self):
        root, counts = self.renamed_tree()
        limited = {"dispositions": [{"id": "s1", "kind": "subject_deleted", "scope": "function", "evidence": "test",
                                     "steps": [STEPS[0]], "subjects": [{"file": FILE, "name": "helper"}]}]}
        code, out = self.case.gate(counts, root, dispositions=limited)
        self.assertEqual(code, 1, out)
        self.assertIn(f"[helper_not_executed] function tests/src/suite.rs | - | helper [step {STEPS[1]}]", out)
        self.assertNotIn(f"helper [step {STEPS[0]}]", out)
        limited["dispositions"][0]["steps"] = list(STEPS)
        code, out = self.case.gate(counts, root, dispositions=limited)
        self.assertEqual(code, 0, out)
        limited["dispositions"][0]["steps"] = []
        code, out = self.case.gate(counts, root, dispositions=limited)
        self.assertEqual(code, 2, out)

    def test_a_case_entry_excuses_nothing(self):
        # Ruling 103: scope `case` documents a case retired inside a function that still runs.
        root, counts = self.renamed_tree()
        case = {"dispositions": [{"id": "c1", "kind": "subject_deleted", "scope": "case", "evidence": "test",
                                  "subjects": [{"file": FILE, "name": "helper", "case": "a retired case"}]}]}
        self.assertFails(self.case.gate(counts, root, dispositions=case),
                         "[helper_not_executed] function tests/src/suite.rs | - | helper")
        code, out = self.case.gate(dispositions=case)
        self.assertEqual(code, 0, out)

    def test_a_bad_dispositions_file_is_an_input_error(self):
        code, out = self.case.gate(dispositions={"dispositions": [{"id": "x", "kind": "obsolete", "scope": "function",
                                                                   "evidence": "e", "subjects": []}]})
        self.assertEqual(code, 2, out)
        self.assertIn("unknown kind", out)


# ====================================================================== listings
def exe(package, cargo_1_100=False):
    crate = gate_common.norm_package(package)
    if cargo_1_100:
        # Cargo 1.100 runs a test executable from its unit's out/, in a directory named by the
        # package (here the normalized name, hyphenated); the gate reads the file name only.
        unit = f"debug/build/{package.replace('_', '-')}/0000000000000000"
        return f"/mnt/target/llvm-cov-target/{unit}/out/{crate}-0000000000000000"
    return f"target/debug/deps/{crate}-0000000000000000"


def capture(name, selected, universe=None, cargo_1_100=False):
    """A capture in the baseline's format: SELECTED, then UNFILTERED when it differs."""
    def body(keys):
        by_pkg = {}
        for p, n in sorted(keys):
            by_pkg.setdefault(p, []).append(n)
        lines = []
        for p, names in sorted(by_pkg.items()):
            lines.append(f"     Running unittests src/lib.rs ({exe(p, cargo_1_100)})")
            lines += [f"{n}: test" for n in names]
            lines.append(f"\n{len(names)} tests, 0 benchmarks")
        return "\n".join(lines)
    text = f"# config: {name}\n## SELECTED\n## command: cargo test -- --list\n{body(selected)}\n"
    if universe is not None and set(universe) != set(selected):
        text += f"\n## UNFILTERED\n## command: cargo test -- --list\n{body(universe)}\n"
    return text


def baseline_candidate(base):
    """Captures and an --executed file built from the baseline's own records."""
    cfgs = base["libtest"]["configurations"]
    tests = base["libtest"]["tests"]
    key = lambda t: (t["package"], t["name"])  # noqa: E731
    listing, ignored = {}, {}
    for name, c in cfgs.items():
        if c["kind"] == "ci_step":
            sel = {key(t) for t in tests if name in t["ci_selected_by"]}
            uni = sel | {key(t) for t in tests if name in t["listed_in"]}
            for sb in c.get("same_binaries_as", []):
                uni |= {key(t) for t in tests if sb in t["listed_in"]}
            listing[name] = (sel, uni)
        elif c["kind"] == "feature_set":
            sel = {key(t) for t in tests if name in t["listed_in"]}
            listing[name] = (sel, sel)
        elif c["kind"] == "ignored_listing":
            sel = {key(t) for t in tests if name in t["ignored_listed_in"]}
            ignored[name] = (sel, sel)
    fns = []
    for h in base["helpers"]["functions"]:
        k = strict_rule.identity(h["file"], h.get("in"), h["name"])
        steps = {s: {"body": v[0], "last": v[1], "exit": v[2], "executed": True} for s, v in (h.get("executed") or {}).items()}
        tail = {s: {"body": v[0], "last": v[1], "exit": v[2], "executed": True}
                for s, v in ((h.get("tail_block") or {}).get("executed") or {}).items()}
        fns.append({"key": k, "file": h["file"], "in": h.get("in"), "name": h["name"], "steps": steps, "tail_steps": tail})
    by_key = {f["key"]: f for f in fns}
    helpers = {(h["file"], h["line"], h["name"]): strict_rule.identity(h["file"], h.get("in"), h["name"])
               for h in base["helpers"]["functions"]}
    for c in base["subtests"]["calls"]:
        lb = (c.get("measured") or {}).get("loop_body")
        if not lb:
            continue
        file, line = c["measured"]["target"].rsplit(":", 1)
        f = by_key[helpers[(file, int(line), c["call"].split("::")[-1])]]
        for s, n in lb["counts"].items():
            f["steps"].setdefault(s, {"body": 1, "last": None, "exit": 1, "executed": False})["loop"] = n
    return listing, ignored, {"steps": base["helpers"]["steps"], "functions": fns}


class ListingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        with open(BASELINE, encoding="utf-8") as f:
            cls.base = json.load(f)
        cls.listing, cls.ignored, cls.executed = baseline_candidate(cls.base)

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="gates-listing-")

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def gate(self, listing=None, ignored=None, executed=None, dispositions=None, cargo_1_100=False):
        listing = self.listing if listing is None else listing
        ignored = self.ignored if ignored is None else ignored
        for d, caps in (("list", listing), ("ign", ignored)):
            shutil.rmtree(os.path.join(self.tmp, d), ignore_errors=True)
            os.makedirs(os.path.join(self.tmp, d))
            for name, (sel, uni) in caps.items():
                write(os.path.join(self.tmp, d, name + ".txt"), capture(name, sel, uni, cargo_1_100))
        write_json(os.path.join(self.tmp, "functions.json"), executed or self.executed)
        if isinstance(dispositions, dict):
            write_json(os.path.join(self.tmp, "dispositions.json"), dispositions)
            dispositions = os.path.join(self.tmp, "dispositions.json")
        return run(INVENTORY, "--baseline", BASELINE, "--dispositions", dispositions or self.empty(),
                   "--listing", os.path.join(self.tmp, "list"), "--ignored-listing", os.path.join(self.tmp, "ign"),
                   "--executed", os.path.join(self.tmp, "functions.json"))

    def empty(self):
        p = os.path.join(self.tmp, "empty.json")
        write_json(p, {})
        return p

    def without(self, pred):
        return {n: ({k for k in sel if not pred(k)}, {k for k in uni if not pred(k)}) for n, (sel, uni) in self.listing.items()}

    def assertFails(self, result, *needles):
        code, out = result
        self.assertEqual(code, 1, out[-3000:])
        for n in needles:
            self.assertIn(n, out)
        return out

    def test_the_baseline_as_its_own_candidate_passes(self):
        for disp in (None, DISPOSITIONS):
            code, out = self.gate(dispositions=disp)
            self.assertEqual(code, 0, out[-3000:])
            self.assertIn("'tests': 795", out)
            self.assertIn("'call_steps': 272", out)
            self.assertIn("'helper_steps': 3833", out)

    def test_captures_naming_cargo_1_100_executables_pass(self):
        # The baseline's captures name target/debug/deps/<crate>-<hash>; Cargo 1.100's name
        # debug/build/<package>/<hash>/out/<crate>-<hash>. The package comes from the file name.
        code, out = self.gate(dispositions=DISPOSITIONS, cargo_1_100=True)
        self.assertEqual(code, 0, out[-3000:])
        self.assertIn("'tests': 795", out)
        self.assertFails(self.gate(self.without(lambda k: k == ("tests", "za::za_tests")), cargo_1_100=True),
                         "[missing] test tests::za::za_tests: not listed by any --listing capture")

    def test_a_missing_test_fails(self):
        self.assertFails(self.gate(self.without(lambda k: k == ("tests", "za::za_tests"))),
                         "[missing] test tests::za::za_tests: not listed by any --listing capture")

    def test_a_test_dropped_from_one_ci_step_fails(self):
        listing = dict(self.listing)
        name = "test.yml__zero-access__zero-access-tests-filesystem-blob-store"
        sel, uni = listing[name]
        listing[name] = (sel - {("tests", "za::za_tests")}, uni)
        self.assertFails(self.gate(listing), "[unselected] test tests::za::za_tests", name)

    def test_a_newly_ignored_test_fails(self):
        ignored = dict(self.ignored)
        sel, uni = ignored["exists__tests__default--ignored-only"]
        ignored["exists__tests__default--ignored-only"] = (sel | {("tests", "imap::imap_tests")}, uni | {("tests", "imap::imap_tests")})
        self.assertFails(self.gate(ignored=ignored), "[newly_ignored] test tests::imap::imap_tests")

    def test_an_empty_selector_fails(self):
        listing = dict(self.listing)
        name = "test.yml__zero-access__zero-access-tests"
        listing[name] = (set(), listing[name][1])
        self.assertFails(self.gate(listing), f"[empty_selector] configuration {name}: selects nothing")

    def test_a_missing_capture_fails(self):
        listing = dict(self.listing)
        del listing["test.yml__unit-tests__workspace-unit-tests"]
        self.assertFails(self.gate(listing), "[capture_missing] configuration test.yml__unit-tests__workspace-unit-tests")
        ignored = dict(self.ignored)
        del ignored["exists__tests__default--ignored-only"]
        self.assertFails(self.gate(ignored=ignored), "[capture_missing] configuration exists__tests__default: no --ignored-listing")

    def test_a_dropped_restored_unit_test_module_fails(self):
        out = self.assertFails(self.gate(self.without(lambda k: k[0] == "common" and k[1].startswith("auth::oauth::token::tests::"))),
                               "[missing] test common::auth::oauth::token::tests::wrong_key_is_rejected")
        self.assertEqual(out.count("[missing] test common::auth::oauth::token::tests::"), 10)

    def test_an_ignored_test_that_now_runs_is_a_status_change(self):
        ignored = dict(self.ignored)
        sel, uni = ignored["exists__tests__default--ignored-only"]
        drop = {("tests", "smtp::inbound::asn::asn")}
        ignored["exists__tests__default--ignored-only"] = (sel - drop, uni - drop)
        self.assertFails(self.gate(ignored=ignored), "[status_change] test tests::smtp::inbound::asn::asn: ignored at 0.16.27")

    def executed_without(self, ident, step):
        ex = copy.deepcopy(self.executed)
        for f in ex["functions"]:
            if f["key"] == ident:
                f["steps"][step] = {"body": 1, "last": 0, "exit": 1, "executed": False}
        return ex

    def test_an_omitted_za_sub_test_in_one_step_fails(self):
        out = self.assertFails(self.gate(executed=self.executed_without("tests/src/za/setup.rs | - | test", "za-tests-fs-blob")),
                               "[call_not_executed] call za::za_tests -> setup::test", "[step za-tests-fs-blob]")
        self.assertNotIn("[step za-tests]", out)

    def test_the_synchronous_sub_tests_are_required(self):
        for ident, call, step in (("tests/src/imap/body_structure.rs | - | test", "imap::imap_tests -> body_structure::test", "imap-key"),
                                  ("tests/src/webdav/cal_itip.rs | - | test", "webdav::webdav_tests -> cal_itip::test", "webdav-plain")):
            self.assertFails(self.gate(executed=self.executed_without(ident, step)), f"[call_not_executed] call {call}", f"[step {step}]")

    def renamed_listing(self, old, new):
        out = {}
        for n, (sel, uni) in self.listing.items():
            ren = lambda s: {(new[0], new[1] + k[1][len(old[1]):]) if k[0] == old[0] and k[1].startswith(old[1]) else k for k in s}  # noqa: E731
            out[n] = (ren(sel), ren(uni))
        return out

    def test_renames_come_only_from_the_dispositions_file(self):
        listing = self.renamed_listing(("groupware", "calendar::sequence::"), ("groupware", "calendar::sequence_floor::"))
        self.assertFails(self.gate(listing), "[missing] test groupware::calendar::sequence::tests::applying_the_sequences_never_lowers_one")
        code, out = self.gate(listing, dispositions=DISPOSITIONS)
        self.assertEqual(code, 0, out[-3000:])

    def test_a_selection_move_must_be_confirmed_and_captured(self):
        unit = "test.yml__unit-tests__workspace-unit-tests"
        target = "test.yml__unit-tests__smtp-unit-tests-without-enterprise"
        dkim = {k for k in self.listing[unit][0] if k[0] == "smtp" and k[1].startswith("inbound::dkim::tests::")}
        self.assertEqual(len(dkim), 7)
        listing = dict(self.listing)
        listing[unit] = (listing[unit][0] - dkim, listing[unit][1] - dkim)
        self.assertFails(self.gate(listing, dispositions=DISPOSITIONS), "[capture_missing]", target)
        listing[target] = (dkim, dkim)
        # Task 8 confirmed the committed moves (ruling 124); an unconfirmed copy still fails.
        with open(DISPOSITIONS, encoding="utf-8") as f:
            disp = json.load(f)
        for m in disp["selection_moves"]:
            m["unconfirmed"] = m["to_configuration"] == target
        self.assertFails(self.gate(listing, dispositions=disp), "[move_unconfirmed]", target)
        for m in disp["selection_moves"]:
            m["unconfirmed"] = False
        code, out = self.gate(listing, dispositions=disp)
        self.assertEqual(code, 0, out[-3000:])
        code, out = self.gate(listing, dispositions=DISPOSITIONS)
        self.assertEqual(code, 0, out[-3000:])


# ====================================================================== coverage
def region_lines(l1, l2, count):
    return [l1, 1, l2, 2, count, 0, 0, 0]


class CoverageTests(unittest.TestCase):
    """The cohort's 40 paths hold synthetic sources: `crates/vault/src/keys.rs` carries every line
    (runtime functions, and functions inside a `#[cfg(test)]` module), the other files none."""

    RUNTIME = 11246
    TEST_LINES = 18490 - 11246
    FILE = "crates/vault/src/keys.rs"

    @classmethod
    def setUpClass(cls):
        with open(BASELINE, encoding="utf-8") as f:
            cls.base = json.load(f)
        # a region from (l1, 1) to (l2, 2) maps l2 - l1 + 1 lines under llvm-cov's rules
        got = cohort.line_stats(cohort.build_segments([region_lines(3, 9, 1)]))
        assert sorted(got) == list(range(3, 10)), got

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="gates-coverage-")
        self.src = os.path.join(self.tmp, "src")
        for f in self.base["cohort"]["files"]:
            write(os.path.join(self.src, f["file"]), "fn empty() {}\n")
        # runtime part: lines 1 .. RUNTIME; test module: the next TEST_LINES lines and two of its own
        r, t = self.RUNTIME, self.TEST_LINES
        text = "\n" * r + "#[cfg(test)]\nmod tests {\n" + "\n" * t + "}\n"
        write(os.path.join(self.src, self.FILE), text)
        self.test_first = r + 3  # the module's first body line

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def export(self, runtime_covered, test_covered, extra=(), restored_covered=50000):
        r, t, f0 = self.RUNTIME, self.TEST_LINES, self.test_first
        path = os.path.join("/ci/checkout", self.FILE)
        fns = [
            {"name": "a", "count": 1, "filenames": [path], "regions": [region_lines(1, runtime_covered, 1)]},
            {"name": "b", "count": 0, "filenames": [path], "regions": [region_lines(runtime_covered + 1, r, 0)]},
            {"name": "c", "count": 1, "filenames": [path], "regions": [region_lines(f0, f0 + test_covered - 1, 1)]},
            {"name": "d", "count": 0, "filenames": [path], "regions": [region_lines(f0 + test_covered, f0 + t - 1, 0)]},
        ]
        files = [{"filename": path, "summary": {"lines": {"count": r + t, "covered": runtime_covered + test_covered}}}]
        restored = "crates/vault/src/keys_restored_tests.rs"
        write(os.path.join(self.src, restored), "\n" * restored_covered)
        fns.append({"name": "e", "count": 1, "filenames": [os.path.join("/ci/checkout", restored)],
                    "regions": [region_lines(1, restored_covered, 1)]})
        files.append({"filename": os.path.join("/ci/checkout", restored),
                      "summary": {"lines": {"count": restored_covered, "covered": restored_covered}}})
        for rel, n, c in extra:
            write(os.path.join(self.src, rel), "\n" * n)
            p = os.path.join("/ci/checkout", rel)
            fns.append({"name": rel, "count": 1 if c else 0, "filenames": [p], "regions": [region_lines(1, n, 1 if c else 0)]})
            files.append({"filename": p, "summary": {"lines": {"count": n, "covered": n if c else 0}}})
        merged = os.path.join(self.tmp, "merged.json")
        write_json(merged, {"data": [{"functions": fns, "files": files}]})
        return merged

    def gate(self, merged, dispositions=None):
        p = os.path.join(self.tmp, "dispositions.json")
        write_json(p, dispositions or {})
        return run(COVERAGE, "--baseline", BASELINE, "--dispositions", p, "--merged", merged, "--sources", self.src,
                   "--out", os.path.join(self.tmp, "out.json"))

    def test_the_baseline_fractions_pass(self):
        code, out = self.gate(self.export(10646, 17821 - 10646))
        self.assertEqual(code, 0, out)
        self.assertIn("cohort full: lines 17821/18490", out)
        self.assertIn("cohort runtime: lines 10646/11246", out)
        self.assertIn("restored_tests: lines 50000/50000", out)

    def test_one_line_below_the_full_threshold_fails_despite_restored_tests(self):
        code, out = self.gate(self.export(10646, 17821 - 10646 - 1))
        self.assertEqual(code, 1, out)
        self.assertIn("FAIL [cohort_full] lines 17820/18490 is below 17821/18490", out)
        self.assertIn("cohort runtime: lines 10646/11246 (94.66%) against 10646/11246 (94.66%): pass", out)
        self.assertIn("restored_tests: lines 50000/50000", out)

    def test_one_line_below_the_runtime_threshold_fails_despite_restored_tests(self):
        code, out = self.gate(self.export(10645, 17821 - 10645))
        self.assertEqual(code, 1, out)
        self.assertIn("FAIL [cohort_runtime] lines 10645/11246 is below 10646/11246", out)
        self.assertIn("cohort full: lines 17821/18490 (96.38%) against 17821/18490 (96.38%): pass", out)
        self.assertNotIn("[cohort_full]", out)

    def test_a_missing_cohort_file_fails(self):
        os.remove(os.path.join(self.src, "crates/groupware/src/calendar/sequence.rs"))
        code, out = self.gate(self.export(10646, 17821 - 10646))
        self.assertEqual(code, 1, out)
        self.assertIn("FAIL [cohort_file_missing] crates/groupware/src/calendar/sequence.rs (moved to "
                      "crates/groupware/src/calendar/sequence_floor.rs)", out)

    def test_a_move_is_measured_at_its_new_path(self):
        write(os.path.join(self.src, "crates/groupware/src/calendar/sequence_floor.rs"), "fn empty() {}\n")
        code, out = self.gate(self.export(10646, 17821 - 10646))
        self.assertEqual(code, 0, out)
        with open(os.path.join(self.tmp, "out.json"), encoding="utf-8") as f:
            mapping = json.load(f)["mapping"]
        self.assertEqual(mapping["crates/groupware/src/calendar/sequence.rs"], "crates/groupware/src/calendar/sequence_floor.rs")

    def test_an_uncovered_addition_counts_and_excluded_files_do_not(self):
        add = "crates/services/src/index_queue/za.rs"
        excl = "crates/store/src/search/index_za_tests.rs"
        merged = self.export(10646, 17821 - 10646, extra=[(add, 10, False), (excl, 1000, False)])
        disp = {"cohort": {"additions": [{"file": add, "evidence": "test"}],
                           "excluded_additions": [{"file": excl, "reason": "test"}]}}
        code, out = self.gate(merged, disp)
        self.assertEqual(code, 1, out)
        self.assertIn("FAIL [cohort_runtime] lines 10646/11256", out)
        self.assertIn("fork_test_files: lines 0/1000", out)
        disp["cohort"]["additions"] = []
        code, out = self.gate(merged, disp)
        self.assertEqual(code, 0, out)


# ====================================================================== the CI workflow
class WorkflowTests(unittest.TestCase):
    """`ci_steps.STEPS`, the one table both the net and the capture read, against test.yml (ruling
    177, final review D4): the committed workflow matches it, and each edit that would let CI run
    less than the gates measure is reported."""

    @classmethod
    def setUpClass(cls):
        with open(ci_steps.WORKFLOW, encoding="utf-8") as f:
            cls.text = f.read()
        cls.shard = ci_steps.WORKFLOW.with_name("test-retention-shard.yml").read_text()

    def edited(self, old, new):
        self.assertEqual(self.text.count(old), 1, f"anchor not unique in test.yml: {old!r}")
        return ci_steps.check_workflow(self.text.replace(old, new), self.shard)

    def assertReports(self, problems, *parts):
        self.assertTrue(any(all(part in problem for part in parts) for problem in problems),
                        f"no problem names {parts}: {problems}")

    def test_the_committed_workflow_matches_the_table(self):
        self.assertEqual(ci_steps.check_workflow(self.text, self.shard), [])

    def test_a_narrowed_filter_fails(self):
        problems = self.edited(
            "run: |\n          cargo test -p tests za::za_tests -- --nocapture || {\n"
            "            echo \"::warning title=za_tests::run failed;",
            "run: |\n          cargo test -p tests za::za_tests::setup -- --nocapture || {\n"
            "            echo \"::warning title=za_tests::run failed;")
        self.assertReports(problems, "'Zero-access Tests'", "command is",
                           "za::za_tests::setup")
        problems = self.edited("run: cargo test -p tests imap -- --nocapture",
                               "run: cargo test -p tests imap::imap_tests -- --nocapture")
        self.assertReports(problems, "'IMAP Tests'", "command is")

    def test_a_dropped_feature_fails(self):
        problems = self.edited(
            "run: cargo test -p http@1.0.3 --lib --bins --features test_mode,enterprise",
            "run: cargo test -p http@1.0.3 --lib --bins --features test_mode")
        self.assertReports(problems, "'HTTP Unit Tests'", "command is")
        problems = self.edited("run: cargo test -p smtp --lib --bins --features test_mode",
                               "run: cargo test -p smtp --lib --bins")
        self.assertReports(problems, "'SMTP Unit Tests (without enterprise)'", "command is")
        # The workspace step's list is the workflow's, which the capture reads; a job that
        # overrides it would list features the capture never sees.
        problems = self.edited(
            "timeout-minutes: 75\n\n    env:\n      STORE: RocksDb\n",
            "timeout-minutes: 75\n\n    env:\n      STORE: RocksDb\n"
            "      UNIT_TEST_FEATURES: common/test_mode\n")
        self.assertReports(problems, "'Workspace Unit Tests'", "overrides")
        problems = self.edited('--lib --bins --features "$UNIT_TEST_FEATURES"',
                               "--lib --bins --features common/test_mode")
        self.assertReports(problems, "'Workspace Unit Tests'", "command is")

    def test_a_changed_environment_fails(self):
        problems = self.edited(
            "run: cargo test -p tests imap::imap_tests -- --nocapture\n"
            "        env:\n          ZA_KEY_ACCOUNTS: \"1\"\n",
            "run: cargo test -p tests imap::imap_tests -- --nocapture\n")
        self.assertReports(problems, "'IMAP Tests (key accounts)'", "ZA_KEY_ACCOUNTS")
        problems = self.edited("          BLOB_STORE: FileSystem\n",
                               "          BLOB_STORE: FileSystem\n          ZA_ONLY: setup\n")
        self.assertReports(problems, "filesystem blob store", "ZA_ONLY")
        problems = self.edited("SPAM_RULES_URL: https://github.com/stalwartlabs/spam-filter/"
                               "releases/download/v3.0.4/",
                               "SPAM_RULES_URL: https://github.com/stalwartlabs/spam-filter/"
                               "releases/download/v3.0.3/")
        self.assertReports(problems, "'SMTP Tests'", "SPAM_RULES_URL")
        problems = self.edited("    timeout-minutes: 240\n\n    env:\n      STORE: RocksDb\n",
                               "    timeout-minutes: 240\n\n    env:\n      STORE: Sqlite\n")
        self.assertReports(problems, "job zero-access", "STORE")

    def test_zero_access_backtraces_are_required_at_job_and_step_scope(self):
        # Removing the job setting must be rejected even when no step overrides it.
        without_backtrace = self.text.replace('      RUST_BACKTRACE: "1"\n', "")
        problems = ci_steps.check_workflow(without_backtrace, self.shard)
        self.assertReports(problems, "job zero-access", "RUST_BACKTRACE")
        self.assertReports(problems, "'Zero-access Tests'", "RUST_BACKTRACE")
        for value in ("0", "full"):
            with self.subTest(value=value):
                problems = self.edited('      RUST_BACKTRACE: "1"\n',
                                       f'      RUST_BACKTRACE: "{value}"\n')
                self.assertReports(problems, "job zero-access", "RUST_BACKTRACE")
                self.assertReports(problems, "'Zero-access Tests'", "RUST_BACKTRACE")
        for value in ("0", "1"):
            with self.subTest(step_override=value):
                problems = self.edited("          BLOB_STORE: FileSystem\n",
                                       "          BLOB_STORE: FileSystem\n"
                                       f'          RUST_BACKTRACE: "{value}"\n')
                self.assertReports(problems, "filesystem blob store", "overrides",
                                   "RUST_BACKTRACE")

    def test_job_diagnostics_do_not_allow_arbitrary_environment(self):
        problems = self.edited('      RUST_BACKTRACE: "1"\n',
                               '      RUST_BACKTRACE: "1"\n      ZA_ONLY: setup\n')
        self.assertReports(problems, "job zero-access", "ZA_ONLY")
        self.assertReports(problems, "'Zero-access Tests'", "ZA_ONLY")
        problems = self.edited('  UNIT_TEST_FEATURES: >-\n',
                               '  RUST_BACKTRACE: "1"\n  UNIT_TEST_FEATURES: >-\n')
        self.assertReports(problems, "'Directory Tests'", "RUST_BACKTRACE")

    def test_a_dropped_or_disabled_step_fails(self):
        problems = self.edited("      - name: Automation Tests\n", "      - name: Automation\n")
        self.assertReports(problems, "'Automation Tests'", "missing")
        self.assertReports(problems, "'Automation'", "not in ci_steps.STEPS")
        problems = self.edited(
            "      - name: Zero-access Cache Swap Tests\n        if: ${{ !cancelled() }}\n",
            "      - name: Zero-access Cache Swap Tests\n        if: false\n")
        self.assertReports(problems, "'Zero-access Cache Swap Tests'", "if is")
        problems = self.edited("  upstream-suites:\n    name: Upstream Suites\n    needs: style\n",
                               "  upstream-suites:\n    name: Upstream Suites\n    needs: style\n"
                               "    continue-on-error: true\n")
        self.assertReports(problems, "job upstream-suites", "continue-on-error")

    def test_a_narrowed_rerun_fails(self):
        problems = self.edited("cargo test -p tests -- --exact --nocapture $failed",
                               "cargo test -p tests -- --exact --nocapture $failed --skip dkim")
        self.assertReports(problems, "'SMTP Tests'", "rerun")
        problems = self.edited(
            "            echo \"::warning title=jmap_tests::plain-mode run failed; retrying once "
            "(known calendar::notification flake)\"\n"
            "            cargo test -p tests jmap::jmap_tests -- --nocapture\n",
            "            echo \"::warning title=jmap_tests::plain-mode run failed; retrying once "
            "(known calendar::notification flake)\"\n"
            "            cargo test -p tests jmap::jmap_tests::x -- --nocapture\n")
        self.assertReports(problems, "'JMAP Tests (plain accounts)'", "rerun")

    def test_an_untracked_cargo_test_step_fails(self):
        problems = self.edited(
            "      - name: CalDAV Tests (plain accounts)\n",
            "      - name: Extra\n        run: cargo test -p tests za::extra -- --nocapture\n\n"
            "      - name: CalDAV Tests (plain accounts)\n")
        self.assertReports(problems, "'Extra'", "not in ci_steps.STEPS")

    def edited_shard(self, old, new):
        self.assertEqual(self.shard.count(old), 1, f"anchor not unique in shard workflow: {old!r}")
        return ci_steps.check_workflow(self.text, self.shard.replace(old, new))

    def test_a_missing_called_workflow_fails_the_api_and_runner_guard(self):
        problems = ci_steps.check_workflow(self.text)
        self.assertReports(problems, "test-retention-shard.yml", "missing")
        with tempfile.TemporaryDirectory() as directory:
            workflow = Path(directory) / "test.yml"
            workflow.write_text(self.text)
            with contextlib.redirect_stderr(io.StringIO()) as report:
                self.assertTrue(ci_steps.refuse_diverged_workflow(workflow))
            self.assertIn("test-retention-shard.yml", report.getvalue())
            code, output = run(str(ci_steps.REPO / "scripts/tests/ci_steps.py"),
                               "--workflow", workflow)
            self.assertEqual(code, 1)
            self.assertIn("test-retention-shard.yml", output)

    def test_a_wrong_shard_selection_or_snapshot_input_fails(self):
        problems = self.edited("      shard-id: za-filesystem\n", "      shard-id: za-rocks\n")
        self.assertReports(problems, "gates-za-filesystem", "with.shard-id")
        problems = self.edited("      shard-id: za-filesystem\n"
                               "      snapshot-artifact-id: ${{ needs.gates-build.outputs.snapshot-artifact-id }}\n",
                               "      shard-id: za-filesystem\n"
                               "      snapshot-artifact-id: 12\n")
        self.assertReports(problems, "gates-za-filesystem", "with.snapshot-artifact-id")

    def test_called_shard_env_timeout_and_output_must_match(self):
        for field, original, changed in (
            ("env.STORE", "      STORE: RocksDb\n", ""),
            ("env.RUST_MIN_STACK", '      RUST_MIN_STACK: "16777216"\n', ""),
            ("env.RUSTUP_TOOLCHAIN", '      RUSTUP_TOOLCHAIN: "${{ inputs.rust-toolchain }}"\n',
             '      RUSTUP_TOOLCHAIN: "stable"\n'),
            ("timeout-minutes", "    timeout-minutes: ${{ inputs.timeout-minutes }}\n",
             "    timeout-minutes: 20\n"),
            ("outputs.receipt-artifact-id.value",
             "        value: ${{ jobs.shard.outputs.receipt-artifact-id }}\n",
             "        value: ${{ jobs.shard.outputs.other }}\n"),
        ):
            with self.subTest(field=field):
                self.assertReports(self.edited_shard(original, changed), "shard", field)

    # The retention toolchain: one job resolves the current stable release once per run, and
    # every other retention job installs exactly that release (no version is written anywhere).
    RESOLVED = "${{ needs.gates-toolchain.outputs.rust-toolchain }}"
    TOOLCHAIN_JOBS = ("gates-build", "gates-listings", "gates")
    SHARD_CALLS = ("gates-short", "gates-za-rocks", "gates-za-filesystem")

    def job_span(self, job):
        start = self.text.index(f"\n  {job}:\n") + 1
        following = re.compile(r"^  [A-Za-z0-9_-]+:\n", re.M).search(self.text, start + 1)
        return start, following.start() if following else len(self.text)

    def edited_job(self, job, old, new):
        """test.yml with one edit inside job `job` only."""
        start, end = self.job_span(job)
        block = self.text[start:end]
        self.assertEqual(block.count(old), 1, f"anchor not unique in job {job}: {old!r}")
        return ci_steps.check_workflow(
            self.text[:start] + block.replace(old, new) + self.text[end:], self.shard)

    def test_the_six_toolchain_sites_take_the_resolvers_output(self):
        jobs = ci_steps.load_yaml(self.text)["jobs"]
        for job in self.TOOLCHAIN_JOBS:
            with self.subTest(job=job):
                self.assertEqual(jobs[job]["env"]["RUSTUP_TOOLCHAIN"], self.RESOLVED)
        for job in self.SHARD_CALLS:
            with self.subTest(job=job):
                self.assertEqual(jobs[job]["with"]["rust-toolchain"], self.RESOLVED)
        callee = ci_steps.load_yaml(self.shard)["jobs"]["shard"]
        self.assertEqual(callee["env"]["RUSTUP_TOOLCHAIN"], "${{ inputs.rust-toolchain }}")
        # The contract describes the same expressions, not a release of its own.
        expected = retention_workflow.producer_jobs(ci_steps.BASE_ENV)
        expected["gates"] = retention_workflow.reporter_job(ci_steps.BASE_ENV)
        for job in self.TOOLCHAIN_JOBS:
            with self.subTest(contract=job):
                self.assertEqual(expected[job]["env"]["RUSTUP_TOOLCHAIN"], self.RESOLVED)
        self.assertFalse(hasattr(retention_workflow, "RUST_TOOLCHAIN"))

    def test_one_resolver_job_exposes_the_run_toolchain(self):
        jobs = ci_steps.load_yaml(self.text)["jobs"]
        self.assertIn("gates-toolchain", jobs)
        resolver = jobs["gates-toolchain"]
        self.assertEqual(resolver["name"], "Resolve Retention Toolchain")
        self.assertEqual(resolver["needs"], "style")
        self.assertEqual(resolver["if"], jobs["gates-build"]["if"])
        self.assertEqual(resolver["runs-on"], jobs["gates-build"]["runs-on"])
        self.assertLessEqual(int(resolver["timeout-minutes"]), 15)
        self.assertEqual(resolver["permissions"], {"contents": "read"})
        self.assertEqual(resolver["outputs"],
                         {"rust-toolchain": "${{ steps.resolve.outputs.rust-toolchain }}"})
        self.assertEqual(resolver["steps"][-1]["run"],
                         "bash scripts/tests/retention_ci_setup.sh resolve-toolchain")
        for job in (*self.TOOLCHAIN_JOBS, *self.SHARD_CALLS):
            with self.subTest(job=job):
                self.assertIn("gates-toolchain", jobs[job]["needs"])

    def test_a_literal_or_foreign_toolchain_fails(self):
        values = ('"9.87.6"', "stable", "${{ needs.gates-build.outputs.rust-toolchain }}")
        for job in self.TOOLCHAIN_JOBS:
            for value in values:
                with self.subTest(job=job, mutation=f"env RUSTUP_TOOLCHAIN {value}"):
                    problems = self.edited_job(job, f"RUSTUP_TOOLCHAIN: {self.RESOLVED}\n",
                                               f"RUSTUP_TOOLCHAIN: {value}\n")
                    self.assertReports(problems, f"job {job}.env.RUSTUP_TOOLCHAIN")
        for job in self.SHARD_CALLS:
            for value in values:
                with self.subTest(job=job, mutation=f"with rust-toolchain {value}"):
                    problems = self.edited_job(job, f"rust-toolchain: {self.RESOLVED}\n",
                                               f"rust-toolchain: {value}\n")
                    self.assertReports(problems, f"job {job}.with.rust-toolchain")

    def test_a_toolchain_at_another_scope_fails(self):
        setting = "RUSTUP_TOOLCHAIN: stable\n"
        tools = "        id: tools\n        shell: bash\n"
        for job in self.TOOLCHAIN_JOBS:
            with self.subTest(mutation=f"step env on {job}'s tools step"):
                problems = self.edited_job(job, tools, tools + "        env:\n          " + setting)
                self.assertReports(problems, f"job {job}, step tools.env: unexpected")
        with self.subTest(mutation="step env on the shard's tools step"):
            problems = self.edited_shard(tools, tools + "        env:\n          " + setting)
            self.assertReports(problems, "job shard, step tools.env: unexpected")
        with self.subTest(mutation="env block on the resolver job"):
            problems = self.edited_job("gates-toolchain", "    permissions:\n",
                                       "    env:\n      " + setting + "    permissions:\n")
            self.assertReports(problems, "job gates-toolchain.env: unexpected")
        with self.subTest(mutation="workflow-level RUSTUP_TOOLCHAIN"):
            # Reported through the ordinary gated steps' effective environment (ci_steps).
            problems = self.edited("  UNIT_TEST_FEATURES: >-\n",
                                   "  " + setting + "  UNIT_TEST_FEATURES: >-\n")
            self.assertReports(problems, "environment RUSTUP_TOOLCHAIN", "'stable'")

    def test_a_retention_job_that_does_not_need_the_resolver_fails(self):
        for job in (*self.TOOLCHAIN_JOBS, *self.SHARD_CALLS):
            with self.subTest(job=job, mutation="gates-toolchain dropped from needs"):
                problems = self.edited_job(job, "      - gates-toolchain\n", "")
                self.assertReports(problems, f"job {job}.needs")

    def test_the_resolver_job_cannot_be_dropped_changed_or_conditioned(self):
        start, end = self.job_span("gates-toolchain")
        problems = self.edited(self.text[start:end], "")
        self.assertIn("job gates-toolchain: missing", problems)
        when = "    if: github.event_name == 'workflow_dispatch' || github.event_name == 'schedule'\n"
        run = "        run: bash scripts/tests/retention_ci_setup.sh resolve-toolchain\n"
        for mutation, old, new, field in (
            ("job condition always()", when, "    if: ${{ always() }}\n", ".if"),
            ("job condition narrowed", when,
             "    if: github.event_name == 'workflow_dispatch'\n", ".if"),
            ("continue-on-error added", when, when + "    continue-on-error: true\n",
             ".continue-on-error: unexpected"),
            ("needs changed", "    needs: style\n", "    needs: gates-listings\n", ".needs"),
            ("runner changed", "    runs-on: ubuntu-24.04\n", "    runs-on: ubuntu-latest\n",
             ".runs-on"),
            ("timeout lengthened", "    timeout-minutes: 10\n", "    timeout-minutes: 120\n",
             ".timeout-minutes"),
            ("write permission", "      contents: read\n", "      contents: write\n",
             ".permissions.contents"),
            ("output from another step",
             "      rust-toolchain: ${{ steps.resolve.outputs.rust-toolchain }}\n",
             "      rust-toolchain: ${{ steps.other.outputs.rust-toolchain }}\n",
             ".outputs.rust-toolchain"),
            ("channel name written as output", run,
             "        run: printf 'rust-toolchain=stable\\n' >> \"$GITHUB_OUTPUT\"\n",
             ", step resolve.run"),
            ("step conditioned", run, "        if: ${{ false }}\n" + run,
             ", step resolve.if: unexpected"),
            ("step allowed to fail", run, "        continue-on-error: true\n" + run,
             ", step resolve.continue-on-error: unexpected"),
        ):
            with self.subTest(mutation=mutation):
                problems = self.edited_job("gates-toolchain", old, new)
                self.assertReports(problems, f"job gates-toolchain{field}")

    def test_reporter_counts_the_resolver_result(self):
        for mutation, old, new, field in (
            ("resolver result dropped from the loop", "for outcome in TOOLCHAIN BUILD ",
             "for outcome in BUILD ", "run"),
            ("resolver result removed",
             "          TOOLCHAIN_OUTCOME: ${{ needs.gates-toolchain.result }}\n", "",
             "env.TOOLCHAIN_OUTCOME: missing"),
            ("resolver result replaced by success",
             "          TOOLCHAIN_OUTCOME: ${{ needs.gates-toolchain.result }}\n",
             "          TOOLCHAIN_OUTCOME: success\n", "env.TOOLCHAIN_OUTCOME"),
        ):
            with self.subTest(mutation=mutation):
                problems = self.edited_job("gates", old, new)
                self.assertReports(problems, "job gates, step qualify", field)

    def run_qualify(self, outcomes):
        """The reporter's final step under the given outcome variables: (exit code, stdout)."""
        with tempfile.TemporaryDirectory() as directory:
            proc = subprocess.run(
                ["bash", "-eo", "pipefail", "-c", self.qualify()["run"]], cwd=directory,
                env={**os.environ, **outcomes,
                     "GITHUB_STEP_SUMMARY": str(Path(directory) / "summary")},
                capture_output=True, text=True,
            )
        return proc.returncode, proc.stdout + proc.stderr

    def qualify(self):
        steps = ci_steps.load_yaml(self.text)["jobs"]["gates"]["steps"]
        return next(step for step in steps if step.get("id") == "qualify")

    @unittest.skipUnless(shutil.which("bash"), "requires bash")
    def test_reporter_counts_the_resolvers_own_outcome(self):
        # Every other outcome success: a state GitHub cannot produce, isolating the resolver's.
        env = self.qualify()["env"]
        self.assertEqual(env["TOOLCHAIN_OUTCOME"], "${{ needs.gates-toolchain.result }}")
        for result, code in (("success", 0), ("failure", 1), ("skipped", 1), ("cancelled", 1),
                             ("", 1)):
            with self.subTest(result=result):
                outcomes = {name: "success" for name in env}
                outcomes["TOOLCHAIN_OUTCOME"] = result
                status, output = self.run_qualify(outcomes)
                self.assertEqual(status, code, output)
                self.assertIn(f"TOOLCHAIN: {result}\n", output)

    @unittest.skipUnless(shutil.which("bash"), "requires bash")
    def test_a_failed_resolver_with_skipped_producers_turns_the_check_red(self):
        # The state GitHub produces when the resolver fails: the jobs that need it are skipped,
        # the reporter still runs (`!cancelled()`), and its gated steps are skipped after its
        # tools step refuses the empty toolchain.
        jobs = ci_steps.load_yaml(self.text)["jobs"]
        self.assertNotIn("gates-toolchain", jobs["gates"]["if"])
        outcomes = {}
        for name, value in self.qualify()["env"].items():
            outcomes[name] = ("failure" if "gates-toolchain" in value else "skipped")
        self.assertEqual(sum(value == "failure" for value in outcomes.values()), 1)
        status, output = self.run_qualify(outcomes)
        self.assertEqual(status, 1, output)
        self.assertIn("TOOLCHAIN: failure\n", output)
        for producer in ("BUILD", "SHORT", "ZA_ROCKS", "ZA_FILESYSTEM", "LISTINGS"):
            self.assertIn(f"{producer}: skipped\n", output)

    def test_retention_trigger_and_style_requirements_are_not_optional(self):
        checks = (
            ("      - style\n      - gates-toolchain\n      - gates-build\n",
             "      - gates-toolchain\n      - gates-build\n", "needs"),
            (" && needs.style.result == 'success'", "", "if"),
            (" && (github.event_name == 'workflow_dispatch' || github.event_name == 'schedule')",
             "", "if"),
        )
        for old, new, field in checks:
            with self.subTest(field=field, old=old):
                self.assertReports(self.edited(old, new), "job gates", field)
        self.assertReports(self.edited('  schedule:\n    - cron: "0 7 * * 1"\n', ""),
                           "test.yml", "on.schedule")

    def test_all_listing_commands_are_required(self):
        problems = self.edited('capture_listings.py --out "$GATES/listings"',
                               'capture_listings.py --out "$GATES/listings" '
                               "test.yml__test__imap-tests")
        self.assertReports(problems, "gates-listings", "step capture", "run")

    def test_report_downloads_complete_by_exact_artifact_ids(self):
        download = "          artifact-ids: ${{ needs.gates-za-filesystem.outputs.receipt-artifact-id }}\n"
        for replacement in ("", "          name: retention-za-filesystem\n",
                            "          artifact-ids: ${{ needs.gates-za-rocks.outputs.receipt-artifact-id }}\n"):
            with self.subTest(replacement=replacement):
                self.assertReports(self.edited(download, replacement), "job gates", "with.artifact-ids")
        problems = self.edited('          check_id za-rocks "$ZA_ROCKS_ID"\n', "")
        self.assertReports(problems, "artifact-ids", "run")

    def test_collector_and_qualification_cannot_be_disabled_or_bypassed(self):
        for old, new, step, field in (
            ("          ${{ !cancelled() && steps.unpack.outcome == 'success' &&\n",
             "          ${{ false && steps.unpack.outcome == 'success' &&\n",
             "collect", "if"),
            ("            --build-manifest \"$GATES/build.json\" --out \"$GATES/reports\" || status=$?\n",
             "            --build-manifest \"$GATES/build.json\" --out \"$GATES/reports\" || true\n",
             "collect", "run"),
            ('            if [[ "${!key}" != success ]]; then\n',
             '            if [[ "${!key}" != success && "${!key}" != skipped ]]; then\n',
             "qualify", "run"),
        ):
            with self.subTest(step=step, field=field):
                self.assertReports(self.edited(old, new), f"job gates, step {step}", field)

    def test_reporter_requires_each_successful_transport(self):
        def condition(predicates):
            return "          ${{ !cancelled() && " + " &&\n          ".join(predicates) + " }}\n"

        downloads = (
            "steps.download-short.outcome == 'success'",
            "steps.download-za-rocks.outcome == 'success'",
            "steps.download-za-filesystem.outcome == 'success'",
            "steps.download-listings.outcome == 'success'",
        )
        for step, prerequisite in (
            ("snapshot-download", ("steps.artifact-ids.outcome == 'success'",
                                   "steps.artifact-ids.outputs.snapshot == 'true'")),
            ("collect", ("steps.unpack.outcome == 'success'",)),
        ):
            predicates = prerequisite + downloads
            for predicate in predicates:
                with self.subTest(step=step, predicate=predicate):
                    remaining = tuple(test for test in predicates if test != predicate)
                    self.assertReports(self.edited(condition(predicates), condition(remaining)),
                                       f"job gates, step {step}", "if")

    def test_native_download_limits_and_work_budget_checks_are_required(self):
        old = ("      - name: Download short evidence\n"
               "        id: download-short\n"
               "        timeout-minutes: 5\n")
        self.assertReports(self.edited(old, old.replace("        timeout-minutes: 5\n", "")),
                           "job gates, step download-short", "timeout-minutes")
        old = "          if (( remaining < 41 * 60 )); then\n"
        self.assertReports(self.edited(old, "          if (( remaining < 1 )); then\n"),
                           "job gates, step artifact-ids", "run")
        old = "          if (( remaining < 21 * 60 )); then\n"
        self.assertReports(self.edited_shard(old, "          if (( remaining < 1 )); then\n"),
                           "job shard, step artifact-id", "run")

    def test_reporter_does_not_remove_work_budget_from_gate_commands(self):
        old = ("          bash scripts/tests/retention_ci_setup.sh within-budget \\\n"
               "            python3 -I scripts/tests/check_coverage.py")
        new = "          python3 -I scripts/tests/check_coverage.py"
        self.assertReports(self.edited(old, new), "job gates, step coverage", "run")

    def test_shard_artifact_is_uploaded_even_if_coverage_failed(self):
        old = ("      - name: Upload coverage evidence\n"
               "        id: receipt\n"
               "        if: ${{ !cancelled() && steps.stage.outcome == 'success' }}\n")
        new = old.replace("!cancelled()", "success()")
        self.assertReports(self.edited_shard(old, new), "job shard, step receipt", "if")
        problems = self.edited_shard("          SHARD_OUTCOME: ${{ steps.coverage.outcome }}\n",
                                     "          SHARD_OUTCOME: success\n")
        self.assertReports(problems, "job shard, step stage", "env.SHARD_OUTCOME")

    def test_seven_day_listing_diagnostics_allowlist_the_receipt(self):
        original = "          path: ${{ env.GATES }}/evidence/receipt.json\n"
        for replacement in ("", "          path: ${{ env.GATES }}/evidence/\n"):
            with self.subTest(replacement=replacement):
                self.assertReports(self.edited(original, replacement), "gates-listings",
                                   "Retain sanitized listing receipt", "with.path")
        days = original + "          if-no-files-found: ignore\n          retention-days: 7\n"
        self.assertReports(self.edited(days, days.replace("retention-days: 7", "retention-days: 1")),
                           "gates-listings", "Retain sanitized listing receipt", "retention-days")
        old = ("      - name: Retain sanitized listing receipt\n"
               "        if: ${{ !cancelled() && steps.stage.outcome == 'success' }}\n")
        self.assertReports(self.edited(old, old.replace("!cancelled()", "success()")),
                           "gates-listings", "Retain sanitized listing receipt", "if")

    def test_seven_day_shard_receipt_upload_uses_staged_allowlist(self):
        steps = {step["name"]: step
                 for step in ci_steps.load_yaml(self.shard)["jobs"]["shard"]["steps"]}
        name = "Retain sanitized shard receipt"
        self.assertIn(name, list(steps))
        self.assertEqual(steps[name]["if"], "${{ !cancelled() && steps.stage.outcome == 'success' }}")
        self.assertEqual(steps[name]["timeout-minutes"], "2")
        self.assertEqual(steps[name]["with"]["retention-days"], "7")
        self.assertEqual(steps[name]["with"]["path"].splitlines(), [
            "${{ env.GATES }}/evidence/receipt.json",
            "${{ env.GATES }}/evidence/logs/",
            "${{ env.GATES }}/evidence/steps.tsv",
        ])
        path = ("          path: |-\n"
                "            ${{ env.GATES }}/evidence/receipt.json\n"
                "            ${{ env.GATES }}/evidence/logs/\n"
                "            ${{ env.GATES }}/evidence/steps.tsv\n")
        self.assertReports(self.edited_shard(path, "          path: ${{ env.GATES }}/evidence/\n"),
                           name, "with.path")

    def test_seven_day_shard_fallback_retains_only_sanitized_diagnostics(self):
        original = "          path: ${{ runner.temp }}/shard-diagnostics/\n"
        for replacement in ("", "          path: ${{ env.GATES }}/evidence/profdata/\n",
                            "          path: ${{ env.GATES }}/net/logs/\n"):
            with self.subTest(replacement=replacement):
                self.assertReports(self.edited_shard(original, replacement),
                                   "Retain sanitized shard diagnostics", "with.path")
        days = original + "          if-no-files-found: ignore\n          retention-days: 7\n"
        shortened = days.replace("retention-days: 7", "retention-days: 1")
        self.assertReports(self.edited_shard(days, shortened),
                           "Retain sanitized shard diagnostics", "retention-days")
        stage = ("      - name: Stage sanitized shard diagnostics\n"
                 "        id: shard-diagnostics\n"
                 "        if: ${{ !cancelled() && steps.stage.outcome != 'success' }}\n")
        self.assertReports(self.edited_shard(stage, stage.replace(" && steps.stage.outcome != 'success'", "")),
                           "step shard-diagnostics", "if")
        timed_stage = stage + "        timeout-minutes: 3\n"
        self.assertReports(self.edited_shard(timed_stage, timed_stage.replace("3\n", "1\n")),
                           "step shard-diagnostics", "timeout-minutes")
        upload = ("      - name: Retain sanitized shard diagnostics\n"
                  "        if: ${{ !cancelled() && "
                  "steps.shard-diagnostics.outcome == 'success' }}\n")
        self.assertReports(self.edited_shard(upload, upload.replace("!cancelled()", "success()")),
                           "Retain sanitized shard diagnostics", "if")

    @unittest.skipUnless(shutil.which("bash") and shutil.which("jq"), "requires bash and jq")
    def test_merge_summary_distinguishes_profiles_without_exports(self):
        steps = ci_steps.load_yaml(self.text)["jobs"]["gates"]["steps"]
        command = next(step["run"] for step in steps if step.get("id") == "merge")
        for merged, missing_exports, code, summary in (
            ("{}", ["imap-plain", "jmap-plain"], 1,
             "Profiles included in merged coverage without a per-step export: jmap-plain\n"),
            ("", ["imap-plain", "jmap-plain"], 1,
             "Validated profiles without a per-step coverage export: jmap-plain\n"),
            ("{}", ["imap-plain"], 1, ""),
            ("{}", ["jmap-plain"], 0,
             "Profiles included in merged coverage without a per-step export: jmap-plain\n"),
        ):
            with self.subTest(merged=merged, missing_exports=missing_exports, code=code), \
                    tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                reports = root / "reports"
                (reports / "net").mkdir(parents=True)
                (reports / "collection.json").write_text(json.dumps({
                    "missing_profiles": ["imap-plain"] if "imap-plain" in missing_exports else [],
                    "missing_exports": missing_exports,
                }))
                fake_merge = root / "scripts/tests/coverage_net.py"
                fake_merge.parent.mkdir(parents=True)
                fake_merge.write_text(
                    "from pathlib import Path\n"
                    f"Path('reports/net/merged.json').write_text({merged!r})\n"
                    f"raise SystemExit({code})\n"
                )
                output = root / "outputs"
                output.touch()
                proc = subprocess.run(
                    ["bash", "-eo", "pipefail", "-c", command], cwd=root,
                    env={**os.environ, "GATES": str(root), "RETENTION_DEADLINE_EPOCH": "0",
                         "GITHUB_OUTPUT": str(output), "GITHUB_STEP_SUMMARY": str(root / "summary")},
                    capture_output=True, text=True,
                )
                self.assertEqual(proc.returncode, code, proc.stderr)
                self.assertEqual(output.read_text(), "available=true\n" if merged else "")
                self.assertEqual((root / "summary").read_text(), summary)

    def test_gate_pipelines_keep_failure_status(self):
        old = ("        if: ${{ !cancelled() && steps.functions.outcome == 'success' }}\n"
               '        shell: bash\n')
        self.assertReports(self.edited(old, old.replace("bash", "sh")),
                           "job gates, step inventory", "shell")
        old = '            --out "$GATES/reports/coverage.json" 2>&1 | tee "$GATES/reports/coverage.out"'
        self.assertReports(self.edited(old, old + " || true"), "job gates, step coverage", "run")

    def test_the_style_job_runs_this_check(self):
        # Final review N14: the step that runs the check is itself checked.
        step = ("      - name: Check Gates\n        run: |\n"
                "          python3 -I scripts/tests/ci_steps.py\n"
                "          python3 -I scripts/tests/test_gates.py\n"
                "          python3 -I scripts/tests/test_retention_artifacts.py\n"
                "          python3 -I scripts/tests/test_retention_evidence.py\n")
        problems = self.edited(step, "")
        self.assertReports(problems, "job style", "'Check Gates'", "missing")
        problems = self.edited(step, step.replace("Check Gates", "Gates"))
        self.assertReports(problems, "job style", "'Check Gates'", "missing")
        problems = self.edited(step, step.replace("        run: |\n",
                                                  "        if: false\n        run: |\n"))
        self.assertReports(problems, "job style", "'Check Gates'", "has if")
        problems = self.edited(step, step.replace("        run: |\n",
                                                  "        continue-on-error: true\n"
                                                  "        run: |\n"))
        self.assertReports(problems, "job style", "has continue-on-error")
        problems = self.edited(step, step.replace(
            "          python3 -I scripts/tests/test_gates.py\n", ""))
        self.assertReports(problems, "job style", "'Check Gates'", "runs")
        problems = self.edited("    name: Check Style\n",
                               "    name: Check Style\n    if: github.event_name == 'push'\n")
        self.assertReports(problems, "job style: has if")

    def test_unsupported_yaml_is_reported_not_skipped(self):
        problems = self.edited("  test:\n    name: Test\n", "  test: &test\n    name: Test\n")
        self.assertTrue(problems and problems[0].startswith("test.yml:"), problems)

    def test_the_net_and_the_capture_follow_the_table_and_the_baseline(self):
        # CI-only job diagnostics must not alter the historical measurement environment.
        with open(BASELINE, encoding="utf-8") as f:
            baseline = json.load(f)
        net = coverage_net.STEPS
        self.assertEqual(set(net) - {"build-A", "build-B"}, set(baseline["helpers"]["steps"]))
        for name, recorded in baseline["subtests"]["steps"].items():
            self.assertEqual(" ".join(net[name]["cmd"]), recorded["command"], name)
            env = dict(pair.split("=", 1) for pair in recorded["env"].split())
            self.assertEqual({**coverage_net.BASE_ENV, **net[name]["env"]}, env, name)
        for name, step in ci_steps.net_steps().items():
            self.assertEqual(net[name]["cmd"][4:], step.argv[2:], name)
            self.assertEqual(net[name]["env"], step.env, name)
        configurations = capture_listings.configurations("FEATURES")
        required = {name for name, c in baseline["libtest"]["configurations"].items()
                    if c["kind"] == "ci_step"}
        self.assertLessEqual(required, set(configurations))
        for name, step in ci_steps.captured_steps().items():
            self.assertEqual(configurations[name]["env"], step.env, name)
            cmd = [a for a in configurations[name]["cmd"] if a not in ci_steps.LISTING_TARGETS]
            argv = ci_steps.expand(step.argv, "FEATURES")
            if "--" not in argv:
                argv = argv + ["--"]
            self.assertEqual(cmd, [a for a in argv if a not in ci_steps.LISTING_TARGETS], name)


# ====================================================================== the retention drivers
class CaptureDriverTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def install_version_tool(self, name, output, status=0):
        path = self.root / name
        path.write_text(f"#!/bin/sh\nprintf '%s\\n' '{output}'\n"
                        f"printf '%s\\n' '{name} setup diagnostic' >&2\nexit {status}\n")
        path.chmod(0o755)

    def test_toolchain_identity_uses_only_stdout(self):
        self.install_version_tool("cargo", "cargo 9.8.7")
        self.install_version_tool("rustc", "rustc 9.8.7\nrelease: 9.8.7")
        with mock.patch.dict(os.environ, {"PATH": str(self.root)}), \
             contextlib.redirect_stdout(io.StringIO()), \
             contextlib.redirect_stderr(io.StringIO()) as diagnostics:
            identity = capture_listings.build_identity(None)
        self.assertIn("cargo setup diagnostic", diagnostics.getvalue())
        self.assertIn("rustc setup diagnostic", diagnostics.getvalue())
        self.assertEqual(identity["cargo"], "cargo 9.8.7")
        self.assertEqual(identity["rustc"], "rustc 9.8.7\nrelease: 9.8.7")

    def test_listing_capture_keeps_both_streams_on_failure(self):
        command = [sys.executable, "-c", "import sys; print('case: test', flush=True); "
                   "sys.stderr.write('compilation diagnostic\\n'); sys.exit(1)"]
        path = self.root / "capture.txt"
        spec = dict(label="fixture", environment="", sections=[("## SELECTED", command)], env={})
        with contextlib.redirect_stdout(io.StringIO()):
            statuses = capture_listings.capture(path, spec)
        self.assertEqual(statuses, [1])
        self.assertIn("case: test\ncompilation diagnostic\n## exit: 1", path.read_text())

    def test_toolchain_identity_keeps_exit_status(self):
        self.install_version_tool("cargo", "cargo 9.8.7", 3)
        with mock.patch.dict(os.environ, {"PATH": str(self.root)}), \
             contextlib.redirect_stdout(io.StringIO()), \
             contextlib.redirect_stderr(io.StringIO()) as diagnostics, \
             self.assertRaisesRegex(ValueError, r"cargo identity unavailable \(3\)"):
            capture_listings.build_identity(None)
        self.assertIn("cargo setup diagnostic", diagnostics.getvalue())

    def test_toolchain_identity_does_not_start_after_deadline(self):
        self.install_version_tool("cargo", "cargo 9.8.7", 3)
        with mock.patch.dict(os.environ, {"PATH": str(self.root)}), \
             self.assertRaisesRegex(ValueError, r"cargo identity unavailable \(not-run\)"):
            capture_listings.build_identity(time.monotonic() - 1)


class RunIdentityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.commit = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True,
        ).strip()

    def setUp(self):
        self.net = object.__new__(coverage_net.Net)
        self.net.deadline = None
        self.environment = {"PATH": os.environ["PATH"]}

    def test_net_and_listings_agree_on_hosted_identity(self):
        for sha_env in ({}, {"GITHUB_SHA": self.commit}):
            with self.subTest(sha_env=sha_env), mock.patch.dict(
                os.environ, {**self.environment, **sha_env, "GITHUB_RUN_ID": "193",
                             "GITHUB_RUN_ATTEMPT": "2"}, clear=True,
            ):
                expected = dict(commit=self.commit, run_id="193", producer_attempt=2)
                self.assertEqual(self.net.run_identity(), expected)
                self.assertEqual(capture_listings.run_identity(), expected)

    def test_hosted_identity_rejects_invalid_run_id_or_attempt(self):
        for run_id, attempt in (("local", "0"), ("001", "1"), ("+1", "1"), ("x", "1"),
                                ("12", "01"), ("12", "+1"), ("12", "0"), ("12", "")):
            with self.subTest(run_id=run_id, attempt=attempt), \
                 mock.patch.dict(os.environ, {**self.environment, "GITHUB_RUN_ID": run_id,
                                              "GITHUB_RUN_ATTEMPT": attempt}, clear=True):
                with self.assertRaises(ValueError):
                    self.net.run_identity()
                with self.assertRaises(ValueError):
                    capture_listings.run_identity()

    def test_net_does_not_treat_partial_hosted_identity_as_local(self):
        for env in ({"GITHUB_RUN_ATTEMPT": "1"}, {"GITHUB_ACTIONS": "true"}):
            with self.subTest(env=env), \
                 mock.patch.dict(os.environ, {**self.environment, **env}, clear=True), \
                 self.assertRaises(ValueError):
                self.net.run_identity()

    def test_net_keeps_local_identity_with_a_future_deadline(self):
        self.net.deadline = time.monotonic() + 60
        with mock.patch.dict(os.environ, self.environment, clear=True):
            self.assertEqual(self.net.run_identity(),
                             dict(commit=self.commit, run_id="local", producer_attempt=0))

    def test_net_rejects_a_deadline_passed_before_identity_check(self):
        self.net.deadline = time.monotonic() - 1
        with mock.patch.dict(os.environ, self.environment, clear=True), \
             self.assertRaisesRegex(ValueError, "deadline"):
            self.net.run_identity()


# ====================================================================== the coverage driver
class CoverageNetTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.net = object.__new__(coverage_net.Net)
        directory = Path(self.directory.name).resolve()
        self.net.out = directory / "net"
        self.net.target = directory / "target"
        self.net.target.mkdir()
        for name in ("steps", "logs", "profdata"):
            (self.net.out / name).mkdir(parents=True)
        self.net.failed = []
        self.net.deadline = None
        self.net.work_deadline = None
        self.net.run_identity = lambda: {
            "commit": "a" * 40, "run_id": "12", "producer_attempt": 3,
        }
        self.net.build_identity = lambda: {"llvm_cov": "LLVM 23.1.1"}

    def attempt(self, number, status, profile_status, names=()):
        profile = None
        if profile_status == "collected":
            profile = self.net.out / "profdata" / f"fixture-{number}"
            profile.write_text(str(number))
        return dict(attempt=number, status=status, profile=profile,
                    profile_status=profile_status, names=list(names))

    def export(self, step, profiles, destination):
        combined = "".join(path.read_text() for path in profiles)
        destination.write_text(combined)
        (self.net.out / "profdata" / f"{step}.profdata").write_text(combined)
        return True

    def test_shards_partition_baseline_in_order(self):
        baseline = json.loads(Path(BASELINE).read_text())["helpers"]["steps"]
        self.assertEqual(set(ci_steps.COVERAGE_SHARDS), {"za-rocks", "za-filesystem", "short"})
        self.assertEqual(ci_steps.COVERAGE_SHARDS["za-rocks"], ("za-tests",))
        self.assertEqual(ci_steps.COVERAGE_SHARDS["za-filesystem"], ("za-tests-fs-blob",))
        groups = list(ci_steps.COVERAGE_SHARDS.values())
        self.assertCountEqual([name for group in groups for name in group], baseline)
        for group in groups:
            self.assertEqual(list(group), sorted(group, key=coverage_net.ORDER.index))

    def test_build_only_stops_on_first_failed_build(self):
        for failing in ("build-A", "build-B", None):
            with self.subTest(failing=failing), contextlib.redirect_stdout(io.StringIO()):
                steps = []
                self.net.failed.clear()
                manifest = self.net.out / "build.json"
                manifest.unlink(missing_ok=True)

                def attempt(step, *args, **kwargs):
                    steps.append(step)
                    return self.attempt("build", int(step == failing), "discarded")

                with mock.patch.object(coverage_net, "Net", return_value=self.net), \
                     mock.patch.object(self.net, "attempt", side_effect=attempt), \
                     mock.patch.object(self.net, "build_manifest", create=True,
                                       return_value={"schema": 1}), \
                     mock.patch.object(sys, "argv", ["coverage_net.py", "--out",
                                                     str(self.net.out), "--build-only"]):
                    try:
                        status = coverage_net.main()
                    except SystemExit as error:
                        status = error.code
                self.assertEqual(status, int(failing is not None))
                self.assertEqual(steps, ["build-A"] if failing == "build-A" else
                                 ["build-A", "build-B"])
                self.assertEqual(manifest.exists(), failing is None)

    def test_smtp_retries_names_and_merges_both_attempts(self):
        outcomes = [self.attempt(1, 101, "collected", ["smtp::one", "smtp::two"]),
                    self.attempt(2, 0, "collected")]
        with mock.patch.object(self.net, "attempt", side_effect=outcomes) as run_attempt, \
             mock.patch.object(self.net, "export", side_effect=self.export), \
             contextlib.redirect_stdout(io.StringIO()):
            record = self.net.step("smtp")
        self.assertEqual(record["status"], "success")
        self.assertEqual([attempt["profile"] for attempt in record["attempts"]],
                         ["collected", "collected"])
        self.assertEqual(run_attempt.call_args.args[2], ["cargo", "llvm-cov", "--no-report",
                         "test", "-p", "tests", "--", "--exact", "--nocapture",
                         "smtp::one", "smtp::two"])
        self.assertEqual((self.net.out / "steps" / "smtp.json").read_text(), "12")

    def test_smtp_without_names_keeps_failed_profile_and_does_not_retry(self):
        with mock.patch.object(self.net, "attempt", return_value=
                               self.attempt(1, 101, "collected")) as run_attempt, \
             mock.patch.object(self.net, "export", side_effect=self.export), \
             contextlib.redirect_stdout(io.StringIO()):
            record = self.net.step("smtp")
        self.assertEqual(run_attempt.call_count, 1)
        self.assertEqual(record["status"], "failure")
        self.assertEqual(record["export"], "success")
        self.assertEqual((self.net.out / "steps" / "smtp.json").read_text(), "1")

    def test_normal_retry_uses_original_command_after_timeout(self):
        outcomes = [self.attempt(1, "timeout", "no-data"),
                    self.attempt(2, 101, "collected")]
        with mock.patch.object(self.net, "attempt", side_effect=outcomes) as run_attempt, \
             mock.patch.object(self.net, "export", side_effect=self.export), \
             contextlib.redirect_stdout(io.StringIO()):
            record = self.net.step("za-tests-fs-blob")
        first, second = [call.args for call in run_attempt.call_args_list]
        self.assertEqual(first[2:], second[2:])
        self.assertEqual(record["status"], "failure")
        self.assertEqual((self.net.out / "steps" / "za-tests-fs-blob.json").read_text(), "2")

    def test_profile_loss_is_reported_even_when_an_export_remains_possible(self):
        for first, second in (("merge-error", "collected"), ("collected", "no-data")):
            with self.subTest(first=first, second=second), \
                 mock.patch.object(self.net, "attempt", side_effect=[self.attempt(1, 101, first),
                                   self.attempt(2, 0, second)]), \
                 mock.patch.object(self.net, "export", side_effect=self.export), \
                 contextlib.redirect_stdout(io.StringIO()):
                record = self.net.step("za-tests")
                self.assertEqual(record["status"], "failure")
                self.assertEqual(record["export"], "success")
                self.assertEqual([a["profile"] for a in record["attempts"]], [first, second])
                export = self.net.out / "steps" / "za-tests.json"
                self.assertEqual(export.read_text(), "1" if first == "collected" else "2")
                export.unlink()

    def test_unit_listing_supplies_sorted_skip_arguments(self):
        names = [f"tests::n{i:03}" for i in range(90)]
        output = "\n".join(f"{name}: test" for name in reversed(names + names[:1]))
        listing = subprocess.CompletedProcess([], 0, stdout=output, stderr="")
        outcome = self.attempt(1, 0, "collected")
        self.net.deadline = time.monotonic() + 600
        self.net.work_deadline = self.net.deadline - 300
        with mock.patch.object(coverage_net, "run", return_value=listing) as cargo, \
             mock.patch.object(self.net, "attempt", return_value=outcome) as run_attempt, \
             mock.patch.object(self.net, "export", side_effect=self.export), \
             contextlib.redirect_stdout(io.StringIO()):
            self.net.step("unit-tests")
        self.assertEqual(cargo.call_args.args[0][-5:], ["test", "-p", "tests", "--", "--list"])
        self.assertEqual(cargo.call_args.kwargs["deadline"], self.net.work_deadline)
        self.assertEqual(run_attempt.call_args.args[2][-180:],
                         [arg for name in names for arg in ("--skip", name)])

    def test_manifest_rejects_added_missing_and_changed_objects(self):
        target = self.net.target
        first, second = target / "first", target / "second"
        first.write_bytes(b"initial")
        with mock.patch.object(self.net, "report_objects", create=True,
                               side_effect=lambda: sorted(target.iterdir())):
            self.assertTrue(hasattr(self.net, "build_manifest"))
            manifest = self.net.build_manifest()
            manifest["build_attempt"] = 2
            self.net.verify_build(manifest)  # A different producer attempt in this run is valid.
            second.write_bytes(b"unexpected")
            with self.assertRaisesRegex(ValueError, "object"):
                self.net.verify_build(manifest)
            second.unlink()
            first.write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "object"):
                self.net.verify_build(manifest)
            first.unlink()
            with self.assertRaisesRegex(ValueError, "object"):
                self.net.verify_build(manifest)

    def test_manifest_rejects_incompatible_inputs(self):
        first = self.net.target / "first"
        first.write_bytes(b"compiled")
        with mock.patch.object(self.net, "report_objects", create=True, return_value=[first]):
            self.assertTrue(hasattr(self.net, "build_manifest"))
            manifest = self.net.build_manifest()
            for key, value in (("commit", "b" * 40), ("run_id", "17"),
                               ("build_attempt", 4), ("identity", {"llvm_cov": "different"}),
                               ("object_set_sha256", "0" * 64)):
                with self.subTest(key=key), self.assertRaises(ValueError):
                    self.net.verify_build({**manifest, key: value})

    def test_object_inventory_accepts_a_symlink_alias_for_target_root(self):
        first = self.net.target / "first"
        first.write_bytes(b"compiled")
        alias = self.net.out / "target-alias"
        alias.symlink_to(self.net.target, target_is_directory=True)
        with mock.patch.object(self.net, "report_objects", return_value=[alias / "first"]):
            try:
                objects = self.net.object_inventory()
            except ValueError as error:
                self.fail(f"target directory alias was rejected: {error}")
        self.assertEqual([entry["path"] for entry in objects], ["first"])

    def test_deadline_prevents_starting_a_subprocess(self):
        marker = self.net.out / "launched"
        self.net.deadline = time.monotonic() - 1
        self.assertTrue(hasattr(self.net, "run"))
        result = self.net.run([sys.executable, "-c", "open(__import__('sys').argv[1], 'w')",
                               str(marker)])
        self.assertEqual(result.returncode, "deadline")
        self.assertFalse(marker.exists())

    def test_timeout_kills_residual_group_when_leader_exits_on_term(self):
        command = ["cargo", "test"]
        child = mock.MagicMock(pid=1234)
        child.__enter__.return_value = child
        child.communicate.side_effect = [subprocess.TimeoutExpired(command, 1), (None, None)]
        log = self.net.out / "attempt.log"
        with mock.patch.object(subprocess, "Popen", return_value=child), \
             mock.patch.object(os, "killpg") as kill, log.open("wb") as output:
            result = self.net.run(command, timeout=1, output=output, work=True)
        self.assertEqual(result.returncode, "timeout")
        self.assertEqual(kill.call_args_list, [mock.call(child.pid, signal.SIGTERM),
                                               mock.call(child.pid, signal.SIGKILL)])

    def test_timeout_bounds_communicate_after_killing_group(self):
        command = ["cargo", "metadata"]
        child = mock.MagicMock(pid=1234)
        child.__enter__.return_value = child
        child.communicate.side_effect = [subprocess.TimeoutExpired(command, 1),
                                         subprocess.TimeoutExpired(command, 60),
                                         subprocess.TimeoutExpired(command, 5, b"partial", b"error")]
        with mock.patch.object(subprocess, "Popen", return_value=child), \
             mock.patch.object(os, "killpg") as kill:
            result = self.net.run(command, timeout=1)
        self.assertEqual((result.returncode, result.stdout, result.stderr),
                         ("timeout", "partial", "error"))
        self.assertEqual(kill.call_args_list, [mock.call(child.pid, signal.SIGTERM),
                                               mock.call(child.pid, signal.SIGKILL)])
        self.assertEqual(child.communicate.call_args_list, [mock.call(timeout=1),
                                                            mock.call(timeout=60),
                                                            mock.call(timeout=5)])

    def test_shard_settles_earlier_steps_after_work_cutoff(self):
        self.net.deadline = time.monotonic() + 600
        self.net.work_deadline = self.net.deadline - 300
        manifest = {"build_attempt": 2, "object_set_sha256": "f" * 64}
        ran = []

        def finish_first_step(step):
            ran.append(step)
            self.net.work_deadline = time.monotonic() - 1
            return dict(status="success", attempts=[{"status": 0}], profile=True, export="success")

        with mock.patch.object(self.net, "verify_build") as verify, \
             mock.patch.object(self.net, "step", side_effect=finish_first_step), \
             contextlib.redirect_stdout(io.StringIO()):
            self.net.shard("short", manifest)
        self.assertEqual(ran, ["leak-unit-test"])
        self.assertEqual(verify.call_count, 2)
        summary = json.loads((self.net.out / "shard.json").read_text())
        self.assertTrue(summary["compatible"])
        self.assertEqual(summary["steps"]["leak-unit-test"]["status"], "success")
        self.assertEqual(summary["steps"]["unit-tests"]["status"], "not-run")
        self.assertTrue(self.net.failed)

    def test_raw_profile_is_merged_during_diagnostics_reserve(self):
        self.net.deadline = time.monotonic() + 600
        self.net.work_deadline = time.monotonic() - 1
        self.net.profdata_tool = "llvm-profdata"
        raw = self.net.target / "profile.profraw"

        def run_phase(cmd, *, deadline, **_):
            if cmd[0] == "cargo":
                self.assertEqual(deadline, self.net.work_deadline)
                raw.write_text("raw profile flushed by terminated cargo process")
                return subprocess.CompletedProcess(cmd, "deadline", "", "")
            self.assertEqual(deadline, self.net.deadline)
            self.assertEqual(Path(cmd[cmd.index("-f") + 1]).read_text(), f"{raw}\n")
            Path(cmd[cmd.index("-o") + 1]).write_text("indexed profile")
            return subprocess.CompletedProcess(cmd, 0, "", "")

        with mock.patch.object(coverage_net, "run", side_effect=run_phase):
            attempt = self.net.attempt("za-tests", 1, ["cargo", "test"], {}, 180 * 60)
        self.assertEqual(attempt["status"], "deadline")
        self.assertEqual(attempt["profile_status"], "collected")
        self.assertEqual(attempt["profile"].read_text(), "indexed profile")
        self.assertFalse(raw.exists())

    def test_hard_deadline_keeps_unmerged_raw_local(self):
        self.net.deadline = time.monotonic() - 1
        self.net.work_deadline = self.net.deadline - 300
        self.net.profdata_tool = "llvm-profdata"
        raw = self.net.target / "profile.profraw"
        raw.write_text("unmerged raw profile")

        attempt = self.net.attempt("za-tests", 1, ["cargo", "test"], {}, 180 * 60)
        self.net.clear_profiles()

        self.assertEqual(attempt["profile_status"], "merge-error")
        self.assertFalse(raw.exists())
        preserved = list(self.net.target.rglob("*.profraw"))
        self.assertEqual(len(preserved), 1)
        self.assertEqual(preserved[0].read_text(), "unmerged raw profile")
        self.assertFalse(list(self.net.out.rglob("*.profraw")))

    def test_partial_merge_publishes_diagnostic_with_missing_steps(self):
        path = self.net.out / "profdata" / "za-tests.profdata"
        path.write_text("measured")
        with mock.patch.object(self.net, "export", side_effect=self.export), \
             contextlib.redirect_stdout(io.StringIO()):
            self.net.merge_all()
        self.assertTrue(self.net.failed)
        report = self.net.out / "merged.json"
        self.assertEqual(report.read_text(), "measured")
        summary = json.loads((self.net.out / "merge.json").read_text())
        self.assertFalse(summary["complete"])
        self.assertEqual(summary["missing_steps"], [name for name in coverage_net.ORDER[2:]
                                                    if name != "za-tests"])


# ====================================================================== the real baseline
class RealBaselineTests(unittest.TestCase):
    """The 0.16.27 baseline's raw inputs fed as the candidate. `--real DIR` holds `list/` and
    `ign/` (the listing captures), `functions.json` (`strict_rule.py functions` on the 19 steps'
    exports), `merged.json` and `sources/` (the 5b914ddb5 tree)."""

    def setUp(self):
        if not REAL:
            self.skipTest("needs --real DIR")
        if not os.path.exists(os.path.join(REAL, "empty.json")):
            write_json(os.path.join(REAL, "empty.json"), {})

    def test_inventory(self):
        for disp in (os.path.join(REAL, "empty.json"), DISPOSITIONS):
            code, out = run(INVENTORY, "--baseline", BASELINE, "--dispositions", disp,
                            "--listing", os.path.join(REAL, "list"), "--ignored-listing", os.path.join(REAL, "ign"),
                            "--executed", os.path.join(REAL, "functions.json"))
            self.assertEqual(code, 0, out[-3000:])
            self.assertIn("'call_steps': 272", out)

    def test_coverage(self):
        code, out = run(COVERAGE, "--baseline", BASELINE, "--dispositions", os.path.join(REAL, "empty.json"),
                        "--merged", os.path.join(REAL, "merged.json"), "--sources", os.path.join(REAL, "sources"))
        self.assertEqual(code, 0, out)
        self.assertIn("cohort full: lines 17821/18490", out)
        self.assertIn("cohort runtime: lines 10646/11246", out)


if __name__ == "__main__":
    argv = list(sys.argv)
    if "--real" in argv:
        i = argv.index("--real")
        REAL = os.path.abspath(argv[i + 1])
        del argv[i:i + 2]
    unittest.main(argv=argv)
