#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Stalwart contributors
# SPDX-License-Identifier: AGPL-3.0-only
"""The 19-step `cargo llvm-cov --no-report` net that feeds both C10 gates.

Usage:
  coverage_net.py --out DIR [STEP ...]
  coverage_net.py --out DIR --build-only
  coverage_net.py --out DIR --shard ID --build-manifest FILE
  coverage_net.py --check-build --build-manifest FILE
  coverage_net.py --out DIR --merge-only --build-manifest FILE
  coverage_net.py --list

With no STEP it runs the two builds, then the baseline's 19 steps in their order, then merges
every step's profile. It writes:

  DIR/steps/<step>.json   each step's `llvm-cov export` JSON, from that step's profile alone
                          (the inventory gate's `strict_rule.py functions --exports`)
  DIR/merged.json         every step's profile merged (the coverage gate's `--merged`)
  DIR/steps.tsv           one row per attempt: command, environment, exit status, counts
  DIR/logs/               each attempt's cargo output, and the profile and export logs
  DIR/profdata/           each step's merged profile

Build mode writes build.json for the prepared report objects. Shard mode records the
settled steps in shard.json and verifies that the report objects still match the build.
Merge mode verifies the same objects and consumes the collected step profiles in
DIR/profdata. An incomplete merge returns failure and names missing steps in merge.json.
With --deadline-epoch, --diagnostics-reserve-seconds stops test work early enough to
settle available profiles and verify the final report objects before the deadline.

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
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import capture_listings  # noqa: E402
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
BUILD_STEPS = tuple(name for name in ORDER if STEPS[name]["rerun"] == "build")
COVERAGE_STEPS = tuple(name for name in ORDER if name not in BUILD_STEPS)
# A failed `smtp` attempt reruns its failed tests by name, as test.yml's SMTP step does.
BY_NAME_RETRY = LLVM_TEST + shlex.split(ci_steps.BY_NAME_RETRY)[2:-1]
REPORT = ["cargo", "llvm-cov", "report", "--no-default-ignore-filename-regex",
          "--ignore-filename-regex", EXTERNAL_RE]
SCHEMA = 1


def now():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def run(cmd, *, env=None, timeout=None, deadline=None, output=None, check=False):
    """Run one phase in its own process group with a per-phase timeout and job deadline."""
    remaining = None if deadline is None else deadline - time.monotonic()
    budget_limited = remaining is not None and (timeout is None or remaining <= timeout)
    if remaining is not None and remaining <= 0:
        status, stdout, stderr = "deadline", "", ""
    else:
        with subprocess.Popen(cmd, cwd=REPO, env=env, stdin=subprocess.DEVNULL,
                              stdout=subprocess.PIPE if output is None else output,
                              stderr=subprocess.PIPE if output is None else subprocess.STDOUT,
                              text=True, start_new_session=True) as child:
            try:
                stdout, stderr = child.communicate(timeout=remaining if budget_limited else timeout)
                status = child.returncode
            except subprocess.TimeoutExpired:
                stdout, stderr = stop_after_timeout(child)
                status = "deadline" if budget_limited else "timeout"
    if check and status != 0:
        raise RuntimeError(f"{' '.join(cmd[:3])} failed ({status})")
    return subprocess.CompletedProcess(cmd, status, stdout or "", stderr or "")


def terminate(child, sig):
    try:
        os.killpg(child.pid, sig)
    except ProcessLookupError:
        pass


def stop_after_timeout(child):
    terminate(child, signal.SIGTERM)
    try:
        return child.communicate(timeout=60)
    except subprocess.TimeoutExpired:
        pass
    finally:
        # With output redirected to a file, communicate can return when the leader exits
        # even though other processes in its group can still write profiles or hold ports.
        terminate(child, signal.SIGKILL)
    try:
        return child.communicate(timeout=5)
    except subprocess.TimeoutExpired as error:
        # Processes outside the group can keep inherited pipes open after group cleanup.
        return ((error.stdout or b"").decode(errors="replace"),
                (error.stderr or b"").decode(errors="replace"))


def canonical_json(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")


def write_json(path, value):
    temporary = path.with_suffix(".tmp")
    temporary.write_bytes(canonical_json(value) + b"\n")
    temporary.replace(path)


class Net:
    def __init__(self, out, deadline_epoch=None, diagnostics_reserve_seconds=0):
        self.out = out
        self.deadline = (None if deadline_epoch is None else
                         time.monotonic() + (deadline_epoch - time.time()))
        self.work_deadline = (None if self.deadline is None else
                              self.deadline - diagnostics_reserve_seconds)
        for sub in ("steps", "logs", "profdata"):
            (out / sub).mkdir(parents=True, exist_ok=True)
        meta = json.loads(self.run(["cargo", "metadata", "--no-deps", "--format-version", "1"],
                                   check=True).stdout)
        self.target = (REPO / (os.environ.get("CARGO_LLVM_COV_TARGET_DIR")
                               or Path(meta["target_directory"]) / "llvm-cov-target")).resolve()
        libdir = self.run(["rustc", "--print", "target-libdir"], check=True).stdout.strip()
        tools = Path(libdir).parent / "bin"
        self.profdata_tool = str(tools / "llvm-profdata")
        self.cov_tool = str(tools / "llvm-cov")
        if not Path(self.profdata_tool).is_file() or not Path(self.cov_tool).is_file():
            raise RuntimeError("LLVM tools not found: rustup component add llvm-tools-preview")
        self.failed = []

    def run(self, cmd, *, work=False, **kwargs):
        return run(cmd, deadline=self.work_deadline if work else self.deadline, **kwargs)

    def env(self, extra):
        env = dict(os.environ)
        for key in STEP_VARIABLES:
            env.pop(key, None)
        env.update(BASE_ENV)
        env.update(extra)
        return env

    def run_identity(self):
        return capture_listings.run_identity(self.deadline, allow_local=True)

    def build_identity(self):
        # Hash the instrumentation environment without adding arbitrary environment values
        # to the public manifest. Workspace paths must match Cargo's saved dependency paths.
        env = self.env({})
        env_text = self.run(["cargo", "llvm-cov", "show-env", "--sh"], env=env, check=True).stdout
        flag_prefixes = ("CARGO_BUILD_", "CARGO_PROFILE_", "CARGO_TARGET_", "CFLAGS", "CXXFLAGS")
        flag_names = {"AR", "CC", "CXX", "CARGO_ENCODED_RUSTFLAGS", "RUSTC", "RUSTDOC",
                      "LLVM_COV_FLAGS", "LLVM_PROFDATA_FLAGS"}
        flags = {key: value for key, value in env.items()
                 if key.startswith(flag_prefixes) or key in flag_names}
        return {
            "workspace": str(REPO.resolve()),
            "target": str(self.target),
            "cargo_home": str(Path(env.get("CARGO_HOME") or Path.home() / ".cargo").resolve()),
            "cargo": self.run(["cargo", "-V"], check=True).stdout.strip(),
            "rustc": self.run(["rustc", "-vV"], check=True).stdout.strip(),
            "cc": self.run(["cc", "--version"], env=env, check=True).stdout.strip(),
            "cxx": self.run(["c++", "--version"], env=env, check=True).stdout.strip(),
            "cargo_llvm_cov": self.run(["cargo", "llvm-cov", "--version"], check=True).stdout.strip(),
            "llvm_cov": self.run([self.cov_tool, "--version"], check=True).stdout.strip(),
            "llvm_profdata": self.run([self.profdata_tool, "--version"], check=True).stdout.strip(),
            "lockfile_sha256": self.digest(REPO / "Cargo.lock"),
            "cargo_config_sha256": self.digest(REPO / ".cargo" / "config.toml"),
            "instrumentation_sha256": hashlib.sha256(canonical_json([env_text, flags])).hexdigest(),
        }

    def digest(self, path):
        digest = hashlib.sha256()
        with path.open("rb") as source:
            for data in iter(lambda: source.read(1024 * 1024), b""):
                if self.deadline is not None and time.monotonic() >= self.deadline:
                    raise RuntimeError("deadline while hashing report objects")
                digest.update(data)
        return digest.hexdigest()

    def report_objects(self):
        """Let the pinned cargo-llvm-cov discover the objects without producing coverage."""
        self.clear_profiles()
        with tempfile.TemporaryDirectory(dir=self.out) as directory:
            scratch = Path(directory)
            profile = scratch / "empty.txt"
            profile.write_text("retention_manifest_placeholder\n# Func Hash:\n0\n"
                               "# Num Counters:\n1\n# Counter Values:\n0\n")
            capture = scratch / "capture-objects"
            capture.write_text(
                "#!/usr/bin/env python3\n"
                "import json, sys\n"
                "args = sys.argv[1:]\n"
                "if args[:1] != ['export']:\n"
                "    sys.exit('expected an llvm-cov export')\n"
                "print(json.dumps([value for key, value in zip(args, args[1:]) if key == '-object']))\n"
            )
            capture.chmod(0o700)
            staged = self.target / "_step_object_probe.profraw"
            output = scratch / "objects.json"
            try:
                self.run([self.profdata_tool, "merge", "-sparse", "-o", str(staged), str(profile)],
                         check=True)
                # The LCOV output path copies the wrapper's stdout verbatim. The report's
                # object discovery is shared with the JSON report used for the actual exports.
                self.run(REPORT + ["--lcov", "--output-path", str(output)], check=True, env=self.env(
                    {"LLVM_COV": str(capture), "LLVM_PROFDATA": self.profdata_tool}))
                return [REPO / path for path in json.loads(output.read_text())]
            finally:
                self.clear_profiles()

    def object_inventory(self):
        # Canonicalize aliases for the target's parent without collapsing different object names.
        report_paths = sorted(path.parent.resolve() / path.name for path in self.report_objects())
        if not report_paths or len(set(report_paths)) != len(report_paths):
            raise ValueError("empty or duplicate report objects")
        objects = []
        for path in report_paths:
            if not path.is_relative_to(self.target) or not path.resolve().is_relative_to(self.target):
                raise ValueError(f"report object outside instrumented target: {path}")
            objects.append({"path": path.relative_to(self.target).as_posix(), "sha256": self.digest(path)})
        return objects

    def build_manifest(self):
        run_info = self.run_identity()
        objects = self.object_inventory()
        return dict(schema=SCHEMA, commit=run_info["commit"], run_id=run_info["run_id"],
                    build_attempt=run_info["producer_attempt"], identity=self.build_identity(),
                    objects=objects, object_set_sha256=hashlib.sha256(canonical_json(objects)).hexdigest())

    def verify_build(self, manifest):
        run_info = self.run_identity()
        first_attempt = 0 if run_info["run_id"] == "local" else 1
        if (not isinstance(manifest, dict) or manifest.get("schema") != SCHEMA
                or manifest.get("commit") != run_info["commit"]
                or manifest.get("run_id") != run_info["run_id"]
                or type(manifest.get("build_attempt")) is not int
                or not first_attempt <= manifest["build_attempt"] <= run_info["producer_attempt"]):
            raise ValueError("build manifest does not match this run, commit or attempt")
        if manifest.get("identity") != self.build_identity():
            raise ValueError("build manifest toolchain or path identity differs")
        objects = self.object_inventory()
        if (objects != manifest.get("objects") or hashlib.sha256(canonical_json(objects)).hexdigest()
                != manifest.get("object_set_sha256")):
            raise ValueError("build manifest report objects differ")

    def profraws(self):
        return sorted(glob.glob(str(self.target / "*.profraw")))

    def clear_profiles(self):
        for pattern in ("*.profraw", "*.profdata", "*-profraw-list"):
            for path in self.target.glob(pattern):
                path.unlink()

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
        result = self.run(LLVM_TEST + ["-p", "tests", "--", "--list"], env=self.env({}), work=True)
        (self.out / "logs" / "unit-tests.listing.log").write_text(result.stdout + result.stderr)
        names = sorted(set(re.findall(r"^(\S+): test$", result.stdout, re.M)))
        if result.returncode != 0 or len(names) < 90:
            raise RuntimeError(f"tests crate listing failed (exit {result.returncode}, "
                               f"{len(names)} tests)")
        return [arg for name in names for arg in ("--skip", name)]

    def attempt(self, step, attempt, cmd, extra, timeout, keep=True):
        log = self.out / "logs" / f"{step}.attempt{attempt}.log"
        start, began = now(), time.monotonic()
        with open(log, "wb") as output:
            status = self.run(cmd, env=self.env(extra), timeout=timeout, output=output,
                              work=True).returncode
        passed, failed, ignored, names, notes = parse_log(log)
        shown = " ".join(cmd)
        if len(shown) > 600:
            shown = (shown[:300] + f" ... [{cmd.count('--skip')} --skip args elided] ... "
                     + shown[-100:])
        self.row([step, attempt, shown,
                  " ".join(f"{k}={v}" for k, v in {**BASE_ENV, **extra}.items()), start, now(),
                  status, passed, failed, ignored, " ".join(names),
                  " | ".join(notes + [f"wall={int(time.monotonic() - began)}s"])])
        raws = self.profraws()
        profile = None
        unmerged = None
        profile_status = "no-data" if keep else "discarded"
        if keep and raws:
            profile = self.out / "profdata" / f"{step}.attempt{attempt}.profdata"
            listing = self.out / "profdata" / f"{step}.attempt{attempt}.list"
            listing.write_text("\n".join(raws) + "\n")
            merged = self.run([self.profdata_tool, "merge", "-sparse", "-f", str(listing), "-o",
                               str(profile)])
            listing.unlink()
            (self.out / "logs" / f"{step}.attempt{attempt}.profdata-merge.log").write_text(
                f"{len(raws)} profraw files, exit {merged.returncode}\n"
                f"{merged.stdout}\n{merged.stderr}\n")
            if merged.returncode == 0 and profile.is_file():
                profile_status = "collected"
            else:
                profile.unlink(missing_ok=True)
                profile = None
                profile_status = "merge-error"
                if merged.returncode == "deadline":
                    # Keep timed-out raw data outside the report search and receipt trees.
                    unmerged = self.target / "_unmerged_profiles" / f"{step}.attempt{attempt}"
                    unmerged.mkdir(parents=True, exist_ok=True)
        for raw in map(Path, raws):
            if unmerged is None:
                raw.unlink()
            else:
                raw.replace(unmerged / raw.name)
        return dict(attempt=attempt, status=status, names=names, profile=profile,
                    profile_status=profile_status)

    def export(self, name, profiles, destination):
        """Merge `profiles` into one and export it with the baseline's filename filter."""
        merged = self.out / "profdata" / f"{name}.profdata"
        if len(profiles) == 1:
            shutil.copyfile(profiles[0], merged)
        else:
            result = self.run([self.profdata_tool, "merge", "-sparse", "-o", str(merged)]
                              + [str(p) for p in profiles])
            (self.out / "logs" / f"{name}.profdata-merge.log").write_text(
                f"{len(profiles)} input profiles, exit {result.returncode}\n"
                f"{result.stdout}\n{result.stderr}\n")
            if result.returncode != 0 or not merged.is_file():
                merged.unlink(missing_ok=True)
                return False
        self.clear_profiles()
        # cargo llvm-cov report merges every profile in its target directory; an indexed
        # profile is a valid merge input.
        staged = self.target / f"_step_{name}.profraw"
        shutil.copyfile(merged, staged)
        try:
            result = self.run(REPORT + ["--json", "--output-path", str(destination)],
                              env=self.env({}))
            (self.out / "logs" / f"{name}.export.log").write_text(
                f"exit {result.returncode}\n{result.stdout}\n{result.stderr}")
            if result.returncode == 0 and destination.is_file():
                return True
            destination.unlink(missing_ok=True)
            return False
        finally:
            staged.unlink(missing_ok=True)
            self.clear_profiles()

    def step(self, step):
        spec = STEPS[step]
        self.clear_profiles()
        print(f"[{step}] start {now()}", flush=True)
        cmd = list(spec["cmd"])
        if spec.get("skip_tests_crate"):
            try:
                cmd += self.tests_crate_skips()
            except RuntimeError as error:
                print(f"[{step}] {error}", flush=True)
                self.failed.append(step)
                return dict(status="failure", attempts=[], profile=False, export="not-attempted")
        if spec["rerun"] == "build":
            result = self.attempt(step, "build", cmd, spec["env"], spec["timeout"], keep=False)
            print(f"[{step}] build exit {result['status']}", flush=True)
            if result["status"] != 0:
                self.failed.append(step)
            return
        destination = self.out / "steps" / f"{step}.json"
        merged = self.out / "profdata" / f"{step}.profdata"
        destination.unlink(missing_ok=True)
        merged.unlink(missing_ok=True)
        attempts = [self.attempt(step, 1, cmd, spec["env"], spec["timeout"])]
        if attempts[0]["status"] not in (0, "deadline"):
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
        successful = attempts[-1]["status"] == 0
        if not successful:
            self.failed.append(step)
        profile_loss = any(a["profile_status"] == "merge-error" or
                           (a["status"] == 0 and not a["profile"]) for a in attempts)
        if profile_loss:
            self.failed.append(f"{step} (profile collection)")
        profiles = [a["profile"] for a in attempts if a["profile"]]
        exported = bool(profiles) and self.export(step, profiles, destination)
        if not exported:
            print(f"[{step}] no export", flush=True)
            self.failed.append(f"{step} (export)")
        print(f"[{step}] done {now()}: exit {attempts[-1]['status']}", flush=True)
        return {
            "status": "success" if successful and exported and not profile_loss else "failure",
            "attempts": [dict(attempt=a["attempt"], status=a["status"], names=a["names"],
                              profile=a["profile_status"]) for a in attempts],
            "profile": merged.is_file(),
            "export": "success" if exported else "failure" if profiles else "not-attempted",
        }

    def shard(self, name, manifest):
        self.verify_build(manifest)
        steps = ci_steps.COVERAGE_SHARDS[name]
        records = {step: dict(status="not-run", attempts=[], profile=False, export="not-attempted")
                   for step in steps}
        summary = dict(schema=SCHEMA, shard=name, compatible=False, **self.run_identity(),
                       build_attempt=manifest["build_attempt"], steps=records,
                       object_set_sha256=manifest["object_set_sha256"])
        receipt = self.out / "shard.json"
        write_json(receipt, summary)
        for step in steps:
            if self.work_deadline is not None and time.monotonic() >= self.work_deadline:
                self.failed.append(f"work cutoff before {step}")
                break
            records[step] = self.step(step)
            write_json(receipt, summary)
        self.verify_build(manifest)
        summary["compatible"] = True
        write_json(receipt, summary)

    def merge_all(self):
        paths = {step: self.out / "profdata" / f"{step}.profdata" for step in COVERAGE_STEPS}
        profiles = [path for path in paths.values() if path.is_file()]
        missing = [step for step, path in paths.items() if not path.is_file()]
        merged = self.out / "merged.json"
        merged.unlink(missing_ok=True)
        exported = bool(profiles) and self.export("merged", profiles, merged)
        if not exported:
            self.failed.append("merged (export)")
        if missing:
            self.failed.append("merged (missing profiles: " + ", ".join(missing) + ")")
        write_json(self.out / "merge.json",
                   dict(schema=SCHEMA, missing_steps=missing, complete=exported and not missing))
        print(f"[merged] {len(profiles)} step profiles", flush=True)


