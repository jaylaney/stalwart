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
import shutil
import subprocess
import sys
import tempfile
import unittest
import warnings

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gate_common  # noqa: E402,F401  (puts tests/qualification/tools on the path)
import capture_listings  # noqa: E402
import ci_steps  # noqa: E402
import cohort  # noqa: E402
import coverage_net  # noqa: E402
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
def exe(package):
    return f"target/debug/deps/{gate_common.norm_package(package)}-0000000000000000"


def capture(name, selected, universe=None):
    """A capture in the baseline's format: SELECTED, then UNFILTERED when it differs."""
    def body(keys):
        by_pkg = {}
        for p, n in sorted(keys):
            by_pkg.setdefault(p, []).append(n)
        lines = []
        for p, names in sorted(by_pkg.items()):
            lines.append(f"     Running unittests src/lib.rs ({exe(p)})")
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

    def gate(self, listing=None, ignored=None, executed=None, dispositions=None):
        listing = self.listing if listing is None else listing
        ignored = self.ignored if ignored is None else ignored
        for d, caps in (("list", listing), ("ign", ignored)):
            shutil.rmtree(os.path.join(self.tmp, d), ignore_errors=True)
            os.makedirs(os.path.join(self.tmp, d))
            for name, (sel, uni) in caps.items():
                write(os.path.join(self.tmp, d, name + ".txt"), capture(name, sel, uni))
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

    def edited(self, old, new):
        self.assertEqual(self.text.count(old), 1, f"anchor not unique in test.yml: {old!r}")
        return ci_steps.check_workflow(self.text.replace(old, new))

    def assertReports(self, problems, *parts):
        self.assertTrue(any(all(part in problem for part in parts) for problem in problems),
                        f"no problem names {parts}: {problems}")

    def test_the_committed_workflow_matches_the_table(self):
        self.assertEqual(ci_steps.check_workflow(self.text), [])

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
        problems = self.edited("    timeout-minutes: 120\n\n    env:\n      STORE: RocksDb\n",
                               "    timeout-minutes: 120\n\n    env:\n      STORE: Sqlite\n")
        self.assertReports(problems, "job zero-access", "STORE")

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

    def test_the_gates_job_runs_the_whole_net_and_every_capture(self):
        problems = self.edited('coverage_net.py --out "$GATES/net"',
                               'coverage_net.py --out "$GATES/net" za-tests')
        self.assertReports(problems, "job gates", "Coverage net")
        problems = self.edited('capture_listings.py --out "$GATES/listings"',
                               'capture_listings.py --out "$GATES/listings" '
                               "test.yml__test__imap-tests")
        self.assertReports(problems, "job gates", "Capture CI step listings")

    def test_the_style_job_runs_this_check(self):
        # Final review N14: the step that runs the check is itself checked.
        step = ("      - name: Check Gates\n        run: |\n"
                "          python3 -I scripts/tests/ci_steps.py\n"
                "          python3 -I scripts/tests/test_gates.py\n")
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
