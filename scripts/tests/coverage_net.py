#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only
"""The 19-step `cargo llvm-cov --no-report` net that feeds both C10 gates.

Usage:
  coverage_net.py --out DIR [STEP ...]
  coverage_net.py --list

With no STEP it runs the two builds, then the baseline's 19 steps in their order, then merges
every step's profile. It writes:

  DIR/steps/<step>.json   each step's `llvm-cov export` JSON, from that step's profile alone
                          (the inventory gate's `strict_rule.py functions --exports`)
  DIR/merged.json         every step's profile merged (the coverage gate's `--merged`)
  DIR/steps.tsv           one row per attempt: command, environment, exit status, counts
  DIR/logs/               each attempt's cargo output, and the profile and export logs
  DIR/profdata/           each step's merged profile

Step names and commands are the baseline's (`helpers.steps` in
`tests/qualification/baseline-0.16.27.json`; the upgrade outcome note's part 7d1). Each step
that measures a CI step takes its command and environment from `ci_steps.STEPS`, the table
`capture_listings.py` reads too and `test_gates.py` checks against test.yml and the baseline;
the two builds and the unit step are the baseline's own commands. A step that
fails is run once more: the same command, except `smtp`, which reruns its failed tests by name
(the documented timing flakes). Every attempt's profile counts. The exit status is 1 when a step
failed both attempts or a build or export failed; every other step still runs and is exported,
so the gates can name what is missing. Run it under `STORE=RocksDb RUST_MIN_STACK=16777216`, from
any directory of the repository; cargo-llvm-cov and the `llvm-tools-preview` component must be
installed. Needs Python 3.9 or later and no packages.
"""
import argparse
import datetime
import glob
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import subprocess
import sys
import time

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import ci_steps  # noqa: E402

REPO = ci_steps.REPO
BASE_ENV = ci_steps.BASE_ENV
# Leave dependency, toolchain and generated sources out of the exports, as the baseline did.
EXTERNAL_RE = r"(/\.cargo/|/rustc/|/\.rustup/|/target/)"
# Variables a step sets itself; any value from the caller is removed.
STEP_VARIABLES = ("ZA_KEY_ACCOUNTS", "BLOB_STORE", "SPAM_RULES_URL", "RUSTFLAGS")
LLVM_TEST = ["cargo", "llvm-cov", "--no-report", "test"]
WORKSPACE = ["--workspace", "--exclude-from-test", "scim", "--exclude-from-test", "scim-proto",
             "--lib", "--bins", "--tests", "--no-fail-fast"]
# The baseline net's order, and each step's timeout in seconds (default one hour).
ORDER = ["build-A", "build-B", "leak-unit-test", "unit-tests", "za-tests", "za-tests-fs-blob",
         "webdav-plain", "webdav-key", "imap-plain", "imap-key", "jmap-plain", "jmap-key", "smtp",
         "system-tests", "store-tests", "blob-tests", "lookup-tests", "search-tests",
         "telemetry-tests", "cal-query-tests", "automation-tests"]
TIMEOUTS = {"za-tests": 3 * 3600, "za-tests-fs-blob": 3 * 3600, "blob-tests": 1800,
            "lookup-tests": 1800, "search-tests": 2 * 3600, "telemetry-tests": 1800,
            "cal-query-tests": 1800, "automation-tests": 1800}


def ci_step(name):
    """A net step measuring a CI step: its command and environment are that step's."""
    step = ci_steps.net_steps()[name]
    return dict(cmd=ci_steps.net_command(step, LLVM_TEST), env=dict(step.env),
                rerun="by-name" if step.rerun == "by-name" else "same",
                timeout=TIMEOUTS.get(name, 3600))


OWN_STEPS = {
    "build-A": dict(cmd=LLVM_TEST + WORKSPACE + ["--", "zz_no_such_test_zz"], env={},
                    rerun="build", timeout=4 * 3600),
    "build-B": dict(cmd=LLVM_TEST + ["-p", "tests", "zz_no_such_test_zz_2", "--", "--nocapture"],
                    env={}, rerun="build", timeout=4 * 3600),
    # The tests crate's own tests are skipped here: the other steps run them.
    "unit-tests": dict(cmd=LLVM_TEST + WORKSPACE + ["--", "--exact"], env={}, rerun="same",
                       timeout=2 * 3600, skip_tests_crate=True),
}
STEPS = {name: OWN_STEPS[name] if name in OWN_STEPS else ci_step(name) for name in ORDER}
# A failed `smtp` attempt reruns its failed tests by name, as test.yml's SMTP step does.
BY_NAME_RETRY = LLVM_TEST + shlex.split(ci_steps.BY_NAME_RETRY)[2:-1]


