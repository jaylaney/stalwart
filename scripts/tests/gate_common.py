#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only
"""Shared parts of the two C10 gates: the dispositions file and the libtest listing captures.

The gates are `check_test_inventory.py` and `check_coverage.py` in this directory. They build on the
strict-rule tooling in `tests/qualification/tools/` (`strict_rule.py`, `cohort.py`, `rustscan.py`),
which this module puts on the import path.
"""
import json
import os
import re
import sys

sys.dont_write_bytecode = True  # keep the committed directories free of __pycache__

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
TOOLS = os.path.join(ROOT, "tests", "qualification", "tools")
if TOOLS not in sys.path:
    sys.path.insert(0, TOOLS)

KINDS = {
    "sel_only": "the subject is SEL-only code (design Decision 1); it is not ported",
    "subject_deleted": "1.0 removed the subject; the test or the part of it that asserted it is removed",
    "internals_replaced_equivalent": "1.0 replaced the internals; an equivalent test covers the same behaviour "
                                     "(named in `equivalents`, which the gate checks in its place)",
    "internals_replaced_no_equivalent": "1.0 replaced the internals; no equivalent test exists",
    "ungated_as_today": "outside the required set at 0.16.27 and still outside it (no CI step selects it)",
    "ignored_as_today": "`#[ignore]` at 0.16.27 and still ignored",
    "needs_docker_ci_only": "runs only in CI (it needs Docker), so the local net has no execution record",
}
# Kinds that let a required subject be absent, unselected or unexecuted.
EXCUSING = {"sel_only", "subject_deleted", "internals_replaced_equivalent", "internals_replaced_no_equivalent"}
SCOPES = {"libtest", "crate", "file", "sub_test_call", "function", "configuration"}


class InputError(Exception):
    pass


def load_json(path, what):
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError) as e:
        raise InputError(f"cannot read {what} {path}: {e}")


def norm_package(p):
    return p.replace("-", "_")


def test_key(package, name):
    return (norm_package(package), name)


def fn_identity(file, in_, name):
    """The strict-rule tooling's function identity (strict_rule.identity)."""
    return f"{file} | {in_ or '-'} | {name}"


def call_label(suite, call):
    return f"{suite} -> {call}"


# ------------------------------------------------------------------ dispositions
def _subject_of_baseline(d):
    """Convert one of the baseline's own dispositions (string subjects) to the file's form."""
    s = d["subject"]
    scope = d["scope"]
    if scope == "libtest":
        package, _, name = s.partition("::")
        return {"package": package, "name": name}
    if scope == "sub_test_call":
        suite, _, call = s.partition(" -> ")
        return {"suite": suite, "call": call}
    if scope == "crate":
        return {"crate": s}
    if scope == "file":
        return {"file": s}
    raise InputError(f"baseline disposition with unknown scope {scope!r}: {s}")


def baseline_dispositions(base):
    out = []
    for i, d in enumerate(base.get("dispositions", [])):
        out.append({"id": f"baseline#{i + 1}", "kind": d["kind"], "scope": d["scope"],
                    "subjects": [_subject_of_baseline(d)], "evidence": d.get("evidence", ""),
                    "unconfirmed": False, "origin": "baseline"})
    return out


def _check_subject(scope, s, where):
    need = {"libtest": ("package", "name"), "crate": ("crate",), "file": ("file",),
            "sub_test_call": ("suite", "call"), "function": ("file", "name"),
            "configuration": ("configuration",)}[scope]
    for k in need:
        if not isinstance(s.get(k), str) or not s[k]:
            raise InputError(f"{where}: subject {s} lacks {k!r}")


