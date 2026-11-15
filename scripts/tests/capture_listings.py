#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only
"""Capture the libtest listings the inventory gate reads (`--listing`, `--ignored-listing`).

Usage:
  capture_listings.py --out DIR [--deadline-epoch SECONDS] [CONFIGURATION ...]

Writes DIR/list/<configuration>.txt and DIR/ign/<configuration>--ignored-only.txt, each the
`-- --list` (or `-- --ignored --list`) output of one CI step's command, stdout and stderr
together, in the baseline's capture format. With no CONFIGURATION it captures all of them.

The CI steps' commands and environments come from `ci_steps.STEPS`, the one table both this
tool and `coverage_net.py` read, and which `test_gates.py` checks against
`.github/workflows/test.yml`; the configuration names are `test.yml__<job>__<step>`. The
workspace unit step's features come from `UNIT_TEST_FEATURES`, which test.yml sets once for
both jobs that use it; the variable is required. Tests crate steps list with `--lib --bins
--tests`, as the baseline's captures did, and filtered steps also capture the unfiltered listing
of the same binaries. Run it from any directory of the repository. Needs Python 3.9 or later
and no packages. Hosted mode also checkpoints listing.json with each section's outcome.
"""
import argparse
import datetime
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import ci_steps  # noqa: E402

# A listing that builds its binaries from cold can take a while; one that outlasts this is
# stopped (with its process group) and reported, instead of holding the job.
COMMAND_TIMEOUT = 45 * 60

REPO = ci_steps.REPO
BASE_ENV = ci_steps.BASE_ENV
TESTS = ci_steps.TESTS_CRATE + ci_steps.LISTING_TARGETS


def plain(cmd):
    return dict(cmd=cmd + ["--"], unfiltered=None, env={})


def configurations(unit_features):
    every = {}
    for name, step in ci_steps.captured_steps().items():
        cmd, unfiltered = ci_steps.listing_command(step, unit_features)
        every[name] = dict(cmd=cmd, unfiltered=unfiltered, env=dict(step.env))
    # Package captures whose ignored-only listings the baseline requires.
    every.update({
        "exists__nlp__default": plain(["cargo", "test", "-p", "nlp", "--lib", "--bins",
                                       "--tests"]),
        "exists__spam-filter__default": plain(["cargo", "test", "-p", "spam-filter", "--lib",
                                               "--bins", "--tests"]),
        "exists__tests__default": plain(list(TESTS)),
    })
    return every


def now():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def run(cmd, extra, deadline=None):
    remaining = None if deadline is None else deadline - time.monotonic()
    if remaining is not None and remaining <= 0:
        return "not-run", ""
    budget_limited = remaining is not None and remaining <= COMMAND_TIMEOUT
    timeout = remaining if budget_limited else COMMAND_TIMEOUT
    env = dict(os.environ)
    for key in ("ZA_KEY_ACCOUNTS", "BLOB_STORE", "SPAM_RULES_URL", "RUSTFLAGS"):
        env.pop(key, None)
    env.update(BASE_ENV)
    env.update(extra)
    print(f"{now()} start: {' '.join(cmd)[:200]}", flush=True)
    child = subprocess.Popen(cmd, cwd=REPO, env=env, stdin=subprocess.DEVNULL,
                             stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                             start_new_session=True)
    try:
        output, _ = child.communicate(timeout=timeout)
        status = child.returncode
    except subprocess.TimeoutExpired:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        output, _ = child.communicate()
        status = "deadline" if budget_limited else "timeout"
    print(f"{now()} exit {status}", flush=True)
    return status, output


def capture_contract(unit_features, selected=()):
    """The paths, labels and commands expected by the distributed collector."""
    every = configurations(unit_features)
    captures = {}
    for name in selected or list(every):
        spec = every[name]
        title = "## SELECTED"
        if spec["unfiltered"]:
            title += f" (filter: {spec['cmd'][len(TESTS)]})"
        environment = " ".join(f"{key}={value}" for key, value in {**BASE_ENV, **spec["env"]}.items())
        for directory, suffix, label in (("list", ["--list"], name),
                                         ("ign", ["--ignored", "--list"], name + "--ignored-only")):
            sections = [(title, spec["cmd"] + suffix)]
            if spec["unfiltered"]:
                sections.append(("## UNFILTERED (everything in the same binaries)",
                                 spec["unfiltered"] + suffix))
            captures[f"{directory}/{label}.txt"] = dict(label=label, environment=environment,
                                                       sections=sections, env=spec["env"])
    return captures


