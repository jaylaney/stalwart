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
`tests/qualification/baseline-0.16.27.json`; the upgrade outcome note's part 7d1). A step that
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
import shutil
import signal
import subprocess
import sys
import time

REPO = Path(__file__).resolve().parents[2]
BASE_ENV = {"STORE": "RocksDb", "RUST_MIN_STACK": "16777216"}
# The SMTP step's rules release; the same URL as test.yml's SMTP step.
SPAM_URL = ("https://github.com/stalwartlabs/spam-filter/releases/download/v3.0.3/"
            "spam-filter-rules.json.gz")
# Leave dependency, toolchain and generated sources out of the exports, as the baseline did.
EXTERNAL_RE = r"(/\.cargo/|/rustc/|/\.rustup/|/target/)"
# Variables a step sets itself; any value from the caller is removed.
STEP_VARIABLES = ("ZA_KEY_ACCOUNTS", "BLOB_STORE", "SPAM_RULES_URL", "RUSTFLAGS")
LLVM_TEST = ["cargo", "llvm-cov", "--no-report", "test"]
WORKSPACE = ["--workspace", "--exclude-from-test", "scim", "--exclude-from-test", "scim-proto",
             "--lib", "--bins", "--tests", "--no-fail-fast"]


def tests_step(filter_, env=None, exact=False, rerun="same", timeout=3600):
    cmd = LLVM_TEST + ["-p", "tests", filter_, "--"] + (["--exact"] if exact else []) + [
        "--nocapture"]
    return dict(cmd=cmd, env=env or {}, rerun=rerun, timeout=timeout)


# The order is the baseline net's.
STEPS = {
    "build-A": dict(cmd=LLVM_TEST + WORKSPACE + ["--", "zz_no_such_test_zz"], env={},
                    rerun="build", timeout=4 * 3600),
    "build-B": dict(cmd=LLVM_TEST + ["-p", "tests", "zz_no_such_test_zz_2", "--", "--nocapture"],
                    env={}, rerun="build", timeout=4 * 3600),
    "leak-unit-test": tests_step("za::leak::tests::try_archive_short_values_are_not_archives",
                                 exact=True),
    # The tests crate's own tests are skipped here: the other steps run them.
    "unit-tests": dict(cmd=LLVM_TEST + WORKSPACE + ["--", "--exact"], env={}, rerun="same",
                       timeout=2 * 3600, skip_tests_crate=True),
    "za-tests": tests_step("za::za_tests", timeout=3 * 3600),
    "za-tests-fs-blob": tests_step("za::za_tests", env={"BLOB_STORE": "FileSystem"},
                                   timeout=3 * 3600),
    "webdav-plain": tests_step("webdav::webdav_tests"),
    "webdav-key": tests_step("webdav::webdav_tests", env={"ZA_KEY_ACCOUNTS": "1"}),
    "imap-plain": tests_step("imap::imap_tests"),
    "imap-key": tests_step("imap::imap_tests", env={"ZA_KEY_ACCOUNTS": "1"}),
    "jmap-plain": tests_step("jmap::jmap_tests"),
    "jmap-key": tests_step("jmap::jmap_tests", env={"ZA_KEY_ACCOUNTS": "1"}),
    "smtp": tests_step("smtp", env={"SPAM_RULES_URL": SPAM_URL}, rerun="by-name"),
    "system-tests": tests_step("system::system_tests"),
    "store-tests": tests_step("store::store_tests"),
    "blob-tests": tests_step("store::blob::blob_tests", timeout=1800),
    "lookup-tests": tests_step("store::lookup::lookup_tests", timeout=1800),
    "search-tests": tests_step("store::search_tests", timeout=2 * 3600),
    "telemetry-tests": tests_step("telemetry::telemetry_tests", timeout=1800),
    "cal-query-tests": tests_step("webdav::cal_query::", timeout=1800),
    "automation-tests": tests_step("automation::automation_tests", timeout=1800),
}


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
                retry = LLVM_TEST + ["-p", "tests", "--", "--exact", "--nocapture"] + names
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