def load_dispositions(path, base):
    """The dispositions file plus the baseline's own dispositions. Validates every entry."""
    d = load_json(path, "dispositions file")
    if not isinstance(d, dict):
        raise InputError("the dispositions file must hold a JSON object")
    out = {"dispositions": baseline_dispositions(base), "renames": {"libtest": [], "functions": [],
           "steps": [], "configurations": []}, "selection_moves": [],
           "cohort": {"moves": [], "additions": [], "excluded_additions": []}}
    seen = set()
    for i, e in enumerate(d.get("dispositions", [])):
        where = f"dispositions[{i}] ({e.get('id', '?')})"
        if e.get("id") in seen or not e.get("id"):
            raise InputError(f"{where}: missing or duplicate id")
        seen.add(e["id"])
        if e.get("kind") not in KINDS:
            raise InputError(f"{where}: unknown kind {e.get('kind')!r}")
        if e.get("scope") not in SCOPES:
            raise InputError(f"{where}: unknown scope {e.get('scope')!r}")
        if not e.get("evidence"):
            raise InputError(f"{where}: no evidence")
        if not e.get("subjects"):
            raise InputError(f"{where}: no subjects")
        for s in e["subjects"]:
            _check_subject(e["scope"], s, where)
        if e["kind"] == "internals_replaced_equivalent":
            if not e.get("equivalents"):
                raise InputError(f"{where}: internals_replaced_equivalent needs `equivalents`")
            eq_scope = "libtest" if e["scope"] == "libtest" else "function"
            if e["scope"] not in ("libtest", "function", "sub_test_call"):
                raise InputError(f"{where}: equivalents are supported for libtest, function and sub_test_call scopes")
            for s in e["equivalents"]:
                _check_subject(eq_scope, s, where + " equivalent")
        out["dispositions"].append(dict(e, origin="file", unconfirmed=bool(e.get("unconfirmed"))))
    r = d.get("renames", {})
    for kind, need in (("libtest", ("package", "name")), ("functions", ("file", "name"))):
        for i, e in enumerate(r.get(kind, [])):
            where = f"renames.{kind}[{i}]"
            for side in ("from", "to"):
                s = e.get(side) or {}
                for k in need:
                    if not isinstance(s.get(k), str) or not s[k]:
                        raise InputError(f"{where}: {side} lacks {k!r}")
            if not e.get("evidence"):
                raise InputError(f"{where}: no evidence")
            out["renames"][kind].append(dict(e, unconfirmed=bool(e.get("unconfirmed"))))
    for kind in ("steps", "configurations"):
        for i, e in enumerate(r.get(kind, [])):
            if not (isinstance(e.get("from"), str) and isinstance(e.get("to"), str) and e.get("evidence")):
                raise InputError(f"renames.{kind}[{i}]: needs string from, to and evidence")
            out["renames"][kind].append(dict(e, unconfirmed=bool(e.get("unconfirmed"))))
    for i, e in enumerate(d.get("selection_moves", [])):
        where = f"selection_moves[{i}]"
        if not e.get("tests") or not e.get("from_configuration") or not e.get("to_configuration") or not e.get("evidence"):
            raise InputError(f"{where}: needs tests, from_configuration, to_configuration and evidence")
        for s in e["tests"]:
            _check_subject("libtest", s, where)
        out["selection_moves"].append(dict(e, unconfirmed=bool(e.get("unconfirmed"))))
    c = d.get("cohort", {})
    for i, e in enumerate(c.get("moves", [])):
        if not (e.get("from") and e.get("to") and e.get("evidence")):
            raise InputError(f"cohort.moves[{i}]: needs from, to and evidence")
        out["cohort"]["moves"].append(e)
    for i, e in enumerate(c.get("additions", [])):
        if not (e.get("file") and e.get("evidence")):
            raise InputError(f"cohort.additions[{i}]: needs file and evidence")
        out["cohort"]["additions"].append(e)
    for i, e in enumerate(c.get("excluded_additions", [])):
        if not (e.get("file") and e.get("reason")):
            raise InputError(f"cohort.excluded_additions[{i}]: needs file and reason")
        out["cohort"]["excluded_additions"].append(e)
    return out


# ------------------------------------------------------------------ listing captures
RUNNING = re.compile(r"^\s*Running\s+(?:unittests\s+)?(\S+)\s+\((.+)\)\s*$")
DOCTESTS = re.compile(r"^\s*Doc-tests\s+(\S+)")
ENTRY = re.compile(r"^(\S+): (test|bench|benchmark)$")
IGNORED_SUFFIX = "--ignored-only"


def exe_package(exe):
    base = os.path.basename(exe.strip().replace("\\", "/"))
    if base.endswith(".exe"):
        base = base[:-4]
    return norm_package(base.rsplit("-", 1)[0] if "-" in base else base)


def parse_capture(text, default_name):
    """One capture: the baseline's format (`# config:` header, `## SELECTED` and `## UNFILTERED`
    sections) or the raw output of `cargo test ... -- --list`. Returns
    {name, selected: set of keys, universe: set of keys, sectioned}."""
    name = default_name
    selected, unfiltered = set(), set()
    section = None
    sectioned = False
    package = None
    for line in text.splitlines():
        if line.startswith("# config:"):
            name = line.split(":", 1)[1].strip()
            continue
        if line.startswith("## SELECTED"):
            section, sectioned, package = "selected", True, None
            continue
        if line.startswith("## UNFILTERED"):
            section, sectioned, package = "unfiltered", True, None
            continue
        m = RUNNING.match(line)
        if m:
            package = exe_package(m.group(2))
            continue
        if DOCTESTS.match(line):
            package = None
            continue
        m = ENTRY.match(line)
        if m and package is not None:
            (unfiltered if section == "unfiltered" else selected).add((package, m.group(1)))
    return {"name": name, "selected": selected, "universe": selected | unfiltered, "sectioned": sectioned}


def load_captures(path, what, ignored=False):
    """A capture file, or a directory of them (every regular file, sorted). Keyed by configuration
    name; for ignored-only captures the `--ignored-only` suffix is dropped from the name."""
    if not os.path.exists(path):
        raise InputError(f"{what} {path} does not exist")
    files = [path] if os.path.isfile(path) else sorted(
        os.path.join(path, f) for f in os.listdir(path) if os.path.isfile(os.path.join(path, f)))
    out = {}
    for f in files:
        stem = os.path.basename(f)
        stem = stem[:-4] if stem.endswith(".txt") else stem
        with open(f, encoding="utf-8", errors="replace") as h:
            cap = parse_capture(h.read(), stem)
        if ignored and cap["name"].endswith(IGNORED_SUFFIX):
            cap["name"] = cap["name"][:-len(IGNORED_SUFFIX)]
        if cap["name"] in out:
            raise InputError(f"{what}: two captures name the configuration {cap['name']!r}")
        out[cap["name"]] = cap
    return out


def strip_ignored(name):
    return name[:-len(IGNORED_SUFFIX)] if name.endswith(IGNORED_SUFFIX) else name