def now():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def run(cmd, **kw):
    return subprocess.run(cmd, cwd=REPO, capture_output=True, text=True, **kw)


class Net:
    def __init__(self, out):
        self.out = out
        for sub in ("steps", "logs", "profdata"):
            (out / sub).mkdir(parents=True, exist_ok=True)
        meta = json.loads(run(["cargo", "metadata", "--no-deps", "--format-version", "1"],
                              check=True).stdout)
        self.target = Path(os.environ.get("CARGO_LLVM_COV_TARGET_DIR")
                           or Path(meta["target_directory"]) / "llvm-cov-target")
        sysroot = run(["rustc", "--print", "sysroot"], check=True).stdout.strip()
        tools = glob.glob(sysroot + "/lib/rustlib/*/bin/llvm-profdata")
        if not tools:
            sys.exit("llvm-profdata not found: rustup component add llvm-tools-preview")
        self.profdata_tool = tools[0]
        self.failed = []

    def env(self, extra):
        env = dict(os.environ)
        for key in STEP_VARIABLES:
            env.pop(key, None)
        env.update(BASE_ENV)
        env.update(extra)
        return env

    def profraws(self):
        return sorted(glob.glob(str(self.target / "*.profraw")))

    def clear_profiles(self):
        for path in self.profraws() + glob.glob(str(self.target / "*.profdata")):
            os.remove(path)

    def row(self, values):
        tsv = self.out / "steps.tsv"
        new = not tsv.exists()
        with open(tsv, "a") as output:
            if new:
                output.write("\t".join(["step", "attempt", "command", "environment", "start",
                                        "end", "exit_status", "passed", "failed", "ignored",
                                        "failing_tests", "notes"]) + "\n")
            output.write("\t".join(str(value).replace("\t", " ").replace("\n", " ")
                                   for value in values) + "\n")

    def tests_crate_skips(self):
        """`--skip` arguments for every test of the tests crate, from its instrumented listing."""
        result = run(LLVM_TEST + ["-p", "tests", "--", "--list"], env=self.env({}))
        names = sorted(set(re.findall(r"^(\S+): test$", result.stdout, re.M)))
        if result.returncode != 0 or len(names) < 90:
            raise RuntimeError(f"tests crate listing failed (exit {result.returncode}, "
                               f"{len(names)} tests)")
        return [arg for name in names for arg in ("--skip", name)]

    def attempt(self, step, attempt, cmd, extra, timeout, keep=True):
        log = self.out / "logs" / f"{step}.attempt{attempt}.log"
        start, began = now(), time.time()
        with open(log, "wb") as output:
            child = subprocess.Popen(cmd, cwd=REPO, env=self.env(extra), stdin=subprocess.DEVNULL,
                                     stdout=output, stderr=subprocess.STDOUT,
                                     start_new_session=True)
            try:
                status = child.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=60)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait()
                status = "timeout"
        passed, failed, ignored, names, notes = parse_log(log)
        shown = " ".join(cmd)
        if len(shown) > 600:
            shown = (shown[:300] + f" ... [{cmd.count('--skip')} --skip args elided] ... "
                     + shown[-100:])
        self.row([step, attempt, shown,
                  " ".join(f"{k}={v}" for k, v in {**BASE_ENV, **extra}.items()), start, now(),
                  status, passed, failed, ignored, " ".join(names),
                  " | ".join(notes + [f"wall={int(time.time() - began)}s"])])
        raws = self.profraws()
        profile = None
        if keep and raws:
            profile = self.out / "profdata" / f"{step}.attempt{attempt}.profdata"
            listing = self.out / "profdata" / f"{step}.attempt{attempt}.list"
            listing.write_text("\n".join(raws) + "\n")
            merged = run([self.profdata_tool, "merge", "-sparse", "-f", str(listing), "-o",
                          str(profile)])
            listing.unlink()
            (self.out / "logs" / f"{step}.attempt{attempt}.profdata-merge.log").write_text(
                f"{len(raws)} profraw files\n{merged.stdout}\n{merged.stderr}\n")
            if merged.returncode != 0:
                profile = None
        for path in raws:
            os.remove(path)
        return dict(status=status, names=names, profile=profile)

    def export(self, name, profiles, destination):
        """Merge `profiles` into one and export it with the baseline's filename filter."""
        merged = self.out / "profdata" / f"{name}.profdata"
        if len(profiles) == 1:
            shutil.copyfile(profiles[0], merged)
        else:
            result = run([self.profdata_tool, "merge", "-sparse", "-o", str(merged)]
                         + [str(p) for p in profiles])
            if result.returncode != 0:
                return False
        self.clear_profiles()
        # cargo llvm-cov report merges every profile in its target directory; an indexed
        # profile is a valid merge input.
        staged = self.target / f"_step_{name}.profraw"
        shutil.copyfile(merged, staged)
        try:
            result = run(["cargo", "llvm-cov", "report", "--json",
                          "--no-default-ignore-filename-regex", "--ignore-filename-regex",
                          EXTERNAL_RE, "--output-path", str(destination)], env=self.env({}))
            (self.out / "logs" / f"{name}.export.log").write_text(
                result.stdout + "\n" + result.stderr)
            return result.returncode == 0
        finally:
            staged.unlink(missing_ok=True)
            self.clear_profiles()

    def step(self, step):
        spec = STEPS[step]
        self.clear_profiles()
        print(f"[{step}] start {now()}", flush=True)
        cmd = list(spec["cmd"])
        if spec.get("skip_tests_crate"):
            cmd += self.tests_crate_skips()
        if spec["rerun"] == "build":
            result = self.attempt(step, "build", cmd, spec["env"], spec["timeout"], keep=False)
            print(f"[{step}] build exit {result['status']}", flush=True)
            if result["status"] != 0:
                self.failed.append(step)
            return
        attempts = [self.attempt(step, 1, cmd, spec["env"], spec["timeout"])]
        if attempts[0]["status"] != 0:
            retry = cmd
            if spec["rerun"] == "by-name":
                names = attempts[0]["names"]
                retry = BY_NAME_RETRY + names
                if not names:
                    retry = None
            if retry is not None:
                print(f"[{step}] attempt 1 failed ({' '.join(attempts[0]['names'])}); "
                      "running it once more", flush=True)
                attempts.append(self.attempt(step, 2, retry, spec["env"], spec["timeout"]))
        if attempts[-1]["status"] != 0:
            self.failed.append(step)
        profiles = [a["profile"] for a in attempts if a["profile"]]
        if not profiles or not self.export(step, profiles, self.out / "steps" / f"{step}.json"):
            print(f"[{step}] no export", flush=True)
            self.failed.append(f"{step} (export)")
        print(f"[{step}] done {now()}: exit {attempts[-1]['status']}", flush=True)

    def merge_all(self):
        profiles = [self.out / "profdata" / f"{s}.profdata" for s in STEPS
                    if (self.out / "profdata" / f"{s}.profdata").exists()]
        if not profiles or not self.export("merged", profiles, self.out / "merged.json"):
            self.failed.append("merged (export)")
        print(f"[merged] {len(profiles)} step profiles", flush=True)


