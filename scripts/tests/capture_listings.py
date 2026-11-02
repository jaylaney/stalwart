#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only
"""Capture the libtest listings the inventory gate reads (`--listing`, `--ignored-listing`).

Usage:
  capture_listings.py --out DIR [CONFIGURATION ...]

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
and no packages.
"""
import argparse
import datetime
import os
from pathlib import Path
import signal
import subprocess
import sys

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


def run(cmd, extra):
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
        output, _ = child.communicate(timeout=COMMAND_TIMEOUT)
        status = child.returncode
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL)
        output, _ = child.communicate()
        status = "timeout"
    print(f"{now()} exit {status}", flush=True)
    return status, output


def capture(out, name, spec, ignored, unit_features):
    suffix = ["--ignored", "--list"] if ignored else ["--list"]
    label = name + ("--ignored-only" if ignored else "")
    environment = " ".join(f"{k}={v}" for k, v in {**BASE_ENV, **spec["env"]}.items())
    lines = [f"# config: {label}", f"# env: {environment}", f"# started: {now()}"]
    statuses = []
    sections = [("## SELECTED" + (f" (filter: {spec['cmd'][len(TESTS)]})"
                                  if spec["unfiltered"] else ""), spec["cmd"])]
    if spec["unfiltered"]:
        sections.append(("## UNFILTERED (everything in the same binaries)", spec["unfiltered"]))
    for index, (title, cmd) in enumerate(sections):
        if index:
            lines.append("")
        lines.append(title)
        cmd = cmd + suffix
        lines.append("## command: " + " ".join(cmd))
        status, text = run(cmd, spec["env"])
        statuses.append(status)
        lines += [text.rstrip("\n"), f"## exit: {status}"]
    directory = out / ("ign" if ignored else "list")
    (directory / f"{label}.txt").write_text("\n".join(lines) + "\n")
    print(label, statuses, flush=True)
    return all(status == 0 for status in statuses)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("configurations", nargs="*")
    args = parser.parse_args()
    if ci_steps.refuse_diverged_workflow():
        return 1
    unit_features = os.environ.get("UNIT_TEST_FEATURES", "").strip()
    if not unit_features:
        parser.error("UNIT_TEST_FEATURES must be set (test.yml's workspace unit features)")
    every = configurations(" ".join(unit_features.split()))
    unknown = [c for c in args.configurations if c not in every]
    if unknown:
        parser.error("unknown configurations: " + " ".join(unknown))
    for sub in ("list", "ign"):
        (args.out / sub).mkdir(parents=True, exist_ok=True)
    ok = True
    for name in args.configurations or list(every):
        for ignored in (False, True):
            ok &= capture(args.out, name, every[name], ignored, unit_features)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
