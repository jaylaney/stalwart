#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only
"""The C10 test-inventory gate (Task 7 of the 1.0.1 upgrade plan; Task 1's rulings 7, 31 and 32).

Usage:
  check_test_inventory.py --baseline PATH --dispositions PATH --listing PATH --ignored-listing PATH \
      --executed PATH

  --baseline         tests/qualification/baseline-0.16.27.json
  --dispositions     tests/qualification/dispositions.json
  --listing          a `cargo test ... -- --list` capture, or a directory of them: one file per
                     configuration (a package and feature set, or a CI step)
  --ignored-listing  the same for `-- --ignored --list`
  --executed         the output of `strict_rule.py functions` run on the candidate's per-step
                     llvm-cov exports and the candidate's sources (the strict rule, recomputed)

It exits 1 when one of these holds and no disposition covers it, and names the test, the step and the
count that failed:
  missing           a test that was listed at 0.16.27 is in no `--listing` capture;
  unselected        a test that ran at 0.16.27 is not selected by a CI step that selected it then;
  newly_ignored     a test that ran at 0.16.27 is in an `--ignored-listing` capture (no disposition
                    excuses this);
  empty_selector    a CI step that selected tests at 0.16.27 selects none;
  capture_missing   a required configuration has no capture (every CI step and every ignored-only
                    listing of the baseline, and a selection move's target where a moved test is not
                    selected by its old step);
  status_change     a test ignored or not selected at 0.16.27 has another status now;
  missing_from      a test is absent from a package and feature-set capture that listed it then;
  call_not_executed a sub-test call's target did not execute, under the three counts, in a step where
                    it executed at 0.16.27 (an omitted call, a future built but never awaited, an
                    early return before the last statement);
  loop_not_executed a loop-tailed sub-test body's loop ran no iteration where it had;
  helper_not_executed, tail_not_executed
                    the same for every other `tests/src` function, and for the `async { }` block a
                    function returns, in every step where it executed at 0.16.27 (ruling 31);
  rename_unconfirmed, move_unconfirmed, unconfirmed
                    a requirement that only an entry marked "unconfirmed" would satisfy.

"Executed" is the strict rule of `tests/qualification/tools/strict_rule.py`: the body record, the
region that starts the body's last top-level statement and the closing-brace exit region all have
positive counts in that step's export. Steps are compared step by step, never mode by mode.

Identifiers are matched exactly. A renamed or moved test or function is matched only through the
dispositions file's rename tables; a test whose CI step changed, only through its selection moves.
A test ignored at 0.16.27 that is still ignored and still listed passes. The baseline's own
dispositions apply as well as the file's. A disposition with a `steps` list applies only to
failures in those (baseline) steps; one of scope `case` documents a case retired inside a function
that still runs and is never applied (ruling 103). Exit status: 0 pass, 1 gate failure, 2 bad input.
"""
import argparse
import collections
import os
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from gate_common import (  # noqa: E402
    DOCUMENTATION_SCOPES, EXCUSING, InputError, call_label, fn_identity, load_captures, load_dispositions, load_json,
    norm_package, strip_ignored, test_key,
)