def parse_log(path):
    """Passed, failed and ignored totals, the failing tests' names and up to six panics."""
    text = open(path, errors="replace").read()
    passed = failed = ignored = 0
    for m in re.finditer(r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; "
                         r"(\d+) ignored", text, re.M):
        passed, failed, ignored = (passed + int(m.group(1)), failed + int(m.group(2)),
                                   ignored + int(m.group(3)))
    names, collecting = [], False
    for line in text.splitlines():
        if line == "failures:":
            collecting = True
        elif collecting and re.match(r"^    [A-Za-z_][A-Za-z0-9_:]*$", line):
            if line.strip() not in names:
                names.append(line.strip())
        elif collecting and (line.startswith("---- ") or line.startswith("test result:")):
            collecting = False
    notes = [f"{m.group(1)} @ {m.group(2)}" for m in re.finditer(
        r"thread '([^']+)'(?: \(\d+\))? panicked at ([^\n]+?):?\n", text)][:6]
    return passed, failed, ignored, names, notes


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--list", action="store_true", help="print the steps and exit")
    parser.add_argument("steps", nargs="*", help="steps to run (default: all, then the merge)")
    args = parser.parse_args()
    if args.list:
        for name, spec in STEPS.items():
            print(name, " ".join(spec["cmd"]), spec["env"] or "")
        return 0
    if args.out is None:
        parser.error("--out is required")
    unknown = [s for s in args.steps if s not in STEPS]
    if unknown:
        parser.error("unknown steps: " + " ".join(unknown))
    net = Net(args.out.resolve())
    for step in args.steps or list(STEPS):
        net.step(step)
        if step.startswith("build-") and step in net.failed:
            print(f"STOP: {step} failed", flush=True)
            return 1
    if not args.steps:
        net.merge_all()
    if net.failed:
        print("FAILED: " + ", ".join(net.failed), flush=True)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