def capture(path, spec, deadline=None):
    lines = [f"# config: {spec['label']}", f"# env: {spec['environment']}", f"# started: {now()}"]
    statuses = []
    for index, (title, cmd) in enumerate(spec["sections"]):
        if index:
            lines.append("")
        lines.extend([title, "## command: " + " ".join(cmd)])
        if statuses and statuses[-1] in ("deadline", "not-run"):
            status, text = "not-run", ""
        else:
            status, text = run(cmd, spec["env"], deadline)
        statuses.append(status)
        lines.extend([text.rstrip("\n"), f"## exit: {status}"])
    if any(status != "not-run" for status in statuses):
        path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(spec["label"], statuses, flush=True)
    return statuses


def run_identity(deadline=None):
    remaining = 15 if deadline is None else min(15, deadline - time.monotonic())
    if remaining <= 0:
        raise ValueError("deadline before reading the checked-out commit")
    commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO, capture_output=True,
                            text=True, check=True, timeout=remaining).stdout.strip()
    if (not re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", commit)
            or commit != os.environ.get("GITHUB_SHA", commit)):
        raise ValueError("checkout does not match GITHUB_SHA")
    run_id = os.environ.get("GITHUB_RUN_ID", "")
    attempt = os.environ.get("GITHUB_RUN_ATTEMPT", "")
    if not all(re.fullmatch(r"[1-9][0-9]*", value) for value in (run_id, attempt)):
        raise ValueError("invalid GitHub workflow run identity")
    return dict(commit=commit, run_id=run_id, producer_attempt=int(attempt))


def build_identity(deadline):
    identity = {}
    for tool, flag in (("cargo", "-V"), ("rustc", "-vV")):
        status, output = run([tool, flag], {}, deadline)
        if status != 0:
            raise ValueError(f"{tool} identity unavailable ({status})")
        identity[tool] = output.strip()
    for key in ("ImageOS", "ImageVersion", "CARGO_TARGET_DIR"):
        if key in os.environ:
            identity[key] = os.environ[key]
    return identity


def checkpoint(out, ledger):
    temporary = out / "listing.tmp"
    temporary.write_text(json.dumps(ledger, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(out / "listing.json")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--deadline-epoch", type=int, help="stop starting work at this Unix time")
    parser.add_argument("configurations", nargs="*")
    args = parser.parse_args(argv)
    if ci_steps.refuse_diverged_workflow():
        return 1
    unit_features = " ".join(os.environ.get(ci_steps.UNIT_FEATURES, "").split())
    if not unit_features:
        parser.error("UNIT_TEST_FEATURES must be set (test.yml's workspace unit features)")
    every = configurations(unit_features)
    unknown = [c for c in args.configurations if c not in every]
    if unknown:
        parser.error("unknown configurations: " + " ".join(unknown))
    hosted = args.deadline_epoch is not None
    if hosted and (args.out.is_symlink() or (args.out.exists()
                    and (not args.out.is_dir() or any(args.out.iterdir())))):
        parser.error("hosted listing output must be fresh")
    for sub in ("list", "ign"):
        (args.out / sub).mkdir(parents=True, exist_ok=True)
    captures = capture_contract(unit_features, args.configurations)
    ledger = dict(schema=1, identity={}, unit_features=unit_features,
                  captures={path: ["not-run"] * len(spec["sections"])
                            for path, spec in captures.items()})
    deadline = None
    if hosted:
        deadline = time.monotonic() + (args.deadline_epoch - time.time())
        try:
            ledger.update(run_identity(deadline))
            ledger["identity"] = build_identity(deadline)
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            print(f"listing capture identity: {error}", file=sys.stderr)
            checkpoint(args.out, ledger)
            return 1
        checkpoint(args.out, ledger)
    ok = True
    for path, spec in captures.items():
        statuses = capture(args.out / path, spec, deadline)
        ok &= all(status == 0 for status in statuses)
        if hosted:
            ledger["captures"][path] = statuses
            checkpoint(args.out, ledger)
            if "deadline" in statuses or "not-run" in statuses:
                print("listing deadline reached", file=sys.stderr)
                break
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