class Gate:
    def __init__(self, base, disp, listings, ignored, executed):
        self.base, self.disp = base, disp
        self.listings, self.ignored = listings, ignored
        self.failures, self.excused = [], collections.Counter()
        self.checked = collections.Counter()
        self.notes = []
        cfgs = base.get("libtest", {}).get("configurations", {})
        self.ci_configs = {k for k, v in cfgs.items() if v.get("kind") == "ci_step"}
        self.feature_configs = {k for k, v in cfgs.items() if v.get("kind") == "feature_set"}
        self.ignored_configs = {strip_ignored(k) for k, v in cfgs.items() if v.get("kind") == "ignored_listing"}
        self.cfg_selected = {k: v.get("selected", 0) for k, v in cfgs.items()}
        self.cfg_rename = {e["from"]: e for e in disp["renames"]["configurations"]}
        self.step_rename = {e["from"]: e for e in disp["renames"]["steps"]}
        self.test_rename = {test_key(e["from"]["package"], e["from"]["name"]): e for e in disp["renames"]["libtest"]}
        self.fn_rename = {fn_identity(e["from"]["file"], e["from"].get("in"), e["from"]["name"]): e
                          for e in disp["renames"]["functions"]}
        self.moves = {}
        for e in disp["selection_moves"]:
            for t in e["tests"]:
                self.moves[(test_key(t["package"], t["name"]), e["from_configuration"])] = e
        self.move_targets = {e["to_configuration"] for e in disp["selection_moves"]}
        self.universe = set()
        for cap in listings.values():
            self.universe |= cap["universe"]
        self.ignored_all = set()
        for cap in ignored.values():
            self.ignored_all |= cap["universe"]
        # candidate CI selection: captures named as a baseline CI step (after renames) or a move target
        ci_names = {self.cfg_name(c) for c in self.ci_configs} | self.move_targets
        self.ci_selected = set()
        for name, cap in listings.items():
            if name in ci_names:
                self.ci_selected |= cap["selected"]
        # executed functions
        if not isinstance(executed, dict) or "functions" not in executed or "steps" not in executed:
            raise InputError("--executed is not the output of `strict_rule.py functions`")
        self.cand_steps = set(executed["steps"])
        self.cand_fns = {f["key"]: f for f in executed["functions"]}
        helpers = base.get("helpers", {}).get("functions", [])
        self.helpers = {fn_identity(h["file"], h.get("in"), h["name"]): h for h in helpers}
        self.by_line = {(h["file"], h["line"], h["name"]): fn_identity(h["file"], h.get("in"), h["name"]) for h in helpers}
        self.call_target = {}
        for c in base.get("subtests", {}).get("calls", []):
            m = c.get("measured")
            if not m:
                continue
            file, line = m["target"].rsplit(":", 1)
            ident = self.by_line.get((file, int(line), c["call"].split("::")[-1]))
            if ident is None:
                # no body record at 0.16.27 (a call compiled out by its cfg): nothing to require
                if any((c.get("executed") or {}).values()):
                    raise InputError(f"baseline call {call_label(c['suite'], c['call'])}: executed, but no helper "
                                     f"at {m['target']}")
                continue
            self.call_target[call_label(c["suite"], c["call"])] = ident
        marker = base.get("subtests", {}).get("strict_rule", {}).get("needs_completion_marker") or []
        if marker:
            raise InputError("the baseline lists sub-tests that need a completion marker; this tool does not "
                             "implement completion markers yet")

    # ---------------------------------------------------------------- names
    def cfg_name(self, c):
        e = self.cfg_rename.get(c)
        return e["to"] if e else c

    def step_name(self, s):
        e = self.step_rename.get(s)
        return e["to"] if e else s

    # ---------------------------------------------------------------- dispositions
    def matching(self, *, test=None, call=None, ident=None, config=None):
        """Dispositions that name this subject: a libtest record, a call label, a function identity or
        a configuration name."""
        out = []
        files = set()
        if test is not None:
            files.add(test["source"].rsplit(":", 1)[0])
        if call is not None and call in self.call_target:
            ident = ident or self.call_target[call]
        if ident is not None:
            files.add(ident.split(" | ", 1)[0])
        for d in self.disp["dispositions"]:
            if d["scope"] in DOCUMENTATION_SCOPES:
                continue  # ruling 103: a retired case inside a running function; never applied
            for s in d["subjects"]:
                sc = d["scope"]
                hit = False
                if sc == "libtest" and test is not None:
                    hit = test_key(s["package"], s["name"]) == test_key(test["package"], test["name"])
                elif sc == "crate" and test is not None:
                    hit = test.get("crate") == s["crate"]
                elif sc == "file":
                    hit = s["file"] in files
                elif sc == "sub_test_call":
                    lab = call_label(s["suite"], s["call"])
                    hit = (call == lab) or (ident is not None and self.call_target.get(lab) == ident)
                elif sc == "function" and ident is not None:
                    hit = fn_identity(s["file"], s.get("in"), s["name"]) == ident
                elif sc == "configuration" and config is not None:
                    hit = s["configuration"] == config
                if hit:
                    out.append(d)
                    break
        return out

    def fail(self, check, subject, detail, step=None, covers=(), equivalent_check=None):
        """Record a failure unless a confirmed disposition of an excusing kind covers it. For an
        `internals_replaced_equivalent` disposition, `equivalent_check(disposition)` returns the
        problems of its equivalents, which fail in the subject's place."""
        where = f" [step {step}]" if step else ""
        confirmed, unconfirmed = [], []
        for d in covers:
            if d["kind"] not in EXCUSING or check == "newly_ignored":
                continue
            if d.get("steps") is not None and step not in d["steps"]:
                continue  # a disposition limited to named steps applies only to failures in them
            (unconfirmed if d.get("unconfirmed") else confirmed).append(d)
        for d in confirmed:
            if d["kind"] == "internals_replaced_equivalent" and equivalent_check is not None:
                problems = equivalent_check(d)
                if problems:
                    for p in problems:
                        self.failures.append(f"FAIL [{check}] {subject}{where}: excused by {d['id']} only if its "
                                             f"equivalent holds, and it does not: {p}")
                    return
            self.excused[(d["id"], d["kind"])] += 1
            return
        if unconfirmed:
            ids = ", ".join(d["id"] for d in unconfirmed)
            self.failures.append(f"FAIL [unconfirmed] {subject}{where}: {detail}; only the unconfirmed "
                                 f"disposition {ids} would excuse it (confirm it to pass)")
            return
        self.failures.append(f"FAIL [{check}] {subject}{where}: {detail}")

    # ---------------------------------------------------------------- libtest
    def resolve_test(self, t):
        """(candidate key, rename entry or None)."""
        k = test_key(t["package"], t["name"])
        if k in self.universe or k in self.ignored_all:
            return k, None
        e = self.test_rename.get(k)
        if e:
            return test_key(e["to"]["package"], e["to"]["name"]), e
        return k, None

    def selected_in(self, key, config):
        cap = self.listings.get(config)
        return cap is not None and key in cap["selected"]

    def required_selection(self, t, key, label):
        """Problems with the candidate's selection of `key` by every CI step that selected `t`."""
        problems = []
        old = test_key(t["package"], t["name"])
        for c in t.get("ci_selected_by", []):
            name = self.cfg_name(c)
            if name in self.listings and self.selected_in(key, name):
                continue
            move = self.moves.get((old, c))
            if move:
                to = move["to_configuration"]
                if to not in self.listings:
                    problems.append(("capture_missing", f"selection move target {to} has no capture", None))
                elif not self.selected_in(key, to):
                    problems.append(("unselected", f"not selected by {to} (moved from {c})", None))
                elif move.get("unconfirmed"):
                    problems.append(("move_unconfirmed", f"selected by {to} only through an unconfirmed "
                                     f"selection move from {c} (confirm it to pass)", None))
                continue
            if name not in self.listings:
                problems.append(("capture_missing", f"configuration {name} (selected it at 0.16.27) has no capture", None))
            else:
                problems.append(("unselected", f"not selected by {name}, which selected it at 0.16.27", None))
        return problems

    def check_tests(self):
        for t in self.base.get("libtest", {}).get("tests", []):
            label = f"test {t['package']}::{t['name']}"
            self.checked["tests"] += 1
            covers = self.matching(test=t)
            key, ren = self.resolve_test(t)
            if ren and ren.get("unconfirmed") and (key in self.universe):
                self.failures.append(f"FAIL [rename_unconfirmed] {label}: listed only as "
                                     f"{key[0]}::{key[1]} through an unconfirmed rename (confirm it to pass)")
            listed = key in self.universe
            ignored_now = key in self.ignored_all
            status = t["status"]

            def eq_check(d, t=t):
                out = []
                for e in d["equivalents"]:
                    ek = test_key(e["package"], e["name"])
                    if ek not in self.universe:
                        out.append(f"equivalent {e['package']}::{e['name']} is not listed")
                        continue
                    if ek in self.ignored_all:
                        out.append(f"equivalent {e['package']}::{e['name']} is ignored")
                    for _, p, _ in self.required_selection(t, ek, label):
                        out.append(f"equivalent {e['package']}::{e['name']}: {p}")
                return out

            if not listed:
                self.fail("missing", label, "not listed by any --listing capture", covers=covers, equivalent_check=eq_check)
                continue
            if status == "run":
                if ignored_now:
                    self.fail("newly_ignored", label, "ran at 0.16.27 and is now ignored", covers=covers)
                for check, p, _ in self.required_selection(t, key, label):
                    self.fail(check, label, p, covers=covers, equivalent_check=eq_check)
            elif status == "ignored":
                if not ignored_now:
                    now = "run" if key in self.ci_selected else "not_selected"
                    self.fail("status_change", label, f"ignored at 0.16.27, now {now}", covers=covers)
                for c in t.get("ignored_listed_in", []):
                    name = self.cfg_name(strip_ignored(c))
                    cap = self.ignored.get(name)
                    if cap is not None and key not in cap["universe"]:
                        self.fail("status_change", label, f"not in the ignored-only capture {name}, which listed it at 0.16.27",
                                  covers=covers)
            elif status == "not_selected":
                if ignored_now:
                    self.fail("status_change", label, "not selected at 0.16.27, now ignored", covers=covers)
                elif key in self.ci_selected:
                    self.fail("status_change", label, "not selected at 0.16.27, now selected by a CI step", covers=covers)
            # package and feature-set captures that listed it at 0.16.27
            if key[0] == norm_package(t["package"]):
                old = test_key(t["package"], t["name"])
                exempt = set()
                for c in t.get("listed_in", []):
                    for (k, frm), mv in self.moves.items():
                        if k == old:
                            exempt |= set(mv.get("absent_from", []))
                for c in t.get("listed_in", []):
                    if c not in self.feature_configs or c in exempt:
                        continue
                    name = self.cfg_name(c)
                    cap = self.listings.get(name)
                    if cap is not None and key not in cap["universe"]:
                        self.fail("missing_from", label, f"not listed by {name}, which listed it at 0.16.27",
                                  covers=covers, equivalent_check=eq_check)

    def check_configurations(self):
        required = {}
        for c in sorted(self.ci_configs):
            required[self.cfg_name(c)] = ("listing", c)
        for c in sorted(self.ignored_configs):
            required.setdefault(("ignored", self.cfg_name(c)), ("ignored", c))
        # a selection move's target is required where a moved test needs it (required_selection);
        # when it is captured, it must select something
        for t in sorted(self.move_targets):
            if t in self.listings:
                required.setdefault(t, ("listing", t))
        for name, (kind, orig) in required.items():
            self.checked["configurations"] += 1
            covers = self.matching(config=orig)
            if kind == "ignored":
                name = name[1]
                if name not in self.ignored:
                    self.fail("capture_missing", f"configuration {name}", "no --ignored-listing capture", covers=covers)
                continue
            if name not in self.listings:
                self.fail("capture_missing", f"configuration {name}", "no --listing capture", covers=covers)
                continue
            if self.cfg_selected.get(orig, 1) > 0 and not self.listings[name]["selected"]:
                self.fail("empty_selector", f"configuration {name}",
                          f"selects nothing (selected {self.cfg_selected.get(orig, '?')} at 0.16.27)", covers=covers)

    # ---------------------------------------------------------------- executed
    def counts(self, ident, step, tail=False):
        """(ok, detail) for function `ident` in candidate step `step` under the three counts."""
        f = self.cand_fns.get(ident)
        if f is None:
            return False, "no body record in any step's export (the function is gone or never entered)"
        v = (f.get("tail_steps") if tail else f.get("steps", {})).get(step)
        if v is None:
            return False, "no body record in this step's export (body count 0)"
        if v.get("executed"):
            return True, ""
        body, last, exit_ = v.get("body"), v.get("last"), v.get("exit")
        if not body:
            why = "body count 0: the body never ran (a future built but never awaited, or an omitted call)"
        elif not exit_:
            why = "final region count %s: the body never reached its closing brace" % exit_
        else:
            why = "last-statement count %s: the body returned before its last statement" % last
        return False, f"{why} (body {body}, last {last}, exit {exit_})"

    def resolve_fn(self, ident):
        """(identity to check, rename entry or None)."""
        if ident in self.cand_fns:
            return ident, None
        e = self.fn_rename.get(ident)
        if e:
            return fn_identity(e["to"]["file"], e["to"].get("in"), e["to"]["name"]), e
        return ident, None

    def check_fn(self, check, label, ident, step, covers, tail=False):
        self.checked[check.replace("_not_executed", "") + "_steps"] += 1
        cstep = self.step_name(step)
        if cstep not in self.cand_steps:
            self.fail(check, label, f"step {cstep} has no export in --executed", step=step, covers=covers)
            return
        target, ren = self.resolve_fn(ident)
        ok, detail = self.counts(target, cstep, tail)
        if ok and ren and ren.get("unconfirmed"):
            self.failures.append(f"FAIL [rename_unconfirmed] {label} [step {step}]: executed only as {target}, "
                                 f"through an unconfirmed rename (confirm it to pass)")
            return
        if ok:
            return
        if ren:
            detail = f"as {target} (renamed): {detail}"

        def eq_check(d):
            out = []
            for e in d["equivalents"]:
                eid = fn_identity(e["file"], e.get("in"), e["name"])
                eok, edetail = self.counts(eid, cstep, tail)
                if not eok:
                    out.append(f"equivalent {eid} in step {step}: {edetail}")
            return out

        self.fail(check, label, detail, step=step, covers=covers, equivalent_check=eq_check)

    def check_executed(self):
        done = set()
        for c in self.base.get("subtests", {}).get("calls", []):
            lab = call_label(c["suite"], c["call"])
            ident = self.call_target.get(lab)
            if ident is None:
                continue
            covers = self.matching(call=lab, ident=ident)
            label = f"call {lab} ({ident})"
            for step, ran in sorted((c.get("executed") or {}).items()):
                if not ran:
                    continue
                self.check_fn("call_not_executed", label, ident, step, covers)
                done.add((ident, step))
            lb = (c.get("measured") or {}).get("loop_body")
            for step in (lb or {}).get("required_in", []):
                self.checked["loop_steps"] += 1
                cstep = self.step_name(step)
                target, _ = self.resolve_fn(ident)
                v = self.cand_fns.get(target, {}).get("steps", {}).get(cstep)
                n = v.get("loop") if v else None
                if not n:
                    self.fail("loop_not_executed", label, f"loop-body count {n} (positive at 0.16.27: "
                              f"{lb['counts'].get(step)}); the loop ran no iteration", step=step, covers=covers)
        for ident, h in sorted(self.helpers.items()):
            covers = None
            for step in sorted(h.get("executed") or {}):
                if (ident, step) in done:
                    continue
                covers = covers if covers is not None else self.matching(ident=ident)
                self.check_fn("helper_not_executed", f"function {ident}", ident, step, covers)
            tb = h.get("tail_block")
            for step in sorted((tb or {}).get("executed") or {}):
                covers = covers if covers is not None else self.matching(ident=ident)
                self.check_fn("tail_not_executed", f"tail block of {ident}", ident, step, covers, tail=True)

    def run(self):
        self.check_configurations()
        self.check_tests()
        self.check_executed()


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--baseline", required=True)
    ap.add_argument("--dispositions", required=True)
    ap.add_argument("--listing", required=True)
    ap.add_argument("--ignored-listing", required=True)
    ap.add_argument("--executed", required=True)
    args = ap.parse_args(argv)
    try:
        base = load_json(args.baseline, "baseline")
        disp = load_dispositions(args.dispositions, base)
        listings = load_captures(args.listing, "--listing")
        ignored = load_captures(args.ignored_listing, "--ignored-listing", ignored=True)
        executed = load_json(args.executed, "--executed")
        gate = Gate(base, disp, listings, ignored, executed)
        gate.run()
    except InputError as e:
        print(f"ERROR: {e}", file=sys.stderr)
        return 2
    for f in gate.failures:
        print(f)
    by_kind = collections.Counter()
    for (did, kind), n in gate.excused.items():
        by_kind[kind] += n
    print(f"captures: {len(listings)} listing, {len(ignored)} ignored-only; steps in --executed: "
          f"{len(gate.cand_steps)}; functions: {len(gate.cand_fns)}")
    print("checked:", dict(sorted(gate.checked.items())))
    print("excused by dispositions:", dict(sorted(by_kind.items())) or "none")
    print("result:", f"FAIL ({len(gate.failures)} failures)" if gate.failures else "OK")
    return 1 if gate.failures else 0


if __name__ == "__main__":
    sys.exit(main())