def parse_log(path):
    """Passed, failed and ignored totals, the failing tests' names and up to six panics."""
    text = Path(path).read_text(errors="replace")
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


def execute(args, out):
    if args.shard or args.build_only:
        if any(path.name not in {"steps", "logs", "profdata"} or not path.is_dir()
               or path.is_symlink() or next(path.iterdir(), None) is not None
               for path in out.glob("*")):
            raise ValueError(f"fresh output directory required: {out}")
    manifest = json.loads(args.build_manifest.read_text()) if args.build_manifest else None
    net = Net(out, args.deadline_epoch, args.diagnostics_reserve_seconds)
    if args.shard:
        net.shard(args.shard, manifest)
    elif args.check_build or args.merge_only:
        net.verify_build(manifest)
        print(f"[build] verified {len(manifest['objects'])} report objects", flush=True)
        if args.merge_only:
            unexpected = {p.stem for p in (out / "profdata").glob("*.profdata")
                          if p.stem not in COVERAGE_STEPS}
            if unexpected or (out / "merged.json").exists() or (out / "merge.json").exists():
                raise ValueError("merge output directory contains unexpected profiles or a prior merge")
            net.merge_all()
    else:
        for step in BUILD_STEPS if args.build_only else args.steps or ORDER:
            net.step(step)
            if step in BUILD_STEPS and step in net.failed:
                print(f"STOP: {step} failed", flush=True)
                return 1
        if args.build_only:
            write_json(out / "build.json", net.build_manifest())
        elif not args.steps:
            net.merge_all()
    if net.failed:
        print("FAILED: " + ", ".join(net.failed), flush=True)
        return 1
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--build-manifest", type=Path)
    parser.add_argument("--deadline-epoch", type=int, help="Unix deadline for completing work")
    parser.add_argument("--diagnostics-reserve-seconds", type=int, default=0,
                        help="time before the deadline reserved for profile settlement and checks")
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--list", action="store_true", help="print the steps and exit")
    modes.add_argument("--build-only", action="store_true")
    modes.add_argument("--shard", choices=ci_steps.COVERAGE_SHARDS)
    modes.add_argument("--check-build", action="store_true")
    modes.add_argument("--merge-only", action="store_true")
    parser.add_argument("steps", nargs="*", help="steps to run (default: all, then the merge)")
    args = parser.parse_args()
    if args.diagnostics_reserve_seconds < 0 or (
            args.diagnostics_reserve_seconds and args.deadline_epoch is None):
        parser.error("a diagnostics reserve must be nonnegative and requires --deadline-epoch")
    needs_manifest = args.shard is not None or args.check_build or args.merge_only
    if needs_manifest != (args.build_manifest is not None):
        parser.error("--build-manifest is only allowed and required for shard, check and merge modes")
    if args.steps and (args.build_only or args.list or needs_manifest):
        parser.error("individual step names cannot be combined with a mode")
    if args.list:
        for name, spec in STEPS.items():
            print(name, " ".join(spec["cmd"]), spec["env"] or "")
        return 0
    if args.out is None and not args.check_build:
        parser.error("--out is required")
    if ci_steps.refuse_diverged_workflow():
        return 1
    unknown = [s for s in args.steps if s not in STEPS]
    if unknown:
        parser.error("unknown steps: " + " ".join(unknown))
    try:
        if args.out is not None:
            return execute(args, args.out.resolve())
        with tempfile.TemporaryDirectory() as directory:
            return execute(args, Path(directory))
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
        print(f"FAILED: {error}", file=sys.stderr, flush=True)
        return 1


if __name__ == "__main__":
    sys.exit(main())
